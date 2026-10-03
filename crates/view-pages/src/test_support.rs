//! Helpers for the GPUI view tests: install the theme and pickers, mount a
//! view in a test window, draw, find elements by debug selector, click, and
//! type.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyView, Bounds, Context, Entity, IntoElement, Modifiers, ParentElement as _, Pixels, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};
use monocode_ui::AppearanceSettings;

pub fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        crate::init(cx);
    });
}

/// A window body that fills the window with one view.
pub struct Host {
    pub view: AnyView,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.view.clone())
    }
}

/// Opens a 1280 by 900 window with the view `build` returns.
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
    cx.simulate_resize(gpui::size(px(1280.), px(900.)));
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

pub fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("no element {selector}"))
}

pub fn exists(cx: &mut VisualTestContext, selector: &str) -> bool {
    let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
    cx.debug_bounds(selector).is_some()
}

pub fn click(cx: &mut VisualTestContext, selector: &str) {
    let at = bounds(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

pub fn hover(cx: &mut VisualTestContext, selector: &str) {
    let at = bounds(cx, selector).center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    draw(cx);
}

pub fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

pub fn type_text(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    draw(cx);
}

/// Records every value a callback receives.
pub struct Calls<T>(Rc<RefCell<Vec<T>>>);

impl<T: Clone + 'static> Calls<T> {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Vec::new())))
    }

    pub fn recorder(&self) -> impl Fn(T) + 'static {
        let calls = self.0.clone();
        move |value| calls.borrow_mut().push(value)
    }

    pub fn all(&self) -> Vec<T> {
        self.0.borrow().clone()
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }
}
