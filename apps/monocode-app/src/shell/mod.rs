//! The app shell. Port of the layout in src/app/App.tsx (the root around
//! lines 10545-11078) and the chrome in src/app/shell: the project rail or
//! compact rail, the session sidebar, and the main column with the title bar,
//! the pane area, and the usage footer.
//!
//! The shell owns the window's `Workspace` entity once the boot restore
//! finishes. Each region renders from one [`ShellData`] value collected from
//! the engine, through its own `impl Shell` block.

mod footer;
mod main_pane;
mod project_rail;
mod sidebar;
mod title_bar;

use std::collections::{HashMap, HashSet};

use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
};
use gpui_component::input::InputState;
use monocode_app::boot::{self, AppServices};
use monocode_app::bridge::ActiveWorkspace;
use monocode_app::history::{SidebarHistory, new_history};
use monocode_app::projects::{RailProject, rail_projects};
use monocode_engine::attention::{Attention, AttentionFocus};
use monocode_engine::runtime::Engine;
use monocode_engine::runtime::util::project_path::same_project_path;
use monocode_engine::workspace::{SessionFactory as _, Workspace};
use monocode_layout::leaf_ids;
use monocode_ui::appearance::{
    PROJECT_RAIL_WIDTH_DEFAULT, PROJECT_RAIL_WIDTH_MAX, PROJECT_RAIL_WIDTH_MIN,
    SESSION_SIDEBAR_WIDTH_DEFAULT, SESSION_SIDEBAR_WIDTH_MAX, SESSION_SIDEBAR_WIDTH_MIN,
};
use monocode_ui::widgets::{MenuEntry, MenuItem, context_menu, menu, toast_stack};
use monocode_ui::{Theme, u};

use crate::file_pane::FilePane;
use crate::session_pane::SessionPane;
use crate::view_data::ShellData;

gpui::actions!(shell, [NewSession]);

/// The session sidebar's tabs, `SidebarTabId` in appearance.ts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

/// Which pane edge a resize drag moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResizeTarget {
    ProjectRail,
    SessionSidebar,
}

#[derive(Clone, Copy, Debug)]
struct Resize {
    target: ResizeTarget,
    start_x: Pixels,
    start_width: f32,
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
}

impl ShellOptions {
    pub fn full() -> Self {
        Self {
            project_rail_open: true,
            compact_rail: false,
            session_sidebar_open: true,
            demo_menu: None,
            open_session: None,
        }
    }
}

/// Set once by `main` for `--open-session`, read by the shell builders.
pub struct StartupSession(pub Option<String>);

impl gpui::Global for StartupSession {}

pub struct Shell {
    project_rail_open: bool,
    /// `collapsedProjectRailMode === "compact"`: show the 48px rail when the
    /// project rail is closed.
    compact_rail: bool,
    session_sidebar_open: bool,
    rail_width: f32,
    sidebar_width: f32,
    sidebar_tab: SidebarTab,
    resize: Option<Resize>,
    /// Armed by a press on a drag region; the first move hands the drag to
    /// the window manager.
    drag_armed: bool,
    session_search: Entity<InputState>,
    session_menu: Option<Point<Pixels>>,
    /// The title bar's tab strip, and the tab it last scrolled to.
    title_scroll: ScrollHandle,
    scrolled_tab: Option<String>,
    /// The window's workspace, once the boot restore is done.
    workspace: Option<Entity<Workspace>>,
    history: Option<Entity<SidebarHistory>>,
    projects: Vec<RailProject>,
    /// `(additions, deletions)` per project path (`useProjectDiffStats`).
    project_stats: HashMap<String, (i64, i64)>,
    session_panes: HashMap<String, Entity<SessionPane>>,
    file_panes: HashMap<String, Entity<FilePane>>,
    /// The sidebar card under the context menu.
    menu_session: Option<String>,
    open_session: Option<String>,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Shell {
    pub fn new(options: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions"));
        let rail_width = AppServices::try_global(cx)
            .map(|services| services.settings.appearance.project_rail_width as f32)
            .filter(|width| *width > 0.0)
            .unwrap_or(PROJECT_RAIL_WIDTH_DEFAULT)
            .clamp(PROJECT_RAIL_WIDTH_MIN, PROJECT_RAIL_WIDTH_MAX);
        let mut shell = Self {
            project_rail_open: options.project_rail_open,
            compact_rail: options.compact_rail,
            session_sidebar_open: options.session_sidebar_open,
            rail_width,
            sidebar_width: SESSION_SIDEBAR_WIDTH_DEFAULT,
            sidebar_tab: SidebarTab::Sessions,
            resize: None,
            drag_armed: false,
            session_search,
            session_menu: options
                .demo_menu
                .map(|(x, y)| gpui::point(gpui::px(x), gpui::px(y))),
            title_scroll: ScrollHandle::new(),
            scrolled_tab: None,
            workspace: None,
            history: None,
            projects: Vec::new(),
            project_stats: HashMap::new(),
            session_panes: HashMap::new(),
            file_panes: HashMap::new(),
            menu_session: None,
            open_session: options.open_session.or_else(|| {
                cx.try_global::<StartupSession>()
                    .and_then(|startup| startup.0.clone())
            }),
            _subscriptions: Vec::new(),
            _tasks: Vec::new(),
        };
        shell.start(window, cx);
        // `onFocusChanged`: banners and the Dock badge follow window focus.
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if Attention::try_global(cx).is_some() {
                Attention::set_window_focused(cx, window.is_window_active());
            }
        });
        shell._subscriptions.push(activation);
        shell
    }

    /// Restore the workspace the last quit saved, then follow it.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if AppServices::try_global(cx).is_none() {
            return;
        }
        let restore = boot::restore_workspace(cx);
        let task = cx.spawn_in(window, async move |this, cx| {
            let restored = restore.await;
            this.update_in(cx, |this, window, cx| {
                let workspace = cx.new(|cx| Workspace::new(restored.config, cx));
                ActiveWorkspace::set(workspace.downgrade(), cx);
                let history = new_history(restored.history, restored.history_cwd, cx);
                this._subscriptions.push(cx.observe_in(
                    &workspace,
                    window,
                    |this, _, window, cx| this.workspace_changed(window, cx),
                ));
                this._subscriptions
                    .push(cx.observe(&history, |_, _, cx| cx.notify()));
                let sessions = Engine::sessions(cx);
                this._subscriptions
                    .push(cx.observe(&sessions, |_, _, cx| cx.notify()));
                if let Some(attention) = Attention::try_global(cx) {
                    let approvals = attention.approvals.clone();
                    let notifier = attention.notifier.clone();
                    this._subscriptions
                        .push(cx.observe(&approvals, |_, _, cx| cx.notify()));
                    this._subscriptions
                        .push(cx.observe(&notifier, |_, _, cx| cx.notify()));
                }
                this.workspace = Some(workspace.clone());
                this.history = Some(history);
                if let Some(id) = this.open_session.take() {
                    workspace
                        .update(cx, |workspace, cx| workspace.open_session(&id, cx))
                        .detach();
                }
                this.workspace_changed(window, cx);
            })
            .ok();
        });
        self._tasks.push(task);
    }

    /// The workspace moved: show its project in the sidebar and the rail,
    /// tell attention which session is on screen, and drop panes no tab
    /// shows anymore.
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
        if let Some(history) = &self.history {
            history.update(cx, |history, cx| history.show(&cwd, cx));
        }
        if !self
            .projects
            .iter()
            .any(|project| same_project_path(&project.path, &cwd))
        {
            self.reload_projects(&cwd, cx);
        }
        if Attention::try_global(cx).is_some() {
            Attention::set_focus(
                cx,
                AttentionFocus {
                    active_session_id: active.clone(),
                    ..Default::default()
                },
            );
        }
        let mut live: HashSet<String> = HashSet::new();
        for tab in &tabs {
            live.extend(leaf_ids(&tab.layout));
            live.extend(tab.editor_panes.iter().map(|pane| pane.id.clone()));
            live.extend(tab.terminal_panes.iter().map(|pane| pane.id.clone()));
        }
        self.session_panes.retain(|id, _| live.contains(id));
        self.file_panes.retain(|id, _| live.contains(id));
        if workspace.read(cx).composer_focused()
            && let Some(id) = active
        {
            let pane = self.session_pane(&id, window, cx);
            pane.update(cx, |pane, cx| pane.focus_composer(window, cx));
        }
        cx.notify();
    }

    /// The rail's projects, and their uncommitted line counts off the UI
    /// thread (`useProjectDiffStats`).
    fn reload_projects(&mut self, cwd: &str, cx: &mut Context<Self>) {
        let Some(services) = AppServices::try_global(cx) else {
            return;
        };
        self.projects = rail_projects(&services.kv, cwd);
        let paths: Vec<String> = self
            .projects
            .iter()
            .map(|project| project.path.clone())
            .filter(|path| !self.project_stats.contains_key(path))
            .collect();
        if paths.is_empty() {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            let stats = smol::unblock(move || {
                paths
                    .into_iter()
                    .map(|path| {
                        let stats = monocode_git::fs::git_diff_stats(path.clone());
                        (path, (stats.additions, stats.deletions))
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            this.update(cx, |this, cx| {
                this.project_stats.extend(stats);
                cx.notify();
            })
            .ok();
        });
        self._tasks.push(task);
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
            .unwrap_or_else(gpui::WeakEntity::new_invalid);
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

    fn data(&self, cx: &App) -> ShellData {
        ShellData::collect(
            self.workspace.as_ref(),
            self.history.as_ref(),
            &self.projects,
            &self.project_stats,
            cx,
        )
    }

    // Actions.

    /// `onSelectHistorySession`: open a sidebar card's session.
    fn open_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace
                .update(cx, |workspace, cx| workspace.open_session(session_id, cx))
                .detach();
        }
    }

    /// `onNew`: a new chat with the default model in a new tab, in the
    /// current project.
    fn new_session(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |workspace, cx| {
                workspace.new_session_tab(cx);
            });
        }
    }

    /// The project rail: show the project's open tab, or start a chat in it.
    fn select_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let existing = {
            let sessions = Engine::sessions(cx);
            let sessions = sessions.read(cx);
            workspace
                .read(cx)
                .tabs()
                .iter()
                .find(|tab| {
                    leaf_ids(&tab.layout).iter().any(|id| {
                        sessions
                            .get(id)
                            .is_some_and(|session| same_project_path(&session.cwd, path))
                    })
                })
                .map(|tab| tab.id.clone())
        };
        if let Some(tab_id) = existing {
            workspace.update(cx, |workspace, cx| {
                workspace.activate_tab(&tab_id, None, cx)
            });
            return;
        }
        let Some(services) = AppServices::try_global(cx) else {
            return;
        };
        let session = services.factory.new_default_session(path, None);
        let id = session.id.clone();
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
        workspace.update(cx, |workspace, cx| {
            workspace.set_project_cwd(path, cx);
            workspace.open_session(&id, cx).detach();
        });
    }

    fn activate_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace.update(cx, |workspace, cx| workspace.activate_tab(tab_id, None, cx));
        }
    }

    fn close_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = &self.workspace {
            workspace
                .update(cx, |workspace, cx| workspace.close_title_tab(tab_id, cx))
                .detach();
        }
    }

    fn compact_rail_visible(&self) -> bool {
        self.compact_rail && !self.project_rail_open
    }

    fn toggle_project_rail(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.project_rail_open = !self.project_rail_open;
        cx.notify();
    }

    fn start_resize(&mut self, target: ResizeTarget, x: Pixels) {
        let start_width = match target {
            ResizeTarget::ProjectRail => self.rail_width,
            ResizeTarget::SessionSidebar => self.sidebar_width,
        };
        self.resize = Some(Resize {
            target,
            start_x: x,
            start_width,
        });
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
            return;
        }
        // Widths are CSS px; the pointer moves in window px.
        let scale = Theme::of(cx).ui_scale();
        let delta = f32::from(event.position.x - resize.start_x) / scale;
        let width = resize.start_width + delta;
        match resize.target {
            ResizeTarget::ProjectRail => {
                self.rail_width = width
                    .clamp(PROJECT_RAIL_WIDTH_MIN, PROJECT_RAIL_WIDTH_MAX)
                    .round();
            }
            ResizeTarget::SessionSidebar => {
                // Sidebar.tsx also caps it at half the window.
                let half = f32::from(window.viewport_size().width) / scale * 0.5;
                let max = SESSION_SIDEBAR_WIDTH_MAX
                    .min(half)
                    .max(SESSION_SIDEBAR_WIDTH_MIN);
                self.sidebar_width = width.clamp(SESSION_SIDEBAR_WIDTH_MIN, max).round();
            }
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_armed = false;
        if self.resize.take().is_some() {
            cx.notify();
        }
    }

    /// The vertical resize handle on a pane's right edge
    /// (`absolute inset-y-0 -right-px w-1.5 cursor-col-resize`).
    fn resize_handle(&self, target: ResizeTarget, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let dragging = self.resize.is_some_and(|resize| resize.target == target);
        let width = theme.metrics.resize_handle_width;
        let hover = theme.content(0.10);
        let id = match target {
            ResizeTarget::ProjectRail => "rail-resize",
            ResizeTarget::SessionSidebar => "sidebar-resize",
        };
        let mut handle = div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .right(u(-1.))
            .w(u(width))
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.start_resize(target, event.position.x);
                    cx.notify();
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                // Double-click resets the width, like `useResizablePane`.
                if event.click_count() == 2 {
                    match target {
                        ResizeTarget::ProjectRail => this.rail_width = PROJECT_RAIL_WIDTH_DEFAULT,
                        ResizeTarget::SessionSidebar => {
                            this.sidebar_width = SESSION_SIDEBAR_WIDTH_DEFAULT
                        }
                    }
                    cx.notify();
                }
            }));
        if dragging {
            handle = handle.bg(theme.content(0.15));
        } else {
            handle = handle.hover(move |s| s.bg(hover));
        }
        handle
    }

    /// Marks an element as a window drag region (`data-tauri-drag-region`).
    /// A double click zooms the window like a native title bar.
    fn drag_region<E>(&self, element: E, cx: &mut Context<Self>) -> E
    where
        E: gpui::InteractiveElement + gpui::StatefulInteractiveElement,
    {
        element
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _, _| this.drag_armed = true),
            )
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

    fn render_session_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let position = self.session_menu?;
        let entries: Vec<MenuEntry> = vec![
            MenuItem::new("open", "Open in New Tab")
                .shortcut("⌘↩")
                .into(),
            MenuItem::new("rename", "Rename").shortcut("F2").into(),
            MenuItem::new("pin", "Pin").into(),
            MenuItem::new("link", "Link Issue or PR…").into(),
            MenuEntry::Separator,
            MenuItem::new("archive", "Archive").into(),
            MenuItem::new("delete", "Delete").danger().into(),
        ];
        let weak = cx.entity().downgrade();
        let pick = weak.clone();
        Some(context_menu(
            position,
            menu("session-menu", entries).on_pick(move |id, _, cx| {
                pick.update(cx, |this, cx| {
                    this.session_menu = None;
                    if id.as_ref() == "open"
                        && let Some(session_id) = this.menu_session.take()
                    {
                        this.open_session(&session_id, cx);
                    }
                    cx.notify();
                })
                .ok();
            }),
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    this.session_menu = None;
                    cx.notify();
                })
                .ok();
            },
            cx,
        ))
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let data = self.data(cx);
        if !data.active_tab_id.is_empty()
            && self.scrolled_tab.as_deref() != Some(data.active_tab_id.as_str())
            && let Some(index) = data
                .tabs
                .iter()
                .position(|tab| tab.id == data.active_tab_id)
        {
            self.title_scroll.scroll_to_item(index);
            self.scrolled_tab = Some(data.active_tab_id.clone());
        }
        // On macOS the compact rail moves the title bar above everything, so
        // the traffic lights sit in it (`compactTitleBar` in App.tsx).
        let compact_title_bar = cfg!(target_os = "macos") && self.compact_rail_visible();
        let rail = if self.project_rail_open {
            Some(
                self.render_project_rail(&data, window, cx)
                    .into_any_element(),
            )
        } else if self.compact_rail_visible() {
            Some(self.render_compact_rail(&data, cx).into_any_element())
        } else {
            None
        };
        let sidebar = self
            .session_sidebar_open
            .then(|| self.render_sidebar(&data, window, cx).into_any_element());
        let mut main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(c.body_glass);
        if !compact_title_bar {
            main = main.child(self.render_title_bar(&data, cx));
        }
        let pane = self.render_main_pane(window, cx);
        let main = main.child(pane).child(self.render_footer(&data, cx));
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
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(c.root_background)
            .text_color(c.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .on_action(cx.listener(|this, _: &NewSession, _, cx| this.new_session(cx)))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up));
        if compact_title_bar {
            root = root.child(self.render_title_bar(&data, cx));
        }
        root = root.child(row).child(toast_stack());
        if self.resize.is_some() {
            root = root.cursor_col_resize();
        }
        root.children(self.render_session_menu(cx))
    }
}

/// macOS-only children, such as the traffic-light spacer.
pub(super) trait WhenMac: Sized {
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

/// Builds the shell for `--view`.
pub fn build(options: ShellOptions, window: &mut Window, cx: &mut App) -> gpui::AnyView {
    cx.new(|cx| Shell::new(options, window, cx)).into()
}
