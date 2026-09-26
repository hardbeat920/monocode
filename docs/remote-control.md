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

Thinking is there too. Across 25 of this machine's real interactive transcripts
the assistant content blocks were `thinking: 2128`, `text: 1249`,
`tool_use: 3991` — so reasoning is mirrored, complete, per message. `@file`
mentions resolve into `attachment` records with the file's content inline, and
`usage` on every assistant record is enough to reconstruct `turn.metrics`.

The granularity is the catch, and §8 is about that.

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
friction. It was right that it would be friction; it is simply not needed. There
is no reason to parse TUI output for something the CLI writes to disk as
structured data — and the screen is the less reliable of the two sources anyway:
the probes here emitted no OSC 8 sequence at all around the link, so an
extractor written against that observation would have found nothing.

## 7. Approvals: the part that does not mirror

Everything in §4 is available because the CLI writes the conversation to disk.
Permission requests are not part of the conversation, and they are not written
anywhere.

Measured, with a `Write` tool call in default permission mode: the
`assistant` record carrying `{type:"tool_use", name:"Write", …}` appeared, and
then **nothing was written to the transcript for 36 seconds** while the TUI sat
on

```
Do you want to create probe.txt?
❯ 1. Yes
  2. Yes, allow all edits during this session (shift+tab)
  3. No
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

**No `tool_progress`, and no streaming tool arguments.** Tool calls appear
fully formed and then their result appears. `handleToolProgress` and
`inputJsonDeltaFromEvent` have nothing to consume.

**Agent and subagent rows are only coarsely reconstructible.**
`handleAgentLifecycle` parses task started/progress/updated/notification records
out of the stream. The transcript offers `isSidechain` user/assistant records and
`agent-name` records — enough to know a subagent exists and what it said, not
enough to reproduce MonoCode's current task rows without rework.

**No `rate_limit_event`**, so `usage.limited` cannot fire, and **no stderr or
`session.error`** — the TUI renders its own errors and the tail sees nothing.

**Interrupts write no record at all.** This is the one that will bite. Measured:
ESC sent mid-turn, process left running 25 seconds, and the transcript simply
ended at the `user` record — no partial assistant text, no `turn_duration`, no
marker of any kind. A mirror that waits for `turn_duration` to clear its
pending state will spin "thinking…" forever after any interrupt taken on the
phone or in the TUI.

The implementation must therefore not treat the transcript as the only source of
turn state. The pty parser already has to watch the screen for §7; it must also
resolve turn-end, using the TUI's own idle indicators together with
`turn_duration`, and treat a return to an idle composer as ending the turn even
when no record arrived. An interrupt that MonoCode itself sent is easy — it knows
it sent ESC. An interrupt taken from the phone is the case that needs the screen.

## 9. What to build

### 9.1 Prerequisite: a command for `pty_spawn`

§5.1. Nothing else can start until a pty can run something other than the login
shell.

### 9.2 Setting

`monocode.remoteControl`, following the existing flag pattern in
`src/features/settings/model/settings.ts` (see `CLAUDE_HOOKS_KEY` /
`loadClaudeHooks` / `saveClaudeHooks`, and note those are `localStorage`-backed).

| value | behaviour |
| --- | --- |
| `manual` | **default.** Nothing automatic; each session can be opened by hand |
| `all` | Every Claude session is opened for remote control automatically, named after its project |

Surface it in `src/features/settings/ui/SettingsView.tsx` next to the existing
Claude toggles.

### 9.3 Per-session action

Available for an **existing, already-running** session, not only new ones — the
handover in §3 works on a live conversation.

- Entry point: session/tab context menu (see `useProjectMenu.tsx` and the tab
  menus added in `0d3db9c`) and/or the composer's slash menu, following
  `RESUME_COMMAND` in `src/features/sessions/model/resumeCommand.ts`.
- Claude-only, the same way `/resume` is gated: `harness === "claude"`.
  Other harnesses have no equivalent.

### 9.4 Naming

`--remote-control <name>` takes a name; use the project name so a phone list of
sessions is readable. `projectName` already exists in `src/shared/lib/paths`.
For `all` mode, name every session after its project (plus a discriminator if
one project has several threads).

### 9.5 Lifetime

Remote control stays open until explicitly closed. It is also killed when the
app quits and when the thread is archived — those are the two cases where the
pty would otherwise outlive anything that could close it. For `all` mode this
means processes accumulate with open threads, which is why `all` should open
lazily on first use rather than opening every thread at once.

### 9.6 Files involved

| Area | File |
| --- | --- |
| pty spawn (needs a command param) | `src-tauri/src/pty.rs` |
| pty write / subscribe | `src/platform/tauri/pty.ts` |
| stop/resume, binding, stream parsing | `src/integrations/harness/providers/claude/claude.ts` |
| transcript tail + record mapping | new |
| pty screen parser (approvals, turn state) | new |
| setting | `src/features/settings/model/settings.ts` |
| settings UI | `src/features/settings/ui/SettingsView.tsx` |
| session action wiring | `src/app/App.tsx` |
| menus | `src/app/shell/useProjectMenu.tsx`, `src/app/shell/TitleBar.tsx` |
| slash command (optional) | `src/features/sessions/model/resumeCommand.ts` as the pattern |

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
- **Injected text beginning with `/`, `!` or `#`.** Untested; assumed unsafe.
- **Whether an interrupted turn ever gets a terminating record.** The probe
  killed the process 25 seconds after ESC. If one arrives later, §8's screen
  watching gets simpler.
- **The Windows ConPTY path.** Every probe was macOS `pty.fork()`.
  `spawn_windows` uses `portable_pty`, and an interactive full-screen TUI under
  it — bracketed paste, resize, ESC — is untested.
