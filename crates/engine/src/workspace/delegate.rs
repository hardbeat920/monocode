//! Calls the workspace makes into code it does not own: dialogs and the
//! window (the app), session history (the `history` package), and remote
//! shells (the `remote` package). Every method has a default that does
//! nothing, so the workspace runs in tests and in the headless host.

use gpui::{App, Task};
use monocode_core::{HarnessId, Session};

/// The cached summary of a remote session, for tab titles
/// (`cachedRemoteSessionSummary`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSummary {
    pub title: String,
    pub harness: HarnessId,
}

/// What the workspace asks of the app and the other packages.
pub trait WorkspaceDelegate {
    /// `ask` from the dialog plugin: a warning dialog with an OK and a
    /// Cancel button. The default accepts.
    fn confirm(&self, _message: &str, _ok_label: &str, _cx: &mut App) -> Task<bool> {
        Task::ready(true)
    }

    /// `hide_window`.
    fn hide_window(&self, _cx: &mut App) {}

    /// `destroy_window`.
    fn close_window(&self, _cx: &mut App) {}

    /// `refreshHistory(cwd)`: reload the session sidebar for a project.
    fn refresh_history(&self, _cwd: &str, _cx: &mut App) {}

    /// `rememberRemoteSession(id)` and `rememberRemotePendingWorktree(id)`
    /// for a pane that is closing, so reopening it finds the remote session.
    fn remember_remote_session(&self, _shell_id: &str, _cx: &mut App) {}

    /// `remoteSessionFor`: the remote session a local shell pane shows.
    fn remote_session_for(&self, _shell_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `remotePendingWorktree`: a remote worktree is being created for the
    /// pane.
    fn remote_pending_worktree(&self, _shell_id: &str, _cx: &App) -> bool {
        false
    }

    /// `cachedRemoteSessionSummary` for a remote project's shell pane.
    fn remote_summary(&self, _session: &Session, _cx: &App) -> Option<RemoteSummary> {
        None
    }

    /// `setRecents(rememberProject(path))`: the user moved to a project.
    fn remember_project(&self, _path: &str, _cx: &mut App) {}
}

/// The default delegate.
pub struct NoDelegate;

impl WorkspaceDelegate for NoDelegate {}
