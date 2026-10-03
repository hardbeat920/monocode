//! Port of src/app/shell/ClosingTab.tsx (`TabWidthMotion`) and
//! `tabCloseDuration` from src/shared/lib/motion.ts.
//!
//! A closing tab keeps its last width and collapses to zero; an opening tab
//! grows from zero to the 14rem tab slot. The CSS transitioned `width`,
//! `min-width`, and `margin-right` on `.tab-closing` and `.tab-opening`;
//! here a GPUI animation plays the same curve. The owner ends the motion
//! with a timer of the same length, as the component's `setTimeout` did.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, ElementId, IntoElement, ParentElement as _,
    Styled as _, div, px,
};
use monocode_ui::{Theme, u};

/// `phase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabMotionPhase {
    Opening,
    Closing,
}

/// `--tab-slot-width`'s fallback, 14rem, in CSS px.
pub const TAB_SLOT_WIDTH: f32 = 224.0;
/// `.tab-opening`'s `--tab-slot-min-width`, 7rem, in CSS px.
pub const TAB_SLOT_MIN_WIDTH: f32 = 112.0;
/// `margin-right: -0.125rem` once collapsed, in CSS px.
const COLLAPSED_MARGIN: f32 = 2.0;

/// `tabCloseDuration`: `--motion-tab-close-duration`, or 180ms when unset.
pub fn tab_close_duration(theme: &Theme) -> Duration {
    if theme.motion.tab_close.is_zero() {
        Duration::from_millis(180)
    } else {
        theme.motion.tab_close
    }
}

/// `TabWidthMotion`: `child` inside a slot whose width animates for
/// `phase`. `width` is the closing tab's measured width in window pixels,
/// or 0 when it was never measured.
pub fn tab_width_motion(
    id: impl Into<ElementId>,
    phase: TabMotionPhase,
    width: f32,
    child: AnyElement,
    theme: &Theme,
) -> AnyElement {
    let duration = tab_close_duration(theme);
    let easing = theme.motion.tab_ease_out;
    let slot = div()
        .relative()
        .flex()
        .h_full()
        .min_w_0()
        .overflow_hidden()
        .child(child);
    let animation = Animation::new(duration).with_easing(easing.easing());
    match phase {
        TabMotionPhase::Closing => slot
            .flex_none()
            .with_animation(id, animation, move |el, t| {
                let el = el.mr(u(-COLLAPSED_MARGIN * t));
                if width > 1.0 {
                    el.w(px(width * (1.0 - t)))
                } else {
                    el.w(u(TAB_SLOT_WIDTH * (1.0 - t)))
                }
            })
            .into_any_element(),
        TabMotionPhase::Opening => slot
            .flex_shrink(1.)
            .with_animation(id, animation, move |el, t| {
                el.w(u(TAB_SLOT_WIDTH * t))
                    .min_w(u(TAB_SLOT_MIN_WIDTH * t))
                    .mr(u(-COLLAPSED_MARGIN * (1.0 - t)))
            })
            .into_any_element(),
    }
}
