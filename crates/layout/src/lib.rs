//! Workspace layout model. Port of src/features/workspace/model and the pure
//! terminal layout helpers in src/features/terminal/model.
//!
//! Pure data and functions: no GPUI and no IO. The workspace snapshot types
//! serialize to the same JSON the TypeScript stored through
//! `workspace_set_snapshot`, so the native app restores the same snapshots.

pub mod ids;
mod js;
pub mod layout;
pub mod pane_drop;
pub mod paths;
pub mod project_return;
pub mod project_terminal;
pub mod session_ref;
pub mod tab_groups;
pub mod tab_keys;
pub mod tab_visit_history;
pub mod terminal_chrome;
pub mod terminal_close;
pub mod terminal_layout;
pub mod terminal_tab;
pub mod workspace_snapshot;
pub mod workspace_tab_groups;

pub use layout::*;
pub use project_return::{ProjectReturnDecision, ProjectReturnMemory};
pub use project_terminal::{DockSide, ProjectTerminalDock};
pub use session_ref::SessionRef;
pub use workspace_snapshot::{
    ResumedWorkspace, WorkspaceSessionStub, WorkspaceSnapshot, collect_workspace_snapshot,
    hydrate_workspace_snapshot, parse_workspace_snapshot,
};
