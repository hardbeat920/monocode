# What projects needs from other packages

The projects package works around each gap locally. The lead can move the
local trait into `runtime::hooks` later; `ProjectsHooks` keeps the
TypeScript names.

## Runtime

1. `ProjectsHooks` (hooks.rs) exists because `EngineHooks` has no calls
   for any of these. The owners install one implementation with
   `ProjectsGlobal::set_hooks`:
   - Workspace: `project_cwd`, `set_project_cwd`, `tabs`, `active_tab_id`,
     `project_return_memory`, `open_files`, `append_tab`,
     `insert_tab_beside`, `set_tabs`, `set_active_tab`, `activate_tab`,
     `set_composer_focused`, `close_pages`, `session_project_changed` (the
     tab group check in `onCwdChange`), `forget_dirty_files`,
     `remove_project_terminals`, `project_location_changed` (maps to
     `Workspace::project_location_changed`), `project_sidebar_tab_removed`,
     `project_sidebar_tab_moved`.
   - History: `patch_summaries`, `refresh_history`, `rebase_loaded_project`,
     `rebase_session_folder_settings`, `place_session_in_folder`
     (history/session_folders.rs has the functions).
   - Submit: `rebase_ci_repairs`, `build_deterministic_handoff`, and
     `append_ready_handoff` (submit/handoff.rs has both; call
     `build_deterministic_handoff(session, None, None)`). Without them a
     session whose worktree was deleted moves without the recap block.
   - Orchestration: `orchestration_running` (`forSession` is active or
     paused).
   - Remote: `remote_session_for`.
   - The harness bridge: `model_catalog` and `harness_availability`, for the
     sessions this package creates. The defaults are the bundled catalog and
     an empty probe.
2. `Sessions` has no way to drop one id from the debounced save queue
   (`pendingPersist.current.delete(id)`). The flows rely on
   `begin_worktree_switch` and `begin_removal`, which make the debounced
   save skip the session; a `Sessions::cancel_pending_persist(id)` would
   match the TypeScript exactly.
3. `Sessions` does not expose the loads in progress (`sessionLoads`).
   Deleting a project invalidates the closed-session cache only.
4. A shared `Kv`. `ProjectsConfig` takes one, as attention's config does.

## Workspace and the app

5. The workspace owns the `notifyGitChanged` signal (`Files::git`) and
   `notifyDirsChanged`. Forward both to `GitStatuses::git_changed` and
   `GitStatuses::dirs_changed`, and forward window focus and visibility to
   `GitStatuses::window_focused` and `GitStatuses::set_hidden`. This
   package's own git changes go through `projects::notify_git_changed`,
   which calls the workspace hook and the registry; calls in one effect
   cycle reload once.
6. The workspace's `WorkspaceDelegate::remember_project` should call
   `actions::remember_project`.
7. The workspace keeps its own `TabGroupAppearance` for title-bar groups.
   `Projects` holds one too, for the rail. Both write the same keys, and
   `Projects` reloads on any `Kv` change to them, but the logo display
   revision is per instance. One owner would be simpler.
8. `ProjectsGlobal::init_native` needs the `SessionStore` (for the session
   ids each worktree holds) and the "in use" checks the Tauri commands made
   with `PtyHost::has_working_dir` and `HarnessHost::has_working_dir`.

## Git crate

9. `monocode_git::fs::ProjectLocation` and `worktrees::WorktreeRemoval`
   have private fields and only derive `Serialize`; the backend reads them
   back through JSON. Public fields would remove that step.
