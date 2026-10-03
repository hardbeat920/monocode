# What history needs from other packages

History works around each gap locally. None blocks the package.

## Shared files I changed

- crates/engine/Cargo.toml: the `history` feature now also turns on
  `dep:chrono`, for the local-midnight start of the "Today" filter.

## Runtime

1. `HistoryHost` (host.rs) exists because `EngineHooks` has none of these
   calls. The app installs one with `History::set_host`:
   - Workspace: `workspace_tabs`, `tab_close_scope`, `commit_removal`,
     `confirm_removal`, `open_session`, `active_pane`,
     `switch_session_in_tab`, `inspect_worker`, `open_note_chat`.
   - Dialogs: `alert`, `confirm`, `choose_delete`.
   - Projects: `unused_worktree`, `remove_worktree`.
   - Orchestration: `run_lead_for_session`, `stop_active_run`,
     `delete_session`, `live_runs`.
   - Submit: `finish_preparing_handoff` (submit/handoff.rs has
     `is_preparing_handoff`, `complete_handoff`, and
     `build_deterministic_handoff`).
   - Inbox: `reveal_linked_update`, `linked_work_item_changed`.
   - Remote: `remote_session_for`, `remember_remote_session`,
     `select_remote_session`.
   - The model catalog: `new_session` and `new_default_session`. The
     defaults make blank chats without the catalog.
2. `Sessions` gaps:
   - No setter for the saved fingerprint alone. Renaming a closed session
     calls `adopt_restored`, which also records the provider and last user
     block.
   - The pending loads (`sessionLoads`) are private, so releasing a deleted
     lead's workers cannot invalidate reads in flight.
3. `SessionBackend` has no note commands. `StoreNotesBackend` takes the
   `Arc<SessionStore>` (`StoreBackend::session_store`) and the data
   directory, which `StoreBackend` does not expose.
4. A shared `Kv`. `HistoryConfig` takes one, as attention and projects do.

## Duplicates to merge

- `remove_session_from_workspace` is in history/session_workspace_lifecycle.rs
  and workspace/lifecycle.rs. It only uses layout types; one copy in
  `monocode-layout` would serve both.
- `looks_like_project` and `is_local_project` (history/paths.rs) also exist
  in submit, attention, workspace, inbox, and projects.
- `locale_compare` is in app_search.rs and workspace/files/file_index.rs.
- `release_orchestration_worker` (history.rs) may also appear in the
  orchestration package.
- `onSelectHistorySession` is `History::select_session` and
  `Workspace::open_session`. History's asks the live orchestrator for the
  lead and reveals inbox updates; wire `HistoryHost::open_session` to the
  workspace's.

## App wiring

- `HistoryPackage::init(HistoryConfig::new(kv, notes_backend, cx), cx)`
  after `Engine::init`.
- `commit_removal` can call `Workspace::apply_session_removal`; its
  `Sessions` update is a no-op after history's own.
- `Search::set_files` with an adapter over the workspace `FileIndex`
  (`rank_project_files_limit` with `recent_opened_files`).
- `ProjectsHooks` history calls map to `History::patch_summaries`,
  `History::refresh`, `History::rebase_loaded_project`,
  `session_folders::rebase_session_folder_settings`, and
  `sidebar::place_session_in_project_folder`.
- On `NotesEvent::AddToChat`, call `notes_entity::add_note_to_chat`.
  Packages that write notes call `Notes::notes_changed`.
