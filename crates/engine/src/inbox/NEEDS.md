# Inbox package needs

Notes for the lead. Each item has a local workaround in place.

- `EngineHooks` has no inbox field. The inbox keeps its own `InboxHooks` object instead. The app must call `Inbox::init(client, cx)` and then `inbox.update(cx, |inbox, _| inbox.set_hooks(..))` with an implementation that wires attention (`inboxNotificationProject`, `allowsProjectNotificationIndicator`, `playCue`, `rememberNotificationProjects`), automations (`claimInboxAutomationRuns`), submit (`onSubmit` with `ciRepair`), and the shell (new sessions, tabs, sidebar, `onSelectHistorySession`, `default_cwd`).
- Nothing tells the inbox when notification preferences change or a mute expires. The attention package or the app should call `Inbox::notification_preferences_changed` then.
- The app should call `Inbox::window_became_visible` and `PrChecks::window_became_visible` when the window shows again (`visibilitychange`). PR check polling reads `WorkspaceHooks::window_hidden`.
- `inbox_ask.rs` includes `src/instructions/inbox.md` by path. Move the file next to the module before `src/` is deleted at cutover.
- `rail.rs` and `inbox_media.rs` carry small copies of `looksLikeProject`/`collectRailProjects` (projects package) and `sniffImageMime` (workspace package), marked `TODO(port)`.
