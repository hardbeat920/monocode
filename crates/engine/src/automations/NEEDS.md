# Notes for the lead from the automations package

Each item has a local workaround in place. None blocks the package.

## Shared files I changed

- crates/engine/Cargo.toml: the `automations` feature now turns on `submit`, `attention`, `history`, `projects`, and `dep:chrono`. Launches submit through `Submit`, reminders read attention's notification rules, launches place sessions in history's session folders, quick launches read projects' recents and worktrees, and schedules use chrono's local time.

## Inbox package

- `InboxItem` lives in the inbox package, which was unfinished when this package started. events.rs reads the same JSON through `InboxEventItem` (unknown fields kept in `extra`) and carries local copies of `inboxStartDraft` (without a body), `linkedWorkItemFromInboxItem`, and `linkedWorkItemFromAutomationEvent`. Once inbox is done, either switch events.rs to the inbox types or convert with serde at the call site.
- The Inbox's `onAppeared` should call `Automations::inbox_appeared(items, cx)`.

## Workspace package

- `Workspace::append_tab` is private. `LaunchHost` asks the app for `append_tab(tab, cwd)`, `activate_tab` (`setActiveTabId` plus `setComposerFocused(false)`), `focus_open_session`, `show_sessions` (close the search, Inbox, notes, and automations pages and show the sessions sidebar tab), `set_project_cwd`, and `place_session`. `place_session` must split the new session beside an open pane synchronously and return the tab id; `place_session_on_pane` is async and opens a stored session, so it does not fit as is.

## Attention package

- `Notifier::notification_clicked` drops identifiers that are not open session ids. Reminder banners use `reminder:<session id>:<due at>`; the click handler should send those to `Reminders::open_from_notification`.
- The submit flow's "dismiss notices for a continued session" (`dismissNoticesForContinuedSession` in App.tsx) should call `Reminders::dismiss_due(session_id)`.

## App wiring

- Store change notices (`StoreEvents::automations_changed` and `reminders_changed`) that come from elsewhere, such as a remote host, should call `Automations::notify_changed` and `Reminders::notify_changed`.
- Each window: `Reminders::attach_window(label, host)`, `register_window(label, ids)` with its open non-Inbox-Ask sessions, and `window_focused`; `QuickLaunch::attach_window(label, host)` and `window_focused`; `Automations::set_host` with the window launches should open in; `Automations::window_visible` on visibility.
- The history sidebar's reminder group (through `HistoryHost`) should read `Reminders::reminders()`.
- `QuickLaunchApp::set_shortcut` needs a global hotkey; monocode-platform has none yet. `QuickLaunch::apply_shortcut` should run again when the quick composer settings change.
- The app API launch path (App.tsx 9040-9069) can call `QuickLaunch::accept(label, launch, id, placement)`.

## Not ported here

- The floating panel window, the quick git popup window, and screenshot persistence before queueing (src-tauri `screenshots::persist`) belong to the later view package.
- The panel's catalog request and answer events: one process holds every window, so the panel reads the catalog directly. `live_quick_catalog` and `apply_quick_catalog` are ported as functions.
