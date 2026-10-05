# What attention needs from other packages

Attention works around each gap locally. The lead can move these into
`runtime::hooks` later; the local traits keep the same method names.

## Runtime

1. A way to send a turn. `SubmitHooks` only has `sessions_flushed`. The queue
   dispatch, Steer, Resume, and the usage limit resume call
   `AttentionSubmit::submit(SubmitRequest)`, which the submit package (or the
   app) installs with `Attention::set_submit`. A `SubmitHooks::submit` would
   replace it.
2. Approval and question answers. `HarnessHooks` lacks `respond_approval`,
   `respond_question`, and `keep_question_open`; `RemoteHooks` lacks a
   remote-session check and remote `approve` and `answer`;
   `OrchestrationHooks` lacks `lead_for_session`; `WorkspaceHooks` lacks
   `focus_open_session`, `open_history_session`, and `inspect_worker`.
   Attention defines them on `ApprovalRouter` (`Attention::set_approval_router`).
3. The visible session and window focus. `WorkspaceHooks` has no
   `active_session_id`. The workspace package must call
   `Attention::set_focus(cx, AttentionFocus { .. })` when the focused pane,
   the Inbox page, or the Inbox Ask changes, and the app must call
   `Attention::set_window_focused(cx, bool)` from the window focus event.
4. A shared `Kv`. The engine has no settings store handle, so
   `AttentionConfig` takes one. An `Engine::kv(cx)` would let every package
   share it.

## Submit and remote

5. At the end of a turn (App.tsx line 6935) and when a remote turn ends
   (line 10425), call `Notifier::announce_finished_later(session_id, cx)`.
6. The submit package plans its own `message_queue` module and queue
   dispatch. Attention's `Queues` entity owns the dispatch effect; submit
   should not run a second one. Submit still needs `queued_message_for_submit`
   and `dequeue_queued_message` inside `onSubmit` (attention::queue has them,
   but submit cannot depend on the attention feature).

## App

7. `Attention::init_native(kv, data_dir, children, cx)` after `Engine::init`.
   `children` is the harness bridge's `Children`; without it the Codex and
   Grok usage chips report their CLI as missing.
8. On `NotifierEvent::Clicked`, unminimize and focus the window. The notifier
   already asks the router to open the session.
