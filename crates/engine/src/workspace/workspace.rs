//! The `Workspace` entity: one window's tabs, split panes, terminal docks,
//! open files, and focus. Ports the state and callbacks of src/app/App.tsx
//! at lines 1608-1630 (running terminals), 2110-2199 (activating tabs and
//! the visit history), 2444-3673 (splits, terminals, closing tabs, panes,
//! and files, tab navigation, diffs, reordering), 3673-3780 and 4056-4085
//! (opening sessions), 4150-4244 (dropping sessions and tabs on panes),
//! 4891-4912 (focus by direction, split ratios), 5419-5691 (moved and
//! deleted files, opening files and plans, dirty and error state), and
//! 10370-10399 (the dock size).
//!
//! Sessions live in the runtime's `Sessions` entity; tabs live here. A
//! change to the tabs copies them into the hooks' `Mirror`, stops the PTYs
//! of closed terminal files, and asks `Sessions` to save the snapshot.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription, Task};
use monocode_core::paths::basename;
use monocode_core::plan::plan_title;
use monocode_core::session::session_work_cwd;
use monocode_core::settings::{DiffViewer, FileTabMode};
use monocode_core::{RuntimeMode, Session};
use monocode_layout::pane_drop::TitleTabDropPosition;
use monocode_layout::paths::{normalize_project_path, project_name, same_project_path};
use monocode_layout::project_return::{
    ProjectReturnMemory, is_blank_session, reconcile_project_return,
};
use monocode_layout::project_terminal::{DockSide, ProjectTerminalDock, Viewport};
use monocode_layout::session_ref::SessionRef;
use monocode_layout::tab_groups::{apply_grouped_reorder, insert_tab_beside_active};
use monocode_layout::tab_visit_history::{
    TabVisitHistory, can_tab_visit_back, can_tab_visit_forward, empty_tab_visit_history,
    prune_tab_visit_history, record_tab_visit, tab_visit_back, tab_visit_forward,
};
use monocode_layout::terminal_close::{close_terminals_prompt, running_terminals};
use monocode_layout::terminal_tab::{RunningTerminal, TerminalMetaPatch, list_running_terminals};
use monocode_layout::workspace_tab_groups::{
    PlaceSessionOnPane, WorkspaceTabClosePlan, WorkspaceTabCloseScope, apply_detach_pane_to_tab,
    apply_place_session_on_pane, apply_place_tab_on_pane, filter_tabs_for_project,
    find_open_session_tab, focused_workspace_tab_cwd, plan_workspace_tab_close, workspace_tab_cwd,
};
use monocode_layout::{
    CommitTabSource, EditorPane, EditorSplitSide, FilePaneTab, FocusDir, GitFileDiffKind,
    OpenEditorTabOptions, PaneEdge, PanePlace, SplitDir, SurfaceKind, WorkspaceTab, close_leaf,
    close_surface_panes, find_surface_pane, first_leaf_id, focused_file_tab, is_filesystem_tab,
    leaf_ids, move_pane, neighbor_leaf_id, new_editor_workspace_tab, new_file_tab, new_plan_tab,
    new_tab, new_terminal_file, new_terminal_workspace_tab, next_terminal_title, open_changes_tab,
    open_commit_tab, open_editor_tab, open_session_changes_tab, open_terminal_tab,
    open_workspace_file, pin_editor_file, remove_pane, replace_leaf_id, reset_tab_to_session,
    set_split_ratio, sibling_leaf_id, split_pane, surface_panes, update_terminal_tab,
    with_surface_panes,
};
use monocode_settings::Kv;
use monocode_settings::settings_store::{load_diff_viewer, load_file_tab_mode};

use super::add_chat::{AddToChatRequest, apply_add_to_chat_request};
use super::chat_context::ChatContextItem;
use super::delegate::{NoDelegate, WorkspaceDelegate};
use super::files::Files;
use super::hooks::{Mirror, WorkspacePackage, mirror_from_layout};
use super::lifecycle::{RemoveSession, SessionWorkspaceRemoval, remove_session_from_workspace};
use super::paths::{
    FileNavigation, is_equal_or_inside, is_local_project, looks_like_project, rebase_path,
};
use super::session_factory::{ModelEnvSessions, SessionFactory, session_seeded_from};
use super::terminals::{DockToggle, ProjectTerminals, TerminalMetaChanged, Terminals};
use super::title_tab::{
    TitleTab, drop_open_files, is_blank_workspace_tab, title_tab_project, to_title_tab,
};
use crate::runtime::in_flight::ResumedWorkspace;
use crate::runtime::util::reorder::{merge_ordered_subset, order_by_ids};
use crate::runtime::window_transfer::{WindowTransferPayload, collect_window_transfer};
use crate::runtime::{Engine, Sessions};

/// `tabCloseScope`: the app closes tabs one project at a time.
pub const TAB_CLOSE_SCOPE: WorkspaceTabCloseScope = WorkspaceTabCloseScope::Project;

/// The label of the confirm button on the close prompts (`ask`'s default).
const CONFIRM_LABEL: &str = "Yes";

/// `EditorNavigationTarget`: where an opened file should scroll to. The
/// token changes on every request, so the same target can apply twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorNavigationTarget {
    pub path: String,
    pub line: i64,
    pub column: Option<i64>,
    pub token: i64,
}

/// `FileOpenOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileOpenOptions {
    /// The caller got this concrete path from the file system or the index.
    pub exact: bool,
    /// Open as a permanent tab instead of the pane's preview tab.
    pub pin: bool,
}

/// A sidebar tab the workspace asks the shell to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Sessions,
    Changes,
}

/// What a `Workspace` tells its views beyond `cx.notify()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    /// `setSidebarTab(tab, project)`.
    ShowSidebarTab {
        tab: SidebarTab,
        project: Option<String>,
    },
    /// The focused editor should move to this position.
    EditorNavigation(EditorNavigationTarget),
}

/// The session a diff belongs to (`{ sessionId, cwd }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSession {
    pub session_id: String,
    pub cwd: String,
}

/// How a window's workspace starts.
pub struct WorkspaceConfig {
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_id: String,
    pub project_cwd: String,
    pub project_terminals: Vec<ProjectTerminalDock>,
    pub last_dock_side: Option<DockSide>,
    pub project_return: ProjectReturnMemory,
    pub dirty_file_ids: Vec<String>,
    pub composer_focused: bool,
    /// Save the workspace snapshot. False for a window a transfer opened.
    pub autosave: bool,
    /// Settings, for the file tab mode and the diff viewer.
    pub kv: Option<Kv>,
    pub sessions: Option<Rc<dyn SessionFactory>>,
    pub delegate: Option<Rc<dyn WorkspaceDelegate>>,
}

impl WorkspaceConfig {
    /// A new window: one tab with a new chat in the last project
    /// (`lastProjectPath() ?? "~"`). `Workspace::new` creates the chat.
    pub fn fresh(project_cwd: Option<&str>) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab_id: String::new(),
            project_cwd: project_cwd.unwrap_or("~").to_string(),
            project_terminals: Vec::new(),
            last_dock_side: None,
            project_return: ProjectReturnMemory::default(),
            dirty_file_ids: Vec::new(),
            composer_focused: false,
            autosave: true,
            kv: None,
            sessions: None,
            delegate: None,
        }
    }

    /// The workspace a boot restored. Its sessions are already in
    /// `Sessions` after `Lifecycle` adopts them. The composer starts focused
    /// when the active tab focuses a chat.
    pub fn resumed(resumed: &ResumedWorkspace) -> Self {
        let mut config = Self::fresh(Some(&resumed.project_cwd));
        if let Some(mirror) = mirror_from_layout(&resumed.layout, &resumed.project_cwd) {
            let tab = mirror
                .tabs
                .iter()
                .find(|tab| tab.id == mirror.active_tab_id)
                .or(mirror.tabs.first());
            config.composer_focused = tab.is_some_and(|tab| {
                resumed
                    .sessions
                    .iter()
                    .any(|session| session.id == tab.focused_id)
            });
            config.tabs = mirror.tabs;
            config.active_tab_id = mirror.active_tab_id;
            config.project_cwd = mirror.project_cwd;
            config.project_terminals = mirror.docks;
            config.last_dock_side = mirror.last_dock_side;
            config.project_return = mirror.project_return;
        }
        config
    }

    /// A window opened by moving tabs out of another (`windowTransfer`).
    pub fn transferred(
        tabs: Vec<WorkspaceTab>,
        active_tab_id: String,
        project_cwd: String,
        dirty_file_ids: Vec<String>,
        project_terminals: Vec<ProjectTerminalDock>,
    ) -> Self {
        Self {
            tabs,
            active_tab_id,
            project_cwd,
            project_terminals,
            dirty_file_ids,
            composer_focused: true,
            autosave: false,
            ..Self::fresh(None)
        }
    }
}

/// An open session as the layout helpers see it, plus the session itself
/// when the helper created it.
#[derive(Debug, Clone)]
struct OpenRef {
    id: String,
    cwd: String,
    created: Option<Box<Session>>,
}

impl SessionRef for OpenRef {
    fn session_id(&self) -> &str {
        &self.id
    }

    fn session_cwd(&self) -> &str {
        &self.cwd
    }
}

impl OpenRef {
    fn created(session: Session) -> Self {
        Self {
            id: session.id.clone(),
            cwd: session.cwd.clone(),
            created: Some(Box::new(session)),
        }
    }
}

fn sessions_entity(cx: &App) -> Option<Entity<Sessions>> {
    Engine::try_global(cx).map(|engine| engine.sessions.clone())
}

/// Every open session (`sessionsRef.current`).
fn all_sessions(cx: &App) -> Vec<Session> {
    sessions_entity(cx)
        .map(|sessions| sessions.read(cx).all().to_vec())
        .unwrap_or_default()
}

fn find_session(session_id: &str, cx: &App) -> Option<Session> {
    sessions_entity(cx).and_then(|sessions| sessions.read(cx).get(session_id).cloned())
}

fn has_session(session_id: &str, cx: &App) -> bool {
    sessions_entity(cx).is_some_and(|sessions| sessions.read(cx).contains(session_id))
}

fn open_refs(cx: &App) -> Vec<OpenRef> {
    sessions_entity(cx)
        .map(|sessions| {
            sessions
                .read(cx)
                .all()
                .iter()
                .map(|session| OpenRef {
                    id: session.id.clone(),
                    cwd: session.cwd.clone(),
                    created: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn update_sessions(cx: &mut App, update: impl FnOnce(&mut Sessions, &mut Context<Sessions>)) {
    if let Some(sessions) = sessions_entity(cx) {
        sessions.update(cx, update);
    }
}

fn pane_files(tab: &WorkspaceTab) -> Vec<FilePaneTab> {
    tab.editor_panes
        .iter()
        .chain(&tab.terminal_panes)
        .flat_map(|pane| pane.files.iter().cloned())
        .collect()
}

/// One window's workspace.
pub struct Workspace {
    tabs: Vec<WorkspaceTab>,
    active_tab_id: String,
    project_cwd: String,
    terminals: Entity<ProjectTerminals>,
    composer_focused: bool,
    dirty_files: HashSet<String>,
    file_error_counts: HashMap<String, i64>,
    tab_visit: TabVisitHistory,
    tab_visit_from_history: bool,
    project_return: ProjectReturnMemory,
    editor_navigation: Option<EditorNavigationTarget>,
    editor_navigation_token: i64,
    known_terminals: HashSet<String>,
    mirror: Rc<RefCell<Mirror>>,
    factory: Rc<dyn SessionFactory>,
    delegate: Rc<dyn WorkspaceDelegate>,
    kv: Option<Kv>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

impl Workspace {
    /// Build the workspace and register it with the hooks. A config with no
    /// tabs gets one tab with a new chat.
    pub fn new(config: WorkspaceConfig, cx: &mut Context<Self>) -> Self {
        let package = WorkspacePackage::try_global(cx);
        let factory = config
            .sessions
            .clone()
            .or_else(|| package.map(|package| package.sessions.clone()))
            .unwrap_or_else(|| Rc::new(ModelEnvSessions::default()));
        let delegate = config
            .delegate
            .clone()
            .or_else(|| package.map(|package| package.delegate.clone()))
            .unwrap_or_else(|| Rc::new(NoDelegate));

        let mut tabs = config.tabs;
        let mut active_tab_id = config.active_tab_id;
        if tabs.is_empty() {
            let session = factory.new_default_session(&config.project_cwd, None);
            let tab = new_tab(&session.id);
            active_tab_id = tab.id.clone();
            tabs.push(tab);
            update_sessions(cx, |sessions, cx| {
                sessions.insert(session, cx);
            });
        }

        let terminals =
            cx.new(|_| ProjectTerminals::new(config.project_terminals, config.last_dock_side));
        let mut subscriptions = vec![cx.observe(&terminals, |this, _, cx| this.tabs_changed(cx))];
        if let Some(signal) = Terminals::try_global(cx).map(|terminals| terminals.signal.clone()) {
            subscriptions.push(cx.subscribe(
                &signal,
                |this, _, event: &TerminalMetaChanged, cx| {
                    this.terminal_meta_changed(&event.file_id, &event.patch, cx);
                },
            ));
        }

        let mirror = Rc::new(RefCell::new(Mirror {
            autosave: config.autosave,
            ..Mirror::default()
        }));
        if let Some(package) = WorkspacePackage::try_global(cx) {
            package.register(&mirror);
        }

        let mut workspace = Self {
            tab_visit: empty_tab_visit_history(&active_tab_id),
            tabs,
            active_tab_id,
            project_cwd: config.project_cwd,
            terminals,
            composer_focused: config.composer_focused,
            dirty_files: config.dirty_file_ids.into_iter().collect(),
            file_error_counts: HashMap::new(),
            tab_visit_from_history: false,
            project_return: config.project_return,
            editor_navigation: None,
            editor_navigation_token: 0,
            known_terminals: HashSet::new(),
            mirror,
            factory,
            delegate,
            kv: config.kv,
            _subscriptions: subscriptions,
        };
        workspace.known_terminals = workspace.terminal_ids(cx);
        workspace.sync_mirror(cx);
        // The snapshot effect ran on mount too.
        update_sessions(cx, |sessions, cx| {
            sessions.sync_in_flight_snapshot(cx);
            sessions.schedule_workspace_snapshot(cx);
        });
        workspace
    }

    // Reading.

    pub fn tabs(&self) -> &[WorkspaceTab] {
        &self.tabs
    }

    pub fn active_tab_id(&self) -> &str {
        &self.active_tab_id
    }

    /// `activeTab`: the active tab, or the first one.
    pub fn active_tab(&self) -> Option<&WorkspaceTab> {
        self.tabs
            .iter()
            .find(|tab| tab.id == self.active_tab_id)
            .or(self.tabs.first())
    }

    pub fn project_cwd(&self) -> &str {
        &self.project_cwd
    }

    /// The project terminal docks of this window.
    pub fn terminals(&self) -> &Entity<ProjectTerminals> {
        &self.terminals
    }

    pub fn composer_focused(&self) -> bool {
        self.composer_focused
    }

    pub fn dirty_files(&self) -> &HashSet<String> {
        &self.dirty_files
    }

    pub fn file_error_counts(&self) -> &HashMap<String, i64> {
        &self.file_error_counts
    }

    /// `tabVisitNav`: whether back and forward have somewhere to go.
    pub fn tab_visit_nav(&self) -> (bool, bool) {
        (
            can_tab_visit_back(&self.tab_visit),
            can_tab_visit_forward(&self.tab_visit),
        )
    }

    pub fn editor_navigation(&self) -> Option<&EditorNavigationTarget> {
        self.editor_navigation.as_ref()
    }

    /// `active`: the focused chat of the active tab, or its first chat.
    pub fn active_session(&self, cx: &App) -> Option<Session> {
        let tab = self.active_tab()?;
        find_session(&tab.focused_id, cx).or_else(|| {
            let ids = leaf_ids(&tab.layout);
            all_sessions(cx)
                .into_iter()
                .find(|session| ids.contains(&session.id))
        })
    }

    /// `sessionDefaults`: the active chat, else the first open one.
    pub fn session_defaults(&self, cx: &App) -> Option<Session> {
        self.active_session(cx)
            .or_else(|| all_sessions(cx).into_iter().next())
    }

    /// `sidebarCwd`: the project the sidebar and new files belong to.
    pub fn sidebar_cwd(&self, cx: &App) -> String {
        if let Some(file) = self.active_tab().and_then(focused_file_tab) {
            return file.project_cwd.clone().unwrap_or_else(|| file.cwd.clone());
        }
        self.active_session(cx)
            .map(|session| session.cwd)
            .unwrap_or_else(|| self.project_cwd.clone())
    }

    /// `gitCwd`: the working copy git views, files, and terminals use.
    pub fn git_cwd(&self, cx: &App) -> String {
        if let Some(file) = self.active_tab().and_then(focused_file_tab) {
            return file.cwd.clone();
        }
        match self.active_session(cx) {
            Some(session) => self
                .delegate
                .remote_working_cwd(&session.cwd, &session.id, cx)
                .unwrap_or_else(|| session_work_cwd(&session).to_string()),
            None => self.sidebar_cwd(cx),
        }
    }

    /// `projectOfTab`: the project folder name the title bar shows.
    pub fn project_of_tab(&self, id: &str, cx: &App) -> Option<String> {
        let tab = self.tabs.iter().find(|tab| tab.id == id)?;
        Some(title_tab_project(tab, &all_sessions(cx)))
    }

    /// `deckProjectTabs`: the tabs of the current project, or only the
    /// active tab when it belongs to no project.
    pub fn deck_project_tabs(&self, cx: &App) -> Vec<WorkspaceTab> {
        let sessions = open_refs(cx);
        if let Some(active) = self.tabs.iter().find(|tab| tab.id == self.active_tab_id)
            && workspace_tab_cwd(active, &sessions).is_none()
        {
            return vec![active.clone()];
        }
        filter_tabs_for_project(&self.tabs, &sessions, &self.project_cwd)
    }

    /// The title bar's tabs, for every workspace tab.
    pub fn title_tabs(&self, unseen_finished_ids: &HashSet<String>, cx: &App) -> Vec<TitleTab> {
        let sessions = all_sessions(cx);
        self.tabs
            .iter()
            .map(|tab| {
                to_title_tab(
                    tab,
                    &sessions,
                    &self.dirty_files,
                    unseen_finished_ids,
                    self.delegate.as_ref(),
                    cx,
                )
            })
            .collect()
    }

    /// `runningTerminals`: terminals of the current dock and of every tab
    /// whose foreground process is not the shell.
    pub fn running_terminals(&self, cx: &App) -> Vec<RunningTerminal> {
        let terminals = self.terminals.read(cx);
        let mut files: Vec<&FilePaneTab> = Vec::new();
        if let Some(dock) = terminals.dock(&self.project_cwd) {
            files.extend(&dock.pane.files);
        }
        for tab in &self.tabs {
            for pane in &tab.terminal_panes {
                files.extend(&pane.files);
            }
        }
        list_running_terminals(files)
    }

    /// `runningTerminalOpen`: a running terminal is on screen now.
    pub fn running_terminal_open(&self, cx: &App) -> bool {
        let ids: HashSet<String> = self
            .running_terminals(cx)
            .into_iter()
            .map(|terminal| terminal.id)
            .collect();
        let terminals = self.terminals.read(cx);
        if let Some(dock) = terminals.dock(&self.project_cwd)
            && dock.open
            && dock.pane.files.iter().any(|file| ids.contains(&file.id))
        {
            return true;
        }
        self.active_tab()
            .and_then(focused_file_tab)
            .is_some_and(|file| ids.contains(&file.id))
    }

    /// The current project's dock side and size while it shows
    /// (`dockVisible`, `currentProjectDock`).
    pub fn dock_layout(&self, cx: &App) -> Option<(DockSide, i64)> {
        self.terminals.read(cx).visible_dock(&self.project_cwd)
    }

    /// `isBlankWorkspaceTab` for one of this window's tabs.
    pub fn is_blank_tab(&self, id: &str, cx: &App) -> bool {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| {
                is_blank_workspace_tab(tab, &all_sessions(cx), self.delegate.as_ref(), cx)
            })
    }

    // Bookkeeping.

    fn file_tab_mode(&self) -> FileTabMode {
        self.kv.as_ref().map(load_file_tab_mode).unwrap_or_default()
    }

    fn diff_viewer(&self) -> DiffViewer {
        self.kv.as_ref().map(load_diff_viewer).unwrap_or_default()
    }

    /// Every terminal file in the tabs and the docks.
    fn terminal_ids(&self, cx: &App) -> HashSet<String> {
        let mut ids: HashSet<String> = self.terminals.read(cx).file_ids().into_iter().collect();
        for tab in &self.tabs {
            for file in tab
                .editor_panes
                .iter()
                .chain(&tab.terminal_panes)
                .flat_map(|pane| &pane.files)
            {
                if file.terminal == Some(true) {
                    ids.insert(file.id.clone());
                }
            }
        }
        ids
    }

    fn sync_mirror(&mut self, cx: &App) {
        let terminals = self.terminals.read(cx);
        let mut mirror = self.mirror.borrow_mut();
        mirror.tabs = self.tabs.clone();
        mirror.active_tab_id = self.active_tab_id.clone();
        mirror.project_cwd = self.project_cwd.clone();
        mirror.docks = terminals.docks().to_vec();
        mirror.last_dock_side = terminals.last_dock_side();
        mirror.project_return = self.project_return.clone();
    }

    /// The effects App.tsx ran when `tabs`, `activeTabId`, or the docks
    /// changed: the visit history, the project return memory, the snapshot
    /// save, and the PTYs of closed terminals.
    fn tabs_changed(&mut self, cx: &mut Context<Self>) {
        let open_ids: HashSet<String> = self.tabs.iter().map(|tab| tab.id.clone()).collect();
        let mut next = prune_tab_visit_history(&self.tab_visit, &open_ids, &self.active_tab_id);
        if self.tab_visit_from_history {
            self.tab_visit_from_history = false;
        } else if next.current != self.active_tab_id {
            next = record_tab_visit(&next, &self.active_tab_id);
        }
        self.tab_visit = prune_tab_visit_history(&next, &open_ids, &self.active_tab_id);
        self.project_return = reconcile_project_return(
            &self.project_return,
            &self.tabs,
            &open_refs(cx),
            &self.active_tab_id,
        );

        let current = self.terminal_ids(cx);
        if let Some(terminals) = Terminals::try_global(cx) {
            for gone in self.known_terminals.difference(&current) {
                terminals.kill(gone);
            }
        }
        self.known_terminals = current;

        self.sync_mirror(cx);
        cx.notify();
        update_sessions(cx, |sessions, cx| {
            sessions.sync_in_flight_snapshot(cx);
            sessions.schedule_workspace_snapshot(cx);
            sessions.schedule_detach(cx);
        });
    }

    /// Reconcile the current tabs before a project selection reads its return target.
    pub fn read_project_return_memory(&mut self, cx: &mut Context<Self>) -> ProjectReturnMemory {
        self.project_return = reconcile_project_return(
            &self.project_return,
            &self.tabs,
            &open_refs(cx),
            &self.active_tab_id,
        );
        self.sync_mirror(cx);
        self.project_return.clone()
    }

    /// A focused session that moved projects leaves an incompatible tab group.
    pub fn session_project_changed(&mut self, session_id: &str, cwd: &str, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| leaf_ids(&tab.layout).iter().any(|id| id == session_id))
        else {
            return;
        };
        let Some(group) = tab.group_id.as_deref() else {
            return;
        };
        if tab.focused_id != session_id {
            return;
        }
        let tab_id = tab.id.clone();
        let others = self
            .tabs
            .iter()
            .filter(|other| other.id != tab_id)
            .cloned()
            .collect::<Vec<_>>();
        let project = project_name(cwd);
        let others_project =
            monocode_layout::tab_groups::tab_group_project(&others, group, &|id| {
                self.project_of_tab(id, cx)
            });
        if others_project
            .is_some_and(|other| !other.is_empty() && !project.is_empty() && other != project)
        {
            self.tabs = monocode_layout::tab_groups::remove_tab_from_group(&self.tabs, &tab_id);
            self.tabs_changed(cx);
        }
    }

    fn set_tabs(&mut self, tabs: Vec<WorkspaceTab>, cx: &mut Context<Self>) {
        self.tabs = tabs;
        self.tabs_changed(cx);
    }

    fn map_tabs(
        &mut self,
        cx: &mut Context<Self>,
        mut update: impl FnMut(&WorkspaceTab) -> WorkspaceTab,
    ) {
        let tabs = self.tabs.iter().map(&mut update).collect();
        self.set_tabs(tabs, cx);
    }

    fn map_tab(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
        update: impl FnOnce(&WorkspaceTab) -> WorkspaceTab,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs[index] = update(&self.tabs[index]);
        self.tabs_changed(cx);
    }

    /// `setComposerFocused`.
    pub fn set_composer_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.composer_focused != focused {
            self.composer_focused = focused;
            cx.notify();
        }
    }

    fn set_dock_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.terminals
            .update(cx, |terminals, cx| terminals.set_focused(focused, cx));
    }

    /// `focusProjectTerminal`.
    fn focus_project_terminal(&mut self, cx: &mut Context<Self>) {
        self.set_dock_focused(true, cx);
        self.set_composer_focused(false, cx);
    }

    fn forget_dirty<'a>(&mut self, ids: impl IntoIterator<Item = &'a String>) {
        for id in ids {
            self.dirty_files.remove(id);
        }
    }

    fn insert_session(session: Session, cx: &mut App) {
        update_sessions(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
    }

    fn persist_session(session_id: &str, cx: &mut App) {
        update_sessions(cx, |sessions, cx| sessions.persist(session_id, cx));
    }

    /// Drop a blank chat whose pane something else takes over: the harness
    /// child, the save fingerprint, and the session (`lastPersisted.delete`,
    /// `forgetHarnessSession`, and the `setSessions` filter).
    fn drop_blank_session(session: &Session, remove: bool, cx: &mut App) {
        let hooks = Engine::hooks(cx);
        hooks
            .harness
            .forget_session(session.harness, &session.id, cx)
            .detach();
        let id = session.id.clone();
        update_sessions(cx, move |sessions, cx| {
            sessions.forget_persisted(&id);
            if remove {
                sessions.remove(&id, cx);
            }
        });
    }

    /// `insertBesideActive`: a new tab beside the active one, joining its
    /// group when the projects match.
    fn insert_beside_active(
        &self,
        tabs: &[WorkspaceTab],
        tab: WorkspaceTab,
        cwd: Option<&str>,
        cx: &App,
    ) -> Vec<WorkspaceTab> {
        let sessions = all_sessions(cx);
        let new_id = tab.id.clone();
        let new_project = cwd.map(project_name);
        let lookup = |id: &str| -> Option<String> {
            if id == new_id {
                return new_project.clone();
            }
            tabs.iter()
                .find(|entry| entry.id == id)
                .map(|entry| title_tab_project(entry, &sessions))
        };
        insert_tab_beside_active(tabs, tab, Some(&self.active_tab_id), Some(&lookup))
    }

    /// `appendTab`.
    fn append_tab(&mut self, tab: WorkspaceTab, cwd: Option<&str>, cx: &mut Context<Self>) {
        let tabs = self.insert_beside_active(&self.tabs, tab, cwd, cx);
        self.tabs = tabs;
    }

    /// Insert a project tab and update the saved workspace.
    pub fn append_project_tab(
        &mut self,
        tab: WorkspaceTab,
        cwd: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        self.append_tab(tab, cwd, cx);
        self.tabs_changed(cx);
    }

    /// Insert beside the requested tab while preserving project grouping.
    pub fn insert_project_tab_beside(
        &mut self,
        tab: WorkspaceTab,
        anchor: &str,
        cwd: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let sessions = all_sessions(cx);
        let new_id = tab.id.clone();
        let project = cwd.map(project_name);
        let lookup = |id: &str| {
            if id == new_id {
                project.clone()
            } else {
                self.tabs
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| title_tab_project(entry, &sessions))
            }
        };
        self.tabs = insert_tab_beside_active(&self.tabs, tab, Some(anchor), Some(&lookup));
        self.tabs_changed(cx);
    }

    /// Replace project tabs and select the requested surviving tab.
    pub fn replace_project_tabs(
        &mut self,
        tabs: Vec<WorkspaceTab>,
        active: &str,
        cx: &mut Context<Self>,
    ) {
        self.tabs = tabs;
        self.active_tab_id = self
            .tabs
            .iter()
            .find(|tab| tab.id == active)
            .or_else(|| self.tabs.first())
            .map(|tab| tab.id.clone())
            .unwrap_or_default();
        self.tabs_changed(cx);
    }

    // Window state.

    /// `document.hidden` changed for this window.
    pub fn set_window_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        self.mirror.borrow_mut().hidden = hidden;
        let all_hidden =
            Engine::try_global(cx).is_some_and(|_| Engine::hooks(cx).workspace.window_hidden(cx));
        Files::set_hidden(all_hidden, cx);
        if let Some(terminals) = Terminals::try_global(cx) {
            terminals.set_hidden(all_hidden, cx);
        }
    }

    /// The window took focus.
    pub fn window_focused(&mut self, cx: &mut Context<Self>) {
        Files::window_focused(cx);
    }

    /// A full page (settings, search, inbox, notes) covers the workspace, so
    /// its chats are not in the foreground.
    pub fn full_page_open(&self) -> bool {
        self.mirror.borrow().covered
    }

    pub fn set_full_page_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.mirror.borrow().covered != open {
            self.mirror.borrow_mut().covered = open;
            cx.notify();
        }
    }

    pub fn set_inbox_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.mirror.borrow().inbox_visible != visible {
            self.mirror.borrow_mut().inbox_visible = visible;
            cx.notify();
        }
    }

    pub fn set_inbox_session(&mut self, session_id: Option<String>, cx: &mut Context<Self>) {
        if self.mirror.borrow().inbox_session_id != session_id {
            self.mirror.borrow_mut().inbox_session_id = session_id;
            cx.notify();
        }
    }

    // Tabs.

    /// `activateTab`: switch to a tab, optionally focusing one of its panes,
    /// and follow it to its project.
    pub fn activate_tab(&mut self, id: &str, pane_id: Option<&str>, cx: &mut Context<Self>) {
        let tab = self.tabs.iter().find(|entry| entry.id == id).cloned();
        let next_focused_id = match (&tab, pane_id) {
            (Some(tab), Some(pane_id))
                if leaf_ids(&tab.layout).iter().any(|leaf| leaf == pane_id)
                    || tab.editor_panes.iter().any(|entry| entry.id == pane_id)
                    || tab.terminal_panes.iter().any(|entry| entry.id == pane_id) =>
            {
                Some(pane_id.to_string())
            }
            (Some(tab), _) => Some(tab.focused_id.clone()),
            (None, _) => None,
        };

        self.active_tab_id = id.to_string();
        if let (Some(tab), Some(next)) = (&tab, &next_focused_id)
            && *next != tab.focused_id
        {
            for entry in &mut self.tabs {
                if entry.id == id {
                    entry.focused_id = next.clone();
                    entry.diff_focused = Some(false);
                }
            }
        }

        if let Some(tab) = &tab {
            let focused_tab = WorkspaceTab {
                focused_id: next_focused_id
                    .clone()
                    .unwrap_or_else(|| tab.focused_id.clone()),
                ..tab.clone()
            };
            if let Some(cwd) = focused_workspace_tab_cwd(&focused_tab, &open_refs(cx))
                && looks_like_project(&cwd)
            {
                let normalized = normalize_project_path(&cwd);
                if !same_project_path(&normalized, &self.project_cwd) {
                    self.project_cwd = normalized.clone();
                    self.delegate.remember_project(&normalized, cx);
                }
            }
        }
        let composer = next_focused_id.is_some_and(|id| has_session(&id, cx));
        self.composer_focused = composer;
        self.tabs_changed(cx);
    }

    /// Move to another project, as the project rail does.
    pub fn set_project_cwd(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if self.project_cwd != cwd {
            self.project_cwd = cwd.to_string();
            self.tabs_changed(cx);
        }
    }

    /// `onNew`: a new chat in a new tab beside the active one. Returns its id.
    pub fn new_session_tab(&mut self, cx: &mut Context<Self>) -> String {
        let defaults = self.session_defaults(cx);
        let cwd = self
            .active_session(cx)
            .map(|session| session.cwd)
            .or_else(|| defaults.as_ref().map(|session| session.cwd.clone()))
            .unwrap_or_else(|| self.project_cwd.clone());
        let session = self
            .factory
            .new_default_session(&cwd, defaults.map(|session| session.runtime_mode));
        let id = session.id.clone();
        let tab = new_tab(&id);
        let tab_id = tab.id.clone();
        self.append_tab(tab, Some(&cwd), cx);
        self.active_tab_id = tab_id;
        self.composer_focused = true;
        self.tabs_changed(cx);
        Self::insert_session(session, cx);
        id
    }

    /// `focusOpenSession`: show a chat that already has a tab.
    pub fn focus_open_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(tab_id) =
            find_open_session_tab(&self.tabs, &open_refs(cx), session_id).map(|tab| tab.id.clone())
        else {
            return false;
        };
        // `loadedSessionCache.delete`: the open copy replaces any cached one.
        update_sessions(cx, |sessions, _| sessions.invalidate_loaded(session_id));
        self.active_tab_id = tab_id.clone();
        let focused = session_id.to_string();
        self.composer_focused = true;
        self.map_tab(&tab_id, cx, move |tab| WorkspaceTab {
            focused_id: focused,
            ..tab.clone()
        });
        true
    }

    /// `replaceBlankPaneWithSession`: put a chat into the active tab's
    /// blank pane.
    pub fn replace_blank_pane_with_session(
        &mut self,
        session: Session,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.active_tab().cloned() else {
            return false;
        };
        let sessions = all_sessions(cx);
        let blank = |id: &str| is_blank_session(sessions.iter().find(|entry| entry.id == id));
        let pane_id = if blank(&tab.focused_id) {
            Some(tab.focused_id.clone())
        } else {
            leaf_ids(&tab.layout).into_iter().find(|id| blank(id))
        };
        let Some(pane_id) = pane_id.filter(|pane| *pane != session.id) else {
            return false;
        };
        let session_id = session.id.clone();
        self.tabs = self
            .tabs
            .iter()
            .map(|entry| {
                if entry.id == tab.id {
                    WorkspaceTab {
                        layout: replace_leaf_id(&entry.layout, &pane_id, &session_id),
                        focused_id: session_id.clone(),
                        ..entry.clone()
                    }
                } else {
                    entry.clone()
                }
            })
            .collect();
        self.active_tab_id = tab.id.clone();
        self.composer_focused = true;
        self.tabs_changed(cx);
        update_sessions(cx, |sessions, cx| {
            sessions.replace_blank(&pane_id, session, cx)
        });
        true
    }

    /// `onSelectHistorySession`: open a stored chat, in its tab if it has
    /// one, else in a blank pane, else in a new tab. A worker opens its
    /// lead's tab.
    // TODO(port): `orchestrator.forSession(id)?.leadId` also found the lead
    // of a running orchestration; only the stored lead id is read here.
    pub fn open_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> Task<()> {
        let Some(sessions) = sessions_entity(cx) else {
            return Task::ready(());
        };
        let opening = sessions.update(cx, |sessions, cx| sessions.ensure_open(session_id, cx));
        let session_id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            let Some(mut session) = opening.await else {
                return;
            };
            if session.inbox_ask.is_some() {
                return;
            }
            if let Some(parent) = session.orchestration_lead_id.clone()
                && parent != session_id
            {
                let opening = cx.update(|cx| {
                    sessions.update(cx, |sessions, cx| sessions.ensure_open(&parent, cx))
                });
                let Some(lead) = opening.await else {
                    return;
                };
                session = lead;
            }
            this.update(cx, |this, cx| {
                if this.focus_open_session(&session.id, cx) {
                    return;
                }
                if this.replace_blank_pane_with_session(session.clone(), cx) {
                    return;
                }
                let tab = new_tab(&session.id);
                let tab_id = tab.id.clone();
                this.append_tab(tab, Some(&session.cwd), cx);
                this.active_tab_id = tab_id;
                this.composer_focused = true;
                this.tabs_changed(cx);
            })
            .ok();
        })
    }

    /// `onSplit`: a new chat beside the focused pane.
    pub fn split(&mut self, dir: SplitDir, cx: &mut Context<Self>) {
        let Some(active) = self.active_tab().cloned() else {
            return;
        };
        let defaults = self.session_defaults(cx);
        let cwd = defaults
            .as_ref()
            .map(|session| session.cwd.clone())
            .unwrap_or_else(|| self.project_cwd.clone());
        let session = self
            .factory
            .new_default_session(&cwd, defaults.map(|session| session.runtime_mode));
        let id = session.id.clone();
        self.map_tab(&active.id, cx, |tab| WorkspaceTab {
            layout: split_pane(&tab.layout, &tab.focused_id, dir, &id),
            focused_id: id.clone(),
            ..tab.clone()
        });
        Self::insert_session(session, cx);
        self.set_composer_focused(true, cx);
    }

    /// `onCloseTab`. `confirmed_terminal_ids` skips the running-process
    /// prompt for terminals the caller already asked about.
    pub fn close_tab(
        &mut self,
        id: &str,
        confirmed_terminal_ids: &[String],
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let current = self.tabs.clone();
        let Some(index) = current.iter().position(|tab| tab.id == id) else {
            return Task::ready(());
        };
        let plan = plan_workspace_tab_close(&current, &open_refs(cx), id, TAB_CLOSE_SCOPE);
        let WorkspaceTabClosePlan::Close { next_active_tab_id } = plan else {
            return Task::ready(());
        };
        let closing = current[index].clone();
        let closing_files = pane_files(&closing);
        let unsaved = closing_files
            .iter()
            .any(|file| is_filesystem_tab(file) && self.dirty_files.contains(&file.id));
        let terminals: Vec<FilePaneTab> = closing_files
            .iter()
            .filter(|file| {
                file.terminal == Some(true) && !confirmed_terminal_ids.contains(&file.id)
            })
            .cloned()
            .collect();
        let sidebar_cwd = self.sidebar_cwd(cx);
        let id = id.to_string();
        let confirm = self.confirm_closing(
            unsaved.then_some("Close this tab with unsaved files?"),
            terminals,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            this.update(cx, |this, cx| {
                // TODO(port): this filters the tab list from when the close
                // started, as the TypeScript did, so a tab opened while the
                // prompt was up is dropped.
                let next: Vec<WorkspaceTab> =
                    current.into_iter().filter(|tab| tab.id != id).collect();
                let gone: Vec<String> = leaf_ids(&closing.layout)
                    .into_iter()
                    .filter(|pane| has_session(pane, cx))
                    .collect();
                for session_id in &gone {
                    Self::persist_session(session_id, cx);
                    this.delegate.remember_remote_session(session_id, cx);
                }
                let closing_ids: Vec<String> =
                    closing_files.iter().map(|file| file.id.clone()).collect();
                this.forget_dirty(&closing_ids);
                this.set_tabs(next, cx);
                if id == this.active_tab_id
                    && let Some(next_active) = next_active_tab_id
                {
                    this.activate_tab(&next_active, None, cx);
                }
                this.delegate.refresh_history(&sidebar_cwd, cx);
            })
            .ok();
        })
    }

    /// Ask about unsaved files, then about running terminals. Resolves to
    /// whether to go on.
    fn confirm_closing(
        &self,
        unsaved_message: Option<&str>,
        terminals: Vec<FilePaneTab>,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let unsaved =
            unsaved_message.map(|message| self.delegate.confirm(message, CONFIRM_LABEL, cx));
        let delegate = self.delegate.clone();
        cx.spawn(async move |_, cx| {
            if let Some(unsaved) = unsaved
                && !unsaved.await
            {
                return false;
            }
            if terminals.is_empty() {
                return true;
            }
            let confirm = cx.update(|cx| confirm_close_terminals(&terminals, delegate.clone(), cx));
            confirm.await
        })
    }

    /// `onCloseTabs`: close several tabs and activate `fallback_id` when the
    /// active one goes. `confirmed` skips the prompts.
    pub fn close_tabs(
        &mut self,
        ids: &[String],
        fallback_id: &str,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let closing_ids: HashSet<String> = ids.iter().cloned().collect();
        let closing: Vec<WorkspaceTab> = self
            .tabs
            .iter()
            .filter(|tab| closing_ids.contains(&tab.id))
            .cloned()
            .collect();
        let has_fallback = self
            .tabs
            .iter()
            .any(|tab| tab.id == fallback_id && !closing_ids.contains(&tab.id));
        if !has_fallback || closing.is_empty() {
            return Task::ready(());
        }
        let closing_files: Vec<FilePaneTab> = closing.iter().flat_map(pane_files).collect();
        let unsaved = closing_files
            .iter()
            .any(|file| is_filesystem_tab(file) && self.dirty_files.contains(&file.id));
        let terminals: Vec<FilePaneTab> = closing_files
            .iter()
            .filter(|file| file.terminal == Some(true))
            .cloned()
            .collect();
        let sidebar_cwd = self.sidebar_cwd(cx);
        let fallback = fallback_id.to_string();
        let finish = move |this: &mut Self, cx: &mut Context<Self>| {
            let session_ids: HashSet<String> = closing
                .iter()
                .flat_map(|tab| leaf_ids(&tab.layout))
                .filter(|pane| has_session(pane, cx))
                .collect();
            for session_id in &session_ids {
                Self::persist_session(session_id, cx);
                this.delegate.remember_remote_session(session_id, cx);
            }
            let file_ids: Vec<String> = closing_files.iter().map(|file| file.id.clone()).collect();
            this.forget_dirty(&file_ids);
            let next = this
                .tabs
                .iter()
                .filter(|tab| !closing_ids.contains(&tab.id))
                .cloned()
                .collect();
            this.set_tabs(next, cx);
            if closing_ids.contains(&this.active_tab_id) {
                this.activate_tab(&fallback, None, cx);
            }
            this.delegate.refresh_history(&sidebar_cwd, cx);
        };
        // The caller already confirmed unsaved files and terminals.
        if confirmed {
            finish(self, cx);
            return Task::ready(());
        }
        let confirm = self.confirm_closing(
            unsaved.then_some("Close these tabs with unsaved files?"),
            terminals,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if confirm.await {
                this.update(cx, finish).ok();
            }
        })
    }

    /// `onCloseOtherTabs`.
    pub fn close_other_tabs(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let active = self.active_tab_id.clone();
        if !self.tabs.iter().any(|tab| tab.id == active) {
            return Task::ready(());
        }
        let others: Vec<String> = self
            .tabs
            .iter()
            .filter(|tab| tab.id != active)
            .map(|tab| tab.id.clone())
            .collect();
        self.close_tabs(&others, &active, false, cx)
    }

    /// `onCloseFile`: close one file or terminal tab, and its pane when it
    /// was the last one.
    pub fn close_file(&mut self, pane_id: &str, file_id: &str, cx: &mut Context<Self>) -> Task<()> {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|entry| find_surface_pane(entry, pane_id).is_some())
            .cloned()
        else {
            return Task::ready(());
        };
        let Some((kind, pane)) =
            find_surface_pane(&tab, pane_id).map(|(kind, pane)| (kind, pane.clone()))
        else {
            return Task::ready(());
        };
        let Some(index) = pane.files.iter().position(|file| file.id == file_id) else {
            return Task::ready(());
        };
        let file = pane.files[index].clone();
        let unsaved = is_filesystem_tab(&file) && self.dirty_files.contains(file_id);
        let active_tab_id = self.active_tab_id.clone();
        let unsaved_prompt = unsaved.then(|| {
            self.delegate.confirm(
                &format!("Close {} without saving?", basename(&file.path)),
                CONFIRM_LABEL,
                cx,
            )
        });
        let delegate = self.delegate.clone();
        let pane_id = pane_id.to_string();
        let file_id = file_id.to_string();
        cx.spawn(async move |this, cx| {
            if let Some(prompt) = unsaved_prompt
                && !prompt.await
            {
                return;
            }
            if file.terminal == Some(true) {
                let confirm = cx.update(|cx| {
                    confirm_close_terminals(std::slice::from_ref(&file), delegate.clone(), cx)
                });
                if !confirm.await {
                    return;
                }
            }
            let Ok(follow_up) = this.update(cx, |this, cx| {
                this.finish_close_file(
                    &tab,
                    kind,
                    &pane,
                    index,
                    &file,
                    &pane_id,
                    &file_id,
                    &active_tab_id,
                    cx,
                )
            }) else {
                return;
            };
            if let Some(task) = follow_up {
                task.await;
            }
        })
    }

    /// The `finishClose` of `onCloseFile`. Returns the tab close it hands
    /// off to, if any.
    #[allow(clippy::too_many_arguments)]
    fn finish_close_file(
        &mut self,
        tab: &WorkspaceTab,
        kind: SurfaceKind,
        pane: &EditorPane,
        index: usize,
        file: &FilePaneTab,
        pane_id: &str,
        file_id: &str,
        active_tab_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Task<()>> {
        let files: Vec<FilePaneTab> = pane
            .files
            .iter()
            .filter(|entry| entry.id != file_id)
            .cloned()
            .collect();
        let next_focus;
        let mut next_layout = tab.layout.clone();
        let mut next_panes = surface_panes(tab, kind).to_vec();
        if !files.is_empty() {
            next_focus = pane_id.to_string();
            let active_file_id = if pane.active_file_id == file_id {
                files[index.min(files.len() - 1)].id.clone()
            } else {
                pane.active_file_id.clone()
            };
            next_panes = next_panes
                .into_iter()
                .map(|entry| {
                    if entry.id == pane_id {
                        EditorPane {
                            files: files.clone(),
                            active_file_id: active_file_id.clone(),
                            ..entry
                        }
                    } else {
                        entry
                    }
                })
                .collect();
        } else {
            let sibling = sibling_leaf_id(&tab.layout, pane_id);
            let Some(without_pane) = remove_pane(&tab.layout, pane_id) else {
                self.dirty_files.remove(file_id);
                let plan =
                    plan_workspace_tab_close(&self.tabs, &open_refs(cx), &tab.id, TAB_CLOSE_SCOPE);
                if matches!(plan, WorkspaceTabClosePlan::Close { .. }) {
                    let confirmed = if file.terminal == Some(true) {
                        vec![file_id.to_string()]
                    } else {
                        Vec::new()
                    };
                    return Some(self.close_tab(&tab.id, &confirmed, cx));
                }
                let seed = all_sessions(cx).into_iter().next();
                let cwd = if file.cwd.is_empty() {
                    self.project_cwd.clone()
                } else {
                    file.cwd.clone()
                };
                let session = session_seeded_from(self.factory.as_ref(), seed.as_ref(), &cwd);
                let id = session.id.clone();
                self.map_tab(&tab.id, cx, |entry| reset_tab_to_session(entry, &id));
                Self::insert_session(session, cx);
                self.set_composer_focused(true, cx);
                return None;
            };
            next_focus = if tab.focused_id == pane_id {
                sibling.unwrap_or_else(|| first_leaf_id(&without_pane).to_string())
            } else {
                tab.focused_id.clone()
            };
            next_layout = without_pane;
            next_panes.retain(|entry| entry.id != pane_id);
        }

        let focus = next_focus.clone();
        self.map_tab(&tab.id, cx, |entry| {
            with_surface_panes(
                &WorkspaceTab {
                    layout: next_layout,
                    focused_id: focus,
                    ..entry.clone()
                },
                kind,
                next_panes,
            )
        });
        self.dirty_files.remove(file_id);
        if tab.id == active_tab_id && files.is_empty() {
            let focused = has_session(&next_focus, cx);
            self.set_composer_focused(focused, cx);
        }
        None
    }

    /// `onCloseOtherFiles`.
    pub fn close_other_files(
        &mut self,
        pane_id: &str,
        file_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|entry| find_surface_pane(entry, pane_id).is_some())
            .cloned()
        else {
            return Task::ready(());
        };
        let Some((_, pane)) = find_surface_pane(&tab, pane_id) else {
            return Task::ready(());
        };
        if !pane.files.iter().any(|file| file.id == file_id) {
            return Task::ready(());
        }
        let closing: Vec<FilePaneTab> = pane
            .files
            .iter()
            .filter(|file| file.id != file_id)
            .cloned()
            .collect();
        if closing.is_empty() {
            return Task::ready(());
        }
        let closing_ids: HashSet<String> = closing.iter().map(|file| file.id.clone()).collect();
        let unsaved = closing
            .iter()
            .any(|file| is_filesystem_tab(file) && self.dirty_files.contains(&file.id));
        let terminals: Vec<FilePaneTab> = closing
            .iter()
            .filter(|file| file.terminal == Some(true))
            .cloned()
            .collect();
        let confirm = self.confirm_closing(
            unsaved.then_some("Close other tabs with unsaved files?"),
            terminals,
            cx,
        );
        let tab_id = tab.id.clone();
        let pane_id = pane_id.to_string();
        let file_id = file_id.to_string();
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            this.update(cx, |this, cx| {
                this.map_tab(&tab_id, cx, |entry| {
                    let Some((kind, current)) = find_surface_pane(entry, &pane_id) else {
                        return entry.clone();
                    };
                    if !current.files.iter().any(|file| file.id == file_id) {
                        return entry.clone();
                    }
                    let panes = surface_panes(entry, kind)
                        .iter()
                        .map(|pane| {
                            if pane.id == pane_id {
                                EditorPane {
                                    files: pane
                                        .files
                                        .iter()
                                        .filter(|file| !closing_ids.contains(&file.id))
                                        .cloned()
                                        .collect(),
                                    active_file_id: file_id.clone(),
                                    ..pane.clone()
                                }
                            } else {
                                pane.clone()
                            }
                        })
                        .collect();
                    with_surface_panes(
                        &WorkspaceTab {
                            focused_id: pane_id.clone(),
                            ..entry.clone()
                        },
                        kind,
                        panes,
                    )
                });
                for id in &closing_ids {
                    this.dirty_files.remove(id);
                }
            })
            .ok();
        })
    }

    /// `onClearTabSession`: replace a tab's contents with a new chat like the
    /// one it showed.
    pub fn clear_tab_session(&mut self, id: &str, cx: &mut Context<Self>) -> Task<()> {
        let Some(tab) = self.tabs.iter().find(|entry| entry.id == id).cloned() else {
            return Task::ready(());
        };
        if is_blank_workspace_tab(&tab, &all_sessions(cx), self.delegate.as_ref(), cx) {
            return Task::ready(());
        }
        let closing_files = pane_files(&tab);
        let unsaved = closing_files
            .iter()
            .any(|file| is_filesystem_tab(file) && self.dirty_files.contains(&file.id));
        let Some(old_session) = leaf_ids(&tab.layout)
            .into_iter()
            .find_map(|pane| find_session(&pane, cx))
        else {
            return Task::ready(());
        };
        let sidebar_cwd = self.sidebar_cwd(cx);
        let id = id.to_string();
        let finish = move |this: &mut Self, cx: &mut Context<Self>| {
            Self::persist_session(&old_session.id, cx);
            for shell_id in leaf_ids(&tab.layout) {
                this.delegate.remember_remote_session(&shell_id, cx);
            }
            let session = this.factory.new_session(
                old_session.harness,
                &old_session.cwd,
                Some(&old_session.model),
                Some(old_session.runtime_mode),
                Some(&old_session.model_settings),
            );
            let session_id = session.id.clone();
            let file_ids: Vec<String> = closing_files.iter().map(|file| file.id.clone()).collect();
            this.forget_dirty(&file_ids);
            this.map_tab(&id, cx, |entry| reset_tab_to_session(entry, &session_id));
            Self::insert_session(session, cx);
            this.set_composer_focused(true, cx);
            this.delegate.refresh_history(&sidebar_cwd, cx);
        };
        if !unsaved {
            finish(self, cx);
            return Task::ready(());
        }
        let confirm = self.delegate.confirm(
            "Close this conversation with unsaved files?",
            CONFIRM_LABEL,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if confirm.await {
                this.update(cx, finish).ok();
            }
        })
    }

    /// `onRemoteSessionDeleted`.
    pub fn remote_session_deleted(
        &mut self,
        remote_session_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let delegate = self.delegate.clone();
        let Some(tab_id) = self
            .tabs
            .iter()
            .find(|entry| {
                leaf_ids(&entry.layout).iter().any(|shell| {
                    delegate.remote_session_for(shell, cx).as_deref() == Some(remote_session_id)
                })
            })
            .map(|tab| tab.id.clone())
        else {
            return Task::ready(());
        };
        match plan_workspace_tab_close(&self.tabs, &open_refs(cx), &tab_id, TAB_CLOSE_SCOPE) {
            WorkspaceTabClosePlan::Keep => self.clear_tab_session(&tab_id, cx),
            WorkspaceTabClosePlan::Close { .. } => self.close_tab(&tab_id, &[], cx),
        }
    }

    /// `onCloseAllTabs`: first the active tab's open files; when it has
    /// none, every other tab, leaving one blank chat.
    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|entry| entry.id == self.active_tab_id)
            .cloned()
        else {
            return Task::ready(());
        };
        let factory = self.factory.clone();
        let project_cwd = self.project_cwd.clone();
        let seed_session = move |cwd: &str, cx: &App| {
            let seed = all_sessions(cx).into_iter().next();
            session_seeded_from(factory.as_ref(), seed.as_ref(), cwd)
        };

        // Stage one: files in the active tab's editor panes close first.
        let editor_files: Vec<FilePaneTab> = tab
            .editor_panes
            .iter()
            .flat_map(|pane| pane.files.iter().cloned())
            .collect();
        if !editor_files.is_empty() {
            let remaining = close_surface_panes(&tab, SurfaceKind::Editor);
            if remaining.is_none()
                && matches!(
                    plan_workspace_tab_close(&self.tabs, &open_refs(cx), &tab.id, TAB_CLOSE_SCOPE),
                    WorkspaceTabClosePlan::Close { .. }
                )
            {
                return self.close_tab(&tab.id, &[], cx);
            }
            let unsaved = editor_files
                .iter()
                .any(|file| is_filesystem_tab(file) && self.dirty_files.contains(&file.id));
            let finish = move |this: &mut Self, cx: &mut Context<Self>| {
                let (next_tab, focuses_session, created) = match remaining {
                    Some(remaining) => {
                        let focuses = has_session(&remaining.focused_id, cx);
                        (remaining, focuses, None)
                    }
                    None => {
                        // The tab held only editor panes and must stay.
                        let cwd = if editor_files[0].cwd.is_empty() {
                            project_cwd.clone()
                        } else {
                            editor_files[0].cwd.clone()
                        };
                        let session = seed_session(&cwd, cx);
                        (reset_tab_to_session(&tab, &session.id), true, Some(session))
                    }
                };
                this.map_tab(&tab.id, cx, |_| next_tab);
                if let Some(session) = created {
                    Self::insert_session(session, cx);
                }
                let file_ids: Vec<String> =
                    editor_files.iter().map(|file| file.id.clone()).collect();
                this.forget_dirty(&file_ids);
                this.set_composer_focused(focuses_session, cx);
            };
            if !unsaved {
                finish(self, cx);
                return Task::ready(());
            }
            let confirm = self.delegate.confirm(
                "Close all open files with unsaved changes?",
                CONFIRM_LABEL,
                cx,
            );
            return cx.spawn(async move |this, cx| {
                if confirm.await {
                    this.update(cx, finish).ok();
                }
            });
        }

        // Stage two: the workspace always keeps one tab, so close every
        // other tab and reset the active one to a blank chat. Every prompt
        // runs before any tab changes.
        let other_ids: Vec<String> = self
            .tabs
            .iter()
            .filter(|entry| entry.id != tab.id)
            .map(|entry| entry.id.clone())
            .collect();
        let terminal_files: Vec<FilePaneTab> = tab
            .terminal_panes
            .iter()
            .flat_map(|pane| pane.files.iter().cloned())
            .collect();
        let mut closing_files: Vec<FilePaneTab> = self
            .tabs
            .iter()
            .filter(|entry| other_ids.contains(&entry.id))
            .flat_map(pane_files)
            .collect();
        closing_files.extend(terminal_files.iter().cloned());
        let unsaved = closing_files
            .iter()
            .any(|file| is_filesystem_tab(file) && self.dirty_files.contains(&file.id));
        let terminals: Vec<FilePaneTab> = closing_files
            .into_iter()
            .filter(|file| file.terminal == Some(true))
            .collect();
        let confirm = self.confirm_closing(
            unsaved.then_some("Close all tabs with unsaved files?"),
            terminals,
            cx,
        );
        let project_cwd = self.project_cwd.clone();
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            let clear = this.update(cx, |this, cx| {
                if !other_ids.is_empty() {
                    this.close_tabs(&other_ids, &tab.id, true, cx).detach();
                }
                let has_chat = leaf_ids(&tab.layout)
                    .iter()
                    .any(|pane| has_session(pane, cx));
                if has_chat {
                    // No editor files remain, so this commits without a prompt.
                    return Some(this.clear_tab_session(&tab.id, cx));
                }
                // The tab held no chat: seed one so the workspace stays usable.
                let cwd = terminal_files
                    .first()
                    .map(|file| file.cwd.clone())
                    .filter(|cwd| !cwd.is_empty())
                    .unwrap_or(project_cwd);
                let session = seed_session(&cwd, cx);
                let id = session.id.clone();
                this.map_tab(&tab.id, cx, |entry| reset_tab_to_session(entry, &id));
                Self::insert_session(session, cx);
                this.set_composer_focused(true, cx);
                None
            });
            if let Ok(Some(clear)) = clear {
                clear.await;
            }
        })
    }

    /// `onClosePane`: close the focused file, or a chat pane, or the tab
    /// when its last pane goes. The dock has its own close buttons.
    pub fn close_pane(&mut self, session_id: Option<&str>, cx: &mut Context<Self>) -> Task<()> {
        let Some(active) = self.active_tab().cloned() else {
            return Task::ready(());
        };
        if session_id.is_none()
            && let Some((_, pane)) = find_surface_pane(&active, &active.focused_id)
        {
            let (pane_id, file_id) = (pane.id.clone(), pane.active_file_id.clone());
            return self.close_file(&pane_id, &file_id, cx);
        }
        let closing_id = session_id
            .map(str::to_string)
            .unwrap_or_else(|| active.focused_id.clone());
        let session_ids: Vec<String> = leaf_ids(&active.layout)
            .into_iter()
            .filter(|pane| has_session(pane, cx))
            .collect();
        if !session_ids.contains(&closing_id) {
            return Task::ready(());
        }
        let Some(next_tab) = close_leaf(&active, &closing_id) else {
            return match plan_workspace_tab_close(
                &self.tabs,
                &open_refs(cx),
                &active.id,
                TAB_CLOSE_SCOPE,
            ) {
                WorkspaceTabClosePlan::Keep => self.clear_tab_session(&active.id, cx),
                WorkspaceTabClosePlan::Close { .. } => self.close_tab(&active.id, &[], cx),
            };
        };
        Self::persist_session(&closing_id, cx);
        let (layout, focused) = (next_tab.layout.clone(), next_tab.focused_id.clone());
        self.map_tab(&active.id, cx, |tab| WorkspaceTab {
            layout,
            focused_id: focused,
            ..tab.clone()
        });
        if closing_id == active.focused_id {
            let focused = has_session(&next_tab.focused_id, cx);
            self.set_composer_focused(focused, cx);
        }
        let sidebar_cwd = self.sidebar_cwd(cx);
        self.delegate.refresh_history(&sidebar_cwd, cx);
        Task::ready(())
    }

    /// `onCloseTitleTab`: a title tab's close button. The active tab the
    /// project keeps closes its focused pane instead.
    pub fn close_title_tab(&mut self, id: &str, cx: &mut Context<Self>) -> Task<()> {
        let plan = plan_workspace_tab_close(&self.tabs, &open_refs(cx), id, TAB_CLOSE_SCOPE);
        if plan == WorkspaceTabClosePlan::Keep && id == self.active_tab_id {
            return self.close_pane(None, cx);
        }
        self.close_tab(id, &[], cx)
    }

    /// `onNext`.
    pub fn next_tab(&mut self, cx: &mut Context<Self>) {
        let deck = self.deck_project_tabs(cx);
        if let Some(index) = deck.iter().position(|tab| tab.id == self.active_tab_id) {
            let id = deck[(index + 1) % deck.len()].id.clone();
            self.activate_tab(&id, None, cx);
        }
    }

    /// `onPrev`.
    pub fn prev_tab(&mut self, cx: &mut Context<Self>) {
        let deck = self.deck_project_tabs(cx);
        if let Some(index) = deck.iter().position(|tab| tab.id == self.active_tab_id) {
            let id = deck[(index + deck.len() - 1) % deck.len()].id.clone();
            self.activate_tab(&id, None, cx);
        }
    }

    /// `onActivate`: the tab in a slot of the deck; a negative slot is the
    /// last tab.
    pub fn activate_slot(&mut self, slot: i64, cx: &mut Context<Self>) {
        let deck = self.deck_project_tabs(cx);
        let tab = if slot < 0 {
            deck.last()
        } else {
            deck.get(slot as usize)
        };
        if let Some(id) = tab.map(|tab| tab.id.clone()) {
            self.activate_tab(&id, None, cx);
        }
    }

    /// `onVisitBack`.
    pub fn visit_back(&mut self, cx: &mut Context<Self>) {
        self.visit(tab_visit_back, cx);
    }

    /// `onVisitForward`.
    pub fn visit_forward(&mut self, cx: &mut Context<Self>) {
        self.visit(tab_visit_forward, cx);
    }

    fn visit(
        &mut self,
        step: fn(&TabVisitHistory) -> Option<TabVisitHistory>,
        cx: &mut Context<Self>,
    ) {
        let open_ids: HashSet<String> = self.tabs.iter().map(|tab| tab.id.clone()).collect();
        let pruned = prune_tab_visit_history(&self.tab_visit, &open_ids, &self.active_tab_id);
        let Some(next) = step(&pruned) else {
            return;
        };
        if !open_ids.contains(&next.current) {
            return;
        }
        self.tab_visit_from_history = true;
        let current = next.current.clone();
        self.tab_visit = next;
        self.activate_tab(&current, None, cx);
    }

    // Panes.

    /// `onFocusPane`.
    pub fn focus_pane(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.set_dock_focused(false, cx);
        let inbox_ask = {
            let mirror = self.mirror.borrow();
            mirror.inbox_visible && mirror.inbox_session_id.as_deref() == Some(pane_id)
        };
        if inbox_ask {
            self.set_composer_focused(true, cx);
            return;
        }
        let active = self.active_tab_id.clone();
        let pane = pane_id.to_string();
        self.map_tab(&active, cx, |tab| WorkspaceTab {
            focused_id: pane,
            diff_focused: Some(false),
            ..tab.clone()
        });
        let focused = has_session(pane_id, cx);
        self.set_composer_focused(focused, cx);
    }

    /// `onFocusDir`.
    pub fn focus_dir(&mut self, dir: FocusDir, cx: &mut Context<Self>) {
        let Some(active) = self.active_tab() else {
            return;
        };
        if let Some(next) = neighbor_leaf_id(&active.layout, &active.focused_id, dir) {
            self.focus_pane(&next, cx);
        }
    }

    /// `onRatio`.
    pub fn set_ratio(
        &mut self,
        tab_id: &str,
        split_id: &str,
        index: usize,
        ratio: f64,
        cx: &mut Context<Self>,
    ) {
        self.map_tab(tab_id, cx, |tab| WorkspaceTab {
            layout: set_split_ratio(&tab.layout, split_id, index, ratio),
            ..tab.clone()
        });
    }

    /// `onMovePane`.
    pub fn move_pane(
        &mut self,
        from_id: &str,
        to_id: &str,
        edge: PaneEdge,
        cx: &mut Context<Self>,
    ) {
        self.map_tabs(cx, |tab| {
            if leaf_ids(&tab.layout).iter().any(|id| id == from_id) {
                WorkspaceTab {
                    layout: move_pane(&tab.layout, from_id, to_id, edge),
                    focused_id: from_id.to_string(),
                    ..tab.clone()
                }
            } else {
                tab.clone()
            }
        });
    }

    /// `onDetachPane`: move a pane into its own title tab.
    pub fn detach_pane(
        &mut self,
        pane_id: &str,
        target_tab_id: &str,
        position: TitleTabDropPosition,
        cx: &mut Context<Self>,
    ) {
        let place = match position {
            TitleTabDropPosition::Before => PanePlace::Before,
            TitleTabDropPosition::After => PanePlace::After,
        };
        let Some(result) =
            apply_detach_pane_to_tab(&self.tabs, pane_id, target_tab_id, place, None)
        else {
            return;
        };
        self.tabs = result.tabs;
        self.set_dock_focused(false, cx);
        self.activate_tab(&result.active_tab_id, Some(&result.focused_id), cx);
    }

    /// `onPlaceSessionOnPane`: drop a chat onto a pane edge. A blank target
    /// gives up its pane.
    pub fn place_session_on_pane(
        &mut self,
        session_id: &str,
        target_id: &str,
        edge: PaneEdge,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if session_id == target_id {
            return Task::ready(());
        }
        let Some(target_tab) = self
            .tabs
            .iter()
            .find(|tab| leaf_ids(&tab.layout).iter().any(|id| id == target_id))
            .cloned()
        else {
            return Task::ready(());
        };
        let already_here = leaf_ids(&target_tab.layout)
            .iter()
            .any(|id| id == session_id);
        let opening = match (already_here, sessions_entity(cx)) {
            (false, Some(sessions)) => {
                Some(sessions.update(cx, |sessions, cx| sessions.ensure_open(session_id, cx)))
            }
            (false, None) => return Task::ready(()),
            (true, _) => None,
        };
        let session_id = session_id.to_string();
        let target_id = target_id.to_string();
        cx.spawn(async move |this, cx| {
            if let Some(opening) = opening
                && opening.await.is_none()
            {
                return;
            }
            this.update(cx, |this, cx| {
                this.finish_place_session(&session_id, &target_id, &target_tab.id, edge, cx)
            })
            .ok();
        })
    }

    fn finish_place_session(
        &mut self,
        session_id: &str,
        target_id: &str,
        target_tab_id: &str,
        edge: PaneEdge,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.iter().find(|entry| entry.id == target_tab_id) else {
            return;
        };
        let ids = leaf_ids(&tab.layout);
        if !ids.iter().any(|id| id == target_id) {
            return;
        }
        let target = find_session(target_id, cx);
        let replace_target =
            !ids.iter().any(|id| id == session_id) && is_blank_session(target.as_ref());
        if replace_target && let Some(blank) = &target {
            Self::drop_blank_session(blank, false, cx);
        }
        let refs = open_refs(cx);
        let factory = self.factory.clone();
        let project_cwd = self.project_cwd.clone();
        let result =
            apply_place_session_on_pane(
                PlaceSessionOnPane {
                    tabs: &self.tabs,
                    sessions: &refs,
                    session_id,
                    target_id,
                    edge,
                    replace_target,
                    scope: TAB_CLOSE_SCOPE,
                },
                |seed| {
                    let seed_session = seed.and_then(|seed| find_session(&seed.id, cx));
                    let cwd = seed
                        .map(|seed| seed.cwd.clone())
                        .unwrap_or_else(|| project_cwd.clone());
                    OpenRef::created(factory.new_default_session(
                        &cwd,
                        seed_session.map(|session| session.runtime_mode),
                    ))
                },
            );
        let Some(result) = result else {
            return;
        };
        self.apply_placed_sessions(&refs, &result.sessions, cx);
        self.tabs = result.tabs;
        self.active_tab_id = result.active_tab_id;
        self.set_dock_focused(false, cx);
        self.composer_focused = true;
        self.tabs_changed(cx);
    }

    /// Bring `Sessions` in line with a layout helper's session list: add the
    /// sessions it created and drop the ones it removed.
    fn apply_placed_sessions(&self, before: &[OpenRef], after: &[OpenRef], cx: &mut App) {
        let kept: HashSet<&str> = after.iter().map(|entry| entry.id.as_str()).collect();
        let removed: Vec<String> = before
            .iter()
            .filter(|entry| !kept.contains(entry.id.as_str()))
            .map(|entry| entry.id.clone())
            .collect();
        let created: Vec<Session> = after
            .iter()
            .filter_map(|entry| entry.created.as_deref().cloned())
            .collect();
        update_sessions(cx, move |sessions, cx| {
            for id in &removed {
                sessions.remove(id, cx);
            }
            for session in created {
                sessions.insert(session, cx);
            }
        });
    }

    /// `onPlaceTabOnPane`: drop a whole tab onto a pane edge.
    pub fn place_tab_on_pane(
        &mut self,
        source_tab_id: &str,
        target_id: &str,
        edge: PaneEdge,
        cx: &mut Context<Self>,
    ) {
        let Some(target_tab) = self
            .tabs
            .iter()
            .find(|tab| leaf_ids(&tab.layout).iter().any(|id| id == target_id))
        else {
            return;
        };
        if target_tab.id == source_tab_id {
            return;
        }
        let blank_target =
            find_session(target_id, cx).filter(|session| is_blank_session(Some(session)));
        let refs = open_refs(cx);
        let Some(result) = apply_place_tab_on_pane(
            &self.tabs,
            &refs,
            source_tab_id,
            target_id,
            edge,
            blank_target.is_some(),
        ) else {
            return;
        };
        if let Some(blank) = &blank_target {
            Self::drop_blank_session(blank, false, cx);
        }
        self.apply_placed_sessions(&refs, &result.sessions, cx);
        self.tabs = result.tabs;
        self.active_tab_id = result.active_tab_id;
        self.set_dock_focused(false, cx);
        self.composer_focused = result
            .sessions
            .iter()
            .any(|session| session.id == result.focused_id);
        self.tabs_changed(cx);
    }

    /// `onReorderTabs`: the title bar's new order for the visible tabs.
    /// `moved_id` keeps tab groups together.
    pub fn reorder_tabs(&mut self, ids: &[String], moved_id: Option<&str>, cx: &mut Context<Self>) {
        let visible_ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let visible: Vec<WorkspaceTab> = self
            .tabs
            .iter()
            .filter(|tab| visible_ids.contains(tab.id.as_str()))
            .cloned()
            .collect();
        let next = match moved_id {
            Some(moved) => {
                let sessions = all_sessions(cx);
                let tabs = &self.tabs;
                let lookup = |id: &str| {
                    tabs.iter()
                        .find(|tab| tab.id == id)
                        .map(|tab| title_tab_project(tab, &sessions))
                };
                apply_grouped_reorder(&visible, ids, moved, Some(&lookup))
                    .and_then(|reordered| merge_ordered_subset(&self.tabs, &reordered))
            }
            None => merge_ordered_subset(&self.tabs, &order_by_ids(&visible, ids)),
        };
        if let Some(next) = next {
            self.set_tabs(next, cx);
        }
    }

    /// `onReorderFiles`.
    pub fn reorder_files(&mut self, pane_id: &str, ids: &[String], cx: &mut Context<Self>) {
        self.map_tabs(cx, |tab| {
            let Some((kind, _)) = find_surface_pane(tab, pane_id) else {
                return tab.clone();
            };
            let panes = surface_panes(tab, kind)
                .iter()
                .map(|pane| {
                    if pane.id == pane_id {
                        EditorPane {
                            files: order_by_ids(&pane.files, ids),
                            ..pane.clone()
                        }
                    } else {
                        pane.clone()
                    }
                })
                .collect();
            with_surface_panes(tab, kind, panes)
        });
    }

    // Terminals.

    /// `openProjectTerminal`: a new terminal in the current project's dock.
    pub fn open_project_terminal(&mut self, cwd: &str, cx: &mut Context<Self>) -> bool {
        let workdir = if cwd.is_empty() {
            self.project_cwd.clone()
        } else {
            cwd.to_string()
        };
        let project = self.project_cwd.clone();
        let opened = self
            .terminals
            .update(cx, |terminals, cx| terminals.open(&project, &workdir, cx));
        if opened {
            self.focus_project_terminal(cx);
        }
        opened
    }

    /// `onOpenTerminal`: a terminal in the dock, or (when the dock is not
    /// available) as a terminal tab or beside the active tab's panes.
    /// `occupy_session_id` names a blank chat the terminal may replace.
    pub fn open_terminal(
        &mut self,
        cwd: &str,
        as_workspace_tab: bool,
        occupy_session_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let workdir = if cwd.is_empty() {
            self.git_cwd(cx)
        } else {
            cwd.to_string()
        };
        if !is_local_project(&self.project_cwd) || !is_local_project(&workdir) {
            return;
        }
        if self.open_project_terminal(&workdir, cx) {
            return;
        }

        // TODO(port): the dock opens for every local project, so the
        // branches below never run; they are kept as the TypeScript had them.
        let sidebar_cwd = self.sidebar_cwd(cx);
        let Some(active) = self.active_tab().cloned().filter(|_| !as_workspace_tab) else {
            let file = new_terminal_file(&workdir, None, Some(&sidebar_cwd));
            let tab = new_terminal_workspace_tab(file);
            let tab_id = tab.id.clone();
            self.append_tab(tab, Some(&sidebar_cwd), cx);
            self.active_tab_id = tab_id;
            self.composer_focused = false;
            self.tabs_changed(cx);
            return;
        };

        let occupying = find_session(occupy_session_id.unwrap_or(&active.focused_id), cx);
        let occupy_pane_id = occupying
            .as_ref()
            .filter(|session| is_blank_session(Some(session)))
            .map(|session| session.id.clone());
        if let (Some(_), Some(occupying)) = (&occupy_pane_id, &occupying) {
            Self::drop_blank_session(occupying, true, cx);
        }
        let file = new_terminal_file(
            &workdir,
            Some(&next_terminal_title(&active, &workdir)),
            Some(&sidebar_cwd),
        );
        self.map_tab(&active.id, cx, |tab| {
            open_terminal_tab(tab, &file, occupy_pane_id.as_deref())
        });
        self.set_composer_focused(false, cx);
    }

    /// `onNewTerminal`.
    pub fn new_terminal(&mut self, cx: &mut Context<Self>) {
        let cwd = self.git_cwd(cx);
        self.open_terminal(&cwd, false, None, cx);
    }

    /// `onNewTerminalTab`.
    pub fn new_terminal_tab(&mut self, cx: &mut Context<Self>) {
        let cwd = self.git_cwd(cx);
        self.open_terminal(&cwd, true, None, cx);
    }

    /// `onNewTerminalInSession`: a terminal in a chat's working copy.
    pub fn new_terminal_in_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let session = find_session(session_id, cx);
        if session
            .as_ref()
            .is_some_and(|session| session.worktree_removed == Some(true))
        {
            return;
        }
        let cwd = session
            .as_ref()
            .map(|session| session_work_cwd(session).to_string())
            .unwrap_or_else(|| self.project_cwd.clone());
        self.open_terminal(&cwd, false, Some(session_id), cx);
    }

    /// `onShowProjectTerminal`.
    pub fn show_project_terminal(&mut self, cx: &mut Context<Self>) {
        let project = self.project_cwd.clone();
        if self
            .terminals
            .update(cx, |terminals, cx| terminals.show(&project, cx))
        {
            self.focus_project_terminal(cx);
            return;
        }
        let cwd = self.git_cwd(cx);
        self.open_terminal(&cwd, false, None, cx);
    }

    /// `onToggleProjectTerminal`.
    pub fn toggle_project_terminal(&mut self, cx: &mut Context<Self>) {
        let project = self.project_cwd.clone();
        let git_cwd = self.git_cwd(cx);
        self.terminals
            .update(cx, |terminals, cx| terminals.toggle(&project, &git_cwd, cx));
        if self.terminals.read(cx).is_focused() {
            self.set_composer_focused(false, cx);
        }
    }

    /// `onHideProjectTerminal`.
    pub fn hide_project_terminal(&mut self, cx: &mut Context<Self>) {
        let project = self.project_cwd.clone();
        self.terminals
            .update(cx, |terminals, cx| terminals.hide(&project, cx));
    }

    /// `onProjectTerminalSide`. `viewport` is the window size.
    pub fn set_project_terminal_side(
        &mut self,
        side: DockSide,
        viewport: Option<Viewport>,
        cx: &mut Context<Self>,
    ) {
        let project = self.project_cwd.clone();
        self.terminals.update(cx, |terminals, cx| {
            terminals.set_side(&project, side, viewport, cx)
        });
    }

    /// `onProjectTerminalSize` and `commitDockSize` after a drag.
    pub fn set_project_terminal_size(
        &mut self,
        size: f64,
        viewport: Option<Viewport>,
        cx: &mut Context<Self>,
    ) {
        let project = self.project_cwd.clone();
        self.terminals.update(cx, |terminals, cx| {
            terminals.set_size(&project, size, viewport, cx)
        });
    }

    /// `onSelectProjectTerminal`.
    pub fn select_project_terminal(&mut self, file_id: &str, cx: &mut Context<Self>) {
        let project = self.project_cwd.clone();
        self.terminals
            .update(cx, |terminals, cx| terminals.select(&project, file_id, cx));
        self.focus_project_terminal(cx);
    }

    /// `onReorderProjectTerminals`.
    pub fn reorder_project_terminals(&mut self, ids: &[String], cx: &mut Context<Self>) {
        let project = self.project_cwd.clone();
        self.terminals
            .update(cx, |terminals, cx| terminals.reorder(&project, ids, cx));
    }

    /// `onCloseProjectTerminal`: ask when a process still runs, then close.
    pub fn close_project_terminal(&mut self, file_id: &str, cx: &mut Context<Self>) -> Task<()> {
        let Some(file) = self
            .terminals
            .read(cx)
            .dock(&self.project_cwd)
            .and_then(|dock| {
                dock.pane
                    .files
                    .iter()
                    .find(|entry| entry.id == file_id)
                    .cloned()
            })
        else {
            return Task::ready(());
        };
        let confirm =
            confirm_close_terminals(std::slice::from_ref(&file), self.delegate.clone(), cx);
        let file_id = file_id.to_string();
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            this.update(cx, |this, cx| {
                let project = this.project_cwd.clone();
                this.terminals
                    .update(cx, |terminals, cx| terminals.close(&project, &file_id, cx));
            })
            .ok();
        })
    }

    /// `onCloseOtherProjectTerminals`.
    pub fn close_other_project_terminals(
        &mut self,
        file_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let project = self.project_cwd.clone();
        let Some(closing) = self.terminals.read(cx).others_than(&project, file_id) else {
            return Task::ready(());
        };
        let closing_ids: HashSet<String> = closing.iter().map(|file| file.id.clone()).collect();
        let confirm = confirm_close_terminals(&closing, self.delegate.clone(), cx);
        let file_id = file_id.to_string();
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            this.update(cx, |this, cx| {
                this.terminals.update(cx, |terminals, cx| {
                    terminals.close_others(&project, &file_id, &closing_ids, cx)
                });
            })
            .ok();
        })
    }

    /// `onTerminalMetaChange`: a PTY reported a title, cwd, or process.
    pub fn terminal_meta_changed(
        &mut self,
        file_id: &str,
        patch: &TerminalMetaPatch,
        cx: &mut Context<Self>,
    ) {
        self.terminals
            .update(cx, |terminals, cx| terminals.patch(file_id, patch, cx));
        let tabs: Vec<WorkspaceTab> = self
            .tabs
            .iter()
            .map(|tab| update_terminal_tab(tab, file_id, patch))
            .collect();
        if tabs != self.tabs {
            self.set_tabs(tabs, cx);
        }
    }

    /// `onToggleRunningTerminal`: the running-terminal chip shows or hides
    /// the terminal.
    pub fn toggle_running_terminal(&mut self, file_id: &str, cx: &mut Context<Self>) {
        match self
            .terminals
            .update(cx, |terminals, cx| terminals.toggle_running(file_id, cx))
        {
            DockToggle::Hidden => return,
            DockToggle::Shown => {
                self.set_composer_focused(false, cx);
                return;
            }
            DockToggle::NotInDock => {}
        }
        let found = self.tabs.iter().find_map(|tab| {
            tab.terminal_panes
                .iter()
                .find(|pane| pane.files.iter().any(|file| file.id == file_id))
                .map(|pane| (tab.clone(), pane.clone()))
        });
        let Some((tab, pane)) = found else {
            return;
        };
        let showing = self.active_tab_id == tab.id
            && tab.focused_id == pane.id
            && pane.active_file_id == file_id;
        if showing {
            self.set_composer_focused(true, cx);
            self.set_dock_focused(false, cx);
            return;
        }
        self.active_tab_id = tab.id.clone();
        self.map_tab(&tab.id, cx, |entry| {
            let panes = entry
                .terminal_panes
                .iter()
                .map(|item| {
                    if item.id == pane.id {
                        EditorPane {
                            active_file_id: file_id.to_string(),
                            ..item.clone()
                        }
                    } else {
                        item.clone()
                    }
                })
                .collect();
            with_surface_panes(
                &WorkspaceTab {
                    focused_id: pane.id.clone(),
                    ..entry.clone()
                },
                SurfaceKind::Terminal,
                panes,
            )
        });
        self.set_dock_focused(false, cx);
        self.set_composer_focused(false, cx);
    }

    // Files and diffs.

    fn file_index(cx: &App) -> Option<Entity<super::files::FileIndex>> {
        Files::try_global(cx).map(|files| files.index.clone())
    }

    fn remember_opened_file(cwd: &str, path: &str, cx: &mut App) {
        if let Some(index) = Self::file_index(cx) {
            index.update(cx, |index, _| index.remember_opened_file(cwd, path));
        }
    }

    /// `onOpenDiff`: a working-tree or session diff in the active tab.
    pub fn open_diff(
        &mut self,
        path: Option<&str>,
        session: Option<DiffSession>,
        change_kind: Option<GitFileDiffKind>,
        pin: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let diff_cwd = session
            .as_ref()
            .map(|session| session.cwd.clone())
            .unwrap_or_else(|| self.git_cwd(cx));
        let diff_project_cwd = match &session {
            Some(session) => find_session(&session.session_id, cx).map(|entry| entry.cwd),
            None => Some(self.sidebar_cwd(cx)),
        };
        let resolving = match (path, Self::file_index(cx)) {
            (Some(path), Some(index)) => {
                let task = index.update(cx, |index, cx| {
                    index.resolve_openable_path(&diff_cwd, path, cx)
                });
                Some((path.to_string(), Some(task)))
            }
            (Some(path), None) => Some((path.to_string(), None)),
            (None, _) => None,
        };
        let active_tab_id = self.active_tab_id.clone();
        let unified = self.diff_viewer() == DiffViewer::Unified;
        cx.spawn(async move |this, cx| {
            let resolved = match resolving {
                Some((path, Some(task))) => Some(task.await.unwrap_or(path)),
                Some((path, None)) => Some(path),
                None => None,
            };
            this.update(cx, |this, cx| {
                if let Some(resolved) = &resolved {
                    Self::remember_opened_file(&diff_cwd, resolved, cx);
                }
                let project = diff_project_cwd.as_deref();
                this.map_tab(&active_tab_id, cx, |tab| {
                    if let Some(session) = &session {
                        return open_session_changes_tab(
                            tab,
                            &session.cwd,
                            &session.session_id,
                            resolved.as_deref(),
                            project,
                            pin,
                        );
                    }
                    if unified {
                        return open_changes_tab(
                            tab,
                            &diff_cwd,
                            resolved.as_deref(),
                            change_kind,
                            project,
                        );
                    }
                    let Some(resolved) = &resolved else {
                        return tab.clone();
                    };
                    open_editor_tab(
                        tab,
                        &new_file_tab(resolved, &diff_cwd, true, change_kind, project),
                        &OpenEditorTabOptions { split: None, pin },
                    )
                });
                cx.emit(WorkspaceEvent::ShowSidebarTab {
                    tab: SidebarTab::Changes,
                    project: diff_project_cwd.clone(),
                });
                this.set_composer_focused(false, cx);
            })
            .ok();
        })
    }

    /// `onOpenWorkingTreeDiff`.
    pub fn open_working_tree_diff(
        &mut self,
        path: &str,
        kind: Option<GitFileDiffKind>,
        pin: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        self.open_diff(Some(path), None, kind, pin, cx)
    }

    /// `onOpenAllChanges`: every working-tree change in one review,
    /// whatever the diff viewer setting.
    pub fn open_all_changes(&mut self, cx: &mut Context<Self>) {
        let git_cwd = self.git_cwd(cx);
        let sidebar_cwd = self.sidebar_cwd(cx);
        let active = self.active_tab_id.clone();
        self.map_tab(&active, cx, |tab| {
            open_changes_tab(tab, &git_cwd, None, None, Some(&sidebar_cwd))
        });
        self.set_composer_focused(false, cx);
    }

    /// `onOpenCommit`.
    pub fn open_commit(&mut self, commit: CommitTabSource, pin: bool, cx: &mut Context<Self>) {
        let git_cwd = self.git_cwd(cx);
        let sidebar_cwd = self.sidebar_cwd(cx);
        let active = self.active_tab_id.clone();
        self.map_tab(&active, cx, |tab| {
            open_commit_tab(tab, &git_cwd, commit, Some(&sidebar_cwd), pin)
        });
        self.set_composer_focused(false, cx);
    }

    /// `onShowSourceControl` and `onToggleChanges`.
    pub fn show_source_control(&mut self, cx: &mut Context<Self>) {
        cx.emit(WorkspaceEvent::ShowSidebarTab {
            tab: SidebarTab::Changes,
            project: None,
        });
    }

    fn navigate_editor(
        &mut self,
        path: &str,
        navigation: Option<FileNavigation>,
        cx: &mut Context<Self>,
    ) {
        let Some(navigation) = navigation else {
            return;
        };
        self.editor_navigation_token += 1;
        let target = EditorNavigationTarget {
            path: path.to_string(),
            line: navigation.line,
            column: navigation.column,
            token: self.editor_navigation_token,
        };
        self.editor_navigation = Some(target.clone());
        cx.emit(WorkspaceEvent::EditorNavigation(target));
    }

    /// `onOpenFile`: open a file in the active tab, or in its own tab in
    /// the workspace file tab mode.
    pub fn open_file(
        &mut self,
        path: &str,
        navigation: Option<FileNavigation>,
        options: FileOpenOptions,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let file_cwd = self.git_cwd(cx);
        let file_project_cwd = self.sidebar_cwd(cx);
        let resolving = Self::file_index(cx).map(|index| {
            index.update(cx, |index, cx| {
                index.resolve_file_open_request(&file_cwd, path, options.exact, cx)
            })
        });
        let path = path.to_string();
        cx.spawn(async move |this, cx| {
            let resolved = match resolving {
                Some(task) => task.await,
                None => path,
            };
            this.update(cx, |this, cx| {
                this.finish_open_file(
                    &resolved,
                    &file_cwd,
                    &file_project_cwd,
                    navigation,
                    options.pin,
                    cx,
                )
            })
            .ok();
        })
    }

    fn finish_open_file(
        &mut self,
        resolved: &str,
        file_cwd: &str,
        file_project_cwd: &str,
        navigation: Option<FileNavigation>,
        pin: bool,
        cx: &mut Context<Self>,
    ) {
        Self::remember_opened_file(file_cwd, resolved, cx);
        let Some(tab_id) = self
            .tabs
            .iter()
            .find(|entry| entry.id == self.active_tab_id)
            .map(|tab| tab.id.clone())
        else {
            return;
        };
        let file = new_file_tab(resolved, file_cwd, false, None, Some(file_project_cwd));
        if self.file_tab_mode() == FileTabMode::Workspace {
            let created = new_editor_workspace_tab(if pin {
                file.clone()
            } else {
                FilePaneTab {
                    preview: Some(true),
                    ..file.clone()
                }
            });
            let opened = open_workspace_file(
                &self.tabs,
                &file,
                created,
                |tabs, tab| self.insert_beside_active(tabs, tab, Some(file_project_cwd), cx),
                pin,
            );
            self.tabs = opened.tabs;
            match opened.pane_id {
                Some(pane_id) => self.activate_tab(&opened.tab_id, Some(&pane_id), cx),
                None => {
                    self.active_tab_id = opened.tab_id;
                    self.tabs_changed(cx);
                }
            }
            self.set_dock_focused(false, cx);
            self.set_composer_focused(false, cx);
            self.navigate_editor(resolved, navigation, cx);
            return;
        }
        // `focusedSession?.blocks.length === 0`: a chat with no turns yet.
        let focused_blank = self
            .tabs
            .iter()
            .find(|entry| entry.id == tab_id)
            .and_then(|entry| find_session(&entry.focused_id, cx))
            .is_some_and(|session| session.blocks.is_empty());
        self.map_tab(&tab_id, cx, |entry| {
            open_editor_tab(
                entry,
                &file,
                &OpenEditorTabOptions {
                    split: Some(if focused_blank {
                        EditorSplitSide::Left
                    } else {
                        EditorSplitSide::Right
                    }),
                    pin,
                },
            )
        });
        self.navigate_editor(resolved, navigation, cx);
        self.set_composer_focused(false, cx);
    }

    /// `onOpenPlan`: a chat's plan block as a document tab.
    pub fn open_plan(&mut self, session_id: &str, block_id: &str, cx: &mut Context<Self>) {
        let active = self.active_tab_id.clone();
        if !self.tabs.iter().any(|entry| entry.id == active) {
            return;
        }
        let Some(session) = find_session(session_id, cx) else {
            return;
        };
        let Some(block) = session.blocks.iter().find(|entry| entry.id == block_id) else {
            return;
        };
        let mut file = new_plan_tab(
            &session.id,
            &block.id,
            &plan_title(&block.text),
            session_work_cwd(&session),
        );
        if session
            .worktree_cwd
            .as_deref()
            .is_some_and(|cwd| !cwd.is_empty())
        {
            file.project_cwd = Some(session.cwd.clone());
        }
        self.map_tab(&active, cx, |entry| {
            open_editor_tab(entry, &file, &OpenEditorTabOptions::default())
        });
        self.set_composer_focused(false, cx);
    }

    /// `onPinFile`: a preview tab becomes permanent.
    pub fn pin_file(&mut self, file_id: &str, cx: &mut Context<Self>) {
        let tabs: Vec<WorkspaceTab> = self
            .tabs
            .iter()
            .map(|tab| pin_editor_file(tab, file_id))
            .collect();
        if tabs != self.tabs {
            self.set_tabs(tabs, cx);
        }
    }

    /// `onFileDirtyChange`. An edited preview tab is pinned, so the next
    /// click does not replace it.
    pub fn file_dirty_change(&mut self, file_id: &str, dirty: bool, cx: &mut Context<Self>) {
        if dirty {
            self.pin_file(file_id, cx);
        }
        let changed = if dirty {
            self.dirty_files.insert(file_id.to_string())
        } else {
            self.dirty_files.remove(file_id)
        };
        if changed {
            cx.notify();
        }
    }

    /// `onFileErrorCountChange`. The editor reports 0 as it closes, so
    /// closed tabs drop out on their own.
    pub fn file_error_count_change(&mut self, file_id: &str, count: i64, cx: &mut Context<Self>) {
        if self.file_error_counts.get(file_id).copied().unwrap_or(0) == count {
            return;
        }
        if count > 0 {
            self.file_error_counts.insert(file_id.to_string(), count);
        } else {
            self.file_error_counts.remove(file_id);
        }
        cx.notify();
    }

    /// `onSelectFileSurface`: a file tab was clicked.
    pub fn select_file_surface(&mut self, pane_id: &str, file_id: &str, cx: &mut Context<Self>) {
        self.map_tabs(cx, |tab| {
            let Some((kind, _)) = find_surface_pane(tab, pane_id) else {
                return tab.clone();
            };
            let panes = surface_panes(tab, kind)
                .iter()
                .map(|pane| {
                    if pane.id == pane_id {
                        EditorPane {
                            active_file_id: file_id.to_string(),
                            ..pane.clone()
                        }
                    } else {
                        pane.clone()
                    }
                })
                .collect();
            with_surface_panes(
                &WorkspaceTab {
                    focused_id: pane_id.to_string(),
                    ..tab.clone()
                },
                kind,
                panes,
            )
        });
        self.set_composer_focused(false, cx);
    }

    /// `onFileMoved`: open files follow a rename or move.
    pub fn file_moved(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        Files::invalidate_project_files(None, cx);
        self.map_tabs(cx, |tab| WorkspaceTab {
            editor_panes: tab
                .editor_panes
                .iter()
                .map(|pane| EditorPane {
                    files: pane
                        .files
                        .iter()
                        .map(|file| {
                            if is_filesystem_tab(file) {
                                FilePaneTab {
                                    path: rebase_path(&file.path, from, to),
                                    ..file.clone()
                                }
                            } else {
                                file.clone()
                            }
                        })
                        .collect(),
                    ..pane.clone()
                })
                .collect(),
            ..tab.clone()
        });
    }

    /// `onFileDeleted`: close tabs of a deleted file or folder.
    pub fn file_deleted(&mut self, path: &str, cx: &mut Context<Self>) {
        Files::invalidate_project_files(None, cx);
        let mut dropped = Vec::new();
        for tab in &self.tabs {
            for pane in &tab.editor_panes {
                for file in &pane.files {
                    if is_filesystem_tab(file) && is_equal_or_inside(&file.path, path) {
                        dropped.push(file.id.clone());
                    }
                }
            }
        }
        self.map_tabs(cx, |tab| {
            drop_open_files(tab, |file_path| is_equal_or_inside(file_path, path))
        });
        self.forget_dirty(&dropped);
    }

    /// The tab and dock part of `applyProjectLocationChange`: a project
    /// folder moved on disk.
    pub fn project_location_changed(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        update_sessions(cx, |sessions, cx| {
            sessions.update_all(cx, |session| {
                same_project_path(&session.cwd, from).then(|| Session {
                    cwd: to.to_string(),
                    ..session.clone()
                })
            });
        });
        if same_project_path(&self.project_cwd, from) {
            self.project_cwd = to.to_string();
        }
        self.terminals
            .update(cx, |terminals, cx| terminals.rebase_project(from, to, cx));
        self.file_moved(from, to, cx);
        Files::notify_dirs_changed(cx);
    }

    // Chats from other places.

    /// `openSessionForAddToChat`: a new chat seeded with a context chip,
    /// unless a session pane can take the chip itself.
    pub fn add_to_chat(
        &mut self,
        item: &ChatContextItem,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let sessions = all_sessions(cx);
        let defaults = self.session_defaults(cx);
        let result = apply_add_to_chat_request(
            AddToChatRequest {
                sessions: &sessions,
                tabs: &self.tabs,
                active_tab_id: Some(&self.active_tab_id),
                project_cwd: &self.project_cwd,
                fallback_cwd: defaults.as_ref().map(|session| session.cwd.as_str()),
                default_runtime_mode: defaults.as_ref().map(|session| session.runtime_mode),
                item,
            },
            self.factory.as_ref(),
        )?;
        let known: HashSet<&str> = sessions.iter().map(|session| session.id.as_str()).collect();
        let created: Vec<Session> = result
            .sessions
            .into_iter()
            .filter(|session| !known.contains(session.id.as_str()))
            .collect();
        self.tabs = result.tabs;
        self.active_tab_id = result.active_tab_id;
        self.set_dock_focused(false, cx);
        self.composer_focused = true;
        self.tabs_changed(cx);
        update_sessions(cx, |sessions, cx| {
            for session in created {
                sessions.insert(session, cx);
            }
        });
        Some(result.session_id)
    }

    /// `removeSessionFromWorkspace` over this window: what archiving or
    /// deleting a chat would do to the tabs. Nothing changes until
    /// `apply_session_removal`.
    pub fn plan_session_removal(&self, session_id: &str, cx: &App) -> SessionWorkspaceRemoval {
        let sessions = all_sessions(cx);
        let latest = sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned();
        let factory = self.factory.clone();
        let project_cwd = self.project_cwd.clone();
        remove_session_from_workspace(
            RemoveSession {
                tabs: &self.tabs,
                sessions: &sessions,
                session_id,
                active_tab_id: &self.active_tab_id,
                scope: TAB_CLOSE_SCOPE,
            },
            |seed| {
                let seed = seed.or(latest.as_ref());
                let cwd = seed.map_or(project_cwd.as_str(), |seed| seed.cwd.as_str());
                session_seeded_from(factory.as_ref(), seed, cwd)
            },
            |_| true,
        )
    }

    /// Commit a planned removal: the new tabs, the replacement chats, and
    /// the removed chat gone from `Sessions`.
    pub fn apply_session_removal(
        &mut self,
        session_id: &str,
        removal: SessionWorkspaceRemoval,
        cx: &mut Context<Self>,
    ) {
        let known: HashSet<String> = all_sessions(cx)
            .into_iter()
            .map(|session| session.id)
            .collect();
        let created: Vec<Session> = removal
            .sessions
            .into_iter()
            .filter(|session| !known.contains(&session.id))
            .collect();
        self.tabs = removal.tabs;
        self.active_tab_id = removal.active_tab_id;
        self.tabs_changed(cx);
        let session_id = session_id.to_string();
        update_sessions(cx, move |sessions, cx| {
            sessions.remove(&session_id, cx);
            for session in created {
                sessions.insert(session, cx);
            }
        });
    }

    /// Apply the layout history computed after it updated the session store.
    #[cfg(feature = "history")]
    pub fn apply_history_removal(
        &mut self,
        removal: crate::history::session_workspace_lifecycle::SessionWorkspaceRemoval,
        cx: &mut Context<Self>,
    ) {
        let open_files: HashSet<String> = removal
            .tabs
            .iter()
            .flat_map(|tab| tab.editor_panes.iter().chain(&tab.terminal_panes))
            .flat_map(|pane| &pane.files)
            .map(|file| file.id.clone())
            .collect();
        self.dirty_files.retain(|id| open_files.contains(id));
        self.file_error_counts
            .retain(|id, _| open_files.contains(id));
        self.tabs = removal.tabs;
        self.active_tab_id = removal.active_tab_id;
        self.tabs_changed(cx);
    }

    /// `collectWindowTransfer` for these tabs: what a new window needs to
    /// show them, with the docks of projects that leave entirely. Nothing
    /// changes here; the caller closes the tabs once the window opened.
    pub fn window_transfer(
        &self,
        tab_ids: &[String],
        cx: &App,
    ) -> Option<WindowTransferPayload<WorkspaceTab, ProjectTerminalDock>> {
        let sessions = all_sessions(cx);
        let (moving, remaining): (Vec<WorkspaceTab>, Vec<WorkspaceTab>) = self
            .tabs
            .iter()
            .cloned()
            .partition(|tab| tab_ids.contains(&tab.id));
        let docks = monocode_layout::project_terminal::split_project_terminals_for_move(
            self.terminals.read(cx).docks(),
            &moving,
            &remaining,
            &sessions,
        )
        .0;
        collect_window_transfer(
            &self.tabs,
            &sessions,
            tab_ids,
            &self.active_tab_id,
            &self.dirty_files,
            &self.project_cwd,
            &docks,
        )
    }

    /// The new chat a fresh window or a reset tab starts with.
    pub fn new_default_session(&self, cwd: &str, runtime_mode: Option<RuntimeMode>) -> Session {
        self.factory.new_default_session(cwd, runtime_mode)
    }
}

/// `confirmCloseTerminals`: ask before closing terminals that still run a
/// process other than the shell. Resolves to `true` when nothing runs.
pub fn confirm_close_terminals(
    files: &[FilePaneTab],
    delegate: Rc<dyn WorkspaceDelegate>,
    cx: &mut App,
) -> Task<bool> {
    let terminal_files: Vec<FilePaneTab> = files
        .iter()
        .filter(|file| file.terminal == Some(true))
        .cloned()
        .collect();
    let Some(terminals) = Terminals::try_global(cx).filter(|_| !terminal_files.is_empty()) else {
        return Task::ready(true);
    };
    let lookup = terminals.foregrounds(
        terminal_files.iter().map(|file| file.id.clone()).collect(),
        cx,
    );
    cx.spawn(async move |cx| {
        let foregrounds = lookup.await;
        // A terminal whose PTY is gone has nothing to confirm.
        let running =
            running_terminals(&terminal_files, |id| foregrounds.get(id).cloned().flatten());
        let Some(prompt) = close_terminals_prompt(&running) else {
            return true;
        };
        let confirm = cx.update(|cx| delegate.confirm(&prompt, CONFIRM_LABEL, cx));
        confirm.await
    })
}
