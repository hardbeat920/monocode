//! Small helpers the cards share.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui::{AnyElement, App, Bounds, Entity, Hsla, IntoElement, Pixels, Styled as _, canvas};
use gpui_base::input::{InputBaseState, InputEditorStyle, InputModeKind};

use monocode_ui::Theme;
use monocode_ui::theme::CubicBezier;

/// A text field without gpui-component's frame: the text takes the size,
/// weight, and line height of its parent, like a bare `<input>` with
/// `bg-transparent outline-none`. `placeholder` is the placeholder ink.
pub fn bare_input<M: InputModeKind + 'static>(
    state: &Entity<InputBaseState<M>>,
    placeholder: Hsla,
    theme: &Theme,
    cx: &mut App,
) -> AnyElement {
    let style = InputEditorStyle {
        foreground: theme.colors.content,
        muted_foreground: placeholder,
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        ..Default::default()
    };
    state.update(cx, |state, _| state.set_editor_style(style));
    state.clone().into_any_element()
}

/// Where keyed elements were drawn in the last frame, for anchoring a
/// popover to one of them (`anchor={ref}` in the React code).
#[derive(Clone, Default)]
pub struct BoundsMap(Rc<RefCell<HashMap<String, Bounds<Pixels>>>>);

impl BoundsMap {
    /// An invisible layer that records its parent's bounds under `key`. Put
    /// it in a `relative` element.
    pub fn track(&self, key: impl Into<String>) -> impl IntoElement {
        let map = self.0.clone();
        let key = key.into();
        canvas(
            move |bounds, _, _| {
                map.borrow_mut().insert(key, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    pub fn get(&self, key: &str) -> Option<Bounds<Pixels>> {
        self.0.borrow().get(key).copied()
    }
}

/// A CSS transition on one number: from the value shown when the target
/// changed, to the target, after `delay_ms`, over `duration_ms`.
#[derive(Debug, Clone, Copy)]
pub struct Transition {
    from: f32,
    to: f32,
    changed: Instant,
    delay_ms: f32,
    duration_ms: f32,
    ease: CubicBezier,
}

impl Transition {
    pub fn new(value: f32, duration_ms: f32, ease: CubicBezier) -> Self {
        Self {
            from: value,
            to: value,
            changed: Instant::now(),
            delay_ms: 0.,
            duration_ms,
            ease,
        }
    }

    /// The value now.
    pub fn value(&self) -> f32 {
        let elapsed = self.changed.elapsed().as_secs_f32() * 1000. - self.delay_ms;
        if elapsed <= 0. {
            return self.from;
        }
        let t = (elapsed / self.duration_ms).min(1.);
        self.from + (self.to - self.from) * self.ease.ease(t)
    }

    /// Whether the value still moves.
    pub fn running(&self) -> bool {
        self.from != self.to
            && self.changed.elapsed().as_secs_f32() * 1000. < self.delay_ms + self.duration_ms
    }

    /// Head for `to`, starting `delay_ms` from now. Without motion the value
    /// jumps there.
    pub fn set(&mut self, to: f32, delay_ms: f32, animate: bool) {
        if to == self.to {
            return;
        }
        self.from = if animate { self.value() } else { to };
        self.to = to;
        self.delay_ms = delay_ms;
        self.changed = Instant::now();
    }
}

/// `Date.now()`.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transition_without_motion_jumps() {
        let mut transition = Transition::new(1., 200., CubicBezier(0., 0., 0.58, 1.));
        transition.set(5., 0., false);
        assert_eq!(transition.value(), 5.);
        assert!(!transition.running());
    }

    #[test]
    fn a_delayed_transition_holds_its_start() {
        let mut transition = Transition::new(1., 200., CubicBezier(0., 0., 0.58, 1.));
        transition.set(5., 10_000., true);
        assert_eq!(transition.value(), 1.);
        assert!(transition.running());
    }
}
