//! Port of src/features/terminal/ui/ProjectTerminalDock.tsx: a project's
//! terminals, docked on one edge of the main area with a tab strip, a
//! resize sash, and a menu to move the dock to another edge.
//!
//! The dock draws what its owner gives it and reports what the user did
//! through [`TerminalDockEvent`]s, the React `on*` props. It does not touch
//! the engine: the owner hands it a [`DockTerminals`] that opens a
//! `TerminalView` for each terminal file, and applies the events to its
//! `ProjectTerminals`.
//!
//! The terminal grid around the dock (`dockGridStyle` in App.tsx) is
//! [`dock_grid`].

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    Styled as _, Subscription, Window, canvas, div, point, px,
};
use monocode_core::Platform;
use monocode_layout::project_terminal::{
    Viewport, clamp_dock_size, default_dock_size, is_vertical_dock,
};
use monocode_layout::{DockSide, FilePaneTab, ProjectTerminalDock};
use monocode_terminal_view::TerminalView;
use monocode_ui::widgets::{MenuItem, context_menu, icon_button, menu};
use monocode_ui::{IconName, Theme, u};

use crate::panes::surface_tabs::{
    SurfaceTabActions, SurfaceTabs, SurfaceTabsEvent, SurfaceTabsProps,
};

/// What the dock reports to its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalDockEvent {
    /// `onFocus`: a mouse down anywhere in the dock.
    Focus,
    /// `onHide`.
    Hide,
    /// `onSideChange`: a pick from the Move Terminal menu.
    SideChange(DockSide),
    /// `onSizePaint`: the size while the sash drags, in CSS px. Lay the
    /// grid out at this size; [`TerminalDock::display_size`] also has it.
    SizePaint(i64),
    /// `onSizeCommit`: the size to store, when the drag ends or the sash is
    /// double-clicked.
    SizeCommit(i64),
    /// `onAddTerminal`.
    AddTerminal,
    /// `onSelectTerminal`.
    SelectTerminal(String),
    /// `onCloseTerminal`.
    CloseTerminal(String),
    /// `onCloseOtherTerminals`.
    CloseOtherTerminals(String),
    /// `onReorderTerminals`.
    ReorderTerminals(Vec<String>),
}

/// Where the dock's terminals come from. The app opens each one through
/// the engine's `Terminals::attach`; tests and the gallery feed canned
/// output.
pub trait DockTerminals {
    /// The view for `file`. The dock asks once per file and keeps the view
    /// while the file stays in the dock.
    fn open(&self, file: &FilePaneTab, window: &mut Window, cx: &mut App) -> Entity<TerminalView>;
}

/// `SIDE_ITEMS`: the Move Terminal menu, in order.
pub const SIDE_ITEMS: [(DockSide, &str); 4] = [
    (DockSide::Bottom, "Dock Bottom"),
    (DockSide::Top, "Dock Top"),
    (DockSide::Left, "Dock Left"),
    (DockSide::Right, "Dock Right"),
];

/// `sideIcon`: the Move Terminal button shows where the dock sits.
pub fn side_icon(side: DockSide) -> IconName {
    match side {
        DockSide::Top => IconName::PanelTop,
        DockSide::Left => IconName::PanelLeft,
        DockSide::Right => IconName::PanelRight,
        DockSide::Bottom => IconName::PanelBottom,
    }
}

/// `hideIcon`: the Hide Terminal chevron points at the edge it hides into.
pub fn hide_icon(side: DockSide) -> IconName {
    match side {
        DockSide::Top => IconName::ChevronUp,
        DockSide::Left => IconName::ChevronLeft,
        DockSide::Right => IconName::ChevronRight,
        DockSide::Bottom => IconName::ChevronDown,
    }
}

/// The signed size change for a pointer that moved `delta` px along the
/// dock's axis: the bottom and right docks grow as the pointer moves back.
pub fn signed_delta(side: DockSide, delta: f64) -> f64 {
    if matches!(side, DockSide::Bottom | DockSide::Right) {
        -delta
    } else {
        delta
    }
}

/// The dock's trailing controls: New, Move, and Hide.
pub struct DockControls {
    side: DockSide,
    /// The Move button's bounds, for placing the menu under it.
    move_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

/// What the controls report to the dock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockControlsEvent {
    Add,
    /// Open the side menu at this window point.
    Move(Point<Pixels>),
    Hide,
}

impl EventEmitter<DockControlsEvent> for DockControls {}

impl DockControls {
    pub fn new(side: DockSide) -> Self {
        Self {
            side,
            move_bounds: Rc::default(),
        }
    }

    pub fn set_side(&mut self, side: DockSide, cx: &mut Context<Self>) {
        if self.side != side {
            self.side = side;
            cx.notify();
        }
    }
}

impl Render for DockControls {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let modifier = Platform::current().mod_label();
        let rem = window.rem_size();
        let bounds = self.move_bounds.clone();
        let measure = self.move_bounds.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(2.))
            .pr(u(6.))
            .child(
                div().debug_selector(|| "terminal-dock-new".into()).child(
                    icon_button("terminal-dock-new", IconName::Plus)
                        .tooltip(format!("New Terminal ({modifier}`)"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DockControlsEvent::Add))),
                ),
            )
            .child(
                div()
                    .debug_selector(|| "terminal-dock-move".into())
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _, _| measure.set(Some(bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(
                        icon_button("terminal-dock-move", side_icon(self.side))
                            .tooltip("Move Terminal")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                let Some(rect) = bounds.get() else {
                                    return;
                                };
                                let gap = u(4.).to_pixels(rem);
                                cx.emit(DockControlsEvent::Move(point(
                                    rect.origin.x,
                                    rect.bottom() + gap,
                                )));
                            })),
                    ),
            )
            .child(
                div().debug_selector(|| "terminal-dock-hide".into()).child(
                    icon_button("terminal-dock-hide", hide_icon(self.side))
                        .tooltip(format!("Hide Terminal ({modifier}J)"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DockControlsEvent::Hide))),
                ),
            )
    }
}

#[derive(Debug, Clone, Copy)]
struct DockDrag {
    /// Pointer position along the axis when the drag began, in CSS px.
    start: f64,
    /// The dock size then.
    size: i64,
}

/// `ProjectTerminalDock`: the docked terminals of one project.
pub struct TerminalDock {
    dock: ProjectTerminalDock,
    focused: bool,
    terminals: Rc<dyn DockTerminals>,
    views: HashMap<String, Entity<TerminalView>>,
    tabs: Entity<SurfaceTabs>,
    controls: Entity<DockControls>,
    drag: Option<DockDrag>,
    /// `pending`: the size the next commit stores.
    pending: i64,
    /// The size painted while a drag runs.
    painted: Option<i64>,
    menu: Option<Point<Pixels>>,
    /// The terminal last focused for being active, so a re-render does
    /// not steal focus back.
    active_focused: Option<String>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TerminalDockEvent> for TerminalDock {}

impl Focusable for TerminalDock {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalDock {
    pub fn new(
        dock: ProjectTerminalDock,
        terminals: Rc<dyn DockTerminals>,
        tab_actions: Rc<dyn SurfaceTabActions>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let props = tab_props(&dock);
        let tabs = cx.new(|cx| SurfaceTabs::new(props, tab_actions, cx));
        let controls = cx.new(|_| DockControls::new(dock.side));
        tabs.update(cx, |tabs, cx| {
            tabs.set_trailing(Some(controls.clone().into()), cx)
        });
        let subscriptions = vec![
            cx.subscribe(&tabs, |_, _, event: &SurfaceTabsEvent, cx| match event {
                SurfaceTabsEvent::Select(id) => {
                    cx.emit(TerminalDockEvent::SelectTerminal(id.clone()))
                }
                SurfaceTabsEvent::Close(id) => {
                    cx.emit(TerminalDockEvent::CloseTerminal(id.clone()))
                }
                SurfaceTabsEvent::CloseOthers(id) => {
                    cx.emit(TerminalDockEvent::CloseOtherTerminals(id.clone()))
                }
                SurfaceTabsEvent::Reorder { ids, .. } => {
                    cx.emit(TerminalDockEvent::ReorderTerminals(ids.clone()))
                }
                SurfaceTabsEvent::Pin(_) | SurfaceTabsEvent::PaneDragStart { .. } => {}
            }),
            cx.subscribe(
                &controls,
                |this, _, event: &DockControlsEvent, cx| match event {
                    DockControlsEvent::Add => cx.emit(TerminalDockEvent::AddTerminal),
                    DockControlsEvent::Hide => cx.emit(TerminalDockEvent::Hide),
                    DockControlsEvent::Move(position) => {
                        this.menu = Some(*position);
                        cx.notify();
                    }
                },
            ),
        ];
        let mut this = Self {
            pending: dock.size,
            dock,
            focused: false,
            terminals,
            views: HashMap::new(),
            tabs,
            controls,
            drag: None,
            painted: None,
            menu: None,
            active_focused: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.sync_views(window, cx);
        this
    }

    pub fn dock(&self) -> &ProjectTerminalDock {
        &self.dock
    }

    /// The size to lay the grid out at: the dragged size while the sash
    /// moves, else the dock's own.
    pub fn display_size(&self) -> i64 {
        self.painted.unwrap_or(self.dock.size)
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The side menu's position, while it is open.
    pub fn menu_position(&self) -> Option<Point<Pixels>> {
        self.menu
    }

    /// Opens the side menu under the Move button, as clicking it does.
    pub fn open_side_menu(&mut self, cx: &mut Context<Self>) {
        let Some(bounds) = self.controls.read(cx).move_bounds.get() else {
            return;
        };
        let gap = u(4.).to_pixels(Theme::of(cx).rem_size());
        self.menu = Some(point(bounds.origin.x, bounds.bottom() + gap));
        cx.notify();
    }

    /// The view open for terminal `file_id`.
    pub fn terminal(&self, file_id: &str) -> Option<&Entity<TerminalView>> {
        self.views.get(file_id)
    }

    pub fn tabs(&self) -> &Entity<SurfaceTabs> {
        &self.tabs
    }

    /// New dock state from the owner.
    pub fn set_dock(
        &mut self,
        dock: ProjectTerminalDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if dock == self.dock {
            return;
        }
        if self.drag.is_none() {
            self.pending = dock.size;
        }
        let props = tab_props(&dock);
        self.tabs.update(cx, |tabs, cx| tabs.set_props(props, cx));
        self.controls
            .update(cx, |controls, cx| controls.set_side(dock.side, cx));
        self.dock = dock;
        self.sync_views(window, cx);
        cx.notify();
    }

    /// `focused`: the dock holds the workspace focus. Its active terminal
    /// takes keyboard focus when this turns on or the active tab changes.
    pub fn set_focused(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Opens views for new terminal files and drops the views of files that
    /// left the dock. The engine keeps a terminal running until its file
    /// closes, so a dropped view loses nothing.
    fn sync_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: HashSet<&str> = self
            .dock
            .pane
            .files
            .iter()
            .map(|file| file.id.as_str())
            .collect();
        self.views.retain(|id, _| ids.contains(id.as_str()));
        for file in &self.dock.pane.files {
            if !self.views.contains_key(&file.id) {
                let view = self.terminals.open(file, window, cx);
                self.views.insert(file.id.clone(), view);
            }
        }
        self.focus_active(window, cx);
    }

    /// The TerminalView `active` effect: focus the terminal that just
    /// became active.
    fn focus_active(&mut self, window: &mut Window, cx: &mut App) {
        let active = self
            .focused
            .then(|| self.dock.pane.active_file_id.clone())
            .filter(|id| self.views.contains_key(id));
        if active == self.active_focused {
            return;
        }
        self.active_focused = active.clone();
        if let Some(view) = active.and_then(|id| self.views.get(&id)) {
            let handle = view.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    fn viewport(window: &Window, scale: f32) -> Viewport {
        let size = window.viewport_size();
        Viewport {
            width: f64::from(f32::from(size.width) / scale),
            height: f64::from(f32::from(size.height) / scale),
        }
    }

    fn axis(&self, position: Point<Pixels>, scale: f32) -> f64 {
        let value = if is_vertical_dock(self.dock.side) {
            position.y
        } else {
            position.x
        };
        f64::from(f32::from(value) / scale)
    }

    /// `onResizePointerDown`. A second click is the double-click reset.
    fn press_sash(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        window.prevent_default();
        if event.click_count >= 2 {
            self.pending = default_dock_size(self.dock.side);
            self.commit(cx);
            return;
        }
        let scale = scale_of(window);
        self.drag = Some(DockDrag {
            start: self.axis(event.position, scale),
            size: self.dock.size,
        });
        self.pending = self.dock.size;
        cx.notify();
    }

    /// `onResizePointerMove`.
    fn drag_to(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        let scale = scale_of(window);
        let delta = self.axis(position, scale) - drag.start;
        let next = clamp_dock_size(
            self.dock.side,
            drag.size as f64 + signed_delta(self.dock.side, delta),
            Some(Self::viewport(window, scale)),
        );
        self.pending = next;
        if self.painted != Some(next) {
            self.painted = Some(next);
            cx.emit(TerminalDockEvent::SizePaint(next));
            cx.notify();
        }
    }

    /// `onResizePointerUp`.
    fn release_sash(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_none() {
            return;
        }
        self.commit(cx);
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        self.painted = None;
        cx.emit(TerminalDockEvent::SizeCommit(self.pending));
        cx.notify();
    }

    fn pick_side(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(side) = DockSide::parse(id) {
            cx.emit(TerminalDockEvent::SideChange(side));
        }
        self.menu = None;
        cx.notify();
    }

    fn render_sash(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let vertical = is_vertical_dock(self.dock.side);
        let mut sash = div()
            .id("terminal-dock-sash")
            .debug_selector(|| "terminal-dock-sash".into())
            .absolute()
            .cursor(if vertical {
                CursorStyle::ResizeRow
            } else {
                CursorStyle::ResizeColumn
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.press_sash(event, window, cx)
                }),
            );
        sash = match self.dock.side {
            DockSide::Top => sash.left_0().right_0().bottom(px(-1.)).h(u(6.)),
            DockSide::Bottom => sash.left_0().right_0().top(px(-1.)).h(u(6.)),
            DockSide::Left => sash.top_0().bottom_0().right(px(-1.)).w(u(6.)),
            DockSide::Right => sash.top_0().bottom_0().left(px(-1.)).w(u(6.)),
        };
        if self.drag.is_some() {
            sash = sash.bg(theme.content(0.15));
        } else {
            let hover = theme.content(0.10);
            sash = sash.hover(move |style| style.bg(hover));
        }
        sash.into_any_element()
    }

    /// Window-wide pointer tracking while the sash drags, as
    /// `setPointerCapture` gave the React sash.
    fn render_drag_listeners(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let dock = cx.entity().downgrade();
        let cursor = if is_vertical_dock(self.dock.side) {
            CursorStyle::ResizeRow
        } else {
            CursorStyle::ResizeColumn
        };
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.set_window_cursor_style(cursor);
                let on_move = dock.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        on_move
                            .update(cx, |this, cx| this.drag_to(event.position, window, cx))
                            .ok();
                    }
                });
                let on_up = dock.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        on_up.update(cx, |this, cx| this.release_sash(cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.menu?;
        let side = self.dock.side;
        let entries = SIDE_ITEMS.iter().map(|(id, label)| {
            MenuItem::new(id.as_str(), *label)
                .checked(*id == side)
                .into()
        });
        let pick = cx.entity().downgrade();
        let dismiss = cx.entity().downgrade();
        Some(
            context_menu(
                position,
                menu("terminal-dock-side-menu", entries).on_pick(move |id, _, cx| {
                    let id = id.to_string();
                    pick.update(cx, |this, cx| this.pick_side(&id, cx)).ok();
                }),
                move |_, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.menu = None;
                            cx.notify();
                        })
                        .ok();
                },
                cx,
            )
            .into_any_element(),
        )
    }
}

fn scale_of(window: &Window) -> f32 {
    f32::from(window.rem_size()) / 16.0
}

fn tab_props(dock: &ProjectTerminalDock) -> SurfaceTabsProps {
    SurfaceTabsProps {
        files: dock.pane.files.clone(),
        active_file_id: dock.pane.active_file_id.clone(),
        label: "Terminals".into(),
        ..SurfaceTabsProps::default()
    }
}

impl Render for TerminalDock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("terminal-dock")
            .debug_selector(|| "terminal-dock".into())
            .track_focus(&self.focus)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .border_color(theme.colors.stroke)
            .capture_any_mouse_down(cx.listener(|_, _, _, cx| cx.emit(TerminalDockEvent::Focus)));
        root = match self.dock.side {
            DockSide::Top => root.border_b_1(),
            DockSide::Bottom => root.border_t_1(),
            DockSide::Left => root.border_r_1(),
            DockSide::Right => root.border_l_1(),
        };

        let active = self
            .views
            .get(&self.dock.pane.active_file_id)
            .cloned()
            .map(|view| div().absolute().top_0().left_0().size_full().child(view));
        root = root
            .child(self.tabs.clone())
            .child(
                div()
                    .debug_selector(|| "terminal-dock-body".into())
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .children(active),
            )
            .child(self.render_sash(&theme, cx));
        if self.drag.is_some() {
            root = root.child(self.render_drag_listeners(cx));
        }
        if let Some(menu) = self.render_menu(cx) {
            root = root.child(menu);
        }
        root
    }
}

/// How [`dock_grid`] arranges the main area and the dock: `dockGridStyle`
/// as a flex layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockGridLayout {
    /// The dock and the main area sit in a column (top and bottom docks).
    pub column: bool,
    /// The dock comes before the main area (top and left docks).
    pub dock_first: bool,
    /// The dock's size across, in CSS px.
    pub dock_px: f32,
}

/// `dockGridStyle`: `None` when the dock is hidden and the main area takes
/// the whole grid.
pub fn dock_grid_layout(side: Option<DockSide>, size: f64) -> Option<DockGridLayout> {
    let side = side?;
    Some(DockGridLayout {
        column: is_vertical_dock(side),
        dock_first: matches!(side, DockSide::Top | DockSide::Left),
        dock_px: monocode_core::js::round(size).max(1.0) as f32,
    })
}

/// The grid in App.tsx that holds the main area and the open dock. Pass
/// `side: None` for a hidden dock, and lay it out at
/// [`TerminalDock::display_size`] so a sash drag paints live.
pub fn dock_grid(
    side: Option<DockSide>,
    size: f64,
    dock: Option<AnyElement>,
    main: AnyElement,
) -> gpui::Div {
    let main = div()
        .debug_selector(|| "dock-grid-main".into())
        .relative()
        .flex()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(main);
    let grid = div().flex().size_full().min_h_0().min_w_0();
    let (Some(layout), Some(dock)) = (dock_grid_layout(side, size), dock) else {
        return grid.child(main);
    };
    let mut slot = div()
        .debug_selector(|| "dock-grid-dock".into())
        .flex_none()
        .overflow_hidden()
        .min_h_0()
        .min_w_0()
        .child(dock);
    slot = if layout.column {
        slot.w_full().h(u(layout.dock_px))
    } else {
        slot.h_full().w(u(layout.dock_px))
    };
    let grid = if layout.column {
        grid.flex_col()
    } else {
        grid.flex_row()
    };
    if layout.dock_first {
        grid.child(slot).child(main)
    } else {
        grid.child(main).child(slot)
    }
}

#[cfg(test)]
#[path = "dock_tests.rs"]
mod tests;
