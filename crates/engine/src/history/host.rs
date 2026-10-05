//! Calls the history package makes into other packages and the app.
//!
//! `runtime::hooks` has nothing yet for the calls below, so history defines
//! them here with no-op defaults, the way the attention and submit packages
//! do (see NEEDS.md). The app fills them in with `History::set_host`. When
//! the runtime grows the same methods, this trait can go.

use std::collections::HashSet;

use gpui::{App, Task};
use monocode_core::{HarnessId, RuntimeMode, Session};
use monocode_layout::layout::WorkspaceTab;
use monocode_layout::workspace_tab_groups::WorkspaceTabCloseScope;

use super::session_removal::{DeleteSession, ReplacementSeed, SessionRemovalMode};
use super::session_workspace_lifecycle::SessionWorkspaceRemoval;
use crate::runtime::session_history::LiveRun;

/// The tabs part of the workspace that removal and navigation read.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceTabs {
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_id: String,
    /// Ids of file tabs with unsaved edits.
    pub dirty_files: HashSet<String>,
}

/// The focused pane of the active tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivePane {
    pub tab_id: String,
    pub focused_id: String,
    pub diff_focused: bool,
}

/// The kind of a message dialog (`message(.., { kind })`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Error,
    Warning,
}

/// `SessionDeleteChoice`: the answer of the delete dialog that offers to
/// remove an unused worktree too.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionDeleteChoice {
    pub confirmed: bool,
    pub delete_worktree: bool,
}

/// Everything history asks of the workspace, the orchestrator, dialogs,
/// worktrees, remote hosts, and the model catalog.
pub trait HistoryHost {
    // Sessions.

    /// `newSession(harness, cwd, model, runtimeMode, modelSettings)` with the
    /// model catalog and preferences. The default makes a blank chat with
    /// the seed's values.
    fn new_session(&self, seed: &ReplacementSeed, _cx: &App) -> Session {
        let mut session = Session::blank(
            uuid::Uuid::new_v4().to_string(),
            seed.harness.unwrap_or(HarnessId::Cursor),
            seed.model.clone().unwrap_or_default(),
            &seed.cwd,
        );
        if let Some(mode) = seed.runtime_mode {
            session.runtime_mode = mode;
        }
        if let Some(settings) = seed.model_settings.clone() {
            session.model_settings = settings;
        }
        session
    }

    /// `newDefaultSession(cwd, runtimeMode)`: a chat with the Providers
    /// defaults.
    fn new_default_session(
        &self,
        cwd: &str,
        runtime_mode: Option<RuntimeMode>,
        cx: &App,
    ) -> Session {
        self.new_session(
            &ReplacementSeed {
                cwd: cwd.to_string(),
                runtime_mode,
                ..ReplacementSeed::default()
            },
            cx,
        )
    }

    // Workspace.

    /// Tabs, the active tab, and dirty files (`tabsRef`, `activeTabIdRef`,
    /// `dirtyFilesRef`).
    fn workspace_tabs(&self, _cx: &App) -> WorkspaceTabs {
        WorkspaceTabs::default()
    }

    /// `tabCloseScope`.
    fn tab_close_scope(&self, _cx: &App) -> WorkspaceTabCloseScope {
        WorkspaceTabCloseScope::Project
    }

    /// The tab half of a finished removal: set the tabs, drop the closing
    /// files from the dirty set, activate `removal.active_tab_id` when it
    /// changed, and focus the composer when the active tab shows a chat.
    /// History has already updated `Sessions`.
    fn commit_removal(&self, _removal: &SessionWorkspaceRemoval, _cx: &mut App) {}

    /// The `confirm` adapter of removal: ask before discarding unsaved files
    /// or closing running terminals in `closed_tabs`.
    fn confirm_removal(
        &self,
        _closed_tabs: &[WorkspaceTab],
        _mode: SessionRemovalMode,
        _cx: &mut App,
    ) -> Task<bool> {
        Task::ready(true)
    }

    /// Show an open session: focus its pane (`focusOpenSession`), else put it
    /// in a blank pane (`replaceBlankPaneWithSession`), else open a new tab,
    /// activate it, and focus the composer.
    fn open_session(&self, _session: &Session, _cx: &mut App) {}

    /// The focused pane of the active tab.
    fn active_pane(&self, _cx: &App) -> Option<ActivePane> {
        None
    }

    /// `switchSessionInTab` for the active tab, then focus the composer.
    fn switch_session_in_tab(
        &self,
        _tab_id: &str,
        _focused_id: &str,
        _next_id: &str,
        _cx: &mut App,
    ) {
    }

    /// `setInspectedWorkerId`: open a worker's transcript beside its lead.
    fn inspect_worker(&self, _session_id: &str, _cx: &mut App) {}

    /// `revealLinkedSessionUpdate` when the inbox has an unseen update for
    /// this session.
    fn reveal_linked_update(&self, _session_id: &str, _cx: &mut App) {}

    /// The work item link of a session changed: close its linked work item
    /// panel (`setLinkedWorkItemPanels`).
    fn linked_work_item_changed(&self, _session_id: &str, _cx: &mut App) {}

    /// The workspace half of `onAddNoteToChat`: close the full pages, show
    /// the sessions sidebar tab of `cwd`, open `session_id` (already in
    /// `Sessions`) in a new tab, activate it, and focus the composer.
    fn open_note_chat(&self, _session_id: &str, _cwd: &str, _cx: &mut App) {}

    // Dialogs.

    /// `message(text, { title: "MonoCode", kind })`.
    fn alert(&self, _message: &str, _kind: AlertKind, _cx: &mut App) {}

    /// `window.confirm(message)`.
    fn confirm(&self, _message: &str, _cx: &mut App) -> Task<bool> {
        Task::ready(true)
    }

    /// The delete dialog that offers to remove an unused worktree too
    /// (`setSessionDeleteDialog`).
    fn choose_delete(
        &self,
        _title: &str,
        _unused_worktree: &str,
        _cx: &mut App,
    ) -> Task<SessionDeleteChoice> {
        Task::ready(SessionDeleteChoice {
            confirmed: true,
            delete_worktree: false,
        })
    }

    // Worktrees.

    /// The worktree a delete may offer to remove: `listWorktrees(cwd)`, the
    /// tree at `worktree_cwd`, not main, not locked, with a branch, and no
    /// session but this one (`worktreeSessionIds`). A failed lookup is `None`.
    fn unused_worktree(
        &self,
        _cwd: &str,
        _worktree_cwd: &str,
        _session_id: &str,
        _cx: &mut App,
    ) -> Task<Option<String>> {
        Task::ready(None)
    }

    /// `onRemoveWorktree(cwd, path, force)`.
    fn remove_worktree(
        &self,
        _cwd: &str,
        _path: &str,
        _force: bool,
        _cx: &mut App,
    ) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }

    // Orchestrator.

    /// `orchestrator.forSession(sessionId)?.leadId`.
    fn run_lead_for_session(&self, _session_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `orchestrator.forSession` then `stopRun` for an active or paused run.
    fn stop_active_run(&self, _session_id: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `orchestrator.deleteSession(sessionId, remove)`.
    fn delete_session(
        &self,
        _session_id: &str,
        remove: DeleteSession,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        remove(cx)
    }

    /// The loaded orchestration runs (`orchestrationRuns`).
    fn live_runs(&self, _cx: &App) -> Vec<LiveRun> {
        Vec::new()
    }

    // Handoff.

    /// `isPreparingHandoff` then `completeHandoff(session,
    /// buildDeterministicHandoff(session))`. `None` keeps the session.
    fn finish_preparing_handoff(&self, _session: &Session) -> Option<Session> {
        None
    }

    // Remote projects.

    /// `remoteSessionFor(shellId)`: the host session behind a local shell.
    fn remote_session_for(&self, _shell_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `rememberRemoteSession(shellId, hostId)` and focus the composer.
    fn remember_remote_session(&self, _shell_id: &str, _host_id: &str, _cx: &mut App) {}

    /// `onSelectRemoteSession(cwd, hostId)`.
    fn select_remote_session(&self, _cwd: &str, _host_id: &str, _cx: &mut App) {}
}

/// The default host: no workspace, no dialogs, no orchestrator.
pub struct NoHistoryHost;

impl HistoryHost for NoHistoryHost {}
