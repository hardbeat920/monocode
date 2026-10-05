//! Helpers for the GPUI view tests: install the theme, mount a view in a
//! test window, draw, find elements by debug selector, click, type, and move
//! the fake clock.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, Bounds, Context, Entity, IntoElement, Modifiers, ParentElement as _, Pixels, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};
use monocode_ui::AppearanceSettings;

pub fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        monocode_view_transcript::transcript::init(cx);
        monocode_view_composer::composer::init(cx);
        monocode_view_composer::pickers::init(cx);
    });
}

/// A window body that fills the window with one view.
pub struct Host {
    pub view: AnyView,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().child(self.view.clone())
    }
}

/// Opens a 1100 by 900 window with the view `build` returns.
pub fn mount<V: Render + 'static>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut gpui::App) -> Entity<V>,
) -> (Entity<V>, &mut VisualTestContext) {
    init(cx);
    let slot: Rc<RefCell<Option<Entity<V>>>> = Rc::default();
    let built = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = build(window, cx);
        *built.borrow_mut() = Some(view.clone());
        Host { view: view.into() }
    });
    cx.simulate_resize(gpui::size(px(1100.), px(900.)));
    draw(cx);
    let view = slot.borrow().clone().expect("mounted view");
    (view, cx)
}

pub fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}

fn leak(selector: &str) -> &'static str {
    Box::leak(selector.to_string().into_boxed_str())
}

pub fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    let selector = leak(selector);
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("no element {selector}"))
}

pub fn exists(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(leak(selector)).is_some()
}

pub fn click(cx: &mut VisualTestContext, selector: &str) {
    let at = bounds(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// Focuses the field under `selector` and types `text` into it.
pub fn fill(cx: &mut VisualTestContext, selector: &str, text: &str) {
    click(cx, selector);
    cx.simulate_input(text);
    draw(cx);
}

pub fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

/// Moves the fake clock and runs what became due.
pub fn advance(cx: &mut VisualTestContext, by: Duration) {
    cx.executor().advance_clock(by);
    cx.run_until_parked();
    draw(cx);
}
