//! Worktree-scoped workspaces. Ports src/features/source-control/model/
//! worktreeFocus.ts, the workspace pins, tab stats, and saved-tab rule from
//! src/app/App.tsx, and the request planning of src/app/hooks/
//! useWorkspaceNavigation.ts. The `Workspace` entity owns the state and runs
//! the requests; this module holds the rules.
//!
//! Each local project shows one workspace at a time: its project folder, or
//! one of its linked worktrees (the focus). A tab belongs to the workspace
//! it was opened in (its pin); a tab without a pin belongs to the working
//! copy it runs in.

use std::collections::{HashMap, HashSet};

use monocode_core::Session;
use monocode_core::paths::path_key;
use monocode_layout::WorkspaceTab;
use monocode_layout::layout::leaf_ids;
use monocode_layout::paths::{is_remote_project_path, same_project_path};
use monocode_layout::project_return::is_blank_session;
use monocode_layout::session_ref::SessionRef;
use monocode_layout::workspace_tab_groups::{
    filter_tabs_for_project, workspace_tab_cwd, workspace_tab_worktree,
};

/// `WorktreeFocus`: the working copy a project's sidebar is narrowed to.
/// New sessions in that project start there too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeFocus {
    pub path: String,
    pub branch: Option<String>,
}

/// The focus of each project, by path key. Kept for this run only: the app
/// reopens on each project's default workspace.
#[derive(Debug, Clone, Default)]
pub struct WorktreeFocuses(HashMap<String, WorktreeFocus>);

impl WorktreeFocuses {
    /// `worktreeFocus(project)`.
    pub fn get(&self, project: &str) -> Option<&WorktreeFocus> {
        self.0.get(&path_key(project))
    }

    /// `setWorktreeFocus`. Returns whether the focus changed.
    pub fn set(&mut self, project: &str, focus: Option<WorktreeFocus>) -> bool {
        let key = path_key(project);
        match focus {
            Some(focus) => self.0.insert(key, focus.clone()).as_ref() != Some(&focus),
            None => self.0.remove(&key).is_some(),
        }
    }

    /// `currentWorkspace`: the worktree a project's workspace shows.
    pub fn current_workspace(&self, project: &str) -> String {
        self.get(project)
            .map(|focus| focus.path.clone())
            .unwrap_or_else(|| project.to_string())
    }
}

/// `inWorktreeFocus`: every session matches when nothing is focused; else
/// only sessions whose working copy is the focused one.
pub fn in_worktree_focus(
    cwd: &str,
    worktree_cwd: Option<&str>,
    focus: Option<&WorktreeFocus>,
) -> bool {
    focus.is_none_or(|focus| {
        let work = worktree_cwd
            .filter(|worktree| !worktree.is_empty())
            .unwrap_or(cwd);
        path_key(work) == path_key(&focus.path)
    })
}

/// `workspacePins`: tab or session id to the workspace it was opened or
/// moved in. Opening a session never switches the workspace, so a tab stays
/// in the workspace it joined whatever worktree it runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspacePins(HashMap<String, String>);

impl WorkspacePins {
    /// `restoredPins`: only the default workspace's tabs were saved, so every
    /// restored tab of a local project belongs to it, including one its
    /// composer moved to a worktree.
    pub fn restored<S: SessionRef>(tabs: &[WorkspaceTab], sessions: &[S]) -> Self {
        Self(
            tabs.iter()
                .filter_map(|tab| {
                    let project = workspace_tab_cwd(tab, sessions)?;
                    (!is_remote_project_path(&project)).then(|| (tab.id.clone(), project))
                })
                .collect(),
        )
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.0.get(id).map(String::as_str)
    }

    /// Pin a tab or session. Returns whether the pin changed.
    pub fn set(&mut self, id: &str, workspace: &str) -> bool {
        if self.get(id) == Some(workspace) {
            return false;
        }
        self.0.insert(id.to_string(), workspace.to_string());
        true
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.0.remove(id).is_some()
    }

    /// `tabWorkspace`: the tab's pin, else a pin of one of its sessions,
    /// else the working copy it runs in. Unpinned tabs (restored ones) group
    /// by their own worktree.
    pub fn tab_workspace<S: SessionRef>(
        &self,
        tab: &WorkspaceTab,
        sessions: &[S],
    ) -> Option<String> {
        if let Some(pinned) = self.get(&tab.id) {
            return Some(pinned.to_string());
        }
        for id in leaf_ids(&tab.layout) {
            if let Some(pinned) = self.get(&id) {
                return Some(pinned.to_string());
            }
        }
        workspace_tab_worktree(tab, sessions)
    }

    /// `keepWorkspaceTab`: the saved workspace keeps a local project's tab
    /// only when it belongs to the project's default workspace. Tabs from
    /// other worktrees close with the app instead of piling up.
    pub fn keep_saved_tab<S: SessionRef>(&self, tab: &WorkspaceTab, sessions: &[S]) -> bool {
        let Some(project) = workspace_tab_cwd(tab, sessions) else {
            return true;
        };
        if is_remote_project_path(&project) {
            return true;
        }
        self.tab_workspace(tab, sessions)
            .is_none_or(|workspace| same_project_path(&workspace, &project))
    }

    /// The tabs of `project` whose workspace is `worktree`, plus tabs with
    /// no working copy.
    pub fn scoped_tabs<S: SessionRef>(
        &self,
        tabs: &[WorkspaceTab],
        sessions: &[S],
        project: &str,
        worktree: &str,
    ) -> Vec<WorkspaceTab> {
        filter_tabs_for_project(tabs, sessions, project)
            .into_iter()
            .filter(|tab| {
                self.tab_workspace(tab, sessions)
                    .is_none_or(|workspace| same_project_path(&workspace, worktree))
            })
            .collect()
    }
}

/// Open tabs of one workspace, for the worktree switcher.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorktreeTabStats {
    pub tabs: usize,
    /// One of them runs a turn.
    pub busy: bool,
}

/// `worktreeTabStats`: open tabs per workspace of `project`, keyed by the
/// worktree's path key, so the switcher can show what each worktree still
/// has open. Empty for home and remote projects.
pub fn worktree_tab_stats(
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    project: &str,
    pins: &WorkspacePins,
) -> HashMap<String, WorktreeTabStats> {
    let mut stats: HashMap<String, WorktreeTabStats> = HashMap::new();
    if project.is_empty() || project == "~" || is_remote_project_path(project) {
        return stats;
    }
    for tab in filter_tabs_for_project(tabs, sessions, project) {
        let workspace = pins
            .tab_workspace(&tab, sessions)
            .unwrap_or_else(|| project.to_string());
        let entry = stats.entry(path_key(&workspace)).or_default();
        entry.tabs += 1;
        entry.busy |= leaf_ids(&tab.layout).iter().any(|id| {
            sessions
                .iter()
                .any(|session| session.id == *id && session.is_busy())
        });
    }
    stats
}

/// `workspaceKey`: one workspace of one project, for the remembered tabs.
pub fn workspace_key(project: &str, path: &str) -> String {
    format!("{}\0{}", path_key(project), path_key(path))
}

/// Why a workspace request was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationKind {
    /// The project rail went back to a project: restore its workspace.
    Project,
    /// The switcher picked a workspace.
    Workspace,
}

/// A workspace switch in progress (`Request`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationRequest {
    pub kind: NavigationKind,
    pub project: String,
    pub focus: Option<WorktreeFocus>,
    /// Tells a request from the one it replaced.
    pub generation: u64,
}

impl NavigationRequest {
    /// The worktree the request shows.
    pub fn path(&self) -> &str {
        self.focus
            .as_ref()
            .map(|focus| focus.path.as_str())
            .unwrap_or(&self.project)
    }
}

/// A failed switch, shown in the switcher of its project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationError {
    pub project: String,
    pub message: String,
}

/// The state of `useWorkspaceNavigation`.
#[derive(Debug, Default)]
pub struct WorkspaceNavigation {
    /// The latest request; a newer one replaces it (`request.current`).
    pub request: Option<NavigationRequest>,
    pub generation: u64,
    /// The request loop is running (`running.current`).
    pub running: bool,
    /// Blank sessions a request is moving. Cleared when the loop ends, so a
    /// cancelled move still blocks submission until it finishes.
    pub moving: HashSet<String>,
    /// The tab last used in each workspace, by `workspace_key`.
    pub memory: HashMap<String, String>,
    pub error: Option<NavigationError>,
    /// The active tab and its focused pane when the request was made or
    /// last shown a tab. Any other activation cancels the request, as every
    /// ordinary `setActiveTabId` did.
    pub anchor: Option<(String, String)>,
    /// The project, active tab, and focused pane last seen, for the rule
    /// that a session opened directly joins the workspace on screen.
    pub seen: Option<(String, String, String)>,
}

impl WorkspaceNavigation {
    /// Replace any request with a new one.
    pub fn select(
        &mut self,
        kind: NavigationKind,
        project: &str,
        focus: Option<WorktreeFocus>,
    ) -> Option<NavigationRequest> {
        if project.is_empty() || project == "~" || is_remote_project_path(project) {
            return None;
        }
        self.generation += 1;
        let request = NavigationRequest {
            kind,
            project: project.to_string(),
            focus,
            generation: self.generation,
        };
        self.request = Some(request.clone());
        self.error = None;
        Some(request)
    }

    /// `cancel`. Returns whether anything changed.
    pub fn cancel(&mut self) -> bool {
        let changed = self.request.is_some() || self.error.is_some();
        self.request = None;
        self.error = None;
        self.anchor = None;
        changed
    }

    /// The request is done; a newer one stays.
    pub fn finish(&mut self, request: &NavigationRequest) {
        if self
            .request
            .as_ref()
            .is_some_and(|current| current.generation == request.generation)
        {
            self.request = None;
            self.anchor = None;
        }
    }

    pub fn is_latest(&self, request: &NavigationRequest) -> bool {
        self.request
            .as_ref()
            .is_some_and(|current| current.generation == request.generation)
    }
}

/// What a request does next (one pass of the `drain` loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationStep {
    /// Its project or landing tab is not on screen yet.
    Wait,
    /// The active tab is grouped under another project (a split with panes
    /// from two projects). Record the focus and leave the tab alone.
    FocusOnly,
    /// Show this tab: the active one when it is already in the workspace,
    /// else the tab last used there.
    Show(String),
    /// Carry the active tab's blank session over to the workspace.
    Move { session_id: String },
    /// Open a new session in the workspace.
    Create,
}

/// Decide the next step of `request` against what is on screen.
pub fn plan_navigation(
    request: &NavigationRequest,
    project_cwd: &str,
    tabs: &[WorkspaceTab],
    active_tab_id: &str,
    sessions: &[Session],
    pins: &WorkspacePins,
    memory: &HashMap<String, String>,
) -> NavigationStep {
    // A project request is recorded in the same event as `openProjects`.
    // Wait for its landing tab before choosing its workspace.
    if !same_project_path(project_cwd, &request.project) {
        return NavigationStep::Wait;
    }
    let Some(tab) = tabs.iter().find(|tab| tab.id == active_tab_id) else {
        return NavigationStep::Wait;
    };
    // Mixed-project split tabs keep their grouping when the focused pane's
    // project changes.
    if !workspace_tab_cwd(tab, sessions)
        .is_some_and(|cwd| same_project_path(&cwd, &request.project))
    {
        return NavigationStep::FocusOnly;
    }
    let path = request.path();
    let in_workspace = pins
        .tab_workspace(tab, sessions)
        .is_none_or(|workspace| same_project_path(&workspace, path));
    if in_workspace {
        return NavigationStep::Show(tab.id.clone());
    }
    let scoped = pins.scoped_tabs(tabs, sessions, &request.project, path);
    let remembered = memory.get(&workspace_key(&request.project, path));
    if let Some(target) = scoped
        .iter()
        .find(|entry| Some(&entry.id) == remembered)
        .or(scoped.last())
    {
        return NavigationStep::Show(target.id.clone());
    }
    let session = sessions.iter().find(|entry| entry.id == tab.focused_id);
    if let Some(session) = session
        && is_blank_session(Some(session))
        && same_project_path(&session.cwd, &request.project)
    {
        return NavigationStep::Move {
            session_id: session.id.clone(),
        };
    }
    if request.kind == NavigationKind::Project {
        // Returning through the project rail keeps its landing conversation
        // when the remembered workspace has no open tab. Its checkout stays
        // attached to that conversation.
        return NavigationStep::Show(tab.id.clone());
    }
    NavigationStep::Create
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::block::{Block, BlockRole};
    use monocode_layout::layout::new_tab;

    fn chat(id: &str, cwd: &str, worktree: Option<&str>, started: bool) -> Session {
        let mut session = Session::blank(id, HarnessId::Codex, "", cwd);
        session.worktree_cwd = worktree.map(str::to_string);
        if started {
            session.blocks = vec![Block::new(format!("p-{id}"), BlockRole::User, "hello")];
        }
        session
    }

    fn tab(id: &str) -> WorkspaceTab {
        WorkspaceTab {
            id: format!("tab-{id}"),
            ..new_tab(id)
        }
    }

    fn focus(path: &str, branch: &str) -> WorktreeFocus {
        WorktreeFocus {
            path: path.into(),
            branch: Some(branch.into()),
        }
    }

    // worktreeFocus.test.ts

    #[test]
    fn keeps_every_session_when_nothing_is_focused() {
        assert!(in_worktree_focus("/repo", None, None));
        assert!(in_worktree_focus("/repo", Some("/trees/a"), None));
    }

    #[test]
    fn matches_sessions_by_their_working_copy() {
        let main = focus("/repo", "main");
        let feature = focus("/trees/a", "feat/a");
        assert!(in_worktree_focus("/repo", None, Some(&main)));
        assert!(!in_worktree_focus("/repo", Some("/trees/a"), Some(&main)));
        assert!(in_worktree_focus("/repo", Some("/trees/a"), Some(&feature)));
        assert!(!in_worktree_focus("/repo", None, Some(&feature)));
        assert!(in_worktree_focus("/repo", Some(""), Some(&main)));
    }

    #[test]
    fn stores_focus_per_project() {
        let mut focuses = WorktreeFocuses::default();
        assert!(focuses.set("/repo", Some(focus("/trees/a", "feat/a"))));
        assert!(!focuses.set("/repo/", Some(focus("/trees/a", "feat/a"))));
        assert_eq!(
            focuses.get("/repo").map(|focus| focus.path.as_str()),
            Some("/trees/a")
        );
        assert!(focuses.get("/other").is_none());
        assert_eq!(focuses.current_workspace("/repo"), "/trees/a");
        assert_eq!(focuses.current_workspace("/other"), "/other");
        assert!(focuses.set("/repo", None));
        assert!(focuses.get("/repo").is_none());
    }

    // The pins and the saved tabs (App.tsx).

    #[test]
    fn pins_win_over_the_working_copy_and_restored_tabs_pin_to_their_project() {
        let sessions = vec![
            chat("main", "/repo", None, true),
            chat("feature", "/repo", Some("/trees/a"), true),
        ];
        let tabs = vec![tab("main"), tab("feature")];
        let mut pins = WorkspacePins::default();
        assert_eq!(
            pins.tab_workspace(&tabs[1], &sessions).as_deref(),
            Some("/trees/a")
        );
        assert!(!pins.keep_saved_tab(&tabs[1], &sessions));
        pins.set("feature", "/repo");
        assert_eq!(
            pins.tab_workspace(&tabs[1], &sessions).as_deref(),
            Some("/repo")
        );
        assert!(pins.keep_saved_tab(&tabs[1], &sessions));
        pins.set("tab-feature", "/trees/b");
        assert_eq!(
            pins.tab_workspace(&tabs[1], &sessions).as_deref(),
            Some("/trees/b")
        );

        let restored = WorkspacePins::restored(&tabs, &sessions);
        assert_eq!(restored.get("tab-main"), Some("/repo"));
        assert_eq!(restored.get("tab-feature"), Some("/repo"));
    }

    #[test]
    fn counts_open_tabs_per_workspace() {
        let mut busy = chat("feature", "/repo", Some("/trees/a"), true);
        busy.busy = Some(true);
        let sessions = vec![
            chat("main", "/repo", None, true),
            busy,
            chat("other", "/other", None, true),
        ];
        let tabs = vec![tab("main"), tab("feature"), tab("other")];
        let stats = worktree_tab_stats(&tabs, &sessions, "/repo", &WorkspacePins::default());
        assert_eq!(
            stats.get(&path_key("/repo")),
            Some(&WorktreeTabStats {
                tabs: 1,
                busy: false
            })
        );
        assert_eq!(
            stats.get(&path_key("/trees/a")),
            Some(&WorktreeTabStats {
                tabs: 1,
                busy: true
            })
        );
        assert_eq!(stats.len(), 2);
        assert!(worktree_tab_stats(&tabs, &sessions, "~", &WorkspacePins::default()).is_empty());
    }

    // The planning half of useWorkspaceNavigation.

    fn request(kind: NavigationKind, focus: Option<WorktreeFocus>) -> NavigationRequest {
        NavigationRequest {
            kind,
            project: "/repo".into(),
            focus,
            generation: 1,
        }
    }

    #[test]
    fn plans_each_way_to_reach_a_workspace() {
        let tree = Some(focus("/trees/a", "a"));
        let memory = HashMap::new();
        let pins = WorkspacePins::default();

        // The active tab is already there.
        let sessions = vec![chat("feature", "/repo", Some("/trees/a"), true)];
        let tabs = vec![tab("feature")];
        let workspace = request(NavigationKind::Workspace, tree.clone());
        assert_eq!(
            plan_navigation(
                &workspace,
                "/repo",
                &tabs,
                "tab-feature",
                &sessions,
                &pins,
                &memory
            ),
            NavigationStep::Show("tab-feature".into())
        );
        // Another project is on screen.
        assert_eq!(
            plan_navigation(
                &workspace,
                "/other",
                &tabs,
                "tab-feature",
                &sessions,
                &pins,
                &memory
            ),
            NavigationStep::Wait
        );

        // Another tab is there.
        let sessions = vec![
            chat("main", "/repo", None, true),
            chat("feature", "/repo", Some("/trees/a"), true),
        ];
        let tabs = vec![tab("main"), tab("feature")];
        assert_eq!(
            plan_navigation(
                &workspace, "/repo", &tabs, "tab-main", &sessions, &pins, &memory
            ),
            NavigationStep::Show("tab-feature".into())
        );

        // Nothing is there: a blank session moves, else a session opens.
        let sessions = vec![chat("blank", "/repo", None, false)];
        let tabs = vec![tab("blank")];
        assert_eq!(
            plan_navigation(
                &workspace,
                "/repo",
                &tabs,
                "tab-blank",
                &sessions,
                &pins,
                &memory
            ),
            NavigationStep::Move {
                session_id: "blank".into()
            }
        );
        let sessions = vec![chat("main", "/repo", None, true)];
        let tabs = vec![tab("main")];
        assert_eq!(
            plan_navigation(
                &workspace, "/repo", &tabs, "tab-main", &sessions, &pins, &memory
            ),
            NavigationStep::Create
        );
        // A project return keeps its landing conversation instead.
        let project = request(NavigationKind::Project, tree);
        assert_eq!(
            plan_navigation(
                &project, "/repo", &tabs, "tab-main", &sessions, &pins, &memory
            ),
            NavigationStep::Show("tab-main".into())
        );
    }

    #[test]
    fn returns_to_the_remembered_tab_of_a_workspace() {
        let sessions = vec![
            chat("main", "/repo", None, true),
            chat("first", "/repo", Some("/trees/a"), true),
            chat("second", "/repo", Some("/trees/a"), true),
        ];
        let tabs = vec![tab("main"), tab("first"), tab("second")];
        let workspace = request(NavigationKind::Workspace, Some(focus("/trees/a", "a")));
        let pins = WorkspacePins::default();
        let mut memory = HashMap::new();
        assert_eq!(
            plan_navigation(
                &workspace, "/repo", &tabs, "tab-main", &sessions, &pins, &memory
            ),
            NavigationStep::Show("tab-second".into())
        );
        memory.insert(workspace_key("/repo", "/trees/a"), "tab-first".to_string());
        assert_eq!(
            plan_navigation(
                &workspace, "/repo", &tabs, "tab-main", &sessions, &pins, &memory
            ),
            NavigationStep::Show("tab-first".into())
        );
    }

    #[test]
    fn a_new_request_replaces_the_last_and_a_stale_finish_keeps_it() {
        let mut navigation = WorkspaceNavigation::default();
        assert!(
            navigation
                .select(NavigationKind::Workspace, "~", None)
                .is_none()
        );
        assert!(
            navigation
                .select(NavigationKind::Workspace, "remote://host/srv", None)
                .is_none()
        );
        let first = navigation
            .select(NavigationKind::Workspace, "/repo", None)
            .unwrap();
        let second = navigation
            .select(
                NavigationKind::Workspace,
                "/repo",
                Some(focus("/trees/a", "a")),
            )
            .unwrap();
        assert!(!navigation.is_latest(&first));
        navigation.finish(&first);
        assert_eq!(navigation.request.as_ref(), Some(&second));
        navigation.finish(&second);
        assert!(navigation.request.is_none());
    }
}
