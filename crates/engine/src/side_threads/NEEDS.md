# What side_threads needs from other packages

Side threads work around each gap locally. The local trait is
`SideThreadPeers` (side_threads/peers.rs), filled in with
`SideThreads::set_peers`. Every method has a default.

## Shared file I changed

1. crates/engine/Cargo.toml: `side_threads` now turns on `submit` and
   `attention`. The task asked to reuse submit's handoff and second-opinion
   models, the second-opinion turn goes through `Submit::on_submit`, and
   attention already ported liveAgents.ts.

## Workspace

2. `SideThreadPeers::open_session_beside(source_id, session_id, cwd,
   focus_composer)`: the tab half of `openSessionBeside` (App.tsx 7661).
   Split the tab holding `source_id` with `splitPane(layout, source_id,
   "right", session_id)`, focus the new pane, clear `diffFocused`, and
   activate that tab. With no such tab, `appendTab(newTab(session_id),
   cwd)` and activate it. Then `setProjectTerminalFocused(false)` and
   `setComposerFocused(focus_composer)`. The workspace is one entity per
   window, so the app glue should route this to the window's `Workspace`.
   The default does nothing, so the new chat is open but has no pane.

## Reminders and inbox

3. `SideThreadPeers::dismiss_due_reminders`: `sessionReminders.dismissDue`
   from `useSessionReminders`. No package has ported reminders yet.
4. `SideThreadPeers::mark_linked_session_update_seen`: the inbox package's
   port of linkedSessionSeen.ts (`monocode.linkedSessionSeen`), which must
   also notify its own listeners. Not written here to avoid a second copy.

## Submit

5. Route `SubmitAttentionHooks::dismiss_notices_for_continued_session` to
   `SideThreads::global(cx).dismiss_notices_for_continued_session(id, cx)`.
   `SideThreads` is a cloneable global, not an entity, so this call is safe
   from inside a submit.
6. `submit::pipeline::turn::drive` is private. side_threads/flows.rs has a
   minimal copy for its own channel type. A generic, `pub(crate)` `drive`
   would remove it.

## Runtime

7. A shared registry, catalog, and `Kv` on `Engine` (submit NEEDS 9).
   `SideThreadsConfig::from_submit(cx)` copies them from the `Submit` entity
   meanwhile.
8. The runtime never calls `SideThreadHooks::stop_all`. `SideThreads::init`
   registers an app quit observer that does the same work, which matches the
   App unmount effect.

## Seen while testing

9. `attention::tests::keeps_concurrent_questions_approvals_and_sessions_with_the_same_request_id_distinct`
   fails at scheduler seed 1 (`ITERATIONS=30`) with only the `attention`
   feature on. It passes on the default seed.
