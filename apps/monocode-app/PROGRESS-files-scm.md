Restart note for the files and source control slice (helper A).
Delete before committing.

Files: src/adapters/files.rs, src/adapters/scm.rs, src/panes/explorer.rs,
src/panes/changes.rs.

Done
- Read view-files, view-scm, engine Files, projects GitStatuses, App.tsx
  and Sidebar.tsx wiring.

In progress
- adapters/files.rs: AppFiles over Files with a LocalFiles fallback.

Next
- adapters/scm.rs: app_scm global, hooks, diff_surface, worktrees_slot.
- panes/explorer.rs, panes/changes.rs.
- check, clippy, tests, screenshots of tab-explorer and tab-changes.

Notes for the report
- Boot should forward Files.git and Files.tree DirsChanged to GitStatuses.
- The app should handle SubmitEvent::AddToChat with Workspace::add_to_chat.
- ProjectSearch (the explorer's search button) has no port yet.
