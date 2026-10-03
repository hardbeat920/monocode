//! Calls orchestration makes into packages it does not build on: the
//! window's workspace and the harness availability probe. Every method has a
//! default, and the app fills them in with `OrchestrationConfig::peers`.
//! NEEDS.md lists them.

use gpui::{App, Task};

use super::agent_app::{AppLaunch, AppSessionPlacement};

/// Workspace and window calls App.tsx made directly.
pub trait OrchestrationPeers {
    /// `launchQuickSession` in the window behind the control `owner`: create
    /// session `id` from `launch`, as a tab or a pane beside another session.
    fn launch_session(
        &self,
        _owner: &str,
        _launch: AppLaunch,
        _id: &str,
        _placement: Option<AppSessionPlacement>,
        _cx: &mut App,
    ) -> Task<Result<(), String>> {
        Task::ready(Err("No MonoCode window can start sessions.".into()))
    }

    /// `checkOpenWorktreeFiles`: close or flag open editors under a worktree
    /// that is about to be removed.
    fn check_open_worktree_files(&self, _path: &str, _cx: &mut App) {}

    /// `probeHarnessAvailability` before discovering worker models.
    fn probe_availability(&self, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }
}

/// The default peers: no window.
pub struct NoPeers;

impl OrchestrationPeers for NoPeers {}
