//! Tests for the terminal dock: the grid layout (the `dockGridStyle` cases
//! of projectTerminal.test.ts that wrote onto an element), the side and
//! hide icons, and the dock view's events.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    Context, Entity, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Render,
    TestAppContext, VisualTestContext, Window, div, point, px, size,
};
use monocode_layout::new_terminal_file;
use monocode_layout::project_terminal::{
    add_terminal_to_dock, create_project_terminal, dock_grid_style,
};
use monocode_terminal_view::{RecordingPty, TerminalTheme};

use super::*;
use crate::panes::surface_tabs::ClipboardOnlyActions;
use crate::terminal_dock::test_support::{draw, init};

#[test]
fn collapses_to_the_main_area_when_hidden() {
    assert_eq!(dock_grid_layout(None, 220.0), None);
}

#[test]
fn places_the_dock_on_the_requested_edge() {
    for (side, column, dock_first) in [
        (DockSide::Bottom, true, false),
        (DockSide::Top, true, true),
        (DockSide::Left, false, true),
        (DockSide::Right, false, false),
    ] {
        let layout = dock_grid_layout(Some(side), 220.0).expect("a layout");
        assert_eq!(
            (layout.column, layout.dock_first),
            (column, dock_first),
            "{side:?}"
        );
        // The same order dockGridStyle wrote into the template areas.
        let areas = dock_grid_style(Some(side), 220.0).grid_template_areas;
        assert_eq!(areas.find("dock") < areas.find("main"), dock_first);
        assert_eq!(areas.matches('"').count() == 4, column);
    }
}

#[test]
fn sizes_the_dock_track_like_the_template() {
    // applyDockGridStyle(el, "left", 300) wrote "300px minmax(0, 1fr)".
    let layout = dock_grid_layout(Some(DockSide::Left), 300.0).expect("a layout");
    assert_eq!(layout.dock_px, 300.0);
    assert_eq!(
        dock_grid_style(Some(DockSide::Left), 300.0).grid_template_columns,
        "300px minmax(0, 1fr)"
    );
    // Rounded, and never under a pixel.
    assert_eq!(
        dock_grid_layout(Some(DockSide::Top), 220.6).map(|l| l.dock_px),
        Some(221.0)
    );
    assert_eq!(
        dock_grid_layout(Some(DockSide::Top), 0.0).map(|l| l.dock_px),
        Some(1.0)
    );
}

#[test]
fn points_the_icons_at_the_dock_edge() {
    assert_eq!(side_icon(DockSide::Bottom), IconName::PanelBottom);
    assert_eq!(side_icon(DockSide::Top), IconName::PanelTop);
    assert_eq!(side_icon(DockSide::Left), IconName::PanelLeft);
    assert_eq!(side_icon(DockSide::Right), IconName::PanelRight);
    assert_eq!(hide_icon(DockSide::Bottom), IconName::ChevronDown);
    assert_eq!(hide_icon(DockSide::Top), IconName::ChevronUp);
    assert_eq!(hide_icon(DockSide::Left), IconName::ChevronLeft);
    assert_eq!(hide_icon(DockSide::Right), IconName::ChevronRight);
}

#[test]
fn grows_the_bottom_and_right_docks_against_the_pointer() {
    assert_eq!(signed_delta(DockSide::Bottom, -50.0), 50.0);
    assert_eq!(signed_delta(DockSide::Right, -50.0), 50.0);
    assert_eq!(signed_delta(DockSide::Top, 50.0), 50.0);
    assert_eq!(signed_delta(DockSide::Left, 50.0), 50.0);
}

#[derive(Default)]
struct FakeTerminals {
    opened: RefCell<Vec<String>>,
}

impl DockTerminals for FakeTerminals {
    fn open(&self, file: &FilePaneTab, window: &mut Window, cx: &mut App) -> Entity<TerminalView> {
        self.opened.borrow_mut().push(file.id.clone());
        cx.new(|cx| TerminalView::new(RecordingPty::new(), TerminalTheme::dark(), window, cx))
    }
}

fn terminal(id: &str, title: &str) -> FilePaneTab {
    let mut file = new_terminal_file("/work/demo", Some(title), None);
    file.id = id.into();
    file
}

/// A bottom-docked pair, "two" active.
fn two_terminals(side: DockSide) -> ProjectTerminalDock {
    let dock = create_project_terminal("/work/demo", terminal("one", "zsh"), Some(side));
    add_terminal_to_dock(&dock, terminal("two", "vim"))
}

/// The dock in its grid, the way App.tsx laid it out.
struct Host {
    dock: Entity<TerminalDock>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.read(cx);
        let side = dock.dock().side;
        let size = dock.display_size() as f64;
        dock_grid(
            Some(side),
            size,
            Some(self.dock.clone().into_any_element()),
            div().size_full().into_any_element(),
        )
    }
}

struct Harness<'a> {
    dock: Entity<TerminalDock>,
    terminals: Rc<FakeTerminals>,
    events: Rc<RefCell<Vec<TerminalDockEvent>>>,
    cx: &'a mut VisualTestContext,
}

impl Harness<'_> {
    /// Everything but the focus reports a press makes.
    fn events(&self) -> Vec<TerminalDockEvent> {
        self.events
            .borrow()
            .iter()
            .filter(|event| **event != TerminalDockEvent::Focus)
            .cloned()
            .collect()
    }
}

fn open(side: DockSide, cx: &mut TestAppContext) -> Harness<'_> {
    cx.update(init);
    let terminals = Rc::new(FakeTerminals::default());
    let source: Rc<dyn DockTerminals> = terminals.clone();
    let events: Rc<RefCell<Vec<TerminalDockEvent>>> = Rc::default();
    let (host, cx) = cx.add_window_view(move |window, cx| {
        let dock = cx.new(|cx| {
            TerminalDock::new(
                two_terminals(side),
                source,
                Rc::new(ClipboardOnlyActions),
                window,
                cx,
            )
        });
        cx.observe(&dock, |_, _, cx| cx.notify()).detach();
        Host { dock }
    });
    cx.simulate_resize(size(px(1000.), px(700.)));
    let dock = host.read_with(cx, |host, _| host.dock.clone());
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&dock, move |_, event: &TerminalDockEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    Harness {
        dock,
        terminals,
        events,
        cx,
    }
}

#[gpui::test]
fn opens_one_view_per_terminal_and_drops_closed_ones(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    assert_eq!(
        *h.terminals.opened.borrow(),
        vec!["one".to_string(), "two".to_string()]
    );

    let mut next = h.dock.read_with(h.cx, |dock, _| dock.dock().clone());
    next.pane.files.retain(|file| file.id != "one");
    next.pane.files.push(terminal("three", "zsh"));
    h.cx.update(|window, cx| {
        h.dock
            .update(cx, |dock, cx| dock.set_dock(next, window, cx))
    });
    draw(h.cx);

    assert_eq!(
        *h.terminals.opened.borrow(),
        vec!["one".to_string(), "two".to_string(), "three".to_string()]
    );
    h.dock.read_with(h.cx, |dock, _| {
        assert!(dock.terminal("one").is_none());
        assert!(dock.terminal("two").is_some());
        assert!(dock.terminal("three").is_some());
    });
}

#[gpui::test]
fn sits_on_the_bottom_edge_at_its_size(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    let dock = h.cx.debug_bounds("terminal-dock").expect("the dock");
    assert_eq!(dock.size.height, px(220.));
    assert_eq!(dock.bottom(), px(700.));
    let main = h.cx.debug_bounds("dock-grid-main").expect("the main area");
    assert_eq!(main.size.height, px(480.));
}

#[gpui::test]
fn sits_on_the_left_edge_at_its_size(cx: &mut TestAppContext) {
    let h = open(DockSide::Left, cx);
    let dock = h.cx.debug_bounds("dock-grid-dock").expect("the dock");
    assert_eq!(dock.origin.x, px(0.));
    assert_eq!(dock.size.width, px(360.));
    let main = h.cx.debug_bounds("dock-grid-main").expect("the main area");
    assert_eq!(main.origin.x, px(360.));
}

#[gpui::test]
fn reports_focus_on_any_press(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    let body = h.cx.debug_bounds("terminal-dock-body").expect("the body");
    h.cx.simulate_click(body.center(), Modifiers::none());
    assert!(h.events.borrow().contains(&TerminalDockEvent::Focus));
}

#[gpui::test]
fn adds_and_hides_from_the_trailing_buttons(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    let add =
        h.cx.debug_bounds("terminal-dock-new")
            .expect("the new button");
    h.cx.simulate_click(add.center(), Modifiers::none());
    let hide =
        h.cx.debug_bounds("terminal-dock-hide")
            .expect("the hide button");
    h.cx.simulate_click(hide.center(), Modifiers::none());
    assert_eq!(
        h.events(),
        vec![TerminalDockEvent::AddTerminal, TerminalDockEvent::Hide]
    );
}

#[gpui::test]
fn selects_a_terminal_from_its_tab(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    let tab =
        h.cx.debug_bounds("surface-tab-button:one")
            .expect("the first tab");
    h.cx.simulate_click(tab.center(), Modifiers::none());
    assert!(
        h.events()
            .contains(&TerminalDockEvent::SelectTerminal("one".into()))
    );
}

#[gpui::test]
fn opens_the_side_menu_under_the_move_button(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    let button =
        h.cx.debug_bounds("terminal-dock-move")
            .expect("the move button");
    h.cx.simulate_click(button.center(), Modifiers::none());
    let position = h
        .dock
        .read_with(h.cx, |dock, _| dock.menu_position())
        .expect("the menu");
    assert_eq!(position, point(button.origin.x, button.bottom() + px(4.)));

    h.dock.update(h.cx, |dock, cx| dock.pick_side("left", cx));
    assert_eq!(
        h.events(),
        vec![TerminalDockEvent::SideChange(DockSide::Left)]
    );
    assert!(
        h.dock
            .read_with(h.cx, |dock, _| dock.menu_position())
            .is_none()
    );
}

#[gpui::test]
fn resizes_from_the_sash_and_commits_on_release(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    let sash = h.cx.debug_bounds("terminal-dock-sash").expect("the sash");
    let start = sash.center();
    h.cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    draw(h.cx);
    // Up 50px grows a bottom dock by 50.
    let moved = point(start.x, start.y - px(50.));
    h.cx.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::none());
    draw(h.cx);
    assert_eq!(h.dock.read_with(h.cx, |dock, _| dock.display_size()), 270);
    let dock = h.cx.debug_bounds("terminal-dock").expect("the dock");
    assert_eq!(dock.size.height, px(270.));

    h.cx.simulate_mouse_up(moved, MouseButton::Left, Modifiers::none());
    draw(h.cx);
    assert_eq!(
        h.events(),
        vec![
            TerminalDockEvent::SizePaint(270),
            TerminalDockEvent::SizeCommit(270)
        ]
    );
    assert!(!h.dock.read_with(h.cx, |dock, _| dock.dragging()));
}

#[gpui::test]
fn clamps_a_drag_to_the_side_minimum(cx: &mut TestAppContext) {
    let h = open(DockSide::Right, cx);
    let sash = h.cx.debug_bounds("terminal-dock-sash").expect("the sash");
    let start = sash.center();
    h.cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    draw(h.cx);
    // Right 400px would shrink a 360px right dock past its 180px floor.
    let moved = point(start.x + px(400.), start.y);
    h.cx.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::none());
    h.cx.simulate_mouse_up(moved, MouseButton::Left, Modifiers::none());
    draw(h.cx);
    assert_eq!(
        h.events(),
        vec![
            TerminalDockEvent::SizePaint(180),
            TerminalDockEvent::SizeCommit(180)
        ]
    );
}

#[gpui::test]
fn restores_the_default_size_on_a_double_click(cx: &mut TestAppContext) {
    let h = open(DockSide::Top, cx);
    let sash = h.cx.debug_bounds("terminal-dock-sash").expect("the sash");
    let at = sash.center();
    for click_count in [1, 2] {
        h.cx.simulate_event(MouseDownEvent {
            position: at,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count,
            first_mouse: false,
        });
        h.cx.simulate_event(MouseUpEvent {
            position: at,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count,
        });
        draw(h.cx);
    }
    assert_eq!(
        h.events(),
        vec![
            TerminalDockEvent::SizeCommit(220),
            TerminalDockEvent::SizeCommit(220)
        ]
    );
}

fn has_focus(dock: &Entity<TerminalDock>, id: &str, cx: &mut VisualTestContext) -> bool {
    let view = dock
        .read_with(cx, |dock, _| dock.terminal(id).cloned())
        .expect("a view");
    cx.update(|window, cx| view.read(cx).focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn focuses_the_active_terminal_when_the_dock_takes_focus(cx: &mut TestAppContext) {
    let h = open(DockSide::Bottom, cx);
    assert!(!has_focus(&h.dock, "two", h.cx));
    h.cx.update(|window, cx| {
        h.dock
            .update(cx, |dock, cx| dock.set_focused(true, window, cx))
    });
    assert!(has_focus(&h.dock, "two", h.cx));

    let mut next = h.dock.read_with(h.cx, |dock, _| dock.dock().clone());
    next.pane.active_file_id = "one".into();
    h.cx.update(|window, cx| {
        h.dock
            .update(cx, |dock, cx| dock.set_dock(next, window, cx))
    });
    assert!(has_focus(&h.dock, "one", h.cx));
}
