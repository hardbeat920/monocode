//! Test doubles for the projects package: a scriptable `ProjectsBackend`
//! that records every call (the TypeScript tests' `invoke` mock) and a
//! `ProjectsHooks` that plays the workspace, history, and other packages.
//!
//! Enable with the `test-support` feature from another crate.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use gpui::App;
use monocode_core::block::{Block, BlockRole, HandoffMeta, HandoffStatus};
use monocode_core::{HarnessId, Session};
use monocode_git::fs::{GitBranches, GitDiffIndex, GitDiffStats};
use monocode_layout::layout::{FilePaneTab, WorkspaceTab};
use monocode_layout::project_return::ProjectReturnMemory;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::backend::{ProjectLocation, ProjectsBackend, Worktree, WorktreeRemoval, Worktrees};
use super::hooks::{ProjectsHooks, SessionFolderTarget};
use crate::runtime::session_store::SessionSummary;

#[derive(Default)]
struct FakeState {
    calls: Vec<(String, Value)>,
    locations: VecDeque<Option<ProjectLocation>>,
    location_calls: Vec<(String, Option<String>)>,
    diff_index: HashMap<String, Result<GitDiffIndex, String>>,
    diff_stats: HashMap<String, Result<GitDiffStats, String>>,
    branches: HashMap<String, Result<GitBranches, String>>,
    worktrees: HashMap<String, Result<Worktrees, String>>,
    failing: HashMap<String, String>,
    removal: Option<WorktreeRemoval>,
    saved_path: Option<String>,
}

/// A `ProjectsBackend` that answers from scripted values.
#[derive(Default)]
pub struct FakeBackend {
    state: Mutex<FakeState>,
}

impl FakeBackend {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn record(&self, command: &str, args: Value) -> Result<(), String> {
        let mut state = self.state.lock();
        state.calls.push((command.to_string(), args));
        match state.failing.get(command) {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// Command names in call order.
    pub fn commands(&self) -> Vec<String> {
        self.state
            .lock()
            .calls
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// The arguments of every call to `command`.
    pub fn calls(&self, command: &str) -> Vec<Value> {
        self.state
            .lock()
            .calls
            .iter()
            .filter(|(name, _)| name == command)
            .map(|(_, args)| args.clone())
            .collect()
    }

    /// How many times `command` ran.
    pub fn count(&self, command: &str) -> usize {
        self.calls(command).len()
    }

    pub fn clear_calls(&self) {
        self.state.lock().calls.clear();
    }

    /// Make `command` fail with `error`, or succeed again with `None`.
    pub fn set_failing(&self, command: &str, error: Option<&str>) {
        let mut state = self.state.lock();
        match error {
            Some(error) => state.failing.insert(command.to_string(), error.to_string()),
            None => state.failing.remove(command),
        };
    }

    /// The next `resolve_project_location` answer.
    pub fn push_location(&self, location: Option<ProjectLocation>) {
        self.state.lock().locations.push_back(location);
    }

    /// Every `resolve_project_location` call as `(path, identity)`.
    pub fn location_calls(&self) -> Vec<(String, Option<String>)> {
        self.state.lock().location_calls.clone()
    }

    pub fn set_diff_index(&self, cwd: &str, index: Result<GitDiffIndex, String>) {
        self.state.lock().diff_index.insert(cwd.to_string(), index);
    }

    pub fn set_diff_stats(&self, cwd: &str, stats: Result<GitDiffStats, String>) {
        self.state.lock().diff_stats.insert(cwd.to_string(), stats);
    }

    pub fn set_branches(&self, cwd: &str, branches: Result<GitBranches, String>) {
        self.state.lock().branches.insert(cwd.to_string(), branches);
    }

    pub fn set_worktrees(&self, cwd: &str, worktrees: Result<Worktrees, String>) {
        self.state
            .lock()
            .worktrees
            .insert(cwd.to_string(), worktrees);
    }

    /// What `git_worktree_remove` returns.
    pub fn set_removal(&self, removal: WorktreeRemoval) {
        self.state.lock().removal = Some(removal);
    }

    /// What the image save commands return.
    pub fn set_saved_path(&self, path: &str) {
        self.state.lock().saved_path = Some(path.to_string());
    }

    fn saved_path(&self) -> String {
        self.state
            .lock()
            .saved_path
            .clone()
            .unwrap_or_else(|| "/app-data/saved.png".into())
    }
}

impl ProjectsBackend for FakeBackend {
    fn git_diff_index(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        self.record("git_diff_index", json!({ "cwd": cwd }))?;
        self.state
            .lock()
            .diff_index
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| Ok(GitDiffIndex::default()))
    }

    fn git_diff_stats(&self, cwd: &str) -> Result<GitDiffStats, String> {
        self.record("git_diff_stats", json!({ "cwd": cwd }))?;
        self.state
            .lock()
            .diff_stats
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| Ok(GitDiffStats::default()))
    }

    fn git_branches(&self, cwd: &str) -> Result<GitBranches, String> {
        self.record("git_branches", json!({ "cwd": cwd }))?;
        self.state
            .lock()
            .branches
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| Ok(GitBranches::default()))
    }

    fn git_worktrees(&self, cwd: &str) -> Result<Worktrees, String> {
        self.record("git_worktrees", json!({ "cwd": cwd }))?;
        self.state
            .lock()
            .worktrees
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| Ok(Worktrees::default()))
    }

    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String> {
        self.record(
            "git_worktree_create",
            json!({ "cwd": cwd, "branch": branch, "base": base, "existing": existing }),
        )?;
        Ok(Worktree::new(
            format!("{cwd}-worktrees/{branch}"),
            Some(branch),
        ))
    }

    fn git_orchestration_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        self.record(
            "git_orchestration_worktree_create",
            json!({ "cwd": cwd, "branch": branch }),
        )?;
        Ok(Worktree::new(
            format!("{cwd}-worktrees/{branch}"),
            Some(branch),
        ))
    }

    fn git_worktree_rename_branch(
        &self,
        cwd: &str,
        path: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        self.record(
            "git_worktree_rename_branch",
            json!({ "cwd": cwd, "path": path, "branch": branch }),
        )?;
        Ok(Worktree::new(path, Some(branch)))
    }

    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        self.record(
            "git_worktree_check_remove",
            json!({ "cwd": cwd, "path": path, "force": force }),
        )
    }

    fn git_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
        force: bool,
        keep_sessions: bool,
    ) -> Result<WorktreeRemoval, String> {
        self.record(
            "git_worktree_remove",
            json!({ "cwd": cwd, "path": path, "force": force, "keepSessions": keep_sessions }),
        )?;
        Ok(self
            .state
            .lock()
            .removal
            .clone()
            .unwrap_or(WorktreeRemoval {
                session_ids: Vec::new(),
                project_cwd: cwd.to_string(),
            }))
    }

    fn git_orchestration_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
    ) -> Result<WorktreeRemoval, String> {
        self.record(
            "git_orchestration_worktree_remove",
            json!({ "cwd": cwd, "path": path }),
        )?;
        Ok(WorktreeRemoval {
            session_ids: Vec::new(),
            project_cwd: cwd.to_string(),
        })
    }

    fn git_orchestration_branch_remove(&self, cwd: &str, branch: &str) -> Result<(), String> {
        self.record(
            "git_orchestration_branch_remove",
            json!({ "cwd": cwd, "branch": branch }),
        )
    }

    fn resolve_project_location(
        &self,
        path: &str,
        identity: Option<&str>,
    ) -> Result<Option<ProjectLocation>, String> {
        self.record(
            "resolve_project_location",
            json!({ "path": path, "identity": identity }),
        )?;
        let mut state = self.state.lock();
        state
            .location_calls
            .push((path.to_string(), identity.map(str::to_string)));
        Ok(state.locations.pop_front().flatten())
    }

    fn save_project_logo(&self, project: &str, source_path: &str) -> Result<String, String> {
        self.record(
            "save_project_logo",
            json!({ "project": project, "sourcePath": source_path }),
        )?;
        Ok(self.saved_path())
    }

    fn forget_logo_file(&self, path: &str) -> Result<(), String> {
        self.record("forget_logo_file", json!({ "path": path }))
    }

    fn remove_project_logo(&self, project: &str) -> Result<(), String> {
        self.record("remove_project_logo", json!({ "project": project }))
    }

    fn save_chat_background(&self, source_path: &str) -> Result<String, String> {
        self.record("save_chat_background", json!({ "sourcePath": source_path }))?;
        Ok(self.saved_path())
    }

    fn remove_chat_background(&self) -> Result<(), String> {
        self.record("remove_chat_background", Value::Null)
    }

    fn save_project_chat_background(
        &self,
        project: &str,
        source_path: &str,
    ) -> Result<String, String> {
        self.record(
            "save_project_chat_background",
            json!({ "project": project, "sourcePath": source_path }),
        )?;
        Ok(self.saved_path())
    }

    fn remove_project_chat_background(&self, project: &str) -> Result<(), String> {
        self.record(
            "remove_project_chat_background",
            json!({ "project": project }),
        )
    }
}

/// What `TestHooks` saw, and the workspace state it plays.
#[derive(Default)]
pub struct TestHooksState {
    pub project_cwd: String,
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_id: String,
    pub memory: ProjectReturnMemory,
    pub open_files: Vec<FilePaneTab>,
    pub composer_focused: Option<bool>,
    pub pages_closed: usize,
    pub calls: Vec<String>,
    pub summaries: Vec<SessionSummary>,
    pub running_orchestration: Vec<String>,
    pub remote_sessions: HashMap<String, String>,
}

/// A `ProjectsHooks` that records calls and keeps a small workspace.
#[derive(Default)]
pub struct TestHooks {
    pub state: RefCell<TestHooksState>,
}

impl TestHooks {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            state: RefCell::new(TestHooksState {
                project_cwd: "~".into(),
                ..TestHooksState::default()
            }),
        })
    }

    fn log(&self, call: String) {
        self.state.borrow_mut().calls.push(call);
    }

    /// Every call as `name(args)`.
    pub fn calls(&self) -> Vec<String> {
        self.state.borrow().calls.clone()
    }

    pub fn project_cwd(&self) -> String {
        self.state.borrow().project_cwd.clone()
    }

    pub fn tabs(&self) -> Vec<WorkspaceTab> {
        self.state.borrow().tabs.clone()
    }

    pub fn active_tab_id(&self) -> String {
        self.state.borrow().active_tab_id.clone()
    }
}

impl ProjectsHooks for TestHooks {
    fn project_cwd(&self, _cx: &App) -> String {
        self.state.borrow().project_cwd.clone()
    }

    fn set_project_cwd(&self, cwd: &str, _cx: &mut App) {
        self.log(format!("set_project_cwd({cwd})"));
        self.state.borrow_mut().project_cwd = cwd.to_string();
    }

    fn tabs(&self, _cx: &App) -> Vec<WorkspaceTab> {
        self.state.borrow().tabs.clone()
    }

    fn active_tab_id(&self, _cx: &App) -> String {
        self.state.borrow().active_tab_id.clone()
    }

    fn project_return_memory(&self, _cx: &mut App) -> ProjectReturnMemory {
        self.state.borrow().memory.clone()
    }

    fn open_files(&self, _cx: &App) -> Vec<FilePaneTab> {
        self.state.borrow().open_files.clone()
    }

    fn append_tab(&self, tab: WorkspaceTab, cwd: Option<&str>, _cx: &mut App) {
        self.log(format!(
            "append_tab({}, {})",
            tab.focused_id,
            cwd.unwrap_or("")
        ));
        self.state.borrow_mut().tabs.push(tab);
    }

    fn insert_tab_beside(
        &self,
        tab: WorkspaceTab,
        anchor_id: Option<&str>,
        cwd: Option<&str>,
        _cx: &mut App,
    ) {
        self.log(format!(
            "insert_tab_beside({}, {}, {})",
            tab.focused_id,
            anchor_id.unwrap_or(""),
            cwd.unwrap_or("")
        ));
        let mut state = self.state.borrow_mut();
        let at = anchor_id
            .and_then(|anchor| state.tabs.iter().position(|entry| entry.id == anchor))
            .map(|index| index + 1)
            .unwrap_or(state.tabs.len());
        state.tabs.insert(at, tab);
    }

    fn set_tabs(&self, tabs: Vec<WorkspaceTab>, active_tab_id: &str, _cx: &mut App) {
        self.log(format!("set_tabs({}, {active_tab_id})", tabs.len()));
        let mut state = self.state.borrow_mut();
        state.tabs = tabs;
        state.active_tab_id = active_tab_id.to_string();
    }

    fn set_active_tab(&self, tab_id: &str, _cx: &mut App) {
        self.log(format!("set_active_tab({tab_id})"));
        self.state.borrow_mut().active_tab_id = tab_id.to_string();
    }

    fn activate_tab(&self, tab_id: &str, pane_id: Option<&str>, _cx: &mut App) {
        self.log(format!("activate_tab({tab_id}, {})", pane_id.unwrap_or("")));
        self.state.borrow_mut().active_tab_id = tab_id.to_string();
    }

    fn set_composer_focused(&self, focused: bool, _cx: &mut App) {
        self.state.borrow_mut().composer_focused = Some(focused);
    }

    fn close_pages(&self, _cx: &mut App) {
        self.state.borrow_mut().pages_closed += 1;
    }

    fn select_project_workspace(&self, path: &str, _cx: &mut App) {
        self.log(format!("select_project_workspace({path})"));
    }

    fn cancel_workspace_navigation(&self, _cx: &mut App) {
        self.log("cancel_workspace_navigation".into());
    }

    fn session_project_changed(&self, session_id: &str, cwd: &str, _cx: &mut App) {
        self.log(format!("session_project_changed({session_id}, {cwd})"));
    }

    fn forget_dirty_files(&self, file_ids: &[String], _cx: &mut App) {
        self.log(format!("forget_dirty_files({})", file_ids.join(",")));
    }

    fn remove_project_terminals(&self, path: &str, _cx: &mut App) {
        self.log(format!("remove_project_terminals({path})"));
    }

    fn project_location_changed(&self, from: &str, to: &str, _cx: &mut App) {
        self.log(format!("project_location_changed({from}, {to})"));
        let mut state = self.state.borrow_mut();
        if crate::projects::recents::same_project_path(&state.project_cwd, from) {
            state.project_cwd = to.to_string();
        }
    }

    fn project_sidebar_tab_removed(&self, path: &str, _cx: &mut App) {
        self.log(format!("project_sidebar_tab_removed({path})"));
    }

    fn project_sidebar_tab_moved(&self, from: &str, to: &str, _cx: &mut App) {
        self.log(format!("project_sidebar_tab_moved({from}, {to})"));
    }

    fn patch_summaries(
        &self,
        patch: &dyn Fn(&SessionSummary) -> Option<SessionSummary>,
        _cx: &mut App,
    ) {
        let mut state = self.state.borrow_mut();
        let next: Vec<SessionSummary> = state
            .summaries
            .iter()
            .map(|summary| patch(summary).unwrap_or_else(|| summary.clone()))
            .collect();
        state.summaries = next;
        state.calls.push("patch_summaries".into());
    }

    fn refresh_history(&self, cwd: &str, _cx: &mut App) {
        self.log(format!("refresh_history({cwd})"));
    }

    fn rebase_loaded_project(&self, from: &str, to: &str, _cx: &mut App) {
        self.log(format!("rebase_loaded_project({from}, {to})"));
    }

    fn rebase_session_folder_settings(&self, from: &str, to: &str, _cx: &mut App) {
        self.log(format!("rebase_session_folder_settings({from}, {to})"));
    }

    fn place_session_in_folder(
        &self,
        cwd: &str,
        session_id: &str,
        target: &SessionFolderTarget,
        _cx: &mut App,
    ) {
        self.log(format!(
            "place_session_in_folder({cwd}, {session_id}, {target:?})"
        ));
    }

    fn rebase_ci_repairs(&self, from: &str, to: &str, _cx: &mut App) {
        self.log(format!("rebase_ci_repairs({from}, {to})"));
    }

    fn build_deterministic_handoff(&self, session: &Session) -> String {
        session
            .blocks
            .iter()
            .map(|block| block.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn append_ready_handoff(
        &self,
        session: &Session,
        from: HarnessId,
        to: HarnessId,
        text: &str,
    ) -> Session {
        let mut next = session.clone();
        let mut block = Block::new(
            format!("handoff-{}", next.blocks.len()),
            BlockRole::Handoff,
            text,
        );
        block.handoff = Some(HandoffMeta {
            from,
            to,
            status: HandoffStatus::Ready,
            pending: Some(true),
            extra: Default::default(),
        });
        next.blocks.push(block);
        next
    }

    fn orchestration_running(&self, session_id: &str, _cx: &App) -> bool {
        self.state
            .borrow()
            .running_orchestration
            .iter()
            .any(|id| id == session_id)
    }

    fn remote_session_for(&self, session_id: &str, _cx: &App) -> Option<String> {
        self.state.borrow().remote_sessions.get(session_id).cloned()
    }
}
