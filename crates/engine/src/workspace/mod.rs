//! Engine package `workspace`: tabs, split panes, the terminal dock and
//! terminal tabs, open files, the file index and watchers, and the
//! workspace snapshot. Ports the stateful side of src/app/App.tsx for
//! those, src/features/files/model, src/features/projects/model/
//! projectTerminal.ts, and src/features/sessions/model/
//! sessionWorkspaceLifecycle.ts and addChatToWorkspace.ts, and the
//! worktree-scoped workspaces of useWorkspaceNavigation.ts and
//! worktreeFocus.ts. The pure layout model lives in `monocode_layout`.

pub mod add_chat;
pub mod chat_context;
pub mod delegate;
pub mod files;
pub mod hooks;
pub mod lifecycle;
pub mod paths;
pub mod session_factory;
pub mod terminals;
pub mod title_tab;
#[allow(clippy::module_inception)]
pub mod workspace;
#[cfg(test)]
mod workspace_tests;
pub mod worktree_scope;
#[cfg(test)]
mod worktree_scope_tests;

pub use delegate::{IsCurrent, NoDelegate, RemoteSummary, WorkspaceDelegate, WorktreeTarget};
pub use files::Files;
pub use hooks::{WorkspaceSetup, init};
pub use session_factory::{ModelEnvSessions, SessionFactory};
pub use terminals::{EnginePty, ProjectTerminals, Terminals};
pub use workspace::{Workspace, WorkspaceConfig, WorkspaceEvent};
pub use worktree_scope::{WorktreeFocus, WorktreeTabStats, in_worktree_focus};

use monocode_layout::{FilePaneTab, WorkspaceTab};

use crate::runtime::util::reorder::HasId;
use crate::runtime::window_transfer::TransferTab;

// `orderByIds` and `mergeOrderedSubset` reorder tabs and files by id.
impl HasId for FilePaneTab {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasId for WorkspaceTab {
    fn id(&self) -> &str {
        &self.id
    }
}

impl TransferTab for WorkspaceTab {
    fn id(&self) -> &str {
        &self.id
    }

    fn leaf_ids(&self) -> Vec<String> {
        monocode_layout::leaf_ids(&self.layout)
    }

    fn file_ids(&self) -> Vec<String> {
        self.editor_panes
            .iter()
            .chain(&self.terminal_panes)
            .flat_map(|pane| pane.files.iter().map(|file| file.id.clone()))
            .collect()
    }
}
