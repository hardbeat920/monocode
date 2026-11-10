# Native preview 14 change audit

Prepared on 2026-11-05 for the next native preview. The release-facing summary is
in [preview release notes](preview-release-notes.md).

## Scope and baseline

- Last published native preview: [0.16.0-preview.13](https://github.com/hardbeat920/monocode/releases/tag/native-v0.16.0-preview.13), published 2026-11-03 at 14:09:31 UTC. GitHub release metadata was checked while preparing these notes.
- Baseline tag: `native-v0.16.0-preview.13`, resolving to `00c693077d7e74ae72092e4194f5cd2384585467`.
- Reviewed through: `da39885e5503a924647bd19b2a2376bc15eea0c0` on `main` (2026-11-05). Local HEAD matched GitHub's `main` when checked.
- [Exact source comparison](https://github.com/hardbeat920/monocode/compare/00c693077d7e74ae72092e4194f5cd2384585467...da39885e5503a924647bd19b2a2376bc15eea0c0).

The inventory uses Git ancestry, rather than a date filter, and covers all 26
non-merge commits and one merge-only integration commit after preview 13.
Descriptions reflect the final source diff. The previous preview's changes are
recorded in the [preview 13 audit](preview-13-change-audit.md).

The native workflow derives `0.16.0-preview.14` from its `native-v…` tag and reads
`docs/native/preview-release-notes.md` for both the release body and update-feed
notes. The production app's version remains `0.16.0`, and its changelog is not
used as the native preview's release body. The source inventory stops at the
last application change; the release-preparation commit adds these notes and
records the candidate's validation.

## Native desktop changes

### Terminals, tabs, and new sessions

- **Terminal placement:** Settings → General → Terminal replaces the former side-panel toggle with Bottom, Side panel, and Title bar choices. Existing preferences fall back to the old toggle until a placement is selected, and the two preferences remain in step. Side-panel placement requires the Focused layout and an eligible local project; title-bar placement is local-only. With title-bar placement, Cmd/Ctrl+J opens a fresh terminal tab, and the title-strip plus button offers New Session and New Terminal. Footer terminal controls are hidden for title-bar and active side-panel placement. From `4180a651`.
- **Split Terminal Right:** Cmd/Ctrl+Shift+J opens a new shell beside the focused pane in title-bar placement. In Bottom or Side panel placement it adds a terminal to that location instead. The action appears in the app menu and keybinding settings. From `058c8473`.
- **Terminal drag-out:** dragging the last shell out of the side panel into a title tab or split keeps the panel open and creates a replacement shell. Rendering the panel also ensures it has a shell. The original running shell still moves into its new pane. From `c78684fe`.
- **Terminal close confirmation:** closing a pane or title tab, including bulk tab closes, prompts before closing terminal panes with foreground processes. A bulk operation lists the running processes together and leaves the tabs intact on Cancel; idle shells close without a prompt. From `c8ec3de2`.
- **Saved drafts and pinned tabs:** a title tab gets a dashed border if any chat pane it contains has a saved draft. Close All and Close Others shortcuts now preserve pinned tabs, matching the tab menu. From `058c8473` and `7c94f408`.
- **Split-session defaults:** the new-tab and split-session paths share settings initialization. Cmd/Ctrl+D and Cmd/Ctrl+Shift+D now apply the provider's model choice, selected account, preferred effort, and default access mode, while preserving the working-copy scope. From `3bf867de`, with the declaration moved before the test module in `da39885e`.

### Composers, navigation, and appearance

- Session composers start one line tall and grow with their text. Transcript text, task panels, and provider-switch rows align with the composer's text inset. Clicking composer chrome preserves input focus instead of allowing an ancestor to take it. From `64fe8347`.
- Mono composers use the session composer layout: model, settings, and access controls move into the box's toolbar, the session action button replaces the separate send button, and the context meter aligns beneath it. Attachment and emoji controls remain below the box. From `1646e4aa` and `64fe8347`.
- Supported models show an amber fast-mode bolt on the model trigger when fast mode is active, including Mono chats and Quick Composer. A separate Fast pill avoids a duplicate indicator in the applicable beside-controls layout. Mono transcript signatures use mascot tiles, and taller menu line boxes prevent Windows text descenders from clipping. From `9ee7b3dd`.
- Focused navigation adds a New row, highlighted for an empty session, a Notes count that updates as the notes view changes, and a compact Search button beside Sidebar options. Navigation hints, counts, unread dots, and section controls share a trailing alignment. From `64fe8347` and `9ee7b3dd`.
- A collapsed sidebar peeks from the left edge or sidebar toggle in both rounded and boxy Focused layouts. Glass blur eases with the peek animation instead of disappearing abruptly at the end. Classic layout remains excluded. From `da39885e`.
- Settings navigation keeps Search above the scrolling page list and Back to app at the bottom. From `11769e9e`.

### Claude authentication and provider lifecycle

- On Unix, provider retirement closes stdin, allows a graceful exit, then escalates through SIGTERM to the leader and finally SIGKILL to the group if needed. Cleanup happens off the UI thread and provider-pool lock. App quit also gives leaders time to stop before killing remaining groups. This avoids interrupting Claude Code's rotating-token Keychain save. Non-Unix retirement retains the existing termination behavior. From `76e72d9a`.
- Claude model catalogs are cached for six hours to reduce cold CLI probes. Provider CLI updates invalidate catalogs for every account; Claude's catalog reload is deferred to the account actually used. From `76e72d9a`.
- An expired Claude access token with a refresh token retains the last usage reading rather than presenting a sign-in prompt. The CLI renews the token on its next run; the usage reader does not refresh it. On macOS, a present but cleared Keychain item is authoritative, so an old credentials file cannot hide a genuine sign-out. From `76e72d9a` and `2ec5b66c`.
- Waiting browser sign-ins can be cancelled from the usage footer or the add-account editor's Back/Cancel controls. Returning to the app schedules a five-second grace period for an unfinished sign-in before resetting it; completed attempts remain intact. From `47e78877`.
- Resumed Claude turns ignore empty results for replayed background-task notifications in the normal turn handler and both stop paths. Usage limits carried by those replays are cleared, allowing the actual turn's reply or error to arrive. Real background-task turns still finish normally. From [#1301](https://github.com/hardbeat920/monocode/pull/1301), `c209a775`.

### Rendering, diagnostics, and performance tooling

- Streaming updates align with the shared animation clock while it is running, combining reply and animation redraws. Finished updates are applied immediately. Sized CoreText fonts are cached, and per-frame notification checks borrow preferences instead of cloning them. From `f8540190`.
- The macOS stalled-frame watcher keeps retrying until a delivered frame answers outstanding demand, including demand raised during a frame. Temporarily occluded or screenless windows are retried; hidden windows remain stopped. The active app's main window qualifies for recovery even when it is not key. From [#1314](https://github.com/hardbeat920/monocode/pull/1314), `e6507351`.
- The Metal layer restores three drawables to avoid the one-second wait encountered when a screenshot overlay holds both buffers. Presented-frame tracking detects frames that were drawn but never shown, and the active window retries a small frame nudge to recover. The third drawable costs one additional window-sized buffer. From `5199ee84`.
- GPUI warnings are saved to `~/Library/Logs/Monocode Native/gpui.log`, with rotation above 4 MiB and repeated-message suppression. Diagnostics include renderer waits of at least 150 ms, covering windows during unshown frames, and short frame summaries after file drops. Logger initialization is available in test builds as well, fixing the all-targets unused-code warning. From `5199ee84` and `32798214`.
- Opt-in `MONOCODE_FRAME_LOG` records frame counts, draw times, main-thread CPU on macOS, rendered/reused views, invalidation origins, transcript measurement time, and text-shaping counts. `scripts/perf/frame-run.sh` runs a scripted Claude reply with isolated development data; `MODE=scroll` measures scrolling after the reply and `PROFILE=1` optionally samples stacks. Automatic prompting requires the replacement Claude binary. See the [performance guide](../../scripts/perf/README.md). From `f8540190`.

## Phone companion changes

The phone composer stays editable when the desktop disconnects, showing
“Reconnecting to desktop…” as its placeholder. Send and Stop are disabled until
reconnection; a draft that can be sent gets a pulsing status dot and an
accessibility hint. Dictation remains available. A session error waits until the
desktop is back before replacing the field. From
[#1319](https://github.com/hardbeat920/monocode/pull/1319), `e2a671ac`.

This change is under `mobile/` and requires a separately distributed companion
update; installing the native desktop preview alone does not update the phone UI.
The contribution reports mocked component tests and TypeScript checks, with no
device or simulator run. Those results are historical PR evidence, not checks
performed as part of this documentation pass.

## Shared React/Tauri and repository changes

- The React/Tauri Claude usage reader receives the same expired-but-refreshable-token handling and authoritative macOS Keychain behavior in `src-tauri/src/rate_limits.rs`. The native implementation is separate, in `crates/monocode-core/src/usage/claude.rs`; the Tauri code does not ship in the native binary. From `76e72d9a` and `2ec5b66c`.
- The root README now has separate Native and Tauri download columns, preview 13 installers, a larger screenshot, corrected image links, and content cleanup. These documentation changes are recorded by `c871ad01`, `3e8b3c84`, `bc8fccf7`, and `7e34a0d3`. Download links intentionally remain on the published preview until a new release exists.
- `94f69d98` formats composer and task-panel code. `eb051cb5` makes the process-retirement escalation test wait for group members to exit on loaded runners. `32798214` corrects logger setup for test compilation. These maintenance changes do not add separate user-facing features.

## Merged contributions

- [#1301](https://github.com/hardbeat920/monocode/pull/1301), @keyCat: replayed Claude background-task result handling in the native provider.
- [#1314](https://github.com/hardbeat920/monocode/pull/1314), @Kaloyanes: persistent macOS frame-demand recovery.
- [#1319](https://github.com/hardbeat920/monocode/pull/1319), @Kaloyanes: editable mobile composer during desktop reconnection.

The integration-only commit
[`775bdbc6`](https://github.com/hardbeat920/monocode/commit/775bdbc658fe5c4f9d12606e7ebb79c03754d6d0)
merges `main`; its source changes are represented by the non-merge commits below.

## Verification boundary

The notes were checked against the complete commit range and final source diff.
Regression coverage in that range addresses Claude replay handling and renewable
credentials, graceful process exit and escalation, abandoned browser sign-in,
terminal placement/splitting/drag-out/close confirmation, split-session defaults,
pinned bulk closes, composer alignment/focus, sidebar navigation
and peeking, frame-demand accounting, and offline phone composition. Added or
modified tests describe coverage, not proof that the release candidate passed.

Documentation validation verified whitespace, relative Markdown links, exact commit
and merged-contribution coverage, preview numbering, and preservation of the
existing installation/testing guidance with links moved to the preview 14 tag.
The subsequent release preflight is recorded in
[preview 14 verification](verification.md#preview-14-candidate--november-5-2026).
Existing [platform limitations](parity.md) still apply.

## Complete non-merge commit inventory

Every non-merge commit in the reviewed range follows in `git log --reverse`
order. The one merge-only integration commit is recorded above.

- [c871ad01](https://github.com/hardbeat920/monocode/commit/c871ad01e33d246e986ab941b356e3b166370498) — Add native preview 13 downloads to README install table
- [3e8b3c84](https://github.com/hardbeat920/monocode/commit/3e8b3c84d6a1bc6a784323c42eabe275313023f6) — Replace logo with larger screenshot
- [bc8fccf7](https://github.com/hardbeat920/monocode/commit/bc8fccf72e07bff969a3e61c121de425e6112bb1) — Fix image links and clean up README content
- [7e34a0d3](https://github.com/hardbeat920/monocode/commit/7e34a0d31d1f5b5816ddec385425a3cbb7344725) — Update README.md
- [4180a651](https://github.com/hardbeat920/monocode/commit/4180a65140cdf5dc47aa3b00bc56817fda769375) — Add title bar option for terminal placement setting
- [c209a775](https://github.com/hardbeat920/monocode/commit/c209a77502538e305197235da92105776eb8f00c) — Keep a replayed background-task result from ending a Native Claude turn (#1301)
- [f8540190](https://github.com/hardbeat920/monocode/commit/f8540190eee49da2c23ddd2b703c89400bd60676) — Add frame profiling and reduce streaming render overhead
- [1646e4aa](https://github.com/hardbeat920/monocode/commit/1646e4aaa9a378c90ace590fa2f63eacbd788887) — Align Mono chat composer with the session composer layout
- [64fe8347](https://github.com/hardbeat920/monocode/commit/64fe83475a5187b29bdf9559fbffdac91b4dc3b6) — Refine focused sidebar navigation and composer layout
- [94f69d98](https://github.com/hardbeat920/monocode/commit/94f69d985a6fcd3350273cb79343f30fe4c5f587) — Format composer and task panel code
- [76e72d9a](https://github.com/hardbeat920/monocode/commit/76e72d9adeab2dc5cb1650f7c6402ad158f7be94) — Stop Claude Code from signing out after token refreshes
- [2ec5b66c](https://github.com/hardbeat920/monocode/commit/2ec5b66ce0e65c49308e97113e388c8fbbc423c0) — Show Claude sign-in when the Keychain login was cleared
- [47e78877](https://github.com/hardbeat920/monocode/commit/47e788776ea2fc62fbfc94ced1a4d11f2ea09ea9) — Let a sign-in stuck waiting on a closed browser be cancelled
- [11769e9e](https://github.com/hardbeat920/monocode/commit/11769e9e4cf4a713fede4b5102e9b07aa7064f9c) — Put settings search atop the nav and Back to app at its bottom
- [e6507351](https://github.com/hardbeat920/monocode/commit/e6507351aacd1b3773d9f7b349f6b1b0b0f7b55c) — Native: keep retrying frames until a window's demand is answered (#1314)
- [eb051cb5](https://github.com/hardbeat920/monocode/commit/eb051cb505d4cdd2d4b364b49eeab3597a6d0519) — Wait for a retired process group to exit in the escalation test
- [5199ee84](https://github.com/hardbeat920/monocode/commit/5199ee84888e25b70caf8d71a2e1f58e1134e23d) — Stop the macOS window freezing after a screenshot drop
- [32798214](https://github.com/hardbeat920/monocode/commit/32798214069334d26b9db7a99bc423fbfc5cd0f9) — Install the GPUI log in test builds too
- [058c8473](https://github.com/hardbeat920/monocode/commit/058c84730743314392ca46021c8007a7ae77f2d4) — Add terminal splitting and title-tab draft indicators
- [3bf867de](https://github.com/hardbeat920/monocode/commit/3bf867de93796746ad1b7d34d4d357342b7ad640) — Start split sessions with the user's default access mode
- [c78684fe](https://github.com/hardbeat920/monocode/commit/c78684fe69cd430d0a6397c599f7235f34c64af2) — Keep the Terminal panel open with a fresh shell after dragging out
- [9ee7b3dd](https://github.com/hardbeat920/monocode/commit/9ee7b3dd1e408a76229d60fc2db29d1ec45728ea) — Show fast mode in model pickers and refine UI details
- [7c94f408](https://github.com/hardbeat920/monocode/commit/7c94f408ccc66de6c33b663b0ebbc3dd8735f8b6) — Keep pinned tabs open on the close-all and close-others shortcuts
- [e2a671ac](https://github.com/hardbeat920/monocode/commit/e2a671ac7e1145b5c85539aef3f1c2bc0b3fad26) — Mobile: keep the composer editable while the desktop reconnects (#1319)
- [c8ec3de2](https://github.com/hardbeat920/monocode/commit/c8ec3de2fe90c2431287316b7257e5e9e12bad89) — Confirm before closing running terminals in title tabs
- [da39885e](https://github.com/hardbeat920/monocode/commit/da39885e5503a924647bd19b2a2376bc15eea0c0) — Enable sidebar peek in boxy Focused layout and ease glass blur
