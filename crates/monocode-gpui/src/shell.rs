mod accounts;
mod actions;
mod app_commands;
mod app_views;
mod automation_runs;
mod classic_sidebar;
#[cfg(test)]
mod classic_surface_tests;
mod classic_surfaces;
mod file_panes;
mod idle_agents;
#[cfg(test)]
mod image_lightbox_tests;
mod layout;
mod live_agents;
mod mobile_host;
mod mono_panes;
pub(crate) mod monos;
mod notifications;
mod orchestration;
mod picker_preferences;
mod project_rail;
mod project_removal;
mod project_settings;
mod quick_composer;
mod quick_open;
mod rails;
mod remote_projects;
mod remote_sessions;
mod search;
mod session_filters;
mod session_folders;
mod session_groups;
mod session_loading;
mod session_preview;
mod session_shortcuts;
mod settings;
mod shortcuts;
mod sidebar_list;
mod sidebar_peek;
mod space_reorder;
mod terminal_dock;
mod title_tabs;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod window_chrome;
mod worktrees;

use crate::{
    conversation::{Conversation, SessionChanged},
    input,
    notes::Notes,
    pane_layout::{Edge, LayoutLeaf, Rect, SplitDir},
    pane_motion::PaneSlide,
    pickers::{ProjectPicked, ProjectPicker},
    popover::{
        Align, Anchor, AnchorSlot, Flyout, Menu, MenuAction, MenuEvent, MenuItem, MenuStyle, Side,
        menu_popover, popover,
    },
    resource_pressure::{ResourceMonitor, StopSession},
    response_actions::Transfer,
    right_panel::{PanelEvent, RightPanel},
    runtime::{Database, locked},
    session_mark::{SessionMark, State as MarkState},
    terminal::{Terminal, TerminalEvent},
    theme::*,
    usage_footer::UsageFooter,
    workspace::{DockSide, TitleTab, VisitHistory, Workspace, project_name},
};
use gpui::{
    AnyElement, App, Bounds, BoxShadow, ClipboardItem, Context, CursorStyle, DispatchPhase, Entity,
    FocusHandle, Focusable, FontWeight, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathPromptOptions, Pixels, Point, Role, ScrollHandle, SharedString,
    StyleRefinement, Subscription, Window, WindowControlArea, actions, canvas, div, prelude::*, px,
    rgb, rgba,
};
use monocode_core::{provider::Kind, store::Session};
use session_groups::local_reminder_label;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

actions!(
    workspace,
    [
        NewCodex,
        NewClaude,
        OpenProject,
        CloseTab,
        CloseAllTabs,
        ArchiveSession,
        VisitBack,
        VisitForward,
        DismissMenu,
        NextTab,
        PreviousTab,
        ToggleSidebar,
        FocusSearch,
        SearchFiles,
        FocusNext,
        FocusPrevious,
        CloseOtherTabs,
        PreviousSession,
        NextSession,
        PreviousSessionInTab,
        NextSessionInTab,
        PreviousProject,
        NextProject,
        MoveProjectBack,
        MoveProjectForward,
        SwitchModel,
        ToggleRightPanel,
        SplitRight,
        SplitDown,
        FocusPaneLeft,
        FocusPaneRight,
        FocusPaneUp,
        FocusPaneDown,
        ToggleTerminal,
        NewTerminal,
        ToggleLayout,
        ToggleWorkspaceStyle,
        OpenSettings,
        GoToFile,
        OpenCommandPalette,
        OpenInbox,
        OpenNotes,
        CheckForUpdates,
        MinimizeWindow,
        ZoomWindow,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        Reload,
        SidebarAppearance,
        About
    ]
);

#[derive(Clone, Copy)]
enum ChromeKind {
    Sidebar,
    Projects,
    Tabs,
}
struct Chrome {
    owner: gpui::WeakEntity<Shell>,
    kind: ChromeKind,
    renders: usize,
}
impl Render for Chrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let kind = self.kind;
        self.owner
            .update(cx, |shell, cx| match kind {
                ChromeKind::Sidebar => {
                    let sidebar = shell.sidebar(cx);
                    if shell.spaces_nudge.is_some() || shell.classic_tabs.animating() {
                        window.request_animation_frame();
                    }
                    sidebar
                }
                ChromeKind::Tabs => shell.tabs(window, cx),
                ChromeKind::Projects => {
                    let rail = shell.project_rail(cx);
                    if shell.project_rail_animating() {
                        window.request_animation_frame();
                    }
                    rail
                }
            })
            .unwrap_or_else(|_| div().into_any())
    }
}

/// `Tab: Activate 1–8` with the slot, or `Tab: Activate Last` with -1.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = workspace, no_json)]
pub(crate) struct ActivateTab(pub isize);

/// The workspace's keys, after TitleBar/App's `tabCommand` and the settings
/// keybinding table: tabs, history, sessions, projects and the sidebar.
pub(crate) fn bind_keys(cx: &mut App) {
    session_groups::bind(cx);
    session_folders::bind(cx);
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    // Scoped to the shell, so the focused composer, menus and pickers outrank
    // these: GPUI ranks an unscoped binding above every context.
    let context = Some("Workspace");
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-n"), NewCodex, context),
        KeyBinding::new(&format!("{modifier}-t"), NewCodex, context),
        KeyBinding::new(&format!("{modifier}-shift-n"), NewClaude, context),
        KeyBinding::new(&format!("{modifier}-o"), OpenProject, context),
        KeyBinding::new(&format!("{modifier}-p"), GoToFile, context),
        KeyBinding::new(&format!("{modifier}-shift-p"), OpenCommandPalette, context),
        KeyBinding::new(&format!("{modifier}-,"), OpenSettings, context),
        KeyBinding::new(&format!("{modifier}-w"), CloseTab, context),
        KeyBinding::new(&format!("{modifier}-shift-w"), CloseAllTabs, context),
        KeyBinding::new(
            &format!("{modifier}-shift-a"),
            ArchiveSession,
            Some("Session > (Composer || TranscriptSelection) && !CodeEditor && !Completion && !PopoverMenu && !Prompt"),
        ),
        KeyBinding::new(&format!("{modifier}-["), VisitBack, context),
        KeyBinding::new(&format!("{modifier}-]"), VisitForward, context),
        KeyBinding::new("escape", DismissMenu, context),
        KeyBinding::new(&format!("{modifier}-b"), ToggleSidebar, context),
        KeyBinding::new(&format!("{modifier}-k"), FocusSearch, context),
        KeyBinding::new(&format!("{modifier}-shift-f"), SearchFiles, context),
        KeyBinding::new(&format!("{modifier}-alt-t"), CloseOtherTabs, context),
        KeyBinding::new(&format!("{modifier}-shift-]"), NextTab, context),
        KeyBinding::new(&format!("{modifier}-}}"), NextTab, context),
        KeyBinding::new(&format!("{modifier}-shift-["), PreviousTab, context),
        KeyBinding::new(&format!("{modifier}-{{"), PreviousTab, context),
        KeyBinding::new(&format!("{modifier}-1"), ActivateTab(0), context),
        KeyBinding::new(&format!("{modifier}-2"), ActivateTab(1), context),
        KeyBinding::new(&format!("{modifier}-3"), ActivateTab(2), context),
        KeyBinding::new(&format!("{modifier}-4"), ActivateTab(3), context),
        KeyBinding::new(&format!("{modifier}-5"), ActivateTab(4), context),
        KeyBinding::new(&format!("{modifier}-6"), ActivateTab(5), context),
        KeyBinding::new(&format!("{modifier}-7"), ActivateTab(6), context),
        KeyBinding::new(&format!("{modifier}-8"), ActivateTab(7), context),
        KeyBinding::new(&format!("{modifier}-9"), ActivateTab(-1), context),
        KeyBinding::new(&format!("{modifier}-shift-up"), PreviousSession, context),
        KeyBinding::new(&format!("{modifier}-shift-down"), NextSession, context),
        KeyBinding::new(&format!("{modifier}-up"), PreviousSessionInTab, context),
        KeyBinding::new(&format!("{modifier}-down"), NextSessionInTab, context),
        KeyBinding::new(&format!("{modifier}-shift-left"), PreviousProject, context),
        KeyBinding::new(&format!("{modifier}-shift-right"), NextProject, context),
        KeyBinding::new(&format!("{modifier}-alt-shift-left"), MoveProjectBack, context),
        KeyBinding::new(&format!("{modifier}-alt-shift-right"), MoveProjectForward, context),
        KeyBinding::new(&format!("{modifier}-."), SwitchModel, context),
        KeyBinding::new(&format!("{modifier}-shift-b"), ToggleRightPanel, context),
        KeyBinding::new(&format!("{modifier}-d"), SplitRight, context),
        KeyBinding::new(&format!("{modifier}-j"), ToggleTerminal, context),
        KeyBinding::new(&format!("{modifier}-`"), NewTerminal, context),
        // Switch between the original two-sidebar layout and the new layout.
        KeyBinding::new(&format!("{modifier}-shift-l"), ToggleLayout, context),
        // Switch the Focused layout between its rounded and boxy workspace.
        KeyBinding::new(&format!("{modifier}-alt-l"), ToggleWorkspaceStyle, context),
        KeyBinding::new(&format!("{modifier}-shift-d"), SplitDown, context),
        KeyBinding::new(&format!("{modifier}-="), ZoomIn, context),
        KeyBinding::new(&format!("{modifier}-+"), ZoomIn, context),
        KeyBinding::new(&format!("{modifier}--"), ZoomOut, context),
        KeyBinding::new(&format!("{modifier}-0"), ResetZoom, context),
        KeyBinding::new(&format!("{modifier}-shift-r"), Reload, context),
        KeyBinding::new(&format!("{modifier}-alt-left"), FocusPaneLeft, context),
        KeyBinding::new(&format!("{modifier}-alt-right"), FocusPaneRight, context),
        KeyBinding::new(&format!("{modifier}-alt-up"), FocusPaneUp, context),
        KeyBinding::new(&format!("{modifier}-alt-down"), FocusPaneDown, context),
        KeyBinding::new("ctrl-tab", NextTab, context),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, context),
        KeyBinding::new("tab", FocusNext, context),
        KeyBinding::new("shift-tab", FocusPrevious, context),
    ]);
}

/// Windows has no caption of its own (`appears_transparent`), so the app
/// draws the window controls. Test windows keep the captionless geometry
/// that Linux tests get from server decorations.
#[cfg(target_os = "windows")]
const WINDOWS_CONTROLS: bool = cfg!(not(test));

pub(crate) fn client_controls_width(window: &Window) -> f32 {
    #[cfg(target_os = "linux")]
    if !window.is_fullscreen()
        && matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        )
    {
        return window_chrome::CONTROLS_WIDTH;
    }
    #[cfg(target_os = "windows")]
    if WINDOWS_CONTROLS && !window.is_fullscreen() {
        return window_chrome::CONTROLS_WIDTH;
    }
    let _ = window;
    0.
}

/// Client-drawn window controls that hold the corner's right end, inside the
/// bar: Windows only. Client-decorated Linux shifts the whole bar left of its
/// controls instead (`titlebar_controls_space`).
pub(crate) fn corner_controls_width(window: &Window) -> f32 {
    if cfg!(target_os = "windows") {
        client_controls_width(window)
    } else {
        0.
    }
}

/// macOS must receive button clicks as app content. Only blank titlebar
/// regions initiate native dragging or the system double-click action.
/// Client-decorated Linux windows also need explicit native drag requests.
/// Windows hit-tests the drag area as the caption, which drags, snaps and
/// maximizes on double-click natively.
pub(crate) fn titlebar_drag_area() -> gpui::Div {
    let bounds = AnchorSlot::default();
    div()
        .relative()
        .h_full()
        .child(bounds.probe())
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            if (cfg!(target_os = "macos")
                || (cfg!(target_os = "linux")
                    && matches!(
                        window.window_decorations(),
                        gpui::Decorations::Client { .. }
                    )))
                && bounds.get().is_some_and(|b| b.contains(&event.position))
            {
                // Check the measured spacer as well as GPUI's cached hit test:
                // native gestures must never start over a neighboring control.
                cx.stop_propagation();
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else if event.click_count == 1 {
                    window.start_window_move();
                }
            }
        })
}

/// tabKeys.ts's `adjacentItemId`: the next or previous item, wrapping; from
/// outside the list, the first or last.
fn adjacent<T: PartialEq + Clone>(items: &[T], current: Option<&T>, step: isize) -> Option<T> {
    if items.is_empty() {
        return None;
    }
    let len = items.len() as isize;
    match current.and_then(|c| items.iter().position(|item| item == c)) {
        Some(ix) => Some(items[(ix as isize + step.signum()).rem_euclid(len) as usize].clone()),
        None if step < 0 => items.last().cloned(),
        None => items.first().cloned(),
    }
}

#[derive(Clone, PartialEq)]
pub(crate) enum MenuTarget {
    Tab(i64),
    MonoTab(String),
    SearchTab,
    AppTab(app_views::AppView),
    Session(i64),
    Folder(String),
    /// The sidebar project header's menu.
    Header,
    /// The original project rail's local or remote folder menu.
    ProjectRailAdd,
    CompactProjectPicker,
    ProjectRailGroup(String),
    SessionFilter,
    SidebarOptions,
    /// A project's actions; `rail` adds the project rail's grouping and pinning.
    Project {
        path: String,
        rail: bool,
    },
    /// ProjectTerminalDock's Move Terminal menu.
    TerminalDock,
    /// A dock terminal tab's context menu.
    TerminalTab(usize),
    /// The footer chip's list of terminals running a process.
    RunningTerminals,
    /// The footer Actions button's list of saved commands.
    Actions,
}
pub(crate) struct ShellMenu {
    pub(crate) target: MenuTarget,
    anchor: Anchor,
    menu: Entity<Menu>,
    _events: Vec<Subscription>,
}
/// The gap the traffic lights take in the title bar (`w-[78px]`, or `w-2.5`
/// where the window has none, as in macOS fullscreen).
const TRAFFIC_LIGHTS: f32 = if cfg!(target_os = "macos") { 78. } else { 10. };
const NO_TRAFFIC_LIGHTS: f32 = 10.;
// The tab strip's padding; the trailing side also has the 2px gap before the
// strip's drag area.
const STRIP_LEAD: f32 = 6.;
const STRIP_TRAIL: f32 = 6.;
const STRIP_PAD: f32 = STRIP_LEAD + STRIP_TRAIL;
/// The selected title tab shrinks with the others only down to this.
const SELECTED_TAB_MIN_WIDTH: f32 = 112.;
/// Narrower tabs drop their close button.
const TAB_CLOSE_MIN_WIDTH: f32 = 72.;
/// The new-tab button and its leading margin.
const STRIP_NEW_TAB: f32 = TAB_PLATE_HEIGHT + 2.;
const CORNER_TABS_LEAD: f32 = 7.;
const CORNER_CONTROL_GAP: f32 = 4.;
const CORNER_BUTTONS_WIDTH: f32 = 48. + 2. * CORNER_CONTROL_GAP;
// TitleBar.tsx's labels, with their shortcuts.
const TOGGLE_SIDEBAR_LABEL: &str = if cfg!(target_os = "macos") {
    "Toggle Sidebar (⌘B)"
} else {
    "Toggle Sidebar (Ctrl+B)"
};
const BACK_LABEL: &str = if cfg!(target_os = "macos") {
    "Back (⌘[)"
} else {
    "Back (Ctrl+[)"
};
const FORWARD_LABEL: &str = if cfg!(target_os = "macos") {
    "Forward (⌘])"
} else {
    "Forward (Ctrl+])"
};
const TOGGLE_RIGHT_PANEL_LABEL: &str = if cfg!(target_os = "macos") {
    "Toggle right sidebar (⌘⇧B)"
} else {
    "Toggle right sidebar (Ctrl+Shift+B)"
};
const TOGGLE_RIGHT_PANEL_DOT_LABEL: &str = if cfg!(target_os = "macos") {
    "Toggle right sidebar (⌘⇧B, uncommitted changes)"
} else {
    "Toggle right sidebar (Ctrl+Shift+B, uncommitted changes)"
};
const MOD: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl+"
};
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(target_os = "windows") {
    "Reveal in File Explorer"
} else {
    "Open Containing Folder"
};

struct TabDrag {
    id: TitleTab,
    from: usize,
    to: usize,
    origin: Pixels,
    offset: Pixels,
    active: bool,
    bounds: Vec<Bounds<Pixels>>,
    /// Pulled far enough below the strip to drop on a pane.
    armed: bool,
}

/// What a pane drop carries: a split pane by its header, a title tab
/// pulled down out of the strip, or a sidebar session.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DragSource {
    Pane(i64),
    Review(i64),
    Tab(i64),
    Session(i64),
    /// A Mono tab pulled down out of the strip; it has no pane yet.
    MonoTab,
}
impl DragSource {
    fn session(self) -> i64 {
        match self {
            DragSource::Pane(id)
            | DragSource::Review(id)
            | DragSource::Tab(id)
            | DragSource::Session(id) => id,
            DragSource::MonoTab => i64::MIN,
        }
    }
}
/// PaneTree.tsx's pane drag and the external drops from TitleBar/Sidebar.
struct PaneDrag {
    source: DragSource,
    start: Point<Pixels>,
    position: Point<Pixels>,
    active: bool,
    over: Option<(i64, Edge)>,
    /// A pane pulled onto the title strip becomes a tab at this index.
    insert: Option<usize>,
}
struct SashDrag {
    tab: i64,
    path: Vec<usize>,
    index: usize,
    dir: SplitDir,
    group: Rect,
}
/// Something the user resizes by dragging its edge.
#[derive(Clone, PartialEq)]
enum ResizeTarget {
    Sidebar,
    Projects,
    RightPanel,
    Dock(String),
    /// A pane sash, with its group's length in pixels.
    Sash {
        tab: i64,
        path: Vec<usize>,
        index: usize,
        span: f32,
    },
}
/// A pane split into the tree pushes in from its edge while its neighbours
/// follow; a closed pane's neighbours grow back over its room.
struct PaneMotion {
    tab: i64,
    from: HashMap<i64, Rect>,
    entering: HashMap<i64, Edge>,
    /// Closed panes riding their edge out: the old leaf, its edge and the
    /// collapsed rect it ends at.
    leaving: Vec<(LayoutLeaf, Edge, Rect)>,
    /// The split header grows as a lone pane gains a neighbour, and shrinks
    /// as the split closes back to one pane.
    header: Option<bool>,
    slide: PaneSlide,
}
struct TabCopy {
    headline: String,
    /// Every pane in layout order; the tab shows one line at a time.
    panes: Vec<TabPane>,
    /// The pane in `panes` that has focus.
    focus: usize,
}
/// One line of a tab's headline: a chat, or the file beside it.
struct TabPane {
    /// The pane's leaf, and whether this is its file side.
    key: (i64, bool),
    title: String,
    mark: PaneMark,
}
enum PaneMark {
    /// The provider, with the session id when its mark is lit.
    Session(String, Option<i64>),
    /// A file's name, or `None` for changes and commits.
    File(Option<String>),
    /// A Mono's mascot and colour.
    Mono(String, u32),
}
/// A split tab's headline rolling to the newly focused pane: the next pane
/// rises from below, the previous one drops from above.
struct PaneTick {
    shown: (i64, bool),
    from: Option<(i64, bool)>,
    /// 1 when the new line rises, -1 when it drops.
    dir: f32,
    start: std::time::Instant,
}
const PANE_TICK_DURATION: f32 = 0.24;
/// useAnimatedReorder: `--motion-reorder-duration` with `--motion-ease-out`.
const REORDER_DURATION: f32 = 0.16;
/// A tab easing from one horizontal offset to another.
#[derive(Clone, Copy)]
struct TabShift {
    from: f32,
    to: f32,
    start: std::time::Instant,
}
impl TabShift {
    fn at(&self, now: std::time::Instant) -> (f32, bool) {
        let t = now.saturating_duration_since(self.start).as_secs_f32() / REORDER_DURATION;
        if t >= 1. {
            return (self.to, true);
        }
        let eased = crate::reveal::bezier(t, 0.22, 1., 0.36, 1.);
        (self.from + (self.to - self.from) * eased, false)
    }
}
/// Below this a chat column stops being comfortable to read.
const MIN_CHAT_WIDTH: f32 = 420.;
/// A sideways tab drag sags; only a deliberate pull below the strip
/// targets a pane (TitleBar's PANE_DROP_PULL).
const PANE_DROP_PULL: f32 = 32.;

struct SessionPreview {
    id: i64,
    data: Option<monocode_core::store::SessionPreviewData>,
    anchor: AnchorSlot,
    surface: AnchorSlot,
}

pub(crate) struct Shell {
    monos: Option<Entity<crate::monos::Monos>>,
    quick_open: Option<quick_open::Open>,
    image_lightbox: Option<(Entity<crate::image_lightbox::ImageLightbox>, Subscription)>,
    app_bridge: Option<monocode_core::mono::bridge::Server>,
    mobile: mobile_host::Mobile,
    app_results: HashMap<(i64, String), (String, Result<serde_json::Value, String>)>,
    app_pending: HashSet<(i64, String)>,
    orchestrations: HashMap<i64, orchestration::Run>,
    /// Each lead's last orchestration, whose agents hang under it in the sidebar.
    orchestration_summaries: HashMap<i64, monocode_core::orchestration::Summary>,
    /// Sessions an /operator thread started, by the session that started them.
    started_by: HashMap<i64, i64>,
    /// The sidebar rails under orchestrators and /operator threads.
    rails: rails::Rails,
    usage_footer: Entity<UsageFooter>,
    /// The footer's high-usage chip and its running-work dialog.
    resources: Entity<ResourceMonitor>,
    sidebar_frame: Entity<Chrome>,
    projects_frame: Entity<Chrome>,
    project_rail_list: project_rail::ProjectRailList,
    project_rail_drag: Option<project_rail::ProjectRailDrag>,
    space_reorder: space_reorder::SpaceReorder,
    project_rail_stats: project_rail::ProjectRailStats,
    project_resize: Option<(Pixels, f32)>,
    classic_sidebar_tab: classic_sidebar::ClassicSidebarTab,
    classic_tabs: classic_sidebar::WorkspaceTabs,
    classic_session_search: Entity<input::Input>,
    classic_worktree_picker: Option<classic_sidebar::WorktreePicker>,
    sidebar_list: sidebar_list::SidebarList,
    sidebar_slide: Option<PaneSlide>,
    sidebar_fraction: f32,
    /// The collapsed inset sidebar peeking out over the workspace.
    sidebar_peek: Option<sidebar_peek::SidebarPeek>,
    peek_animating: bool,
    peek_close: Option<gpui::Task<()>>,
    peek_toggle_bounds: AnchorSlot,
    /// useTrafficLightsVisible: fullscreen hides the native buttons, so the
    /// title bar takes their room back.
    fullscreen: bool,
    /// `corner_controls_width` as of this render, for the corner's layout.
    window_controls: f32,
    /// The narrowest right panel whose header holds the corner's tools
    /// beside client-drawn window controls.
    right_panel_min: f32,
    #[cfg(target_os = "linux")]
    client_decorations: bool,
    tabs_frame: Entity<Chrome>,
    store: Database,
    remote: Arc<monocode_core::remote::Connections>,
    remote_project_picker: Option<remote_projects::Open>,
    remote_refreshing: bool,
    remote_marks: HashMap<i64, (bool, bool)>,
    _remote_clock: gpui::Task<()>,
    sessions: HashMap<i64, Entity<Conversation>>,
    /// Lightweight history index; only opened or previewed chats own GPUI entities.
    session_summaries: HashMap<i64, Session>,
    draft_sessions: HashSet<i64>,
    history_recency: HashMap<i64, u64>,
    history_clock: u64,
    session_marks: HashMap<i64, Entity<SessionMark>>,
    tab_marks: HashMap<i64, Entity<SessionMark>>,
    /// The live agents card's spinners, apart from the sidebar rows' marks.
    live_marks: HashMap<i64, Entity<SessionMark>>,
    live_agents_expanded: bool,
    /// Repaints the live agents card when one of its elapsed times changes.
    live_tick: Option<gpui::Task<()>>,
    /// Milliseconds until the next elapsed label in the live agents card changes.
    live_next_change: u64,
    busy_sessions: HashSet<i64>,
    notification_state: notifications::State,
    quick: crate::quick_composer::Controller,
    unseen_finished: HashSet<i64>,
    archiving: HashSet<i64>,
    archive_shortcut_held: bool,
    /// Editors offered by a project's "Open in editor"; `None` while scanning.
    external_editors: Option<Vec<monocode_core::external_editor::ExternalEditor>>,
    groups: HashMap<String, monocode_core::session_groups::Groups>,
    reminders: Vec<monocode_core::store::Reminder>,
    renaming: Option<session_groups::Renaming>,
    link_dialog: Option<session_groups::LinkDialog>,
    action_dialog: Option<actions::ActionDialog>,
    actions_anchor: AnchorSlot,
    /// The dock terminal each action last ran in, by project and action id.
    action_terminals: HashMap<(String, String), gpui::WeakEntity<Terminal>>,
    deleting: HashSet<i64>,
    group_writes: Option<gpui::Task<()>>,
    selected_sessions: HashSet<i64>,
    selection_anchor: Option<i64>,
    context_selection: bool,
    sidebar_target: Option<session_groups::RenameTarget>,
    session_row_bounds: HashMap<i64, AnchorSlot>,
    folder_bounds: HashMap<String, AnchorSlot>,
    sidebar_drop: Option<session_groups::ListDrop>,
    folder_drag: Option<session_groups::FolderDrag>,
    skip_folder_click: bool,
    filter_anchor: AnchorSlot,
    folder_picker: Option<session_folders::Open>,
    pending_folder: Option<(i64, AnchorSlot)>,
    _session_clock: gpui::Task<()>,
    notes: Option<Entity<Notes>>,
    inbox: Option<Entity<crate::inbox::Inbox>>,
    automations: Option<Entity<crate::automations::Automations>>,
    active_app_view: Option<app_views::AppView>,
    app_view_events: Vec<Subscription>,
    automation_state: automation_runs::State,
    settings: Option<Entity<crate::settings::SettingsView>>,
    settings_events: Option<Subscription>,
    settings_overlay_watch: Option<Subscription>,
    app_search: Option<search::Open>,
    saved_search: Option<(String, monocode_core::search::Scope)>,
    order: Vec<i64>,
    pub(crate) workspace: Workspace,
    focus: FocusHandle,
    sidebar_focus: FocusHandle,
    query: String,
    sidebar_hover: Option<&'static str>,
    tab_hover: Option<i64>,
    /// Tab animations: opening and closing title tabs, with each tab's last label.
    title_tab_motion: crate::tab_motion::TabMotion<TitleTab, ()>,
    /// Tab animations: the selected tab before the last switch, the one now
    /// selected and when it changed, so one eases down as the other widens.
    title_tab_selection: (Option<TitleTab>, Option<TitleTab>, std::time::Instant),
    session_hover: Option<i64>,
    session_preview: Option<SessionPreview>,
    preview_close: Option<gpui::Task<()>>,
    preview_open: Option<gpui::Task<()>>,
    sidebar_last_scroll: Option<std::time::Instant>,
    window_observed: bool,
    shortcuts: Option<shortcuts::Shortcuts>,
    window_active: bool,
    window_glass: Option<bool>,
    /// The project whose colors were last painted; leaving it blends colors.
    theme_project: Option<String>,
    quit_prompted: bool,
    pub(crate) menu: Option<ShellMenu>,
    background_dialog: Option<project_settings::OpenBackground>,
    backgrounds: HashMap<i64, Entity<crate::project_background::ProjectBackground>>,
    /// The last active chat and whether it drew a background, so arriving
    /// from one without fades the next one in.
    backdrop: Option<(i64, bool)>,
    background_card: AnchorSlot,
    project_anchor: AnchorSlot,
    sidebar_options_anchor: AnchorSlot,
    history: VisitHistory,
    /// User navigation invalidates pending requests that would otherwise
    /// focus a new session in a tab or project the user has already left.
    navigation_revision: u64,
    pending_project_blanks: HashSet<String>,
    new_tab_task: Option<gpui::Task<()>>,
    worktree_focus: HashMap<String, crate::worktree_switcher::WorktreeFocus>,
    session_worktrees: HashMap<i64, Option<String>>,
    session_details: HashMap<i64, monocode_core::store::MobileSessionMetadata>,
    sidebar_hidden_sessions: HashSet<i64>,
    worktree_returns: HashMap<(String, String), i64>,
    tab_drag: Option<TabDrag>,
    /// Tabs sliding between slots: neighbours making room for a drag and,
    /// after a drop, every tab gliding from where it was drawn into place.
    tab_shifts: HashMap<TitleTab, TabShift>,
    pane_ticks: HashMap<i64, PaneTick>,
    skip_tab_click: bool,
    spaces_start: usize,
    /// The spaces bar's short nudge after a chevron pages it.
    spaces_nudge: Option<(f32, std::time::Instant)>,
    /// The space under the pointer, whose name and shortcuts show above it.
    hovered_space: Option<String>,
    space_anchor: AnchorSlot,
    error: Option<String>,
    tab_scroll: ScrollHandle,
    tab_scroll_task: Option<gpui::Task<()>>,
    resize: Option<(Pixels, f32)>,
    pub(crate) git: Entity<crate::git_status::ProjectGit>,
    pub(crate) right_panel: Option<Entity<RightPanel>>,
    right_resize: Option<(Pixels, f32)>,
    /// Either right-panel handle is hovered; both of its hairlines light up.
    right_resize_hovered: bool,
    right_slide: Option<PaneSlide>,
    reviews: HashMap<i64, Entity<crate::review::ReviewPane>>,
    /// A review sliding in or out: its session and pane.
    review_slide: Option<(i64, PaneSlide, Entity<crate::review::ReviewPane>)>,
    /// A session review card's Review, opened beside its pane next frame.
    pending_review: Option<(i64, crate::review::ReviewTab)>,
    pending_composer_focus: Option<i64>,
    review_focused: bool,
    review_drag: bool,
    split_bounds: HashMap<i64, AnchorSlot>,
    pane_bounds: AnchorSlot,
    /// The visible Mono's view, a drop target for tabs and panes.
    mono_bounds: AnchorSlot,
    pane_drag: Option<PaneDrag>,
    pane_sash: Option<SashDrag>,
    pane_motion: Option<PaneMotion>,
    skip_session_click: bool,
    /// Live terminals of each project's dock, in the dock's tab order.
    terminals: HashMap<String, Vec<Entity<Terminal>>>,
    /// A project's dock sliding open (toward 1) or shut (toward 0).
    dock_slide: Option<(String, PaneSlide)>,
    dock_resize: Option<(f32, f32)>,
    dock_hover: Option<usize>,
    /// Tab animations for the dock's terminals, with each tab's last title.
    dock_tab_motion: crate::tab_motion::TabMotion<gpui::EntityId, SharedString>,
    dock_drag: Option<terminal_dock::DockTabDrag>,
    /// The right panel's drawn width this frame, 0 while it is hidden.
    right_drawn: f32,
    /// Classic: the corner tab opening out and the one folding away.
    corner_reveal: Option<crate::project_tabs::TabReveal>,
    dock_shifts: HashMap<gpui::EntityId, TabShift>,
    dock_skip_click: bool,
    dock_tab_scroll: ScrollHandle,
    move_terminal_anchor: AnchorSlot,
    terminal_chip_anchor: AnchorSlot,
    live_mark: Option<Entity<terminal_dock::LiveMark>>,
    /// The sidebar collapsed itself to make room; it returns with the room.
    sidebar_auto_hidden: bool,
    /// The room the sidebar was last fitted to; a change refits it.
    sidebar_fitted_room: Option<(i32, bool)>,
    viewport_width: f32,
    viewport_height: f32,
    observations: Vec<Subscription>,
    generation: Arc<AtomicU64>,
    latest_state: Arc<Mutex<String>>,
    pub(crate) render_passes: usize,
}
impl Shell {
    #[cfg(test)]
    pub(crate) fn new(
        store: Database,
        summaries: Vec<Session>,
        cwd: String,
        saved: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let paths = summaries
            .iter()
            .map(|s| (s.id, s.cwd.clone()))
            .collect::<Vec<_>>();
        let workspace = Workspace::restore(saved.as_deref(), &paths, &cwd);
        let mut projects = workspace.projects;
        if !projects.contains(&workspace.project) {
            projects.push(workspace.project);
        }
        let metadata = locked(&store)
            .map(|db| db.session_sidebar_metadata(&projects))
            .unwrap_or_default();
        Self::new_with_metadata(store, summaries, cwd, saved, metadata, cx)
    }
    pub(crate) fn new_with_metadata(
        store: Database,
        summaries: Vec<Session>,
        cwd: String,
        saved: Option<String>,
        sidebar_metadata: monocode_core::store::SessionSidebarMetadata,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::accounts::install(store.clone(), cx);
        let remote_dir = locked(&store)
            .ok()
            .and_then(|store| store.data_dir().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("monocode-remote-{}", std::process::id()))
            });
        let remote = Arc::new(monocode_core::remote::Connections::new(remote_dir));
        monocode_core::remote::set_default_connections(remote.clone());
        let quit_remote = remote.clone();
        cx.on_app_quit(move |_, cx| {
            let remote = quit_remote.clone();
            cx.background_executor().spawn(async move {
                remote.shutdown();
            })
        })
        .detach();
        let remote_clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                if this
                    .update(cx, |this, cx| this.refresh_remote_projects(cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let metadata: Vec<_> = summaries.iter().map(|s| (s.id, s.cwd.clone())).collect();
        let workspace = Workspace::restore(saved.as_deref(), &metadata, &cwd);
        monocode_core::cli::initialize(&workspace.preferences.binary_paths);
        let latest_state = Arc::new(Mutex::new(serde_json::to_string(&workspace).unwrap()));
        let quit_state = latest_state.clone();
        let quit_store = store.clone();
        // This application-level hook survives the last window/entity being released.
        let app: &mut App = cx;
        app.on_app_quit(move |cx| {
            let state = quit_state.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let store = quit_store.clone();
            cx.background_executor().spawn(async move {
                let _ = locked(&store).and_then(|s| s.save_ui_state(&state));
            })
        })
        .detach();
        let owner = cx.weak_entity();
        let sidebar_frame = cx.new(|_| Chrome {
            owner: owner.clone(),
            kind: ChromeKind::Sidebar,
            renders: 0,
        });
        let tabs_frame = cx.new(|_| Chrome {
            owner: owner.clone(),
            kind: ChromeKind::Tabs,
            renders: 0,
        });
        let projects_frame = cx.new(|_| Chrome {
            owner,
            kind: ChromeKind::Projects,
            renders: 0,
        });
        let active_kind = summaries
            .iter()
            .find(|s| Some(s.id) == workspace.active)
            .map(|s| Kind::from_id(&s.provider))
            .unwrap_or(Kind::Codex);
        let session_clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.poll_reminders(cx);
                        this.poll_automations(cx);
                        this.update_chat_visibility(cx);
                        if this.workspace.sidebar_visible {
                            this.notify_sidebar(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let git_project = workspace.project.clone();
        let git = cx.new(|cx| crate::git_status::ProjectGit::new(git_project, cx));
        let git_observation = cx.observe(&git, |this, git, cx| {
            let cwd = git.read(cx).root.clone();
            for view in this.sessions.values() {
                if view.read(cx).review.read(cx).session().1 == cwd {
                    view.update(cx, |view, cx| view.refresh_git_actions(cx));
                }
            }
            // The title bar's dot and the panel's tab label follow its stats.
            this.sync_project_rail_git_stats(cx);
            if this.workspace.classic_layout {
                this.sidebar_frame.update(cx, |_, cx| cx.notify());
            }
            cx.notify();
        });
        let resources = cx.new(ResourceMonitor::new);
        let resource_observation = cx.observe(&resources, |_, _, cx| cx.notify());
        let resource_stops = cx.subscribe(&resources, |this, _, StopSession(id), cx| {
            if let Some(session) = this.sessions.get(id) {
                session.update(cx, |session, cx| session.stop_if_busy(cx));
            } else if let Some(m) = this.monos.clone() {
                m.update(cx, |m, cx| m.stop_session(*id, cx));
            }
        });
        let worktree_focus = workspace.worktree_focus.clone();
        let session_worktrees = sidebar_metadata.worktrees;
        let notification_state = notifications::State::new(&workspace, store.clone(), cx);
        let classic_session_search = classic_sidebar::session_search(cx);
        let classic_search_subscription =
            classic_sidebar::bind_session_search(&classic_session_search, cx);
        let classic_sidebar_tab = workspace
            .classic_sidebar_tabs
            .get(&workspace.project)
            .map(|id| classic_sidebar::ClassicSidebarTab::from_id(id))
            .unwrap_or_default();
        let mut this = Self {
            monos: None,
            quick_open: None,
            image_lightbox: None,
            app_bridge: None,
            mobile: mobile_host::Mobile::default(),
            app_results: HashMap::new(),
            app_pending: HashSet::new(),
            orchestrations: HashMap::new(),
            orchestration_summaries: sidebar_metadata.orchestrations,
            started_by: sidebar_metadata.started_by,
            rails: Default::default(),
            usage_footer: cx.new(|cx| UsageFooter::new(active_kind, cx)),
            resources,
            sidebar_frame,
            projects_frame,
            project_rail_list: project_rail::ProjectRailList::new(cx),
            project_rail_drag: None,
            space_reorder: Default::default(),
            project_rail_stats: Default::default(),
            project_resize: None,
            classic_sidebar_tab,
            classic_tabs: Default::default(),
            classic_session_search,
            classic_worktree_picker: None,
            sidebar_list: sidebar_list::SidebarList::new(cx),
            sidebar_slide: None,
            sidebar_fraction: if workspace.sidebar_visible { 1. } else { 0. },
            sidebar_peek: None,
            peek_animating: false,
            peek_close: None,
            peek_toggle_bounds: AnchorSlot::default(),
            fullscreen: false,
            window_controls: 0.,
            right_panel_min: crate::right_panel::WIDTH_MIN,
            #[cfg(target_os = "linux")]
            client_decorations: false,
            tabs_frame,
            store,
            remote,
            remote_project_picker: None,
            remote_refreshing: false,
            remote_marks: HashMap::new(),
            _remote_clock: remote_clock,
            sessions: HashMap::new(),
            session_summaries: HashMap::new(),
            draft_sessions: sidebar_metadata.drafts,
            history_recency: HashMap::new(),
            history_clock: 0,
            session_marks: HashMap::new(),
            tab_marks: HashMap::new(),
            live_marks: HashMap::new(),
            live_agents_expanded: false,
            live_tick: None,
            live_next_change: 1000,
            busy_sessions: HashSet::new(),
            notification_state,
            quick: Default::default(),
            unseen_finished: HashSet::new(),
            archiving: HashSet::new(),
            archive_shortcut_held: false,
            external_editors: None,
            groups: sidebar_metadata.groups,
            reminders: sidebar_metadata.reminders,
            renaming: None,
            link_dialog: None,
            action_dialog: None,
            actions_anchor: AnchorSlot::default(),
            action_terminals: HashMap::new(),
            deleting: HashSet::new(),
            group_writes: None,
            selected_sessions: HashSet::new(),
            selection_anchor: None,
            context_selection: false,
            sidebar_target: None,
            session_row_bounds: HashMap::new(),
            folder_bounds: HashMap::new(),
            sidebar_drop: None,
            folder_drag: None,
            skip_folder_click: false,
            filter_anchor: AnchorSlot::default(),
            folder_picker: None,
            pending_folder: None,
            _session_clock: session_clock,
            notes: None,
            inbox: None,
            automations: None,
            active_app_view: None,
            app_view_events: Vec::new(),
            automation_state: Default::default(),
            settings: None,
            settings_events: None,
            settings_overlay_watch: None,
            app_search: None,
            saved_search: None,
            order: Vec::new(),
            workspace,
            focus: cx.focus_handle(),
            sidebar_focus: cx.focus_handle(),
            query: String::new(),
            sidebar_hover: None,
            tab_hover: None,
            title_tab_motion: Default::default(),
            title_tab_selection: (None, None, std::time::Instant::now()),
            session_hover: None,
            session_preview: None,
            preview_close: None,
            preview_open: None,
            sidebar_last_scroll: None,
            window_observed: false,
            shortcuts: None,
            window_active: false,
            window_glass: None,
            theme_project: None,
            quit_prompted: false,
            menu: None,
            background_dialog: None,
            backgrounds: HashMap::new(),
            backdrop: None,
            background_card: AnchorSlot::default(),
            project_anchor: AnchorSlot::default(),
            sidebar_options_anchor: AnchorSlot::default(),
            history: VisitHistory::default(),
            navigation_revision: 0,
            pending_project_blanks: HashSet::new(),
            new_tab_task: None,
            worktree_focus,
            session_worktrees,
            session_details: sidebar_metadata.details,
            sidebar_hidden_sessions: sidebar_metadata.sidebar_hidden,
            worktree_returns: HashMap::new(),
            tab_drag: None,
            tab_shifts: HashMap::new(),
            pane_ticks: HashMap::new(),
            skip_tab_click: false,
            spaces_start: 0,
            spaces_nudge: None,
            hovered_space: None,
            space_anchor: AnchorSlot::default(),
            error: None,
            tab_scroll: ScrollHandle::new(),
            tab_scroll_task: None,
            resize: None,
            git,
            right_panel: None,
            right_resize: None,
            right_resize_hovered: false,
            right_slide: None,
            reviews: HashMap::new(),
            review_slide: None,
            pending_review: None,
            pending_composer_focus: None,
            review_focused: false,
            review_drag: false,
            split_bounds: HashMap::new(),
            pane_bounds: AnchorSlot::default(),
            mono_bounds: AnchorSlot::default(),
            pane_drag: None,
            pane_sash: None,
            pane_motion: None,
            skip_session_click: false,
            sidebar_auto_hidden: false,
            sidebar_fitted_room: None,
            terminals: HashMap::new(),
            dock_slide: None,
            dock_resize: None,
            dock_hover: None,
            dock_tab_motion: Default::default(),
            dock_drag: None,
            right_drawn: 0.,
            corner_reveal: None,
            dock_shifts: HashMap::new(),
            dock_skip_click: false,
            dock_tab_scroll: ScrollHandle::new(),
            move_terminal_anchor: AnchorSlot::default(),
            terminal_chip_anchor: AnchorSlot::default(),
            live_mark: None,
            viewport_width: 1200.,
            viewport_height: 800.,
            observations: vec![
                git_observation,
                resource_observation,
                resource_stops,
                classic_search_subscription,
            ],
            generation: Arc::new(AtomicU64::new(0)),
            latest_state,
            render_passes: 0,
        };
        if !cfg!(test) {
            this.scan_external_editors(cx);
        }
        this.observe_accounts(cx);
        this.observe_picker_preferences(cx);
        let mono_owner = cx.weak_entity();
        let mono_active =
            this.workspace
                .scope_focus
                .get(&this.tab_scope())
                .and_then(|tab| match tab {
                    TitleTab::Mono { id, .. } => Some(id.clone()),
                    _ => None,
                });
        let monos = cx.new(|cx| {
            crate::monos::Monos::new(
                this.store.clone(),
                mono_owner,
                this.workspace.preferences.clone(),
                mono_active,
                this.notification_state.region.clone(),
                cx,
            )
        });
        this.observations.push(
            cx.subscribe(&monos, |this, _, _: &crate::monos::Changed, cx| {
                this.sync_mono_search(cx);
                this.update_usage_provider(cx);
                this.invalidate_chrome(cx);
                cx.notify();
            }),
        );
        this.observations.push(cx.subscribe(
            &monos,
            |this, _, opened: &crate::monos::TabOpened, cx| {
                this.mono_opened(opened.0.clone(), cx);
            },
        ));
        this.observations.push(cx.subscribe(
            &monos,
            |_, _, request: &crate::monos::ShowDetails, cx| {
                let id = request.0.clone();
                cx.spawn(async move |this, cx| {
                    let _ = this.update_in(cx, |this, window, cx| {
                        if this
                            .monos
                            .as_ref()
                            .is_some_and(|m| m.read(cx).active.as_ref() == Some(&id))
                        {
                            this.set_right_panel_open(true, window, cx);
                        }
                    });
                })
                .detach();
            },
        ));
        let mut mono_chrome = monos.read(cx).overview_chrome();
        let mut mono_error = monos.read(cx).error.clone();
        let mut mono_session = monos.read(cx).routed_session();
        this.observations
            .push(cx.observe(&monos, move |this, monos, cx| {
                let session = monos.read(cx).routed_session();
                if session != mono_session {
                    mono_session = session;
                    this.update_chat_visibility(cx);
                }
                let chrome = monos.read(cx).overview_chrome();
                if chrome != mono_chrome {
                    mono_chrome = chrome;
                    this.invalidate_chrome(cx);
                    cx.notify();
                }
                let error = &monos.read(cx).error;
                if *error != mono_error {
                    mono_error = error.clone();
                    cx.notify();
                }
            }));
        this.monos = Some(monos);
        this.bind_notices(cx);
        this.init_app_bridge(cx);
        for summary in summaries {
            if this
                .monos
                .as_ref()
                .is_some_and(|m| m.read(cx).is_mono_session(summary.id))
            {
                continue;
            }
            this.order.push(summary.id);
            this.session_summaries.insert(summary.id, summary);
        }
        this.init_mobile_host(cx);
        if let Some(id) = this.workspace.active {
            this.ensure_session(this.workspace.session_for_pane(id), cx);
            this.record_worktree_session(id, cx);
        }
        if let Some(id) = this.workspace.active {
            this.history.record(id);
        }
        this.reveal_active_tab(cx);
        this.update_chat_visibility(cx);
        this.update_usage_provider(cx);
        this.apply_preferences(cx);
        this.poll_reminders(cx);
        this.initialize_automations(cx);
        if this.workspace.active.is_none() {
            cx.spawn(async move |this, cx| {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.ensure_project_session(window, cx);
                    this.persist(cx);
                });
            })
            .detach();
        }
        this
    }
    fn update_chat_visibility(&mut self, cx: &mut Context<Self>) {
        if let Some(inbox) = &self.inbox {
            let active = self.active_app_view == Some(app_views::AppView::Inbox)
                && self.settings.is_none()
                && self.app_search.is_none();
            inbox.update(cx, |inbox, cx| inbox.set_active(active, cx));
        }
        // Every pane of the active tab is on screen, not just the focused one.
        let mut shown: HashSet<i64> = match self.workspace.active_tab() {
            Some(tab)
                if !self.app_view_open()
                    && self.settings.is_none()
                    && self.app_search.is_none()
                    && !self.mono_active(cx) =>
            {
                self.workspace.layout(tab).leaf_ids().into_iter().collect()
            }
            _ => HashSet::new(),
        };
        // A session opened in the Mono details panel is on screen too. Left
        // out, it was trimmed as inactive and the panel showed it empty.
        if self.mono_active(cx)
            && self.workspace.right_panel_open
            && !self.app_view_open()
            && self.settings.is_none()
            && self.app_search.is_none()
        {
            shown.extend(
                self.monos
                    .as_ref()
                    .and_then(|m| m.read(cx).routed_session()),
            );
        }
        // Hidden tabs keep their prepared image and any in-flight load. Only
        // closing a tab/pane should discard its background.
        let open: HashSet<i64> = self.workspace.open_sessions().into_iter().collect();
        self.backgrounds.retain(|id, _| open.contains(id));
        for &id in &shown {
            self.ensure_session(id, cx);
            self.history_clock += 1;
            self.history_recency.insert(id, self.history_clock);
        }
        for (&id, view) in &self.sessions {
            view.update(cx, |v, cx| {
                if shown.contains(&id) {
                    v.load(cx);
                }
                v.transcript
                    .update(cx, |t, cx| t.visible(shown.contains(&id), cx))
            });
        }
        self.trim_session_histories(&shown, cx);
    }
    fn add(&mut self, summary: Session, cx: &mut Context<Self>) {
        self.workspace.add_project(&summary.cwd);
        self.add_loaded(summary, cx);
    }
    /// Loading history must not turn every session directory into a rail project.
    fn add_loaded(&mut self, summary: Session, cx: &mut Context<Self>) {
        let id = summary.id;
        if self.sessions.contains_key(&id) {
            return;
        }
        self.session_summaries.insert(id, summary.clone());
        self.session_marks
            .insert(id, cx.new(|_| SessionMark::new(summary.provider.clone())));
        self.tab_marks
            .insert(id, cx.new(|_| SessionMark::tab(summary.provider.clone())));
        let appearance = self.appearance(&summary.cwd);
        let worktree = self.session_worktrees.get(&id).cloned().flatten();
        let view = cx.new(|cx| Conversation::new(summary, self.store.clone(), cx));
        view.update(cx, |view, cx| {
            // Events may arrive before the first asynchronous agent load. Seed
            // the indexed working directory so those events cannot erase it.
            view.restore_worktree(worktree, None, cx);
            view.draft_hint = self.draft_sessions.contains(&id);
            if let Some(bridge) = &self.app_bridge {
                view.set_app_environment(bridge.environment(id));
            }
            view.set_project_appearance(appearance, cx);
            view.transcript.update(cx, |transcript, cx| {
                transcript.apply_preferences(&self.workspace.preferences, cx)
            });
        });
        self.observations
            .push(cx.subscribe(&view, move |this, _, _: &SessionChanged, cx| {
                let Some(view) = this.sessions.get(&id) else {
                    return;
                };
                this.session_worktrees
                    .insert(id, view.read(cx).settings.worktree_cwd.clone());
                // A finished turn may have edited files or moved to a worktree.
                if this.workspace.active == Some(id) {
                    this.sync_git(cx);
                    this.git.update(cx, |git, cx| git.reload(cx));
                }
                this.sync_search_session(id, cx);
                this.orchestration_session_changed(id, cx);
                this.automation_session_changed(id, cx);
                this.sync_review_locks(cx);
                this.refresh_session_mark(id, cx);
                this.notification_session_changed(id, cx);
                this.update_usage_provider(cx);
                this.update_chat_visibility(cx);
                this.invalidate_chrome(cx);
                cx.notify();
            }));
        self.observations
            .push(cx.subscribe(&view, |this, _, transfer: &Transfer, cx| {
                this.transfer_response(transfer.clone(), cx)
            }));
        self.observations.push(cx.subscribe(
            &view,
            move |this, _, event: &crate::conversation::OpenSessionChanges, cx| {
                let tab = event.0.clone();
                // The card's pane takes focus, so its review opens beside it.
                this.pending_review = Some((id, tab));
                cx.notify();
            },
        ));
        self.observations
            .push(cx.observe(&view, move |this, _, cx| {
                if this
                    .session_preview
                    .as_ref()
                    .is_some_and(|preview| preview.id == id)
                {
                    this.sidebar_frame.update(cx, |_, cx| cx.notify());
                }
            }));
        self.observe_orchestration(&view, id, cx);
        self.observe_session_folders(&view, id, cx);
        self.sessions.insert(id, view);
        if !self.order.contains(&id) {
            self.order.push(id);
        }
    }
    /// SessionPane's reviewUndoLocked: another running session shares the checkout.
    fn sync_review_locks(&mut self, cx: &mut Context<Self>) {
        let running: Vec<(i64, String)> = self
            .sessions
            .iter()
            .filter(|(_, view)| view.read(cx).busy)
            .map(|(&id, view)| (id, view.read(cx).review.read(cx).session().1.to_owned()))
            .collect();
        for (&id, view) in &self.sessions {
            let review = view.read(cx).review.clone();
            let cwd = review.read(cx).session().1.to_owned();
            let locked = running
                .iter()
                .any(|(other, path)| *other != id && *path == cwd);
            review.update(cx, |review, cx| review.set_undo_locked(locked, cx));
        }
    }
    fn refresh_session_mark(&mut self, id: i64, cx: &mut Context<Self>) {
        let id = self.workspace.session_for_pane(id);
        let Some(view) = self.sessions.get(&id) else {
            return;
        };
        let session = view.read(cx);
        let busy = session.responding();
        let focused = !self.app_view_open() && self.workspace.active == Some(id);
        if busy {
            self.busy_sessions.insert(id);
            self.unseen_finished.remove(&id);
        } else if session.busy {
            // Explicit cancellation is already idle in the UI. The worker is
            // only finishing its save, so it must not announce a completed turn.
            self.busy_sessions.remove(&id);
            self.unseen_finished.remove(&id);
        } else if self.busy_sessions.remove(&id) && !focused && !session.summary.archived {
            self.unseen_finished.insert(id);
        }
        if focused || session.summary.archived {
            self.unseen_finished.remove(&id);
        }
        let state = if busy {
            MarkState::Working
        } else if self.unseen_finished.contains(&id) {
            MarkState::Done
        } else {
            MarkState::Idle
        };
        // A blocked turn is still busy, but the tab asks for attention first.
        let attention = session.needs_approval();
        let tab_state = if attention {
            MarkState::Attention
        } else {
            state
        };
        let provider = session.summary.provider.clone();
        self.session_marks[&id].update(cx, |mark, cx| mark.set(&provider, state, cx));
        self.tab_marks[&id].update(cx, |mark, cx| mark.set(&provider, tab_state, cx));
    }
    fn toggle_archive(&mut self, id: i64, cx: &mut Context<Self>) {
        if !self.archiving.insert(id) {
            return;
        }
        self.close_preview(cx);
        let archived = !self.session_metadata(id, cx).summary.archived;
        let store = self.store.clone();
        let work = cx
            .background_executor()
            .spawn(async move { locked(&store)?.set_archived(id, archived) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.archiving.remove(&id);
                match result {
                    Ok(()) => {
                        if let Some(summary) = this.session_summaries.get_mut(&id) {
                            summary.archived = archived;
                        }
                        if let Some(view) = this.sessions.get(&id) {
                            view.update(cx, |session, cx| {
                                session.summary.archived = archived;
                                if archived {
                                    session.stop_if_busy(cx);
                                }
                            });
                        }
                        this.refresh_session_mark(id, cx);
                        if archived && this.workspace.tab_of(id).is_some() {
                            this.close_pane(id, window, cx);
                        } else {
                            this.invalidate_chrome(cx);
                            cx.notify();
                        }
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not update archive: {error}"));
                        this.invalidate_chrome(cx);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        self.invalidate_chrome(cx);
    }
    fn transfer_response(&mut self, transfer: Transfer, cx: &mut Context<Self>) {
        use monocode_core::{
            agent::{Handoff, Settings},
            response::{self, TransferKind},
        };
        let navigation_revision = self.navigation_revision;
        let store = self.store.clone();
        let target_kind = Kind::from_id(&transfer.target.provider);
        let account = crate::accounts::get(cx)
            .map(|center| center.read(cx).selected(target_kind, &transfer.cwd))
            .unwrap_or_default();
        let work = cx.background_executor().spawn(async move {
            let mut settings = Settings {
                account,
                revision: response::now_ms() * 1_000_000,
                model: transfer.target.model.clone(),
                effort: transfer.target.effort.clone(),
                model_settings: transfer.target.settings.clone(),
                access: transfer.settings.access,
                worktree_cwd: transfer.settings.worktree_cwd.clone(),
                ..Settings::default()
            };
            let prompt = if transfer.kind == TransferKind::SecondOpinion {
                Some(response::second_opinion(&transfer.from, &transfer.turn))
            } else {
                settings.handoff = Some(Handoff {
                    from: transfer.from.clone(),
                    brief: response::handoff(&transfer.through),
                });
                None
            };
            let mut summary = locked(&store)?.create(&transfer.target.provider, &transfer.cwd)?;
            summary.title = if transfer.kind == TransferKind::SecondOpinion {
                "Second opinion".into()
            } else {
                transfer.title
            };
            locked(&store)?.title(summary.id, &summary.title)?;
            locked(&store)?.configure_agent(summary.id, &summary.provider, &settings)?;
            Ok::<_, String>((summary, settings, prompt))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok((summary, settings, prompt)) => {
                    let id = summary.id;
                    this.add(summary, cx);
                    this.sessions[&id]
                        .update(cx, |view, cx| view.install_transfer(settings, prompt, cx));
                    this.order.retain(|s| *s != id);
                    this.order.insert(0, id);
                    if this.navigation_revision == navigation_revision {
                        this.activate_session(id, window, cx);
                    }
                }
                Err(error) => {
                    this.error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn notify_sidebar(&self, cx: &mut Context<Self>) {
        self.sidebar_list.invalidate_structure();
        self.sidebar_frame.update(cx, |_, cx| cx.notify());
    }
    fn invalidate_chrome(&self, cx: &mut Context<Self>) {
        self.notify_sidebar(cx);
        self.tabs_frame.update(cx, |_, cx| cx.notify());
        if self.workspace.classic_layout {
            self.projects_frame.update(cx, |_, cx| cx.notify());
        }
    }
    fn reveal_active_tab(&self, cx: &App) {
        if self.workspace.classic_layout {
            self.reveal_active_project_rail();
        }
        if let Some(ix) = self
            .visible_tabs(cx)
            .iter()
            .position(|tab| Some(tab) == self.active_title_tab(cx).as_ref())
        {
            self.tab_scroll.scroll_to_item(ix);
        }
    }
    /// Sidebar.tsx's `gitRoot`: the active session's worktree or folder.
    fn sync_git(&mut self, cx: &mut Context<Self>) {
        self.classic_sidebar_tab = self
            .workspace
            .classic_sidebar_tabs
            .get(&self.workspace.project)
            .map(|id| classic_sidebar::ClassicSidebarTab::from_id(id))
            .unwrap_or_default();
        let session = self
            .workspace
            .active
            .and_then(|id| self.sessions.get(&self.workspace.session_for_pane(id)))
            .map(|view| {
                let view = view.read(cx);
                (
                    view.settings
                        .worktree_cwd
                        .clone()
                        .unwrap_or_else(|| view.summary.cwd.clone()),
                    Kind::from_id(&view.summary.provider),
                )
            });
        let (project, provider) = match session {
            Some((cwd, kind)) => (cwd, Some(kind)),
            None => (self.working_copy().to_owned(), None),
        };
        self.git.update(cx, |git, cx| git.set_project(project, cx));
        if let (Some(panel), Some(kind)) = (&self.right_panel, provider) {
            let changes = panel.read(cx).changes.clone();
            changes.update(cx, |c, _| c.text_provider = kind);
        }
        self.sync_right_panel(cx);
    }
    /// App.tsx's `insetRightPanelShown`: open (or sliding shut), over a
    /// project, and not covered by an app view.
    fn right_panel_shown(&self, cx: &App) -> bool {
        (self.workspace.right_panel_open || self.right_slide.is_some())
            && (!self.workspace.classic_layout || self.mono_active(cx))
            && !self.app_view_open()
            && self.settings.is_none()
            && self.app_search.is_none()
            && (self.mono_active(cx) || self.git.read(cx).available())
    }
    fn sync_right_panel(&mut self, cx: &mut Context<Self>) {
        let opening = self.right_slide.as_ref().is_some_and(PaneSlide::opening);
        let shown = !self.mono_active(cx)
            && !self.workspace.classic_layout
            && self.workspace.right_panel_open
            && !self.app_view_open()
            && self.settings.is_none()
            && self.app_search.is_none()
            && !opening;
        if let Some(panel) = &self.right_panel {
            let sidebar_tab = (self.workspace.classic_layout
                && self.session_sidebar_available(cx)
                && self.workspace.sidebar_visible
                && self.settings.is_none())
            .then(|| self.classic_sidebar_tab.tool_tab())
            .flatten();
            let terminal_tab = self.terminal_in_panel(cx);
            panel.update(cx, |p, cx| {
                p.set_shown(shown, cx);
                p.set_sidebar_tab(sidebar_tab, cx);
                p.set_terminal_tab(terminal_tab, cx);
            });
        }
    }
    fn select_classic_sidebar_tab(
        &mut self,
        tab: classic_sidebar::ClassicSidebarTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_right_panel(window, cx);
        self.leave_app_views(cx);
        self.close_app_search(window, cx);
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |monos, cx| monos.close(cx));
        }
        self.classic_sidebar_tab = tab;
        self.workspace
            .classic_sidebar_tabs
            .insert(self.workspace.project.clone(), tab.id().into());
        self.close_preview(cx);
        self.sidebar_auto_hidden = false;
        self.set_sidebar_visible(true, cx);
        self.sync_right_panel(cx);
        self.notify_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    fn classic_tool_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(panel) = &self.right_panel else {
            return div().into_any_element();
        };
        let panel = panel.read(cx);
        match self.classic_sidebar_tab {
            classic_sidebar::ClassicSidebarTab::Files => panel
                .explorer
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            classic_sidebar::ClassicSidebarTab::Changes => panel
                .changes
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            classic_sidebar::ClassicSidebarTab::Sessions => div().into_any_element(),
        }
    }
    fn toggle_right_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_right_panel_open(!self.workspace.right_panel_open, window, cx);
    }
    fn set_right_panel_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_view_open()
            || (!self.mono_active(cx) && !self.git.read(cx).available())
            || self.workspace.right_panel_open == open
        {
            return;
        }
        let details_focus =
            if let Some(monos) = self.monos.as_ref().filter(|m| m.read(cx).active.is_some()) {
                Some(monos.read(cx).details.focus_handle(cx))
            } else {
                self.right_panel.as_ref().map(|p| p.focus_handle(cx))
            };
        if !open
            && details_focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
        {
            self.focus_active(window, cx);
        }
        let full = self.right_panel_width();
        let to = if open { full } else { 0. };
        self.workspace.right_panel_open = open;
        let previous = self.right_slide.take();
        self.right_slide = (!cx.reduce_motion()).then(|| match previous {
            Some(slide) => slide.retarget(to, std::time::Instant::now()),
            None => PaneSlide::new(if open { 0. } else { full }, to, full),
        });
        self.sync_right_panel(cx);
        if open
            && !self.mono_active(cx)
            && let Some(panel) = self.right_panel.clone()
        {
            panel.update(cx, |p, cx| p.focus_pane(window, cx));
        }
        self.fit_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    fn ensure_right_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right_panel.is_some() {
            return;
        }
        let git = self.git.clone();
        let (tab, order, view) = (
            self.workspace.right_panel_tab,
            self.workspace.right_panel_tabs.clone(),
            self.workspace.changes_view,
        );
        let terminal_tab = self.terminal_in_panel(cx);
        let panel = cx.new(|cx| {
            let mut panel = RightPanel::new(git, tab, order, window, cx);
            panel.set_terminal_tab(terminal_tab, cx);
            panel
        });
        let explorer = panel.read(cx).explorer.clone();
        self.observations.push(cx.subscribe(
            &explorer,
            |this, _, event: &crate::explorer::FileActionError, cx| {
                this.notification_state
                    .file_error
                    .update(cx, |notice, cx| notice.set_message(event.0.clone(), cx));
                cx.notify();
            },
        ));
        self.observations.push(cx.subscribe_in(
            &explorer,
            window,
            |this, _, event: &crate::explorer::OpenFile, window, cx| {
                this.open_review(
                    crate::review::ReviewTab::File {
                        cwd: this.git.read(cx).project().into(),
                        path: event.0.clone(),
                    },
                    window,
                    cx,
                );
            },
        ));
        self.observations.push(cx.subscribe_in(
            &explorer,
            window,
            |this, _, event: &crate::explorer::OpenSearchMatch, window, cx| {
                this.open_review_with_preview(
                    crate::review::ReviewTab::File {
                        cwd: event.cwd.clone(),
                        path: event.matched.path.clone(),
                    },
                    Some(event.pin),
                    window,
                    cx,
                );
                if let Some(pane) = this.active_review(cx) {
                    pane.update(cx, |pane, cx| {
                        pane.navigate_file(
                            event.matched.line as usize,
                            event.matched.column as usize,
                            cx,
                        )
                    });
                }
            },
        ));
        let changes = panel.read(cx).changes.clone();
        changes.update(cx, |c, cx| c.set_view(view, cx));
        self.observations
            .push(cx.subscribe(&panel, |this, _, event: &PanelEvent, cx| {
                match event {
                    PanelEvent::Tab(tab) => {
                        this.workspace.right_panel_tab = *tab;
                        if *tab == crate::right_panel::RightTab::Terminal {
                            this.ensure_panel_terminal(cx);
                        }
                    }
                    PanelEvent::Reordered(order) => this.workspace.right_panel_tabs = order.clone(),
                }
                this.persist(cx);
            }));
        self.observations.push(cx.subscribe_in(
            &changes,
            window,
            |this, _, event: &crate::changes::ChangesEvent, window, cx| match event {
                crate::changes::ChangesEvent::View(view) => {
                    this.workspace.changes_view = *view;
                    this.persist(cx);
                }
                crate::changes::ChangesEvent::OpenDiff { cwd, path, staged } => {
                    this.open_review(
                        crate::review::ReviewTab::Changes {
                            cwd: cwd.clone(),
                            path: path.clone(),
                            staged: Some(*staged),
                        },
                        window,
                        cx,
                    );
                }
            },
        ));
        let history = panel.read(cx).history.clone();
        self.observations.push(cx.subscribe_in(
            &history,
            window,
            |this, _, event: &crate::history::HistoryEvent, window, cx| {
                let crate::history::HistoryEvent::OpenCommit {
                    cwd,
                    sha,
                    short_sha,
                    subject,
                } = event;
                this.open_review(
                    crate::review::ReviewTab::Commit {
                        cwd: cwd.clone(),
                        sha: sha.clone(),
                        short_sha: short_sha.clone(),
                        subject: subject.clone(),
                    },
                    window,
                    cx,
                );
            },
        ));
        self.right_panel = Some(panel);
        self.sync_git(cx);
    }
    /// The active tab's review pane, created from its saved state.
    fn unsaved_files(&self, cx: &App) -> bool {
        self.reviews
            .values()
            .any(|pane| pane.read(cx).unsaved_files(cx))
    }
    /// Steps the interface scale one notch up or down, or resets it with 0.
    fn step_ui_scale(&mut self, step: i32, cx: &mut Context<Self>) {
        use crate::settings::UI_SCALE_PERCENTS;
        let current = self.workspace.preferences.ui_scale;
        let next = match step {
            0 => Some(100),
            1 => UI_SCALE_PERCENTS.into_iter().find(|&p| p > current),
            _ => UI_SCALE_PERCENTS.into_iter().rev().find(|&p| p < current),
        }
        .unwrap_or(current);
        if next == current {
            return;
        }
        self.workspace.preferences.ui_scale = next;
        self.apply_preferences(cx);
        self.sync_settings(cx);
        self.persist(cx);
        cx.notify();
    }
    /// The interface scale as a window zoom, applied after the frame that
    /// noticed it so layout never runs against a viewport that changes mid-draw.
    fn sync_ui_scale(&self, window: &mut Window, cx: &mut Context<Self>) {
        let zoom = self.workspace.preferences.ui_scale.clamp(50, 200) as f32 / 100.;
        if (window.content_zoom() - zoom).abs() < f32::EPSILON {
            return;
        }
        window.defer(cx, move |window, cx| {
            window.set_content_zoom(zoom, cx);
            #[cfg(target_os = "macos")]
            {
                let height = crate::theme::TITLE_BAR_HEIGHT;
                window.set_traffic_light_position(crate::traffic_lights::position(zoom, height));
                crate::traffic_lights::pin(window, height);
            }
        });
    }
    /// The native stand-in for the web app's Reload: restart the process.
    /// Unlike a webview reload this stops running agents and terminals, so it
    /// always asks first.
    fn request_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quit_prompted {
            return;
        }
        self.quit_prompted = true;
        let detail = if self.unsaved_files(cx) {
            "Running agents and terminals will stop, and unsaved file edits will be lost."
        } else {
            "Running agents and terminals will stop."
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            "Reload Monocode Native?",
            Some(detail),
            &["Reload", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.quit_prompted = false;
                if matches!(answer, Ok(0)) {
                    crate::restart(cx);
                }
            });
        })
        .detach();
    }
    fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.unsaved_files(cx) {
            crate::quit(cx);
            return;
        }
        if self.quit_prompted {
            return;
        }
        self.quit_prompted = true;
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            "Quit with unsaved file edits?",
            Some("Save your files before quitting to keep your changes."),
            &["Quit Without Saving", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.quit_prompted = false;
                if matches!(answer, Ok(0)) {
                    crate::quit(cx);
                }
            });
        })
        .detach();
    }
    fn active_review(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::review::ReviewPane>> {
        let id = self.workspace.active?;
        self.review_for(id, cx)
    }
    fn review_for(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::review::ReviewPane>> {
        if let Some(review) = self.reviews.get(&id) {
            return Some(review.clone());
        }
        let state = self.workspace.reviews.get(&id)?.clone();
        if state.tabs.is_empty() {
            self.workspace.reviews.remove(&id);
            return None;
        }
        Some(self.make_review(id, state, cx))
    }
    fn make_review(
        &mut self,
        id: i64,
        state: crate::review::ReviewState,
        cx: &mut Context<Self>,
    ) -> Entity<crate::review::ReviewPane> {
        use crate::review::ReviewEvent;
        let git = self.git.clone();
        let review = cx.new(|cx| {
            let mut pane = crate::review::ReviewPane::new(git, state, cx);
            pane.pane_draggable = true;
            pane
        });
        self.observations.push(cx.subscribe(
            &review,
            move |this, review, event: &ReviewEvent, cx| {
                let Some(id) = this
                    .reviews
                    .iter()
                    .find_map(|(id, pane)| (pane == &review).then_some(*id))
                else {
                    return;
                };
                match event {
                    ReviewEvent::WordWrap(wrap) => {
                        this.workspace.preferences.file_word_wrap = *wrap;
                        this.apply_preferences(cx);
                        this.sync_settings(cx);
                        this.persist(cx);
                    }
                    ReviewEvent::Changed => {
                        let state = review.read(cx).state.clone();
                        // A closed last tab never comes back as an empty pane.
                        if state.tabs.is_empty() {
                            return;
                        }
                        this.workspace.reviews.insert(id, state);
                        this.persist(cx);
                    }
                    ReviewEvent::Comment(text) => {
                        let owner = this.workspace.session_for_pane(id);
                        this.add_to_chat(owner, text, cx);
                    }
                    ReviewEvent::Empty => this.close_review(id, cx),
                    ReviewEvent::DragPane(position) => {
                        this.tab_drag = None;
                        this.pane_drag = Some(PaneDrag {
                            source: DragSource::Review(id),
                            start: *position,
                            position: *position,
                            active: false,
                            over: None,
                            insert: None,
                        });
                    }
                }
                cx.notify();
            },
        ));
        self.reviews.insert(id, review.clone());
        review
    }
    /// openChangesTab / openCommitTab in the active tab; the pane slides
    /// in beside the chat when it is new.
    fn open_review(
        &mut self,
        tab: crate::review::ReviewTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_review_with_preview(tab, None, window, cx);
    }
    /// Explorer search previews reuse a clean file tab; explicit opens keep it.
    fn open_review_with_preview(
        &mut self,
        tab: crate::review::ReviewTab,
        pin: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let crate::review::ReviewTab::File { cwd, path } = &tab {
            let recent = self.workspace.recent_files.entry(cwd.clone()).or_default();
            recent.retain(|old| old != path);
            recent.insert(0, path.clone());
            recent.truncate(30);
        }
        // Closing a title tab retains its pane, like a conversation tab.
        // Reopening one of its files must recover the existing dirty editor.
        let retained = self.workspace.review_owners.keys().copied().find(|pane| {
            self.workspace.tab_of(*pane).is_none()
                && self
                    .sessions
                    .get(&self.workspace.session_for_pane(*pane))
                    .is_some_and(|s| s.read(cx).summary.cwd == self.workspace.project)
                && self.in_worktree_scope(*pane, cx)
                && self
                    .workspace
                    .reviews
                    .get(pane)
                    .is_some_and(|r| r.tabs.iter().any(|existing| existing.same_tab(&tab)))
        });
        if let Some(pane) = retained {
            self.open_file_pane(pane, window, cx);
        }
        let Some(id) = self.workspace.active else {
            return;
        };
        let review = match self.review_for(id, cx) {
            Some(review) => review,
            None => {
                let review = self.make_review(id, Default::default(), cx);
                if !cx.reduce_motion() {
                    let ratio = review.read(cx).state.ratio;
                    let distance = self.split_width() * ratio;
                    let slide = match self.review_slide.take() {
                        Some((sid, slide, _)) if sid == id => {
                            slide.retarget(ratio, std::time::Instant::now())
                        }
                        _ => PaneSlide::new(0., ratio, distance),
                    };
                    self.review_slide = Some((id, slide, review.clone()));
                }
                review
            }
        };
        let entering = self
            .review_slide
            .as_ref()
            .is_some_and(|(sid, slide, _)| *sid == id && slide.opening());
        review.update(cx, |r, cx| {
            r.set_deferred(entering, cx);
            if let Some(pin) = pin {
                r.open_file(tab, pin, cx);
            } else {
                r.open(tab, cx);
            }
            r.set_visible(true, cx);
        });
        self.review_focused = true;
        review.update(cx, |r, cx| r.focus_active(window, cx));
        self.fit_sidebar(cx);
        cx.notify();
    }
    /// The pane's last tab closed: it slides out, and a tab without its
    /// chat closes with it.
    fn close_review(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(review) = self.reviews.remove(&id) else {
            return;
        };
        review.update(cx, |r, cx| r.cache_closed(cx));
        let chat_hidden = review.read(cx).state.chat_hidden;
        let ratio = review.read(cx).state.ratio;
        self.workspace.reviews.remove(&id);
        if self.workspace.active == Some(id) {
            self.review_focused = false;
            if !chat_hidden {
                // The closing pane stays mounted for its slide-out. Move focus
                // before it disappears, rather than leaving keys in its editor.
                self.pending_composer_focus = Some(self.workspace.session_for_pane(id));
            }
        }
        if chat_hidden {
            // The event has no Window; use the title tabs' close path once
            // it is available, including project and focus restoration.
            cx.spawn(async move |this, cx| {
                let _ = this.update_in(cx, |this, window, cx| {
                    let Some(tab) = this.workspace.tab_of(id) else {
                        return;
                    };
                    if this.reviews.contains_key(&id) {
                        return;
                    }
                    if this.workspace.layout(tab).is_split() {
                        this.close_pane(id, window, cx);
                    } else {
                        this.close_tabs(&[id], None, window, cx);
                    }
                });
            })
            .detach();
        } else if !cx.reduce_motion() && self.workspace.active == Some(id) {
            let distance = self.split_width() * ratio;
            let slide = match self.review_slide.take() {
                Some((sid, slide, _)) if sid == id => slide.retarget(0., std::time::Instant::now()),
                _ => PaneSlide::new(ratio, 0., distance),
            };
            self.review_slide = Some((id, slide, review));
        }
        self.fit_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    /// requestAddToChat(text, "plain"): appended to the tab's draft as its
    /// own paragraph.
    fn add_to_chat(&mut self, id: i64, text: &str, cx: &mut Context<Self>) {
        let Some(view) = self.ensure_session(id, cx) else {
            return;
        };
        let input = view.read(cx).input.clone();
        if input.read(cx).is_composing() {
            return;
        }
        input.update(cx, |input, cx| {
            let draft = input.content.to_string();
            let separator = if draft.is_empty() || draft.ends_with("\n\n") {
                ""
            } else if draft.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            input.set_text(format!("{draft}{separator}{}\n\n", text.trim()), cx);
        });
        self.chat_hidden_off(id, cx);
        self.pending_composer_focus = Some(id);
        cx.notify();
    }
    fn chat_hidden_off(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(review) = self.reviews.get(&id) {
            review.update(cx, |r, cx| {
                if r.state.chat_hidden {
                    r.state.chat_hidden = false;
                    cx.emit(crate::review::ReviewEvent::Changed);
                }
            });
        }
    }
    /// The session pane's split header (SessionPane.tsx): grip, focus dot,
    /// title and a close button. The header moves the chat independently of
    /// its review; closing a lone chat leaves the review in the tab.
    /// `shown` is the review split's progress: the header grows and fades
    /// with the slide instead of shifting the chat at once.
    fn split_header(
        &self,
        id: i64,
        shown: f32,
        in_split: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = self
            .sessions
            .get(&id)
            .map(|s| s.read(cx).display_title())
            .or_else(|| self.mono_pane_title(id, cx))
            .unwrap_or_default();
        let focused = self.workspace.active == Some(id) && !self.review_focused;
        div()
            .id(("split-chat-header", id as usize))
            .debug_selector(move || format!("split-header-{id}"))
            .h(px(36. * shown))
            .opacity(shown)
            .overflow_hidden()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .px_2()
            .border_b_1()
            .border_color(crate::right_panel::stroke())
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.review_focused = false;
                    this.focus_pane(id, window, cx);
                    this.tab_drag = None;
                    this.pane_drag = Some(PaneDrag {
                        source: DragSource::Pane(id),
                        start: event.position,
                        position: event.position,
                        active: false,
                        over: None,
                        insert: None,
                    });
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id(("drag-session-pane", id as usize))
                    .debug_selector(move || format!("drag-session-pane-{id}"))
                    .role(Role::Button)
                    .aria_label("Drag to reorder pane")
                    .w(px(20.))
                    .h(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .text_color(content(0.35))
                    .hover(|s| s.bg(content(0.05)).text_color(content(0.7)))
                    .child(icon("grip-vertical", 14.)),
            )
            .child(
                div()
                    .size(px(8.))
                    .flex_shrink_0()
                    .rounded_full()
                    .when(focused, |d| d.bg(accent_color())),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(content(1.))
                    .child(title),
            )
            .child(
                div()
                    .id(("close-chat-pane", id as usize))
                    .role(Role::Button)
                    .aria_label("Close pane")
                    .group("close-chat-pane")
                    .size(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.1)))
                    .child(
                        icon("x", 12.)
                            .text_color(content(0.5))
                            .group_hover("close-chat-pane", |s| s.text_color(content(1.))),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if in_split {
                            this.close_pane(id, window, cx);
                        } else if let Some(review) = this.reviews.get(&id).cloned() {
                            this.review_focused = true;
                            review.update(cx, |r, cx| {
                                r.state.chat_hidden = true;
                                r.focus_active(window, cx);
                                cx.emit(crate::review::ReviewEvent::Changed);
                            });
                        }
                    })),
            )
            .into_any_element()
    }
    fn split_width(&self) -> f32 {
        self.workspace
            .active
            .and_then(|id| self.split_bounds.get(&id))
            .and_then(AnchorSlot::get)
            .map_or(800., |b| b.size.width.as_f32())
    }
    /// The card's workspace: the active tab's pane tree, or its one session.
    fn workspace_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(view) = self.app_view_body() {
            return view;
        }
        let Some(id) = self.workspace.active else {
            return div().size_full().into_any_element();
        };
        let tab = self.workspace.tab_of(id).unwrap_or(id);
        let layout = self.workspace.layout(tab);
        if layout.is_split() || self.pane_motion.as_ref().is_some_and(|m| m.tab == tab) {
            return self.pane_tree(tab, layout.leaves(), window, cx);
        }
        let hint = self.drop_hint(id);
        div()
            .size_full()
            .relative()
            .child(self.pane_bounds.layout_probe())
            .child(self.session_surface(id, None, window, cx))
            .children(hint)
            .into_any_element()
    }
    /// PaneTree.tsx: absolutely placed panes, sashes on shared edges and a
    /// drop hint on the pane under a drag.
    fn pane_tree(
        &mut self,
        tab: i64,
        leaves: Vec<LayoutLeaf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let size = self.pane_bounds.get().map_or((800., 600.), |b| {
            (b.size.width.as_f32(), b.size.height.as_f32())
        });
        let motion = match self.pane_motion.as_ref().filter(|m| m.tab == tab) {
            Some(motion) => {
                let (t, done) = motion.slide.at(std::time::Instant::now());
                if done {
                    self.pane_motion = None;
                    None
                } else {
                    window.request_animation_frame();
                    Some(t)
                }
            }
            None => {
                self.pane_motion = None;
                None
            }
        };
        let dragging = self
            .pane_drag
            .as_ref()
            .filter(|d| d.active && !matches!(d.source, DragSource::Review(_)))
            .map(|d| d.source.session());
        let (header, leaving) = match (motion, self.pane_motion.as_ref()) {
            (Some(_), Some(m)) => (m.header, m.leaving.clone()),
            _ => (None, Vec::new()),
        };
        let t = motion.unwrap_or(1.);
        // A sliding pane holds its full size against the edge it rides; its
        // neighbours rewrap as they give way.
        let pinned = |leaf: &LayoutLeaf, edge: Edge, surface: AnyElement| {
            div()
                .absolute()
                .w(px(leaf.rect.w * size.0))
                .h(px(leaf.rect.h * size.1))
                .map(|d| match edge {
                    Edge::Left => d.top_0().right_0(),
                    Edge::Top => d.left_0().bottom_0(),
                    Edge::Right | Edge::Bottom => d.top_0().left_0(),
                })
                .flex()
                .flex_col()
                .child(surface)
        };
        let frame = |id: i64, rect: Rect| {
            div()
                .id(("pane", id as usize))
                .debug_selector(move || format!("pane-{id}"))
                .absolute()
                .left(gpui::relative(rect.x))
                .top(gpui::relative(rect.y))
                .w(gpui::relative(rect.w))
                .h(gpui::relative(rect.h))
                .overflow_hidden()
                .flex()
                .flex_col()
                .border_color(crate::right_panel::stroke())
                .when(rect.x > 0.001, |d| d.border_l_1())
                .when(rect.y > 0.001, |d| d.border_t_1())
        };
        let mut panes = Vec::with_capacity(leaves.len() + leaving.len());
        for leaf in &leaves {
            let id = leaf.id;
            let (rect, entering) = match (motion, self.pane_motion.as_ref()) {
                (Some(t), Some(m)) => (
                    m.from
                        .get(&id)
                        .copied()
                        .unwrap_or(leaf.rect)
                        .lerp(leaf.rect, t),
                    m.entering.get(&id).copied(),
                ),
                _ => (leaf.rect, None),
            };
            // A pane with its review keeps its header either way.
            let shown = if entering.is_some() || self.review_for(id, cx).is_some() {
                1.
            } else {
                match header {
                    Some(true) => t,
                    Some(false) => 1. - t,
                    None => 1.,
                }
            };
            // Once the last neighbour has left, render the lone-pane header
            // state on this frame too, rather than flashing a full header.
            let split = (leaves.len() > 1 || motion.is_some()).then_some(shown);
            let surface = self.session_surface(id, split, window, cx);
            let content = match entering {
                Some(edge) => pinned(leaf, edge, surface),
                None => div().size_full().flex().flex_col().child(surface),
            };
            panes.push(
                frame(id, rect)
                    .when(
                        dragging == Some(id)
                            && self
                                .workspace
                                .reviews
                                .get(&id)
                                .is_none_or(|r| r.chat_hidden),
                        |d| d.opacity(0.4),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| this.focus_pane(id, window, cx)),
                    )
                    .child(content)
                    .children(self.drop_hint(id))
                    .into_any_element(),
            );
        }
        // Leaving panes paint over the neighbours growing into their room.
        for (leaf, edge, to) in &leaving {
            let surface = self.session_surface(leaf.id, Some(1.), window, cx);
            panes.push(
                frame(leaf.id, leaf.rect.lerp(*to, t))
                    .opacity(1. - 0.5 * t)
                    .child(pinned(leaf, *edge, surface))
                    .into_any_element(),
            );
        }
        let sashes = if motion.is_some() {
            Vec::new()
        } else {
            self.workspace.layout(tab).sashes()
        };
        div()
            .id("pane-tree")
            .size_full()
            .relative()
            .child(self.pane_bounds.layout_probe())
            .children(panes)
            .children(sashes.into_iter().map(|sash| {
                let row = sash.dir == SplitDir::Right;
                let g = sash.group;
                let key = sash.path.iter().fold(sash.index, |k, ix| k * 31 + ix + 1);
                div()
                    .id(("pane-sash", key))
                    .group("resize-handle")
                    .role(Role::Splitter)
                    .aria_label("Resize panes")
                    .absolute()
                    .map(|d| {
                        if row {
                            d.left(gpui::relative(g.x + sash.boundary * g.w))
                                .top(gpui::relative(g.y))
                                .h(gpui::relative(g.h))
                                .ml(px(-5.5))
                                .w(px(12.))
                                .cursor(CursorStyle::ResizeLeftRight)
                        } else {
                            d.top(gpui::relative(g.y + sash.boundary * g.h))
                                .left(gpui::relative(g.x))
                                .w(gpui::relative(g.w))
                                .mt(px(-5.5))
                                .h(px(12.))
                                .cursor(CursorStyle::ResizeUpDown)
                        }
                    })
                    .child(resize_border(
                        row,
                        self.pane_sash
                            .as_ref()
                            .is_some_and(|drag| drag.path == sash.path && drag.index == sash.index),
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.pane_sash = Some(SashDrag {
                                tab,
                                path: sash.path.clone(),
                                index: sash.index,
                                dir: sash.dir,
                                group: g,
                            });
                        }),
                    )
            }))
            .into_any_element()
    }
    /// PaneDropHint: half the pane washed toward the edge it will take.
    fn drop_hint(&self, id: i64) -> Option<AnyElement> {
        let drag = self.pane_drag.as_ref().filter(|d| d.active)?;
        let (target, edge) = drag.over?;
        if target != id {
            return None;
        }
        let wash = div().absolute().bg(accent_color()).opacity(0.15);
        let line = div().absolute().bg(accent_color());
        let (wash, line) = match edge {
            Edge::Left => (
                wash.top_0().bottom_0().left_0().w_1_2(),
                line.top_0().bottom_0().left_0().w(px(2.)),
            ),
            Edge::Right => (
                wash.top_0().bottom_0().right_0().w_1_2(),
                line.top_0().bottom_0().right_0().w(px(2.)),
            ),
            Edge::Top => (
                wash.left_0().right_0().top_0().h_1_2(),
                line.left_0().right_0().top_0().h(px(2.)),
            ),
            Edge::Bottom => (
                wash.left_0().right_0().bottom_0().h_1_2(),
                line.left_0().right_0().bottom_0().h(px(2.)),
            ),
        };
        let review_ratio = (drag.source == DragSource::Pane(id))
            .then(|| self.workspace.reviews.get(&id))
            .flatten()
            .filter(|state| !state.chat_hidden)
            .map(|state| state.ratio);
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .when_some(review_ratio, |d, ratio| {
                    d.left(gpui::relative(1. - ratio)).w(gpui::relative(ratio))
                })
                .child(wash)
                .child(line)
                .into_any_element(),
        )
    }
    /// One session's surface: its chat, with its review pane beside it.
    /// `split` is the split header's progress while this is a pane of a
    /// split tab; None for a tab's lone session.
    fn session_surface(
        &mut self,
        id: i64,
        split: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.mono_panes.contains_key(&id) {
            return self.mono_pane_surface(id, split, cx);
        }
        let in_split = split.is_some();
        let chat = self.sessions.get(&id).cloned();
        let now = std::time::Instant::now();
        let sliding = self
            .review_slide
            .as_ref()
            .filter(|(sid, _, _)| *sid == id)
            .map(|(_, slide, review)| {
                let (width, done) = slide.at(now);
                (width, done, slide.opening(), review.clone())
            });
        // A completed close must use the same surface as the next frame.
        // Keeping the removed review here would retain its collapsing header
        // and change the layout again on the following redraw.
        let sliding = match sliding {
            Some((_, true, _, _)) => {
                self.review_slide = None;
                None
            }
            sliding => sliding,
        };
        let review = sliding
            .as_ref()
            .map(|(_, _, _, review)| review.clone())
            .or_else(|| self.review_for(id, cx));
        if let Some(chat) = &chat {
            let beside = in_split || review.is_some();
            chat.update(cx, |session, cx| session.set_beside_pane(beside, cx));
        }
        let Some(review) = review else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .when_some(split.filter(|p| *p > 0.), |d, p| {
                    d.child(self.split_header(id, p, true, cx))
                })
                .child(self.chat_with_background(id, chat, cx))
                .into_any_element();
        };
        let state = review.read(cx).state.clone();
        let (ratio, deferred) = match sliding {
            Some((width, _, opening, _)) => {
                window.request_animation_frame();
                (width, opening)
            }
            None => (state.ratio, false),
        };
        // Mount the body once the pane has arrived; cold file reads and diff
        // construction cannot compete with its opening frames.
        review.update(cx, |r, cx| {
            r.set_visible(true, cx);
            r.set_deferred(deferred, cx);
        });
        let chat_hidden = state.chat_hidden;
        let header = if in_split {
            1.
        } else if state.ratio > 0. {
            (ratio / state.ratio).clamp(0., 1.)
        } else {
            1.
        };
        let header_shown = split.unwrap_or(header);
        let ratio = if chat_hidden { 1. } else { ratio.clamp(0., 1.) };
        let bounds = self.split_bounds.entry(id).or_default().clone();
        let full_width = bounds
            .get()
            .map_or(600., |b| b.size.width.as_f32() * state.ratio);
        let dragging = self.review_drag && self.workspace.active == Some(id);
        let moving = self
            .pane_drag
            .as_ref()
            .is_some_and(|d| d.active && d.source == DragSource::Review(id));
        let moving_chat = self
            .pane_drag
            .as_ref()
            .is_some_and(|d| d.active && d.source == DragSource::Pane(id));
        let sliding_now = self
            .review_slide
            .as_ref()
            .is_some_and(|(sid, _, _)| *sid == id);
        div()
            .size_full()
            .relative()
            .flex()
            .child(bounds.layout_probe())
            .when(!chat_hidden, |d| {
                d.child(
                    div()
                        .h_full()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .when(moving_chat, |d| d.opacity(0.4))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                if this.review_focused {
                                    this.review_focused = false;
                                    cx.notify();
                                }
                            }),
                        )
                        .child(self.split_header(id, header_shown, in_split, cx))
                        .child(self.chat_with_background(id, chat, cx)),
                )
            })
            .child(
                div()
                    .id(("review-frame", id as usize))
                    .debug_selector(|| "review-frame".into())
                    .relative()
                    .h_full()
                    .flex_shrink_0()
                    .w(gpui::relative(ratio))
                    .overflow_hidden()
                    .when(moving, |d| d.opacity(0.4))
                    .when(!chat_hidden, |d| {
                        d.border_l_1().border_color(crate::right_panel::stroke())
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if !this.review_focused {
                                this.review_focused = true;
                                cx.notify();
                            }
                        }),
                    )
                    // Ride the moving leading edge at a fixed layout width.
                    // Pinning to the outer right edge would only uncover the
                    // contents, losing the slide React gives the whole surface.
                    .map(|d| {
                        // The same progress drives both directions, so a
                        // reversal cannot flash from a faded exit to opaque.
                        let d = d.opacity(if moving { header * 0.4 } else { header });
                        let review = review.cached(StyleRefinement::default().size_full());
                        if sliding_now && !chat_hidden {
                            d.child(
                                div()
                                    .id("review-content")
                                    .debug_selector(|| "review-content".into())
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .h_full()
                                    .w(px(full_width))
                                    .child(review),
                            )
                        } else {
                            d.child(div().id("review-content").size_full().child(review))
                        }
                    }),
            )
            .when(!chat_hidden && !sliding_now, |d| {
                d.child(
                    div()
                        .id(("review-sash", id as usize))
                        .group("resize-handle")
                        .role(Role::Splitter)
                        .aria_label("Resize panes")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(gpui::relative(1. - ratio))
                        .ml(px(-5.5))
                        .w(px(12.))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .child(resize_border(true, dragging))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.focus_pane(id, window, cx);
                                this.review_drag = true;
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        ),
                )
            })
            .into_any_element()
    }
    fn drag_review(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.review_drag {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.review_drag = false;
            self.persist(cx);
            return;
        }
        let bounds = self
            .workspace
            .active
            .and_then(|id| self.split_bounds.get(&id))
            .and_then(AnchorSlot::get);
        let (Some(bounds), Some(review)) = (bounds, self.active_review(cx)) else {
            return;
        };
        let width = bounds.size.width.as_f32().max(1.);
        let ratio = (1. - (event.position.x - bounds.left()).as_f32() / width).clamp(0.15, 0.85);
        review.update(cx, |r, cx| {
            r.state.ratio = ratio;
            cx.notify();
        });
        cx.notify();
    }
    /// The narrowest chat column the active tab would have, with or without
    /// the sidebar: each pane's share of the card, less its review.
    fn narrowest_chat(&self, sidebar: bool, cx: &App) -> f32 {
        // The card's inset and border.
        let mut main = self.viewport_width - 10. - self.project_rail_drawn_width();
        if sidebar && self.session_sidebar_available(cx) {
            main -= self.workspace.sidebar_width;
        }
        if self.right_panel_shown(cx) {
            main -= self.right_panel_width();
        }
        if self.mono_active(cx) {
            return main;
        }
        if let Some(dock) = self.workspace.docks.get(&self.workspace.project)
            && dock.open
            && !dock.side.vertical()
        {
            main -= dock.size;
        }
        let Some(tab) = self
            .workspace
            .active_tab()
            .filter(|_| !self.app_view_open())
        else {
            return main;
        };
        self.workspace
            .layout(tab)
            .leaves()
            .iter()
            .map(|leaf| {
                let width = leaf.rect.w * main;
                let review = self
                    .reviews
                    .get(&leaf.id)
                    .map(|r| r.read(cx).state.clone())
                    .or_else(|| self.workspace.reviews.get(&leaf.id).cloned())
                    .filter(|r| !r.tabs.is_empty());
                match review {
                    Some(r) if r.chat_hidden => f32::INFINITY,
                    Some(r) => width * (1. - r.ratio),
                    None => width,
                }
            })
            .fold(f32::INFINITY, f32::min)
    }
    /// A panel or pane that would squeeze a chat below a readable width
    /// collapses the sidebar as it opens. A sidebar collapsed that way comes
    /// back once there is room again; one the user hid stays hidden.
    fn fit_sidebar(&mut self, cx: &mut Context<Self>) {
        if !self.session_sidebar_available(cx) {
            return;
        }
        if self.workspace.sidebar_visible {
            if self.narrowest_chat(true, cx) < MIN_CHAT_WIDTH {
                self.set_sidebar_visible(false, cx);
                self.sidebar_auto_hidden = true;
            }
        } else if self.sidebar_auto_hidden && self.narrowest_chat(true, cx) >= MIN_CHAT_WIDTH {
            self.set_sidebar_visible(true, cx);
            self.sidebar_auto_hidden = false;
        }
    }
    /// Refit the sidebar whenever the room around the chats changes, however
    /// it changed: a window resize, a tab or project switch, a review or pane
    /// closed from inside, a panel's slide settling. Room is measured without
    /// the sidebar so showing or hiding it by hand never counts as a change,
    /// and drags refit once they let go.
    fn refit_sidebar_to_room(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.resize.is_some()
            || self.right_resize.is_some()
            || self.review_drag
            || self.pane_sash.is_some()
            || self.dock_resize.is_some()
            || self.project_resize.is_some()
        {
            return;
        }
        let room = (
            self.narrowest_chat(false, cx).round() as i32,
            self.session_sidebar_available(cx),
        );
        if self.sidebar_fitted_room.replace(room) == Some(room) {
            return;
        }
        let this = cx.entity();
        window.defer(cx, move |_, cx| {
            this.update(cx, |this, cx| {
                let visible = this.workspace.sidebar_visible;
                this.fit_sidebar(cx);
                if visible != this.workspace.sidebar_visible {
                    this.persist(cx);
                    cx.notify();
                }
            });
        });
    }
    /// newTerminalCwd: the focused session's worktree, else its folder, else
    /// the project.
    fn terminal_cwd(&self, cx: &App) -> String {
        self.workspace
            .active
            .and_then(|id| self.sessions.get(&id))
            .map(|view| {
                let view = view.read(cx);
                view.settings
                    .worktree_cwd
                    .clone()
                    .unwrap_or_else(|| view.summary.cwd.clone())
            })
            .unwrap_or_else(|| self.working_copy().to_owned())
    }
    fn spawn_terminal(&mut self, cwd: String, cx: &mut Context<Self>) -> Entity<Terminal> {
        let terminal = cx.new(|cx| Terminal::new(cwd, cx));
        self.observations.push(cx.subscribe(
            &terminal,
            |_, terminal, event: &TerminalEvent, cx| {
                if matches!(event, TerminalEvent::ExitedCleanly) {
                    // `exit` closes its tab, as in other terminals.
                    cx.spawn(async move |this, cx| {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.close_exited_terminal(&terminal, window, cx)
                        });
                    })
                    .detach();
                }
                cx.notify()
            },
        ));
        terminal
    }
    fn close_exited_terminal(
        &mut self,
        terminal: &Entity<Terminal>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((project, ix)) = self.terminals.iter().find_map(|(project, live)| {
            live.iter()
                .position(|t| t == terminal)
                .map(|ix| (project.clone(), ix))
        }) else {
            return;
        };
        let last = self.terminals[&project].len() == 1;
        let panel = project == self.workspace.project
            && self.terminal_in_panel(cx)
            && self.panel_terminal_shown(cx);
        self.remove_terminal(&project, ix, window, cx);
        // The panel makes a fresh shell when it opens again.
        if panel && last {
            self.set_right_panel_open(false, window, cx);
        }
    }
    /// A restored dock reopens its tabs as fresh shells once it is shown.
    fn ensure_terminals(&mut self, project: &str, cx: &mut Context<Self>) {
        if project.starts_with("remote://") {
            return;
        }
        let Some(dock) = self.workspace.docks.get(project) else {
            return;
        };
        let wanted = dock.terminals.clone();
        let have = self.terminals.get(project).map_or(0, Vec::len);
        for cwd in wanted.into_iter().skip(have) {
            let terminal = self.spawn_terminal(cwd, cx);
            self.terminals
                .entry(project.to_owned())
                .or_default()
                .push(terminal);
        }
    }
    /// The focused terminal of the current project's dock.
    fn dock_focused(&self, window: &Window, cx: &App) -> Option<usize> {
        self.terminals
            .get(&self.workspace.project)?
            .iter()
            .position(|t| t.focus_handle(cx).contains_focused(window, cx))
    }
    fn focus_terminal(&self, window: &mut Window, cx: &mut Context<Self>) {
        let project = &self.workspace.project;
        if let Some(dock) = self.workspace.docks.get(project)
            && let Some(terminal) = self.terminals.get(project).and_then(|t| t.get(dock.active))
        {
            let focus = terminal.focus_handle(cx);
            window.focus(&focus, cx);
        }
    }
    fn slide_dock(&mut self, open: bool, cx: &mut Context<Self>) {
        let project = self.workspace.project.clone();
        let Some(size) = self.workspace.docks.get(&project).map(|d| d.size) else {
            return;
        };
        if cx.reduce_motion() {
            self.dock_slide = None;
            return;
        }
        let to = if open { 1. } else { 0. };
        self.dock_slide = Some((
            project.clone(),
            match self.dock_slide.take() {
                Some((p, slide)) if p == project => slide.retarget(to, std::time::Instant::now()),
                _ => PaneSlide::new(1. - to, to, size),
            },
        ));
    }
    /// Terminal: New (cmd-`): another terminal in the project's dock, which
    /// opens if it was hidden.
    fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.project.starts_with("remote://") {
            self.error = Some("Terminals are not available on remote projects.".into());
            cx.notify();
            return;
        }
        if self.app_view_open() {
            return;
        }
        let project = self.workspace.project.clone();
        let cwd = self.terminal_cwd(cx);
        let was_open = self
            .workspace
            .docks
            .get(&project)
            .is_some_and(|d| d.open && !d.terminals.is_empty());
        self.ensure_terminals(&project, cx);
        let viewport = self.viewport();
        let dock = self.workspace.docks.entry(project.clone()).or_default();
        dock.size = dock.side.clamp(dock.size, viewport);
        let added = dock.add(cwd.clone());
        self.dock_tab_scroll.scroll_to_item(added);
        let terminal = self.spawn_terminal(cwd, cx);
        self.terminals.entry(project).or_default().push(terminal);
        if self.terminal_in_panel(cx) {
            if let Some(dock) = self.workspace.docks.get_mut(&self.workspace.project) {
                dock.open = false;
            }
            self.show_panel_terminal(window, cx);
            self.persist(cx);
            return;
        }
        if !was_open {
            self.slide_dock(true, cx);
        }
        self.focus_terminal(window, cx);
        self.fit_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    /// Terminal: Toggle (cmd-j).
    fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.project.starts_with("remote://") {
            self.error = Some("Terminals are not available on remote projects.".into());
            cx.notify();
            return;
        }
        if self.terminal_in_panel(cx) {
            if self.panel_terminal_shown(cx) {
                self.set_right_panel_open(false, window, cx);
            } else {
                self.show_panel_terminal(window, cx);
            }
            return;
        }
        let project = self.workspace.project.clone();
        let Some(dock) = self
            .workspace
            .docks
            .get_mut(&project)
            .filter(|d| !d.terminals.is_empty())
        else {
            self.new_terminal(window, cx);
            return;
        };
        dock.open = !dock.open;
        let open = dock.open;
        self.slide_dock(open, cx);
        if open {
            self.ensure_terminals(&project, cx);
            self.focus_terminal(window, cx);
        } else if self.dock_focused(window, cx).is_some() {
            self.focus_active(window, cx);
        }
        self.fit_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    fn select_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dock) = self.workspace.docks.get_mut(&self.workspace.project) {
            dock.active = ix.min(dock.terminals.len().saturating_sub(1));
            self.dock_tab_scroll.scroll_to_item(dock.active);
        }
        self.focus_terminal(window, cx);
        self.persist(cx);
        cx.notify();
    }
    /// confirmCloseTerminal: a terminal running something asks first.
    fn close_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.workspace.project.clone();
        let Some(terminal) = self
            .terminals
            .get(&project)
            .and_then(|t| t.get(ix))
            .cloned()
        else {
            return;
        };
        let (process, label) = {
            let t = terminal.read(cx);
            (t.foreground.clone(), project_name(&t.cwd))
        };
        // The panel's last terminal stays: closing it hides the panel, and
        // stops whatever it was running.
        if self.terminal_in_panel(cx) && self.terminals.get(&project).is_some_and(|t| t.len() == 1)
        {
            let Some(process) = process else {
                self.set_right_panel_open(false, window, cx);
                return;
            };
            let answer = window.prompt(
                gpui::PromptLevel::Warning,
                &format!("\"{process}\" is still running in {label}. Stop it and close the panel?"),
                None,
                &["Stop", "Cancel"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if matches!(answer.await, Ok(0)) {
                    let _ = this.update_in(cx, |this, window, cx| {
                        terminal.update(cx, |t, cx| t.stop_foreground(cx));
                        this.set_right_panel_open(false, window, cx);
                    });
                }
            })
            .detach();
            return;
        }
        let Some(process) = process else {
            self.remove_terminal(&project, ix, window, cx);
            return;
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("\"{process}\" is still running in {label}. Close this terminal anyway?"),
            None,
            &["Close", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update_in(cx, |this, window, cx| {
                    if let Some(ix) = this
                        .terminals
                        .get(&project)
                        .and_then(|t| t.iter().position(|e| *e == terminal))
                    {
                        this.remove_terminal(&project, ix, window, cx);
                    }
                });
            }
        })
        .detach();
    }
    fn remove_terminal(
        &mut self,
        project: &str,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_focus = self.dock_focused(window, cx).is_some();
        if let Some(live) = self.terminals.get_mut(project)
            && ix < live.len()
        {
            live.remove(ix);
        }
        let Some(dock) = self.workspace.docks.get_mut(project) else {
            return;
        };
        if !dock.close(ix) {
            // The last terminal closes the dock; it slides shut empty.
            dock.open = false;
            self.slide_dock(false, cx);
            if had_focus {
                self.focus_active(window, cx);
            }
            self.fit_sidebar(cx);
        } else if had_focus {
            self.focus_terminal(window, cx);
        }
        self.persist(cx);
        cx.notify();
    }
    fn set_dock_side(&mut self, side: DockSide, cx: &mut Context<Self>) {
        let viewport = self.viewport();
        if let Some(dock) = self.workspace.docks.get_mut(&self.workspace.project) {
            dock.set_side(side);
            dock.size = side.clamp(dock.size, viewport);
        }
        self.dock_slide = None;
        self.fit_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    fn viewport(&self) -> (f32, f32) {
        (self.viewport_width, self.viewport_height)
    }
    fn drag_dock(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start, size)) = self.dock_resize else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.release_resizes(cx);
            return;
        }
        let Some(dock) = self.workspace.docks.get(&self.workspace.project) else {
            return;
        };
        let at = if dock.side.vertical() {
            event.position.y.as_f32()
        } else {
            event.position.x.as_f32()
        };
        let delta = at - start;
        let signed = if matches!(dock.side, DockSide::Bottom | DockSide::Right) {
            -delta
        } else {
            delta
        };
        let target = ResizeTarget::Dock(self.workspace.project.clone());
        self.drag_resize(target, size + signed, cx);
    }
    /// App.tsx's dock grid: the project's terminal dock on its side of the
    /// workspace. Opening or closing slides the track while the terminal
    /// keeps its full size and rides the moving edge, so it fits once.
    fn with_dock(
        &mut self,
        body: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let project = self.workspace.project.clone();
        // Other projects' docks stop polling for their foreground commands.
        // A hidden dock keeps polling for the footer's running-terminal chip.
        for (path, live) in &self.terminals {
            let shown = *path == project && !self.app_view_open();
            for terminal in live {
                terminal.update(cx, |t, _| t.visible = shown);
            }
        }
        // The right panel's Terminal tab hosts the shells instead.
        if self.terminal_in_panel(cx) {
            return body;
        }
        let Some(dock) = self.workspace.docks.get(&project).cloned() else {
            return body;
        };
        let progress = match self.dock_slide.as_ref().filter(|(p, _)| *p == project) {
            Some((_, slide)) => {
                let (t, done) = slide.at(std::time::Instant::now());
                if done {
                    self.dock_slide = None;
                    None
                } else {
                    window.request_animation_frame();
                    Some(t)
                }
            }
            None => None,
        };
        let shown = (dock.open || progress.is_some()) && !self.app_view_open();
        if !shown {
            return body;
        }
        self.ensure_terminals(&project, cx);
        let size = dock.size;
        let track = size * progress.unwrap_or(1.);
        let vertical = dock.side.vertical();
        let leading = matches!(dock.side, DockSide::Top | DockSide::Left);
        let surface = div()
            .absolute()
            .flex()
            .flex_col()
            .map(|d| {
                if vertical {
                    d.left_0().w_full().h(px(size))
                } else {
                    d.top_0().h_full().w(px(size))
                }
            })
            .map(|d| match dock.side {
                DockSide::Top => d.bottom_0(),
                DockSide::Left => d.right_0(),
                DockSide::Bottom => d.top_0(),
                DockSide::Right => d.left_0(),
            })
            .child(self.dock_header(&dock, false, window, cx))
            .child(
                div().flex_1().min_h_0().min_w_0().children(
                    self.terminals
                        .get(&project)
                        .and_then(|t| t.get(dock.active))
                        // The grid shapes every line; redraw it only when the
                        // terminal itself changes.
                        .map(|t| t.clone().cached(StyleRefinement::default().size_full())),
                ),
            );
        let sash = div()
            .id("terminal-dock-sash")
            .debug_selector(|| "terminal-dock-sash".into())
            .group("resize-handle")
            .role(Role::Splitter)
            .aria_label("Resize terminal")
            .absolute()
            .child(resize_border(!vertical, self.dock_resize.is_some()))
            .map(|d| match dock.side {
                DockSide::Bottom => d.bottom(px(track - 3.5)).left_0().right_0().h(px(6.)),
                DockSide::Top => d.top(px(track - 3.5)).left_0().right_0().h(px(6.)),
                DockSide::Left => d.left(px(track - 3.5)).top_0().bottom_0().w(px(6.)),
                DockSide::Right => d.right(px(track - 3.5)).top_0().bottom_0().w(px(6.)),
            })
            .cursor(if vertical {
                CursorStyle::ResizeUpDown
            } else {
                CursorStyle::ResizeLeftRight
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    // A double click restores the side's default size.
                    if event.click_count == 2 {
                        if let Some(dock) = this.workspace.docks.get_mut(&this.workspace.project) {
                            dock.size = dock.side.default_size();
                        }
                        this.persist(cx);
                        cx.notify();
                        return;
                    }
                    let at = if vertical {
                        event.position.y.as_f32()
                    } else {
                        event.position.x.as_f32()
                    };
                    this.dock_resize = Some((at, size));
                }),
            );
        let frame = div()
            .id("terminal-dock")
            .debug_selector(|| "terminal-dock".into())
            .role(Role::Region)
            .aria_label("Terminal")
            .relative()
            .flex_shrink_0()
            .overflow_hidden()
            .border_color(crate::right_panel::stroke())
            .map(|d| match dock.side {
                DockSide::Bottom => d.border_t_1(),
                DockSide::Top => d.border_b_1(),
                DockSide::Left => d.border_r_1(),
                DockSide::Right => d.border_l_1(),
            })
            .map(|d| {
                if vertical {
                    d.w_full().h(px(track))
                } else {
                    d.h_full().w(px(track))
                }
            })
            .child(surface);
        let main = div().flex_1().min_w_0().min_h_0().child(body);
        div()
            .size_full()
            .relative()
            .flex()
            .when(vertical, |d| d.flex_col())
            .map(|d| {
                if leading {
                    d.child(frame).child(main)
                } else {
                    d.child(main).child(frame)
                }
            })
            // Paint the full hit area above both surfaces, outside the dock's clip.
            .when(progress.is_none(), |d| d.child(sash))
            .into_any_element()
    }
    /// A click anywhere in a pane focuses it (PaneTree's onFocus).
    fn focus_pane(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.review_owners.contains_key(&id) {
            self.open_file_pane(id, window, cx);
            return;
        }
        if self.workspace.mono_panes.contains_key(&id) {
            if self.workspace.active != Some(id) {
                self.open_mono_pane(id, window, cx);
            }
            return;
        }
        if self.workspace.active == Some(id) {
            return;
        }
        self.navigation_revision += 1;
        self.history.record(id);
        self.workspace.open(id);
        if let Some(view) = self.sessions.get(&id) {
            self.workspace.project = view.read(cx).summary.cwd.clone();
            self.workspace
                .project_focus
                .insert(self.workspace.project.clone(), id);
            view.update(cx, |v, cx| v.load(cx));
            self.record_worktree_session(id, cx);
        }
        if !self.review_focused {
            self.focus_active(window, cx);
        }
        self.persist(cx);
        cx.notify();
    }
    /// Close one pane of a split tab; the tab's only pane closes the tab.
    fn close_pane(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.workspace.active;
        let before = self.active_leaves();
        if !self.workspace.close_pane(id) {
            self.close(id, window, cx);
            return;
        }
        self.history
            .prune(&self.workspace.open_sessions(), self.workspace.active);
        self.navigation_revision += 1;
        self.begin_pane_motion(before, cx);
        self.fit_sidebar(cx);
        // A Mono left alone becomes its tab again, and shows when focused.
        self.settle_mono_panes(window, cx);
        if self.mono_active(cx) {
            return;
        }
        if self.workspace.active != active {
            if let Some(active) = self.workspace.active {
                self.open(active, window, cx);
            }
        } else {
            self.persist(cx);
            cx.notify();
        }
    }
    /// cmd-d / shift-cmd-d: a new session beside the focused pane, with the
    /// focused session's provider.
    fn split_session(&mut self, dir: SplitDir, cx: &mut Context<Self>) {
        if self.app_view_open() || self.workspace.active.is_none() {
            return;
        }
        let kind = self
            .workspace
            .active
            .and_then(|id| self.sessions.get(&id))
            .map_or(Kind::Codex, |s| Kind::from_id(&s.read(cx).summary.provider));
        let navigation_revision = self.navigation_revision;
        let target = self.workspace.active;
        let store = self.store.clone();
        let cwd = self.workspace.project.clone();
        let worktree = target.and_then(|id| {
            let path = self.session_working_copy(id, cx);
            (monocode_core::files::path_key(path) != monocode_core::files::path_key(&cwd)).then(
                || crate::worktree_switcher::WorktreeFocus {
                    path: path.into(),
                    branch: self.worktree_focus.get(&cwd).and_then(|f| f.branch.clone()),
                },
            )
        });
        let worktree_cwd = worktree.as_ref().map(|f| f.path.clone());
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            let summary = store.create(kind.id(), &cwd)?;
            if worktree_cwd.is_some() {
                store.save_agent_settings(
                    summary.id,
                    &monocode_core::agent::Settings {
                        worktree_cwd,
                        ..Default::default()
                    },
                )?;
            }
            Ok::<_, String>(summary)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(summary) => {
                    let id = summary.id;
                    this.add(summary, cx);
                    this.seed_worktree(id, worktree.as_ref(), cx);
                    this.order.retain(|s| *s != id);
                    this.order.insert(0, id);
                    if this.navigation_revision != navigation_revision
                        || this.workspace.active != target
                    {
                        this.invalidate_chrome(cx);
                        cx.notify();
                        return;
                    }
                    let before = this.active_leaves();
                    this.workspace.split(id, dir);
                    this.begin_pane_motion(before, cx);
                    this.fit_sidebar(cx);
                    this.activate_session(id, window, cx);
                }
                Err(error) => {
                    this.error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn focus_neighbor(&mut self, dir: Edge, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.workspace.neighbor(dir) {
            self.review_focused = false;
            self.open(id, window, cx);
        }
    }
    fn active_leaves(&self) -> Vec<LayoutLeaf> {
        self.workspace
            .active_tab()
            .map(|tab| self.workspace.layout(tab).leaves())
            .unwrap_or_default()
    }
    /// Animate the active tab from `before` to its current tree. New panes
    /// enter from the outer edge they sit on, or open from their middle.
    fn begin_pane_motion(&mut self, before: Vec<LayoutLeaf>, cx: &mut Context<Self>) {
        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        let after = self.workspace.layout(tab).leaves();
        if cx.reduce_motion() || (after.len() < 2 && before.len() < 2) || before.is_empty() {
            self.pane_motion = None;
            return;
        }
        let (width, height) = self.pane_bounds.get().map_or((800., 600.), |b| {
            (b.size.width.as_f32(), b.size.height.as_f32())
        });
        let mut from = HashMap::new();
        let mut entering = HashMap::new();
        let mut travel: f32 = 0.;
        for leaf in &after {
            let start = match before.iter().find(|b| b.id == leaf.id) {
                Some(old) => old.rect,
                None => {
                    let (edge, start) = enter_from(leaf);
                    entering.insert(leaf.id, edge);
                    start
                }
            };
            travel = travel.max(
                ((start.w - leaf.rect.w).abs() * width)
                    .max((start.h - leaf.rect.h).abs() * height)
                    .max((start.x - leaf.rect.x).abs() * width)
                    .max((start.y - leaf.rect.y).abs() * height),
            );
            from.insert(leaf.id, start);
        }
        // A closed pane slides back out the way a new one comes in. One that
        // moved to another tab is shown there instead.
        let open = self.workspace.open_sessions();
        let leaving: Vec<_> = before
            .iter()
            .filter(|old| {
                !after.iter().any(|leaf| leaf.id == old.id)
                    && !open.contains(&old.id)
                    && self.sessions.contains_key(&old.id)
            })
            .map(|old| {
                let (edge, to) = enter_from(old);
                travel = travel.max((old.rect.w * width).max(old.rect.h * height) * 0.5);
                (*old, edge, to)
            })
            .collect();
        if travel < 1. {
            self.pane_motion = None;
            return;
        }
        let header = match (before.len() > 1, after.len() > 1) {
            (false, true) => Some(true),
            (true, false) => Some(false),
            _ => None,
        };
        self.pane_motion = Some(PaneMotion {
            tab,
            from,
            entering,
            leaving,
            header,
            slide: PaneSlide::new(0., 1., travel),
        });
    }
    /// The pane under a point and the edge it would take, in the active tab.
    fn pane_hit(&self, position: Point<Pixels>, cx: &App) -> Option<(i64, Edge)> {
        if self.app_view_open() {
            return None;
        }
        // The visible Mono hides the tab's panes and stands in for them.
        if self.mono_active(cx) {
            return self.mono_view_hit(position, cx);
        }
        let bounds = self.pane_bounds.get()?;
        if !bounds.contains(&position) {
            return None;
        }
        let tab = self.workspace.active_tab()?;
        let (w, h) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
        let (x, y) = (
            (position.x - bounds.left()).as_f32(),
            (position.y - bounds.top()).as_f32(),
        );
        self.workspace
            .layout(tab)
            .leaves()
            .into_iter()
            .find_map(|leaf| {
                let r = Rect {
                    x: leaf.rect.x * w,
                    y: leaf.rect.y * h,
                    w: leaf.rect.w * w,
                    h: leaf.rect.h * h,
                };
                (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
                    .then(|| (leaf.id, crate::pane_layout::edge_from_point(x, y, r)))
            })
    }
    /// titleTabDropFromPoint: the strip slot nearest the pointer.
    fn tab_insert_at(&self, position: Point<Pixels>, cx: &App) -> Option<usize> {
        let strip = self.tab_scroll.bounds();
        if !strip.contains(&position) {
            return None;
        }
        let count = self.visible_tabs(cx).len();
        let bounds: Vec<_> = (0..count)
            .filter_map(|ix| self.tab_scroll.bounds_for_item(ix))
            .collect();
        if bounds.len() != count {
            return Some(count);
        }
        Some(
            bounds
                .iter()
                .position(|b| position.x < b.center().x)
                .unwrap_or(count),
        )
    }
    fn move_pane_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.pane_drag.as_ref() else {
            return;
        };
        if matches!(drag.source, DragSource::Tab(_)) {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.cancel_pane_drag(cx);
            return;
        }
        let source = drag.source;
        let start = drag.start;
        let delta = event.position - start;
        if !drag.active && delta.x.as_f32().hypot(delta.y.as_f32()) < 5. {
            return;
        }
        let insert = match source {
            DragSource::Pane(_) | DragSource::Review(_) => self.tab_insert_at(event.position, cx),
            _ => None,
        };
        let over = if insert.is_some() {
            None
        } else {
            self.pane_hit(event.position, cx)
                .and_then(|(id, edge)| {
                    if id != source.session()
                        || matches!(source, DragSource::Review(id) if self.workspace.reviews.get(&id).is_some_and(|r| !r.chat_hidden))
                    {
                        Some((id, edge))
                    } else if matches!(source, DragSource::Pane(_)) {
                        self.session_review_hit(id, event.position).map(|edge| (id, edge))
                    } else {
                        None
                    }
                })
        };
        let drag = self.pane_drag.as_mut().unwrap();
        if !drag.active && matches!(source, DragSource::Session(_)) {
            self.skip_session_click = true;
        }
        drag.active = true;
        drag.position = event.position;
        drag.over = over;
        if drag.insert != insert {
            drag.insert = insert;
            self.tabs_frame.update(cx, |_, cx| cx.notify());
        }
        let drop = matches!(source, DragSource::Session(_))
            .then(|| self.sidebar_drop_at(event.position, source.session(), cx))
            .flatten();
        if self.sidebar_drop != drop {
            self.sidebar_drop = drop;
            self.notify_sidebar(cx);
        }
        cx.notify();
    }
    fn cancel_pane_drag(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_drop.take().is_some() {
            self.notify_sidebar(cx);
        }
        if let Some(drag) = self.pane_drag.take() {
            if drag.insert.is_some() {
                self.tabs_frame.update(cx, |_, cx| cx.notify());
            }
            cx.notify();
        }
    }
    fn finish_pane_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.pane_drag.take() else {
            return;
        };
        self.tabs_frame.update(cx, |_, cx| cx.notify());
        cx.notify();
        if !drag.active {
            return;
        }
        if let Some(target) = self.sidebar_drop.take() {
            self.apply_sidebar_drop(drag.source.session(), target, window, cx);
            return;
        }
        let mono = match drag.source {
            DragSource::Pane(pane) => self.workspace.mono_panes.get(&pane).cloned(),
            _ => None,
        };
        let mut over = drag
            .over
            .map(|(target, edge)| (self.resolve_mono_target(target, cx), edge));
        let mut before = self.active_leaves();
        let id = drag.source.session();
        if matches!(drag.source, DragSource::Pane(_))
            && (over.is_some() || drag.insert.is_some())
            && let Some(review) = self.separate_session_review(id, cx)
        {
            // The file keeps the old pane's place while the chat moves.
            for leaf in &mut before {
                if leaf.id == id {
                    leaf.id = review;
                }
            }
            if let Some((target, _)) = &mut over
                && *target == id
            {
                *target = review;
            }
        }
        match (drag.source, over, drag.insert) {
            (DragSource::Review(_), over, insert) if over.is_some() || insert.is_some() => {
                let Some(id) = self.independent_file_pane(id, cx) else {
                    return;
                };
                if let Some(to) = insert {
                    self.workspace.detach(id, self.workspace.tabs.len());
                    self.reorder_title_tab(&TitleTab::Session(id), to, cx);
                    self.pane_motion = None;
                } else if let Some((target, edge)) = over {
                    self.workspace.place(id, target, edge);
                    self.begin_pane_motion(before, cx);
                }
            }
            (DragSource::Pane(_), _, Some(to)) => {
                let tabs = self.visible_tabs(cx);
                let session_index = tabs
                    .get(to)
                    .and_then(TitleTab::session)
                    .and_then(|next| self.workspace.tabs.iter().position(|id| *id == next))
                    .unwrap_or(self.workspace.tabs.len());
                self.workspace.detach(id, session_index);
                self.reorder_title_tab(&TitleTab::Session(id), to, cx);
                self.pane_motion = None;
            }
            (DragSource::Tab(tab), Some((target, edge)), _) => {
                self.workspace.place_tab(tab, target, edge);
                self.begin_pane_motion(before, cx);
            }
            (DragSource::Pane(_) | DragSource::Session(_), Some((target, edge)), _) => {
                self.workspace.place(id, target, edge);
                self.begin_pane_motion(before, cx);
            }
            _ => return,
        }
        self.history
            .prune(&self.workspace.open_sessions(), self.workspace.active);
        self.fit_sidebar(cx);
        // A Mono pulled out of its split onto the strip is its tab again.
        self.settle_mono_panes(window, cx);
        if let (Some(mono), Some(to)) = (mono, drag.insert)
            && self.workspace.title_order.contains(&mono)
        {
            self.reorder_title_tab(&mono, to, cx);
        }
        if self.mono_active(cx) {
            self.persist(cx);
            return;
        }
        if let Some(active) = self.workspace.active {
            self.open(active, window, cx);
        }
    }
    fn drag_sash(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(sash) = self.pane_sash.as_ref() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.release_resizes(cx);
            return;
        }
        let Some(target) = self.sash_target() else {
            return;
        };
        let Some(bounds) = self.pane_bounds.get() else {
            return;
        };
        let g = sash.group;
        let (at, origin) = if sash.dir == SplitDir::Right {
            let origin = bounds.left().as_f32() + g.x * bounds.size.width.as_f32();
            (event.position.x.as_f32(), origin)
        } else {
            let origin = bounds.top().as_f32() + g.y * bounds.size.height.as_f32();
            (event.position.y.as_f32(), origin)
        };
        let ResizeTarget::Sash { span, .. } = target else {
            return;
        };
        self.drag_resize(target, (at - origin) / span.max(1.), cx);
    }
    fn sash_target(&self) -> Option<ResizeTarget> {
        let sash = self.pane_sash.as_ref()?;
        let bounds = self.pane_bounds.get()?;
        let span = if sash.dir == SplitDir::Right {
            sash.group.w * bounds.size.width.as_f32()
        } else {
            sash.group.h * bounds.size.height.as_f32()
        };
        Some(ResizeTarget::Sash {
            tab: sash.tab,
            path: sash.path.clone(),
            index: sash.index,
            span,
        })
    }
    /// The target's current size, the range it rests in, and pixels per unit.
    fn resize_extent(&self, target: &ResizeTarget) -> Option<(f32, f32, f32, f32)> {
        Some(match target {
            ResizeTarget::Projects => (self.workspace.project_rail_width, 176., 360., 1.),
            ResizeTarget::Sidebar => {
                let max = (self.viewport_width * 0.5).clamp(200., 560.);
                (self.workspace.sidebar_width, 200., max, 1.)
            }
            ResizeTarget::RightPanel => {
                let (min, max) = (self.right_panel_min, crate::right_panel::WIDTH_MAX);
                let max = max.min((self.viewport_width * 0.5).floor()).max(min);
                (self.right_panel_width(), min, max, 1.)
            }
            ResizeTarget::Dock(project) => {
                let dock = self.workspace.docks.get(project)?;
                let (min, max) = dock.side.limits(self.viewport());
                (dock.size, min, max, 1.)
            }
            ResizeTarget::Sash {
                tab,
                path,
                index,
                span,
            } => {
                let (at, min, max) = self.workspace.sash_range(*tab, path, *index)?;
                (at, min, max, span.max(1.))
            }
        })
    }
    fn set_resize(&mut self, target: &ResizeTarget, value: f32) {
        match target {
            ResizeTarget::Projects => self.workspace.project_rail_width = value,
            ResizeTarget::Sidebar => self.workspace.sidebar_width = value,
            ResizeTarget::RightPanel => self.workspace.right_panel_width = value,
            ResizeTarget::Dock(project) => {
                if let Some(dock) = self.workspace.docks.get_mut(project) {
                    dock.size = value;
                }
            }
            ResizeTarget::Sash {
                tab, path, index, ..
            } => self.workspace.place_sash(*tab, path, *index, value),
        }
    }
    /// Follows the pointer to `value`, stopping at the target's limits.
    fn drag_resize(&mut self, target: ResizeTarget, value: f32, cx: &mut Context<Self>) {
        let Some((_, min, max, _)) = self.resize_extent(&target) else {
            return;
        };
        let clamped = value.clamp(min, max);
        let to = if matches!(target, ResizeTarget::Sash { .. }) {
            clamped
        } else {
            clamped.round()
        };
        self.set_resize(&target, to);
        cx.notify();
    }
    /// Ends every resize in progress.
    fn release_resizes(&mut self, cx: &mut Context<Self>) -> bool {
        let any = [
            self.pane_sash.take().is_some(),
            self.resize.take().is_some(),
            self.project_resize.take().is_some(),
            self.right_resize.take().is_some(),
            self.dock_resize.take().is_some(),
        ]
        .contains(&true);
        if any {
            self.persist(cx);
            cx.notify();
        }
        any
    }
    fn resume_git(&mut self, cx: &mut Context<Self>) {
        self.git.update(cx, |git, cx| git.resume(cx));
        if let Some(panel) = &self.right_panel {
            let panel = panel.read(cx);
            let (explorer, changes, history) = (
                panel.explorer.clone(),
                panel.changes.clone(),
                panel.history.clone(),
            );
            explorer.update(cx, |e, cx| e.resume(cx));
            changes.update(cx, |c, cx| c.resume(cx));
            history.update(cx, |h, cx| h.resume(cx));
        }
    }
    fn persist(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.workspace.active {
            self.refresh_session_mark(id, cx);
        }
        self.workspace.worktree_focus = self.worktree_focus.clone();
        self.workspace.normalize_title_order();
        self.sync_git(cx);
        self.update_chat_visibility(cx);
        self.update_usage_provider(cx);
        self.sync_right_panel(cx);
        self.invalidate_chrome(cx);
        self.save_state(cx);
    }
    fn save_window_state(&mut self, window: &Window, cx: &mut Context<Self>) {
        let bounds = window.inner_window_bounds();
        // X11 can report Windowed bounds while tracking fullscreen separately.
        let bounds = if window.is_fullscreen() {
            gpui::WindowBounds::Fullscreen(bounds.get_bounds())
        } else {
            bounds
        };
        if self.workspace.window.update(bounds) {
            self.save_state(cx);
        }
    }
    fn save_state(&mut self, cx: &mut Context<Self>) {
        let state = serde_json::to_string(&self.workspace).unwrap();
        *self.latest_state.lock().unwrap_or_else(|e| e.into_inner()) = state.clone();
        let revision = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            if generation.load(Ordering::SeqCst) == revision {
                store.save_ui_state(&state)?;
            }
            Ok::<_, String>(())
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = work.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(format!("Could not save workspace: {error}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn update_usage_provider(&self, cx: &mut Context<Self>) {
        let view = self
            .monos
            .as_ref()
            .filter(|_| self.mono_active(cx))
            .and_then(|m| m.read(cx).chat())
            .or_else(|| {
                self.workspace.active.and_then(|id| {
                    self.sessions
                        .get(&self.workspace.session_for_pane(id))
                        .cloned()
                })
            });
        if let Some(view) = view {
            let view = view.read(cx);
            let kind = Kind::from_id(&view.summary.provider);
            let profile = view.settings.account.clone();
            let model = view.account_model().map(str::to_owned);
            let project = view.summary.cwd.clone();
            let session = Some(view.summary.id);
            self.usage_footer.update(cx, |footer, cx| {
                footer.context(kind, profile, model.as_deref(), project, session, cx)
            });
        }
    }
    pub(crate) fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keep_visible_idle_agents(cx);
        if let Some(search) = &self.app_search {
            window.focus(&search.view.focus_handle(cx), cx);
            return;
        }
        if self.focus_app_view(window, cx) {
            return;
        }
        if let Some(monos) = self.monos.clone().filter(|m| m.read(cx).active.is_some()) {
            monos.update(cx, |m, cx| m.focus_active(window, cx));
            return;
        }
        if self.focus_mono_pane(window, cx) {
            return;
        }
        if let Some(review) = self
            .active_review(cx)
            .filter(|review| review.read(cx).state.chat_hidden)
        {
            review.update(cx, |r, cx| r.focus_active(window, cx));
            return;
        }
        if let Some(view) = self.workspace.active.and_then(|id| self.sessions.get(&id)) {
            let focus = view.read(cx).input.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus, cx);
        }
    }
    pub(crate) fn open(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.review_owners.contains_key(&id) {
            self.open_file_pane(id, window, cx);
            return;
        }
        if self.workspace.mono_panes.contains_key(&id) {
            self.open_mono_pane(id, window, cx);
            return;
        }
        if !self.session_summaries.contains_key(&id) || self.deleting.contains(&id) {
            return;
        }
        self.navigation_revision += 1;
        self.replace_blank_pane(id, cx);
        self.activate_session(id, window, cx);
    }
    /// Opening a conversation that has no tab takes over a blank pane in the
    /// active tab instead of adding a tab, as in the desktop app.
    fn replace_blank_pane(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.workspace.tab_of(id).is_some()
            || !self.session_metadata(id, cx).summary.has_messages
        {
            return;
        }
        let Some(tab) = self.workspace.active_tab() else {
            return;
        };
        let cwd = &self.session_metadata(id, cx).summary.cwd;
        let working_copy = monocode_core::files::path_key(self.session_working_copy(id, cx));
        let blank = |pane: i64| {
            if self.workspace.review_owners.contains_key(&pane)
                || !self.session_summaries.contains_key(&pane)
            {
                return false;
            }
            let s = self.session_metadata(pane, cx);
            &s.summary.cwd == cwd
                && monocode_core::files::path_key(self.session_working_copy(pane, cx))
                    == working_copy
                && !s.summary.has_messages
                && !s.busy
                && !s.has_saved_draft()
                && self
                    .sessions
                    .get(&pane)
                    .is_none_or(|v| v.read(cx).input.read(cx).content.is_empty())
        };
        let focused = self.workspace.tab_focus(tab);
        let Some(pane) = std::iter::once(focused)
            .chain(self.workspace.layout(tab).leaf_ids())
            .find(|pane| blank(*pane))
        else {
            return;
        };
        self.workspace.swap_session(pane, id);
        if let Some(next) = self.workspace.tab_of(id) {
            self.replace_title_tab_key(tab, next);
        }
    }
    fn activate_session(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.review_owners.contains_key(&id) {
            self.open_file_pane(id, window, cx);
            return;
        }
        if self.workspace.mono_panes.contains_key(&id) {
            self.open_mono_pane(id, window, cx);
            return;
        }
        if let Some(monos) = &self.monos {
            monos.update(cx, |m, cx| m.close(cx));
        }
        self.leave_app_views(cx);
        self.leave_app_search(cx);
        self.close_preview(cx);
        self.dismiss_due_reminder(id, cx);
        let Some(view) = self.ensure_session(id, cx) else {
            return;
        };
        self.history.record(id);
        self.workspace.open(id);
        self.workspace.project = view.read(cx).summary.cwd.clone();
        self.workspace
            .project_focus
            .insert(self.workspace.project.clone(), id);
        self.workspace.collapsed.remove(&self.workspace.project);
        view.update(cx, |v, cx| v.load(cx));
        self.record_worktree_session(id, cx);
        self.remember_title_tab(TitleTab::Session(self.workspace.tab_of(id).unwrap()));
        self.reveal_active_tab(cx);
        self.review_focused = self
            .workspace
            .reviews
            .get(&id)
            .is_some_and(|review| review.chat_hidden);
        self.focus_active(window, cx);
        self.persist(cx);
        cx.notify();
    }
    pub(crate) fn close(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tab_can_close(id, cx) {
            return;
        }
        self.close_menu(cx);
        self.close_tabs(&[id], None, window, cx);
    }
    /// Apply a close operation to one snapshot. Intermediate neighbours must
    /// never change the project used for the final selection or blank tab.
    fn close_tabs(
        &mut self,
        ids: &[i64],
        preferred: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tabs = ids
            .iter()
            .filter_map(|id| self.workspace.tab_of(*id))
            .map(TitleTab::Session)
            .collect::<Vec<_>>();
        let preferred = preferred
            .and_then(|id| self.workspace.tab_of(id))
            .map(TitleTab::Session);
        self.close_title_tabs(&tabs, preferred, window, cx);
    }
    fn visit(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        // Settings sit over the history: Back leaves them, and Forward
        // leaves them on its way to the next visit.
        if self.settings.is_some() {
            self.close_settings(window, cx);
            if step < 0 {
                return;
            }
        }
        if let Some(id) = self.history.step(step) {
            self.open(id, window, cx);
        }
    }
    fn cycle(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = self.visible_tabs(cx);
        if tabs.is_empty() {
            return;
        }
        let active = self.active_title_tab(cx);
        let current = tabs.iter().position(|tab| Some(tab) == active.as_ref());
        let next = current.map_or(if step < 0 { tabs.len() - 1 } else { 0 }, |index| {
            (index as isize + step).rem_euclid(tabs.len() as isize) as usize
        });
        self.open_title_tab(&tabs[next], window, cx);
    }
    fn focus_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(settings) = &self.settings {
            window.focus(&settings.focus_handle(cx), cx);
            return;
        }
        window.focus(&self.focus, cx);
        if self.focus_app_view(window, cx) {
            return;
        }
        if self.review_focused
            && let Some(review) = self.active_review(cx)
        {
            review.update(cx, |r, cx| r.focus_active(window, cx));
        } else {
            self.focus_active(window, cx);
        }
    }
    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview(cx);
        if self.workspace.sidebar_visible && self.sidebar_focus.contains_focused(window, cx) {
            self.focus_workspace(window, cx);
        }
        self.sidebar_auto_hidden = false;
        self.set_sidebar_visible(!self.workspace.sidebar_visible, cx);
        self.persist(cx);
        cx.notify();
    }
    fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if visible == self.workspace.sidebar_visible {
            return;
        }
        let to = if visible { 1. } else { 0. };
        self.sidebar_slide = if cx.reduce_motion() {
            None
        } else {
            Some(match self.sidebar_slide.take() {
                Some(slide) => slide.retarget(to, std::time::Instant::now()),
                None => PaneSlide::new(1. - to, to, self.workspace.sidebar_width),
            })
        };
        self.resize = None;
        self.drop_peek();
        self.workspace.sidebar_visible = visible;
        self.sync_right_panel(cx);
    }
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_app_search(window, cx);
    }

    pub(super) fn select_project(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_sessions.clear();
        self.selection_anchor = None;
        self.sidebar_target = None;
        self.navigation_revision += 1;
        self.leave_app_views(cx);
        self.leave_app_search(cx);
        self.close_preview(cx);
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |m, cx| m.close(cx));
        }
        self.workspace.project = path.clone();
        self.workspace.active = None;
        if let Some(ix) = self.workspace.projects.iter().position(|p| p == &path) {
            if ix < self.spaces_start {
                self.spaces_start = ix;
            } else if ix >= self.spaces_start + 5 {
                self.spaces_start = ix - 4;
            }
        }
        self.workspace.collapsed.remove(&path);
        // Original-layout project buttons leave the standalone Mono/app view.
        // The new layout restores each project's selected tab, including Monos.
        let remembered_session = self
            .workspace
            .scope_focus
            .get(&self.tab_scope())
            .is_some_and(|tab| matches!(tab, TitleTab::Session(_)));
        if (!self.workspace.classic_layout || remembered_session)
            && self.restore_scope_tab(window, cx)
        {
            return;
        }
        self.workspace.active = self
            .workspace
            .project_focus
            .get(&path)
            .copied()
            .filter(|id| {
                self.workspace.tab_of(*id).is_some()
                    && self
                        .session_summaries
                        .contains_key(&self.workspace.session_for_pane(*id))
                    && self
                        .session_metadata(self.workspace.session_for_pane(*id), cx)
                        .summary
                        .cwd
                        == path
                    && self.in_worktree_scope(*id, cx)
            })
            .or_else(|| {
                self.workspace
                    .tabs
                    .iter()
                    .rev()
                    .find(|id| {
                        self.session_summaries.contains_key(
                            &self
                                .workspace
                                .session_for_pane(self.workspace.scope_pane(**id)),
                        ) && self
                            .session_metadata(
                                self.workspace
                                    .session_for_pane(self.workspace.scope_pane(**id)),
                                cx,
                            )
                            .summary
                            .cwd
                            == path
                            && self.in_worktree_scope(self.workspace.scope_pane(**id), cx)
                    })
                    .map(|tab| self.workspace.tab_focus(*tab))
            });
        if let Some(id) = self.workspace.active {
            self.activate_session(id, window, cx);
        } else {
            self.ensure_project_session(window, cx);
        }
        self.focus_active(window, cx);
        self.persist(cx);
        cx.notify();
    }
    fn ensure_project_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.active.is_some() || self.mono_active(cx) {
            return;
        }
        if let Some(id) = self.order.iter().copied().find(|id| {
            let s = self.session_metadata(*id, cx);
            s.summary.cwd == self.workspace.project
                && monocode_core::files::path_key(self.session_working_copy(*id, cx))
                    == monocode_core::files::path_key(self.working_copy())
                && !s.summary.archived
                && !s.summary.has_messages
                && !s.busy
                && !s.has_saved_draft()
                && self
                    .sessions
                    .get(id)
                    .is_none_or(|v| v.read(cx).input.read(cx).content.is_empty())
                && self.workspace.tab_of(*id).is_none()
                && self
                    .workspace
                    .reviews
                    .get(id)
                    .is_none_or(|r| r.tabs.is_empty())
        }) {
            self.activate_session(id, window, cx);
        } else {
            self.create_session(self.default_kind(), true, cx);
        }
    }
    fn open_project(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open project".into()),
        });
        cx.spawn(async move |this, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.first() {
                    let path = path.to_string_lossy().into_owned();
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.workspace.add_project(&path);
                        this.select_project(path, window, cx);
                    });
                }
            }
            Ok(Ok(None)) => {}
            error => {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(format!("Could not open folder: {error:?}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn create(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.create_session(kind, false, cx);
    }
    fn create_session(&mut self, kind: Kind, only_if_empty: bool, cx: &mut Context<Self>) {
        self.create_session_target(kind, only_if_empty, None, cx);
    }
    fn create_in_folder(&mut self, folder: String, cx: &mut Context<Self>) {
        self.create_session_target(self.default_kind(), false, Some(folder), cx);
    }
    fn create_session_target(
        &mut self,
        kind: Kind,
        only_if_empty: bool,
        folder: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let cwd = self.workspace.project.clone();
        let focus = self.worktree_focus.get(&cwd).cloned();
        // The project folder as a selected checkout is not a separate worktree.
        let copy = focus.clone().filter(|f| {
            monocode_core::files::path_key(&f.path) != monocode_core::files::path_key(&cwd)
        });
        let blank_key = format!("{cwd}\0{}", focus.as_ref().map_or("", |f| f.path.as_str()));
        if only_if_empty && !self.pending_project_blanks.insert(blank_key.clone()) {
            return;
        }
        let model = self
            .workspace
            .preferences
            .project_providers
            .get(&cwd)
            .and_then(|p| p.models.get(kind.id()))
            .or(self.workspace.preferences.default_models.get(kind.id()))
            .cloned();
        let navigation_revision = self.navigation_revision;
        let account = crate::accounts::get(cx)
            .map(|center| center.read(cx).selected(kind, &cwd))
            .unwrap_or_default();
        let access = crate::conversation::default_access(cx);
        let effort = crate::conversation::preferred_effort(kind.id(), cx);
        let work = cx.background_executor().spawn({
            let cwd = cwd.clone();
            let worktree_cwd = copy.as_ref().map(|f| f.path.clone());
            async move {
                let store = locked(&store)?;
                let summary = store.create(kind.id(), &cwd)?;
                let mut settings = session_loading::new_session_settings(
                    store
                        .desktop_preferences()
                        .and_then(|preferences| preferences.session_defaults(kind.id(), &cwd)),
                    model,
                );
                settings.access = access;
                if let Some(effort) = effort {
                    settings.model_settings.retain(|key, _| {
                        !matches!(
                            key.as_str(),
                            "effort" | "reasoning" | "reasoningEffort" | "reasoning_effort"
                        )
                    });
                    settings.effort = Some(effort);
                }
                settings.account = account;
                settings.worktree_cwd = worktree_cwd;
                if settings.model.is_some()
                    || settings.effort.is_some()
                    || !settings.model_settings.is_empty()
                    || settings.worktree_cwd.is_some()
                    || settings.account.id != monocode_core::accounts::DEFAULT
                    || access != Default::default()
                {
                    store.save_agent_settings(summary.id, &settings)?;
                }
                Ok::<_, String>((summary, settings))
            }
        });
        let previous = self.new_tab_task.take();
        self.new_tab_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            if let Some(previous) = previous {
                previous.await;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if only_if_empty {
                    this.pending_project_blanks.remove(&blank_key);
                }
                match result {
                    Ok((summary, settings)) => {
                        let id = summary.id;
                        this.add(summary, cx);
                        if let Some(folder) = &folder {
                            let groups = this.groups.entry(cwd.clone()).or_default();
                            groups.add_to_folder(folder, id);
                            let groups = groups.clone();
                            let project = cwd.clone();
                            this.store_write("save folders", cx, move |store| {
                                store.save_groups(&project, &groups)
                            });
                        }
                        if let Some(view) = this.sessions.get(&id) {
                            view.update(cx, |view, cx| view.seed_new_settings(settings, cx));
                        }
                        this.seed_worktree(id, copy.as_ref(), cx);
                        // A pending blank belongs to the copy that requested it;
                        // navigation to another copy must never redirect that request.
                        if this.workspace.project != cwd
                            || this.worktree_focus.get(&cwd).map(|f| &f.path)
                                != focus.as_ref().map(|f| &f.path)
                            || if only_if_empty {
                                this.workspace.active.is_some()
                                    || this.mono_active(cx)
                                    || this.app_view_open()
                                    || this.settings.is_some()
                            } else {
                                this.navigation_revision != navigation_revision
                            }
                        {
                            this.invalidate_chrome(cx);
                            cx.notify();
                            return;
                        }
                        this.order.retain(|s| *s != id);
                        this.order.insert(0, id);
                        this.activate_session(id, window, cx);
                    }
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }
    /// The title bar's navigation (`data-inset-title-nav`): as wide as the
    /// open sidebar, with history at its far edge; with the sidebar closed,
    /// just the traffic lights' gap and its three buttons.
    fn traffic_lights(&self) -> f32 {
        if self.fullscreen {
            NO_TRAFFIC_LIGHTS
        } else {
            TRAFFIC_LIGHTS
        }
    }
    fn navigation_width(&self) -> f32 {
        let closed = self.traffic_lights() + 3. * 24.;
        if self.workspace.classic_layout {
            return if self.workspace.project_rail_open || self.settings.is_some() {
                self.project_rail_drawn_width().max(closed + 6.)
            } else {
                closed
            };
        }
        closed + (self.workspace.sidebar_width.max(closed + 6.) - closed) * self.sidebar_fraction
    }
    /// Classic: the strip's room before its tabs, wherever the navigation
    /// overhangs the sidebar.
    fn strip_lead(&self, cx: &App) -> f32 {
        (self.navigation_width() - self.left_sidebar_width(cx)).max(0.)
    }
    /// The traffic lights, sidebar toggle and visit arrows, spanning the
    /// open sidebar so the arrows sit at its edge.
    fn navigation(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .w(px(self.navigation_width()))
            .h(px(TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            // `pr-1.5` only beside the open sidebar; closed, the tab
            // strip's own inset is gap enough after the arrows.
            .pr(px(if self.workspace.classic_layout {
                if self.workspace.project_rail_open || self.settings.is_some() {
                    6.
                } else {
                    0.
                }
            } else {
                6. * self.sidebar_fraction
            }))
            .child(
                // Fixed, so the toggle holds still as the sidebar opens
                // and closes.
                titlebar_drag_area()
                    .w(px(self.traffic_lights()))
                    .flex_shrink_0(),
            )
            .child(self.sidebar_toggle(cx))
            .child(titlebar_drag_area().flex_1())
            .child(
                icon_button_enabled(
                    "visit-back",
                    BACK_LABEL,
                    "chevron-left",
                    self.settings.is_some()
                        || (self.workspace.classic_layout && self.mono_active(cx))
                        || !self.history.back.is_empty(),
                )
                .debug_selector(|| "visit-back".into())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.workspace.classic_layout {
                        this.classic_rail_back(window, cx)
                    } else {
                        this.visit(-1, window, cx)
                    }
                })),
            )
            .child(
                icon_button_enabled(
                    "visit-forward",
                    FORWARD_LABEL,
                    "chevron-right",
                    !self.history.forward.is_empty(),
                )
                .debug_selector(|| "visit-forward".into())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, window, cx| this.visit(1, window, cx))),
            )
    }
    /// Settings fill the sidebar's place, so its toggle steps aside. Its
    /// slot stays, so nothing beside it moves.
    fn sidebar_toggle(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        if self.settings.is_some() {
            return div()
                .id("toggle-sidebar")
                .debug_selector(|| "toggle-sidebar".into())
                .flex_shrink_0()
                .size(px(24.))
                .child(self.peek_toggle_bounds.probe());
        }
        icon_button("toggle-sidebar", TOGGLE_SIDEBAR_LABEL, "sidebar")
            .debug_selector(|| "toggle-sidebar".into())
            .child(self.peek_toggle_bounds.probe())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Hovering the toggle peeks a collapsed sidebar out.
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if !this.workspace.classic_layout {
                    this.hover_peek_toggle(*hovered, cx)
                }
            }))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_primary_sidebar(window, cx)))
    }
    /// Part of the card: the project tools beside the workspace, behind a
    /// hairline. Its contents keep their full width while it slides.
    fn right_panel_frame(
        &mut self,
        panel: Entity<RightPanel>,
        width: f32,
        full: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content =
            if let Some(monos) = self.monos.as_ref().filter(|m| m.read(cx).active.is_some()) {
                let manager = monos.read(cx);
                let details = manager.details.clone();
                let html = manager.native_detail_content(cx);
                // Native artifact views must update their frames and clipping
                // on every paint; replaying cached GPU quads skips that work.
                if html {
                    details.into_any_element()
                } else {
                    details
                        .cached(StyleRefinement::default().size_full())
                        .into_any_element()
                }
            } else if self.terminal_in_panel(cx)
                && panel.read(cx).tab() == crate::right_panel::RightTab::Terminal
            {
                self.panel_terminal(window, cx)
            } else {
                panel
                    .cached(StyleRefinement::default().size_full())
                    .into_any_element()
            };
        div()
            .id("right-panel-frame")
            .debug_selector(|| "right-panel-frame".into())
            .relative()
            .h_full()
            .flex_shrink_0()
            .w(px(width))
            .border_l_1()
            .border_color(crate::right_panel::stroke())
            // The full-width contents travel with the leading edge instead of
            // reflowing into the growing clip on every animation frame.
            .child(
                div().relative().size_full().overflow_hidden().child(
                    div()
                        .id("right-panel-content")
                        .debug_selector(|| "right-panel-content".into())
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(px(full - 1.))
                        .child(content),
                ),
            )
            .child(
                self.right_panel_resize_handle("right-panel-resize", cx)
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-3.5))
                    .child(resize_border(
                        true,
                        self.right_resize.is_some() || self.right_resize_hovered,
                    )),
            )
            .into_any_element()
    }

    fn right_panel_resize_handle(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .group("resize-handle")
            .role(Role::Splitter)
            .aria_label("Resize right sidebar")
            .w(px(6.))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.right_resize_hovered != *hovered {
                    this.right_resize_hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.right_resize = Some((event.position.x, this.right_panel_width()));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    this.right_resize = None;
                    this.workspace.right_panel_width = crate::right_panel::WIDTH_DEFAULT;
                    this.persist(cx);
                    cx.notify();
                }
            }))
    }
    /// Mono detail layers occlude the shell's ordinary mouse handlers. An
    /// active resize must keep receiving moves and release over those layers.
    fn right_panel_resize_watch(&self, cx: &Context<Self>) -> AnyElement {
        let shell = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moves = shell.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moves.update(cx, |this, cx| {
                            if this.drag_right_panel(event, cx) {
                                cx.stop_propagation();
                            }
                        });
                    }
                });
                let release = shell.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = release.update(cx, |this, cx| {
                            if this.right_resize.is_some() {
                                this.release_resizes(cx);
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_0()
        .into_any_element()
    }

    fn drag_right_panel(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        let Some((start, width)) = self.right_resize else {
            return false;
        };
        if event.pressed_button == Some(MouseButton::Left) {
            let width = width - (event.position.x - start).as_f32();
            self.drag_resize(ResizeTarget::RightPanel, width, cx);
        } else {
            self.release_resizes(cx);
        }
        true
    }

    /// Client-decorated Linux: the bar ends left of the window controls.
    /// The panel's width as drawn: the user's width, widened while Windows'
    /// controls need room beside the corner's tools. The stored width stays
    /// as the user set it, so closing a Mono gives it back.
    pub(super) fn right_panel_width(&self) -> f32 {
        self.workspace.right_panel_width.max(self.right_panel_min)
    }
    fn titlebar_controls_space(&self, bar: AnyElement, window: &Window) -> AnyElement {
        let room = client_controls_width(window) - corner_controls_width(window);
        if room == 0. {
            return bar;
        }
        div()
            .w_full()
            .h(px(TITLE_BAR_HEIGHT))
            .pr(px(room))
            .child(bar)
            .into_any_element()
    }
    fn titlebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let bar = self.titlebar_content(window, cx);
        self.titlebar_controls_space(bar, window)
    }
    fn titlebar_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Same hierarchy as TitleBar.tsx's inset layout: window navigation and
        // session tabs share the outer shell, above the rounded workspace card.
        // Windows' controls sit in the settings header's padding, so its
        // bottom rule still runs beneath them. Session bars leave them to the
        // corner (`toggle_room`, `corner_room`).
        let controls = px(self.window_controls);
        if let Some(settings) = self.settings.clone() {
            let classic = self.workspace.boxy_layout();
            let lead = if classic {
                self.strip_lead(cx)
            } else {
                self.navigation_width() + 6.
            };
            let header = settings.update(cx, |v, cx| v.header(!classic, cx));
            return div()
                .h(px(TITLE_BAR_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .pr(controls)
                .when(classic, |d| d.border_b_1().border_color(content(0.1)))
                .child(if classic {
                    titlebar_drag_area()
                        .w(px(lead))
                        .flex_shrink_0()
                        .into_any_element()
                } else {
                    self.navigation(cx).into_any_element()
                })
                .child(header)
                .into_any_element();
        }
        let tabs_width = px(self.tabs_width(window, cx));
        let classic = self.workspace.boxy_layout();
        div()
            .h(px(TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .when(self.workspace.classic_layout, |d| d.items_start())
            // Classic: the strip heads the workspace column, above a rule.
            .when(classic, |d| {
                d.border_b_1().border_color(crate::right_panel::stroke())
            })
            .child(if classic && !self.classic_titlebar_spans_window(cx) {
                // The navigation floats over the sidebar's header
                // (classic_body); the strip keeps only the part it uncovers.
                titlebar_drag_area()
                    .w(px(self.strip_lead(cx)))
                    .flex_shrink_0()
                    .into_any_element()
            } else {
                self.navigation(cx).into_any_element()
            })
            .child(
                self.tabs_frame.clone().cached(
                    StyleRefinement::default()
                        .w(tabs_width)
                        .h(px(TITLE_BAR_HEIGHT)),
                ),
            )
            .child(if classic {
                // The toggle holds the window's corner (classic_body); the
                // strip keeps only the part an open panel leaves uncovered.
                titlebar_drag_area()
                    .w(px(self.strip_trail(cx)))
                    .flex_shrink_0()
                    .into_any_element()
            } else {
                self.panel_toggle(window, cx)
            })
            .into_any()
    }

    /// TitleBar.tsx's right panel toggle, `pl-1.5 pr-3` clear of the edge.
    fn panel_toggle(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mono = self.mono_active(cx);
        if self.workspace.classic_layout && !mono {
            return div().into_any_element();
        }
        if self.workspace.boxy_layout() && !mono {
            return self.classic_corner(window, cx);
        }
        let toggle = mono || self.git.read(cx).available();
        let panel_off = self.app_view_open();
        let dot = !mono && !panel_off && self.git.read(cx).has_changes();
        let panel_active = self.workspace.right_panel_open && !panel_off;
        let tabs = if mono && panel_active {
            self.monos.clone().map(|m| {
                m.update(cx, |m, cx| {
                    m.overview_controls(
                        self.right_panel_width() >= 312. + self.window_controls,
                        window,
                        cx,
                    )
                })
            })
        } else {
            self.right_panel
                .as_ref()
                .filter(|_| panel_active && !mono)
                .map(|panel| {
                    panel
                        .read(cx)
                        .tabs
                        .clone()
                        .cached(
                            StyleRefinement::default()
                                .w(px(self.project_tabs_width(cx)))
                                .h(px(TITLE_BAR_HEIGHT)),
                        )
                        .into_any_element()
                })
        };
        let show_tabs = tabs.is_some();
        div()
            .relative()
            .w(px(self.toggle_room(cx)))
            .h(px(TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .map(|d| {
                if !toggle {
                    return d
                        .pl_1p5()
                        .pr(px(12. + self.window_controls))
                        .child(self.file_search_button(cx));
                }
                let ink = if panel_off {
                    content(0.25)
                } else if panel_active && !self.workspace.boxy_layout() {
                    content(1.)
                } else {
                    content(0.5)
                };
                d.child(
                    div()
                        .h_full()
                        .w_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .pl(px(if show_tabs && !self.workspace.boxy_layout() {
                            CORNER_TABS_LEAD
                        } else {
                            6.
                        }))
                        .pr(px(12. + self.window_controls))
                        .when_some(tabs, |d, tabs| {
                            d.child(tabs)
                                .when(!mono, |d| d.child(titlebar_drag_area().flex_1().min_w_0()))
                        })
                        .children(self.project_diff_stat(cx))
                        .child(self.file_search_button(cx).mx(px(CORNER_CONTROL_GAP)))
                        .child(
                            div()
                                .id("toggle-right-panel")
                                .debug_selector(|| "toggle-right-panel".into())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .role(Role::Button)
                                .aria_label(if dot {
                                    TOGGLE_RIGHT_PANEL_DOT_LABEL
                                } else {
                                    TOGGLE_RIGHT_PANEL_LABEL
                                })
                                .aria_toggled(if panel_active {
                                    gpui::Toggled::True
                                } else {
                                    gpui::Toggled::False
                                })
                                .group("toggle-right-panel")
                                .cursor_pointer()
                                .size(px(24.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .when(!panel_off, |d| d.hover(|s| s.bg(content(0.1))))
                                .child(
                                    div()
                                        .relative()
                                        .size(px(14.))
                                        .child(icon("sidebar-right", 14.).text_color(ink).when(
                                            !panel_off,
                                            |i| {
                                                i.group_hover("toggle-right-panel", |s| {
                                                    s.text_color(content(1.))
                                                })
                                            },
                                        ))
                                        .when(dot, |d| {
                                            d.child(
                                                div()
                                                    .absolute()
                                                    .top(px(-2.))
                                                    .right(px(-2.))
                                                    .size(px(6.))
                                                    .rounded_full()
                                                    .bg(accent_color()),
                                            )
                                        }),
                                )
                                .when(!panel_off, |d| {
                                    d.on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_right_panel(window, cx)
                                    }))
                                }),
                        ),
                )
            })
            .when(show_tabs && !self.workspace.boxy_layout(), |d| {
                // The card has 6px of outer padding and a 1px right border.
                // Keep the rule on its divider even during the opening slide.
                let offset = self.toggle_room(cx) - (self.right_drawn + 7.);
                d.child(
                    self.right_panel_resize_handle("titlebar-right-panel-resize", cx)
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(offset - 2.5))
                        .child(
                            div()
                                .id("project-tabs-separator")
                                .debug_selector(|| "project-tabs-separator".into())
                                .absolute()
                                .left(px(2.5))
                                .top(px((TITLE_BAR_HEIGHT - 16.) / 2.))
                                .w(px(1.))
                                .h(px(16.))
                                .bg(crate::right_panel::stroke())
                                .when(
                                    self.right_resize.is_some() || self.right_resize_hovered,
                                    |d| d.bg(content(0.16)),
                                )
                                .group_hover("resize-handle", |s| s.bg(content(0.16))),
                        ),
                )
            })
            .into_any()
    }
    fn project_diff_stat(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let width = self.diff_stat_room(cx);
        // With no Changes tab, the stat marks the open Changes pane.
        let selected = self.workspace.right_panel_open
            && !self.app_view_open()
            && self
                .right_panel
                .as_ref()
                .is_some_and(|p| p.read(cx).tab() == crate::right_panel::RightTab::Changes);
        (width > 0.).then(|| {
            crate::right_panel::diff_stat(self.git.read(cx).stats.as_ref(), width)
                .when(selected, |d| d.bg(content(0.1)))
                .when(!self.app_view_open(), |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(content(0.1)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.ensure_right_panel(window, cx);
                            let panel = this.right_panel.clone().unwrap();
                            let previous = panel.read(cx).tab();
                            let changes = crate::right_panel::RightTab::Changes;
                            if previous != changes {
                                this.corner_reveal =
                                    Some(crate::project_tabs::TabReveal::new(changes, previous));
                            }
                            panel.update(cx, |panel, cx| panel.select(changes, cx));
                            this.set_right_panel_open(true, window, cx);
                            panel.update(cx, |panel, cx| panel.focus_pane(window, cx));
                            this.invalidate_chrome(cx);
                            cx.notify();
                        }))
                })
                .into_any_element()
        })
    }
    fn file_search_button(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let label = if cfg!(target_os = "macos") {
            "Search files (⌘P)"
        } else {
            "Search files (Ctrl+P)"
        };
        icon_button("titlebar-file-search", label, "search")
            .debug_selector(|| "titlebar-file-search".into())
            .flex_shrink_0()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.open_quick_open(false, window, cx)))
    }
    fn diff_stat_room(&self, cx: &App) -> f32 {
        if self.mono_active(cx) || !self.git.read(cx).available() {
            return 0.;
        }
        let natural = crate::right_panel::diff_stat_width(self.git.read(cx).stats.as_ref());
        if !self.workspace.right_panel_open || self.app_view_open() {
            return natural;
        }
        let controls = crate::right_panel::tab_icons_width(self.right_tab_count(cx))
            + 6.
            + CORNER_BUTTONS_WIDTH
            + 12.
            + self.window_controls;
        natural.min((self.right_panel_width() - controls).max(48.))
    }
    fn project_tabs_width(&self, cx: &App) -> f32 {
        let padding = if self.workspace.boxy_layout() {
            24.
        } else {
            12.
        };
        (self.right_panel_width()
            - self.diff_stat_room(cx)
            - CORNER_BUTTONS_WIDTH
            - padding
            - self.window_controls)
            .clamp(
                crate::right_panel::tab_icons_width(self.right_tab_count(cx)),
                crate::right_panel::tabs_width(self.right_tab_count(cx)),
            )
    }
    /// Explorer and History, and Terminal when it lives in the panel. Changes
    /// opens from the diff stat instead.
    fn right_tab_count(&self, cx: &App) -> usize {
        2 + usize::from(self.terminal_in_panel(cx))
    }
    /// The toggle's room at the end of the strip: `pl-1.5`, the button, `pr-3`.
    fn toggle_room(&self, cx: &App) -> f32 {
        if self.workspace.boxy_layout() {
            return self.corner_room(cx);
        }
        // Client-drawn window controls hold the corner's right end.
        let controls = self.window_controls;
        if self.mono_active(cx) || self.git.read(cx).available() {
            if self.workspace.right_panel_open && !self.app_view_open() {
                if self.mono_active(cx) {
                    let minimum = if self.right_panel_width() >= 312. + controls {
                        312.
                    } else {
                        252.
                    };
                    return (self.right_drawn + 7.).max(minimum + controls);
                }
                // Follow the rounded panel's left edge, including the card's
                // right inset, while keeping room for all controls when narrow.
                (self.right_drawn + 7.).max(
                    CORNER_TABS_LEAD
                        + self.project_tabs_width(cx)
                        + self.diff_stat_room(cx)
                        + CORNER_BUTTONS_WIDTH
                        + 12.
                        + controls,
                )
            } else {
                6. + self.diff_stat_room(cx) + CORNER_BUTTONS_WIDTH + 12. + controls
            }
        } else {
            6. + 24. + 12. + controls
        }
    }
    /// Classic: the strip's room after its tabs, wherever the toggle
    /// overhangs an open right panel.
    fn strip_trail(&self, cx: &App) -> f32 {
        (self.toggle_room(cx) - self.right_drawn).max(0.)
    }
    fn menu_open(&self, target: &MenuTarget) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|open| open.target == *target)
    }
    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            self.clear_context_selection();
            self.invalidate_chrome(cx);
            cx.notify();
        }
    }
    pub(crate) fn open_menu(
        &mut self,
        target: MenuTarget,
        anchor: Anchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_preview(cx);
        // Close the previous menu first, so the new one remembers the focus
        // that was there before either opened.
        self.menu = None;
        let mut events = Vec::new();
        let (label, items, width, style) = self.menu_items(&target, &mut events, window, cx);
        let menu = cx.new(|cx| {
            Menu::new(label, items, window, cx)
                .width(width)
                .style(style)
        });
        events.push(
            cx.subscribe_in(&menu, window, |this, _, event: &MenuEvent, window, cx| {
                if let MenuEvent::Picked(id, _) = event
                    && let Some(open) = this
                        .menu
                        .as_ref()
                        .filter(|m| m.target == MenuTarget::SidebarOptions)
                {
                    let menu = open.menu.clone();
                    this.change_sidebar_option(id, cx);
                    let items = this.sidebar_options_menu_items();
                    menu.update(cx, |menu, cx| menu.replace_items(items, cx));
                    return;
                }
                if let MenuEvent::Picked(id, _) = event
                    && id != "archived"
                    && let Some(open) = this
                        .menu
                        .as_ref()
                        .filter(|m| m.target == MenuTarget::SessionFilter)
                {
                    let menu = open.menu.clone();
                    this.change_session_filter(id, cx);
                    let items = this.filter_menu_items(cx);
                    menu.update(cx, |menu, cx| menu.replace_items(items, cx));
                    return;
                }
                let Some(open) = this.menu.take() else {
                    return;
                };
                let target = open.target.clone();
                if let MenuEvent::Picked(id, row) = event {
                    this.run_menu(target, id, *row, window, cx);
                }
                this.clear_context_selection();
                this.invalidate_chrome(cx);
                cx.notify();
            }),
        );
        self.menu = Some(ShellMenu {
            target,
            anchor,
            menu,
            _events: events,
        });
        self.invalidate_chrome(cx);
        cx.notify();
    }
    fn menu_items(
        &self,
        target: &MenuTarget,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (SharedString, Vec<MenuItem>, f32, MenuStyle) {
        if let Some(menu) = self.terminal_menu_items(target, cx) {
            return menu;
        }
        let title = |id: &i64| {
            self.session_summaries
                .get(id)
                .map(|_| self.session_metadata(*id, cx).display_title())
                .unwrap_or_default()
        };
        match target {
            // Answered by terminal_menu_items above.
            MenuTarget::TerminalTab(_) | MenuTarget::RunningTerminals => {
                ("".into(), Vec::new(), 160., MenuStyle::Explorer)
            }
            MenuTarget::Actions => (
                "Actions".into(),
                self.actions_menu(),
                200.,
                MenuStyle::Explorer,
            ),
            MenuTarget::Tab(_)
            | MenuTarget::MonoTab(_)
            | MenuTarget::SearchTab
            | MenuTarget::AppTab(_) => {
                let tab = match target {
                    MenuTarget::Tab(id) => TitleTab::Session(*id),
                    MenuTarget::MonoTab(id) => self.mono_tab(id.clone()),
                    MenuTarget::SearchTab => TitleTab::Search,
                    MenuTarget::AppTab(view) => TitleTab::App(*view),
                    _ => unreachable!(),
                };
                let tabs = self.visible_tabs(cx);
                let ix = tabs.iter().position(|entry| *entry == tab).unwrap_or(0);
                let count = tabs.len();
                let closable = tab.session().is_none_or(|id| self.tab_can_close(id, cx));
                let mut items = vec![
                    MenuAction::new("close", "Close Tab")
                        .shortcut(format!("{MOD}W"))
                        .disabled(!closable)
                        .into(),
                    MenuItem::Separator,
                    MenuAction::new("others", "Close Other Tabs")
                        .disabled(count < 2)
                        .into(),
                    MenuAction::new("right", "Close Tabs to the Right")
                        .disabled(ix + 1 >= count)
                        .into(),
                    MenuAction::new("left", "Close Tabs to the Left")
                        .disabled(ix == 0)
                        .into(),
                    MenuAction::new("all", "Close All Tabs")
                        .shortcut(if cfg!(target_os = "macos") {
                            "⌘⇧W"
                        } else {
                            "Ctrl+Shift+W"
                        })
                        .disabled(count < 2 && !closable)
                        .into(),
                ];
                // Closing a tab keeps its session; archive also hides it from
                // the active history while preserving everything on disk.
                if let Some(id) = tab.session().filter(|id| {
                    self.session_summaries.contains_key(id)
                        && self.session_metadata(*id, cx).summary.has_messages
                }) {
                    items.extend([
                        MenuItem::Separator,
                        MenuAction::new(
                            "archive",
                            if self.session_metadata(id, cx).summary.archived {
                                "Unarchive"
                            } else {
                                "Archive"
                            },
                        )
                        .into(),
                        MenuAction::new("delete", "Delete").danger().into(),
                    ]);
                }
                (
                    format!(
                        "Tab actions for {}",
                        match &tab {
                            TitleTab::Session(id) => title(id),
                            TitleTab::Search => "Search".into(),
                            TitleTab::App(view) => view.label().into(),
                            TitleTab::Mono { id, .. } => self
                                .monos
                                .as_ref()
                                .and_then(|m| m.read(cx).roster.get(id))
                                .map(|m| m.label())
                                .unwrap_or_default(),
                        }
                    )
                    .into(),
                    items,
                    244.,
                    MenuStyle::Explorer,
                )
            }
            MenuTarget::Session(id) => (
                format!("Session actions for {}", title(id)).into(),
                self.session_menu_items(*id, cx),
                228.,
                MenuStyle::Explorer,
            ),
            MenuTarget::Folder(folder) => (
                "Folder actions".into(),
                self.folder_menu_items(folder, events, cx),
                228.,
                MenuStyle::Explorer,
            ),
            MenuTarget::TerminalDock => {
                let side = self
                    .workspace
                    .docks
                    .get(&self.workspace.project)
                    .map_or(DockSide::Bottom, |d| d.side);
                (
                    "Move terminal".into(),
                    [
                        ("bottom", "Dock Bottom", DockSide::Bottom),
                        ("top", "Dock Top", DockSide::Top),
                        ("left", "Dock Left", DockSide::Left),
                        ("right", "Dock Right", DockSide::Right),
                    ]
                    .into_iter()
                    .map(|(id, label, s)| MenuAction::new(id, label).checked(s == side).into())
                    .collect(),
                    160.,
                    MenuStyle::Explorer,
                )
            }
            MenuTarget::SessionFilter => (
                "Filter sessions".into(),
                self.filter_menu_items(cx),
                228.,
                MenuStyle::Filter,
            ),
            MenuTarget::SidebarOptions => (
                "Sidebar options".into(),
                self.sidebar_options_menu_items(),
                184.,
                MenuStyle::Filter,
            ),
            MenuTarget::ProjectRailAdd => (
                "Open project".into(),
                vec![
                    MenuAction::new("open-project", "Open folder…")
                        .icon("folder-add")
                        .into(),
                    MenuAction::new("open-remote-project", "Open folder on a machine…")
                        .icon("globe")
                        .into(),
                ],
                230.,
                MenuStyle::Explorer,
            ),
            MenuTarget::CompactProjectPicker => {
                let (monos, active) = self
                    .monos
                    .as_ref()
                    .filter(|_| self.workspace.classic_layout && self.workspace.preferences.monos)
                    .map(|manager| {
                        let manager = manager.read(cx);
                        let monos = manager
                            .roster
                            .monos
                            .iter()
                            .map(|mono| crate::pickers::PickerMono {
                                id: mono.id.clone(),
                                name: mono.label(),
                                mascot: mono.mascot.clone(),
                                color: mono.color,
                            })
                            .collect();
                        (monos, manager.active.clone())
                    })
                    .unwrap_or_default();
                let picker = cx.new(|cx| {
                    ProjectPicker::new(&self.workspace.projects, &self.workspace.project, cx)
                        .appearance(self.workspace.project_appearance.clone())
                        .allow_remote()
                        .monos(monos, active, cx)
                });
                events.push(cx.subscribe_in(
                    &picker,
                    window,
                    |this, _, picked: &ProjectPicked, window, cx| {
                        this.close_menu(cx);
                        match picked {
                            ProjectPicked::Project(path) => {
                                this.select_project(path.clone(), window, cx)
                            }
                            ProjectPicked::Mono(id) => {
                                if let Some(monos) = this.monos.clone() {
                                    monos.update(cx, |m, cx| m.open(id, window, cx));
                                }
                            }
                            ProjectPicked::New => this.open_project(cx),
                            ProjectPicked::Remote => {
                                this.open_remote_project(window, cx);
                                return;
                            }
                        }
                        this.focus_active(window, cx);
                        this.invalidate_chrome(cx);
                        cx.notify();
                    },
                ));
                let focus = picker.read(cx).search_focus(cx);
                (
                    "Switch project".into(),
                    vec![MenuItem::Custom(picker.into(), focus)],
                    286.,
                    MenuStyle::Switcher,
                )
            }
            // SidebarWorktreeSwitcher's menu under the project header.
            MenuTarget::Header => {
                let picker = cx.new(|cx| {
                    ProjectPicker::new(&self.workspace.projects, &self.workspace.project, cx)
                        .appearance(self.workspace.project_appearance.clone())
                        .allow_remote()
                });
                events.push(cx.subscribe_in(
                    &picker,
                    window,
                    |this, _, picked: &ProjectPicked, window, cx| {
                        this.close_menu(cx);
                        match picked {
                            ProjectPicked::Project(path) if *path != this.workspace.project => {
                                this.select_project(path.clone(), window, cx)
                            }
                            ProjectPicked::Project(_) | ProjectPicked::Mono(_) => {}
                            ProjectPicked::New => this.open_project(cx),
                            ProjectPicked::Remote => {
                                this.open_remote_project(window, cx);
                                return;
                            }
                        }
                        this.focus_active(window, cx);
                        this.invalidate_chrome(cx);
                        cx.notify();
                    },
                ));
                let focus = picker.read(cx).search_focus(cx);
                let branch = self
                    .worktree_focus
                    .get(&self.workspace.project)
                    .and_then(|focus| focus.branch.clone())
                    .or_else(|| {
                        crate::conversation::git_branch(std::path::Path::new(
                            &self.workspace.project,
                        ))
                    });
                let worktree = MenuAction::new("switch-worktree", "Switch worktree")
                    .icon("folder-tree")
                    .flyout(self.worktree_flyout(events, window, cx));
                (
                    format!("{} menu", self.project_label(&self.workspace.project)).into(),
                    vec![
                        match branch {
                            Some(branch) => worktree.value(branch),
                            None => worktree,
                        }
                        .into(),
                        MenuItem::Separator,
                        MenuAction::new("switch-project", "Switch project")
                            .icon("folder")
                            .flyout(Flyout {
                                view: picker.into(),
                                width: 286.,
                                padding: 0.,
                                height: None,
                                max_height: Some(380.),
                                focus: Some(focus),
                                surface: None,
                            })
                            .into(),
                        MenuAction::new("project-settings", "Project settings")
                            .icon("sliders")
                            .flyout(self.project_settings_flyout(events, window, cx))
                            .into(),
                        MenuAction::new("open-remote-project", "Open folder on a machine…")
                            .icon("globe")
                            .into(),
                        MenuAction::new("settings", "Settings")
                            .icon("settings")
                            .shortcut(format!("{MOD},"))
                            .into(),
                    ],
                    240.,
                    MenuStyle::Switcher,
                )
            }
            MenuTarget::ProjectRailGroup(id) => {
                let controls = self.project_rail_group_controls(id, events, window, cx);
                (
                    "Project group actions".into(),
                    vec![
                        MenuItem::Custom(controls.clone().into(), controls.read(cx).focus(cx)),
                        MenuItem::Separator,
                        MenuAction::new("delete-project-group", "Delete group")
                            .icon("trash")
                            .description("Projects will become ungrouped")
                            .danger()
                            .into(),
                    ],
                    260.,
                    MenuStyle::Group,
                )
            }
            // TabGroupMenu's appearance controls and project actions.
            MenuTarget::Project { path, rail } => {
                let controls = self.project_appearance_controls(path, events, window, cx);
                let mut items: Vec<MenuItem> = vec![
                    MenuItem::Custom(controls.clone().into(), controls.read(cx).focus(cx)),
                    MenuItem::Separator,
                    MenuAction::new("project-theme", "Theme colors")
                        .icon("palette")
                        .into(),
                    MenuAction::new("background", "Background image")
                        .icon("image-add")
                        .into(),
                ];
                if *rail {
                    let mut groups = vec![MenuAction::new("project-group:new", "New group…")];
                    if self.workspace.project_rail_assignments.contains_key(path) {
                        groups.push(MenuAction::new("project-group:", "No group"));
                    }
                    groups.extend(self.workspace.project_rail_groups.iter().map(|group| {
                        MenuAction::new(format!("project-group:{}", group.id), group.name.clone())
                            .checked(
                                self.workspace.project_rail_assignments.get(path)
                                    == Some(&group.id),
                            )
                    }));
                    let pinned = self.workspace.project_rail_pinned.contains(path);
                    items.extend([
                        MenuAction::new("project-group", "Move to group")
                            .icon("folder-tree")
                            .submenu(groups)
                            .into(),
                        MenuAction::new(
                            "pin",
                            if pinned {
                                "Unpin project"
                            } else {
                                "Pin project"
                            },
                        )
                        .icon(if pinned { "pin-off" } else { "pin" })
                        .into(),
                    ]);
                }
                let peers = self.workspace.project_peers(path);
                if peers.len() > 1 {
                    let ix = peers.iter().position(|p| p == path).unwrap_or(0);
                    let (first, last) = (ix == 0, ix + 1 == peers.len());
                    let (back, forward) = if self.workspace.classic_layout {
                        ("Move up", "Move down")
                    } else {
                        ("Move left", "Move right")
                    };
                    let shortcut = |command| {
                        crate::settings::command_shortcut(&self.workspace.preferences, command)
                    };
                    let step = |id, label, command, disabled| {
                        let action = MenuAction::new(id, label).disabled(disabled);
                        match shortcut(command) {
                            Some(keys) => action.shortcut(keys),
                            None => action,
                        }
                    };
                    items.push(
                        MenuAction::new("move", "Move")
                            .icon("chevrons-up-down")
                            .submenu(vec![
                                step("move:back", back, "Project: Move Back", first),
                                step("move:forward", forward, "Project: Move Forward", last),
                                MenuAction::new("move:start", "Move to start").disabled(first),
                                MenuAction::new("move:end", "Move to end").disabled(last),
                            ])
                            .into(),
                    );
                }
                items.extend([
                    MenuAction::new("reveal", REVEAL_LABEL)
                        .icon("folder-open")
                        .into(),
                    self.external_editor_menu(path).into(),
                    MenuItem::Separator,
                    self.project_notification_menu(path).into(),
                    MenuItem::Separator,
                    MenuAction::new("archive", "Archive").icon("archive").into(),
                    MenuAction::new("delete", "Delete")
                        .icon("trash")
                        .danger()
                        .into(),
                ]);
                (
                    format!("Project actions for {}", project_name(path)).into(),
                    items,
                    260.,
                    MenuStyle::Group,
                )
            }
        }
    }
    fn run_menu(
        &mut self,
        target: MenuTarget,
        action: &str,
        row: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let MenuTarget::Tab(id) = target {
            match action {
                "archive" => return self.toggle_archive(id, cx),
                "delete" => return self.confirm_delete_session(id, window, cx),
                _ => {}
            }
        }
        match target {
            MenuTarget::TerminalTab(ix) => self.run_terminal_tab_menu(ix, action, window, cx),
            MenuTarget::RunningTerminals => self.run_running_terminals_menu(action, window, cx),
            MenuTarget::Actions => self.run_actions_menu(action, window, cx),
            MenuTarget::TerminalDock => {
                let side = match action {
                    "top" => DockSide::Top,
                    "left" => DockSide::Left,
                    "right" => DockSide::Right,
                    _ => DockSide::Bottom,
                };
                self.set_dock_side(side, cx);
            }
            MenuTarget::SessionFilter => self.change_session_filter(action, cx),
            MenuTarget::SidebarOptions => self.change_sidebar_option(action, cx),
            MenuTarget::Tab(id) => {
                self.run_title_tab_menu(TitleTab::Session(id), action, window, cx)
            }
            MenuTarget::MonoTab(id) => {
                self.run_title_tab_menu(self.mono_tab(id), action, window, cx)
            }
            MenuTarget::SearchTab => self.run_title_tab_menu(TitleTab::Search, action, window, cx),
            MenuTarget::AppTab(view) => {
                self.run_title_tab_menu(TitleTab::App(view), action, window, cx)
            }
            MenuTarget::Session(id) => self.run_session_menu(id, action, window, cx),
            MenuTarget::Folder(folder) => self.run_folder_menu(folder, action, window, cx),
            MenuTarget::ProjectRailAdd => match action {
                "open-project" => self.open_project(cx),
                "open-remote-project" => self.open_remote_project(window, cx),
                _ => {}
            },
            MenuTarget::CompactProjectPicker => {}
            MenuTarget::ProjectRailGroup(id) => {
                if action == "delete-project-group" {
                    self.workspace.project_rail_groups.retain(|g| g.id != id);
                    self.workspace
                        .project_rail_assignments
                        .retain(|_, group| group != &id);
                    self.persist(cx);
                    self.projects_frame.update(cx, |_, cx| cx.notify());
                }
            }
            MenuTarget::Header => {
                // "Project settings" opens the project's menu beside its row.
                if action == "settings" {
                    self.open_settings(crate::settings::Section::General, window, cx);
                } else if action == "open-remote-project" {
                    self.open_remote_project(window, cx);
                } else if action == "project-settings" {
                    let at = row
                        .map_or_else(Point::default, |r| Point::new(r.right(), r.top() - px(8.)));
                    let target = MenuTarget::Project {
                        path: self.workspace.project.clone(),
                        rail: false,
                    };
                    self.open_menu(target, Anchor::Point(at), window, cx);
                }
            }
            MenuTarget::Project { path, .. } => {
                if action == "pin" {
                    self.toggle_project_rail_pin(&path, cx);
                } else if action == "project-group:new" {
                    let id = self.create_project_rail_group(Some(&path), cx);
                    let at = row.map_or_else(Point::default, |r| Point::new(r.left(), r.top()));
                    self.open_menu(
                        MenuTarget::ProjectRailGroup(id),
                        Anchor::Point(at),
                        window,
                        cx,
                    );
                } else if let Some(group) = action.strip_prefix("project-group:") {
                    self.assign_project_rail_group(&path, group, cx);
                } else if let Some(direction) = action.strip_prefix("move:") {
                    let moved = match direction {
                        "back" => self.workspace.step_project_order(&path, -1),
                        "forward" => self.workspace.step_project_order(&path, 1),
                        "start" => self.workspace.move_project_to_end(&path, -1),
                        _ => self.workspace.move_project_to_end(&path, 1),
                    };
                    if moved {
                        self.project_order_changed(&path, cx);
                    }
                } else if action == "reveal" {
                    if path.starts_with("remote://") {
                        self.error = Some("Remote folders stay on their host machine.".into());
                        cx.notify();
                    } else {
                        cx.reveal_path(std::path::Path::new(&path));
                    }
                } else if action == "project-theme" {
                    self.open_project_theme(path, window, cx);
                } else if action == "background" {
                    self.open_background_dialog(path, window, cx);
                } else if action == "archive" {
                    self.archive_project(&path, window, cx);
                } else if action == "delete" {
                    self.confirm_delete_project(path, window, cx);
                } else if let Some(editor) = action
                    .strip_prefix("external-editor:")
                    .filter(|editor| *editor != "none")
                {
                    self.open_in_external_editor(editor, &path, cx);
                } else if action.starts_with("mute:") {
                    self.run_project_notification_menu(&path, action, window, cx);
                }
            }
        }
    }
    fn close_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_open = None;
        self.preview_close = None;
        if self.session_preview.take().is_some() {
            self.sidebar_frame.update(cx, |_, cx| cx.notify());
        }
    }
    fn sidebar_scrolled(&mut self, cx: &mut Context<Self>) {
        self.sidebar_last_scroll = Some(std::time::Instant::now());
        self.close_preview(cx);
        if let Some(id) = self.session_hover
            && let Some(anchor) = self.session_row_bounds.get(&id).cloned()
        {
            self.schedule_preview(id, anchor, cx);
        }
    }
    fn schedule_preview(&mut self, id: i64, anchor: AnchorSlot, cx: &mut Context<Self>) {
        if self.menu.is_some() || self.classic_worktree_picker.is_some() {
            return;
        }
        let settle = std::time::Duration::from_millis(180);
        let delay = self
            .sidebar_last_scroll
            .map(|last| settle.saturating_sub(last.elapsed()))
            .unwrap_or_default()
            + std::time::Duration::from_millis(120);
        self.preview_open = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let Ok(Some(store)) = this.update_in(cx, |this, window, cx| {
                if this.session_hover != Some(id)
                    || this.menu.is_some()
                    || !this.session_summaries.contains_key(&id)
                    || !anchor
                        .get()
                        .is_some_and(|bounds| bounds.contains(&window.mouse_position()))
                {
                    return None;
                }
                this.session_preview = Some(SessionPreview {
                    id,
                    data: None,
                    anchor,
                    surface: AnchorSlot::default(),
                });
                this.sidebar_frame.update(cx, |_, cx| cx.notify());
                // An already-open conversation has its own live metadata.
                (!this.sessions.contains_key(&id)).then(|| this.store.clone())
            }) else {
                return;
            };
            let data = cx
                .background_executor()
                .spawn(async move { locked(&store).and_then(|store| store.session_preview(id)) })
                .await
                .ok();
            let _ = this.update(cx, |this, cx| {
                if let Some(preview) = this
                    .session_preview
                    .as_mut()
                    .filter(|preview| preview.id == id)
                {
                    preview.data = data;
                    this.sidebar_frame.update(cx, |_, cx| cx.notify());
                }
            });
        }));
    }
    fn hover_session(
        &mut self,
        id: i64,
        anchor: AnchorSlot,
        hovered: bool,
        compact: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.deleting.contains(&id) {
            return;
        }
        let previous = self.session_hover;
        if hovered {
            self.session_hover = Some(id);
        } else if self.session_hover == Some(id) {
            self.session_hover = None;
        }
        if previous != self.session_hover {
            self.sidebar_frame.update(cx, |_, cx| cx.notify());
        }
        if hovered && compact {
            self.preview_close = None;
            if self
                .session_preview
                .as_ref()
                .is_none_or(|preview| preview.id != id)
            {
                self.close_preview(cx);
                self.schedule_preview(id, anchor, cx);
            }
        } else {
            // A row's leave can arrive after the next row's enter. Do not
            // cancel that newer hover, or metadata loading for an open card.
            if self.session_hover.is_none()
                && self
                    .session_preview
                    .as_ref()
                    .is_none_or(|preview| preview.id != id)
            {
                self.preview_open = None;
            }
            if self
                .session_preview
                .as_ref()
                .is_some_and(|preview| preview.id == id)
            {
                self.schedule_preview_close(window, cx);
            }
        }
    }
    fn schedule_preview_close(&mut self, window: &Window, cx: &mut Context<Self>) {
        // During the entrance the card can already cover the gap beside its
        // row. The row's leave event must not undo the card's enter event.
        if self.session_preview.as_ref().is_some_and(|preview| {
            preview
                .surface
                .get()
                .is_some_and(|bounds| bounds.contains(&window.mouse_position()))
        }) {
            self.preview_close = None;
            return;
        }
        self.preview_close = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let _ = this.update(cx, |this, cx| this.close_preview(cx));
        }));
    }
    fn session_row(&self, id: i64, cx: &mut Context<Self>) -> AnyElement {
        self.session_row_with(id, false, None, cx)
    }
    /// SessionCard, dense: `nested` for a session on another's rail; `rail`
    /// for a thread with one under it, and whether it is open.
    fn session_row_with(
        &self,
        id: i64,
        nested: bool,
        rail: Option<(&rails::Rail, bool)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(row) = self.rename_row(&session_groups::RenameTarget::Session(id), cx) {
            return row;
        }
        // A collapsed thread shows a chevron in its mark's place on hover:
        // selecting it opens the rail below.
        let expandable = rail.is_some_and(|(_, open)| !open);
        let mode = rail.map(|(rail, _)| match rail {
            rails::Rail::Agents(summary) => (
                "share",
                // fuchsia-200/90
                gpui::Hsla::from(rgb(0xf5d0fe)).opacity(0.9),
                format!(
                    "Orchestrator, {} {}",
                    summary.tasks.len(),
                    if summary.tasks.len() == 1 {
                        "agent"
                    } else {
                        "agents"
                    }
                ),
            ),
            rails::Rail::Started(children) => (
                "cursor-magic",
                // sky-200/90
                gpui::Hsla::from(rgb(0xbae6fd)).opacity(0.9),
                format!(
                    "Operator, started {} {}",
                    children.len(),
                    if children.len() == 1 {
                        "session"
                    } else {
                        "sessions"
                    }
                ),
            ),
        });
        // startedInView: one of its sessions is the one in view or chosen,
        // so the thread keeps its hover fill as the group's head.
        let started_in_view = match rail {
            Some((rails::Rail::Started(children), _)) => children.iter().any(|c| {
                self.selected_sessions.contains(c)
                    || (self.workspace.active == Some(*c)
                        && !self.mono_active(cx)
                        && !self.app_view_open())
            }),
            _ => false,
        };
        let compact = nested || self.workspace.compact_session_cards;
        let side = if nested || !compact { 14. } else { 16. };
        if let Some(mark) = self.session_marks.get(&id) {
            mark.update(cx, |mark, cx| mark.set_size(side, cx));
        }
        let work_item = self.work_item_badge(id, compact, cx);
        let has_badges = work_item.is_some()
            || mode.is_some()
            || self.automation_state.session_automations.contains_key(&id);
        let session = self.session_metadata(id, cx);
        let active = self.selected_sessions.contains(&id)
            || (self.workspace.active == Some(id)
                && !self.mono_active(cx)
                && !self.app_view_open());
        let anchor = self
            .session_row_bounds
            .get(&id)
            .cloned()
            .unwrap_or_default();
        self.sidebar_list.track_anchor(anchor.clone());
        let draft = session.has_saved_draft();
        let (model, branch, workspace_label) = if compact {
            (SharedString::default(), None, String::new())
        } else {
            let saved = self.session_details.get(&id);
            let live = self
                .sessions
                .get(&id)
                .and_then(|view| view.read(cx).sidebar_details());
            let (model, branch) = live.unwrap_or_else(|| {
                (
                    crate::pickers::cached_model_name(
                        Kind::from_id(&session.summary.provider),
                        saved.and_then(|data| data.model.as_deref()),
                    ),
                    None,
                )
            });
            let cwd = self
                .session_worktrees
                .get(&id)
                .and_then(|cwd| cwd.as_deref())
                .unwrap_or(&session.summary.cwd);
            let branch = branch
                .or_else(|| saved.and_then(|data| data.branch.clone()))
                .or_else(|| {
                    let git = self.git.read(cx);
                    (git.root == cwd)
                        .then(|| git.index.as_ref().and_then(|index| index.branch.clone()))
                        .flatten()
                });
            let directory = std::path::Path::new(cwd)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(cwd);
            let workspace_label = branch.clone().unwrap_or_else(|| directory.to_owned());
            (model, branch, workspace_label)
        };
        let pinned = session.summary.pinned;
        let mark = {
            let mark = match self.session_marks.get(&id) {
                Some(mark) if self.sidebar_uncached() => div()
                    .size(px(side))
                    .flex_shrink_0()
                    .child(mark.clone())
                    .into_any_element(),
                Some(mark) => mark
                    .clone()
                    .cached(StyleRefinement::default().size(px(side)).flex_shrink_0())
                    .into_any_element(),
                None => provider_icon(&session.summary.provider, side).into_any_element(),
            };
            div()
                .relative()
                .size(px(side))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .when(nested, |d| d.overflow_hidden())
                .child(
                    div()
                        .size_full()
                        .when(expandable, |d| {
                            d.group_hover("session-row", |s| s.opacity(0.))
                        })
                        .child(mark),
                )
                .when(expandable, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.)
                            .group_hover("session-row", |s| s.opacity(1.))
                            .child(icon("chevron-down", 14.).text_color(content(0.6))),
                    )
                })
        };
        let title = div()
            .debug_selector(move || format!("session-title-{id}"))
            .flex_1()
            .min_w_0()
            .h(px(20.))
            .when(nested, |d| {
                d.text_size(px(13.))
                    .text_color(content(if active { 1. } else { 0.55 }))
                    .when(!active, |d| {
                        d.group_hover("session-row", |s| s.text_color(content(0.8)))
                    })
            })
            .when(!nested && compact, |d| {
                d.text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
            })
            .when(!compact, |d| {
                d.text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(1.))
            })
            .child(crate::particle_text::particle_text(session.display_title()));
        let badges = div()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .children(work_item)
            .when(
                self.automation_state.session_automations.contains_key(&id),
                |row| row.child(icon("zap", 12.).text_color(content(0.35))),
            )
            // Marked on the right like a linked issue or PR, in its mode's colour.
            .when_some(mode, |d, (glyph, color, label)| {
                d.child(
                    div()
                        .id(("session-mode", id as usize))
                        .role(Role::Image)
                        .aria_label(label)
                        .size(px(16.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph, 12.).text_color(color)),
                )
            });
        let suffix = self.session_suffix(id, &session, compact, cx);
        let body = if compact {
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(if nested { 6. } else { 8. }))
                .child(mark)
                .child(title)
                .when(has_badges, |d| d.child(badges))
                .child(suffix)
                .into_any_element()
        } else {
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(pinned, |d| {
                            d.child(icon("pin", 12.).text_color(content(0.45)))
                        })
                        .child(title)
                        .child(suffix),
                )
                .child(
                    div()
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child(icon(
                                    if branch.is_some() {
                                        "git-branch"
                                    } else {
                                        "folder"
                                    },
                                    12.,
                                ))
                                .child(
                                    div()
                                        .debug_selector(move || format!("session-workspace-{id}"))
                                        .min_w_0()
                                        .truncate()
                                        .child(workspace_label),
                                ),
                        )
                        .when(has_badges, |d| d.child(badges))
                        .child(
                            div()
                                .flex_shrink_0()
                                .min_w_0()
                                .max_w(gpui::relative(0.5))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .child(
                                    div()
                                        .debug_selector(move || format!("session-model-{id}"))
                                        .min_w_0()
                                        .truncate()
                                        .child(model),
                                )
                                .child(provider_icon(&session.summary.provider, 14.)),
                        ),
                )
                .into_any_element()
        };
        div()
            .id(("sidebar-session", id as usize))
            .debug_selector(move || format!("sidebar-session-{id}"))
            .group("session-row")
            .role(Role::Button)
            .aria_label(session.display_title())
            .aria_description(format!(
                "{} conversation",
                Kind::from_id(&session.summary.provider).label()
            ))
            .cursor_pointer()
            // A nested row's `h-5` line and `px-1.5`, inside the card border.
            .h(px(if compact {
                if nested { 28. } else { 32. }
            } else if nested {
                56.
            } else {
                60.
            }))
            // Sidebar.tsx's `px-2` inside its 1px card border.
            .px(px(if nested { 7. } else { 9. }))
            .rounded_md()
            // The border is dashed for a saved draft. It overlays the row, so
            // the hover anchor still spans the whole card.
            .when(draft, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded_md()
                        .border_1()
                        .border_dashed()
                        .border_color(content(if active { 0.3 } else { 0.25 })),
                )
            })
            .flex()
            .items_center()
            .when(active, |d| d.bg(content(0.1)))
            .when(started_in_view && !active, |d| d.bg(content(0.05)))
            .hover(|s| s.bg(content(if active { 0.1 } else { 0.05 })))
            .when(
                self.sidebar_drop == Some(session_groups::ListDrop::Session(id)),
                |d| d.bg(content(0.14)),
            )
            .when(self.deleting.contains(&id), |d| d.opacity(0.5))
            .child(body)
            .child(anchor.probe())
            .relative()
            .on_hover(cx.listener(move |this, hovered, window, cx| {
                this.hover_session(id, anchor.clone(), *hovered, compact, window, cx);
            }))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if std::mem::take(&mut this.skip_session_click) {
                        return;
                    }
                    this.select_sidebar_session(id, event.modifiers(), window, cx)
                }),
            )
            // Sidebar.tsx: a session card dragged onto a pane edge opens there.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                    this.skip_session_click = false;
                    this.tab_drag = None;
                    this.pane_drag = Some(PaneDrag {
                        source: DragSource::Session(id),
                        start: event.position,
                        position: event.position,
                        active: false,
                        over: None,
                        insert: None,
                    });
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let anchor = Anchor::Point(event.position);
                    this.context_selection = !this.selected_sessions.contains(&id);
                    if this.context_selection {
                        this.selected_sessions.clear();
                        this.selected_sessions.insert(id);
                    }
                    this.open_menu(MenuTarget::Session(id), anchor, window, cx);
                }),
            )
            .into_any()
    }

    fn session_archive_button(
        &self,
        id: i64,
        session: &session_loading::Metadata<'_>,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id(("archive-session", id as usize))
            .debug_selector(move || format!("archive-session-{id}"))
            .role(Role::Button)
            .aria_label(format!(
                "{} {}",
                if session.summary.archived {
                    "Unarchive"
                } else {
                    "Archive"
                },
                session.display_title()
            ))
            .group("session-archive")
            .size(px(20.))
            .mr(px(-3.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .opacity(if self.archiving.contains(&id) {
                0.4
            } else {
                1.
            })
            .hover(|s| s.bg(content(0.1)))
            .child(
                icon("archive", 12.).group_hover("session-archive", |s| s.text_color(content(1.))),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_archive(id, cx);
            }))
            .into_any()
    }

    fn session_suffix(
        &self,
        id: i64,
        session: &session_loading::Metadata<'_>,
        compact: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let detailed = !compact;
        if self.session_hover == Some(id) {
            return self.session_archive_button(id, session, cx);
        }
        div()
            .flex_shrink_0()
            .text_size(px(11.))
            .line_height(px(16.))
            .font_weight(FontWeight::NORMAL)
            .text_color(content(0.4))
            .child(if session.needs_approval() {
                div()
                    .id(("session-approval", id as usize))
                    .role(Role::Image)
                    .aria_label("Needs approval")
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(rgb(0xfbbf24))
                    .child(icon("circle-alert", 14.))
                    .when(detailed, |d| d.child("Needs approval"))
                    .into_any()
            } else if detailed && session.responding() {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(content(0.5))
                    .children(self.session_marks.get(&id).cloned())
                    .child("Working…")
                    .into_any()
            } else if detailed && self.unseen_finished.contains(&id) {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(rgb(0x2dd4bf))
                    .child(icon("check-circle-2", 14.).text_color(rgb(0x2dd4bf)))
                    .child("Done")
                    .into_any()
            } else if session.has_saved_draft() {
                div()
                    .id(("session-draft", id as usize))
                    .role(Role::Image)
                    .aria_label("Draft")
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(icon("circle-dashed-hi", 14.).text_color(content(0.4)))
                    .when(detailed, |d| d.child("Draft"))
                    .into_any()
            } else if let Some(reminder) = self.reminders.iter().find(|r| r.session_id == id) {
                div()
                    .id(("session-reminder", id as usize))
                    .aria_label(local_reminder_label(reminder.due_at))
                    .text_color(rgb(0xf59e0b))
                    .child(session_groups::reminder_caption(reminder.due_at))
                    .into_any()
            } else {
                div()
                    .child(monocode_core::time::relative(
                        session.summary.updated_at,
                        monocode_core::time::now_ms(),
                    ))
                    .into_any()
            })
            .into_any()
    }

    /// The project's sessions as the sidebar lists them, top to bottom.
    fn sidebar_session_ids(&self, cx: &App) -> Vec<i64> {
        let start = self
            .workspace
            .session_filters
            .time
            .start(monocode_core::time::now_ms());
        // No selected worktree means the whole project, as in the desktop
        // sidebar. Tab restoration still uses the stricter execution scope.
        let listable = |id: i64| {
            let s = self.session_metadata(id, cx);
            !self
                .monos
                .as_ref()
                .is_some_and(|m| m.read(cx).hidden_session(id))
                && s.summary.cwd == self.workspace.project
                && s.summary.has_messages
                && !self.orchestration_worker(id)
                && self.in_sidebar_worktree_scope(id, cx)
        };
        let shown: HashSet<_> = self
            .order
            .iter()
            .copied()
            .filter(|id| !self.sidebar_hidden_sessions.contains(id) && listable(*id))
            .collect();
        let mut ids: Vec<_> = self
            .order
            .iter()
            .copied()
            .filter(|id| {
                let s = self.session_metadata(*id, cx);
                listable(*id)
                    && (!self.sidebar_hidden_sessions.contains(id)
                        || self
                            .started_by
                            .get(id)
                            .is_some_and(|caller| shown.contains(caller)))
                    && self.filter_session(*id, start, cx)
                    && (self.query.is_empty()
                        || if self.workspace.classic_layout {
                            self.classic_session_matches_query(*id, cx)
                        } else {
                            format!("{} {}", s.summary.title, s.summary.provider)
                                .to_lowercase()
                                .contains(&self.query)
                        })
            })
            .collect();
        ids.sort_by_key(|id| {
            std::cmp::Reverse((self.session_metadata(*id, cx).summary.updated_at, *id))
        });
        ids
    }
    /// `Session: Previous/Next` opens the sidebar's neighbour of the active
    /// session; `in_tab` swaps it into the current tab instead.
    fn step_session(
        &mut self,
        step: isize,
        in_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.is_some() {
            return;
        }
        let ids = self.sidebar_navigation_ids(cx);
        let current = self.workspace.active;
        let Some(next) = adjacent(&ids, current.as_ref(), step) else {
            return;
        };
        if Some(next) == current {
            return;
        }
        if in_tab
            && self.workspace.tab_of(next).is_none()
            && let Some(active) = current
        {
            let previous_tab = self.workspace.tab_of(active);
            self.workspace.swap_session(active, next);
            if let Some((from, to)) = previous_tab.zip(self.workspace.tab_of(next)) {
                self.replace_title_tab_key(from, to);
            }
        }
        self.open(next, window, cx);
    }
    /// `Project: Move Back/Forward` moves the current project one place.
    fn move_current_project(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            return;
        }
        let path = self.workspace.project.clone();
        if self.workspace.step_project_order(&path, step) {
            self.project_order_changed(&path, cx);
        }
    }
    /// Saves a new project order and keeps the moved project in view.
    fn project_order_changed(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.workspace.projects.iter().position(|p| p == path) {
            if ix < self.spaces_start {
                self.spaces_start = ix;
            } else if ix >= self.spaces_start + 5 {
                self.spaces_start = ix - 4;
            }
        }
        self.persist(cx);
        self.invalidate_chrome(cx);
        self.projects_frame.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
    /// `Project: Previous/Next` along the project rail.
    fn step_project(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            return;
        }
        let projects = self.workspace.visible_project_order();
        if let Some(next) = adjacent(&projects, Some(&self.workspace.project), step)
            && next != self.workspace.project
        {
            self.select_project(next, window, cx);
        }
    }
    fn close_other_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active_title_tab(cx) else {
            return;
        };
        let tabs = self
            .visible_tabs(cx)
            .into_iter()
            .filter(|tab| *tab != active)
            .collect::<Vec<_>>();
        self.close_title_tabs(&tabs, Some(active), window, cx);
    }
    fn activate_tab(&mut self, slot: isize, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = self.visible_tabs(cx);
        let index = if slot < 0 {
            tabs.len().saturating_sub(1)
        } else {
            slot as usize
        };
        if let Some(tab) = tabs.get(index) {
            self.open_title_tab(tab, window, cx);
        }
    }
    fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(settings) = &self.settings {
            let section = settings.read(cx).section;
            let select = cx.weak_entity();
            let back = select.clone();
            return crate::settings::nav(
                section,
                move |section, _, cx| {
                    let _ = select.update(cx, |this, cx| this.select_settings(section, cx));
                },
                move |window, cx| {
                    let _ = back.update(cx, |this, cx| this.close_settings(window, cx));
                },
            );
        }
        if self.workspace.classic_layout {
            return self.classic_sidebar(cx);
        }
        self.refresh_sidebar_structure(cx);
        let live_agents = self.live_agents_preview(cx);
        div()
            .id("monocode-sidebar")
            .debug_selector(|| "sidebar-content".into())
            .track_focus(&self.sidebar_focus)
            .key_context("SessionSidebar")
            .on_action(
                cx.listener(|this, _: &session_groups::RenameSidebarItem, window, cx| {
                    this.rename_sidebar_item(window, cx)
                }),
            )
            .on_action(cx.listener(
                |this, _: &session_groups::DeleteSidebarSelection, window, cx| {
                    this.delete_sidebar_selection(window, cx)
                },
            ))
            .w(px(self.workspace.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div().flex_1().min_h_0().child(
                    div()
                        .relative()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(39.))
                                .flex_shrink_0()
                                .px_3()
                                .pb_px()
                                .relative()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div().flex_1().min_w_0().flex().items_center().child(
                                        div()
                                            .id("workspace-project")
                                            .debug_selector(|| "sidebar-project-button".into())
                                            .relative()
                                            .role(Role::Button)
                                            .aria_label(format!(
                                                "Switch project, {}",
                                                self.project_label(&self.workspace.project)
                                            ))
                                            .aria_description(self.workspace.project.clone())
                                            .aria_expanded(self.menu_open(&MenuTarget::Header))
                                            .cursor_pointer()
                                            .h(px(28.))
                                            // The whole picker shares Search's left edge.
                                            .px_1p5()
                                            .ml(px(-4.))
                                            .rounded_md()
                                            .max_w_full()
                                            .hover(|s| s.bg(content(0.08)))
                                            .when(self.menu_open(&MenuTarget::Header), |d| {
                                                d.bg(content(0.08))
                                            })
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(crate::project_settings::header_project_icon(
                                                &self.appearance(&self.workspace.project),
                                                &self.workspace.project,
                                            ))
                                            .child(
                                                div().min_w_0().truncate().child(
                                                    self.project_label(&self.workspace.project),
                                                ),
                                            )
                                            .when_some(
                                                self.worktree_focus.get(&self.workspace.project),
                                                |d, focus| {
                                                    d.child(
                                                        div()
                                                            .min_w_0()
                                                            .truncate()
                                                            .font_weight(FontWeight::NORMAL)
                                                            .text_color(content(0.45))
                                                            .child(
                                                                focus
                                                                    .branch
                                                                    .clone()
                                                                    .unwrap_or_else(|| {
                                                                        "detached".into()
                                                                    }),
                                                            ),
                                                    )
                                                },
                                            )
                                            .child(icon("chevron-down", 14.))
                                            .child(self.project_anchor.probe())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                if this.menu_open(&MenuTarget::Header) {
                                                    this.close_menu(cx);
                                                } else {
                                                    let anchor =
                                                        Anchor::Slot(this.project_anchor.clone());
                                                    this.open_menu(
                                                        MenuTarget::Header,
                                                        anchor,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                            })),
                                    ),
                                )
                                .child(
                                    icon_button(
                                        "sidebar-options",
                                        "Sidebar options",
                                        "more-horizontal",
                                    )
                                    .debug_selector(|| "sidebar-options".into())
                                    .aria_expanded(self.menu_open(&MenuTarget::SidebarOptions))
                                    .relative()
                                    .child(self.sidebar_options_anchor.probe())
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            if this.menu_open(&MenuTarget::SidebarOptions) {
                                                this.close_menu(cx);
                                            } else {
                                                this.open_menu(
                                                    MenuTarget::SidebarOptions,
                                                    Anchor::Slot(
                                                        this.sidebar_options_anchor.clone(),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        },
                                    )),
                                ),
                        )
                        .child(self.virtual_sidebar(cx))
                        .children(live_agents)
                        .child(self.app_update_footer(cx)),
                ),
            )
            .when(!self.workspace.classic_layout, |d| {
                d.child(self.project_spaces(cx))
            })
            .when_some(self.session_preview.as_ref(), |d, preview| {
                let id = preview.id;
                d.child(
                    popover("session-preview", Anchor::Slot(preview.anchor.clone()))
                        .side(Side::Right)
                        .gap(8.)
                        .width(260.)
                        .max_height(440.)
                        .track(preview.surface.clone())
                        .child(
                            div()
                                .id("session-preview-content")
                                .role(Role::Tooltip)
                                .max_h(px(438.))
                                .overflow_y_scroll()
                                .on_hover(cx.listener(move |this, hovered, window, cx| {
                                    if this
                                        .session_preview
                                        .as_ref()
                                        .is_none_or(|preview| preview.id != id)
                                    {
                                        return;
                                    }
                                    if *hovered {
                                        this.preview_close = None;
                                    } else {
                                        this.schedule_preview_close(window, cx);
                                    }
                                }))
                                .child(self.sessions.get(&preview.id).map_or_else(
                                    || {
                                        session_preview::render(
                                            self.session_metadata(preview.id, cx).summary,
                                            preview.data.as_ref(),
                                            self.unseen_finished.contains(&preview.id),
                                        )
                                    },
                                    |view| {
                                        view.read(cx)
                                            .preview(self.unseen_finished.contains(&preview.id))
                                    },
                                )),
                        ),
                )
            })
            .into_any()
    }

    fn project_spaces(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // Sidebar.tsx's ProjectSpacesBar: five equal spaces, paged by chevrons.
        let shown = self.workspace.projects.len().min(5);
        let last = self.workspace.projects.len().saturating_sub(shown);
        self.spaces_start = self.spaces_start.min(last);
        let first = self.spaces_start;
        let projects: Vec<_> = self
            .space_reorder
            .order(&self.workspace.projects)
            .into_iter()
            .enumerate()
            .skip(first)
            .take(shown)
            .collect();
        if self
            .hovered_space
            .as_ref()
            .is_some_and(|hovered| !projects.iter().any(|(_, path)| path == hovered))
        {
            self.hovered_space = None;
        }
        let prefs = &self.workspace.preferences;
        let shortcuts = ["Project: Previous", "Project: Next"]
            .into_iter()
            .filter_map(|command| crate::settings::command_shortcut(prefs, command))
            .collect::<Vec<_>>()
            .join(" / ");
        // A short slide in from the side the new spaces come from.
        let (nudge_x, nudge_opacity) = match self.spaces_nudge {
            Some((direction, start)) => {
                let t = start.elapsed().as_secs_f32() / 0.2;
                if t >= 1. {
                    self.spaces_nudge = None;
                    (0., 1.)
                } else {
                    let eased = crate::reveal::bezier(t, 0.32, 0.72, 0., 1.);
                    (direction * 12. * (1. - eased), 0.4 + 0.6 * eased)
                }
            }
            None => (0., 1.),
        };
        let tooltip = self.hovered_space.clone().map(|path| {
            popover(
                "project-space-tooltip",
                Anchor::Slot(self.space_anchor.clone()),
            )
            .side(Side::Top)
            .align(Align::Center)
            .gap(8.)
            .padding(8.)
            .without_animation()
            .allow_outside_press()
            .child(
                div()
                    .id("project-space-tip")
                    .role(Role::Tooltip)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(1.))
                            .child(self.project_label(&path)),
                    )
                    .when(!shortcuts.is_empty(), |d| {
                        d.child(div().text_color(content(0.55)).child(shortcuts.clone()))
                    }),
            )
        });
        div()
            .h(px(30.))
            .flex_shrink_0()
            .px_2()
            .pb_0p5()
            .flex()
            .items_center()
            .gap_1()
            .child(
                icon_button("previous-projects", "Previous projects", "chevron-left")
                    .size(px(20.))
                    .opacity(if first == 0 { 0.3 } else { 1. })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.spaces_start > 0 {
                            this.spaces_start -= 1;
                            this.nudge_spaces(-1., cx);
                        }
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .relative()
                    .left(px(nudge_x))
                    .opacity(nudge_opacity)
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.space_reorder.strip.probe())
                    .children(projects.into_iter().map(|(ix, path)| {
                        let active = path == self.workspace.project;
                        let hovered = self.hovered_space.as_ref() == Some(&path);
                        let dragging = self.space_reorder.dragging(&path);
                        let offset = self.space_reorder.offset(&path, ix, first);
                        div()
                            .id(("project-space", ix))
                            .debug_selector(move || format!("project-space-{ix}"))
                            .group("project-space")
                            .role(Role::Tab)
                            .aria_label(format!("Project {}", self.project_label(&path)))
                            .aria_description(path.clone())
                            .aria_selected(active)
                            .cursor_pointer()
                            .flex_1()
                            .min_w_0()
                            .h(px(20.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .relative()
                            .left(px(offset))
                            .opacity(if active || dragging { 1. } else { 0.45 })
                            .when(active || dragging, |d| d.bg(content(0.1)))
                            .when(dragging, |d| d.cursor(CursorStyle::ClosedHand))
                            .hover(|s| s.opacity(1.).bg(content(if active { 0.1 } else { 0.05 })))
                            .child(self.project_mark(&path, 12.))
                            .when(hovered, |d| d.child(self.space_anchor.probe()))
                            .on_hover({
                                let path = path.clone();
                                cx.listener(move |this, hovered: &bool, _, cx| {
                                    if *hovered {
                                        this.hovered_space = Some(path.clone());
                                    } else if this.hovered_space.as_ref() == Some(&path) {
                                        this.hovered_space = None;
                                    } else {
                                        return;
                                    }
                                    this.invalidate_chrome(cx);
                                })
                            })
                            .on_mouse_down(MouseButton::Left, {
                                let path = path.clone();
                                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                                    this.start_space_drag(&path, event);
                                })
                            })
                            .on_mouse_down(MouseButton::Right, {
                                let path = path.clone();
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    let anchor = Anchor::Point(event.position);
                                    // Spaces have no pins or groups to offer.
                                    let target = MenuTarget::Project {
                                        path: path.clone(),
                                        rail: false,
                                    };
                                    this.open_menu(target, anchor, window, cx);
                                })
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.space_reorder.take_click() {
                                    return;
                                }
                                // The tip steps aside once a space is clicked.
                                this.hovered_space = None;
                                this.select_project(path.clone(), window, cx)
                            }))
                    })),
            )
            .child(
                icon_button("more-projects", "More projects", "chevron-right")
                    .size(px(20.))
                    .opacity(if first >= last { 0.3 } else { 1. })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.spaces_start < last {
                            this.spaces_start += 1;
                            this.nudge_spaces(1., cx);
                        }
                    })),
            )
            .children(tooltip)
            .into_any()
    }

    fn nudge_spaces(&mut self, direction: f32, cx: &mut Context<Self>) {
        if !cx.reduce_motion() {
            self.spaces_nudge = Some((direction, std::time::Instant::now()));
        }
        self.invalidate_chrome(cx);
    }

    /// App.tsx's toTitleTab with TitleBar's tabCopy: a split tab, or a chat
    /// with a file beside it, heads with the focused pane and names the rest
    /// on a second line.
    fn tab_copy(&self, tab: i64, cx: &App) -> TabCopy {
        let focus = self.workspace.tab_focus(tab);
        let review = |id: &i64| {
            self.workspace
                .reviews
                .get(id)
                .filter(|r| !r.tabs.is_empty())
        };
        let mut leaves = self.workspace.layout(tab).leaf_ids();
        // Focused first, as harnesses and files are ordered.
        if let Some(ix) = leaves.iter().position(|id| *id == focus) {
            let id = leaves.remove(ix);
            leaves.insert(0, id);
        }
        // A chat closed beside its file leaves only the file pane.
        let sessions: Vec<i64> = leaves
            .iter()
            .copied()
            .filter(|id| !review(id).is_some_and(|r| r.chat_hidden))
            .filter(|id| self.session_summaries.contains_key(id))
            .collect();
        let mut files = Vec::new();
        for id in &leaves {
            if let Some(r) = review(id) {
                let mut labels = r.tabs.iter().map(|t| t.label());
                let active = r.tabs.get(r.active).map(|t| t.label());
                for label in active.into_iter().chain(&mut labels) {
                    if !files.contains(&label) {
                        files.push(label);
                    }
                }
            }
        }
        let file_panes = leaves.iter().filter(|id| review(id).is_some()).count();
        let mono_panes = leaves
            .iter()
            .filter(|id| self.workspace.mono_panes.contains_key(id))
            .count();
        let multi_pane = sessions.len() + file_panes + mono_panes > 1;
        let session_focused = sessions.first() == Some(&focus);
        let file_focused =
            !session_focused || (self.tab_review_focused(tab, cx) && review(&focus).is_some());
        let title = |id: &i64| {
            let title = self.session_metadata(*id, cx).display_title();
            if title == "New session" {
                String::new()
            } else {
                title
            }
        };
        let conversation = sessions.first().map(title).unwrap_or_default();
        let file = files.first().cloned().unwrap_or_default();
        let untitled = "New session".to_owned();
        let mono = self.mono_pane_title(focus, cx);
        let headline = if let Some(mono) = mono {
            mono
        } else if multi_pane && file_focused && !file.is_empty() {
            file
        } else {
            [conversation, file]
                .into_iter()
                .find(|t| !t.is_empty())
                .unwrap_or_else(|| untitled.clone())
        };
        // Layout order, a chat before the file beside it, so moving focus
        // right or down rolls the headline up.
        let mut panes = Vec::new();
        for id in self.workspace.layout(tab).leaf_ids() {
            if let (Some(title), Some(mark)) =
                (self.mono_pane_title(id, cx), self.mono_pane_mark(id, cx))
            {
                panes.push(TabPane {
                    key: (id, false),
                    title,
                    mark,
                });
            }
            if sessions.contains(&id) {
                let session = self.session_metadata(id, cx);
                let lit = session.responding()
                    || session.needs_approval()
                    || self.unseen_finished.contains(&id);
                let title = title(&id);
                panes.push(TabPane {
                    key: (id, false),
                    title: if title.is_empty() {
                        untitled.clone()
                    } else {
                        title
                    },
                    mark: PaneMark::Session(session.summary.provider.clone(), lit.then_some(id)),
                });
            }
            if let Some(r) = review(&id)
                && let Some(t) = r.tabs.get(r.active)
            {
                panes.push(TabPane {
                    key: (id, true),
                    title: t.label(),
                    mark: PaneMark::File(
                        matches!(t, crate::review::ReviewTab::File { .. }).then(|| t.label()),
                    ),
                });
            }
        }
        let focused = (focus, file_focused && review(&focus).is_some());
        let focus = panes.iter().position(|p| p.key == focused).unwrap_or(0);
        TabCopy {
            headline,
            panes,
            focus,
        }
    }
    /// A pane's mark: its provider, spinning while it works and checked when
    /// it finished unseen, or the file's icon.
    fn pane_mark(&self, mark: &PaneMark, active: bool) -> AnyElement {
        let ink = content(if active { 1. } else { 0.5 });
        let mark = match mark {
            PaneMark::Session(provider, lit) => match lit.and_then(|id| self.tab_marks.get(&id)) {
                Some(mark) => mark
                    .clone()
                    .cached(StyleRefinement::default().size(px(14.)))
                    .into_any_element(),
                None => provider_icon(provider, 14.)
                    .opacity(if active { 1. } else { 0.55 })
                    .into_any_element(),
            },
            PaneMark::File(Some(name)) => {
                crate::file_icons::file_type_icon(name, false, false, 14.)
            }
            PaneMark::File(None) => icon("git-compare", 14.).text_color(ink).into_any_element(),
            PaneMark::Mono(mascot, color) => div()
                .opacity(if active { 1. } else { 0.55 })
                .child(crate::monos::avatar::avatar(
                    mascot, *color, "idle", 14., true,
                ))
                .into_any_element(),
        };
        div()
            .size(px(14.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(mark)
            .into_any_element()
    }
    /// The focused pane's mark and title on one line; while focus moves,
    /// the line leaving rolls out as the new one rolls in behind it.
    fn pane_headline(
        &self,
        copy: &TabCopy,
        (leaving, dir, progress): (Option<usize>, f32, f32),
        active: bool,
        hovered: bool,
    ) -> AnyElement {
        const LINE: f32 = 20.;
        let line = |pane: &TabPane, top: f32, opacity: f32| {
            div()
                .absolute()
                .left_0()
                .top(px(top))
                .w_full()
                .h(px(LINE))
                .opacity(opacity)
                .flex()
                .items_center()
                .gap(px(6.))
                .child(self.pane_mark(&pane.mark, active))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(crate::tab_label::tab_label(pane.title.clone())),
                )
        };
        div()
            .flex_1()
            .min_w_0()
            .h(px(LINE))
            .relative()
            .overflow_hidden()
            .text_size(px(TITLE_TAB_TEXT_SIZE))
            .text_color(content(if active || hovered { 1. } else { 0.5 }))
            .group_hover("title-tab", |s| s.text_color(content(1.)))
            .when_some(copy.panes.get(copy.focus), |d, pane| {
                d.child(line(pane, dir * (1. - progress) * LINE, progress.max(0.2)))
            })
            .when_some(leaving.and_then(|ix| copy.panes.get(ix)), |d, pane| {
                d.child(line(pane, -dir * progress * LINE, 1. - progress))
            })
            .into_any_element()
    }
    /// Where tab `tab`'s headline is in its roll to the focused pane: the
    /// pane leaving, its direction, and the eased progress.
    fn pane_tick(&mut self, tab: i64, copy: &TabCopy, animate: bool) -> (Option<usize>, f32, f32) {
        let Some(shown) = copy.panes.get(copy.focus).map(|p| p.key) else {
            self.pane_ticks.remove(&tab);
            return (None, 0., 1.);
        };
        let now = std::time::Instant::now();
        let position = |key: (i64, bool)| copy.panes.iter().position(|p| p.key == key);
        let tick = self.pane_ticks.entry(tab).or_insert(PaneTick {
            shown,
            from: None,
            dir: 0.,
            start: now,
        });
        if tick.shown != shown {
            let from = position(tick.shown).filter(|_| animate);
            tick.dir = if from.is_some_and(|ix| ix > copy.focus) {
                -1.
            } else {
                1.
            };
            tick.from = from.map(|_| tick.shown);
            tick.shown = shown;
            tick.start = now;
        }
        if !animate {
            tick.from = None;
        }
        let t = now.saturating_duration_since(tick.start).as_secs_f32() / PANE_TICK_DURATION;
        let from = tick.from.and_then(position).filter(|_| t < 1.);
        if from.is_none() {
            tick.from = None;
            return (None, 0., 1.);
        }
        (from, tick.dir, crate::reveal::bezier(t, 0.22, 1., 0.36, 1.))
    }
    /// The dragged session's title, following the pointer.
    fn drag_ghost(&self, cx: &App) -> Option<AnyElement> {
        let drag = self.pane_drag.as_ref().filter(|d| d.active)?;
        let id = drag.source.session();
        let review = (matches!(drag.source, DragSource::Review(_))
            || self.workspace.review_owners.contains_key(&id))
        .then(|| self.workspace.reviews.get(&id))
        .flatten()
        .and_then(|r| r.tabs.get(r.active));
        let owner = self.workspace.session_for_pane(id);
        self.session_summaries.get(&owner)?;
        let session = self.session_metadata(owner, cx);
        let title = if let Some(tab) = review {
            tab.label()
        } else if self.workspace.review_owners.contains_key(&id) {
            self.tab_copy(id, cx).headline
        } else {
            session.display_title()
        };
        Some(
            div()
                .absolute()
                .left(drag.position.x + px(14.))
                .top(drag.position.y + px(10.))
                .max_w(px(240.))
                .h(px(28.))
                .px_2()
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(pane_fill())
                .shadow_md()
                .text_size(px(12.))
                .text_color(content(1.))
                .child(if let Some(tab) = review {
                    if matches!(tab, crate::review::ReviewTab::File { .. }) {
                        crate::file_icons::file_type_icon(&tab.label(), false, false, 14.)
                    } else {
                        icon("git-compare", 14.).into_any_element()
                    }
                } else {
                    provider_icon(&session.summary.provider, 14.).into_any_element()
                })
                .child(div().min_w_0().truncate().child(title))
                .into_any_element(),
        )
    }
    fn tab_can_close(&self, id: i64, cx: &App) -> bool {
        let Some(tab) = self.workspace.tab_of(id) else {
            return false;
        };
        let layout = self.workspace.layout(tab);
        self.visible_tabs(cx).len() > 1
            || layout.is_split()
            || layout.leaf_ids().iter().any(|id| {
                self.session_summaries.contains_key(id) && {
                    let s = self.session_metadata(*id, cx);
                    s.summary.has_messages
                        || s.busy
                        || s.has_saved_draft()
                        || self
                            .sessions
                            .get(id)
                            .is_some_and(|v| !v.read(cx).input.read(cx).content.is_empty())
                } || self
                    .workspace
                    .reviews
                    .get(id)
                    .is_some_and(|r| !r.tabs.is_empty())
            })
    }
    fn start_tab_drag(&mut self, id: TitleTab, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.skip_tab_click = false;
        self.close_menu(cx);
        let tabs = self.visible_tabs(cx);
        let Some(from) = tabs.iter().position(|tab| *tab == id) else {
            return;
        };
        if tabs.len() < 2 {
            return;
        }
        let bounds: Vec<_> = (0..tabs.len())
            .filter_map(|ix| self.tab_scroll.bounds_for_item(ix))
            .collect();
        if bounds.len() != tabs.len() {
            return;
        }
        self.tab_drag = Some(TabDrag {
            id,
            from,
            to: from,
            origin: event.position.x,
            offset: px(0.),
            active: false,
            bounds,
            armed: false,
        });
        self.tabs_frame.update(cx, |_, cx| cx.notify());
    }
    fn move_tab_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.tab_drag.as_mut() else {
            return;
        };
        // A press whose release never reached the shell (the title bar can
        // swallow it) must not wake up on the next drag elsewhere.
        if event.pressed_button != Some(MouseButton::Left) {
            self.tab_drag = None;
            self.cancel_pane_drag(cx);
            self.tabs_frame.update(cx, |_, cx| cx.notify());
            return;
        }
        let delta = event.position.x - drag.origin;
        let pulled = event.position.y.as_f32() - TITLE_BAR_HEIGHT;
        if delta.abs() < px(4.) && !drag.active && pulled < 4. {
            return;
        }
        drag.active = true;
        self.skip_tab_click = true;
        // Once armed it stays armed until the pointer is back over the tabs.
        if pulled <= 0. {
            drag.armed = false;
        } else if pulled >= PANE_DROP_PULL {
            drag.armed = true;
        }
        if drag.armed {
            // Pulled out of the strip: the tab settles back into its slot
            // and the strip holds still while the drag targets a pane.
            drag.offset = px(0.);
            drag.to = drag.from;
        } else {
            let start = drag.bounds[drag.from].left();
            drag.offset = delta.clamp(
                drag.bounds[0].left() - start,
                drag.bounds.last().unwrap().left() - start,
            );
            drag.to = drag
                .bounds
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (a.left() - start - drag.offset)
                        .abs()
                        .partial_cmp(&(b.left() - start - drag.offset).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(ix, _)| ix)
                .unwrap_or(drag.from);
        }
        let (id, armed) = (drag.id.clone(), drag.armed);
        // The active tab has no other pane to land on, but its title still
        // follows the pointer.
        let own = self.active_title_tab(cx).as_ref() == Some(&id);
        let source = match &id {
            TitleTab::Session(tab) => Some(DragSource::Tab(*tab)),
            TitleTab::Mono { .. } => Some(DragSource::MonoTab),
            TitleTab::Search | TitleTab::App(_) => None,
        };
        let was = self.pane_drag.is_some();
        self.pane_drag = source.filter(|_| armed).map(|source| PaneDrag {
            source,
            start: event.position,
            position: event.position,
            active: true,
            over: if own {
                None
            } else {
                self.pane_hit(event.position, cx)
            },
            insert: None,
        });
        if armed || was {
            cx.notify();
        }
        self.tabs_frame.update(cx, |_, cx| cx.notify());
    }
    fn finish_tab_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(drag) = self.tab_drag.take() {
            let onto_pane = self.pane_drag.as_ref().is_some_and(|d| {
                d.over.is_some()
                    && match d.source {
                        DragSource::Tab(tab) => drag.id.session() == Some(tab),
                        DragSource::MonoTab => matches!(drag.id, TitleTab::Mono { .. }),
                        _ => false,
                    }
            });
            if onto_pane {
                self.skip_tab_click = true;
                if matches!(drag.id, TitleTab::Mono { .. }) {
                    self.drop_mono_tab(drag.id, window, cx);
                } else {
                    self.finish_pane_drag(window, cx);
                }
            } else if drag.active {
                self.cancel_pane_drag(cx);
                // Each tab glides from where it is drawn into its new slot.
                let now = std::time::Instant::now();
                let visible = self.visible_tabs(cx);
                let drawn: Vec<(TitleTab, f32)> = visible
                    .iter()
                    .enumerate()
                    .map(|(ix, id)| {
                        let shift = self.tab_shifts.get(id).map_or(0., |s| s.at(now).0);
                        (
                            id.clone(),
                            drag.bounds.get(ix).map_or(0., |b| b.left().as_f32()) + shift,
                        )
                    })
                    .collect();
                self.reorder_title_tab(&drag.id, drag.to, cx);
                self.tab_shifts.clear();
                if !cx.reduce_motion() {
                    for (ix, id) in self.visible_tabs(cx).iter().enumerate() {
                        let (Some((_, left)), Some(slot)) =
                            (drawn.iter().find(|(t, _)| t == id), drag.bounds.get(ix))
                        else {
                            continue;
                        };
                        let from = left - slot.left().as_f32();
                        if from.abs() > 0.5 {
                            self.tab_shifts.insert(
                                id.clone(),
                                TabShift {
                                    from,
                                    to: 0.,
                                    start: now,
                                },
                            );
                        }
                    }
                }
                self.skip_tab_click = true;
                self.persist(cx);
            }
            self.tabs_frame.update(cx, |_, cx| cx.notify());
        }
    }
    /// Where tab `id` is drawn now, easing toward `target`.
    fn eased_shift(
        &mut self,
        id: TitleTab,
        target: f32,
        follow: bool,
        reduced: bool,
    ) -> (f32, bool) {
        let now = std::time::Instant::now();
        // The dragged tab rides the pointer; it eases only into a slot.
        if follow || reduced {
            self.tab_shifts.remove(&id);
            if follow {
                self.tab_shifts.insert(
                    id.clone(),
                    TabShift {
                        from: target,
                        to: target,
                        start: now - std::time::Duration::from_secs(1),
                    },
                );
            }
            return (target, false);
        }
        let current = self.tab_shifts.get(&id).map(|s| s.at(now));
        if current.is_none_or(|_| self.tab_shifts[&id].to != target) {
            let from = current.map_or(0., |(value, _)| value);
            if from == target {
                self.tab_shifts.remove(&id);
                return (target, false);
            }
            self.tab_shifts.insert(
                id.clone(),
                TabShift {
                    from,
                    to: target,
                    start: now,
                },
            );
        }
        let (value, done) = self.tab_shifts[&id].at(now);
        if done && target == 0. {
            self.tab_shifts.remove(&id);
        }
        (value, !done)
    }
    fn tab_shift(&self, ix: usize) -> Pixels {
        let Some(d) = self.tab_drag.as_ref().filter(|d| d.active) else {
            return px(0.);
        };
        if ix == d.from {
            return d.offset;
        }
        if d.from < d.to && ix > d.from && ix <= d.to {
            return d.bounds[ix - 1].left() - d.bounds[ix].left();
        }
        if d.from > d.to && ix >= d.to && ix < d.from {
            return d.bounds[ix + 1].left() - d.bounds[ix].left();
        }
        px(0.)
    }
    /// The tab strip's width: between the navigation and TitleBar.tsx's right
    /// panel toggle, `pl-1.5 pr-3` clear of the edge.
    fn tabs_width(&self, window: &Window, cx: &App) -> f32 {
        let trailing = if self.workspace.boxy_layout() {
            self.right_drawn + self.strip_trail(cx)
        } else {
            self.toggle_room(cx)
        };
        // The classic strip starts beside the sidebar, after whatever the
        // navigation leaves uncovered.
        let lead = if self.workspace.boxy_layout() && !self.classic_titlebar_spans_window(cx) {
            self.left_sidebar_width(cx) + self.strip_lead(cx)
        } else {
            self.navigation_width()
        };
        (window.viewport_size().width.as_f32()
            - lead
            - trailing
            - (client_controls_width(window) - self.window_controls))
            .max(0.)
    }
    fn tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let viewport = self.tabs_width(window, cx);
        let tabs = self.visible_tabs(cx);
        let count = tabs.len();
        let insert = self.pane_drag.as_ref().and_then(|d| d.insert);
        let reduced = cx.reduce_motion();
        let animate = self.workspace.preferences.tab_animations
            && !reduced
            && !self.tab_drag.as_ref().is_some_and(|d| d.active);
        let (slots, opening) = self.title_tab_motion.sync(
            tabs.iter().map(|tab| (tab.clone(), ())).collect(),
            animate,
            std::time::Instant::now(),
        );
        // Every tab fits: they share the strip, however many there are. A tab
        // easing in or out holds its share of a slot, so the others give way
        // to it gradually, as flex-shrink does in TitleBar.tsx.
        let share: f32 = slots
            .iter()
            .map(|s| match s {
                crate::tab_motion::Slot::Live { open, .. }
                | crate::tab_motion::Slot::Closing { open, .. } => *open,
            })
            .sum();
        let avail = viewport - STRIP_PAD - STRIP_NEW_TAB - share * 2.;
        let width = if share <= 0. {
            TITLE_TAB_MAX_WIDTH
        } else {
            (avail / share).clamp(0., TITLE_TAB_MAX_WIDTH)
        };
        // The selected tab keeps a wider share; the others split the rest.
        let selected = self.active_title_tab(cx);
        let now = std::time::Instant::now();
        let selection_changed = self.title_tab_selection.1 != selected;
        if selection_changed {
            let previous = self.title_tab_selection.1.take();
            self.title_tab_selection = (previous.filter(|_| animate), selected.clone(), now);
        }
        let switch = crate::tab_motion::progress(self.title_tab_selection.2, now);
        if switch >= 1. {
            self.title_tab_selection.0 = None;
        }
        let leaving = self.title_tab_selection.0.clone();
        let switching = leaving.is_some();
        // How much of the selected tab's wider share each tab holds: the new
        // one eases in while the previous one eases out.
        let weight = |tab: &TitleTab| {
            if Some(tab) == selected.as_ref() {
                if switching { switch } else { 1. }
            } else if leaving.as_ref() == Some(tab) {
                1. - switch
            } else {
                0.
            }
        };
        let held: f32 = slots
            .iter()
            .map(|s| match s {
                crate::tab_motion::Slot::Live { key, open } => weight(key) * open,
                crate::tab_motion::Slot::Closing { .. } => 0.,
            })
            .sum();
        // The selected tab shrinks with the rest down to a floor, then holds
        // there while the others keep giving way.
        let (width, floor) = if width < SELECTED_TAB_MIN_WIDTH && held > 0. && share - held > 0. {
            let floor = SELECTED_TAB_MIN_WIDTH.min(avail.max(0.));
            (((avail - floor * held) / (share - held)).max(0.), floor)
        } else {
            (width, width)
        };
        // Fractional widths round differently from tab to tab and the error
        // adds up along the strip, so a full strip's new-tab button would
        // wander as tabs come and go. Every slot snaps to device pixels and
        // the button's margin takes what is left, keeping it still.
        let scale = window.scale_factor();
        let snap = |x: f32| (x * scale).floor() / scale;
        let crowded = width < TITLE_TAB_MAX_WIDTH;
        let base_width = |tab: &TitleTab| snap(width + (floor - width) * weight(tab));
        let slot_size = |tab: &TitleTab, open: f32| {
            (
                snap(base_width(tab) * open),
                snap(crate::tab_motion::GAP * (1. - open)),
            )
        };
        let used: f32 = slots
            .iter()
            .map(|s| match s {
                crate::tab_motion::Slot::Live { key, open }
                | crate::tab_motion::Slot::Closing { key, open, .. } => {
                    if *open >= 1. {
                        base_width(key) + 2.
                    } else {
                        let (w, give) = slot_size(key, *open);
                        w + 2. - give
                    }
                }
            })
            .sum();
        // The settled tabs take back what snapping left over, a device pixel
        // each from the left, so the new-tab button keeps its usual 2px gap.
        let mut extra: HashMap<TitleTab, f32> = HashMap::new();
        let mut new_tab_lead = 2.;
        if crowded {
            let leftover = (viewport - STRIP_PAD - STRIP_NEW_TAB - used).max(0.);
            let settled: Vec<&TitleTab> = slots
                .iter()
                .filter_map(|s| match s {
                    crate::tab_motion::Slot::Live { key, open } if *open >= 1. => Some(key),
                    _ => None,
                })
                .collect();
            let units = (leftover * scale).floor() as usize;
            if !settled.is_empty() {
                for i in 0..units {
                    *extra.entry(settled[i % settled.len()].clone()).or_default() += 1. / scale;
                }
                new_tab_lead += leftover - units as f32 / scale;
            } else {
                new_tab_lead += leftover;
            }
        }
        let tab_width = |tab: &TitleTab| base_width(tab) + extra.get(tab).copied().unwrap_or(0.);
        let dragged = self
            .tab_drag
            .as_ref()
            .filter(|d| d.active && !d.armed)
            .map(|d| d.id.clone());
        let mut moving = false;
        let shifts: Vec<Pixels> = tabs
            .clone()
            .into_iter()
            .enumerate()
            .map(|(ix, id)| {
                let target = self.tab_shift(ix).as_f32();
                let (value, active) =
                    self.eased_shift(id.clone(), target, dragged.as_ref() == Some(&id), reduced);
                moving |= active;
                px(value)
            })
            .collect();
        // Keep inactive headlines on their last pane. Only an in-tab focus
        // change rolls the title; selecting a tab settles it in the same frame.
        self.pane_ticks
            .retain(|id, _| self.workspace.tabs.contains(id));
        let mut copies = HashMap::new();
        for id in tabs.iter().filter_map(|tab| tab.session()) {
            if self
                .session_summaries
                .contains_key(&self.workspace.session_for_pane(id))
            {
                let copy = self.tab_copy(id, cx);
                let tick = self.pane_tick(
                    id,
                    &copy,
                    animate && !selection_changed && selected == Some(TitleTab::Session(id)),
                );
                moving |= tick.0.is_some();
                copies.insert(id, (copy, tick));
            }
        }
        let mut session_elements: HashMap<i64, AnyElement> = tabs
            .iter()
            .enumerate()
            .filter_map(|(ix, tab)| {
                tab.session().and_then(|id| {
                    self.session_summaries
                        .contains_key(&self.workspace.session_for_pane(id))
                        .then_some((ix, id))
                })
            })
            .filter_map(|(ix, id)| {
                let (copy, (leaving, dir, progress)) = copies.remove(&id)?;
                let active = self.workspace.active_tab() == Some(id)
                    && !self.app_view_open()
                    && !self.mono_active(cx)
                    && self.app_search.is_none();
                // The press itself redraws the strip to arm tab dragging.
                // Keep its ink stable until that press is released, even if
                // hover hitboxes are being replaced during that redraw.
                let hovered = self.tab_hover == Some(id)
                    || self
                        .tab_drag
                        .as_ref()
                        .is_some_and(|drag| drag.id == TitleTab::Session(id) && !drag.active);
                // A collapsed tab drops its close button, so a click anywhere
                // on it selects it; selected, it is wide enough to show it again.
                let close = self.tab_can_close(id, cx)
                    && (active || tab_width(&TitleTab::Session(id)) >= TAB_CLOSE_MIN_WIDTH);
                let dragging = self
                    .tab_drag
                    .as_ref()
                    .is_some_and(|d| d.id == TitleTab::Session(id) && d.active);
                let tab = div()
                    .id(("tab", id as usize))
                    .debug_selector(move || format!("title-tab-{id}"))
                    .group("title-tab")
                    .role(Role::Tab)
                    .aria_label(copy.headline.clone())
                    .aria_selected(active)
                    .cursor_pointer()
                    .w(px(tab_width(&TitleTab::Session(id))))
                    .flex_shrink_0()
                    .h(px(TITLE_BAR_HEIGHT))
                    .relative()
                    .left(shifts[ix])
                    .flex()
                    .items_center()
                    .child(
                        title_tab_plate(active, hovered)
                            .id(("tab-plate", id as usize))
                            .w_full()
                            .min_w_0()
                            // A narrow tab keeps its mark; the close button
                            // overlays it on hover instead of taking room.
                            .when(
                                close && tab_width(&TitleTab::Session(id)) >= TAB_CLOSE_MIN_WIDTH,
                                |d| d.pr_7(),
                            )
                            // Too narrow for the plate's padding: the mark sits centred.
                            .when(tab_width(&TitleTab::Session(id)) < 30., |d| {
                                d.px(px(((tab_width(&TitleTab::Session(id)) - 14.) / 2.).max(0.)))
                            })
                            .gap(px(6.))
                            .group_hover("title-tab", |s| s.bg(title_tab_fill(active, true)))
                            .child(self.pane_headline(
                                &copy,
                                (leaving, dir, progress),
                                active,
                                hovered,
                            )),
                    )
                    .when(close, |d| {
                        d.child(
                            div()
                                .id(("close-tab", id as usize))
                                .debug_selector(move || format!("close-title-tab-{id}"))
                                .group("tab-close")
                                .role(Role::Button)
                                .aria_label(format!("Close tab {}", copy.headline))
                                .absolute()
                                .right_1()
                                .top(px(10.))
                                .size(px(20.))
                                .rounded_sm()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(content(0.1)))
                                .when(copy.panes.len() > 1, |d| {
                                    d.child(
                                        div()
                                            .debug_selector(move || {
                                                format!("title-tab-pane-count-{id}")
                                            })
                                            .text_size(px(11.))
                                            .line_height(px(20.))
                                            .text_color(content(0.4))
                                            .group_hover("title-tab", |s| s.opacity(0.))
                                            .child(copy.panes.len().to_string()),
                                    )
                                })
                                .child(
                                    div()
                                        .absolute()
                                        .top_1()
                                        .left_1()
                                        .size(px(12.))
                                        .opacity(0.)
                                        .group_hover("title-tab", |s| s.opacity(1.))
                                        .child(icon("close", 12.).group_hover("tab-close", |s| {
                                            s.text_color(content(1.))
                                        })),
                                )
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close(id, window, cx);
                                })),
                        )
                    })
                    .map(|d| {
                        let bar = || {
                            div()
                                .absolute()
                                .top(px(9.))
                                .h(px(22.))
                                .w(px(2.))
                                .rounded_full()
                                .bg(accent_color())
                        };
                        d.when(insert == Some(ix), |d| d.child(bar().left(px(-2.))))
                            .when(insert == Some(count) && ix + 1 == count, |d| {
                                d.child(bar().right(px(-2.)))
                            })
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, _, cx| {
                            this.start_tab_drag(TitleTab::Session(id), event, cx)
                        }),
                    )
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        if *hovered {
                            this.tab_hover = Some(id);
                        } else if this.tab_hover == Some(id) {
                            this.tab_hover = None;
                        }
                        this.tabs_frame.update(cx, |_, cx| cx.notify());
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.open_menu(
                                MenuTarget::Tab(id),
                                Anchor::Point(event.position),
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close(id, window, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if std::mem::take(&mut this.skip_tab_click) {
                            return;
                        }
                        this.open_title_tab(&TitleTab::Session(id), window, cx);
                    }));
                let element = if dragging {
                    gpui::deferred(tab).with_priority(1).into_any()
                } else {
                    tab.into_any()
                };
                Some((id, element))
            })
            .collect();
        let elements: Vec<AnyElement> = tabs
            .iter()
            .enumerate()
            .filter_map(|(ix, tab)| match tab {
                TitleTab::Session(id) => session_elements.remove(id),
                TitleTab::App(view) => {
                    let element = self
                        .app_title_tab(*view, tab_width(tab), cx)
                        .left(shifts[ix]);
                    Some(if dragged.as_ref() == Some(tab) {
                        gpui::deferred(element).with_priority(1).into_any_element()
                    } else {
                        element.into_any_element()
                    })
                }
                TitleTab::Search => {
                    let element = self.search_title_tab(tab_width(tab), cx).left(shifts[ix]);
                    Some(if dragged.as_ref() == Some(tab) {
                        gpui::deferred(element).with_priority(1).into_any_element()
                    } else {
                        element.into_any_element()
                    })
                }
                TitleTab::Mono { id, .. } => {
                    let manager = self.monos.clone()?;
                    let w = tab_width(tab);
                    let element = manager.update(cx, |m, cx| m.title_tab(id, w, cx))?;
                    let drag_id = tab.clone();
                    let menu_id = id.clone();
                    let dragging = dragged.as_ref() == Some(tab);
                    let element = div()
                        .id(SharedString::from(format!("mono-tab-slot-{id}")))
                        .h(px(TITLE_BAR_HEIGHT))
                        .w(px(w))
                        .flex_shrink_0()
                        .relative()
                        .left(shifts[ix])
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, _, cx| {
                                this.start_tab_drag(drag_id.clone(), event, cx);
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                this.open_menu(
                                    MenuTarget::MonoTab(menu_id.clone()),
                                    Anchor::Point(event.position),
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .child(element);
                    Some(if dragging {
                        gpui::deferred(element).with_priority(1).into_any()
                    } else {
                        element.into_any()
                    })
                }
            })
            .collect();
        if moving || opening || switching {
            window.request_animation_frame();
        }
        let mut live: HashMap<TitleTab, AnyElement> = tabs.iter().cloned().zip(elements).collect();
        // TabWidthMotion: the slot's width eases while the tab keeps its own.
        let slot = |tab: &TitleTab, open: f32| {
            let (w, give) = slot_size(tab, open);
            div()
                .flex_shrink_0()
                .h(px(TITLE_BAR_HEIGHT))
                .w(px(w))
                .mr(px(-give))
                .overflow_hidden()
        };
        let elements: Vec<AnyElement> = slots
            .into_iter()
            .filter_map(|s| match s {
                crate::tab_motion::Slot::Live { key, open } => {
                    let element = live.remove(&key)?;
                    Some(if open >= 1. {
                        element
                    } else {
                        slot(&key, open)
                            // In a full strip the tab opens out of the new-tab
                            // button: its right edge shows first, so it grows
                            // leftward instead of sliding under the button.
                            .when(crowded, |d| d.flex().justify_end())
                            .child(element)
                            .into_any_element()
                    })
                }
                crate::tab_motion::Slot::Closing { key, open, .. } => {
                    let w = tab_width(&key);
                    // A closed conversation stays loaded, so its copy still reads.
                    let headline = key
                        .session()
                        .filter(|id| {
                            self.session_summaries
                                .contains_key(&self.workspace.session_for_pane(*id))
                        })
                        .map(|id| {
                            let copy = self.tab_copy(id, cx);
                            self.pane_headline(&copy, (None, 0., 1.), false, false)
                        });
                    Some(
                        slot(&key, open)
                            .child(
                                div()
                                    .w(px(w))
                                    .h(px(TITLE_BAR_HEIGHT))
                                    .flex()
                                    .items_center()
                                    .child(
                                        title_tab_plate(false, false)
                                            .w_full()
                                            .min_w_0()
                                            .gap(px(6.))
                                            .children(headline),
                                    ),
                            )
                            .into_any_element(),
                    )
                }
            })
            .collect();
        div()
            .id("title-tab-strip")
            .debug_selector(|| "title-tab-strip".into())
            .w(px(viewport))
            .min_w_0()
            .h(px(TITLE_BAR_HEIGHT))
            .relative()
            .flex()
            .items_center()
            .overflow_hidden()
            .child(
                div()
                    .id("session-tabs")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .track_scroll(&self.tab_scroll)
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .pl(px(STRIP_LEAD))
                    .pr(px(STRIP_TRAIL - 2.))
                    .children(elements)
                    .child(
                        div()
                            .id("new-tab")
                            .debug_selector(|| "new-tab".into())
                            .group("new-tab")
                            .role(Role::Button)
                            .aria_label("New session tab")
                            .cursor_pointer()
                            .size(px(TAB_PLATE_HEIGHT))
                            .flex_shrink_0()
                            .ml(px(new_tab_lead))
                            .rounded_md()
                            .bg(content(0.04))
                            .opacity(0.65)
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.opacity(1.).bg(content(0.07)))
                            .child(
                                icon("plus", 14.)
                                    .group_hover("new-tab", |s| s.text_color(content(1.))),
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.create(this.default_kind(), cx)),
                            ),
                    )
                    .child(titlebar_drag_area().flex_1().min_w_0()),
            )
            .into_any()
    }
}
/// Highlight only the existing hairline; its parent keeps the wider, transparent
/// grab area centered on that line. The wash brightens the underlying border.
fn resize_border(vertical: bool, dragging: bool) -> gpui::Div {
    div()
        .absolute()
        .bg(content(0.12))
        .opacity(if dragging { 1. } else { 0. })
        .group_hover("resize-handle", |s| s.opacity(1.))
        .map(|d| {
            if vertical {
                d.top_0()
                    .h_full()
                    .left(gpui::relative(0.5))
                    .ml(px(-0.5))
                    .w(px(1.))
            } else {
                d.left_0()
                    .w_full()
                    .top(gpui::relative(0.5))
                    .mt(px(-0.5))
                    .h(px(1.))
            }
        })
}

/// paneEnterFrom/collapsedRect: a pane on an outer edge slides in from it;
/// one in the middle opens from its centre line.
fn enter_from(leaf: &LayoutLeaf) -> (Edge, Rect) {
    let r = leaf.rect;
    let edge = 0.001;
    if leaf.axis == SplitDir::Right {
        if r.x + r.w >= 1. - edge {
            return (
                Edge::Right,
                Rect {
                    x: r.x + r.w,
                    w: 0.,
                    ..r
                },
            );
        }
        if r.x <= edge {
            return (Edge::Left, Rect { w: 0., ..r });
        }
        (
            Edge::Right,
            Rect {
                x: r.x + r.w / 2.,
                w: 0.,
                ..r
            },
        )
    } else {
        if r.y + r.h >= 1. - edge {
            return (
                Edge::Bottom,
                Rect {
                    y: r.y + r.h,
                    h: 0.,
                    ..r
                },
            );
        }
        if r.y <= edge {
            return (Edge::Top, Rect { h: 0., ..r });
        }
        (
            Edge::Bottom,
            Rect {
                y: r.y + r.h / 2.,
                h: 0.,
                ..r
            },
        )
    }
}
impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.keep_visible_idle_agents(cx);
        self.sync_ui_scale(window, cx);
        if self.mono_panes_unsettled(cx) {
            let this = cx.entity();
            window.defer(cx, move |window, cx| {
                this.update(cx, |this, cx| this.settle_mono_panes(window, cx));
            });
        }
        if let Some(id) = self.pending_composer_focus.take()
            && self
                .workspace
                .active
                .map(|pane| self.workspace.session_for_pane(pane))
                == Some(id)
            && let Some(view) = self.sessions.get(&id)
        {
            self.review_focused = false;
            window.focus(&view.read(cx).input.focus_handle(cx), cx);
        }
        if let Some(m) = &self.monos {
            m.read(cx).hide_html(cx);
            m.update(cx, |m, cx| m.bind_window(window, cx));
        }
        self.render_passes += 1;
        if let Some((id, tab)) = self.pending_review.take()
            && self.workspace.tab_of(id).is_some()
        {
            self.focus_pane(id, window, cx);
            self.open_review(tab, window, cx);
        }
        // Native fullscreen transitions resize the window, which redraws it.
        self.fullscreen = window.is_fullscreen();
        self.window_controls = corner_controls_width(window);
        // The corner's tools share the panel's header with client-drawn
        // window controls, so the panel keeps room for both. A Mono's icon
        // tabs need 252px (`corner_room`).
        let tools = if self.window_controls > 0. && self.mono_active(cx) {
            252.
        } else {
            crate::right_panel::WIDTH_MIN
        };
        self.right_panel_min = tools + self.window_controls;
        #[cfg(target_os = "linux")]
        {
            let client = matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            );
            if self.client_decorations != client {
                self.client_decorations = client;
                self.invalidate_chrome(cx);
            }
        }
        if cx.reduce_motion()
            && (self.sidebar_slide.is_some()
                || self.right_slide.is_some()
                || self.review_slide.is_some())
        {
            self.sidebar_slide = None;
            self.right_slide = None;
            self.review_slide = None;
            self.sync_right_panel(cx);
        }
        let p = &self.workspace.preferences;
        let system_light = matches!(
            cx.window_appearance(),
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
        );
        let project = &self.workspace.project;
        let animate = !cx.reduce_motion()
            && self
                .theme_project
                .as_ref()
                .is_some_and(|previous| previous != project);
        let project_theme = self
            .workspace
            .project_appearance
            .get(project)
            .and_then(|a| a.theme.as_ref());
        // Repainting bypasses cached views, so every surface follows the blend.
        if configure_project(p, project_theme, system_light, animate) {
            cx.refresh_windows();
        }
        if tweening() {
            window.request_animation_frame();
        }
        if self.theme_project.as_ref() != Some(project) {
            self.theme_project = Some(project.clone());
        }
        let glass = window_glass(p, system_light);
        if self.window_glass != Some(glass) {
            self.window_glass = Some(glass);
            window.set_background_appearance(crate::theme::window_backdrop(glass));
        }
        self.sync_settings(cx);
        self.sidebar_fraction = match &self.sidebar_slide {
            Some(slide) => {
                let (fraction, done) = slide.at(std::time::Instant::now());
                if done {
                    self.sidebar_slide = None;
                } else {
                    window.request_animation_frame();
                }
                fraction
            }
            None => {
                if self.workspace.sidebar_visible {
                    1.
                } else {
                    0.
                }
            }
        };
        if self.settings.is_some() {
            self.sidebar_fraction = 1.;
        }
        let sidebar_width = self.workspace.sidebar_width * self.sidebar_fraction;
        if !self.window_observed {
            self.window_observed = true;
            self.shortcuts = Some(shortcuts::install(cx.weak_entity(), window, cx));
            self.observations
                .push(cx.on_focus_lost(window, |this, window, cx| {
                    // Cached editors and previews can outlive their rendered pane.
                    // Recover as soon as its focus node leaves the tree, including
                    // close paths without a Window in their original event.
                    let target = window
                        .focus_lost_restore_target(cx)
                        .unwrap_or_else(|| this.focus.clone());
                    window.focus(&target, cx);
                }));
            self.window_active = window.is_window_active();
            self.observations
                .push(cx.observe_window_activation(window, |this, window, cx| {
                    this.window_active = window.is_window_active();
                    this.refresh_notification_permission();
                    cx.notify();
                }));
            self.start_native_notifications(cx);
            let shell = cx.entity().downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                shell
                    .update(cx, |shell, cx| {
                        shell.save_window_state(window, cx);
                        #[cfg(all(target_os = "macos", not(test)))]
                        if shell.workspace.preferences.close_to_tray
                            && shell.workspace.preferences.mono_menu_bar_icon
                            && shell
                                .monos
                                .as_ref()
                                .is_some_and(|m| !m.read(cx).roster.monos.is_empty())
                            && !shell.unsaved_files(cx)
                        {
                            crate::traffic_lights::hide(window);
                            return false;
                        }
                        if shell.unsaved_files(cx) {
                            shell.request_quit(window, cx);
                            false
                        } else {
                            true
                        }
                    })
                    .unwrap_or(true)
            });
            self.observations
                .push(cx.observe_window_activation(window, |this, window, cx| {
                    if !window.is_window_active() {
                        this.close_preview(cx);
                    } else {
                        this.resume_git(cx);
                    }
                }));
            self.observations
                .push(cx.observe_window_bounds(window, |this, window, cx| {
                    this.save_window_state(window, cx);
                    this.close_preview(cx);
                }));
        }
        self.viewport_width = window.viewport_size().width.as_f32();
        self.viewport_height = window.viewport_size().height.as_f32();
        self.refit_sidebar_to_room(window, cx);
        self.ensure_right_panel(window, cx);
        let full = self.right_panel_width();
        let right_width = match &self.right_slide {
            Some(slide) => {
                let (width, done) = slide.at(std::time::Instant::now());
                if done {
                    self.right_slide = None;
                    self.sync_right_panel(cx);
                    if self.workspace.right_panel_open
                        && !self.mono_active(cx)
                        && let Some(panel) = self.right_panel.clone()
                    {
                        panel.update(cx, |p, cx| p.finish_opening(window, cx));
                    }
                } else {
                    window.request_animation_frame();
                }
                width
            }
            None => full,
        };
        let right_shown = self.right_panel_shown(cx);
        self.right_drawn = if right_shown { right_width } else { 0. };
        let poll_git = self.window_active && !self.mono_active(cx);
        self.git.update(cx, |git, cx| {
            git.set_polling(poll_git && git.available(), cx)
        });
        if let Some(panel) = &self.right_panel {
            let classic = self.workspace.boxy_layout();
            let tab_width = self.project_tabs_width(cx);
            panel.update(cx, |p, cx| {
                p.set_top_inset(if classic { TITLE_BAR_HEIGHT } else { 0. }, cx);
                p.set_tab_width(tab_width, cx);
                p.set_show_excluded(self.workspace.preferences.show_excluded_files, cx);
            });
        }
        if let Some(search) = &self.app_search {
            let original = self.workspace.classic_layout.then(|| {
                if self.workspace.project_rail_open {
                    12.
                } else {
                    self.strip_lead(cx) + 6.
                }
            });
            search
                .view
                .update(cx, |search, cx| search.set_original_header(original, cx));
        }
        if let Some(monos) = &self.monos {
            monos.update(cx, |monos, cx| {
                monos.set_original_chrome(self.workspace.classic_layout, cx)
            });
            let details = monos.read(cx).details.clone();
            let inset = if self.workspace.boxy_layout() && !self.workspace.classic_layout {
                TITLE_BAR_HEIGHT
            } else {
                0.
            };
            details.update(cx, |view, cx| {
                view.set_top_inset(inset, cx);
            });
        }
        self.sync_notices(window, cx);
        div()
            .id("monocode-workspace")
            .relative()
            .role(Role::Application)
            .aria_label("Monocode Native")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .capture_key_up(cx.listener(|this, _, _, _| this.archive_shortcut_held = false))
            .on_action(cx.listener(|this, _: &ArchiveSession, window, cx| {
                this.archive_active_session(window, cx)
            }))
            .on_action(
                cx.listener(|this, open: &crate::image_lightbox::Open, window, cx| {
                    let view = cx.new(|cx| {
                        crate::image_lightbox::ImageLightbox::new(open.clone(), window, cx)
                    });
                    let closed =
                        cx.subscribe(&view, |this, _, _: &crate::image_lightbox::Closed, cx| {
                            this.image_lightbox = None;
                            cx.notify();
                        });
                    this.image_lightbox = Some((view, closed));
                    cx.notify();
                }),
            )
            .flex()
            .flex_col()
            .size_full()
            .bg(shell_fill())
            .text_color(content(1.))
            .font_family(crate::fonts::UI)
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if this.session_preview.as_ref().is_some_and(|preview| {
                    !preview
                        .surface
                        .get()
                        .is_some_and(|bounds| bounds.contains(&event.position))
                }) {
                    this.close_preview(cx);
                }
            }))
            .on_action(
                cx.listener(|this, event: &crate::file_editor::OpenPath, window, cx| {
                    let cwd = this.git.read(cx).project().to_owned();
                    let (path, location) = crate::file_editor::file_location(&event.0, &cwd);
                    this.open_review(crate::review::ReviewTab::File { cwd, path }, window, cx);
                    // Monos render above the session pane tree. Give its file
                    // an independent visible tab using the normal navigation
                    // path, which retains the Mono tab and live conversation.
                    if this.mono_active(cx)
                        && let Some(owner) = this.workspace.active
                        && let Some(pane) = this.independent_file_pane(owner, cx)
                    {
                        this.open_file_pane(pane, window, cx);
                    }
                    if let Some((line, column)) = location
                        && let Some(pane) = this.active_review(cx)
                    {
                        pane.update(cx, |pane, cx| pane.navigate_file(line, column, cx));
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::Quit, window, cx| this.request_quit(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GoToFile, window, cx| {
                    this.open_quick_open(false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| {
                this.open_quick_open(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_settings(crate::settings::Section::General, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenInbox, window, cx| this.open_inbox(window, cx)))
            .on_action(cx.listener(|this, _: &OpenNotes, window, cx| this.open_notes(window, cx)))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| {
                this.notification_state
                    .updater
                    .update(cx, |updater, cx| updater.check(true, cx));
            }))
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.step_ui_scale(1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.step_ui_scale(-1, cx)))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.step_ui_scale(0, cx)))
            .on_action(cx.listener(|this, _: &Reload, window, cx| this.request_reload(window, cx)))
            .on_action(cx.listener(|this, _: &SidebarAppearance, window, cx| {
                this.open_settings(crate::settings::Section::Appearance, window, cx)
            }))
            .on_action(|_: &About, window, cx| {
                let detail = format!("Version {}", crate::notifications::updater::version());
                drop(window.prompt(
                    gpui::PromptLevel::Info,
                    "Monocode Native",
                    Some(&detail),
                    &["OK"],
                    cx,
                ));
            })
            .on_action(|_: &ZoomWindow, window, _| window.zoom_window())
            .on_action(
                cx.listener(|this, _: &session_groups::RenameSidebarItem, window, cx| {
                    this.rename_sidebar_item(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &NewCodex, window, cx| {
                // ⌘T in the focused dock opens another terminal there.
                if this.dock_focused(window, cx).is_some() {
                    this.new_terminal(window, cx);
                } else {
                    this.create(this.default_kind(), cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::file_editor::ToggleAutosave, _, cx| {
                    this.workspace.preferences.autosave = !this.workspace.preferences.autosave;
                    this.apply_preferences(cx);
                    this.persist(cx);
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|this, _: &NewClaude, _, cx| this.create(Kind::Claude, cx)))
            .on_action(cx.listener(|this, _: &OpenProject, _, cx| this.open_project(cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if let Some(view) = this.active_app_view.filter(|_| this.settings.is_none()) {
                    if this.workspace.classic_layout {
                        this.close_app_view(window, cx);
                    } else {
                        this.close_title_tabs(&[TitleTab::App(view)], None, window, cx);
                    }
                    return;
                }
                if this.app_search.is_some() {
                    this.close_search_tab(window, cx);
                    return;
                }
                if let Some(TitleTab::Mono { id, .. }) = this.active_title_tab(cx) {
                    this.close_mono_tab(&id, window, cx);
                    return;
                }
                // ⌘W in the focused dock closes its terminal.
                if let Some(ix) = this.dock_focused(window, cx) {
                    this.close_terminal(ix, window, cx);
                    return;
                }
                // A focused review closes its tab first, as a focused file pane does.
                if this.review_focused
                    && let Some(review) = this.active_review(cx)
                {
                    review.update(cx, |r, cx| r.close_active(cx));
                    return;
                }
                if let Some(id) = this.workspace.active {
                    // Close Pane (⌘W) in a split; the whole tab otherwise.
                    this.close_pane(id, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle(1, window, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| this.cycle(-1, window, cx)))
            .on_action(cx.listener(|this, _: &VisitBack, window, cx| this.visit(-1, window, cx)))
            .on_action(cx.listener(|this, _: &VisitForward, window, cx| this.visit(1, window, cx)))
            .on_action(cx.listener(|this, _: &CloseAllTabs, window, cx| {
                // An empty strip can still have a new-tab request in flight.
                this.navigation_revision += 1;
                this.close_title_tabs(&this.visible_tabs(cx), None, window, cx);
                if !this.mono_active(cx) {
                    this.ensure_project_session(window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &CloseOtherTabs, window, cx| {
                    this.close_other_tabs(window, cx)
                }),
            )
            .on_action(cx.listener(|this, action: &ActivateTab, window, cx| {
                this.activate_tab(action.0, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousSession, window, cx| {
                this.step_session(-1, false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextSession, window, cx| {
                this.step_session(1, false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousSessionInTab, window, cx| {
                this.step_session(-1, true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextSessionInTab, window, cx| {
                this.step_session(1, true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousProject, window, cx| {
                this.step_project(-1, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &NextProject, window, cx| this.step_project(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveProjectBack, _, cx| this.move_current_project(-1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveProjectForward, _, cx| this.move_current_project(1, cx)),
            )
            .on_action(cx.listener(|this, _: &SwitchModel, window, cx| {
                if let Some(view) = this.workspace.active.and_then(|id| this.sessions.get(&id)) {
                    view.update(cx, |v, cx| v.toggle_recent_models(window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &DismissMenu, window, cx| {
                if this.cancel_project_rail_drag(cx) || this.cancel_space_drag(cx) {
                    return;
                }
                if this.cancel_classic_sidebar_tab(cx) {
                    return;
                }
                if this.remote_project_picker.is_some() {
                    this.close_remote_project(window, cx);
                    return;
                }
                if this.quick_open.is_some() {
                    this.close_quick_open(window, cx);
                    return;
                }
                if this.app_search.is_some() {
                    this.close_app_search(window, cx);
                    return;
                }
                if this.link_dialog.is_some() {
                    this.close_link_dialog(window, cx);
                    return;
                }
                if this.action_dialog.is_some() {
                    this.close_action_dialog(window, cx);
                    return;
                }
                if let Some(settings) = this.settings.clone() {
                    if !settings.update(cx, |s, cx| s.dismiss_transient(window, cx)) {
                        this.close_settings(window, cx);
                    }
                    return;
                }
                if let Some(monos) = this.monos.clone()
                    && monos.update(cx, |m, cx| m.dismiss_settings(window, cx))
                {
                    return;
                }
                if this.app_view_open() {
                    this.close_app_view(window, cx);
                    return;
                }
                if this.session_preview.is_some() {
                    this.close_preview(cx);
                    return;
                }
                if this.peek_open() && this.menu.is_none() {
                    this.set_peek(false, cx);
                    return;
                }
                if this.menu.is_none()
                    && this.right_panel_shown(cx)
                    && !this.mono_active(cx)
                    && let Some(panel) = &this.right_panel
                    && panel.read(cx).tab() == crate::right_panel::RightTab::Files
                {
                    let explorer = panel.read(cx).explorer.clone();
                    if explorer.update(cx, |explorer, cx| explorer.close_search(window, cx)) {
                        cx.stop_propagation();
                        return;
                    }
                }
                // `Session: Stop` when no menu is open to close.
                if this.menu.is_none()
                    && let Some(view) = this.workspace.active.and_then(|id| this.sessions.get(&id))
                {
                    view.update(cx, |v, cx| v.stop_if_busy(cx));
                }
                this.close_menu(cx);
                this.tab_drag = None;
                this.folder_drag = None;
                this.cancel_pane_drag(cx);
                this.pane_sash = None;
                this.invalidate_chrome(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                this.toggle_primary_sidebar(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRightPanel, window, cx| {
                if this.workspace.classic_layout {
                    this.toggle_sidebar(window, cx)
                } else {
                    this.toggle_right_panel(window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &SearchFiles, window, cx| {
                if this.app_view_open() || this.settings.is_some() || this.mono_active(cx) {
                    return;
                }
                this.close_app_search(window, cx);
                this.ensure_right_panel(window, cx);
                if this.workspace.classic_layout {
                    this.select_classic_sidebar_tab(
                        classic_sidebar::ClassicSidebarTab::Files,
                        window,
                        cx,
                    );
                    let explorer = this.right_panel.as_ref().unwrap().read(cx).explorer.clone();
                    explorer.update(cx, |explorer, cx| explorer.open_search(window, cx));
                    return;
                }
                this.set_right_panel_open(true, window, cx);
                if let Some(panel) = &this.right_panel {
                    let explorer = panel.read(cx).explorer.clone();
                    panel.update(cx, |panel, cx| {
                        panel.select(crate::right_panel::RightTab::Files, cx)
                    });
                    explorer.update(cx, |explorer, cx| explorer.open_search(window, cx));
                }
            }))
            .on_action(
                cx.listener(|this, _: &FocusSearch, window, cx| this.focus_search(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &NewTerminal, window, cx| this.new_terminal(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleLayout, _, cx| this.toggle_layout(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleWorkspaceStyle, _, cx| {
                    this.toggle_workspace_style(cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SplitRight, _, cx| this.split_session(SplitDir::Right, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SplitDown, _, cx| this.split_session(SplitDir::Down, cx)),
            )
            .on_action(cx.listener(|this, _: &FocusPaneLeft, window, cx| {
                this.focus_neighbor(Edge::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneRight, window, cx| {
                this.focus_neighbor(Edge::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneUp, window, cx| {
                this.focus_neighbor(Edge::Top, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPaneDown, window, cx| {
                this.focus_neighbor(Edge::Bottom, window, cx)
            }))
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if let Some(monos) = &this.monos {
                    monos.update(cx, |m, cx| m.move_rail_drag(event, cx));
                }
                this.move_tab_drag(event, cx);
                this.move_dock_drag(event, cx);
                this.move_pane_drag(event, cx);
                this.move_folder_drag(event, cx);
                this.move_project_rail_drag(event, cx);
                this.move_space_drag(event, window, cx);
                this.move_classic_sidebar_tab(event, cx);
                this.drag_sash(event, cx);
                this.drag_dock(event, cx);
                this.drag_review(event, cx);
                if let Some((start, width)) = this.project_resize {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.drag_resize(
                            ResizeTarget::Projects,
                            width + (event.position.x - start).as_f32(),
                            cx,
                        );
                    } else {
                        this.release_resizes(cx);
                    }
                }
                if let Some((start, width)) = this.resize {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let width = width + (event.position.x - start).as_f32();
                        this.drag_resize(ResizeTarget::Sidebar, width, cx);
                    } else {
                        this.release_resizes(cx);
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.finish_tab_drag(window, cx);
                    this.finish_dock_drag(cx);
                    this.finish_pane_drag(window, cx);
                    this.finish_folder_drag(cx);
                    this.finish_project_rail_drag(cx);
                    this.finish_space_drag(cx);
                    this.end_classic_sidebar_tab(cx);
                    if let Some(m) = this.monos.clone() {
                        m.update(cx, |m, cx| m.finish_reorder(cx));
                    }
                    this.release_resizes(cx);
                    if std::mem::take(&mut this.review_drag)
                        && let Some(review) = this.active_review(cx)
                    {
                        review.update(cx, |_, cx| cx.emit(crate::review::ReviewEvent::Changed));
                    }
                }),
            )
            .relative()
            .child(self.right_panel_resize_watch(cx))
            .children(self.layout_body(sidebar_width, right_shown, right_width, full, window, cx))
            .children(self.peek_watch(window, cx))
            .child(self.notices_overlay(cx))
            .children(self.menu.as_ref().map(|open| {
                let point = matches!(open.anchor, Anchor::Point(_));
                let beside = matches!(open.target, MenuTarget::Project { .. });
                // Context menus open at the pointer, as ExplorerMenu
                // does; TabGroupMenu hangs its top-left corner there.
                menu_popover(&open.menu, open.anchor.clone(), cx)
                    .when(
                        self.workspace.classic_layout && open.target == MenuTarget::SessionFilter,
                        |p| p.align(Align::End).gap(2.),
                    )
                    .when(point, |p| p.gap(0.))
                    .when(beside, |p| p.side(Side::Right))
                    .when(open.target == MenuTarget::SidebarOptions, |p| {
                        p.align(Align::Start)
                    })
                    .when(open.target == MenuTarget::Actions, |p| {
                        p.side(Side::Top).align(crate::popover::Align::End)
                    })
                    .when(open.target == MenuTarget::RunningTerminals, |p| {
                        p.side(Side::Top).align(crate::popover::Align::End)
                    })
            }))
            .children(self.drag_ghost(cx))
            .children(self.background_modal(window, cx))
            .children(self.link_modal(window, cx))
            .children(self.action_modal(window, cx))
            .children(self.resource_modal(window, cx))
            .children(self.quick_open_modal(window, cx))
            .children(self.remote_project_modal(window, cx))
            .children(self.notice_modals(window, cx))
            .children(self.folder_picker_popover(window, cx))
            .children(crate::accounts::get(cx))
            .children(
                self.settings
                    .clone()
                    .and_then(|view| view.update(cx, |view, cx| view.mobile_overlay(window, cx))),
            )
            .children(
                self.image_lightbox
                    .as_ref()
                    .map(|(view, _)| crate::image_lightbox::overlay(view)),
            )
            .map(|root| {
                #[cfg(target_os = "linux")]
                let caption = self.client_decorations;
                // `resize_handles` stays empty on Windows: GPUI reports server
                // decorations and the native frame does the resizing.
                #[cfg(target_os = "windows")]
                let caption = WINDOWS_CONTROLS;
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                let root = root
                    .when(caption && !self.fullscreen, |root| {
                        root.child(window_chrome::controls(
                            window,
                            cx.listener(|this, _, window, cx| this.request_quit(window, cx)),
                        ))
                    })
                    .children(window_chrome::resize_handles(window));
                root
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{EntityInputHandler, TestAppContext, WindowHandle};
    use monocode_core::store::Store;
    #[gpui::test]
    fn window_state_is_saved_on_resize_and_restored_with_the_workspace(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.refresh().unwrap();
        cx.run_until_parked();
        let dimensions = gpui::size(px(1440.), px(900.));
        cx.simulate_window_resize(window.into(), dimensions);
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let saved = locked(&shell.store).unwrap().ui_state().unwrap().unwrap();
                assert_eq!(*shell.latest_state.lock().unwrap(), saved);
                let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                assert_eq!(restored.window.bounds(cx).get_bounds().size, dimensions);
            })
            .unwrap();
    }
    #[gpui::test]
    fn window_state_preserves_fullscreen_when_backend_bounds_report_windowed(
        cx: &mut TestAppContext,
    ) {
        let (window, _, _) = window(cx);
        let normal_size = gpui::size(px(1440.), px(900.));
        cx.simulate_window_resize(window.into(), normal_size);
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| window.toggle_fullscreen())
            .unwrap();
        cx.simulate_window_resize(window.into(), gpui::size(px(1920.), px(1080.)));
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                // The test backend reproduces X11: fullscreen is tracked separately from bounds.
                assert!(window.is_fullscreen());
                assert!(matches!(
                    window.inner_window_bounds(),
                    gpui::WindowBounds::Windowed(_)
                ));
                let saved = locked(&shell.store).unwrap().ui_state().unwrap().unwrap();
                assert_eq!(*shell.latest_state.lock().unwrap(), saved);
                let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                let bounds = restored.window.bounds(cx);
                assert!(matches!(bounds, gpui::WindowBounds::Fullscreen(_)));
                assert_eq!(bounds.get_bounds().size, normal_size);
            })
            .unwrap();
    }
    #[gpui::test]
    fn hovering_archive_preserves_history_and_does_not_select_the_row(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |session, _| {
                    session.summary.cwd = "/work/monocode".into();
                    session.summary.has_messages = true;
                });
                shell.sessions[&b].update(cx, |session, _| session.summary.has_messages = true);
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let row = visual.debug_bounds("sidebar-session-1").unwrap();
        visual.simulate_mouse_move(row.center(), None, Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        let archive = visual
            .debug_bounds("archive-session-1")
            .expect("hover reveals Archive");
        visual.simulate_click(archive.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, window, cx| {
                assert_eq!(
                    shell.workspace.active,
                    Some(b),
                    "the archive click must not select its row"
                );
                assert!(!shell.workspace.tabs.contains(&a));
                assert!(shell.sessions[&a].read(cx).summary.archived);
                assert!(!shell.sidebar_session_ids(cx).contains(&a));
                assert!(
                    !locked(&shell.store)
                        .unwrap()
                        .load(a)
                        .unwrap()
                        .rows
                        .is_empty()
                );
                shell.run_menu(MenuTarget::SessionFilter, "archived", None, window, cx);
                assert!(shell.sidebar_session_ids(cx).contains(&a));
                shell.toggle_archive(a, cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, window, cx| {
                assert!(!shell.sessions[&a].read(cx).summary.archived);
                shell.run_menu(MenuTarget::SessionFilter, "clear", None, window, cx);
                assert!(shell.sidebar_session_ids(cx).contains(&a));
                assert_eq!(shell.workspace.active, Some(b));
            })
            .unwrap();
    }
    #[gpui::test]
    fn working_marks_animate_independently_and_completed_replies_clear_when_seen(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = window(cx);
        let transcript = window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |session, cx| {
                    session.busy = true;
                    session.summary.cwd = "/work/monocode".into();
                    session.summary.has_messages = true;
                    cx.emit(SessionChanged);
                });
                shell.sessions[&b].read(cx).transcript.clone()
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        let before = transcript.read_with(cx, |view, _| view.render_passes);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
        cx.run_until_parked();
        assert_eq!(
            before,
            transcript.read_with(cx, |view, _| view.render_passes)
        );
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |session, cx| {
                    session.busy = false;
                    cx.emit(SessionChanged);
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                assert!(
                    shell.unseen_finished.contains(&a),
                    "a reply completed in the background"
                );
                shell.open(a, window, cx);
                assert!(
                    !shell.unseen_finished.contains(&a),
                    "opening it restores the provider mark"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn hover_preview_waits_and_never_materializes_a_conversation(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        let (id, count) = window
            .update(cx, |shell, window, cx| {
                // Classic previews remain available without loading a conversation.
                shell.workspace.classic_layout = true;
                let mut summary = locked(&shell.store)
                    .unwrap()
                    .create("codex", "/work/monocode")
                    .unwrap();
                summary.has_messages = true;
                let id = summary.id;
                shell.order.push(id);
                shell.session_summaries.insert(id, summary);
                let anchor = AnchorSlot::default();
                anchor.set(Some(Bounds::new(
                    window.mouse_position() - gpui::point(px(5.), px(5.)),
                    gpui::size(px(20.), px(20.)),
                )));
                let count = shell.sessions.len();
                shell.hover_session(id, anchor, true, true, window, cx);
                assert!(shell.session_preview.is_none());
                (id, count)
            })
            .unwrap();
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(130));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(
                    shell.session_preview.as_ref().map(|preview| preview.id),
                    Some(id)
                );
                assert_eq!(shell.sessions.len(), count);
                assert!(!shell.sessions.contains_key(&id));
            })
            .unwrap();
    }

    #[gpui::test]
    fn sidebar_rows_have_a_gap_and_preview_survives_crossing_into_the_card(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |session, _| {
                    session.summary.cwd = "/work/monocode".into();
                    session.summary.has_messages = true;
                });
                shell.sessions[&b].update(cx, |session, _| session.summary.has_messages = true);
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let first = visual
            .debug_bounds(Box::leak(format!("sidebar-session-{b}").into_boxed_str()))
            .unwrap();
        let second = visual
            .debug_bounds(Box::leak(format!("sidebar-session-{a}").into_boxed_str()))
            .unwrap();
        assert_eq!(first.size.height, px(32.));
        assert_eq!(
            second.top() - first.bottom(),
            px(1.),
            "rows retain their 1px gap"
        );
        let move_to = |position, cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.simulate_mouse_move(position, cx)
            })
            .unwrap();
            cx.run_until_parked();
            cx.refresh().unwrap();
        };
        move_to(first.origin + gpui::point(px(20.), px(16.)), cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(130));
        cx.run_until_parked();
        cx.refresh().unwrap();
        let (first, surface) = window
            .update(cx, |shell, _, _| {
                let preview = shell
                    .session_preview
                    .as_ref()
                    .expect("hover opens a preview");
                assert_eq!(preview.id, b);
                (
                    preview.anchor.get().unwrap(),
                    preview.surface.get().unwrap(),
                )
            })
            .unwrap();
        // The close timer lets the pointer cross the 8px gap into the preview.
        move_to(
            gpui::point(first.right() + px(4.), first.top() + px(16.)),
            cx,
        );
        move_to(surface.center(), cx);
        window
            .update(cx, |shell, _, _| {
                assert!(shell.session_preview.is_some());
                assert!(
                    shell.preview_close.is_none(),
                    "entering the card cancels the close timer"
                );
            })
            .unwrap();
        move_to(second.origin + gpui::point(px(20.), px(16.)), cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(130));
        cx.run_until_parked();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, window, cx| {
                let preview = shell.session_preview.as_ref().unwrap();
                assert_eq!(preview.id, a, "another row replaces the old preview");
                assert_eq!(preview.anchor.get().unwrap().top() - first.bottom(), px(1.));
                shell.open(a, window, cx);
                assert!(
                    shell.session_preview.is_none(),
                    "selection dismisses the card"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn high_usage_chip_opens_running_work_and_escape_closes_it(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(crate::popover::bind);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("resource-pressure-chip").is_none());
        window
            .update(visual, |shell, _, cx| {
                shell
                    .resources
                    .update(cx, |monitor, cx| monitor.saturate(cx));
            })
            .unwrap();
        visual.run_until_parked();
        let chip = visual.debug_bounds("resource-pressure-chip").unwrap();
        let viewport = window.update(visual, |_, w, _| w.viewport_size()).unwrap();
        assert!(
            viewport.height - chip.bottom() < px(10.),
            "the chip sits in the footer"
        );
        visual.simulate_click(chip.center(), Default::default());
        visual.run_until_parked();
        let dialog = visual.debug_bounds("resource-dialog").unwrap();
        assert_eq!(dialog.size.width, px(418.));
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(visual.debug_bounds("resource-dialog").is_none());
        assert!(visual.debug_bounds("resource-pressure-chip").is_some());
    }
    #[gpui::test]
    fn usage_footer_follows_active_tabs_and_provider_changes(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.usage_footer.read(cx).kind(), Kind::Claude);
                shell.open(a, w, cx);
                assert_eq!(shell.usage_footer.read(cx).kind(), Kind::Codex);
                shell.close(a, w, cx);
                assert_eq!(shell.workspace.project, "/work/another-project");
                assert_eq!(shell.workspace.active, None);
                shell.open(b, w, cx);
                assert_eq!(shell.usage_footer.read(cx).kind(), Kind::Claude);
                shell.sessions[&b].update(cx, |conversation, cx| {
                    conversation.summary.provider = "codex".into();
                    cx.emit(SessionChanged);
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.usage_footer.read(cx).kind(), Kind::Codex);
            })
            .unwrap();
    }
    #[gpui::test]
    fn context_menu_click_does_not_activate_the_tab_beneath_it(cx: &mut TestAppContext) {
        let (window, a, b) = same_project_window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(a, w, cx);
                let anchor = Anchor::Point(gpui::point(px(600.), px(5.)));
                shell.open_menu(MenuTarget::Tab(b), anchor, w, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            let position = gpui::point(px(612.), px(23.));
            w.simulate_mouse_move(position, cx);
            w.dispatch_event(
                gpui::PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            w.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                }),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs, [a]);
                assert_eq!(shell.workspace.active, Some(a));
                assert!(shell.menu.is_none());
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_close_all_stays_in_the_selected_project(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(a, w, cx);
                shell.run_menu(MenuTarget::Tab(a), "all", None, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.workspace.project, "/work/another-project");
                assert_eq!(shell.visible_tabs(cx).len(), 1);
                assert_eq!(shell.workspace.tabs.len(), 2);
                let active = shell.workspace.active.unwrap();
                assert!(![a, b].contains(&active));
                assert!(!shell.sessions[&active].read(cx).summary.has_messages);
                assert!(shell.sessions.contains_key(&a) && shell.sessions.contains_key(&b));
                assert!(shell.history.back.iter().all(|id| *id == b));
                assert!(shell.history.forward.is_empty());
                let saved = locked(&shell.store).unwrap().ui_state().unwrap().unwrap();
                let saved: Workspace = serde_json::from_str(&saved).unwrap();
                assert_eq!(saved.project, shell.workspace.project);
                assert_eq!(saved.tabs, shell.workspace.tabs);
                assert_eq!(saved.active, shell.workspace.active);
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_new_and_split_requests_do_not_undo_project_navigation(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = window(cx);
        for split in [false, true] {
            window
                .update(cx, |shell, w, cx| {
                    shell.open(b, w, cx);
                    if split {
                        shell.split_session(SplitDir::Right, cx);
                    } else {
                        shell.create(Kind::Codex, cx);
                    }
                    shell.select_project("/work/another-project".into(), w, cx);
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |shell, _, _| {
                    assert_eq!(shell.workspace.project, "/work/another-project");
                    assert_eq!(shell.workspace.active, Some(a));
                    assert_eq!(shell.workspace.tabs, [a, b]);
                    assert!(shell.workspace.layouts.is_empty());
                })
                .unwrap();
        }
    }
    #[gpui::test]
    fn tab_lifecycle_closing_an_inactive_tab_keeps_notes_and_focus(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open_notes(w, cx);
                shell.close(a, w, cx);
                assert!(shell.notes.is_some());
                assert_eq!(shell.workspace.active, Some(b));
                assert_eq!(shell.workspace.project, "/work/monocode");
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_closing_an_inactive_split_pane_keeps_notes(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                let summary = locked(&shell.store)
                    .unwrap()
                    .create("codex", "/work/another-project")
                    .unwrap();
                let c = summary.id;
                shell.add(summary, cx);
                shell.open(a, w, cx);
                shell.workspace.split(c, SplitDir::Right);
                shell.open(b, w, cx);
                shell.open_notes(w, cx);
                shell.close_pane(a, w, cx);
                assert!(shell.notes.is_some());
                assert_eq!(shell.workspace.active, Some(b));
                assert_eq!(shell.workspace.tabs, [c, b]);
                assert_eq!(shell.workspace.project, "/work/monocode");
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_pending_project_blanks_are_not_duplicated(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                for _ in 0..5 {
                    shell.select_project("/work/empty".into(), w, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.workspace.project, "/work/empty");
                assert_eq!(
                    shell
                        .sessions
                        .values()
                        .filter(|s| s.read(cx).summary.cwd == "/work/empty")
                        .count(),
                    1
                );
                assert!(shell.workspace.active.is_some());
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_close_all_shortcut_keeps_the_project(cx: &mut TestAppContext) {
        let (window, a, _) = window(cx);
        cx.update(bind_keys);
        window
            .update(cx, |shell, w, cx| shell.open(a, w, cx))
            .unwrap();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-shift-w"));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.workspace.project, "/work/another-project");
                assert_eq!(shell.visible_tabs(cx).len(), 1);
                assert_eq!(shell.workspace.tabs.len(), 2);
                let active = shell.workspace.active.unwrap();
                assert!(!shell.sessions[&active].read(cx).summary.has_messages);
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_close_stays_in_project_and_keeps_running_drafts(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.sessions[&b].update(cx, |s, cx| {
                    s.busy = true;
                    s.input.update(cx, |i, cx| {
                        i.replace_text_in_range(None, "Keep this draft", w, cx)
                    });
                });
                shell.close(b, w, cx);
                assert!(shell.focus.is_focused(w));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.workspace.project, "/work/monocode");
                let blank = shell.workspace.active.unwrap();
                assert_ne!(blank, b);
                assert_eq!(shell.workspace.tabs, [a, blank]);
                assert!(!shell.sessions[&blank].read(cx).summary.has_messages);
                assert!(shell.sessions[&b].read(cx).busy);
                shell.open(b, w, cx);
                assert_eq!(
                    shell.sessions[&b].read(cx).input.read(cx).content.as_ref(),
                    "Keep this draft"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_opening_a_conversation_replaces_the_blank_tab(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.sessions[&b].update(cx, |s, _| s.summary.has_messages = true);
                shell.close(b, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                let blank = shell.workspace.active.unwrap();
                assert_eq!(shell.workspace.tabs, [a, blank]);
                shell.open(b, w, cx);
                assert_eq!(shell.workspace.tabs, [a, b]);
                assert_eq!(shell.workspace.active, Some(b));
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_close_prefers_right_then_left_in_the_same_project(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                let mut extra = Vec::new();
                for _ in 0..2 {
                    let summary = locked(&shell.store)
                        .unwrap()
                        .create("codex", "/work/monocode")
                        .unwrap();
                    extra.push(summary.id);
                    shell.add(summary, cx);
                    shell.open(*extra.last().unwrap(), w, cx);
                }
                let (c, d) = (extra[0], extra[1]);
                shell.open(c, w, cx);
                shell.close(c, w, cx);
                assert_eq!(shell.workspace.active, Some(d));
                shell.close(b, w, cx);
                assert_eq!(shell.workspace.active, Some(d));
                let before = shell.workspace.tabs.clone();
                shell.close(c, w, cx);
                shell.run_menu(MenuTarget::Tab(c), "others", None, w, cx);
                shell.run_menu(MenuTarget::Tab(d), "unknown", None, w, cx);
                assert_eq!(shell.workspace.tabs, before);
                assert_eq!(shell.workspace.active, Some(d));
                shell.run_menu(MenuTarget::Tab(a), "others", None, w, cx);
                assert_eq!(shell.workspace.tabs, [a, d]);
                assert_eq!(shell.workspace.active, Some(d));
                assert_eq!(shell.workspace.project, "/work/monocode");
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_project_switches_remember_the_last_focused_tab(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                let summary = locked(&shell.store)
                    .unwrap()
                    .create("codex", "/work/monocode")
                    .unwrap();
                let c = summary.id;
                shell.add(summary, cx);
                shell.open(c, w, cx);
                shell.open(b, w, cx);
                shell.select_project("/work/another-project".into(), w, cx);
                assert_eq!(shell.workspace.active, Some(a));
                shell.select_project("/work/monocode".into(), w, cx);
                assert_eq!(shell.workspace.active, Some(b));
                let saved = serde_json::to_string(&shell.workspace).unwrap();
                let summaries: Vec<_> = shell
                    .sessions
                    .iter()
                    .map(|(id, s)| (*id, s.read(cx).summary.cwd.clone()))
                    .collect();
                let restored = Workspace::restore(Some(&saved), &summaries, "/work/monocode");
                assert_eq!(restored.project_focus["/work/monocode"], b);
                assert_eq!(restored.project_focus["/work/another-project"], a);
                assert_eq!(restored.active, Some(b));
            })
            .unwrap();
    }
    #[gpui::test]
    fn tab_lifecycle_closing_a_file_only_tab_restores_the_project_chat(cx: &mut TestAppContext) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open_review(
                    crate::review::ReviewTab::Commit {
                        cwd: "/work/monocode".into(),
                        sha: "abcdef1".into(),
                        short_sha: "abcdef1".into(),
                        subject: "Fix".into(),
                    },
                    w,
                    cx,
                );
                let review = shell.reviews[&b].clone();
                review.update(cx, |r, cx| {
                    r.state.chat_hidden = true;
                    r.close_active(cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.workspace.project, "/work/monocode");
                assert!(shell.workspace.tabs.contains(&a));
                assert!(!shell.reviews.contains_key(&b));
                let active = shell.workspace.active.unwrap();
                assert_eq!(
                    shell.sessions[&active].read(cx).summary.cwd,
                    "/work/monocode"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn projects_without_tabs_land_on_one_reusable_blank_session(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        let blank = |shell: &Shell, cx: &App| {
            let id = shell
                .workspace
                .active
                .expect("a project always has an active session");
            let summary = &shell.sessions[&id].read(cx).summary;
            assert_eq!(summary.cwd, "/work/empty");
            assert!(!summary.has_messages);
            id
        };
        window
            .update(cx, |shell, w, cx| {
                shell.workspace.add_project("/work/empty");
                shell.select_project("/work/empty".into(), w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let (first, count) = window
            .update(cx, |shell, w, cx| {
                let id = blank(shell, cx);
                // Close its tab, leave, and come back: the same blank returns.
                shell.close(id, w, cx);
                shell.open(b, w, cx);
                shell.select_project("/work/empty".into(), w, cx);
                (id, shell.sessions.len())
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(blank(shell, cx), first);
                assert_eq!(shell.sessions.len(), count);
                assert!(shell.workspace.tabs.contains(&first));
            })
            .unwrap();
    }
    #[gpui::test]
    fn header_menu_switches_projects_from_its_picker_flyout(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
        });
        let before = window
            .update(cx, |shell, w, cx| {
                let anchor = Anchor::Slot(shell.project_anchor.clone());
                shell.open_menu(MenuTarget::Header, anchor, w, cx);
                shell.workspace.project.clone()
            })
            .unwrap();
        cx.run_until_parked();
        // Switch worktree is first; skip the separator to Switch project, open
        // its picker (which focuses the search field), and take the next project.
        cx.simulate_keystrokes(window.into(), "down right down enter");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.menu.is_none());
                assert_ne!(shell.workspace.project, before);
            })
            .unwrap();
    }
    #[gpui::test]
    fn project_appearance_menu_edits_and_background_dialog_persist_per_project(
        cx: &mut TestAppContext,
    ) {
        use monocode_core::project_appearance::{Background, COLORS, Effect, Scope};
        let (window, _, active) = window(cx);
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                let anchor = Anchor::Slot(shell.project_anchor.clone());
                shell.open_menu(MenuTarget::Header, anchor, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        // The settings flyout opens beside the header menu and focuses its name.
        cx.simulate_keystrokes(window.into(), "down down right");
        cx.run_until_parked();
        cx.simulate_input(window.into(), "Small");
        cx.simulate_keystrokes(window.into(), "space");
        cx.simulate_input(window.into(), "Studio");
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let flyout = window
            .update(visual, |shell, _, cx| {
                shell
                    .menu
                    .as_ref()
                    .unwrap()
                    .menu
                    .read(cx)
                    .submenu_region()
                    .get()
                    .unwrap()
            })
            .unwrap();
        let controls = visual.debug_bounds("project-appearance-controls").unwrap();
        let name = visual.debug_bounds("project-appearance-name").unwrap();
        let logo = visual.debug_bounds("choose-project-logo").unwrap();
        // TabGroupMenu's p-2 belongs to the whole card, including its actions.
        for inset in [
            controls.left() - flyout.left(),
            flyout.right() - controls.right(),
            name.top() - flyout.top(),
        ] {
            assert_eq!(inset, px(9.), "8px of padding inside the card's 1px frame");
        }
        assert_eq!(name.left(), controls.left());
        assert_eq!(logo.left() - controls.left(), px(2.));
        assert_eq!(logo.top() - name.bottom(), px(8.));
        assert_eq!(logo.size, gpui::size(px(36.), px(36.)));
        let background = visual.debug_bounds("menu-action-background").unwrap();
        assert_eq!(background.left() - flyout.left(), px(9.));
        assert_eq!(flyout.right() - background.right(), px(9.));
        let color = visual
            .debug_bounds("project-color-4")
            .expect("appearance controls in the flyout");
        visual.simulate_click(color.center(), Default::default());
        visual.run_until_parked();
        let mascot = visual.debug_bounds("project-mascot-3").unwrap();
        visual.simulate_click(mascot.center(), Default::default());
        visual.run_until_parked();
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(shell.menu.is_none());
                assert_eq!(shell.project_label("/work/monocode"), "Small Studio");
                let appearance = shell
                    .workspace
                    .project_appearance
                    .get_mut("/work/monocode")
                    .unwrap();
                assert_eq!(appearance.color, Some(COLORS[4]));
                assert_eq!(appearance.mascot.as_deref(), Some("cat"));
                appearance.background = Some(Background {
                    path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets/monocode.png").into(),
                    ..Default::default()
                });
                assert!(
                    !shell
                        .workspace
                        .project_appearance
                        .contains_key("/work/another-project")
                );
                shell.open_menu(
                    MenuTarget::Project {
                        path: "/work/monocode".into(),
                        rail: false,
                    },
                    Anchor::Point(gpui::point(px(300.), px(60.))),
                    w,
                    cx,
                );
            })
            .unwrap();
        visual.run_until_parked();
        let card = window
            .update(visual, |shell, _, cx| {
                let menu = shell.menu.as_ref().unwrap().menu.read(cx);
                menu.surface_region().get().unwrap()
            })
            .unwrap();
        let background_row = visual.debug_bounds("menu-action-background").unwrap();
        let last_row = visual.debug_bounds("menu-action-delete").unwrap();
        assert_eq!(background_row.left() - card.left(), px(9.));
        assert_eq!(card.right() - background_row.right(), px(9.));
        assert_eq!(background_row.size.height, px(32.));
        assert_eq!(card.bottom() - last_row.bottom(), px(9.));
        visual.simulate_click(background_row.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("project-background-dialog").is_some());
        let dialog = visual.debug_bounds("project-background-dialog").unwrap();
        let header = visual.debug_bounds("background-dialog-header").unwrap();
        let preview = visual.debug_bounds("background-dialog-preview").unwrap();
        let change = visual
            .debug_bounds("project-settings-Change image")
            .unwrap();
        let effect = visual.debug_bounds("background-effect-row").unwrap();
        let scope = visual.debug_bounds("background-scope-row").unwrap();
        let scope_control = visual.debug_bounds("background-scope-control").unwrap();
        let empty_row = visual.debug_bounds("background-empty-row").unwrap();
        let session_row = visual.debug_bounds("background-session-row").unwrap();
        let remove = visual
            .debug_bounds("project-settings-Remove background image")
            .unwrap();
        // Modal.tsx + ProjectBackgroundDialog.tsx: match the React box metrics.
        assert_eq!(dialog.size.width, px(418.));
        assert_eq!(preview.size, gpui::size(px(386.), px(162.)));
        assert_eq!(preview.left() - dialog.left(), px(16.));
        assert_eq!(preview.top() - header.bottom(), px(16.));
        assert_eq!(change.top() - preview.bottom(), px(8.));
        assert_eq!(change.size.height, px(32.));
        assert_eq!(effect.size.height, px(57.5));
        assert_eq!(scope_control.size, gpui::size(px(176.), px(32.)));
        for (before, after) in [
            (effect, scope),
            (scope, empty_row),
            (empty_row, session_row),
            (session_row, remove),
        ] {
            assert_eq!(after.top() - before.bottom(), px(20.));
        }
        assert_eq!(dialog.bottom() - remove.bottom(), px(16.));
        window
            .update(visual, |_, w, _| {
                assert_eq!(
                    w.painted_quads()
                        .iter()
                        .filter(|q| q.background == content(0.15).into()
                            && q.bounds.size.width.0 == 172. * w.scale_factor()
                            && q.bounds.size.height.0 == 4. * w.scale_factor())
                        .count(),
                    2
                );
            })
            .unwrap();
        let effect_trigger = visual.debug_bounds("project-background-effect").unwrap();
        assert_eq!(effect_trigger.size, gpui::size(px(144.), px(36.)));
        assert_eq!(effect_trigger.right(), preview.right());
        visual.simulate_click(effect_trigger.center(), Default::default());
        visual.run_until_parked();
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(visual.debug_bounds("project-background-dialog").is_some());
        assert!(visual.debug_bounds("project-settings-Haze").is_none());
        visual.simulate_click(effect_trigger.center(), Default::default());
        visual.run_until_parked();
        for selector in ["project-settings-Haze", "project-settings-Empty only"] {
            let bounds = visual.debug_bounds(selector).unwrap();
            visual.simulate_click(bounds.center(), Default::default());
            visual.run_until_parked();
        }
        let opacity = visual.debug_bounds("empty-background-opacity").unwrap();
        visual.simulate_click(opacity.center(), Default::default());
        visual.simulate_keystrokes("right");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                let background = shell.workspace.project_appearance["/work/monocode"]
                    .background
                    .as_ref()
                    .unwrap();
                assert_eq!(background.effect, Effect::GradientBlur);
                assert_eq!(background.scope, Scope::Empty);
                assert!((background.empty_opacity - 0.36).abs() < 0.001);
                let restored = Workspace::restore(
                    Some(&serde_json::to_string(&shell.workspace).unwrap()),
                    &[(active, "/work/monocode".into())],
                    "/work/monocode",
                );
                assert_eq!(
                    restored.project_appearance,
                    shell.workspace.project_appearance
                );
            })
            .unwrap();
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert!(shell.background_dialog.is_none())
            })
            .unwrap();
        assert!(visual.debug_bounds("project-chat-background").is_some());
        // Empty-only backgrounds disappear once a conversation has messages.
        window
            .update(visual, |shell, _, cx| {
                shell.sessions[&active].update(cx, |session, cx| {
                    session.summary.has_messages = true;
                    cx.notify();
                });
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("project-chat-background").is_none());
        visual.simulate_resize(gpui::size(px(820.), px(520.)));
        window
            .update(visual, |shell, w, cx| {
                shell.open_background_dialog("/work/monocode".into(), w, cx);
            })
            .unwrap();
        visual.run_until_parked();
        let compact = visual.debug_bounds("project-background-dialog").unwrap();
        let body = visual.debug_bounds("background-dialog-body").unwrap();
        assert!(compact.top() >= px(16.));
        assert!(compact.bottom() <= px(504.));
        assert!(body.bottom() <= compact.bottom());
    }
    #[gpui::test]
    fn theme_colors_dialog_saves_a_project_theme_and_can_return_to_app_colors(
        cx: &mut TestAppContext,
    ) {
        let (window, _, _) = window(cx);
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.set_reduce_motion(true);
        });
        let project = window
            .update(cx, |shell, w, cx| {
                let project = shell.workspace.project.clone();
                shell.open_project_theme(project.clone(), w, cx);
                project
            })
            .unwrap();
        cx.run_until_parked();
        let theme = |cx: &mut TestAppContext| {
            window
                .update(cx, |shell, _, _| shell.appearance(&project).theme)
                .unwrap()
        };
        assert_eq!(theme(cx), None, "opening the dialog alone changes nothing");
        // The modal centers itself once its height is measured.
        cx.refresh().unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let dialog = visual.debug_bounds("project-theme-dialog").unwrap();
        assert!(dialog.top() >= px(16.));
        let harbor = visual.debug_bounds("project-theme-palette-harbor").unwrap();
        visual.simulate_click(harbor.center(), gpui::Modifiers::none());
        let red = visual.debug_bounds("project-theme-accent-4").unwrap();
        visual.simulate_click(red.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        let saved = theme(visual).expect("the first pick gives the project its colors");
        assert_eq!(saved.workspace_theme, "harbor");
        assert_eq!(saved.accent_color.as_deref(), Some("#ef4444"));
        assert_eq!(crate::theme::accent_color(), gpui::rgb(0xef4444).into());
        let reset = visual
            .debug_bounds("project-settings-Use app colors")
            .unwrap();
        visual.simulate_click(reset.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(theme(visual), None);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert!(shell.background_dialog.is_none())
            })
            .unwrap();
    }
    #[gpui::test]
    fn backgrounds_survive_tab_and_notes_switches_until_the_tab_closes(cx: &mut TestAppContext) {
        use monocode_core::project_appearance::{Background, Effect, ProjectAppearance};
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                for path in ["/work/another-project", "/work/monocode"] {
                    shell.workspace.project_appearance.insert(
                        path.into(),
                        ProjectAppearance {
                            background: Some(Background {
                                path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets/monocode.png")
                                    .into(),
                                effect: Effect::Dither,
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    );
                }
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let background_b = window
            .update(cx, |shell, _, _| shell.backgrounds[&b].clone())
            .unwrap();
        let image_b =
            background_b.read_with(cx, |background, _| background.prepared_image().unwrap());
        window
            .update(cx, |shell, w, cx| shell.open(a, w, cx))
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let background_a = window
            .update(cx, |shell, _, _| shell.backgrounds[&a].clone())
            .unwrap();
        let image_a =
            background_a.read_with(cx, |background, _| background.prepared_image().unwrap());
        assert!(
            Arc::ptr_eq(&image_a, &image_b),
            "projects using the same artwork share its texture"
        );
        for _ in 0..3 {
            window
                .update(cx, |shell, w, cx| {
                    shell.open(b, w, cx);
                    shell.create(Kind::Codex, cx);
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |shell, w, cx| {
                    let new = shell.workspace.active.unwrap();
                    assert_ne!(new, b);
                    let prepared = shell.backgrounds[&new].read(cx).prepared_image().unwrap();
                    assert!(
                        Arc::ptr_eq(&prepared, &image_b),
                        "new tabs must reuse the already decoded artwork"
                    );
                    shell.close(new, w, cx);
                })
                .unwrap();
            cx.run_until_parked();
        }
        for id in [b, a, b, a, b] {
            window
                .update(cx, |shell, w, cx| shell.open(id, w, cx))
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |shell, _, cx| {
                    for (id, background, image) in
                        [(a, &background_a, &image_a), (b, &background_b, &image_b)]
                    {
                        assert_eq!(
                            shell.backgrounds[&id].entity_id(),
                            background.entity_id(),
                            "hidden tabs retain the original background view"
                        );
                        let prepared = shell.backgrounds[&id].read(cx).prepared_image().unwrap();
                        assert!(
                            Arc::ptr_eq(&prepared, image),
                            "switching tabs must not regenerate the texture"
                        );
                    }
                })
                .unwrap();
        }
        window
            .update(cx, |shell, w, cx| shell.open_notes(w, cx))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.backgrounds.len(), 2);
                shell.open(b, w, cx);
                shell.close(a, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(
                    !shell.backgrounds.contains_key(&a),
                    "closing a tab releases its background"
                );
                assert_eq!(shell.backgrounds[&b].entity_id(), background_b.entity_id());
                assert!(Arc::ptr_eq(
                    &shell.backgrounds[&b].read(cx).prepared_image().unwrap(),
                    &image_b
                ));
            })
            .unwrap();
    }
    #[gpui::test]
    fn sidebar_shortcut_keeps_working_after_hiding_focused_search(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| {
            input::bind(cx);
            bind_keys(cx);
        });
        window
            .update(cx, |shell, w, cx| shell.focus_search(w, cx))
            .unwrap();
        cx.refresh().unwrap();
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        for visible in [false, true, false, true] {
            cx.simulate_keystrokes(window.into(), &format!("{m}-b"));
            cx.run_until_parked();
            window
                .update(cx, |shell, _, _| {
                    assert_eq!(shell.workspace.sidebar_visible, visible)
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn right_panel_shortcut_keeps_working_after_hiding_focused_history(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            bind_keys(cx);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.toggle_right_panel(w, cx);
                let panel = shell.right_panel.clone().unwrap();
                panel.update(cx, |p, cx| {
                    p.select(crate::right_panel::RightTab::Graph, cx)
                });
                let history = panel.read(cx).history.clone();
                w.focus(&history.focus_handle(cx), cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        for open in [false, true, false, true] {
            cx.simulate_keystrokes(window.into(), &format!("{m}-shift-b"));
            cx.run_until_parked();
            window
                .update(cx, |shell, _, _| {
                    assert_eq!(shell.workspace.right_panel_open, open)
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn layout_shortcut_toggles_from_the_focused_composer(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            bind_keys(cx);
            crate::input::bind(cx);
        });
        window
            .update(cx, |shell, w, cx| shell.focus_active(w, cx))
            .unwrap();
        cx.refresh().unwrap();
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        for classic in [true, false] {
            cx.simulate_keystrokes(window.into(), &format!("{m}-shift-l"));
            cx.run_until_parked();
            window
                .update(cx, |shell, _, _| {
                    assert_eq!(shell.workspace.classic_layout, classic);
                    assert!(!shell.workspace.right_panel_open);
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn title_tabs_draw_their_own_session_marks(cx: &mut TestAppContext) {
        let (window, a, _) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |session, cx| {
                    session.busy = true;
                    cx.emit(SessionChanged);
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                // One entity drawn in both the sidebar and the cached tab strip
                // only repaints along one path, freezing the tab's spinner.
                assert_ne!(
                    shell.tab_marks[&a].entity_id(),
                    shell.session_marks[&a].entity_id()
                );
                assert!(shell.tab_marks[&a].read(cx).state() == MarkState::Working);
            })
            .unwrap();
    }
    #[gpui::test]
    fn switching_layout_with_working_session_marks_keeps_cached_paint_valid(
        cx: &mut TestAppContext,
    ) {
        use std::time::Duration;
        let (window, a, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            crate::input::bind(cx);
        });
        window
            .update(cx, |shell, w, cx| {
                for id in [a, b] {
                    shell.sessions[&id].update(cx, |session, cx| {
                        session.summary.cwd = "/work/monocode".into();
                        session.summary.has_messages = true;
                        cx.emit(SessionChanged);
                    });
                }
                shell.sessions[&a].update(cx, |session, cx| {
                    session.busy = true;
                    cx.emit(SessionChanged);
                });
                shell.focus_active(w, cx);
                shell.toggle_right_panel(w, cx);
                for _ in 0..30 {
                    shell.workspace.tabs.extend([a, b]);
                }
                shell.invalidate_chrome(cx);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, w, cx| {
                w.simulate_next_frame(cx);
                w.draw(cx).clear(cx);
            })
            .unwrap();
        };
        draw(cx);
        // Reusing the sidebar after an earlier sibling shrinks relocates its
        // text cache. A subsequent uncached slide must not replay old indices.
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.tabs.truncate(2);
                shell.tabs_frame.update(cx, |_, cx| cx.notify());
                cx.notify();
            })
            .unwrap();
        draw(cx);
        window
            .update(cx, |shell, w, cx| shell.toggle_sidebar(w, cx))
            .unwrap();
        draw(cx);
        for step in 0..20 {
            for _ in 0..5 {
                cx.executor().advance_clock(Duration::from_millis(20));
                cx.run_until_parked();
                cx.update_window(window.into(), |_, w, cx| {
                    w.simulate_next_frame(cx);
                    w.draw(cx).clear(cx);
                })
                .unwrap();
            }
            window
                .update(cx, |shell, w, cx| {
                    if step % 3 == 0 {
                        shell.toggle_sidebar(w, cx);
                    }
                    shell.toggle_layout(cx);
                })
                .unwrap();
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn titlebar_panel_buttons_keep_window_bounds_for_single_and_double_clicks(
        cx: &mut TestAppContext,
    ) {
        let (window, _, _) = window(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        window
            .update(cx, |shell, w, cx| shell.focus_active(w, cx))
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let original = window.update(cx, |_, w, _| w.window_bounds()).unwrap();
        for selector in ["toggle-sidebar", "toggle-right-panel"] {
            for click_count in [1, 2, 1, 2] {
                let position = cx.debug_bounds(selector).unwrap().center();
                cx.simulate_mouse_move(position, None, Default::default());
                cx.run_until_parked();
                let before = window
                    .update(cx, |shell, _, _| {
                        (
                            shell.workspace.sidebar_visible,
                            shell.workspace.right_panel_open,
                        )
                    })
                    .unwrap();
                cx.simulate_event(MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    click_count,
                    first_mouse: false,
                    modifiers: Default::default(),
                });
                cx.simulate_event(gpui::MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    click_count,
                    modifiers: Default::default(),
                });
                cx.run_until_parked();
                window
                    .update(cx, |shell, w, _| {
                        assert_eq!(w.window_bounds(), original);
                        if selector == "toggle-sidebar" {
                            assert_eq!(shell.workspace.sidebar_visible, !before.0);
                        } else {
                            assert_eq!(shell.workspace.right_panel_open, !before.1);
                        }
                    })
                    .unwrap();
            }
        }
    }

    #[gpui::test]
    fn panel_tab_switches_and_animated_reversals_keep_shortcuts_live(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            bind_keys(cx);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.focus_active(w, cx);
                shell.toggle_right_panel(w, cx);
                shell.open_review(
                    crate::review::ReviewTab::File {
                        cwd: "/work/monocode".into(),
                        path: "README.md".into(),
                    },
                    w,
                    cx,
                );
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        for width in [
            crate::right_panel::WIDTH_MIN,
            420.,
            crate::right_panel::WIDTH_DEFAULT,
        ] {
            window
                .update(cx, |shell, _, cx| {
                    shell.workspace.right_panel_width = width;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            assert_eq!(
                cx.debug_bounds("project-tabs-separator").unwrap().left(),
                cx.debug_bounds("right-panel-frame").unwrap().left(),
                "the title-bar rule follows the panel's divider"
            );
        }
        let handle = cx.debug_bounds("titlebar-right-panel-resize").unwrap();
        let start = handle.center();
        let end = start - gpui::point(px(40.), px(0.));
        cx.simulate_event(MouseDownEvent {
            position: start,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
            modifiers: Default::default(),
        });
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
        cx.simulate_event(gpui::MouseUpEvent {
            position: end,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Default::default(),
        });
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(
                    shell.workspace.right_panel_width,
                    crate::right_panel::WIDTH_DEFAULT + 40.
                );
                assert!(shell.right_resize.is_none());
            })
            .unwrap();
        assert_eq!(
            cx.debug_bounds("project-tabs-separator").unwrap().left(),
            cx.debug_bounds("right-panel-frame").unwrap().left()
        );
        for (selector, header) in [
            ("right-panel-tab-History", "history-toolbar"),
            ("project-diff-stat", "changes-header"),
            ("right-panel-tab-Explorer", "explorer-header"),
        ] {
            let tab = cx.debug_bounds(selector).unwrap();
            cx.simulate_click(tab.center(), Default::default());
            cx.run_until_parked();
            assert_eq!(
                cx.debug_bounds(header).unwrap().bottom(),
                cx.debug_bounds("review-tab-strip").unwrap().bottom(),
                "tool headers align with the open-file tabs"
            );
            window
                .update(cx, |shell, w, cx| {
                    assert!(
                        shell
                            .right_panel
                            .as_ref()
                            .unwrap()
                            .focus_handle(cx)
                            .contains_focused(w, cx)
                    );
                })
                .unwrap();
        }
        cx.update(|_, cx| cx.set_reduce_motion(false));
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        for open in [false, true, false, true, false] {
            cx.simulate_keystrokes(&format!("{m}-shift-b"));
            cx.run_until_parked();
            window
                .update(cx, |shell, w, cx| {
                    assert_eq!(shell.workspace.right_panel_open, open);
                    assert!(shell.focus.contains_focused(w, cx));
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn monos_and_session_title_tabs_share_width_spacing_and_scroll_slots(cx: &mut TestAppContext) {
        let (window, a, b) = same_project_window(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        let mono = window
            .update(cx, |shell, w, cx| {
                let manager = shell.mono_manager();
                manager.update(cx, |m, cx| m.create(Vec::new(), w, cx));
                manager.read(cx).active.clone().unwrap()
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let mono = cx
            .debug_bounds(Box::leak(format!("mono-title-tab-{mono}").into_boxed_str()))
            .unwrap();
        let first = cx
            .debug_bounds(Box::leak(format!("title-tab-{a}").into_boxed_str()))
            .unwrap();
        let second = cx
            .debug_bounds(Box::leak(format!("title-tab-{b}").into_boxed_str()))
            .unwrap();
        assert_eq!(mono.left() - second.right(), px(2.));
        assert_eq!(second.left() - first.right(), px(2.));
        assert_eq!(mono.size, first.size);
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.visible_tabs(cx).len(), 3);
                assert_eq!(shell.tab_scroll.bounds_for_item(2), Some(mono));
                assert_eq!(shell.tab_scroll.bounds_for_item(0), Some(first));
            })
            .unwrap();
    }

    #[gpui::test]
    fn crowded_tabs_fit_and_hold_the_new_tab_button_still_while_one_opens(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.preferences.tab_animations = true;
                for _ in 0..12 {
                    shell.create(Kind::Codex, cx);
                }
            })
            .unwrap();
        // Tab motion runs on the wall clock, as the window's frames do.
        let settle = |cx: &mut gpui::VisualTestContext| {
            std::thread::sleep(crate::tab_motion::DURATION + std::time::Duration::from_millis(20));
            window
                .update(cx, |shell, _, cx| {
                    shell.tabs_frame.update(cx, |_, cx| cx.notify())
                })
                .unwrap();
            cx.run_until_parked();
        };
        // Once to start the motions a render notices, once to finish them.
        settle(cx);
        settle(cx);
        let strip = cx.debug_bounds("title-tab-strip").unwrap();
        let first_control = cx
            .debug_bounds("project-diff-stat")
            .or_else(|| cx.debug_bounds("titlebar-file-search"))
            .unwrap();
        // Keep the strip's 6px gap before the trailing controls.
        assert_eq!(strip.right(), first_control.left() - px(6.));
        let settled = cx.debug_bounds("new-tab").unwrap();
        assert!(settled.right() <= strip.right(), "every tab fits");
        let last = window
            .update(cx, |shell, _, cx| {
                shell.visible_tabs(cx).last().and_then(|t| t.session())
            })
            .unwrap()
            .unwrap();
        let last = cx
            .debug_bounds(Box::leak(format!("title-tab-{last}").into_boxed_str()))
            .unwrap();
        // The strip's 2px gap and the button's 2px margin, as when it has room.
        assert!(
            settled.left() - last.right() <= px(4.5),
            "a full strip keeps the usual gap before the new-tab button: {:?}",
            settled.left() - last.right()
        );
        let (selected, other) = window
            .update(cx, |shell, _, cx| {
                let tabs = shell.visible_tabs(cx);
                let selected = shell
                    .active_title_tab(cx)
                    .and_then(|t| t.session())
                    .unwrap();
                let other = tabs
                    .iter()
                    .filter_map(|t| t.session())
                    .find(|id| *id != selected);
                (selected, other.unwrap())
            })
            .unwrap();
        let bounds = |cx: &mut gpui::VisualTestContext, id: i64| {
            cx.debug_bounds(Box::leak(format!("title-tab-{id}").into_boxed_str()))
                .unwrap()
        };
        assert_eq!(
            bounds(cx, selected).size.width,
            px(SELECTED_TAB_MIN_WIDTH),
            "the selected tab shrinks only down to its floor"
        );
        assert!(bounds(cx, other).size.width < px(SELECTED_TAB_MIN_WIDTH));
        let close = |cx: &mut gpui::VisualTestContext, id: i64| {
            cx.debug_bounds(Box::leak(format!("close-title-tab-{id}").into_boxed_str()))
        };
        assert!(close(cx, selected).is_some(), "the selected tab can close");
        if bounds(cx, other).size.width < px(TAB_CLOSE_MIN_WIDTH) {
            assert!(close(cx, other).is_none(), "a collapsed tab only selects");
        }
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.tab_scroll.max_offset().x, px(0.), "nothing scrolls");
                shell.create(Kind::Codex, cx);
            })
            .unwrap();
        let mut seen = Vec::new();
        for _ in 0..16 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            window
                .update(cx, |shell, _, cx| {
                    shell.tabs_frame.update(cx, |_, cx| cx.notify())
                })
                .unwrap();
            cx.run_until_parked();
            seen.push(cx.debug_bounds("new-tab").unwrap().left());
        }
        assert!(
            seen.iter().all(|x| (*x - settled.left()).abs() < px(0.5)),
            "the new-tab button holds still while a crowded strip makes room: {settled:?} {seen:?}"
        );
        window
            .update(cx, |shell, w, cx| {
                let first = shell.visible_tabs(cx)[0].session().unwrap();
                shell.close(first, w, cx);
            })
            .unwrap();
        let mut closing = Vec::new();
        for _ in 0..16 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            window
                .update(cx, |shell, _, cx| {
                    shell.tabs_frame.update(cx, |_, cx| cx.notify())
                })
                .unwrap();
            cx.run_until_parked();
            closing.push(cx.debug_bounds("new-tab").unwrap().left());
        }
        assert!(
            closing
                .iter()
                .all(|x| (*x - settled.left()).abs() < px(0.5)),
            "and while a closed tab collapses: {closing:?}"
        );
    }
    #[gpui::test]
    fn sidebar_toggle_holds_still_and_history_follows_the_sidebar(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let width = window
            .update(cx, |shell, _, _| {
                assert!(shell.workspace.sidebar_visible);
                shell.workspace.sidebar_width
            })
            .unwrap();
        let open_toggle = cx.debug_bounds("toggle-sidebar").unwrap();
        let open_forward = cx.debug_bounds("visit-forward").unwrap();
        window
            .update(cx, |shell, window, cx| shell.toggle_sidebar(window, cx))
            .unwrap();
        cx.run_until_parked();
        let closed_toggle = cx.debug_bounds("toggle-sidebar").unwrap();
        let closed_back = cx.debug_bounds("visit-back").unwrap();
        let closed_forward = cx.debug_bounds("visit-forward").unwrap();
        // The toggle sits just past the traffic lights either way.
        assert_eq!(open_toggle.origin, closed_toggle.origin);
        assert_eq!(open_toggle.left(), px(TRAFFIC_LIGHTS));
        // Open, history rides the sidebar's edge, inside its `pr-1.5`.
        assert_eq!(open_forward.right(), px(width - 6.));
        // Closed, it follows the toggle directly.
        assert_eq!(closed_back.left(), closed_toggle.right());
        assert_eq!(closed_forward.right(), px(TRAFFIC_LIGHTS + 72.));
        // Fullscreen hides the native buttons, and the toggle takes their room.
        window
            .update(cx, |_, window, _| window.toggle_fullscreen())
            .unwrap();
        cx.run_until_parked();
        window.update(cx, |_, window, _| window.refresh()).unwrap();
        cx.run_until_parked();
        assert_eq!(
            cx.debug_bounds("toggle-sidebar").unwrap().left(),
            px(NO_TRAFFIC_LIGHTS)
        );
        assert_eq!(
            cx.debug_bounds("visit-forward").unwrap().right(),
            px(NO_TRAFFIC_LIGHTS + 72.)
        );
    }
    #[gpui::test]
    fn collapsed_sidebar_peeks_from_the_edge_and_the_toggle(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| shell.toggle_sidebar(window, cx))
            .unwrap();
        cx.run_until_parked();
        let width = window
            .update(cx, |shell, _, _| shell.workspace.sidebar_width)
            .unwrap();
        let open = |cx: &mut gpui::VisualTestContext| {
            window.update(cx, |shell, _, _| shell.peek_open()).unwrap()
        };
        assert!(cx.debug_bounds("sidebar-peek").is_none());
        // The window's left edge peeks it out, below the title bar.
        cx.simulate_mouse_move(gpui::point(px(2.), px(20.)), None, Default::default());
        assert!(!open(cx));
        cx.simulate_mouse_move(gpui::point(px(2.), px(300.)), None, Default::default());
        cx.run_until_parked();
        assert!(open(cx));
        let panel = cx.debug_bounds("sidebar-peek").unwrap();
        assert_eq!(panel.left(), px(6.));
        assert_eq!(panel.top(), px(TITLE_BAR_HEIGHT));
        assert_eq!(panel.size.width, px(width));
        // Crossing onto the panel keeps it out; leaving it puts it away after
        // a grace period.
        cx.simulate_mouse_move(gpui::point(px(100.), px(300.)), None, Default::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert!(open(cx));
        cx.simulate_mouse_move(
            gpui::point(px(width + 200.), px(300.)),
            None,
            Default::default(),
        );
        cx.run_until_parked();
        assert!(open(cx));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        assert!(!open(cx));
        assert!(cx.debug_bounds("sidebar-peek").is_none());
        // Hovering the toggle peeks it too, and a click elsewhere dismisses it.
        let toggle = cx.debug_bounds("toggle-sidebar").unwrap();
        cx.simulate_mouse_move(toggle.center(), None, Default::default());
        cx.run_until_parked();
        assert!(open(cx));
        cx.simulate_click(gpui::point(px(width + 200.), px(300.)), Default::default());
        cx.run_until_parked();
        assert!(!open(cx));
        // Docking the sidebar drops the peek, and an open sidebar never peeks.
        cx.simulate_mouse_move(toggle.center(), None, Default::default());
        cx.run_until_parked();
        assert!(open(cx));
        cx.simulate_click(toggle.center(), Default::default());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.workspace.sidebar_visible);
                assert!(shell.sidebar_peek.is_none());
            })
            .unwrap();
        cx.simulate_mouse_move(gpui::point(px(2.), px(300.)), None, Default::default());
        cx.run_until_parked();
        assert!(!open(cx));
    }
    #[gpui::test]
    fn original_sidebar_geometry_matches_tauri_chrome(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.project_rail_open = true;
                shell.classic_sidebar_tab = classic_sidebar::ClassicSidebarTab::Sessions;
                shell.layout_changed(cx);
            })
            .unwrap();
        visual.run_until_parked();

        let projects = visual.debug_bounds("project-rail-frame").unwrap();
        let sidebar = visual.debug_bounds("sidebar-frame").unwrap();
        let header = visual.debug_bounds("classic-workspace-header").unwrap();
        let tabs = visual.debug_bounds("classic-workspace-tabs").unwrap();
        let search = visual.debug_bounds("classic-conversation-search").unwrap();
        assert_eq!(projects.size.width, px(220.));
        assert_eq!(sidebar.size.width, px(260.));
        assert_eq!(projects.top(), px(0.));
        assert_eq!(sidebar.top(), px(0.));
        assert_eq!(sidebar.left(), projects.right());
        assert_eq!(header.top(), sidebar.top());
        assert_eq!(header.size.height, px(40.));
        assert_eq!(tabs.top(), header.bottom());
        assert_eq!(tabs.size.height, px(36.));
        assert_eq!(search.top(), tabs.bottom());
        assert_eq!(search.size.height, px(36.));
        assert_eq!(
            visual.debug_bounds("sidebar-sessions").unwrap().top(),
            search.bottom()
        );
        assert_eq!(
            visual.debug_bounds("main-pane-frame").unwrap().left(),
            sidebar.right()
        );

        let back = visual.debug_bounds("visit-back").unwrap();
        let forward = visual.debug_bounds("visit-forward").unwrap();
        let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
        assert_eq!(back.right(), forward.left());
        assert_eq!(toggle.left(), px(TRAFFIC_LIGHTS));
        assert!(toggle.right() <= back.left());
        assert_eq!(forward.right(), projects.right() - px(6.));
        assert_eq!(toggle.center().y, header.center().y);
        assert!(visual.debug_bounds("sidebar-project-button").is_none());
    }

    #[gpui::test]
    fn original_sidebar_tools_replace_sessions_in_the_left_column(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.project_rail_open = true;
                shell.layout_changed(cx);
            })
            .unwrap();
        visual.run_until_parked();
        let sidebar = visual.debug_bounds("sidebar-frame").unwrap();
        let tabs = visual.debug_bounds("classic-workspace-tabs").unwrap();

        for (id, selector) in [("files", "explorer-header"), ("changes", "changes-header")] {
            let tab = visual
                .debug_bounds(if id == "files" {
                    "classic-workspace-tab-files"
                } else {
                    "classic-workspace-tab-changes"
                })
                .unwrap();
            visual.simulate_click(tab.center(), Default::default());
            visual.run_until_parked();
            let tool = visual.debug_bounds(selector).unwrap();
            assert_eq!(tool.top(), tabs.bottom());
            assert_eq!(tool.left(), sidebar.left());
            assert!(tool.right() <= sidebar.right());
            assert!(visual.debug_bounds("right-panel-frame").is_none());
            assert!(visual.debug_bounds("classic-conversation-search").is_none());
            window
                .update(visual, |shell, _, _| {
                    assert_eq!(shell.classic_sidebar_tab.id(), id);
                    assert_eq!(
                        shell
                            .workspace
                            .classic_sidebar_tabs
                            .get(&shell.workspace.project)
                            .map(String::as_str),
                        Some(id)
                    );
                })
                .unwrap();
        }
        let sessions = visual
            .debug_bounds("classic-workspace-tab-sessions")
            .unwrap();
        visual.simulate_click(sessions.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("classic-conversation-search").is_some());
        assert!(visual.debug_bounds("explorer-header").is_none());
        assert!(visual.debug_bounds("changes-header").is_none());
    }

    #[gpui::test]
    fn original_sidebar_compact_rail_uses_vertical_workspace_tabs(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.project_rail_open = true;
                shell.layout_changed(cx);
            })
            .unwrap();
        visual.run_until_parked();
        let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
        visual.simulate_click(toggle.center(), Default::default());
        visual.run_until_parked();

        let projects = visual.debug_bounds("project-rail-frame").unwrap();
        let sidebar = visual.debug_bounds("sidebar-frame").unwrap();
        let header = visual.debug_bounds("classic-workspace-header").unwrap();
        assert_eq!(projects.size.width, px(48.));
        assert_eq!(sidebar.left(), projects.right());
        assert_eq!(sidebar.size.width, px(260.));
        assert!(visual.debug_bounds("classic-workspace-tabs").is_none());
        assert!(visual.debug_bounds("rail-workspace-sessions").is_some());
        assert!(visual.debug_bounds("rail-workspace-files").is_some());
        assert!(visual.debug_bounds("rail-workspace-changes").is_some());
        assert_eq!(
            visual
                .debug_bounds("classic-conversation-search")
                .unwrap()
                .top(),
            header.bottom()
        );
        #[cfg(target_os = "macos")]
        {
            let strip = visual.debug_bounds("title-tab-strip").unwrap();
            assert_eq!(strip.top(), px(0.));
            assert_eq!(strip.size.height, px(40.));
            assert_eq!(projects.top(), px(40.));
            assert_eq!(header.top(), px(40.));
        }

        let expand = visual.debug_bounds("toggle-sidebar").unwrap();
        assert_eq!(expand.origin, toggle.origin);
        assert!(visual.debug_bounds("rail-expand-projects").is_none());
        assert_eq!(
            visual.debug_bounds("visit-back").unwrap().left(),
            expand.right(),
            "collapsed history follows the toggle, independent of the sessions sidebar"
        );
        assert_eq!(
            visual.debug_bounds("visit-forward").unwrap().right(),
            px(TRAFFIC_LIGHTS + 72.)
        );
        visual.simulate_click(expand.center(), Default::default());
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("project-rail-frame")
                .unwrap()
                .size
                .width,
            px(220.)
        );
        assert!(visual.debug_bounds("classic-workspace-tabs").is_some());
    }

    #[gpui::test]
    fn original_sidebar_workspace_tabs_reorder_and_persist(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.project_rail_open = true;
                shell.layout_changed(cx);
            })
            .unwrap();
        visual.run_until_parked();
        let sessions = visual
            .debug_bounds("classic-workspace-tab-sessions")
            .unwrap();
        let changes = visual
            .debug_bounds("classic-workspace-tab-changes")
            .unwrap();
        visual.simulate_mouse_down(sessions.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_move(changes.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_up(changes.center(), MouseButton::Left, Default::default());
        visual.run_until_parked();
        let files = visual.debug_bounds("classic-workspace-tab-files").unwrap();
        let changes = visual
            .debug_bounds("classic-workspace-tab-changes")
            .unwrap();
        let sessions = visual
            .debug_bounds("classic-workspace-tab-sessions")
            .unwrap();
        assert!(files.left() < changes.left() && changes.left() < sessions.left());
        window
            .update(visual, |shell, _, _| {
                assert_eq!(
                    shell.workspace.classic_sidebar_tab_order,
                    ["files", "changes", "sessions"]
                );
                assert_eq!(
                    shell.classic_sidebar_tab,
                    classic_sidebar::ClassicSidebarTab::Sessions
                );
                let saved = shell.latest_state.lock().unwrap().clone();
                let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                assert_eq!(
                    restored.classic_sidebar_tab_order,
                    ["files", "changes", "sessions"]
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn new_boxy_layout_runs_the_sidebar_and_workspace_full_height(cx: &mut TestAppContext) {
        crate::theme::configure(&crate::settings::Preferences::default(), false);
        let (window, _, _) = window(cx);
        let frames = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, w, cx| {
                w.simulate_next_frame(cx);
                w.draw(cx).clear(cx);
            })
            .unwrap();
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            window
                .update(cx, |_, w, _| {
                    let (blur, radius) = (24., 11.);
                    let marker = gpui::backdrop_blur_marker(blur * w.scale_factor());
                    assert!(
                        w.painted_quads().iter().any(|q| q.border_color == marker
                            && q.corner_radii.top_left.0 == radius * w.scale_factor()),
                        "composer frosting keeps the same blur and radius in both layouts"
                    );
                })
                .unwrap();
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            (
                visual.debug_bounds("sidebar-frame"),
                visual.debug_bounds("main-pane-frame").unwrap(),
                visual.debug_bounds("title-tab-strip").unwrap(),
            )
        };
        let (sidebar, main, tabs) = frames(cx);
        let sidebar = sidebar.unwrap();
        // Inset: the title bar spans the window above the sidebar and card.
        assert_eq!(sidebar.top(), px(TITLE_BAR_HEIGHT));
        assert_eq!(main.top(), px(TITLE_BAR_HEIGHT));
        assert!(tabs.bottom() <= main.top());
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.new_layout_boxy = true;
                shell.layout_changed(cx)
            })
            .unwrap();
        let (sidebar, main, tabs) = frames(cx);
        let sidebar = sidebar.unwrap();
        assert_eq!(sidebar.top(), px(0.));
        assert_eq!(main.top(), px(0.));
        assert_eq!(main.left(), sidebar.right());
        // The strip lives in the workspace column, beside the sidebar.
        assert!(tabs.left() >= main.left());
        // The right panel toggle holds the window's corner, and the strip
        // runs up to it.
        let corner = |cx: &mut TestAppContext| {
            gpui::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("toggle-right-panel")
                .unwrap()
        };
        let closed_corner = corner(cx);
        assert_eq!(closed_corner.top(), px(TITLE_BAR_HEIGHT / 2. - 12.));
        assert!(tabs.right() <= closed_corner.left());
        window
            .update(cx, |shell, w, cx| {
                cx.set_reduce_motion(true);
                shell.toggle_right_panel(w, cx);
            })
            .unwrap();
        let (_, main_open, _) = frames(cx);
        assert_eq!(corner(cx), closed_corner);
        assert!(main_open.right() < main.right());
        // Open, the tools start at the panel's left edge as it is resized;
        // the toggle stays at the window's right edge.
        for width in [
            crate::right_panel::WIDTH_MIN,
            420.,
            crate::right_panel::WIDTH_MAX,
        ] {
            window
                .update(cx, |shell, _, cx| {
                    shell.workspace.right_panel_width = width;
                    cx.notify();
                })
                .unwrap();
            let (_, main, tabs) = frames(cx);
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            let explorer = visual.debug_bounds("right-panel-explorer").unwrap();
            assert!(visual.debug_bounds("right-panel-changes").is_none());
            let history = visual.debug_bounds("right-panel-history").unwrap();
            assert_eq!(explorer.left(), main.right() + px(6.));
            assert_eq!(explorer.center().y, closed_corner.center().y);
            assert_eq!(history.top(), explorer.top());
            assert!(explorer.right() <= history.left());
            assert!(history.right() <= closed_corner.left());
            assert!(tabs.right() <= explorer.left());
            assert_eq!(corner(cx), closed_corner);
        }
        let history = gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("right-panel-history")
            .expect("the panel's pane buttons show beside the toggle");
        assert!(history.right() <= closed_corner.left());
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(history.center(), Default::default());
        visual.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let panel = shell.right_panel.clone().unwrap();
                assert_eq!(panel.read(cx).tab(), crate::right_panel::RightTab::Graph);
                assert_eq!(
                    shell.workspace.right_panel_tab,
                    crate::right_panel::RightTab::Graph
                );
            })
            .unwrap();
        window
            .update(cx, |shell, w, cx| {
                shell.toggle_right_panel(w, cx);
                cx.set_reduce_motion(false);
            })
            .unwrap();
        frames(cx);
        assert!(
            gpui::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("right-panel-history")
                .is_none(),
            "closed, the corner holds only the toggle"
        );
        // The visit arrows stay at the open sidebar's edge, not in the strip.
        let forward = gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("visit-forward")
            .unwrap();
        assert!(forward.right() <= sidebar.right());
        assert!(forward.right() > sidebar.right() - px(16.));
        // The sidebar toggle holds still as the sidebar closes and reopens.
        let toggle = |cx: &mut TestAppContext| {
            gpui::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("toggle-sidebar")
                .unwrap()
        };
        let open = toggle(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        for visible in [false, true] {
            window
                .update(cx, |shell, w, cx| {
                    shell.toggle_sidebar(w, cx);
                    assert_eq!(shell.workspace.sidebar_visible, visible);
                })
                .unwrap();
            let (_, main, _) = frames(cx);
            assert_eq!(main.left() == px(0.), !visible);
            assert_eq!(toggle(cx), open);
        }
        cx.update(|cx| cx.set_reduce_motion(false));
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.workspace.new_layout_boxy);
                let saved = serde_json::to_string(&shell.workspace).unwrap();
                assert!(Workspace::restore(Some(&saved), &[], "/work/monocode").new_layout_boxy);
                shell.workspace.new_layout_boxy = !shell.workspace.new_layout_boxy;
                shell.layout_changed(cx);
                assert!(!shell.workspace.new_layout_boxy);
            })
            .unwrap();
    }
    #[gpui::test]
    fn toggling_the_sidebar_after_its_cached_frames_does_not_replay_stale_paint(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = window(cx);
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, w, cx| {
                w.simulate_next_frame(cx);
                w.draw(cx).clear(cx);
            })
            .unwrap();
        };
        // The sidebar's search records its paint behind a long tab strip.
        window
            .update(cx, |shell, _, cx| {
                for _ in 0..30 {
                    shell.workspace.tabs.extend([a, b]);
                }
                shell.invalidate_chrome(cx);
                cx.notify();
            })
            .unwrap();
        draw(cx);
        // The strip shrinks while the sidebar replays its cached frame.
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.tabs.truncate(2);
                shell.tabs_frame.update(cx, |_, cx| cx.notify());
                cx.notify();
            })
            .unwrap();
        draw(cx);
        // Sliding paints the sidebar uncached; its search must not replay
        // the indices it recorded two frames ago.
        window
            .update(cx, |shell, w, cx| shell.toggle_sidebar(w, cx))
            .unwrap();
        draw(cx);
        draw(cx);
    }
    #[gpui::test]
    fn main_pane_covers_a_fixed_sidebar_with_a_clipped_fade(cx: &mut TestAppContext) {
        use std::time::Duration;
        let (window, _, active) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&active].update(cx, |session, _| {
                    session.summary.has_messages = true;
                });
                shell.invalidate_chrome(cx);
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, w, cx| {
            w.simulate_next_frame(cx);
            w.draw(cx).clear(cx);
        })
        .unwrap();
        let row = window
            .update(cx, |shell, w, _| {
                w.painted_quads()
                    .into_iter()
                    .find(|q| {
                        q.background == content(0.1).into()
                            && q.bounds.size.height.0 == 32. * w.scale_factor()
                            && q.bounds.size.width.0
                                > shell.workspace.sidebar_width / 2. * w.scale_factor()
                            && q.bounds.right().0 < shell.workspace.sidebar_width * w.scale_factor()
                    })
                    .expect("selected session has a painted background")
                    .bounds
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
        for visible in [false, true] {
            window
                .update(visual, |shell, w, cx| {
                    shell.toggle_sidebar(w, cx);
                    assert_eq!(shell.workspace.sidebar_visible, visible);
                })
                .unwrap();
            let mut previous = if visible { 0. } else { 1. };
            for ms in [30, 65, 110, 170] {
                window
                    .update(visual, |shell, _, cx| {
                        shell
                            .sidebar_slide
                            .as_ref()
                            .unwrap()
                            .freeze_elapsed(Duration::from_millis(ms));
                        cx.notify();
                    })
                    .unwrap();
                visual
                    .update_window(window.into(), |_, w, cx| {
                        w.simulate_next_frame(cx);
                        w.draw(cx).clear(cx);
                    })
                    .unwrap();
                let clip = visual.debug_bounds("sidebar-frame").unwrap();
                let content = visual.debug_bounds("sidebar-content").unwrap();
                let main = visual.debug_bounds("main-pane-frame").unwrap();
                assert_eq!(
                    visual.debug_bounds("toggle-sidebar").unwrap().origin,
                    toggle.origin
                );
                assert_eq!(content.left(), px(0.));
                assert!((clip.right() - main.left()).abs() < px(1.));
                window
                    .update(visual, |shell, w, _| {
                        let fraction = shell.sidebar_fraction;
                        assert!(if visible {
                            fraction > previous
                        } else {
                            fraction < previous
                        });
                        previous = fraction;
                        assert_eq!(content.size.width, px(shell.workspace.sidebar_width));
                        let quad = w.painted_quads().into_iter().find(|q| q.bounds == row);
                        if clip.right().as_f32() * w.scale_factor() <= row.left().0 {
                            assert!(quad.is_none(), "fully covered rows are culled");
                            return;
                        }
                        let quad = quad.expect("exposed sidebar row paints at its full size");
                        let alpha = quad.background.as_solid().unwrap().a;
                        assert!(
                            (alpha - 0.1 * fraction).abs() < 0.0001,
                            "painted alpha {alpha} follows {fraction}"
                        );
                        assert!(
                            (quad.content_mask.bounds.right().0
                                - main.left().as_f32() * w.scale_factor())
                            .abs()
                                < w.scale_factor()
                        );
                    })
                    .unwrap();
            }
            window
                .update(visual, |shell, _, cx| {
                    shell
                        .sidebar_slide
                        .as_ref()
                        .unwrap()
                        .elapsed(Duration::from_secs(1));
                    cx.notify();
                })
                .unwrap();
            visual
                .update_window(window.into(), |_, w, cx| {
                    w.simulate_next_frame(cx);
                    w.draw(cx).clear(cx);
                })
                .unwrap();
            window
                .update(visual, |shell, _, _| {
                    assert!(shell.sidebar_slide.is_none());
                    assert_eq!(shell.sidebar_fraction, if visible { 1. } else { 0. });
                })
                .unwrap();
        }
        // Reopening the sidebar reverses its closing motion without jumping its edge.
        window
            .update(visual, |shell, w, cx| {
                shell.toggle_sidebar(w, cx);
                shell
                    .sidebar_slide
                    .as_ref()
                    .unwrap()
                    .elapsed(Duration::from_millis(65));
                let before = shell
                    .sidebar_slide
                    .as_ref()
                    .unwrap()
                    .at(std::time::Instant::now())
                    .0;
                shell.toggle_sidebar(w, cx);
                assert!(shell.workspace.sidebar_visible);
                let after = shell
                    .sidebar_slide
                    .as_ref()
                    .unwrap()
                    .at(std::time::Instant::now())
                    .0;
                assert!((after - before).abs() < 0.01);
            })
            .unwrap();
        visual.update(|_, cx| cx.set_reduce_motion(true));
        visual
            .update_window(window.into(), |_, w, cx| {
                w.simulate_next_frame(cx);
                w.draw(cx).clear(cx);
            })
            .unwrap();
        window
            .update(visual, |shell, _, _| {
                assert!(shell.sidebar_slide.is_none());
                assert_eq!(shell.sidebar_fraction, 1.);
            })
            .unwrap();
    }
    #[gpui::test]
    fn switching_session_in_current_tab_keeps_its_slot_and_selection_settled(
        cx: &mut TestAppContext,
    ) {
        let (window, first, current) = same_project_window(cx);
        cx.update(|cx| {
            cx.set_reduce_motion(false);
            input::bind(cx);
            bind_keys(cx);
        });
        let next = window
            .update(cx, |shell, w, cx| {
                let mut summary = locked(&shell.store)
                    .unwrap()
                    .create("codex", &shell.workspace.project)
                    .unwrap();
                summary.has_messages = true;
                summary.updated_at = 3;
                let next = summary.id;
                shell.add(summary, cx);
                for (id, updated_at) in [(first, 1), (current, 2)] {
                    shell.sessions[&id].update(cx, |session, _| {
                        session.summary.has_messages = true;
                        session.summary.updated_at = updated_at;
                    });
                }
                shell.open(current, w, cx);
                next
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let original_button = visual.debug_bounds("new-tab").unwrap();
        for (step, expected) in [(-1, next), (1, current), (-1, next), (1, current)] {
            window
                .update(visual, |shell, w, cx| {
                    shell.workspace.preferences.tab_animations = true;
                    shell.step_session(step, true, w, cx);
                    assert_eq!(shell.workspace.active, Some(expected));
                    let tabs = shell.visible_tabs(cx);
                    assert_eq!(
                        tabs,
                        [TitleTab::Session(first), TitleTab::Session(expected)]
                    );
                    let (slots, moving) = shell.title_tab_motion.sync(
                        tabs.into_iter().map(|tab| (tab, ())).collect(),
                        true,
                        std::time::Instant::now(),
                    );
                    assert!(
                        !moving,
                        "replacing a session must not open or close a tab slot"
                    );
                    assert!(slots.iter().all(|slot| matches!(
                        slot,
                        crate::tab_motion::Slot::Live { open, .. } if *open == 1.
                    )));
                    assert_eq!(
                        shell.title_tab_selection.1,
                        Some(TitleTab::Session(expected))
                    );
                    assert!(
                        shell.title_tab_selection.0.is_none(),
                        "the selected slot did not change"
                    );
                })
                .unwrap();
            visual.run_until_parked();
            assert_eq!(visual.debug_bounds("new-tab").unwrap(), original_button);
        }
        // Also exercise the actual shortcuts from the focused composer.
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        for (key, expected) in [("up", next), ("down", current)] {
            visual.simulate_keystrokes(&format!("{modifier}-{key}"));
            visual.run_until_parked();
            window
                .update(visual, |shell, _, _| {
                    assert_eq!(shell.workspace.active, Some(expected));
                    assert_eq!(shell.workspace.tabs.len(), 2);
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn tab_project_and_session_keys_follow_monocode(cx: &mut TestAppContext) {
        let (window, a, b) = same_project_window(cx);
        cx.update(|cx| {
            input::bind(cx);
            bind_keys(cx);
        });
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        let active = |cx: &mut TestAppContext| {
            window
                .update(cx, |shell, _, _| shell.workspace.active)
                .unwrap()
        };
        let keys = |keys: String, cx: &mut TestAppContext| {
            cx.simulate_keystrokes(window.into(), &keys);
            cx.run_until_parked();
        };
        window
            .update(cx, |shell, w, cx| shell.focus_active(w, cx))
            .unwrap();
        // Tabs are [a, b]: slots, the last slot, and wrapping next/previous.
        keys(format!("{m}-1"), cx);
        assert_eq!(active(cx), Some(a));
        keys(format!("{m}-9"), cx);
        assert_eq!(active(cx), Some(b));
        keys(format!("{m}-}}"), cx);
        assert_eq!(active(cx), Some(a));
        keys(format!("{m}-shift-["), cx);
        assert_eq!(active(cx), Some(b));
        keys("ctrl-tab".into(), cx);
        assert_eq!(active(cx), Some(a));

        // An empty composer hands Cmd+Shift+Right to project navigation...
        let project = |cx: &mut TestAppContext| {
            window
                .update(cx, |shell, _, _| shell.workspace.project.clone())
                .unwrap()
        };
        let before = project(cx);
        keys(format!("{m}-shift-right"), cx);
        let after = project(cx);
        assert_ne!(after, before);
        // ...but with a draft it selects text, as in any field.
        window
            .update(cx, |shell, w, cx| {
                let id = shell.workspace.active.unwrap();
                let input = shell.sessions[&id].read(cx).input.clone();
                w.focus(&input.focus_handle(cx), cx);
                input.update(cx, |i, cx| i.replace_text_in_range(None, "draft", w, cx));
            })
            .unwrap();
        keys(format!("{m}-shift-right"), cx);
        assert_eq!(project(cx), after);

        // Close the other tabs, keeping the active one.
        keys(format!("{m}-alt-t"), cx);
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(shell.workspace.active_tab().unwrap())]
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn session_menu_copies_ids_through_its_submenu(cx: &mut TestAppContext) {
        let (window, a, _) = window(cx);
        cx.update(crate::popover::bind);
        window
            .update(cx, |shell, w, cx| {
                let anchor = Anchor::Point(gpui::point(px(40.), px(120.)));
                shell.open_menu(MenuTarget::Session(a), anchor, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        // Pin → Rename → Copy session ID, open it, then skip the harness ID
        // (this session has none) for the MonoCode one.
        cx.simulate_keystrokes(window.into(), "down down right down enter");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.menu.is_none()))
            .unwrap();
        let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        assert_eq!(copied, Some(a.to_string()));
    }
    #[gpui::test]
    fn desktop_database_sessions_load_and_copy_their_original_ids(cx: &mut TestAppContext) {
        let mut store = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/work/monocode").unwrap();
        let id = summary.id;
        let original = store.persisted_session_id(id).unwrap();
        store
            .write_rows(
                id,
                0,
                &[monocode_core::transcript::Row {
                    role: "user".into(),
                    text: "Existing desktop history".into(),
                    first: true,
                    sent_at: Some(123),
                }],
            )
            .unwrap();
        let summaries = store.sessions().unwrap();
        let mut state = Workspace::default();
        state.open(id);
        let saved = serde_json::to_string(&state).unwrap();
        let database = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    Shell::new(
                        database,
                        summaries,
                        "/work/monocode".into(),
                        Some(saved),
                        cx,
                    )
                })
            })
            .unwrap()
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        cx.update(crate::popover::bind);
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.workspace.active, Some(id));
                let transcript = shell.sessions[&id].read(cx).transcript.read(cx);
                assert_eq!(transcript.source_rows()[0].text, "Existing desktop history");
                shell.open_menu(
                    MenuTarget::Session(id),
                    Anchor::Point(gpui::point(px(40.), px(120.))),
                    w,
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "down down right down enter");
        cx.run_until_parked();
        let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        assert_eq!(copied, Some(original));
    }
    #[gpui::test]
    fn sidebar_hover_updates_labels_without_rebuilding_the_chat(cx: &mut TestAppContext) {
        let (window, _, active) = window(cx);
        let root = window.root(cx).unwrap();
        let transcript =
            root.read_with(cx, |s, cx| s.sessions[&active].read(cx).transcript.clone());
        let before = transcript.read_with(cx, |t, _| t.render_passes);
        cx.update_window(window.into(), |_, w, cx| {
            w.simulate_mouse_move(gpui::point(px(40.), px(136.)), cx)
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(root.read_with(cx, |s, _| s.sidebar_hover), Some("Inbox"));
        cx.update_window(window.into(), |_, w, cx| {
            w.simulate_mouse_move(gpui::point(px(40.), px(169.)), cx)
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(root.read_with(cx, |s, _| s.sidebar_hover), Some("Notes"));
        assert_eq!(transcript.read_with(cx, |t, _| t.render_passes), before);
    }
    #[gpui::test]
    fn many_blank_tabs_fit_without_creating_sidebar_conversations(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                for _ in 0..18 {
                    let summary = locked(&shell.store)
                        .unwrap()
                        .create("codex", "/work/monocode")
                        .unwrap();
                    let id = summary.id;
                    assert!(!summary.has_messages);
                    shell.add(summary, cx);
                    shell.workspace.open(id);
                }
                shell.invalidate_chrome(cx);
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                // The tabs share the strip instead of scrolling it.
                assert_eq!(
                    shell.tab_scroll.max_offset().x,
                    px(0.),
                    "viewport {:?}, strip {:?}, children {}, first {:?}, last {:?}",
                    w.viewport_size(),
                    shell.tab_scroll.bounds(),
                    shell.tab_scroll.children_count(),
                    shell.tab_scroll.bounds_for_item(0),
                    shell
                        .tab_scroll
                        .bounds_for_item(shell.workspace.tabs.len() - 1)
                );
                let id = shell.workspace.active.unwrap();
                let anchor = Anchor::Point(gpui::point(px(400.), px(30.)));
                shell.open_menu(MenuTarget::Tab(id), anchor, w, cx);
                // Closing every tab preserves one fresh blank tab, as in Monocode.
                for id in shell.workspace.tabs.clone() {
                    shell.close(id, w, cx);
                }
                assert_eq!(shell.workspace.tabs.len(), 1);
                assert!(!shell.tab_can_close(shell.workspace.tabs[0], cx));
            })
            .unwrap();
        cx.run_until_parked();
    }
    fn same_project_window(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |s, _| s.summary.cwd = "/work/monocode".into());
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        cx.run_until_parked();
        (window, a, b)
    }
    #[gpui::test]
    fn selected_file_reference_preserves_the_live_draft_and_requests_focus(
        cx: &mut TestAppContext,
    ) {
        let (window, _, active) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&active].update(cx, |chat, cx| {
                    chat.input.update(cx, |input, cx| {
                        input.set_text("Existing draft\n".into(), cx)
                    });
                });
                shell.add_to_chat(active, "@src/main.rs (lines 2-4)", cx);
                let chat = shell.sessions[&active].read(cx);
                assert_eq!(
                    chat.input.read(cx).content,
                    "Existing draft\n\n@src/main.rs (lines 2-4)\n\n"
                );
                assert_eq!(shell.pending_composer_focus, Some(active));
            })
            .unwrap();
    }
    pub(super) fn window(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
        let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut a = store.create("codex", "/work/another-project").unwrap();
        let b = store.create("claude", "/work/monocode").unwrap();
        store
            .write_rows(
                a.id,
                0,
                &[monocode_core::transcript::Row {
                    role: "assistant".into(),
                    text: "History loaded when the adjacent tab becomes active".into(),
                    first: true,
                    sent_at: None,
                }],
            )
            .unwrap();
        a.has_messages = true;
        let ids = (a.id, b.id);
        // Layout tests start from the rounded frame and opt into boxy.
        let mut state = Workspace {
            new_layout_boxy: false,
            ..Default::default()
        };
        state.add_project("/work/another-project");
        state.add_project("/work/monocode");
        state.open(a.id);
        state.open(b.id);
        let saved = serde_json::to_string(&state).unwrap();
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut shell =
                        Shell::new(store, vec![b, a], "/work/monocode".into(), Some(saved), cx);
                    // This shared fixture intentionally exposes both entities to
                    // interaction tests; production restoration stays lazy.
                    shell.ensure_session(ids.0, cx);
                    shell
                })
            })
            .unwrap()
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        (window, ids.0, ids.1)
    }
    #[gpui::test]
    fn leaving_the_original_layout_defaults_to_boxy_only_before_the_notice(
        cx: &mut TestAppContext,
    ) {
        let (window, _, _) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.new_layout_boxy = false;
                shell.workspace.new_layout_prompt_dismissed = false;
                shell.toggle_layout(cx);
                assert!(!shell.workspace.classic_layout);
                assert!(shell.workspace.new_layout_boxy);
                assert!(shell.workspace.new_layout_prompt_dismissed);

                shell.workspace.classic_layout = true;
                shell.workspace.new_layout_boxy = false;
                shell.toggle_layout(cx);
                assert!(!shell.workspace.new_layout_boxy);
            })
            .unwrap();
    }
    #[gpui::test]
    fn workspace_style_shortcut_only_switches_the_focused_layout(cx: &mut TestAppContext) {
        let (window, _, _) = window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.classic_layout = false;
                shell.workspace.new_layout_boxy = false;
                shell.toggle_workspace_style(cx);
                assert!(shell.workspace.new_layout_boxy);
                shell.toggle_workspace_style(cx);
                assert!(!shell.workspace.new_layout_boxy);

                shell.workspace.classic_layout = true;
                shell.toggle_workspace_style(cx);
                assert!(!shell.workspace.new_layout_boxy);
            })
            .unwrap();
    }
    #[gpui::test]
    fn slash_menu_stays_below_tabs_with_centered_and_docked_input(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, id) = window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for boxy in [false, true] {
            for height in [700., 500.] {
                visual.simulate_resize(gpui::size(px(1200.), px(height)));
                for has_messages in [false, true] {
                    window
                        .update(visual, |shell, window, cx| {
                            shell.workspace.new_layout_boxy = boxy;
                            shell.layout_changed(cx);
                            shell.open(id, window, cx);
                            shell.sessions[&id].update(cx, |view, cx| {
                                view.summary.has_messages = has_messages;
                                window.focus(&view.input.focus_handle(cx), cx);
                                view.input
                                    .update(cx, |input, cx| input.set_text("/".into(), cx));
                                cx.notify();
                            });
                        })
                        .unwrap();
                    visual.run_until_parked();
                    visual.refresh().unwrap();
                    let tabs = visual.debug_bounds("title-tab-strip").unwrap();
                    let menu = visual.debug_bounds("composer-completion-list").unwrap();
                    let composer = visual.debug_bounds("composer-box").unwrap();
                    assert!(
                        menu.top() >= tabs.bottom(),
                        "slash menu must leave the tabs visible"
                    );
                    assert!(menu.bottom() <= composer.top() - px(6.));
                    assert!((menu.left() - composer.left()).abs() <= px(2.));
                    assert!((menu.right() - composer.right()).abs() <= px(2.));
                }
            }
        }
    }

    #[gpui::test]
    fn pane_surfaces_follow_the_edge_at_a_constant_width(cx: &mut TestAppContext) {
        use std::time::Duration;
        let (window, _, id) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(id, w, cx);
                shell.toggle_right_panel(w, cx);
            })
            .unwrap();
        for ms in [30, 65, 110, 170] {
            window
                .update(cx, |shell, _, cx| {
                    shell
                        .right_slide
                        .as_ref()
                        .unwrap()
                        .freeze_elapsed(Duration::from_millis(ms));
                    cx.notify();
                })
                .unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            })
            .unwrap();
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            let clip = visual.debug_bounds("right-panel-frame").unwrap();
            let content = visual.debug_bounds("right-panel-content").unwrap();
            assert!((content.left() - clip.left()).abs() < px(2.));
            let full = window
                .update(visual, |shell, _, _| shell.workspace.right_panel_width)
                .unwrap();
            assert!((content.size.width - px(full - 1.)).abs() < px(1.));
            assert!(content.size.width > clip.size.width);
        }
        window
            .update(cx, |shell, w, cx| {
                shell
                    .right_slide
                    .as_ref()
                    .unwrap()
                    .elapsed(Duration::from_secs(1));
                shell.open_review(
                    crate::review::ReviewTab::Changes {
                        cwd: "/work/monocode".into(),
                        path: None,
                        staged: None,
                    },
                    w,
                    cx,
                );
                shell
                    .review_slide
                    .as_ref()
                    .unwrap()
                    .1
                    .freeze_elapsed(Duration::ZERO);
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
        let mut settled_width = None;
        for ms in [30, 65, 110, 170] {
            window
                .update(cx, |shell, _, cx| {
                    shell
                        .review_slide
                        .as_ref()
                        .unwrap()
                        .1
                        .freeze_elapsed(Duration::from_millis(ms));
                    cx.notify();
                })
                .unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            })
            .unwrap();
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            let clip = visual.debug_bounds("review-frame").unwrap();
            let content = visual.debug_bounds("review-content").unwrap();
            assert!((content.left() - clip.left()).abs() < px(2.));
            let expected = *settled_width.get_or_insert(content.size.width);
            assert!((content.size.width - expected).abs() < px(1.));
            assert!(content.size.width > clip.size.width);
        }
        cx.update(|cx| cx.set_reduce_motion(true));
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.right_slide.is_none());
                assert!(shell.review_slide.is_none());
            })
            .unwrap();
    }

    #[cfg(target_os = "macos")]
    #[gpui::test]
    #[ignore = "Manual CoreText CPU frame-cost measurement; opens no native window"]
    fn pane_motion_frame_cost_with_long_history(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        let cx = &mut TestAppContext::build_with_text_system(
            cx.dispatcher.clone(),
            Some("pane_motion_frame_cost"),
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, _, id) = window(cx);
        let transcript = window.update(cx, |shell, w, cx| {
            shell.open(id, w, cx);
            let session = shell.sessions[&id].clone();
            session.update(cx, |session, cx| {
                session.summary.has_messages = true;
                let mut history = monocode_core::transcript::Transcript::default();
                for _ in 0..10_000 {
                    history.begin("assistant");
                    history.append("A conversation with enough text to wrap while its pane changes width.\nA second line of native text.");
                }
                session.transcript.update(cx, |view, cx| view.patch(0, history.rows, cx));
                cx.notify();
                session.transcript.clone()
            })
        }).unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
        let mut frames = Vec::new();
        for opening in [true, false] {
            window
                .update(cx, |shell, w, cx| shell.toggle_right_panel(w, cx))
                .unwrap();
            for frame in 1..=40 {
                let root = window.root(cx).unwrap();
                let before = root.read_with(cx, |shell, _| shell.render_passes);
                let start = Instant::now();
                window
                    .update(cx, |shell, _, cx| {
                        if let Some(slide) = &shell.right_slide {
                            slide.elapsed(Duration::from_secs_f32(frame as f32 / 120.));
                        }
                        cx.notify();
                    })
                    .unwrap();
                if root.read_with(cx, |shell, _| shell.render_passes) == before {
                    cx.update_window(window.into(), |_, window, cx| {
                        window.simulate_next_frame(cx);
                        window.draw(cx).clear(cx);
                    })
                    .unwrap();
                }
                assert_eq!(
                    root.read_with(cx, |shell, _| shell.render_passes),
                    before + 1,
                    "measure one rendered frame per sample"
                );
                frames.push(start.elapsed().as_secs_f64() * 1000.);
                let rendered = transcript.read_with(cx, |view, _| view.rendered);
                assert!(
                    rendered > 0 && rendered < 100,
                    "expected visible history work, got {rendered}"
                );
            }
            window
                .update(cx, |shell, _, _| {
                    assert_eq!(shell.workspace.right_panel_open, opening)
                })
                .unwrap();
        }
        frames.sort_by(f64::total_cmp);
        eprintln!(
            "CoreText, 10,000 messages, {} simulated 120Hz frames: CPU p50={:.2}ms p95={:.2}ms max={:.2}ms (excludes presentation/GPU)",
            frames.len(),
            frames[frames.len() / 2],
            frames[frames.len() * 95 / 100],
            frames.last().unwrap()
        );
    }

    #[gpui::test]
    fn explorer_files_share_the_review_pane_and_dirty_close_can_cancel_or_save(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        use gpui::EntityInputHandler;
        let path =
            std::env::temp_dir().join(format!("monocode-file-pane-{}.rs", std::process::id()));
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let (window, _, id) = window(cx);
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(id, w, cx);
                shell.open_review(
                    crate::review::ReviewTab::File {
                        cwd: "/tmp".into(),
                        path: path.to_string_lossy().into_owned(),
                    },
                    w,
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                let pane = shell.reviews[&id].clone();
                pane.update(cx, |pane, cx| {
                    pane.navigate_file(1, 4, cx);
                    let editor = pane.active_editor().unwrap();
                    let input = editor.read(cx).input.clone();
                    input.update(cx, |i, cx| i.replace_text_in_range(None, " edited", w, cx));
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.unsaved_files(cx));
                shell.reviews[&id].update(cx, |pane, cx| pane.close_active(cx));
            })
            .unwrap();
        cx.refresh().unwrap();
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.reviews[&id].read(cx).state.tabs.len(), 1);
                assert!(shell.unsaved_files(cx));
                shell.reviews[&id].update(cx, |pane, cx| pane.close_active(cx));
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.simulate_prompt_answer("Save");
        cx.run_until_parked();
        assert!(std::fs::read_to_string(&path).unwrap().contains(" edited"));
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.reviews.contains_key(&id));
                assert!(!shell.unsaved_files(cx));
            })
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[gpui::test]
    fn panes_split_resize_and_take_drops_from_panes_tabs_and_the_sidebar(cx: &mut TestAppContext) {
        use gpui::{Modifiers, point};
        let (window, a, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.sessions[&b].update(cx, |chat, _| chat.summary.has_messages = true);
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let count_selector: &'static str =
            Box::leak(format!("title-tab-pane-count-{b}").into_boxed_str());
        let close_selector: &'static str =
            Box::leak(format!("close-title-tab-{b}").into_boxed_str());
        assert!(
            cx.debug_bounds(close_selector).is_some(),
            "a single pane can still be closed"
        );
        assert!(
            cx.debug_bounds(count_selector).is_none(),
            "a single pane has no count badge"
        );
        cx.simulate_keystrokes("cmd-d");
        cx.run_until_parked();
        let new = window
            .update(cx, |shell, _, _| {
                let new = shell.workspace.active.unwrap();
                assert_eq!(shell.workspace.tabs, [a, b]);
                assert_eq!(shell.workspace.layout(b).leaf_ids(), [b, new]);
                new
            })
            .unwrap();
        window
            .update(cx, |shell, _, cx| {
                // Two sessions, one line each; the new pane has focus.
                let copy = shell.tab_copy(b, cx);
                let keys: Vec<_> = copy.panes.iter().map(|p| p.key).collect();
                assert_eq!(keys, [(b, false), (new, false)]);
                assert_eq!(copy.focus, 1);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert!(
            cx.debug_bounds(count_selector).is_some(),
            "split tabs show their pane count"
        );
        let left = cx
            .debug_bounds(Box::leak(format!("pane-{b}").into_boxed_str()))
            .unwrap();
        let right = cx
            .debug_bounds(Box::leak(format!("pane-{new}").into_boxed_str()))
            .unwrap();
        assert!((left.right() - right.left()).abs() < px(1.));
        assert!((left.size.width - right.size.width).abs() < px(2.));

        // The sash between them drags the split.
        let sash = point(left.right(), left.center().y);
        cx.simulate_mouse_down(sash, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            sash - point(px(100.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            sash - point(px(100.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                let leaves = shell.workspace.layout(b).leaves();
                assert!(leaves[0].rect.w < 0.45, "{leaves:?}");
            })
            .unwrap();

        // A sidebar session dropped on a pane's lower half stacks under it,
        // and the tab that held it closes.
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&a].update(cx, |s, _| {
                    s.summary.cwd = "/work/monocode".into();
                    s.summary.has_messages = true;
                });
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        cx.run_until_parked();
        let row = cx
            .debug_bounds(Box::leak(format!("sidebar-session-{a}").into_boxed_str()))
            .unwrap()
            .center();
        let below = point(right.center().x, right.bottom() - px(20.));
        cx.simulate_mouse_down(row, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            row + point(px(20.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(below, MouseButton::Left, Modifiers::default());
        window
            .update(cx, |shell, _, _| {
                let drag = shell.pane_drag.as_ref().unwrap();
                assert_eq!(drag.over, Some((new, Edge::Bottom)));
            })
            .unwrap();
        cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs, [b]);
                assert_eq!(shell.workspace.layout(b).leaf_ids(), [b, new, a]);
                assert_eq!(shell.workspace.active, Some(a));
                assert!(shell.pane_drag.is_none());
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        // Its header drags it up onto the title strip: a tab of its own.
        let grip = cx
            .debug_bounds(Box::leak(format!("drag-session-pane-{a}").into_boxed_str()))
            .unwrap()
            .center();
        let strip = cx
            .debug_bounds(Box::leak(format!("title-tab-{b}").into_boxed_str()))
            .unwrap();
        let before_b = point(strip.left() + px(10.), strip.center().y);
        cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            grip + point(px(0.), px(-20.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(before_b, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(before_b, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs, [a, b]);
                assert_eq!(shell.workspace.layout(b).leaf_ids(), [b, new]);
                assert_eq!(shell.workspace.active, Some(a));
            })
            .unwrap();

        // A tab press whose release the title bar swallowed stays inert:
        // dragging a pane below never drags that tab.
        let pressed = cx
            .debug_bounds(Box::leak(format!("title-tab-{b}").into_boxed_str()))
            .unwrap()
            .center();
        window
            .update(cx, |shell, _, cx| {
                let event = MouseDownEvent {
                    position: pressed,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                shell.start_tab_drag(TitleTab::Session(b), &event, cx);
                assert!(shell.tab_drag.is_some());
            })
            .unwrap();
        cx.simulate_mouse_move(
            pressed + point(px(0.), px(300.)),
            None,
            Modifiers::default(),
        );
        window
            .update(cx, |shell, _, _| assert!(shell.tab_drag.is_none()))
            .unwrap();
        // A title tab pulled down onto a pane's left half moves in beside it.
        window
            .update(cx, |shell, w, cx| shell.open(b, w, cx))
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let tab = cx
            .debug_bounds(Box::leak(format!("title-tab-{a}").into_boxed_str()))
            .unwrap()
            .center();
        let pane = cx
            .debug_bounds(Box::leak(format!("pane-{b}").into_boxed_str()))
            .unwrap();
        let left_half = point(pane.left() + px(20.), pane.center().y);
        cx.simulate_mouse_down(tab, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            point(tab.x, px(TITLE_BAR_HEIGHT + PANE_DROP_PULL + 10.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        // Pulled out of the strip, sideways motion no longer slides the tab.
        cx.simulate_mouse_move(
            point(
                tab.x + px(300.),
                px(TITLE_BAR_HEIGHT + PANE_DROP_PULL + 10.),
            ),
            MouseButton::Left,
            Modifiers::default(),
        );
        window
            .update(cx, |shell, _, _| {
                let drag = shell.tab_drag.as_ref().unwrap();
                assert!(drag.armed);
                assert_eq!((drag.offset, drag.to), (px(0.), drag.from));
                assert!(shell.pane_drag.is_some());
            })
            .unwrap();
        cx.simulate_mouse_move(left_half, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(left_half, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs, [b]);
                assert_eq!(shell.workspace.layout(b).leaf_ids(), [a, b, new]);
                assert_eq!(shell.workspace.active, Some(a));
            })
            .unwrap();

        // ⌘W closes the focused pane only; its neighbour takes focus.
        cx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs, [b]);
                assert_eq!(shell.workspace.layout(b).leaf_ids(), [b, new]);
                assert_eq!(shell.workspace.active, Some(b));
            })
            .unwrap();
    }
    #[gpui::test]
    fn a_split_pane_slides_in_from_its_edge_then_settles(cx: &mut TestAppContext) {
        let (window, new, b) = same_project_window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                // Freeze before the first paint: keyboard simulation can draw
                // enough slow frames for the animation to finish on CI. The
                // shortcut itself is covered by the split-and-drag test above.
                let before = shell.active_leaves();
                shell.workspace.split(new, SplitDir::Right);
                shell.begin_pane_motion(before, cx);
                shell
                    .pane_motion
                    .as_ref()
                    .unwrap()
                    .slide
                    .freeze_elapsed(std::time::Duration::from_millis(16));
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        let new = window
            .update(cx, |shell, _, _| {
                let new = shell.workspace.active.unwrap();
                let motion = shell.pane_motion.as_ref().expect("the split animates");
                assert_eq!(motion.entering.get(&new), Some(&Edge::Right));
                assert_eq!(motion.from[&b].w, 1.);
                new
            })
            .unwrap();
        let entering = cx
            .debug_bounds(Box::leak(format!("pane-{new}").into_boxed_str()))
            .unwrap();
        let pane = cx
            .debug_bounds(Box::leak(format!("pane-{b}").into_boxed_str()))
            .unwrap();
        // Mid-slide: the chat still holds most of the width.
        assert!(
            entering.size.width < pane.size.width,
            "{entering:?} {pane:?}"
        );
        window
            .update(cx, |shell, _, cx| {
                shell
                    .pane_motion
                    .as_ref()
                    .unwrap()
                    .slide
                    .elapsed(std::time::Duration::from_secs(1));
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.pane_motion.is_none()))
            .unwrap();
        let entering = cx
            .debug_bounds(Box::leak(format!("pane-{new}").into_boxed_str()))
            .unwrap();
        let pane = cx
            .debug_bounds(Box::leak(format!("pane-{b}").into_boxed_str()))
            .unwrap();
        assert!((entering.size.width - pane.size.width).abs() < px(2.));
    }
    #[gpui::test]
    fn a_closed_pane_slides_out_to_its_edge_while_its_neighbour_grows(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-d");
        cx.run_until_parked();
        cx.update(|_, cx| cx.set_reduce_motion(false));
        let new = window
            .update(cx, |shell, w, cx| {
                let new = shell.workspace.active.unwrap();
                shell.close_pane(new, w, cx);
                let motion = shell.pane_motion.as_ref().expect("closing animates");
                assert_eq!(motion.header, Some(false));
                assert_eq!(motion.leaving.len(), 1);
                assert_eq!(
                    (motion.leaving[0].0.id, motion.leaving[0].1),
                    (new, Edge::Right)
                );
                assert_eq!(shell.workspace.layout(b).leaf_ids(), [b]);
                new
            })
            .unwrap();
        cx.refresh().unwrap();
        assert!(
            cx.debug_bounds(Box::leak(format!("split-header-{new}").into_boxed_str()))
                .is_some(),
            "still painted while it slides out"
        );
        window
            .update(cx, |shell, _, _| {
                // Midway it has slid half its width toward the right edge,
                // and the lone pane has grown over the same distance.
                let motion = shell.pane_motion.as_ref().unwrap();
                let (leaf, _, to) = motion.leaving[0];
                let mid = leaf.rect.lerp(to, 0.5);
                let grown = motion.from[&b].lerp(Rect::FULL, 0.5);
                assert!((mid.x - (grown.x + grown.w)).abs() < 1e-5);
                assert!((mid.w - 0.25).abs() < 1e-5);
            })
            .unwrap();
        window
            .update(cx, |shell, _, cx| {
                let motion = shell.pane_motion.as_ref().unwrap();
                motion.slide.elapsed(std::time::Duration::from_secs(1));
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.pane_motion.is_none()))
            .unwrap();
        assert!(
            cx.debug_bounds(Box::leak(format!("pane-{new}").into_boxed_str()))
                .is_none()
        );
    }
    #[gpui::test]
    fn closing_session_header_stays_hidden_on_the_settling_frame(cx: &mut TestAppContext) {
        assert_closed_header_stays_hidden(cx, false);
    }

    #[gpui::test]
    fn closing_file_header_stays_hidden_on_the_settling_frame(cx: &mut TestAppContext) {
        assert_closed_header_stays_hidden(cx, true);
    }

    fn assert_closed_header_stays_hidden(cx: &mut TestAppContext, file: bool) {
        use std::time::Duration;
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, session, other) = same_project_window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(session, w, cx);
                if file {
                    shell.open_review(
                        crate::review::ReviewTab::File {
                            cwd: "/work/monocode".into(),
                            path: "/tmp/header-close.rs".into(),
                        },
                        w,
                        cx,
                    );
                } else {
                    shell.workspace.place(other, session, Edge::Right);
                    shell.open(session, w, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let header = Box::leak(format!("split-header-{session}").into_boxed_str());
        assert!(visual.debug_bounds(header).is_some());
        visual.update(|_, cx| cx.set_reduce_motion(false));
        window
            .update(visual, |shell, w, cx| {
                if file {
                    shell.close_review(session, cx);
                } else {
                    shell.close_pane(other, w, cx);
                }
            })
            .unwrap();

        // Inspect the exact frame that consumes the finished animation. A
        // later refresh would hide a one-frame return of the full header.
        visual
            .update_window(window.into(), |_, w, cx| {
                let root = w.root::<Shell>().flatten().unwrap();
                root.update(cx, |shell, _| {
                    if file {
                        shell
                            .review_slide
                            .as_ref()
                            .unwrap()
                            .1
                            .elapsed(Duration::from_secs(1));
                    } else {
                        shell
                            .pane_motion
                            .as_ref()
                            .unwrap()
                            .slide
                            .elapsed(Duration::from_secs(1));
                    }
                });
                w.refresh();
                w.draw(cx).clear(cx);
            })
            .unwrap();
        assert!(
            visual.debug_bounds(header).is_none(),
            "the settled frame must already omit the disappearing header"
        );
        let transcript = window
            .update(visual, |shell, _, cx| {
                shell.sessions[&session].read(cx).transcript.clone()
            })
            .unwrap();
        let settled = transcript.read_with(visual, |t, _| t.list.viewport_bounds());
        visual
            .update_window(window.into(), |_, w, cx| {
                w.refresh();
                w.draw(cx).clear(cx);
            })
            .unwrap();
        assert!(visual.debug_bounds(header).is_none());
        assert_eq!(
            transcript.read_with(visual, |t, _| t.list.viewport_bounds()),
            settled,
            "the transcript must not jump after the closing animation settles"
        );
    }

    #[gpui::test]
    fn a_crowding_pane_collapses_the_sidebar_and_its_room_brings_it_back(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(1000.), px(800.)));
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.workspace.sidebar_visible);
                assert_eq!(shell.viewport_width, 1000.);
            })
            .unwrap();
        cx.simulate_keystrokes("cmd-d");
        cx.run_until_parked();
        let new = window
            .update(cx, |shell, _, cx| {
                // Two panes beside a 260px sidebar would be ~365px each.
                assert!(!shell.workspace.sidebar_visible);
                assert!(shell.narrowest_chat(false, cx) >= MIN_CHAT_WIDTH);
                shell.workspace.active.unwrap()
            })
            .unwrap();
        window
            .update(cx, |shell, w, cx| shell.close_pane(new, w, cx))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.workspace.sidebar_visible))
            .unwrap();
        // A sidebar the user hid stays hidden when room comes back.
        cx.simulate_keystrokes("cmd-d");
        cx.run_until_parked();
        // Collapsed for the split; shown, then hidden again by hand.
        cx.simulate_keystrokes("cmd-b cmd-b");
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert!(!shell.workspace.sidebar_visible);
                let pane = shell.workspace.active.unwrap();
                shell.close_pane(pane, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(!shell.workspace.sidebar_visible))
            .unwrap();
    }
    #[gpui::test]
    fn the_sidebar_follows_the_room_however_it_changes(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(1400.), px(800.)));
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        // Wide enough for two panes beside the sidebar.
        cx.simulate_keystrokes("cmd-d");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.workspace.sidebar_visible))
            .unwrap();
        // Narrowing the window crowds the panes; the sidebar makes room.
        cx.simulate_resize(gpui::size(px(1000.), px(800.)));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(!shell.workspace.sidebar_visible))
            .unwrap();
        // Widening it again brings the sidebar back.
        cx.simulate_resize(gpui::size(px(1400.), px(800.)));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.workspace.sidebar_visible))
            .unwrap();
        // Shown by hand while crowded, it stays until the room changes again.
        cx.simulate_resize(gpui::size(px(1000.), px(800.)));
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-b");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.workspace.sidebar_visible))
            .unwrap();
        // Hidden by hand, it stays hidden however wide the window grows.
        cx.simulate_keystrokes("cmd-b");
        cx.simulate_resize(gpui::size(px(1600.), px(800.)));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(!shell.workspace.sidebar_visible))
            .unwrap();
    }
    #[gpui::test]
    fn reordered_tabs_slide_into_place_instead_of_jumping(cx: &mut TestAppContext) {
        use gpui::{Modifiers, point};
        let (window, a, b) = same_project_window(cx);
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let first = cx
            .debug_bounds(Box::leak(format!("title-tab-{a}").into_boxed_str()))
            .unwrap();
        let second = cx
            .debug_bounds(Box::leak(format!("title-tab-{b}").into_boxed_str()))
            .unwrap();
        let slot = (second.left() - first.left()).as_f32();
        let grab = first.center();
        cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            grab + point(px(slot * 0.8), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                // The neighbour makes room by easing toward the vacated slot.
                let shift = shell.tab_shifts[&TitleTab::Session(b)];
                assert_eq!(shift.to, -slot);
                assert!(shift.at(shift.start).0 == 0.);
                let (later, _) = shift.at(shift.start + std::time::Duration::from_millis(80));
                assert!(later < 0. && later > -slot);
            })
            .unwrap();
        cx.simulate_mouse_up(
            grab + point(px(slot * 0.8), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs, [b, a]);
                // Released short of its slot, the tab glides the rest of
                // the way rather than snapping there.
                let shift = shell.tab_shifts[&TitleTab::Session(a)];
                assert!((shift.from - (-slot * 0.2)).abs() < 2., "{}", shift.from);
                assert_eq!(shift.to, 0.);
                let end = shift.start + std::time::Duration::from_millis(200);
                assert_eq!(shift.at(end), (0., true));
            })
            .unwrap();
    }
    #[gpui::test]
    fn the_terminal_dock_toggles_adds_closes_and_moves(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            crate::terminal::bind(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("terminal-dock").is_none());
        cx.simulate_keystrokes("cmd-j");
        cx.run_until_parked();
        let project = window
            .update(cx, |shell, w, cx| {
                let project = shell.workspace.project.clone();
                let dock = &shell.workspace.docks[&project];
                assert!(dock.open);
                assert_eq!(dock.terminals.as_slice(), std::slice::from_ref(&project));
                assert_eq!(shell.terminals[&project].len(), 1);
                // The new terminal has the keys.
                assert_eq!(shell.dock_focused(w, cx), Some(0));
                project
            })
            .unwrap();
        let dock = cx.debug_bounds("terminal-dock").unwrap();
        assert!((dock.size.height - px(220.)).abs() < px(2.));
        // ⌘T in the dock opens another terminal, not a session tab.
        let tabs = window.update(cx, |s, _, _| s.workspace.tabs.len()).unwrap();
        cx.simulate_keystrokes("cmd-t");
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.workspace.tabs.len(), tabs);
                assert_eq!(shell.workspace.docks[&project].terminals.len(), 2);
                assert_eq!(shell.workspace.docks[&project].active, 1);
                assert_eq!(shell.dock_focused(w, cx), Some(1));
            })
            .unwrap();
        assert!(cx.debug_bounds("terminal-tab-1").is_some());
        // ⌘W closes the focused terminal; the session tab stays.
        cx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.tabs.len(), tabs);
                assert_eq!(shell.workspace.docks[&project].terminals.len(), 1);
                assert_eq!(shell.terminals[&project].len(), 1);
            })
            .unwrap();
        // Docked right, it takes width: a narrow window gives up its sidebar.
        cx.simulate_resize(gpui::size(px(1000.), px(800.)));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.docks.get_mut(&project).unwrap().size = 360.;
                shell.set_dock_side(DockSide::Right, cx);
                assert!(!shell.workspace.sidebar_visible);
            })
            .unwrap();
        cx.run_until_parked();
        let dock = cx.debug_bounds("terminal-dock").unwrap();
        assert!((dock.size.width - px(360.)).abs() < px(2.), "{dock:?}");
        // ⌘J hides it and the composer takes the keys back.
        window
            .update(cx, |shell, w, cx| shell.focus_terminal(w, cx))
            .unwrap();
        cx.simulate_keystrokes("cmd-j");
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert!(!shell.workspace.docks[&project].open);
                assert_eq!(shell.dock_focused(w, cx), None);
                assert!(shell.workspace.sidebar_visible);
            })
            .unwrap();
        assert!(cx.debug_bounds("terminal-dock").is_none());
    }
    #[gpui::test]
    fn closing_the_last_review_tab_removes_the_pane(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open(b, window, cx);
                shell.open_review(
                    crate::review::ReviewTab::Commit {
                        cwd: "/work/monocode".into(),
                        sha: "abcdef1".into(),
                        short_sha: "abcdef1".into(),
                        subject: "Fix".into(),
                    },
                    window,
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.reviews.contains_key(&b));
                assert_eq!(shell.workspace.reviews[&b].tabs.len(), 1);
                // A chat with a file beside it: the chat's line, then the
                // file's, which heads the tab while it has focus.
                let label = shell.workspace.reviews[&b].tabs[0].label();
                let copy = shell.tab_copy(b, cx);
                assert_eq!(copy.headline, label);
                let keys: Vec<_> = copy.panes.iter().map(|p| p.key).collect();
                assert_eq!((keys, copy.focus), (vec![(b, false), (b, true)], 1));
                shell.review_focused = false;
                shell.sessions[&b].update(cx, |s, _| s.summary.title = "Fix login".into());
                let copy = shell.tab_copy(b, cx);
                assert_eq!(copy.headline, "Fix login");
                assert_eq!((copy.panes[0].title.as_str(), copy.focus), ("Fix login", 0));
                let review = shell.reviews[&b].clone();
                review.update(cx, |r, cx| r.close_active(cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.reviews.contains_key(&b));
                assert!(!shell.workspace.reviews.contains_key(&b));
                // Rendering again never rebuilds an empty pane.
                assert!(shell.review_for(b, cx).is_none());
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(!shell.reviews.contains_key(&b)))
            .unwrap();
    }
    #[gpui::test]
    fn closing_and_reopening_tabs_preserves_busy_session_draft_and_history(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.sessions[&b].update(cx, |v, cx| {
                    v.busy = true;
                    v.input.update(cx, |i, cx| {
                        i.replace_text_in_range(None, "A retained draft 🙂", w, cx)
                    });
                });
                shell.close(b, w, cx);
                assert_eq!(shell.workspace.active, None);
                assert_eq!(shell.workspace.project, "/work/monocode");
                assert!(shell.sessions[&b].read(cx).busy);
                shell.open(a, w, cx);
                shell.open(b, w, cx);
                assert_eq!(shell.workspace.active, Some(b));
                assert_eq!(
                    shell.sessions[&b].read(cx).input.read(cx).content.as_ref(),
                    "A retained draft 🙂"
                );
                shell.close(a, w, cx);
                shell.close(b, w, cx);
                assert!(shell.workspace.tabs.is_empty());
                assert!(shell.workspace.active.is_none());
                assert_eq!(shell.sessions.len(), 2);
                shell.open(b, w, cx);
                assert_eq!(shell.workspace.tabs, vec![b]);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let saved = locked(&shell.store).unwrap().ui_state().unwrap().unwrap();
                let saved: Workspace = serde_json::from_str(&saved).unwrap();
                assert_eq!(saved.tabs, vec![b]);
                assert_eq!(
                    shell.sessions[&a]
                        .read(cx)
                        .transcript
                        .read(cx)
                        .list
                        .item_count(),
                    4,
                    "reopening a closed session must load its receipt, answer, actions and footer"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn typing_and_streaming_do_not_rebuild_sidebar_or_tabs(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        let root = window.root(cx).unwrap();
        let chrome = root.read_with(cx, |s, _| (s.sidebar_frame.clone(), s.tabs_frame.clone()));
        let before = (
            chrome.0.read_with(cx, |v, _| v.renders),
            chrome.1.read_with(cx, |v, _| v.renders),
        );
        assert!(
            before.0 > 0 && before.1 > 0,
            "the cached surfaces must have actually rendered"
        );
        window
            .update(cx, |shell, w, cx| {
                shell.sessions[&b].update(cx, |v, cx| {
                    v.input.update(cx, |i, cx| {
                        i.replace_text_in_range(None, "Local keystrokes", w, cx)
                    });
                    v.transcript.update(cx, |t, cx| {
                        t.patch(
                            0,
                            vec![monocode_core::transcript::Row {
                                role: "assistant".into(),
                                text: "Streaming output".into(),
                                first: true,
                                sent_at: None,
                            }],
                            cx,
                        )
                    });
                    cx.notify();
                })
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert_eq!(
            chrome.0.read_with(cx, |v, _| v.renders),
            before.0,
            "chat changes must reuse the sidebar"
        );
        assert_eq!(
            chrome.1.read_with(cx, |v, _| v.renders),
            before.1,
            "chat changes must reuse the tabs"
        );
    }

    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn title_particle_frames_only_invalidate_the_sidebar(cx: &mut TestAppContext) {
        let (window, _, id) = window(cx);
        cx.update(|cx| cx.set_reduce_motion(false));
        // Blank tabs deliberately have no sidebar card. Mount a conversation
        // before changing its title, so this exercises an existing label.
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&id].update(cx, |session, cx| {
                    session.summary.has_messages = true;
                    session.rename("Original session title".into(), cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&id].update(cx, |session, cx| {
                    session.rename("Matching the generated title effect".into(), cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        // Deliver the title-change frame's deferred layout/hover work before
        // measuring the subsequent decorative animation frames in isolation.
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
        cx.run_until_parked();
        let (sidebar, tabs, transcript) = window
            .read_with(cx, |shell, cx| {
                (
                    shell.sidebar_frame.clone(),
                    shell.tabs_frame.clone(),
                    shell.sessions[&id].read(cx).transcript.clone(),
                )
            })
            .unwrap();
        let before = (
            sidebar.read_with(cx, |view, _| view.renders),
            tabs.read_with(cx, |view, _| view.renders),
            transcript.read_with(cx, |view, _| view.render_passes),
        );
        for _ in 0..3 {
            cx.update_window(window.into(), |_, window, cx| {
                assert!(
                    window.simulate_next_frame(cx) > 0,
                    "title requested its next frame"
                );
                window.draw(cx).clear(cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
        assert!(sidebar.read_with(cx, |view, _| view.renders) > before.0);
        assert_eq!(tabs.read_with(cx, |view, _| view.renders), before.1);
        assert_eq!(
            transcript.read_with(cx, |view, _| view.render_passes),
            before.2
        );
    }
}
