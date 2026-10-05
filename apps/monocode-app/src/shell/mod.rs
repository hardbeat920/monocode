//! The app shell. Port of the layout in src/app/App.tsx (the root around
//! lines 10545-11078) and the chrome in src/app/shell: the project rail or
//! the compact rail, the session sidebar, and the main column with the
//! title bar, the workspace or a full page, and the usage footer.
//!
//! `Shell` is the window's root view. It owns the window's `Workspace` and
//! `History` once the boot restore finishes, the layout state the regions
//! share ([`ShellLayout`]), and the actions every region calls. Each region
//! is its own entity (`ProjectRail`, `SessionSidebar`, `TitleBar`) that
//! holds a `WeakEntity<Shell>`: it reads the layout in its render and calls
//! the shell's methods from its handlers, the way the React regions read
//! props and called App.tsx callbacks.

mod actions;
mod footer;
mod github_star;
mod glass_backdrop;
mod hosts;
mod launch_host;
mod live_agents;
mod main_pane;
mod menu_bar;
mod packages;
mod preload;
mod project_menu;
mod project_picker;
mod project_rail;
mod projects;
mod rail_action;
mod session_actions;
mod settings_rail;
mod shortcuts;
mod sidebar;
mod sidebar_update;
#[cfg(test)]
mod tests;
pub(crate) mod title_bar;
mod update_rail_card;
pub(crate) mod whats_new;

// The keymap, menus, and windows sit at the top of `src/` but compile as
// part of the shell, because `main.rs` belongs to the boot code.
#[path = "../keymap.rs"]
pub mod keymap;
#[path = "../menus.rs"]
pub mod menus;
#[path = "../windows.rs"]
pub mod windows;

use std::collections::HashMap;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, WeakEntity, Window, div,
};
use monocode_app::boot::{self, AppServices};
use monocode_app::bridge::ActiveWorkspace;
use monocode_core::appearance::SidebarTabId;
use monocode_engine::attention::{Attention, AttentionFocus};
use monocode_engine::history::{History, HistoryPackage};
use monocode_engine::runtime::Engine;
use monocode_engine::runtime::util::project_path::same_project_path;
use monocode_engine::workspace::{Workspace, WorkspaceConfig};
use monocode_layout::leaf_ids;
use monocode_ui::appearance::{
    PROJECT_RAIL_WIDTH_DEFAULT, PROJECT_RAIL_WIDTH_MAX, PROJECT_RAIL_WIDTH_MIN,
    SESSION_SIDEBAR_WIDTH_DEFAULT, SESSION_SIDEBAR_WIDTH_MAX, SESSION_SIDEBAR_WIDTH_MIN,
};
use monocode_ui::widgets::toast_stack;
use monocode_ui::{Theme, UiStyled as _, u};

use crate::file_pane::FilePane;
use crate::session_pane::SessionPane;
use crate::slots::{AppSlots, Page};

use project_rail::ProjectRail;
use sidebar::{CompactRail, SessionSidebar};
use title_bar::TitleBar;

/// Bind the keymap, install the app-level actions, and set the native menu
/// bar. Call once at startup, after `monocode_ui::init`.
pub fn init(cx: &mut App) {
    keymap::init(cx);
    windows::init(cx);
    menus::init(cx);
}

/// The session sidebar's tabs, `SidebarTabId` in appearance.ts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SidebarTab {
    Sessions,
    Inbox,
    Files,
    Changes,
}

impl SidebarTab {
    /// `DEFAULT_SIDEBAR_TAB_ORDER`.
    pub const ORDER: [SidebarTab; 4] = [Self::Sessions, Self::Inbox, Self::Files, Self::Changes];

    /// `TAB_LABELS` in Sidebar.tsx.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Inbox => "Inbox",
            Self::Files => "Explorer",
            Self::Changes => "Changes",
        }
    }

    pub fn id(self) -> SidebarTabId {
        match self {
            Self::Sessions => SidebarTabId::Sessions,
            Self::Inbox => SidebarTabId::Inbox,
            Self::Files => SidebarTabId::Files,
            Self::Changes => SidebarTabId::Changes,
        }
    }
}

impl From<SidebarTabId> for SidebarTab {
    fn from(id: SidebarTabId) -> Self {
        match id {
            SidebarTabId::Sessions => Self::Sessions,
            SidebarTabId::Inbox => Self::Inbox,
            SidebarTabId::Files => Self::Files,
            SidebarTabId::Changes => Self::Changes,
        }
    }
}

/// Which pane edge a resize drag moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizeTarget {
    ProjectRail,
    SessionSidebar,
}

#[derive(Clone, Copy, Debug)]
struct Resize {
    target: ResizeTarget,
    start_x: Pixels,
    start_width: f32,
}

/// How a window's workspace starts.
#[derive(Clone, Debug, Default)]
pub enum WorkspaceStart {
    /// The workspace the last quit saved (the first window).
    #[default]
    Restore,
    /// A new window: one new chat in `project`, or the last project.
    Fresh { project: Option<String> },
}

/// How the shell starts, for `--view` variants and the command line.
#[derive(Clone, Debug, Default)]
pub struct ShellOptions {
    pub project_rail_open: bool,
    pub compact_rail: bool,
    pub session_sidebar_open: bool,
    /// Opens the session context menu at this point, for screenshots.
    pub demo_menu: Option<(f32, f32)>,
    /// Open this session once the workspace restores (`--open-session`).
    pub open_session: Option<String>,
    pub start: WorkspaceStart,
}

impl ShellOptions {
    pub fn full() -> Self {
        Self {
            project_rail_open: true,
            compact_rail: false,
            session_sidebar_open: true,
            demo_menu: None,
            open_session: None,
            start: WorkspaceStart::Restore,
        }
    }

    pub fn saved(cx: &App) -> Self {
        AppServices::try_global(cx)
            .map(|services| Self::from_preferences(&services.kv))
            .unwrap_or_else(Self::full)
    }

    fn from_preferences(kv: &monocode_settings::Kv) -> Self {
        let preferences =
            monocode_settings::load_app_settings(kv, monocode_core::Platform::current());
        Self {
            project_rail_open: preferences.appearance.project_rail_open,
            compact_rail: preferences.settings.collapsed_project_rail_mode
                == monocode_core::settings::CollapsedProjectRailMode::Compact,
            session_sidebar_open: preferences.appearance.session_sidebar_open,
            ..Self::full()
        }
    }
}

struct RememberedSidebarWidth(f32);
impl gpui::Global for RememberedSidebarWidth {}

/// Set once by `main` for `--open-session`, read by the shell builders.
pub struct StartupSession(pub Option<String>);

impl gpui::Global for StartupSession {}

/// The layout state the regions share. App.tsx kept these in React state
/// and passed them down as props.
#[derive(Clone, Debug)]
pub struct ShellLayout {
    /// `projectRailOpen`.
    pub project_rail_open: bool,
    /// `collapsedProjectRailMode === "compact"`: show the 48px rail when the
    /// project rail is closed.
    pub compact_rail: bool,
    /// `sessionSidebarOpen`.
    pub session_sidebar_open: bool,
    pub rail_width: f32,
    pub sidebar_width: f32,
    /// The sidebar's visible tab.
    pub sidebar_tab: SidebarTab,
    /// The full page over the workspace (`searchViewOpen`, `inboxViewOpen`,
    /// `notesViewOpen`, `automationsViewOpen`, `settingsOpen`).
    pub page: Option<Page>,
    /// `settingsReturnViewRef`: the page Settings replaced, shown again when
    /// Settings closes.
    pub settings_return: Option<Page>,
}

impl ShellLayout {
    /// `compactRailVisible`.
    pub fn compact_rail_visible(&self) -> bool {
        self.compact_rail && !self.project_rail_open
    }

    /// `compactTitleBar`: on macOS the compact rail moves the title bar
    /// above everything, so the traffic lights sit in it.
    pub fn compact_title_bar(&self) -> bool {
        cfg!(target_os = "macos") && self.compact_rail_visible()
    }
}

pub struct Shell {
    focus: FocusHandle,
    pub(crate) layout: ShellLayout,
    sidebar_project: String,
    window_label: String,
    resize: Option<Resize>,
    /// Armed by a press on a drag region; the first move hands the drag to
    /// the window manager.
    drag_armed: bool,
    rail: Entity<ProjectRail>,
    compact_rail: Entity<CompactRail>,
    sidebar: Entity<SessionSidebar>,
    title_bar: Entity<TitleBar>,
    live_agents: Entity<live_agents::LiveAgentsArea>,
    /// The window's workspace, once the boot restore is done.
    pub(crate) workspace: Option<Entity<Workspace>>,
    /// The window's history: the package's for the first window, a new one
    /// for each later window.
    pub(crate) history: Option<Entity<History>>,
    usage_footer: Option<Entity<monocode_view_settings::accounts::UsageFooter>>,
    file_picker: Option<Entity<monocode_view_files::FilePicker>>,
    picker_subscription: Option<Subscription>,
    project_menu: Option<Entity<monocode_view_workbench::panes::tab_group_menu::TabGroupMenu>>,
    project_menu_subscription: Option<Subscription>,
    project_menu_return_focus: Option<gpui::FocusHandle>,
    project_picker: Option<Entity<project_picker::ProjectPicker>>,
    project_picker_subscription: Option<Subscription>,
    project_picker_return_focus: Option<gpui::FocusHandle>,
    project_dialog: Option<gpui::AnyView>,
    project_dialog_subscription: Option<Subscription>,
    project_dialog_return_focus: Option<gpui::FocusHandle>,
    session_panes: HashMap<String, Entity<SessionPane>>,
    file_panes: HashMap<String, Entity<FilePane>>,
    open_session: Option<String>,
    start: WorkspaceStart,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Shell {
    pub fn new(options: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        packages::ensure(cx);
        let rail_width = AppServices::try_global(cx)
            .map(|services| {
                monocode_settings::load_app_settings(
                    &services.kv,
                    monocode_core::Platform::current(),
                )
                .appearance
                .project_rail_width as f32
            })
            .filter(|width| *width > 0.0)
            .unwrap_or(PROJECT_RAIL_WIDTH_DEFAULT)
            .clamp(PROJECT_RAIL_WIDTH_MIN, PROJECT_RAIL_WIDTH_MAX);
        let shell = cx.weak_entity();
        let rail = cx.new(|cx| ProjectRail::new(shell.clone(), window, cx));
        let compact_rail = cx.new(|cx| CompactRail::new(shell.clone(), window, cx));
        let sidebar =
            cx.new(|cx| SessionSidebar::new(shell.clone(), options.demo_menu, window, cx));
        let title_bar = cx.new(|cx| TitleBar::new(shell.clone(), window, cx));
        let live_agents = cx.new(|cx| live_agents::LiveAgentsArea::new(shell.clone(), cx));
        let mut this = Self {
            focus: cx.focus_handle().tab_stop(false),
            sidebar_project: "~".into(),
            window_label: windows::window_label(window.window_handle().window_id()),
            layout: ShellLayout {
                project_rail_open: options.project_rail_open,
                compact_rail: options.compact_rail,
                session_sidebar_open: options.session_sidebar_open,
                rail_width,
                sidebar_width: cx
                    .try_global::<RememberedSidebarWidth>()
                    .map(|width| width.0)
                    .unwrap_or(SESSION_SIDEBAR_WIDTH_DEFAULT),
                sidebar_tab: SidebarTab::Sessions,
                page: None,
                settings_return: None,
            },
            resize: None,
            drag_armed: false,
            rail,
            compact_rail,
            sidebar,
            title_bar,
            live_agents,
            workspace: None,
            history: None,
            usage_footer: None,
            file_picker: None,
            picker_subscription: None,
            project_menu: None,
            project_menu_subscription: None,
            project_menu_return_focus: None,
            project_picker: None,
            project_picker_subscription: None,
            project_picker_return_focus: None,
            project_dialog: None,
            project_dialog_subscription: None,
            project_dialog_return_focus: None,
            session_panes: HashMap::new(),
            file_panes: HashMap::new(),
            open_session: options.open_session.or_else(|| {
                cx.try_global::<StartupSession>()
                    .and_then(|startup| startup.0.clone())
            }),
            start: options.start,
            _subscriptions: Vec::new(),
            _tasks: Vec::new(),
        };
        this.start(window, cx);
        if window.focused(cx).is_none() {
            this.focus.focus(window, cx);
        }
        this._subscriptions
            .push(cx.on_focus_lost(window, |this, window, cx| {
                this.focus.focus(window, cx);
            }));
        // `onFocusChanged`: banners and the Dock badge follow window focus,
        // and the focused window's workspace takes app-level calls.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            windows::sync_window_visibility(window, cx);
            let active = window.is_window_active();
            if active && let Some(workspace) = &this.workspace {
                ActiveWorkspace::set(workspace.downgrade(), cx);
                hosts::install(this.history.clone(), cx);
                if let Some(package) =
                    monocode_engine::automations::AutomationsPackage::try_global(cx)
                {
                    let host = std::rc::Rc::new(launch_host::WindowLaunchHost::new(
                        cx.weak_entity(),
                        window,
                    ));
                    let automations = package.automations.clone();
                    automations.update(cx, |automations, cx| {
                        automations.set_host(host);
                        automations.window_visible(cx);
                    });
                }
                workspace.update(cx, |workspace, cx| workspace.window_focused(cx));
                if let Some(package) =
                    monocode_engine::automations::AutomationsPackage::try_global(cx)
                {
                    let (quick, reminders) =
                        (package.quick_launch.clone(), package.reminders.clone());
                    quick.update(cx, |quick, cx| quick.window_focused(&this.window_label, cx));
                    reminders.update(cx, |reminders, cx| {
                        reminders.window_focused(&this.window_label, cx)
                    });
                }
                if let Some(projects) = monocode_engine::projects::ProjectsGlobal::try_global(cx) {
                    let git = projects.git.clone();
                    git.update(cx, |git, cx| git.window_focused(cx));
                }
                if let Some(inbox) = monocode_engine::inbox::inbox::Inbox::try_global(cx) {
                    inbox.update(cx, |inbox, cx| inbox.window_became_visible(cx));
                }
            }
            if Attention::try_global(cx).is_some() {
                Attention::set_window_focused(cx, active);
            }
        });
        this._subscriptions.push(activation);
        this._subscriptions
            .push(cx.observe_window_bounds(window, |_, window, cx| {
                windows::sync_window_visibility(window, cx);
            }));
        if let Some(submit) = monocode_engine::submit::Submit::try_global(cx) {
            this._subscriptions.push(cx.subscribe_in(
                &submit,
                window,
                |this, _, event: &monocode_engine::submit::SubmitEvent, window, cx| {
                    let monocode_engine::submit::SubmitEvent::AddToChat(item) = event;
                    let item = workspace_chat_context(item);
                    let active = ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade());
                    let Some(workspace) = this.workspace.clone().filter(|workspace| {
                        active
                            .as_ref()
                            .is_some_and(|active| active.entity_id() == workspace.entity_id())
                    }) else {
                        return;
                    };
                    let existing = Engine::sessions(cx)
                        .read(cx)
                        .all()
                        .iter()
                        .map(|session| session.id.clone())
                        .collect::<std::collections::HashSet<_>>();
                    let target =
                        workspace.update(cx, |workspace, cx| workspace.add_to_chat(&item, cx));
                    if let Some(target) =
                        target.as_ref().filter(|target| existing.contains(*target))
                    {
                        let area = crate::panes::workspace_area(window, cx);
                        area.update(cx, |area, cx| {
                            area.add_to_chat_to(target, &item, window, cx)
                        });
                    } else if target.is_none() {
                        let area = crate::panes::workspace_area(window, cx);
                        area.update(cx, |area, cx| area.add_to_chat(&item, window, cx));
                    }
                },
            ));
        }
        let requests = monocode_app::bridge::shell::ShellRequests::entity(cx);
        this._subscriptions.push(cx.subscribe_in(&requests, window, |this, _, request: &monocode_app::bridge::shell::ShellRequest, window, cx| {
            use monocode_app::bridge::shell::{ShellRequest, ShellPage};
            if let ShellRequest::SetCompactRail(compact) = request {
                this.set_compact_rail(*compact, cx);
                return;
            }
            let active = ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade());
            if this.workspace.as_ref().zip(active.as_ref()).is_none_or(|(a, b)| a.entity_id() != b.entity_id()) { return; }
            let page = |page: ShellPage| match page {
                ShellPage::Search => Page::Search, ShellPage::Inbox => Page::Inbox,
                ShellPage::Notes => Page::Notes, ShellPage::Automations => Page::Automations,
                ShellPage::Settings => Page::Settings,
            };
            match request {
                ShellRequest::ClosePages => this.close_page(cx),
                ShellRequest::ClosePage(requested) => if this.layout.page == Some(page(*requested)) { this.close_page(cx) },
                ShellRequest::OpenPage(requested) => this.open_page(page(*requested), cx),
                ShellRequest::ShowSessions { cwd } => {
                    this.close_page(cx); this.set_sidebar_tab(SidebarTab::Sessions, cx);
                    if let Some(cwd) = cwd { this.select_project(cwd, cx); }
                }
                ShellRequest::BringForward => windows::bring_forward(window, cx),
                ShellRequest::HideWindow => windows::hide_window(window, cx),
                ShellRequest::SetCompactRail(_) => {},
                ShellRequest::ProjectSidebarRemoved(path) => {
                    if same_project_path(&this.sidebar_project, path) {
                        this.sidebar_project = "~".into();
                        if let Some(services) = AppServices::try_global(cx) {
                            this.layout.sidebar_tab = monocode_engine::projects::project_sidebar_tab::load_project_sidebar_tab(&services.kv, "~").into();
                        }
                        cx.notify();
                    }
                }
                ShellRequest::ProjectSidebarMoved { from, to } => {
                    if same_project_path(&this.sidebar_project, from) { this.sidebar_project = to.clone(); cx.notify(); }
                },
            }
        }));
        this
    }

    /// Start the window's workspace: the saved one for the first window, a
    /// fresh one for a new window. Then follow it.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(services) = AppServices::try_global(cx) else {
            return;
        };
        match self.start.clone() {
            WorkspaceStart::Restore => {
                let restore = boot::restore_workspace(cx);
                let task = cx.spawn_in(window, async move |this, cx| {
                    let restored = restore.await;
                    this.update_in(cx, |this, window, cx| {
                        let history = HistoryPackage::history(cx);
                        history.update(cx, |history, cx| {
                            history.set_boot_rows(
                                restored.history,
                                restored.history_cwd.as_deref(),
                                cx,
                            )
                        });
                        this.attach(restored.config, history, window, cx);
                    })
                    .ok();
                });
                self._tasks.push(task);
            }
            WorkspaceStart::Fresh { project } => {
                let kv = services.kv.clone();
                let project =
                    project.or_else(|| monocode_engine::projects::recents::last_project_path(&kv));
                let mut config = WorkspaceConfig::fresh(project.as_deref());
                config.kv = Some(kv.clone());
                let history = cx.new(|cx| History::new(kv, cx));
                self.attach(config, history, window, cx);
            }
        }
    }

    /// Take the window's workspace and history and follow them.
    fn attach(
        &mut self,
        config: WorkspaceConfig,
        history: Entity<History>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = cx.new(|cx| Workspace::new(config, cx));
        ActiveWorkspace::set(workspace.downgrade(), cx);
        crate::slots::register_workspace(workspace.clone(), window, cx);
        self._subscriptions
            .push(cx.observe_in(&workspace, window, |this, _, window, cx| {
                this.workspace_changed(window, cx)
            }));
        self._subscriptions
            .push(cx.observe(&history, |_, _, cx| cx.notify()));
        let sessions = Engine::sessions(cx);
        self._subscriptions
            .push(cx.observe(&sessions, |_, _, cx| cx.notify()));
        if let Some(attention) = Attention::try_global(cx) {
            let approvals = attention.approvals.clone();
            let notifier = attention.notifier.clone();
            self._subscriptions
                .push(cx.observe(&approvals, |_, _, cx| cx.notify()));
            self._subscriptions
                .push(cx.observe(&notifier, |_, _, cx| cx.notify()));
        }
        self.workspace = Some(workspace.clone());
        self.history = Some(history);
        launch_host::attach(cx.weak_entity(), window, &self.window_label, cx);
        hosts::install(self.history.clone(), cx);
        if let Some(id) = self.open_session.take() {
            workspace
                .update(cx, |workspace, cx| workspace.open_session(&id, cx))
                .detach();
        }
        self.workspace_changed(window, cx);
        preload::after_paint(window, cx, Self::preload_navigation);
    }

    /// The workspace moved: show its project in the sidebar, tell attention
    /// which session is on screen, and drop panes no tab shows anymore.
    fn workspace_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let (cwd, active, tabs) = {
            let workspace = workspace.read(cx);
            (
                workspace.sidebar_cwd(cx),
                workspace.active_session(cx).map(|session| session.id),
                workspace.tabs().to_vec(),
            )
        };
        if !same_project_path(&self.sidebar_project, &cwd) {
            self.sidebar_project = cwd.clone();
            if let Some(services) = AppServices::try_global(cx) {
                self.layout.sidebar_tab =
                    monocode_engine::projects::project_sidebar_tab::load_project_sidebar_tab(
                        &services.kv,
                        &cwd,
                    )
                    .into();
            }
        }
        if let Some(history) = &self.history {
            history.update(cx, |history, cx| history.set_sidebar_cwd(&cwd, cx));
        }
        self.sync_sessions_tab_active(cx);
        if Attention::try_global(cx).is_some() {
            Attention::set_focus(
                cx,
                AttentionFocus {
                    active_session_id: active.clone(),
                    ..Default::default()
                },
            );
        }
        if let Some(package) = monocode_engine::automations::AutomationsPackage::try_global(cx) {
            let reminders = package.reminders.clone();
            let ids = tabs.iter().flat_map(|tab| leaf_ids(&tab.layout)).collect();
            reminders.update(cx, |reminders, _| {
                reminders.register_window(&self.window_label, ids)
            });
        }
        let mut live: std::collections::HashSet<String> = std::collections::HashSet::new();
        for tab in &tabs {
            live.extend(leaf_ids(&tab.layout));
            live.extend(tab.editor_panes.iter().map(|pane| pane.id.clone()));
            live.extend(tab.terminal_panes.iter().map(|pane| pane.id.clone()));
        }
        self.session_panes.retain(|id, _| live.contains(id));
        self.file_panes.retain(|id, _| live.contains(id));
        if AppSlots::get(cx).workspace.is_none()
            && workspace.read(cx).composer_focused()
            && let Some(id) = active
        {
            let pane = self.session_pane(&id, window, cx);
            pane.update(cx, |pane, cx| pane.focus_composer(window, cx));
        }
        cx.notify();
    }

    /// The pane entity for a session leaf, created on first use.
    fn session_pane(
        &mut self,
        session_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SessionPane> {
        if let Some(pane) = self.session_panes.get(session_id) {
            return pane.clone();
        }
        let workspace = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.downgrade())
            .unwrap_or_else(WeakEntity::new_invalid);
        let id = session_id.to_string();
        let pane = cx.new(|cx| SessionPane::new(id, workspace, window, cx));
        self.session_panes
            .insert(session_id.to_string(), pane.clone());
        pane
    }

    /// The pane entity for an editor or terminal surface, created on first use.
    fn file_pane(
        &mut self,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<FilePane>> {
        if let Some(pane) = self.file_panes.get(pane_id) {
            return Some(pane.clone());
        }
        let workspace = self.workspace.as_ref()?.downgrade();
        let id = pane_id.to_string();
        let pane = cx.new(|cx| FilePane::new(id, workspace, window, cx));
        self.file_panes.insert(pane_id.to_string(), pane.clone());
        Some(pane)
    }

    // What the regions read.

    #[cfg(test)]
    pub fn layout(&self) -> &ShellLayout {
        &self.layout
    }

    #[cfg(test)]
    pub fn workspace(&self) -> Option<&Entity<Workspace>> {
        self.workspace.as_ref()
    }

    #[cfg(test)]
    pub fn history(&self) -> Option<&Entity<History>> {
        self.history.as_ref()
    }

    /// The project the sidebar shows (`sidebarCwd`).
    pub fn sidebar_cwd(&self, cx: &App) -> String {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).sidebar_cwd(cx))
            .unwrap_or_else(|| "~".to_string())
    }

    // Actions the regions call.

    /// `onSelectHistorySession`: open a sidebar card's session.
    pub fn open_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.close_page(cx);
        if let Some(history) = self.history.clone() {
            history
                .update(cx, |history, cx| history.select_session(session_id, cx))
                .detach();
        } else if let Some(workspace) = &self.workspace {
            workspace
                .update(cx, |workspace, cx| workspace.open_session(session_id, cx))
                .detach();
        }
    }

    /// `onNew`: a new chat with the default model in a new tab, in the
    /// current project. Returns the new session's id.
    pub fn new_session(&mut self, cx: &mut Context<Self>) -> Option<String> {
        self.close_page(cx);
        let workspace = self.workspace.clone()?;
        workspace.update(cx, |workspace, cx| {
            workspace.new_session_tab(cx);
        });
        workspace
            .read(cx)
            .active_session(cx)
            .map(|session| session.id)
    }

    /// Restore the project's selected tab through the projects package.
    pub fn select_project(&mut self, path: &str, cx: &mut Context<Self>) {
        self.close_page(cx);
        if self.workspace.is_some() {
            monocode_engine::projects::actions::on_select_project(path, cx);
        }
    }

    pub fn activate_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        self.close_page(cx);
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |workspace, cx| workspace.activate_tab(tab_id, None, cx));
        }
    }

    pub fn close_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace
                .update(cx, |workspace, cx| workspace.close_title_tab(tab_id, cx))
                .detach();
        }
    }

    /// `onVisitBack`.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |workspace, cx| workspace.visit_back(cx));
        }
    }

    /// `onVisitForward`.
    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |workspace, cx| workspace.visit_forward(cx));
        }
    }

    /// `(canGoBack, canGoForward)`.
    pub fn visit_nav(&self, cx: &App) -> (bool, bool) {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).tab_visit_nav())
            .unwrap_or((false, false))
    }

    /// Open a full page over the workspace, closing any other.
    pub fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.layout.page != Some(page) {
            if page == Page::Settings {
                self.layout.settings_return = self.layout.page;
            }
            self.close_page(cx);
            self.layout.page = Some(page);
            if let Some(package) = HistoryPackage::try_global(cx) {
                match page {
                    Page::Notes => {
                        let notes = package.notes.clone();
                        notes.update(cx, |notes, cx| notes.open_page(cx));
                    }
                    Page::Search => {
                        let search = package.search.clone();
                        let cwd = self.sidebar_cwd(cx);
                        let recents = monocode_engine::projects::ProjectsGlobal::try_global(cx)
                            .map(|p| {
                                p.projects
                                    .read(cx)
                                    .recents()
                                    .iter()
                                    .map(|project| project.path.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        search.update(cx, |search, cx| search.open(&cwd, recents, cx));
                    }
                    _ => {}
                }
            }
            if let Some(workspace) = &self.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.set_full_page_open(true, cx);
                    workspace.set_inbox_visible(page == Page::Inbox, cx);
                });
            }
            cx.notify();
        }
    }

    /// Open `page`, or close it when it is already open.
    pub fn toggle_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.layout.page != Some(page) {
            self.open_page(page, cx);
        } else if page == Page::Settings {
            self.close_settings(cx);
        } else {
            self.close_page(cx);
        }
    }

    /// `onCloseSettings`: leave Settings for the page it replaced, or for
    /// the workspace. Notes stays closed once Settings turned it off.
    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        if self.layout.page != Some(Page::Settings) {
            return;
        }
        let back = self.layout.settings_return.take();
        self.close_page(cx);
        let back = back.filter(|page| {
            *page != Page::Notes
                || self
                    .settings_kv(cx)
                    .is_none_or(|kv| monocode_settings::settings_store::load_notes_enabled(&kv))
        });
        if let Some(page) = back {
            self.open_page(page, cx);
        }
    }

    /// Close the full page and show the workspace again.
    pub fn close_page(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.layout.page.take() {
            if let Some(package) = HistoryPackage::try_global(cx) {
                match page {
                    Page::Notes => {
                        let notes = package.notes.clone();
                        notes.update(cx, |notes, cx| notes.close_page(cx));
                    }
                    Page::Search => {
                        let search = package.search.clone();
                        search.update(cx, |search, cx| search.close(cx));
                    }
                    _ => {}
                }
            }
            if let Some(workspace) = &self.workspace {
                workspace.update(cx, |workspace, cx| {
                    workspace.set_full_page_open(false, cx);
                    workspace.set_inbox_visible(false, cx);
                });
            }
            cx.notify();
        }
    }

    pub(super) fn open_file_picker(
        &mut self,
        commands: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let cwd = workspace.read(cx).project_cwd().to_string();
        let paths = workspace
            .read(cx)
            .tabs()
            .iter()
            .flat_map(|tab| tab.editor_panes.iter().chain(&tab.terminal_panes))
            .flat_map(|pane| pane.files.iter())
            .map(|file| file.path.clone())
            .collect();
        let data = crate::adapters::files::app_files(cx);
        let picker = cx.new(|cx| {
            monocode_view_files::FilePicker::new(
                data,
                cwd,
                paths,
                if commands { ">" } else { "" },
                window,
                cx,
            )
        });
        let actions = [
            ("new-session", "New session"),
            ("open-project", "Open project"),
            ("new-terminal", "New terminal"),
            ("settings", "Open settings"),
            ("search", "Search project"),
            ("notes", "Open notes"),
            ("inbox", "Open inbox"),
            ("automations", "Open automations"),
            ("reload", "Reload MonoCode"),
        ]
        .into_iter()
        .map(
            |(id, label)| monocode_view_files::file_picker::PaletteAction {
                id: id.into(),
                label: label.into(),
                hint: None,
            },
        )
        .collect();
        picker.update(cx, |picker, cx| picker.set_actions(actions, cx));
        self.picker_subscription = Some(cx.subscribe_in(
            &picker,
            window,
            move |this, _, event, window, cx| {
                use monocode_view_files::FilePickerEvent;
                match event {
                    FilePickerEvent::OpenFile { path, options } => {
                        workspace
                            .update(cx, |workspace, cx| {
                                workspace.open_file(
                                    path,
                                    None,
                                    monocode_engine::workspace::workspace::FileOpenOptions {
                                        exact: options.exact,
                                        pin: options.pin,
                                    },
                                    cx,
                                )
                            })
                            .detach();
                    }
                    FilePickerEvent::RunAction(id) => match id.as_ref() {
                        "new-session" => {
                            this.new_session(cx);
                        }
                        "settings" => this.open_page(Page::Settings, cx),
                        "search" => this.open_page(Page::Search, cx),
                        "notes" => this.open_page(Page::Notes, cx),
                        "inbox" => this.open_page(Page::Inbox, cx),
                        "automations" => this.open_page(Page::Automations, cx),
                        "new-terminal" => {
                            workspace.update(cx, |workspace, cx| workspace.new_terminal(cx));
                        }
                        "open-project" => window.dispatch_action(Box::new(keymap::OpenProject), cx),
                        "reload" => window.dispatch_action(Box::new(keymap::Reload), cx),
                        _ => {}
                    },
                    FilePickerEvent::Close => {
                        this.file_picker = None;
                        this.picker_subscription = None;
                        cx.notify();
                    }
                }
            },
        ));
        window.focus(&picker.read(cx).focus_handle(cx), cx);
        self.file_picker = Some(picker);
        cx.notify();
    }

    /// `onToggleSidebar` (⌘B): the project rail.
    pub fn toggle_project_rail(&mut self, cx: &mut Context<Self>) {
        self.layout.project_rail_open = !self.layout.project_rail_open;
        self.save_open_state(
            monocode_core::appearance::PROJECT_RAIL_OPEN_KEY,
            self.layout.project_rail_open,
            cx,
        );
        cx.notify();
    }

    /// `onToggleSessionSidebar` (⌘⇧B).
    pub fn toggle_session_sidebar(&mut self, cx: &mut Context<Self>) {
        self.set_session_sidebar_open(!self.layout.session_sidebar_open, cx);
    }

    pub fn set_session_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.layout.session_sidebar_open != open {
            self.layout.session_sidebar_open = open;
            self.save_open_state(
                monocode_core::appearance::SESSION_SIDEBAR_OPEN_KEY,
                open,
                cx,
            );
            self.sync_sessions_tab_active(cx);
            cx.notify();
        }
    }

    fn settings_kv(&self, cx: &App) -> Option<monocode_settings::Kv> {
        AppServices::try_global(cx)
            .map(|services| services.kv.clone())
            .or_else(|| {
                self.history
                    .as_ref()
                    .map(|history| history.read(cx).kv().clone())
            })
    }

    fn save_open_state(&self, key: &str, open: bool, cx: &App) {
        if let Some(kv) = self.settings_kv(cx) {
            monocode_settings::storage_flags::write_flag(&kv, key, open);
        }
    }

    fn commit_width(&self, target: ResizeTarget, cx: &mut App) {
        match target {
            ResizeTarget::ProjectRail => {
                if let Some(kv) = self.settings_kv(cx) {
                    let width = monocode_core::appearance::clamp_project_rail_width(
                        self.layout.rail_width as f64,
                    );
                    kv.set_item(
                        monocode_core::appearance::PROJECT_RAIL_WIDTH_KEY,
                        &width.to_string(),
                    );
                }
            }
            ResizeTarget::SessionSidebar => {
                cx.set_global(RememberedSidebarWidth(self.layout.sidebar_width));
            }
        }
    }

    /// `onTabChange`: show a sidebar tab.
    pub fn set_sidebar_tab(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.sidebar_project = self.sidebar_cwd(cx);
        if let Some(services) = AppServices::try_global(cx) {
            monocode_engine::projects::project_sidebar_tab::save_project_sidebar_tab(
                &services.kv,
                &self.sidebar_project,
                tab.id(),
            );
        }
        if self.layout.sidebar_tab != tab {
            self.layout.sidebar_tab = tab;
            self.sync_sessions_tab_active(cx);
            cx.notify();
        }
    }

    fn sync_sessions_tab_active(&mut self, cx: &mut Context<Self>) {
        let active =
            self.layout.session_sidebar_open && self.layout.sidebar_tab == SidebarTab::Sessions;
        if let Some(history) = &self.history {
            history.update(cx, |history, cx| {
                history.set_sessions_tab_active(active, cx)
            });
        }
    }

    /// `collapsedProjectRailMode` changed in Settings.
    pub fn set_compact_rail(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.layout.compact_rail != compact {
            self.layout.compact_rail = compact;
            cx.notify();
        }
    }

    // Resizing and window dragging.

    pub(crate) fn start_resize(&mut self, target: ResizeTarget, x: Pixels) {
        let start_width = match target {
            ResizeTarget::ProjectRail => self.layout.rail_width,
            ResizeTarget::SessionSidebar => self.layout.sidebar_width,
        };
        self.resize = Some(Resize {
            target,
            start_x: x,
            start_width,
        });
    }

    pub(crate) fn resizing(&self, target: ResizeTarget) -> bool {
        self.resize.is_some_and(|resize| resize.target == target)
    }

    /// Double-click on a handle resets the width, like `useResizablePane`.
    pub(crate) fn reset_width(&mut self, target: ResizeTarget, cx: &mut Context<Self>) {
        match target {
            ResizeTarget::ProjectRail => self.layout.rail_width = PROJECT_RAIL_WIDTH_DEFAULT,
            ResizeTarget::SessionSidebar => {
                self.layout.sidebar_width = SESSION_SIDEBAR_WIDTH_DEFAULT
            }
        }
        self.commit_width(target, cx);
        cx.notify();
    }

    pub(crate) fn arm_window_drag(&mut self) {
        self.drag_armed = true;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.drag_armed {
            self.drag_armed = false;
            if event.pressed_button == Some(MouseButton::Left) {
                window.start_window_move();
                return;
            }
        }
        let Some(resize) = self.resize else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.resize = None;
            self.commit_width(resize.target, cx);
            cx.notify();
            return;
        }
        // Widths are CSS px; the pointer moves in window px.
        let scale = Theme::of(cx).ui_scale();
        let delta = f32::from(event.position.x - resize.start_x) / scale;
        let width = resize.start_width + delta;
        match resize.target {
            ResizeTarget::ProjectRail => {
                self.layout.rail_width = width
                    .clamp(PROJECT_RAIL_WIDTH_MIN, PROJECT_RAIL_WIDTH_MAX)
                    .round();
            }
            ResizeTarget::SessionSidebar => {
                // Sidebar.tsx also caps it at half the window.
                let half = f32::from(window.viewport_size().width) / scale * 0.5;
                let max = SESSION_SIDEBAR_WIDTH_MAX
                    .min(half)
                    .max(SESSION_SIDEBAR_WIDTH_MIN);
                self.layout.sidebar_width = width.clamp(SESSION_SIDEBAR_WIDTH_MIN, max).round();
            }
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_armed = false;
        if let Some(resize) = self.resize.take() {
            self.commit_width(resize.target, cx);
            cx.notify();
        }
    }

    /// The main column's body: a full page, or the workspace.
    fn render_main_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let slots = AppSlots::get(cx);
        if let Some(page) = self.layout.page {
            if let Some(view) = slots
                .page
                .as_ref()
                .and_then(|page_slot| page_slot(page, window, cx))
            {
                return div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(view)
                    .into_any_element();
            }
            let theme = Theme::of(cx);
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_px(theme.text.label)
                .text_color(theme.content(0.45))
                .child(format!("{page:?}"))
                .into_any_element();
        }
        if let Some(factory) = slots.workspace.as_ref() {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(factory(window, cx))
                .into_any_element();
        }
        self.render_main_pane(window, cx).into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let layout = self.layout.clone();
        self.live_agents.update(cx, |agents, cx| {
            agents.set_visible(
                layout.page != Some(Page::Settings)
                    && (layout.project_rail_open || layout.session_sidebar_open),
                cx,
            )
        });
        let compact_title_bar = layout.compact_title_bar();
        let rail: Option<AnyElement> = if layout.project_rail_open {
            Some(if layout.page == Some(Page::Settings) {
                settings_rail::view(cx.weak_entity(), layout.rail_width, window, cx)
                    .into_any_element()
            } else {
                self.rail.clone().into_any_element()
            })
        } else if layout.compact_rail_visible() {
            Some(self.compact_rail.clone().into_any_element())
        } else {
            None
        };
        let sidebar = (layout.session_sidebar_open && layout.page != Some(Page::Settings))
            .then(|| self.sidebar.clone().into_any_element());
        let mut main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(c.body_glass);
        if !compact_title_bar {
            main = main.child(self.title_bar.clone());
        }
        let body = self.render_main_body(window, cx);
        let main = main.child(body).child(self.render_footer(cx));
        let row = div()
            .flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .children(rail)
            .children(sidebar)
            .child(main);

        let mut root = div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(c.root_background)
            .text_color(c.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .on_key_down(cx.listener(Self::on_unhandled_key))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up));
        root = self.register_actions(root, cx);
        if compact_title_bar {
            root = root.child(self.title_bar.clone());
        }
        root = root
            .child(row)
            .child(toast_stack())
            .children(self.file_picker.clone())
            .children(self.project_picker.clone())
            .children(self.project_menu.clone())
            .children(self.project_dialog.clone())
            .child(crate::shell::whats_new::layer(window, cx));
        if self.resize.is_some() {
            root = root.cursor_col_resize();
        }
        root
    }
}

/// macOS-only children, such as the traffic-light spacer.
pub(crate) trait WhenMac: Sized {
    fn when_mac(self, f: impl FnOnce(Self) -> Self) -> Self;
}

impl<T: IntoElement> WhenMac for T {
    fn when_mac(self, f: impl FnOnce(Self) -> Self) -> Self {
        if cfg!(target_os = "macos") {
            f(self)
        } else {
            self
        }
    }
}

/// Marks an element as a window drag region (`data-tauri-drag-region`).
/// A double click zooms the window like a native title bar.
pub(crate) fn drag_region<E>(element: E, shell: WeakEntity<Shell>) -> E
where
    E: gpui::InteractiveElement + gpui::StatefulInteractiveElement,
{
    element
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            shell.update(cx, |shell, _| shell.arm_window_drag()).ok();
        })
        .on_click(|event, window, _| {
            if event.click_count() == 2 {
                if cfg!(target_os = "macos") {
                    window.titlebar_double_click();
                } else {
                    window.zoom_window();
                }
            }
        })
}

/// The vertical resize handle on a pane's right edge
/// (`absolute inset-y-0 -right-px w-1.5 cursor-col-resize`).
pub(crate) fn resize_handle(
    target: ResizeTarget,
    shell: WeakEntity<Shell>,
    cx: &App,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let dragging = shell
        .upgrade()
        .is_some_and(|shell| shell.read(cx).resizing(target));
    let width = theme.metrics.resize_handle_width;
    let hover = theme.content(0.10);
    let id = match target {
        ResizeTarget::ProjectRail => "rail-resize",
        ResizeTarget::SessionSidebar => "sidebar-resize",
    };
    let down = shell.clone();
    let mut handle = div()
        .id(id)
        .absolute()
        .top_0()
        .bottom_0()
        .right(u(-1.))
        .w(u(width))
        .cursor_col_resize()
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            cx.stop_propagation();
            down.update(cx, |shell, cx| {
                shell.start_resize(target, event.position.x);
                cx.notify();
            })
            .ok();
        })
        .on_click(move |event, _, cx| {
            if event.click_count() == 2 {
                shell
                    .update(cx, |shell, cx| shell.reset_width(target, cx))
                    .ok();
            }
        });
    if dragging {
        handle = handle.bg(theme.content(0.15));
    } else {
        handle = handle.hover(move |s| s.bg(hover));
    }
    handle
}

/// Builds the shell for `--view`.
pub fn build(options: ShellOptions, window: &mut Window, cx: &mut App) -> gpui::AnyView {
    cx.new(|cx| Shell::new(options, window, cx)).into()
}

fn workspace_chat_context(
    item: &monocode_engine::submit::chat_context::ChatContextItem,
) -> monocode_engine::workspace::chat_context::ChatContextItem {
    use monocode_engine::submit::chat_context::{
        ChatContextItem as Source, DiffLineChange as SourceChange,
    };
    use monocode_engine::workspace::chat_context::{
        ChatContextItem as Target, DiffLineChange as TargetChange,
    };
    match item {
        Source::Quote { text } => Target::Quote { text: text.clone() },
        Source::Code {
            path,
            start_line,
            end_line,
        } => Target::Code {
            path: path.clone(),
            start_line: *start_line,
            end_line: *end_line,
        },
        Source::Comment {
            path,
            line,
            change,
            code,
            comment,
        } => Target::Comment {
            path: path.clone(),
            line: *line,
            change: match change {
                SourceChange::Added => TargetChange::Added,
                SourceChange::Removed => TargetChange::Removed,
                SourceChange::Unchanged => TargetChange::Unchanged,
            },
            code: code.clone(),
            comment: comment.clone(),
        },
    }
}
