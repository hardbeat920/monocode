//! The app shell. Port of the layout in src/app/App.tsx (the root around
//! lines 10545-11078) and the chrome in src/app/shell: the project rail or
//! compact rail, the session sidebar, and the main column with the title bar,
//! the pane area, and the usage footer.
//!
//! Each region renders from its own module through an `impl Shell` block, so
//! later work can swap the mock data for engine entities one region at a time.

mod footer;
mod main_pane;
mod project_rail;
mod sidebar;
mod title_bar;

use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};
use gpui_component::input::InputState;
use monocode_ui::appearance::{
    PROJECT_RAIL_WIDTH_DEFAULT, PROJECT_RAIL_WIDTH_MAX, PROJECT_RAIL_WIDTH_MIN,
    SESSION_SIDEBAR_WIDTH_DEFAULT, SESSION_SIDEBAR_WIDTH_MAX, SESSION_SIDEBAR_WIDTH_MIN,
};
use monocode_ui::widgets::{MenuEntry, MenuItem, context_menu, menu, toast_stack};
use monocode_ui::{Theme, u};

use crate::mock::ShellData;

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

/// How the shell starts, for `--view` variants.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShellOptions {
    pub project_rail_open: bool,
    pub compact_rail: bool,
    pub session_sidebar_open: bool,
    /// Opens the session context menu at this point, for screenshots.
    pub demo_menu: Option<(f32, f32)>,
}

impl ShellOptions {
    pub fn full() -> Self {
        Self {
            project_rail_open: true,
            compact_rail: false,
            session_sidebar_open: true,
            demo_menu: None,
        }
    }
}

pub struct Shell {
    data: ShellData,
    project_rail_open: bool,
    /// `collapsedProjectRailMode === "compact"`: show the 48px rail when the
    /// project rail is closed.
    compact_rail: bool,
    session_sidebar_open: bool,
    rail_width: f32,
    sidebar_width: f32,
    sidebar_tab: SidebarTab,
    active_tab: usize,
    active_session: Option<usize>,
    selected_session: Option<usize>,
    resize: Option<Resize>,
    /// Armed by a press on a drag region; the first move hands the drag to
    /// the window manager.
    drag_armed: bool,
    session_search: Entity<InputState>,
    session_menu: Option<Point<Pixels>>,
}

impl Shell {
    pub fn new(options: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions"));
        Self {
            data: ShellData::sample(),
            project_rail_open: options.project_rail_open,
            compact_rail: options.compact_rail,
            session_sidebar_open: options.session_sidebar_open,
            rail_width: PROJECT_RAIL_WIDTH_DEFAULT,
            sidebar_width: SESSION_SIDEBAR_WIDTH_DEFAULT,
            sidebar_tab: SidebarTab::Sessions,
            active_tab: 0,
            active_session: Some(2),
            selected_session: None,
            resize: None,
            drag_armed: false,
            session_search,
            session_menu: options
                .demo_menu
                .map(|(x, y)| gpui::point(gpui::px(x), gpui::px(y))),
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
            menu("session-menu", entries).on_pick(move |_, _, cx| {
                pick.update(cx, |this, cx| {
                    this.session_menu = None;
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
        // On macOS the compact rail moves the title bar above everything, so
        // the traffic lights sit in it (`compactTitleBar` in App.tsx).
        let compact_title_bar = cfg!(target_os = "macos") && self.compact_rail_visible();
        let rail = if self.project_rail_open {
            Some(self.render_project_rail(window, cx).into_any_element())
        } else if self.compact_rail_visible() {
            Some(self.render_compact_rail(cx).into_any_element())
        } else {
            None
        };
        let sidebar = self
            .session_sidebar_open
            .then(|| self.render_sidebar(window, cx).into_any_element());
        let mut main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(c.body_glass);
        if !compact_title_bar {
            main = main.child(self.render_title_bar(cx));
        }
        let main = main
            .child(self.render_main_pane(cx))
            .child(self.render_footer(cx));
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
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up));
        if compact_title_bar {
            root = root.child(self.render_title_bar(cx));
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
