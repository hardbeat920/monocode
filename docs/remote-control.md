# Remote Control

Hand a MonoCode conversation to an interactive Claude CLI, so the same thread can
be carried on from a phone — and keep MonoCode showing it, and able to answer it,
the whole time.

Section numbers here are cited from the code (`§5`, `§8`, `§9.6`, …). If you
renumber, fix the citations.

---

## 1. Why this exists

MonoCode runs Claude headlessly: it spawns the CLI in a child process, speaks
JSON-RPC to it, and renders the result. That child belongs to the desktop app. If
you walk away from the machine, the conversation stops where you left it.

Claude Code has its own answer to this — `claude --remote-control <name>` starts
an interactive session that registers with claude.ai, so it can be driven from a
phone. But it is an *interactive* session: a TUI on a terminal, not a JSON-RPC
peer. The two cannot both own a conversation:

- the CLI refuses a second process on the same session, and
- two processes appending to one transcript would interleave their records.

So this is a **hand-over**, not a second connection. MonoCode stops its own child,
hands the conversation to a CLI it hosts in a pty, and then follows along by
reading the same two things a human at that terminal would: the transcript file on
disk, and the screen.

Everything awkward about this feature follows from that one constraint. Nothing
here is an API. The transcript is a log the CLI happens to write, the screen is
text painted for a person, and both may change shape between CLI versions. The
rule the code holds itself to is therefore: **recognise something, or say you did
not.** Nothing is inferred from a shape we have not seen in a real session.

---

## 2. The hand-over

`openRemoteControl` in `src/integrations/harness/providers/claude/remoteControl.ts`.
The ordering is the whole of it, and every impure step is a port so the ordering
can be asserted without a CLI or a pty:

1. **Refuse early** if the thread has no `providerSessionId`. Without one there is
   nothing for `--resume` to open, and going ahead would stop a working child to
   gain nothing.
2. **Stop the headless child** (`stopSession`, which keeps the resume binding —
   `forgetClaudeSession` would delete the very conversation being handed over).
3. **Measure the transcript's length** while neither process is running. That
   offset is the exact boundary between what MonoCode has already rendered and
   what the pty-hosted CLI will append. Measured in this gap, no record can land
   between the measurement and the spawn and be missed by both sides.
4. **Spawn the pty** running `claude --resume <id> --remote-control <name>`.

On any failure the thread is left with no process at all, which is the state every
thread sits in between turns: the resume binding is untouched, so the next turn
starts a headless child on the same conversation.

The pty id is **derived** from the thread — `remote-control:<sessionId>` — and not
stored. That is what lets a close work from a window that never opened it: after a
reload nothing holds the handle, but the pty is still addressable.

---

## 3. Where the conversation is written

`~/.claude/projects/<encoded-cwd>/<providerSessionId>.jsonl`, where the encoding
maps every character that is not an ASCII letter or digit to `-`.

That rule exists twice: `claude_project_dir` in `src-tauri/src/fs.rs` and
`encodeProjectDir` in `src/features/remoteControl/model/transcriptPath.ts`. The
Rust one is a private `fn` rather than a command, so it cannot be called from the
front end. **Change one and you must change the other** — the failure is silent,
because a wrong directory simply contains no file and the mirror shows nothing.
The TypeScript test copies its cases from the Rust tests so a divergence is a red
test instead of an empty session.

One subtlety worth keeping: Rust maps over code points, so the TypeScript must
too. A regex over the string would work on UTF-16 units and spend two dashes on an
astral character where Rust spends one.

---

## 4. Following it

`transcriptWatch.ts` tails the file from the offset taken in §2, using
`read_file_range` (added to `src-tauri/src/fs.rs` for this) so a long conversation
is not re-read on every poll. `transcript.ts` turns the records into the same
events MonoCode's own harness emits, so the thread renders identically whoever is
driving it.

What the mirror deliberately does **not** do:

- It does not filter user records by `promptSource` or `origin.kind`. A
  phone-originated turn has never been observed and its values are unknown, so an
  unrecognised one must still appear (§12).
- It does not require text. One block of any kind is enough: requiring text used
  to drop 55 messages across 161 transcripts on one machine — images, mostly.
- It does not treat `MirrorState.tools` as an answer to "is a call still pending".
  Entries are only ever added, never deleted, so "still in the map" is true
  forever.

---

## 5. Sending into the TUI

There is no API. A message is **typed**, and the sequence is measured rather than
assumed (`injectRemoteText`, `planInjection`).

- Bracketed paste and a return: `\x1b[200~<text>\x1b[201~\r`.
- **Clear first, then check.** After an ESC the CLI restores the interrupted
  prompt into its composer, and bracketed paste *appends*. Measured: injecting
  `say OK` after an interrupt produced one user message reading
  `…Think carefully first.say OK`. So a held composer is cleared with Ctrl-U and
  the screen is re-read; a composer that will not come back empty ends in a
  refusal. Not knowing what is in the composer stops a send. It never permits one.
- **Refusals**, each in words the user can act on: nothing to send, a permission
  prompt is waiting, a modal is waiting, the screen is unrecognised, or the text
  starts with `/`, `!` or `#` — those drive the TUI's own menus instead of being
  sent.
- A modal swallows injected text **silently**, which is why "no modal" is part of
  the gate rather than something to find out afterwards.

A send is also reported as queued or not, and that is keyed on the *turn*, not the
screen kind: a streaming screen is `idle` with an unknown turn, so conflating the
two reported every mid-stream send as "sent now" when the TUI had queued it.
`"unknown"` is a third answer and must not be rendered as either of the others.

---

## 6. The link, and the phone

The CLI writes a `bridge_status` record carrying `{active, url}`. The URL is used
**verbatim** — nothing is appended. The `/rc` that appears to be a suffix on
screen is the TUI's status-line indicator, painted 114 columns and 28 rows away
from the link. `content` is a fallback only, because this is CLI output and its
shape is not an API.

`RemoteControlLink` renders it. The URL belongs to the conversation rather than to
the process, so it survives a close and a reopen — whatever renders a QR from it
must not regenerate the code each time.

---

## 7. Permission prompts

Permission prompts are **not written to the transcript**. One sat on screen for 36
seconds and the file recorded nothing at all. So the rendered screen is the only
source, and `promptScreen.ts` is the only thing that answers "is a prompt
pending".

Answering is a keystroke, and the keystrokes were measured over five runs against
both the transcript and the filesystem:

- A bare digit both selects and acts. No cursor-move step, no `\r`.
- A trailing CR is 80 bytes of mouse-mode housekeeping and no second action.
- An out-of-range digit is a clean no-op that leaves the prompt pending.
- Esc **denies** the tool call; it does not dismiss the prompt. It produces a
  `tool_result` byte-identical to option 3's — `is_error: true`, `"User rejected
  tool use"`. The field is therefore called `deny`, not `cancel`, however the
  screen words it.

Because Esc and an explicit No are indistinguishable in the transcript, the client
that sent the keystroke is the only thing that knows which happened, so the answer
is captured at the moment of injection.

A prompt is only raised once **two consecutive frames agree** on its options. A
half-painted box gives `1..2 of 3`, which is contiguous, singly-selected, and
parses as a finished two-option prompt — on the captured prompt that is an
approval offering *Yes* or *Yes*, with the way out not yet painted. A settled
prompt costs one extra chunk; a half-painted one is never shown.

Keystrokes exist only as fields of options parsed off a recognised prompt, so
there is no way to derive an answer from a screen the parser did not understand —
including the trust modal, whose options are deliberately returned without
keystrokes.

---

## 8. The turn, and the interrupt that leaves no record

A user record opens a turn; `system` / `subtype:"turn_duration"` closes one.

**An interrupt closes neither.** ESC leaves no record at all, so a mirror waiting
for `turn_duration` would show a turn running forever. The mirror therefore never
claims a turn is still running on the strength of a missing record: it reports
what it saw, and exposes `resolveTurnFromScreen` so the caller — which watches the
pty — can end a turn the file never ended. The evidence for an interrupt is an
absence: measured pty silence, which an event handler can never observe, so a
clock drives it.

Reading the screen for turn state needs two markers, and both are matched by
shape rather than by tail:

- A spinner is a glyph then one *word* ending in an ellipsis. A looser pattern
  reads the welcome box's abbreviated path (`│ /…/scratchpad/keytest │`) as a
  spinner, and a false spinner is a false live turn.
- A finished turn is `✻ Baked for 2s`, and past a minute `✻ Brewed for 4m 12s`.
  An assistant sentence ending "…waited for 3s" is not one. The pattern is
  anchored, because a running turn's counter carries a nested duration in raw
  seconds — `✻ Osmosing… (1m 25s · thought for 83s)` — which an unanchored match
  would read as finished, the dangerous direction.

A shape outside both reads as `unknown`, which is the safe way to be wrong.

---

## 9. Process lifetime

### 9.1 One slot per id

Every way to reach a pty goes through its id. `pty_spawn` removes and terminates
whatever holds an id before it takes it, and `PtyHost::replace` holds the same
invariant at the map level so no future caller can bypass it by dropping a return
value.

### 9.2 Closing

`closeRemote` tears down this window's entry *if it has one* and kills the pty
**unconditionally**. The kill is not conditional on the entry, because the id is
derived precisely so that a close works from a window that never opened it (§2).

### 9.3 A pty that exits on its own

`subscribePty`'s `onExit` is wired: the entry is torn down and the session leaves
`remoteControlIds`. Discarding that exit left the session listed as handed over
forever — every menu offered *Close* for a process that was gone, and because
`shouldAutoOpen` reads the same set, `all` mode could never retake it.

An exit also counts as **dismissed**, for two reasons that agree. A clean exit is
the user quitting the CLI, and retaking it would be arguing with them. An unclean
one is worse: retaking spawns again, and the notice changes `sessions`, which
re-runs the effect, which spawns again.

### 9.4 A hand-over whose tab is gone

Closing a tab does **not** end the hand-over — continuing on the phone after
putting the app away is the point of the feature. But the sidebar entry is built
from the workspace's sessions, and a closed tab leaves only a history summary, so
the entry used to vanish while the CLI kept running. Unreachable and alive is the
worst state available, so a thread this window holds a hand-over for is always
offered *Close*, loaded or not.

### 9.5 A thread on its way out

Archiving stops the child, saves, and archives the record *before* taking the
session out of the workspace. In that window the thread is absent from the sidebar
and still present in the workspace, idle and bound — which is exactly
`shouldAutoOpen`'s yes. `removing` closes that door explicitly rather than relying
on the by-hand close's dismissal to do it as a side effect.

### 9.6 Processes must not accumulate

The cost to avoid is one CLI per open tab. Lazy opening (§10) means processes
accumulate against *used* threads rather than open tabs, because a thread nobody
has used never becomes eligible.

The app quitting kills them: `reapWindowRuntime` covers terminals by id, and
`reapRemoteControl` covers these. On a clean window close the unload kills are the
only cleanup there is, so they stay.

What none of that covers is the app **dying** — a crash, a force-quit, or a dev
rebuild. A new process starts with an empty `PtyHost`, so the previous run's
children are unreachable by id and reparented away. Fifty of them accumulated in
one afternoon this way, all still listed on the phone, and two thirds of them
survived a direct SIGTERM because the interactive CLI traps it.

The fix is the mechanism that already existed for harness children:
`reap_orphaned_harness_processes` finds a stray by reading
`MONOCODE_HARNESS_PARENT` out of the process and checking that the run which
wrote it is gone. The pty spawns now carry that marker, and the reaper's argv gate
decides the rest — a login shell is marked and never matched, an agent CLI is
marked and reapable. A child of the *current* run always names a living parent, so
it is never a candidate, which makes this safe for several windows and free of any
ordering race with a hand-over happening at the same moment.

A pty child must not go through `isolate_child`: `process_group(0)` makes it a
group leader and the pty's `setsid()` then fails with EPERM.

**Known residual.** After a reload, a pty that no session retakes lingers in the
live map — the reaper will not touch it, because its marker names this living
process, and no UI reaches it because a fresh page's `remoteControlIds` is empty.
It is bounded: one generation at most, cleaned by `kill_all` at exit.

---

## 10. Modes

`manual` (default) and `all`, in Settings.

`all` hands a session over **lazily**, and the moment is not a preference but the
only one that satisfies §2's constraints: the session must be idle, with a
conversation bound. Before a first turn there is no `providerSessionId`; during a
turn the hand-over would stop the child the user is waiting on. So a brand-new
thread goes active by itself once its first turn ends, and the menu says so rather
than saying "send a message first", which reads as a chore the user must do.

Two rules keep the mode from arguing with the user:

- A hand-over closed by hand is **dismissed** and `all` will not retake it.
- Switching *into* `all` clears those dismissals, because choosing "every session"
  is a later instruction than any close that came before it.

A click during a turn is **queued**, not obeyed at once: the hand-over would stop
the running turn, and the user asked to hand the session over, not to lose the
turn. It is drained by the same effect that opens `all`-mode targets, on the
`sessions` change that ends the turn, and the menu says so before the click.

---

## 11. Reading the screen

`renderScreen` is a quarter of a terminal emulator: a grid of cells, cursor moves,
erases, scrolling. It has to be. The CLI paints words at absolute columns rather
than writing spaces, so stripping escapes instead of placing cells runs
`Do you want to` together into `Doyouwantto` and loses every column the layout
depends on.

The pty is opened at a **fixed** 120×40 and the parser is given the same
constants, not a window measurement. A disagreement silently moves every column
the parser reads: the same capture that reads `idle` at its true size reads
`unrecognised` two columns narrower.

The grid is **stateful and outlives the chunks** (`createScreenBuffer`). This is
not a detail. The replay buffer a mounted terminal gets is bounded — `trimReplay`
drops the oldest chunks past 256KB — and re-rendering that buffer from blank loses
every row painted before the retained window and not repainted since. The
composer's two `─` rules are exactly those rows, while the `❯` between them is
repainted constantly. Measured on a 996KB capture: the whole stream reads `idle`,
its last 256KB reads `unrecognised`/`no-composer` — and a screen with no composer
refuses every send and cannot see an approval either. One long turn crosses 256KB.

So each chunk is written to the buffer once, and never re-fed. A half-escape at
the end of a chunk is carried into the next one; a stateless renderer could drop
it because the following call re-rendered everything anyway.

---

## 12. What we have not observed

Written down because the parser's rule is to recognise or refuse, and these are
the places where refusing is what it will do.

- **A phone-originated turn.** Never seen. Its `promptSource` and `origin.kind`
  values are unknown, which is why they are not filtered on.
- **A hand-over verified end to end on one build.** Every fix in this branch is
  covered by tests, typecheck and code trace; the empirical evidence is the
  captures under probe and one process-count measurement. Nobody has yet watched
  a new session under `all` go active by itself, be answered from a phone, and
  come back — that is the first thing to check against a real build.
- **Prompts other than the ones captured.** The corpus is this machine's sessions.
  A prompt shaped differently reads as `unrecognised`, which refuses injection and
  carries no answer — the intended failure, but a failure.
- **Windows.** The pty path exists and is marked, but none of this has been run
  there.
