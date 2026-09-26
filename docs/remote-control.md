# Remote Control for MonoCode sessions

Plan for letting a MonoCode conversation be driven from claude.ai / a phone,
using Claude Code's own Remote Control — while MonoCode keeps showing the
replies in its own UI and keeps sending from its own composer. The earlier
version of this plan handed the conversation away and blocked the composer for
the duration. That turned out to be unnecessary: both directions were measured
and both work. The implementation is not started.

Everything below was measured on this machine, at CLI 2.1.221. Where a number
appears it is an observation, not an estimate, and it is quoted because it
constrains the design.

---

## 1. Why it does not work today

MonoCode runs Claude headless so it can drive its own UI:

```
claude --output-format stream-json --input-format stream-json
       --verbose --include-partial-messages --permission-prompt-tool stdio ...
```

Remote Control never starts in that mode. Measured by the presence of a
`bridge-session` record in the session file:

| session | version | `entrypoint` | `bridge-session` |
| --- | --- | --- | --- |
| terminal (interactive TUI) | 2.1.221 | `cli` | **present** |
| started by MonoCode | 2.1.221 | `sdk-cli` | absent |
| older MonoCode sessions | 2.1.50 | — | absent |

Same CLI version, two entrypoints, different outcome — so the deciding factor
is the **mode**, not the version.

Passing `--remote-control` explicitly does **not** help in headless mode. Tested
with MonoCode's full argument set plus a real turn: the process accepted the
flag, exited 0, wrote a 10-line session file, and produced **zero**
`bridge-session` records.

> Note for whoever continues: an earlier reading of this data blamed CLI 2.1.50
> ("remote control does not exist in 2.1.50", 0/14 vs 69). That was wrong —
> every 2.1.50 session was also a MonoCode session, so version and mode changed
> together and the wrong variable got picked. The `sdk-cli` row above is the one
> that isolates it.

## 2. What does work

Interactive mode needs a TTY. Given one, Remote Control starts and prints its
link. Run under a pty — `script -q /dev/null` is enough to get one by hand, and
`pty.fork()` is what the later probes used — the TUI emitted:

```
Continue here, on your phone, or at
https://claude.ai/code/session_01Q5d1qZzZoArEnaHyjYwm91
```

**MonoCode already has pty support** (`src-tauri/src/pty.rs`, the terminal
panes, `killPty`). So the capability needed here is not new infrastructure —
it is wiring something the app can already do.

Relevant CLI flags (from `claude --help`):

```
--remote-control [name]                        Start an interactive session with
                                               Remote Control enabled (optionally named)
--remote-control-session-name-prefix <prefix>  Prefix for auto-generated names
                                               (default: hostname)
```

## 3. The design: one process, two clients

One process owns the conversation. That part is unavoidable — but owning the
conversation is not the same as owning the *view* of it. The interactive CLI
writes every message to a file on disk as the turn progresses, and it reads
messages from its stdin, which MonoCode holds. So MonoCode stops being a
process that was replaced and becomes a second client of the one that replaced
it: it reads the conversation from the transcript and writes into the pty.

1. **Stop the headless child.** `stopClaudeSession` preserves the resume state
   (verified while fixing #445). This is not optional: `ensureLive` keeps a
   long-lived child per thread in `liveByThread`, and two processes appending to
   one session file would interleave their records.
2. **Note where the transcript ends.** Capture its byte length at this moment.
   Everything after that offset belongs to the pty-hosted process, and the
   mirror replays from there rather than re-rendering the whole history the UI
   already has.
3. **Open.** Spawn in a pty:
   `claude --resume <providerSessionId> --remote-control <name>`
4. **Surface the link.** Read it out of the transcript (§6), show link + QR.
5. **Mirror.** Tail the transcript from the offset for replies (§4); send the
   composer's text into the pty with `writePty` (§5). The composer stays live.
6. **Close.** Kill the pty; MonoCode resumes the same conversation headless on
   the next turn.

**Step 1 needs the per-thread stop epoch to be a prerequisite, not a
nice-to-have.** Stopping the headless child is only sound if a startup already
in flight can be superseded. Without the epoch, opening remote control while a
turn is starting lets the headless startup finish *after* the pty has spawned —
two processes on one conversation, precisely what this step exists to prevent,
and arrived at by a race rather than by a mistake anyone can see in the code.
With it, `stopEpoch(sessionId)` moves and the in-flight startup abandons itself
(`claude.ts:418`). That work lives on `fix/claude-poisoned-resume` and is **not
on this branch**, so it has to land first.

What makes the round trip safe is that `--resume` **appends to the same file** —
no fork, no new session id. Measured across three successive processes against
one conversation: the same `<uuid>.jsonl` grew each time and no second file
appeared. So the transcript path MonoCode computes once stays valid through
headless → pty → headless, and so does the resume binding.

## 4. Inbound: tailing the transcript

The session file lives at:

```
~/.claude/projects/<cwd with every non-alphanumeric replaced by ->/<providerSessionId>.jsonl
```

The filename is exactly the session UUID, so MonoCode already holds both halves
of the path: `sessionWorkCwd(session)` and the bound provider session id. The
directory rule is the same one-way encoding described in §10.

**It is append-only, and that is load-bearing.** Verified byte-wise rather than
assumed: the first 74973 bytes of a probe transcript hashed to
`c507d7229144d7dbda4066dc26b2aa0f`; after two further `--resume` processes and
four more turns the file had grown to 77505 bytes and those first 74973 bytes
hashed identically. Nothing is rewritten in place — even state that looks
mutable (`bridge-session.lastSequenceNum`, `mode`, `permission-mode`,
`ai-title`) is re-appended as a fresh record every turn. A tail that only ever
reads forward from an offset is therefore correct, not merely convenient.

**Records land during the turn, not at the end of it.** One measured turn, with
the send at t=0:

```
+0.8s   user       {promptSource:"typed", origin:{kind:"human"}, message:{...}}
+1.3s   ai-title
+4.3s   assistant  content:[{type:"text",...}] with stop_reason and full usage
+4.5s   system     subtype:"stop_hook_summary"
+4.5s   system     subtype:"turn_duration"   <- turn end
```

And a turn that used a tool:

```
+0.7s   user       message.content: "Use the Bash tool to run: echo hi"
+3.8s   assistant  content:[{type:"tool_use", name:"Bash", input:{command:"echo hi"}}]
+7.1s   user       content:[{type:"tool_result", ...}] + toolUseResult:{stdout,stderr,interrupted}
+8.8s   assistant  content:[{type:"text", text:"hi"}]
+8.8s   system     subtype:"turn_duration"
```

So the mirror gets user messages within about a second of the keystroke,
assistant messages as each one completes, tool calls with their full input, and
tool results with stdout/stderr. `system` / `subtype:"turn_duration"` is the
turn-end signal, carrying `durationMs` and `messageCount`.

`@file` mentions resolve into `attachment` records with the file's content
inline, and `usage` on every assistant record is enough to reconstruct
`turn.metrics`.

**Reasoning is not here, despite the `thinking` blocks.** An earlier version of
this section claimed the opposite, and the mistake is worth naming because it is
easy to repeat: it counted content-block *types* — `thinking: 2128`,
`text: 1249`, `tool_use: 3991` across 25 transcripts — and never opened a
payload. Every one of those blocks persists with an **empty** `thinking` string,
keeping only its ~980-character `signature`. Measured: **14334 of 14334** blocks
written by an interactive 2.1.2xx CLI are empty. Machine-wide only 154 of 14600
carry text, and 151 of those came from CLI 2.1.50. Reasoning therefore belongs in
§8's losses, not here. Count nothing without reading one.

The granularity is the other catch, and §8 covers both.

## 5. Outbound: writing into the TUI

The write path already exists. `pty_write` (`src-tauri/src/pty.rs`) is exposed
to the front end as `writePty` (`src/platform/tauri/pty.ts`), alongside
`resizePty`, `killPty`, `killAllPtys` and `getPtyStatus`. Nothing new is needed
in the bridge.

Injecting a user message into a full-screen rendered app sounds fragile. It is
less fragile than it sounds, and the cheap technique is bracketed paste —
`ESC[200~<text>ESC[201~` followed by `\r`. Measured:

- **It submits.** Four separate turns, four `user` records with
  `promptSource:"typed"`, each within a second of the write.
- **Multi-line stays one message.** A three-line paste landed as a single
  `user` record with the newlines intact — the `\n`s did not submit early.
- **`@file` works**, resolving to an `attachment` record as it would if typed.
- **Sending during a turn queues.** A second paste mid-turn produced
  `queue-operation:enqueue`, then `dequeue` when the turn ended, then a `user`
  record with `promptSource:"queued"`. So the composer never needs a busy state
  and never needs to hold the message itself.
- **ESC interrupts** the running turn.

The real hazard is not the text, it is the screen. **A modal silently swallows
injected text and there is no error.** The first probe lost its entire message
to the first-run "Is this a project you trust?" dialog, and the trailing `\r`
answered *that* instead of submitting anything. Nothing in the transcript, and
nothing in the write's return value, said so.

That is why the pty output parser is not an optional nicety: MonoCode must know
what is on the screen before it injects. Gate every write on parsed pty state —
composer idle, no modal — and when the state is not recognised, refuse to
inject and show the user the raw pty instead of guessing. The same rule covers
approvals (§7), which is the case it exists for.

**Reading the screen means rendering it, not stripping escapes.** Know this
before writing any of it, because it decides the shape of the whole parser. The
CLI does not write spaces between words; it paints each word at an absolute
column, so §7's question arrives as
`DoESC[5GyouESC[9GwantESC[14GtoESC[17Gcreate` — no spaces anywhere — and the
bytes never contain `Do you want` at all. Strip the escapes and the words
collapse into `Doyouwanttocreateprobe.txt?`, taking with them every column the
layout carries
— and with those, any way to tell an option line from a footer. So the parser
keeps a cell grid and replays the cursor into it: CUP, CHA, CUU/CUD/CUF/CUB, EL,
ED, CR, LF, DECSC/DECRC, with OSC and charset sequences skipped and SGR ignored.
That is a quarter of a terminal emulator, and it is the price of asking the
screen anything at all.

A grid needs a size, which is why the pty is spawned at a fixed 120×40
(`remoteControlArgs` in
`src/integrations/harness/providers/claude/remoteControl.ts`) instead of at
whatever the window happens to be — the parser should not be at the mercy of
geometry. The width has a second consequence: the CLI hard-wraps box text into
painted lines of its own, so a question carrying a path is easily wider than 120
columns and arrives in two pieces that have to be joined back before it can be
recognised. Whichever width is chosen, the parser and the spawn have to agree
about it.

**The `screens*.txt` captures are not a fixture source.** They were flattened
the lossy way, and `screens4.txt` contains `Doyouwanttocreateprobe.txt?` — a
test built on them would bake in the very mistake the parser exists to avoid.
Only the raw `pty_raw*.bin` captures preserve what the terminal saw; render
those.

Two further limits: text beginning with `/`, `!` or `#` drives the TUI's own
slash-command, bash and memory affordances rather than being sent as a message
(untested — treat as unsafe to inject blind), and there is no injection path for
images or pasted attachments at all.

### 5.1 `pty_spawn` cannot run this command

`pty_spawn` takes `(id, cwd, cols, rows)` and always runs `default_shell()`
with login args. It has no command parameter, so it cannot start
`claude --resume … --remote-control …` as it stands. Either add optional
`command`/`args`, or write `exec claude …\n` into the shell it does start. The
first is the right shape; the second inherits rc-file noise and puts the command
in shell history. Either way this is a prerequisite, not a detail — see §9.

## 6. The remote-control link

The link does not need to be scraped out of the TUI. It is written to the
transcript, clean, twice:

```json
{"type":"bridge-session","sessionId":"f58085bb-…","bridgeSessionId":"cse_01V1ZQG948YQbhEq2nusnbMc","lastSequenceNum":0}
{"type":"system","subtype":"bridge_status",
 "content":"/remote-control is active · Continue here, on your phone, or at https://claude.ai/code/session_01V1ZQG948YQbhEq2nusnbMc",
 "url":"https://claude.ai/code/session_01V1ZQG948YQbhEq2nusnbMc"}
```

Read the `url` field of the `bridge_status` record. `bridgeSessionId` is
`cse_<suffix>` and the URL is `session_<same suffix>`, so either one identifies
the bridge.

One `bridge_status` record is written per process start, which means the same
record answers a second question for free: whether Remote Control is currently
active for this conversation. The mirror does not need to track that separately.

**The link survives a resume.** One conversation was resumed three times and all
three records carried the same `cse_01P9pJ…`; separately-started sessions each
got their own. So the bridge identity belongs to the conversation, not to the
process — a handover does not invalidate the URL, and a UI that re-renders the
link and regenerates the QR on every close and reopen is doing work for nothing.
Worth stating because the opposite is the natural assumption: a new process
looks like it should mean a new session.

**There is no `/rc` suffix.** An earlier draft read one off the screen and
called it "the remote-control URL". It is not part of the link — the raw bytes
show the URL ending at the session id, followed by `\r`, a 114-column and
28-row cursor move, and only then a green `/rc` painted in the status line. It is
the TUI's own slash-command indicator (the same `/rc` that appears in
`/rc connecting…` while the bridge starts), and it looked like a suffix only
because flattening the screen put the two next to each other. Use the `url`
field and append nothing to it.

An earlier draft of this plan specified stripping ANSI and matching an OSC 8
hyperlink payload out of the rendered output, and warned that it would be
friction. For the link it is simply not needed: there is no reason to parse TUI
output for something the CLI writes to disk as structured data, and the screen is
the less reliable of the two sources anyway — the probes here emitted no OSC 8
sequence at all around the link, so an extractor written against that observation
would have found nothing.

"Friction" is too kind for where the screen genuinely is the only source, though.
For §7 stripping escapes is not friction, it is impossible, and §5 says why: the
words are not in the bytes, only their columns are.

## 7. Approvals: the part that does not mirror

Everything in §4 is available because the CLI writes the conversation to disk.
Permission requests are not part of the conversation, and they are not written
anywhere.

Measured, with a `Write` tool call in default permission mode: the
`assistant` record carrying `{type:"tool_use", name:"Write", …}` appeared, and
then **nothing was written to the transcript for 36 seconds** while the TUI sat
on

```
──────────────────────────────────────────────────────────
 Create file
 probe.txt
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
  1 hello
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 Do you want to create probe.txt?
 ❯ 1. Yes
   2. Yes, allow all edits during this session (shift+tab)
   3. No

 Esc to cancel · Tab to amend
```

The `tool_result` record appeared only after a `\r` was injected. Nothing on
disk said a prompt was pending, which tool was asking, or what its input was.

This is the whole of MonoCode's `handleControlRequest` path
(`claude.ts`) disappearing: `approval.requested` / `approval.resolved`,
`question.asked` for `AskUserQuestion`, and the `type:"plan"` capture for
`ExitPlanMode` all arrive over `--permission-prompt-tool stdio`, which exists
only in headless mode. Note also that `ExitPlanMode` *diverges* rather than
merely vanishing: MonoCode denies it and captures the proposed plan itself,
while the interactive CLI handles plan approval natively.

An earlier version of this section quoted only the question and the three
options. The box carries more than that, and both of the parts it left out are
the parts an approval UI needs. Above the question, the tool's own header and
body — `Create file`, `probe.txt`, and the diff it would write — are the only
statement anywhere of *what the prompt would do*; the question itself names the
file but not the content, and the transcript names neither, so dropping them
leaves a user approving a write they cannot see. Below the options, the footer is
the only thing that advertises how to get out without answering: `Esc to cancel`
dismisses the prompt, `Tab to amend` opens it for editing. A prompt parsed
without its footer is one the user can only say yes or no to, which is exactly
the kind of narrowing that turns a safe parser into a coercive one.

**Answering one: the digit acts, and Esc denies.** Measured over five runs, with
ground truth taken from the `tool_result` record *and* from whether the file
appeared on disk, because the record alone would not have caught a no-op. An
option's own digit selects and acts in a single keystroke — there is no highlight
to move first. `1` returned a success result and `probe.txt` appeared with its
contents; `3` returned `is_error: true` with `"User rejected tool use"` and no
file. A digit no option carries does nothing and leaves the prompt pending, which
is the benign way for this to go wrong.

Two corrections to the obvious reading, both of which would have cost something.
The `\r` above is not the actuator — after a digit it is 80 bytes of mouse-mode
and cursor housekeeping the TUI answers with; it mattered in the first probe only
because, sent alone, it takes whichever option the cursor already sits on. And
**Esc denies**: it produces the identical result to option 3, so a UI that offers
`Esc to cancel` as a way out of deciding is quietly rejecting the tool call. The
footer is the TUI's wording, not a description of the effect.

So there is no approval bias to design around — every option including denial is
one keystroke away, and the never-auto-approve rule below is implementable as
written. One consequence is live and easy to get wrong: because Esc and option 3
are byte-identical in the transcript, nothing reading the mirror can tell a cancel
from an explicit No, while MonoCode's `approval.resolved` distinguishes `deny`
from `cancelled`. Only the client that sent the keystroke knows which happened, so
it has to remember rather than re-derive it.

**So the pty output parser is load-bearing.** It is the only source of approval
state, and the feature cannot be built without it.

The decided behaviour: MonoCode owns the pty's stdin, so it can do more than
notice a pending prompt — it can answer one. Parse the prompt off the screen,
render it in MonoCode's own approval UI, and inject the chosen option. When the
screen cannot be parsed with confidence, **do not guess**: surface the raw pty
and let the user answer it directly, in the TUI, as themselves.

Two rules follow from that and are not negotiable:

- **Never auto-approve.** Running these sessions under `bypassPermissions` or
  `acceptEdits` would make the problem disappear by removing the user's
  decision, which is the wrong trade. Rejected.
- **The phone is an option, not an obligation.** The bridge can presumably
  answer prompts too, but the user must never be forced to pick up a phone to
  unblock a session they are looking at.

Failing safe here means failing *visible*: an unparseable screen must become a
raw terminal in front of the user, never a silent wait and never an assumed
"yes".

## 8. What the mirror loses

Honest accounting. None of these block the design, but an implementer who
expects the headless stream's fidelity will be surprised by all of them.

**Token-level streaming is gone.** `message.delta` and `reasoning.delta` come
from `stream_event` records via `handleStreamEvent`; the transcript writes whole
assistant messages when they complete. Measured silence between the user record
and the assistant record: 3.5s for a one-word reply, and proportionally longer
for a real thinking turn. `--include-partial-messages` has no interactive
equivalent, so partial messages are gone with it. The UI needs a pending-message
affordance that is honest about not knowing how far along the turn is.

**Reasoning is gone, not merely unstreamed.** The `thinking` blocks are written
but redacted — empty text, signature only, on all 14334 of them (§4). So there
is nothing to show at any granularity, and a mirror must not present a thinking
affordance it cannot fill. The transcript module emits `reasoning.delta` only
when a block actually carries text, which costs nothing today and means a CLI
that starts persisting reasoning is mirrored without a change.

**No `tool_progress`, and no streaming tool arguments.** Tool calls appear
fully formed and then their result appears. `handleToolProgress` and
`inputJsonDeltaFromEvent` have nothing to consume.

**Subagent traffic is absent, not coarse.** An earlier version of this section
said the transcript offers `isSidechain` records, "enough to know a subagent
exists and what it said". It does not. `isSidechain` is present on every user and
assistant record and was **`false` on all 23628** of them, and
`parent_tool_use_id` appears **nowhere** in any transcript on this machine —
subagent messages evidently go to their own files. So the mirrored session file
carries nothing to reconstruct, and `handleAgentLifecycle`'s task rows have no
transcript equivalent at all.

This one reaches into the implementation, which is the useful half: reusing
`isSubagentMessage` from `claudeProtocol` looks right and is not. It keys off
`parent_tool_use_id`, which the **stream** carries and a transcript record never
does, so it would compile, read correctly, and never once fire. The rule
generalises — the stream and the transcript describe the same conversation with
different routing fields, so borrow the helpers that read *content* and write
your own for anything that decides *where a record belongs*.

**No `rate_limit_event`**, so `usage.limited` cannot fire, and **no stderr or
`session.error`** — the TUI renders its own errors and the tail sees nothing.

**Interrupts write no record at all.** This is the one that will bite. Measured:
ESC sent mid-turn, process left running 25 seconds, and the transcript simply
ended at the `user` record — no partial assistant text, no `turn_duration`, no
marker of any kind. A mirror that waits for `turn_duration` to clear its
pending state will spin "thinking…" forever after any interrupt taken on the
phone or in the TUI.

**And it is never written later.** The 25-second reading left open whether a
terminating record arrives eventually or on the next action; it does neither. ESC
into a thinking turn, then 240 seconds with the process alive: the session file
stayed byte-identical. A follow-up message at 241s got its own `user`,
`assistant` and `turn_duration` records, and **nothing was back-filled** for the
interrupted turn — `interruptedMessageId` was `null` on both user records. So the
screen is not a stopgap until a record shows up. There is no record.

The implementation must therefore not treat the transcript as the only source of
turn state. The pty parser already has to watch the screen for §7; it must also
resolve turn-end. An interrupt that MonoCode itself sent is easy — it knows it
sent ESC. An interrupt taken from the phone is the case that needs the screen.

"The TUI's own idle indicators" was how an earlier version of this section put
it, which hides that the screen carries two different signals and only one of
them is about being idle:

- A **running** turn paints a spinner line: a glyph, one gerund ending in an
  ellipsis, sometimes an elapsed time and a token count — `✢ Synthesizing…`,
  `✻ Ruminating… (3s · ↓143 tokens)`.
- A **finished** turn leaves a past-tense summary where the spinner was:
  `✻ Baked for 2s`, `✻ Sautéed for 6s`, `✻ Churned for 6s`. **Match the
  `for <duration>` shape and never the word.** The vocabulary rotates from turn
  to turn — eight finished-turn words and twenty-odd gerunds counted across the
  captures — so it is decoration, and a matcher keyed on it fails on the next
  turn, never mind the next CLI version.

**Both durations change format at 60 seconds, and not in the same way.** The
minutes form lives in one capture only — `interrupt_raw.bin`, the 252-second turn
— which is why a check across the six older `pty_raw*.bin` files finds none of it
and is right to. Two independent attestations, neither needing anyone's harness:
that turn's own record says `turn_duration: 252410`, and
`grep -c "Brewed for 4m 12s" interrupt_raw.bin` returns 1. This cost a real bug: a finished turn past a minute reads `✻ Brewed for 4m 12s`, so a
matcher understanding only `for <N>s` recognises short turns and silently stops
recognising long ones — which are exactly the turns someone walks away from. Match
`for (<N>h )?(<N>m )?<N>s`. The running counter is the other half of the trap: its
elapsed time also switches to `1m 25s`, but the nested figure inside it stays in
raw seconds past 60 — `✻ Osmosing… (1m 25s · thought for 83s)` — so a finished-turn
matcher without an end anchor swallows a *running* turn's own counter and calls it
finished. Anchor it to the end of the line, and test the spinner first.

**The absence of a spinner is not a third signal, and an earlier version of this
section said it was.** "Treat a return to an idle composer as ending the turn"
was the instruction here, and it is false: while the assistant streams its reply
the spinner is not on the visible grid at all — 325 of 348 frames of one turn had
none anywhere in 45 rows — so the line above the composer is prose and the screen
is indistinguishable from an idle one. Measured on a 252-second turn replayed in
4KB frames the way a consumer of pty data sees it: 11 frames correctly
in-progress, then **337 consecutive frames reported as ended**, about 166 seconds
of a turn that was still running. Anything reading that would close the turn and
emit a completion mid-sentence, and would permit an injection into a live turn.

So `ended` requires a positive marker — the summary, freshest above the composer —
and everything else is `unknown`. Accepting the summary from *anywhere* on screen
is the tempting weaker rule and is the same mistake: it reads a stale marker as
evidence about the current turn. It happens to work on the long capture only
because a 252-second essay scrolls the previous summary away.

**Which leaves the interrupt case genuinely unresolved from the screen alone, and
it is worth being exact about why.** The trace an interrupt leaves is a composer
*holding text* — the CLI restores the interrupted prompt into it, so "empty
composer" is the wrong condition and a guard written on it never fires. But a
composer holding text with no spinner above it is also what a user typing ahead
mid-turn looks like, which the TUI allows and queues. One frame cannot separate
those. Resolving it needs the transcript: `turn_duration` where there is one, and
otherwise whether a user record opened a turn that nothing has closed. Until that
rule lands, an interrupt taken on the phone shows as running — which is the safe
direction, and strictly better than closing live turns 337 frames early.

One string not to write code against: `esc to interrupt` appears nowhere in any
capture, raw, ANSI-stripped or whitespace-squashed. A matcher looking for it finds
nothing on every screen, including the ones where a turn plainly is running; what
a running turn shows is the gerund and its counter above.

An earlier version of this paragraph overstated that as "the substring
`interrupt` occurs zero times", which was true of the six captures it was checked
against and is not true of the corpus as it grew. The word does occur, once, in a
line that has nothing to do with turn state: `⎿ Tip: Use /btw to ask a quick side
question without interrupting Claude's current work`. Which is its own small
lesson — the exact string is the claim worth making, and a substring search is a
different, weaker one.

## 9. What to build

### 9.1 Prerequisite: a command for `pty_spawn`

§5.1. Nothing else can start until a pty can run something other than the login
shell.

### 9.2 Also missing: the transcript path

Nothing computes `~/.claude/projects/<encoded cwd>/<id>.jsonl` yet. The handover
deliberately takes a `HandoverTarget` rather than a path, so the seam is open on
purpose and has to be filled before §4 can read anything: encode
`sessionWorkCwd(session)` by the rule in §10, and name the file for the bound
provider session id. The encoding is one-way, so it has to be derived from the
cwd every time rather than stored and trusted.

### 9.3 Setting

`monocode.remoteControl`, following the existing flag pattern in
`src/features/settings/model/settings.ts` (see `CLAUDE_HOOKS_KEY` /
`loadClaudeHooks` / `saveClaudeHooks`, and note those are `localStorage`-backed).

| value | behaviour |
| --- | --- |
| `manual` | **default.** Nothing automatic; each session can be opened by hand |
| `all` | Every Claude session is opened for remote control automatically, named after its project |

Surface it in `src/features/settings/ui/SettingsView.tsx` next to the existing
Claude toggles.

### 9.4 Per-session action

Available for an **existing, already-running** session, not only new ones — the
handover in §3 works on a live conversation.

- Entry point: session/tab context menu, following the conventions of the tab
  menus added in `0d3db9c` (see `useProjectMenu.tsx`).
- Claude-only, gated on `harness === "claude"` the way every other Claude-only
  affordance is — `App.tsx:1452`, `App.tsx:7368`, `UsageFooter.tsx:111`. Other
  harnesses have no equivalent.

> An earlier version of this plan told you to follow `RESUME_COMMAND` in
> `src/features/sessions/model/resumeCommand.ts`. **Neither exists** — no such
> file, and the identifier appears nowhere in `src/`. The reference came from the
> first draft and was wrong the whole time, which is what a plausible-looking
> path costs when nobody opens it. The sites above are real and are what the
> action module ended up following.

### 9.5 Naming

`--remote-control <name>` takes a name; use the project name so a phone list of
sessions is readable. `projectName` already exists in `src/shared/lib/paths`.
For `all` mode, name every session after its project (plus a discriminator if
one project has several threads).

### 9.6 Lifetime

Remote control stays open until explicitly closed. It is also killed when the
app quits and when the thread is archived — those are the two cases where the
pty would otherwise outlive anything that could close it. For `all` mode this
means processes accumulate with open threads, which is why `all` should open
lazily on first use rather than opening every thread at once.

### 9.7 Files involved

| Area | File |
| --- | --- |
| pty spawn (needs a command param) | `src-tauri/src/pty.rs` |
| pty write / subscribe | `src/platform/tauri/pty.ts` |
| stop/resume, binding, stream parsing | `src/integrations/harness/providers/claude/claude.ts` |
| transcript tail + record mapping | `src/features/remoteControl/model/transcript.ts` |
| transcript path helper | still missing — §9.2 |
| pty screen parser (approvals, turn state) | new |
| setting | `src/features/settings/model/settings.ts` |
| settings UI | `src/features/settings/ui/SettingsView.tsx` |
| session action wiring | `src/app/App.tsx` |
| menus | `src/app/shell/useProjectMenu.tsx`, `src/app/shell/TitleBar.tsx` |
| Claude-only gating, as the pattern | `src/app/App.tsx`, `src/app/shell/UsageFooter.tsx` |

## 10. Constraints learned the hard way

Each of these cost real debugging time; none is obvious from the code.

- **The CLI must be the user's own.** `resolve_claude` tried fixed paths before
  the login shell, so MonoCode ran `~/.local/bin/claude` (2.1.50, ten months
  old) while the terminal ran 2.1.221. Fixed in #448 — remote control needs a
  recent CLI, so that fix is a prerequisite.
- **A live child ignores a new resume binding.** `ensureLive` returns the
  existing child before the resume state is read, so the child must be stopped
  *before* binding or the next turn continues the wrong conversation (#445).
- **Bind to the working copy.** For a worktree session, `cwd` is the project and
  the checkout is `worktreeCwd`; Claude resumes only when the bound directory
  matches the one the next turn runs in. Use `sessionWorkCwd(session)` (#445).
- **Session directory encoding.** Claude maps the absolute cwd to a directory by
  replacing every non-ASCII-alphanumeric character with `-` — spaces,
  punctuation and drive colons included. One-way only (#445). Confirmed by
  prediction while investigating the mirror: the encoded name for a probe cwd
  was computed from this rule and the directory was there. The session file
  inside it is named for the session UUID, nothing else.
- **The transcript is append-only.** Verified by hashing a prefix across three
  processes and four turns (§4). Tail forward from an offset; never re-read the
  file to diff it.
- **Records land during the turn, about a second behind the keystroke**, and
  each assistant message appears whole when it completes (§4). Turn end is
  `system` / `subtype:"turn_duration"` — except when the turn was interrupted,
  when nothing is written at all (§8).
- **A modal swallows injected text silently.** No error, no record, no signal in
  the write's result. Gate injection on parsed pty state (§5).
- **A turn holds open while tasks are listed.** `maybeFinishTurn` refuses to end
  a turn while `agentTasks`/`backgroundTasks` are non-empty, and the grace timer
  is only armed when a task is *removed*. Anything that registers a task must
  remove it (#446).
- **stderr is the only place a dying child explains itself.** Unclaimed stderr
  now reaches the console; keep that in mind when the pty child misbehaves.

## 11. Tests worth writing

- Setting defaults to `manual`; `all` is opt-in.
- The action is offered for Claude and hidden for other harnesses (mirror the
  `/resume` visibility tests in `Composer.test.ts`).
- Link extraction from a `bridge_status` record: the `url` field is used, a
  session file with no such record produces no link, and a second record from a
  later process start does not produce a duplicate.
- Transcript tail: a fixture session file mapped to UI messages — user,
  assistant text, thinking, `tool_use` paired with its `tool_result`,
  `attachment`; records before the captured offset ignored; a partially written
  final line tolerated and re-read once complete.
- `turn_duration` ends the turn; a tail that stops after a `user` record with no
  `turn_duration` does **not** leave the turn pending forever (the interrupt
  case from §8).
- Injection gating: a recognised idle composer accepts a write, a recognised
  modal refuses it and surfaces the pty, an unrecognised screen refuses it too.
- Approval parsing: a permission prompt is parsed into a title, tool and
  options; the chosen option is injected; an unparseable prompt falls back to
  the raw pty and never answers on the user's behalf.
- Handover ordering: the headless child is stopped before the pty starts, the
  transcript offset is captured at that point, and the conversation binding
  survives the round trip.
- `all` mode names each session after its project.

## 12. Still unknown

Each of these is unproven. They are listed in the order they would hurt.

- **A phone-originated turn has never been observed.** No phone was available,
  and there is no historical turn to inspect either: `lastSequenceNum` is `0` in
  all 3379 `bridge-session` records across 82 transcripts on this machine, and
  `promptSource` only ever takes the values `typed`, `queued` and `system`. So
  nobody has actually driven a session from a phone here. **This is the
  assumption the whole feature rests on** — confirm it first, before building
  anything. The reason to expect it to hold: the bridge is served by the same
  process that owns and appends the transcript, and the transcript is the state
  `--resume` reads back, so an accepted turn has nowhere else to live. When
  confirming it, check what `promptSource` and `origin.kind` a phone turn
  carries, and make the tail tolerate values it does not recognise rather than
  filtering for `"typed"`.
- **Whether the bridge relays permission prompts to the phone.** Likely — it
  would be strange if it did not — but untested. It matters because it decides
  whether the phone is a real fallback when §7's parser gives up.
- **Which keystroke takes which option** was open here, and is now measured —
  see §7. It moved out of this section because the answer inverted the guess:
  the digit acts, and Esc denies rather than dismissing.
- **Every permission prompt except one.** The corpus contains a single prompt:
  `Write` → `Create file`, three options, in default permission mode. No
  `AskUserQuestion`, no `ExitPlanMode` plan approval, no Bash-command approval, no
  "don't ask again" variant, and no option count other than three. Nor is there a
  capture of a repaint caught mid-prompt. The parser refuses everything it does
  not recognise, so each unobserved shape costs a raw-pty fallback rather than a
  wrong answer — but each is also a prompt MonoCode cannot yet answer, and they
  will be common. One probe per shape closes this.
- **Whether a wrapped question really wraps the way we assume.** Of the parser's
  fixtures, one is reconstructed and the rest are verbatim: no capture contains a
  permission question long enough to wrap, so the wrapped-question fixture is the
  real question broken across two lines by hand. The wrapping *behaviour* it
  imitates is captured — the trust dialog's safety paragraph is painted as two
  lines — but the wrapped question is not, and a reader should know which fixture
  is which.
- **Injected text beginning with `/`, `!` or `#`.** Untested; assumed unsafe.
- **Whether an interrupted turn ever gets a terminating record.** The probe
  killed the process 25 seconds after ESC. If one arrives later, §8's screen
  watching gets simpler.
- **The Windows ConPTY path.** Every probe was macOS `pty.fork()`.
  `spawn_windows` uses `portable_pty`, and an interactive full-screen TUI under
  it — bracketed paste, resize, ESC — is untested.
