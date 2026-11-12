# Resume running chats after a MonoCode restart (GPUI): audit and plan

Audit date: 2026-10-26. Scope: the GPUI app (`crates/monocode-gpui`) and the core it uses (`crates/monocode-core`), all local providers. This is a plan; no behavior was changed and no user data was read.

Goal: when MonoCode quits, crashes, or restarts for an update while chats are running, those chats continue on their own after relaunch.

## Status (2026-10-26): implemented

| Piece | Where |
| --- | --- |
| Marker with turn policy | `core/store/in_flight.rs`. Written and cleared in `runtime::run_turn_using`, stored as `in-flight:{id}` in the GPUI settings sidecar |
| Quit flag and cancel-before-kill (750ms budget, app hidden at once) | `core/process.rs` (`shutdown_all`, `register_turn`, `shutting_down`); `main.rs::hide_while_shutting_down` |
| Quit kill recorded as an interruption (no error row) | `runtime.rs` settle pass |
| Seal at launch (Cancelled tools and notice) | `Store::seal_interrupted_turn`, called from `restart_resume::prepare` in `main.rs` |
| Orphan reaper | `core/orphans.rs`, run off-thread at launch; resumes wait for it |
| Staggered resume, active chat first | `shell/interrupted_turns.rs` → `Conversation::resume_after_restart` (`conversation/restart.rs`) |
| Codex fresh-thread guard | `Settings::restart_continuation`, `provider.rs` |
| Menu-bar quit uses the shared quit | `monos/mod.rs` |
| Quit and Reload prompts | `Shell::request_quit`, `request_reload` |
| Automation and worker exclusion | `Conversation::managed`, set in `automation_runs.rs` and `orchestration.rs` |

Verified: unit tests for every piece, plus a live launch against an isolated `--dev-data` database. That run settled a seeded running tool, reaped a planted orphan, auto-continued a background Codex chat, and hit the Codex guard with a nonexistent thread.

Not yet verified live: Cmd-Q during a real provider turn, and each provider continuing a real interrupted session (the provider checks below). The OpenCode v2 reattach mode remains a follow-up.

## How a turn ends today

- `Conversation::launch_prompt` (`conversation.rs:1021`) → `begin_turn` sets `busy` in memory only → `runtime::run_turn` runs the provider on a background thread and writes rows to the store as they stream.
- When the provider returns, `run_turn` (`runtime.rs:~970–1130`) settles the turn. A cancel marks running tools Cancelled and writes no error. Any other error appends an `error` row and marks running tools **Failed**. It then flushes rows, finishes the checkpoint, and saves the turn receipt.
- Nothing records "this chat was mid-turn". The sidebar's Working state (`shell/session_loading.rs::Metadata::busy`) comes from the live `Conversation` only.

## Findings

### Exit paths

1. **P1: Every exit kills providers without marking anything.** `main.rs::quit` and `main.rs::restart` call `process::shutdown_all`, which SIGKILLs each registered provider's process group and waits up to 1s, then exits. All of these route through it:
   - Cmd-Q / menu Quit (`Shell::request_quit`, `shell.rs:1887`)
   - Closing the last window (`main.rs:442`)
   - "Reload Monocode Native" (`shell.rs:~1860`)
   - Updater install and relaunch (`notifications/updater.rs:265–277`)
2. **P1: Quitting from the Mono menu-bar icon skips `shutdown_all`.** `monos/mod.rs:203` calls `cx.quit()` directly. Providers are spawned in their own process group (`process::isolate`), so they are not killed with the app. Some exit on stdin EOF. `opencode serve` has `stdin(null)` and keeps running. A crash or `kill -9` leaves the same orphans.
3. **P1: There is no orphan reaper.** `isolate` sets `MONOCODE_HARNESS_PARENT=<pid>` on every child, but nothing in the GPUI app or core reads it. Auto-resume makes orphans dangerous: an orphaned agent and the resumed turn could both edit the same worktree, or drive the same provider session.

### What a killed turn leaves on disk

4. **P1: A quit-time kill is saved as a failure, or not saved at all.** After `shutdown_all` kills the child, the run thread sees an ordinary provider error ("OpenCode server exited", a broken pipe, and so on). That is not a cancel, so `run_turn` writes an `error` row, marks running tools Failed, and saves the receipt. It races the 1s exit window, so the result varies:
   - **Settled in time:** the chat reloads showing a scary provider error for a turn the user simply quit.
   - **Lost the race, or crashed:** rows are left exactly as last streamed.
5. **P1: Unsettled tool rows spin forever on reload.** Running tools are only cleared in memory (`TranscriptView::stop_streaming`, `transcript.rs:~2455`) or by `run_turn`'s settle pass. A turn that never settles leaves tool rows with `state: Running` in the store. On reload they render with the live spinner (`transcript.rs:~4871`) and are never cleared.
6. **P2: Checkpoints already handle continuation.** `checkpoint::begin_turn` (`checkpoint.rs:203–212`) finds an unfinished active turn and continues from its original baseline. Review and undo of a resumed turn cover the interrupted turn's edits too. No change needed. Keep `recover_orphaned_turn` as it is.
7. **P2: Queued follow-ups already recover safely.** `Store::follow_ups` re-pauses any undelivered queue on load ("recovery is always explicit"). Resume must not unpause them; the user releases the queue.

### Resuming each provider

`provider_id` is saved as soon as the provider reports it (`runtime.rs:741`, `store.provider_id`), so it survives a kill.

| Provider | Resume path | Risk after a mid-turn kill |
| --- | --- | --- |
| Claude | `--resume <id>`. New chats get `--session-id` up front (`provider.rs:1509`), so an id exists from the first frame. | Low. The transcript ends with a dangling tool call; verify Claude accepts a new prompt after it. |
| Codex | `thread/resume`. A missing thread silently falls back to a **new thread** (`provider.rs:500–508`, `recoverable_codex_resume`). | **High.** If the kill came before the rollout was written, "Continue from where you left off." goes to an empty thread and the agent improvises. |
| OpenCode v1 | Fresh `serve`, `GET /session/{id}`, `repair_unsupported_file_turn` | Low. |
| OpenCode v2 | Managed service, which is not a MonoCode child | **High.** The service keeps running the turn unattended after quit; no `/interrupt` is sent because `cancel` is never set. A Continue at relaunch hits a busy session. |
| Pi | `--resume` / `--session` (`pi.rs:50`) | Verify. |
| ACP (Hermes etc.) | `session/resume` or `session/load` (`acp.rs:247–249, 559`) | `session/load` replays history and only live updates render. Verify with one ACP agent. |
| Devin | Separate module | Verify. |
| Remote projects | The turn runs on the remote host; `remote.shutdown()` closes only the connection | Nothing to resume. The remote snapshot (`store/remote.rs`, `running`) already shows it running after relaunch. Exclude. |

8. **P1: A chat with no `provider_id` cannot be continued.** If a non-Claude chat is killed before the provider reports a session, "Continue" would start a fresh session that never saw the request. The only correct option is to re-send the original user prompt, and only when the turn produced no tool activity, so no side effects are repeated. Otherwise show the interrupted notice and let the user decide.

### Turn policy

9. **P1: The interrupted turn's mode is not saved, and launch changes it.**
   - `launch_prompt` clears `settings.orchestrator` (`conversation.rs:~1156`).
   - Composer modes (plan, operator) reset after Send. `operator_enabled` and `app_read_only` are only derived at launch.
   - `TurnInfo` records provider and model, not mode or access.

   A plain Continue could therefore run a **plan-mode or read-only turn with write access**. The marker must store the effective mode and access, and resume must re-apply them.
10. **P2: Some turn states must never auto-resume:**
    - `pending_provider_switch` (a handoff that was mid-flight)
    - `new_worktree` with no `worktree_cwd` (worktree creation was interrupted; resuming could create a second one)
    - `worktree_removed`
    - `usage_limit` set (the usage-limit flow owns that chat)
    - Automation runs: `automations::recover` already marks them "Interrupted when MonoCode last stopped" and cancels them (`shell/automation_runs.rs:139`)
    - Mono-driven sessions (`settings.mono`): Monos have their own outbox, delivery receipts, and `UsageResumed` path
    - Orchestration workers: resume the lead only, never workers independently
11. **P2: Several chats resuming at once all start together.** Each resumed chat starts a provider process plus catalog and git refreshes at launch. With several chats that is a startup spike; stagger resumes (for example one every 500ms, active tab first).

### UI

12. **P2: Launch builds only the chats on screen.** Background tabs and history stay metadata-only. Resume needs a narrow exception: call `ensure_session` for marked ids only. The usage-limit audit's open item ("armed threads after restart", `usage-limit-continuation-audit.md` step 1) needs the same piece.
13. **P3: Quitting gives no warning.** `request_quit` asks only about unsaved files, and the Reload prompt says "Running agents and terminals will stop." Once resume ships, both should say running chats will resume after reopening.

## Plan

### Step 1: Core — the marker and a quit flag

- **Store API** (`store.rs` plus the shared store):
  - `set_in_flight(InFlight)`, `clear_in_flight(id)`, `in_flight() -> Vec<InFlight>`.
  - `InFlight { session_id, cwd, turn_start_row, mode: { plan, orchestrator, operator, access }, had_tool_activity, prompt_row }`.
  - Use the existing shared table `in_flight_sessions` for `(session_id, cwd, sort_index)`. Put the policy fields in a `in-flight:{id}` settings value, because existing databases are never altered.
- **Quit flag:** `process::begin_shutdown()` sets a process-wide `QUITTING` flag and is called first in `main.rs::quit` and `restart`. Fix `monos/mod.rs:203` to call `crate::quit` instead of `cx.quit()`.
- **`run_turn` under `QUITTING`:** treat the provider error as an interruption.
  - Mark running tools Cancelled with the output "Interrupted when MonoCode quit."
  - Append no error row, and flush quickly.
  - Never clear the marker.

### Step 2: GPUI — mark and clear around each turn

- `Conversation::launch_prompt`, right after `begin_turn`: write the marker with the effective mode captured *before* the orchestrator and plan flags are cleared. Write it on the background executor, but before `run_turn` spawns, so a crash in the first second is covered.
- Clear it in the `finished` branch, unless `QUITTING` is set, and in `stop()`.
- Skip remote chats, mono chats and automation chats entirely, so no marker is ever written for them.

### Step 3: Core — clean shutdown and orphan reaping

- **Before killing:** set `cancel` on every running turn (keep a registry of turn cancel flags next to `process::active`). Then give providers up to 1s to send their abort before `shutdown_all` SIGKILLs. For OpenCode v2 this sends `/interrupt` to the managed service. For ACP it allows a `session/cancel`.
- **Orphan reaper at startup:** before any resume, find processes whose `MONOCODE_HARNESS_PARENT` is a dead pid and kill their process groups. Include `opencode serve`. Port the Tauri reaper (`src-tauri/src/harness.rs:1544`), which already parses `ps` output for this env.

### Step 4: GPUI — restore at launch

In `Shell::new`, after the workspace restores and the orphan reaper finishes, read `in_flight()` and order the entries: active tab, then visible panes, then the rest. For each, call `ensure_session`, then `Conversation::resume_after_restart(entry)` through `defer_history_action`. That method:

1. **Seals the turn's rows on disk:** running tools become Cancelled ("Interrupted when MonoCode quit."), unanswered approval and question rows are dropped, and streaming ends. This fixes finding 5 even when resume is skipped.
2. **Appends a system row** "Turn interrupted when MonoCode quit.", once per turn.
3. **Checks eligibility** (finding 10, account exists, cwd/worktree exists, chat has a user message). If the chat is ineligible, it clears the marker and stops, leaving the notice and a manual Continue.
4. **Restores the turn's mode and access** from the marker, then:
   - If `provider_id` is set: `launch_prompt("Continue from where you left off.")`.
   - If there is no `provider_id` and `!had_tool_activity`: re-send the original prompt from `prompt_row`.
   - Otherwise: show the notice only.
5. Lets the new turn write its own marker. The old one is cleared after launch, so resume happens at most once.

Stagger launches (finding 11). Leave queued follow-ups paused.

### Step 5: Provider fixes

- **Codex:** when resume falls back to a new thread (`provider.rs:507`) during a restart resume, fail with "The saved Codex thread was not found" instead of starting fresh. Pass a `restart_resume` flag on `Settings` so normal sends keep today's behavior. Mono already does the same.
- **OpenCode v2:** covered by the step 3 cancel pass. As an optional later step, check session status at relaunch and re-attach to the event stream instead of interrupting and re-prompting.
- Verify Claude, Pi, ACP and Devin with a real mid-turn kill (see the checks below).

### Step 6: Copy and quit prompt

- `request_quit` does not ask about running chats. They continue on the next launch, and a confirmation would force a mouse click on a keyboard-only Cmd-Q. Only unsaved files still ask.
- Update the Reload prompt text to match.
- Optional preference: "Resume running chats after restart", on by default.

## Order and size

1. Steps 1–2 (marker, quit flag, `run_turn` handling): small, with unit tests in `runtime` and `store`.
2. Step 4 sealing only (no auto-continue yet). This alone fixes the error rows and stuck spinners.
3. Step 3 (cancel pass and reaper).
4. Step 4 auto-continue, plus the Codex guard from step 5.
5. Step 6.

## Acceptance checks

- For each provider (Claude, Codex, OpenCode v1/v2, Pi, one ACP agent, Devin): start a turn that is running a tool, Cmd-Q, relaunch.
  - The chat shows the interrupted notice and continues in the same provider session without being opened.
  - No error row. No spinning tool.
- Repeat with `kill -9`, the menu-bar Quit, closing the last window, Reload, and an update relaunch.
- After any of those exits, `ps` shows no leftover provider processes once relaunch finishes. OpenCode v2 reports the session idle before relaunch.
- A plan-mode or read-only turn resumes in plan or read-only mode.
- Stop then quit: no resume. Finished turn then quit: no resume. Quit twice during resumed turns: one notice and one resume each time.
- Codex chat killed before its thread exists: shows the notice and does not start an empty thread.
- Usage-limited chats, Mono chats, automation runs, orchestration workers, remote projects, removed worktrees, and deleted accounts: none auto-resume, and each shows the correct state.
- Five chats running at quit resume one after another, active tab first, without freezing the UI.
- Queued follow-ups remain paused after resume.
- Undo/review after a resumed turn covers the edits from before and after the restart.

These are proposed checks, not completed tests. No database access or changes were performed during this audit.
