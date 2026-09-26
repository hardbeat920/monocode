# Remote Control for MonoCode sessions

Plan for letting a MonoCode conversation be driven from claude.ai / a phone,
using Claude Code's own Remote Control. Written at the end of a session that
established *why* it does not work today and *that* there is a way to make it
work; the implementation itself is not started.

---

## 1. Why it does not work today

MonoCode runs Claude headless so it can drive its own UI:

```
claude --output-format stream-json --input-format stream-json
       --verbose --include-partial-messages --permission-prompt-tool stdio ...
```

Remote Control never starts in that mode. Measured on this machine by the
presence of a `bridge-session` record in the session file:

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
link. Run under `script -q /dev/null` (a pty), the TUI emitted:

```
here, on your phone, or at
https://claude.ai/code/session_01Q5d1qZzZoArEnaHyjYwm91/rc
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

## 3. The mechanism

One conversation, one process at a time — hand it over and take it back:

1. **Hand over.** Stop the thread's headless child. `stopClaudeSession`
   preserves the resume state (verified while fixing #445), so the conversation
   is not lost.
2. **Open.** Spawn in a pty:
   `claude --resume <providerSessionId> --remote-control <name>`
3. **Surface.** Parse the `claude.ai/code/session_*/rc` link out of the TUI
   output and show it (link + QR).
4. **Use.** The user drives the conversation from the phone. The pty process
   owns it.
5. **Take back.** On close, kill the pty; MonoCode resumes the same
   conversation headless on the next turn.

Both processes must never run at once. That is already enforced: Claude refuses
a second process on the same conversation (the "cannot be launched inside
another Claude Code session" check seen today), so the sequencing is not
optional — it is the only thing that works.

## 4. What to build

### 4.1 Setting

`monocode.remoteControl`, following the existing flag pattern in
`src/features/settings/model/settings.ts` (see `CLAUDE_HOOKS_KEY` /
`loadClaudeHooks` / `saveClaudeHooks`, and note those are `localStorage`-backed).

| value | behaviour |
| --- | --- |
| `manual` | **default.** Nothing automatic; each session can be opened by hand |
| `all` | Every Claude session is opened for remote control automatically, named after its project |

Surface it in `src/features/settings/ui/SettingsView.tsx` next to the existing
Claude toggles.

### 4.2 Per-session action

Available for an **existing, already-running** session, not only new ones — the
hand-over in §3 works on a live conversation.

- Entry point: session/tab context menu (see `useProjectMenu.tsx` and the tab
  menus added in `0d3db9c`) and/or the composer's slash menu, following
  `RESUME_COMMAND` in `src/features/sessions/model/resumeCommand.ts`.
- Claude-only, the same way `/resume` is gated: `harness === "claude"`.
  Other harnesses have no equivalent.

### 4.3 Naming

`--remote-control <name>` takes a name; use the project name so a phone list of
sessions is readable. `projectName` already exists in `src/shared/lib/paths`.
For `all` mode, name every session after its project (plus a discriminator if
one project has several threads).

### 4.4 Link extraction — expect friction here

The link arrives inside the TUI's rendered output, wrapped in ANSI escapes and
an **OSC 8 hyperlink**, not as a bare URL. Raw bytes from the test:

```
]8;id=1d1gso6;https://claude.ai/code/session_01Q5d1qZzZoArEnaHyjYwm91[...]8;;
```

So: strip ANSI, and match the OSC 8 payload rather than scanning for plain text.
The `/rc` suffix is the remote-control URL. Match defensively — this is TUI
output, not an API, and it can change between CLI versions.

## 5. Files likely involved

| Area | File |
| --- | --- |
| pty spawn | `src-tauri/src/pty.rs` |
| stop/resume, binding | `src/integrations/harness/providers/claude/claude.ts` |
| setting | `src/features/settings/model/settings.ts` |
| settings UI | `src/features/settings/ui/SettingsView.tsx` |
| session action wiring | `src/app/App.tsx` |
| menus | `src/app/shell/useProjectMenu.tsx`, `src/app/shell/TitleBar.tsx` |
| slash command (optional) | `src/features/sessions/model/resumeCommand.ts` as the pattern |

## 6. Constraints learned the hard way today

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
  punctuation and drive colons included. One-way only (#445).
- **A turn holds open while tasks are listed.** `maybeFinishTurn` refuses to end
  a turn while `agentTasks`/`backgroundTasks` are non-empty, and the grace timer
  is only armed when a task is *removed*. Anything that registers a task must
  remove it (#446).
- **stderr is the only place a dying child explains itself.** Unclaimed stderr
  now reaches the console; keep that in mind when the pty child misbehaves.

## 7. Tests worth writing

- Setting defaults to `manual`; `all` is opt-in.
- The action is offered for Claude and hidden for other harnesses (mirror the
  `/resume` visibility tests in `Composer.test.ts`).
- Link extraction: OSC 8 wrapped URL, ANSI-laden output, and output with no link
  (should not produce a bogus one).
- Hand-over ordering: the headless child is stopped before the pty starts, and
  the conversation binding survives the round trip.
- `all` mode names each session after its project.

## 8. Open questions

- **Message sent in MonoCode while remote control is active.** Two processes on
  one conversation is impossible, so MonoCode must either block sending with a
  clear reason, or close remote control first and take the conversation back.
  Decide deliberately — silently doing either would confuse.
- **Lifetime.** Does remote control stay open until explicitly closed, or close
  when the app closes / thread is archived? For `all` mode this decides whether
  dozens of pty processes accumulate.
- **`all` mode cost.** One interactive CLI process per session is not free.
  Consider opening lazily (on first use) rather than for every thread at once.
- **Unverified end to end.** The pty test proved the link is produced; nobody has
  yet connected from a phone and driven a MonoCode conversation through it.
  Confirm that early — it is the assumption the whole feature rests on.
