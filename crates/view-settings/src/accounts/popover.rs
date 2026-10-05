//! Placement and dismissal for the popovers these views open, following
//! src/shared/ui/Popover.tsx: anchored to a trigger, in the popover layer,
//! kept inside the window, closed by a press outside or Escape.

use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, Bounds, InteractiveElement as _, IntoElement, MouseDownEvent,
    ParentElement as _, Pixels, Styled as _, Window, anchored, deferred, div, point, px, relative,
};
use monocode_ui::widgets::POPOVER_PADDING;
use monocode_ui::{Theme, u};

use crate::settings::controls::TriggerBounds;

pub type OnOutside = Rc<dyn Fn(&mut Window, &mut App)>;

/// Which side of the trigger the popover opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
}

/// Places `content` against the trigger's wrapper, which must be
/// `relative()`. `align_end` lines up the right edges, otherwise the left
/// edges. `gap` is in CSS px.
pub fn anchored_to_trigger(
    side: Side,
    align_end: bool,
    gap: f32,
    window: &Window,
    cx: &App,
    content: impl IntoElement,
) -> AnyElement {
    let layer = Theme::of(cx).layer.popover;
    let gap = u(gap).to_pixels(window.rem_size());
    let (corner, offset) = match (side, align_end) {
        (Side::Bottom, true) => (Anchor::TopRight, point(px(0.), gap)),
        (Side::Bottom, false) => (Anchor::TopLeft, point(px(0.), gap)),
        (Side::Top, true) => (Anchor::BottomRight, point(px(0.), -gap)),
        (Side::Top, false) => (Anchor::BottomLeft, point(px(0.), -gap)),
    };
    let mut slot = div().absolute().size_0();
    slot = match side {
        Side::Top => slot.top_0(),
        Side::Bottom => slot.top(relative(1.)),
    };
    slot = if align_end {
        slot.right_0()
    } else {
        slot.left_0()
    };
    slot.child(
        deferred(
            anchored()
                .anchor(corner)
                .offset(offset)
                .snap_to_window_with_margin(px(POPOVER_PADDING))
                .child(content),
        )
        .with_priority(layer),
    )
    .into_any_element()
}

/// Wraps popover content so a press outside it, and outside the trigger,
/// runs `on_outside`. A press on the trigger is left to the trigger, which
/// toggles.
pub fn dismiss_outside(
    trigger: &TriggerBounds,
    on_outside: OnOutside,
    content: impl IntoElement,
) -> gpui::Div {
    let trigger = trigger.clone();
    div()
        .occlude()
        .on_mouse_down_out(move |event: &MouseDownEvent, window, cx| {
            if inside(trigger.get(), event.position) {
                return;
            }
            on_outside(window, cx);
        })
        .child(content)
}

fn inside(bounds: Option<Bounds<Pixels>>, position: gpui::Point<Pixels>) -> bool {
    bounds.is_some_and(|bounds| bounds.contains(&position))
}
