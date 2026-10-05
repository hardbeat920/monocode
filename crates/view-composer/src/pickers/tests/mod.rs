//! GPUI behavior tests for the pickers: clicks, hovers, and keys in a test
//! window, ported from the React component tests.

mod model_picker;
mod others;

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyView, Bounds, Context, IntoElement, Modifiers, MouseButton, ParentElement as _, Pixels,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};
use monocode_ui::AppearanceSettings;

/// Installs gpui-component, the MonoCode theme, and the picker key bindings.
pub(super) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        super::init(cx);
    });
}

/// A window body that puts its child near the bottom left, where the
/// composer's toolbar sits, so top-side popovers have room.
pub(super) struct Host {
    pub children: Vec<AnyView>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .items_start()
            .p(px(24.))
            .pb(px(120.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(4.))
                    .children(self.children.clone()),
            )
    }
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}

pub(super) fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("no element {selector}"))
}

pub(super) fn exists(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = bounds(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

pub(super) fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = bounds(cx, selector).center();
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw(cx);
}

pub(super) fn hover(cx: &mut VisualTestContext, selector: &'static str) {
    let at = bounds(cx, selector).center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    draw(cx);
}

pub(super) fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

/// Records every value a callback receives.
pub(super) struct Calls<T>(Rc<RefCell<Vec<T>>>);

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

    pub fn last(&self) -> Option<T> {
        self.0.borrow().last().cloned()
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }
}
