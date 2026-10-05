//! Calls the workspace makes into code it does not own: dialogs and the
//! window (the app), session history (the `history` package), remote
//! shells (the `remote` package), and working copies (the `projects`
//! package). Every method has a default that does nothing, so the workspace
//! runs in tests and in the headless host.

use std::rc::Rc;

use gpui::{App, Task};
use monocode_core::{HarnessId, Session};

/// Whether a workspace switch still applies. A move started for it reads
/// this between its steps and stops without an error once it is false.
pub type IsCurrent = Rc<dyn Fn(&App) -> bool>;

/// The working copy a workspace switch moves a blank session to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeTarget {
    pub path: String,
    pub branch: Option<String>,
    /// The project folder itself rather than a linked worktree.
    pub is_main: bool,
}

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

    /// The encoded remote path of a pane's cached or pending host checkout.
    fn remote_working_cwd(&self, _project: &str, _shell_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `cachedRemoteSessionSummary` for a remote project's shell pane.
    fn remote_summary(&self, _session: &Session, _cx: &App) -> Option<RemoteSummary> {
        None
    }

    /// `setRecents(rememberProject(path))`: the user moved to a project.
    fn remember_project(&self, _path: &str, _cx: &mut App) {}

    /// `onWorktreeChange(id, tree, false, isCurrent)`: move a blank session
    /// into another working copy for a workspace switch.
    fn move_session_to_worktree(
        &self,
        _session_id: &str,
        _target: WorktreeTarget,
        _is_current: IsCurrent,
        _cx: &mut App,
    ) -> Task<Result<(), String>> {
        Task::ready(Err("Working copies are not available.".into()))
    }
}

/// The default delegate.
pub struct NoDelegate;

impl WorkspaceDelegate for NoDelegate {}
