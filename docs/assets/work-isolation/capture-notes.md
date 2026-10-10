# UI capture provenance

These PNGs are browser captures of the application’s React components and CSS,
using Playwright and installed Chromium. They use generic deterministic Tauri IPC
fixtures; they are **not native desktop end-to-end screenshots or proof of a
successful Git publication**. No personal chats, credentials, or real project
paths appear in the captures.

The before screenshots load the original component source from `origin/main`
commit `00d68d3`. The after screenshots load the current feature implementation
following its merge with that main revision.

| Asset | Production surface shown | Fixture state |
| --- | --- | --- |
| `isolation-before.png` | `WorkspacePicker` before the feature | Local checkout; existing worktree available |
| `isolation-after.png` | `WorkspacePicker` after the feature | APFS capability supported; CoW option enabled |
| `settings-before.png` | `SettingsView`, `SettingsNav`, and `WorktreesPage` before the feature | One existing worktree |
| `settings-after.png` | The same settings components after the feature | CoW default selected; one worktree and one CoW workspace |
| `cleanup-dialog.png` | The existing `DeleteWorktreeDialog` used for CoW | One retained session, dirty files, one unpublished commit |
| `shared-git-publish.png` | The existing `GitChangesPanel` and `WorkspaceIdentity` | Dirty feature branch without an upstream; Publish Branch enabled |
| `shared-git-sync-pr.png` | The same Git panel and workspace indication | Clean feature branch with an upstream; Sync Changes and Create PR enabled |
| `shared-git-actions.png` | The same Git panel’s branch actions | Pull available for the branch with an upstream |

The picker captures place the production control in a minimal composer shell;
the Git captures mount the production panel directly rather than reproducing an
entire session. The fixture layer supplies APFS capability, branches, history,
workspace listings, and Git status. It does not execute filesystem or Git
operations. The screenshots therefore illustrate UI behavior; the native and
host integration tests provide the separate evidence for isolation and Git
operations.

The diagrams in this directory are implementation documentation, rather than
runtime captures. Their Mermaid sources and SVG exports describe the APFS-only
isolation lifecycle and the reuse of existing session, Git, and cleanup flows.
