//! Popover plumbing shared by the pickers. Port of the parts of
//! src/shared/ui/Popover.tsx the pickers rely on: placement against a
//! trigger, the glass frame, outside-click and Escape dismissal.
//!
//! A popover is a child of its trigger's `relative()` wrapper. It sits in a
//! zero-size absolute slot on the trigger edge and draws through `deferred`
//! in the popover layer, so pane clipping never hides it. It snaps to the
//! window edge instead of flipping to the other side, which the composer's
//! top-anchored menus never need.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, Bounds, ElementId, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement as _, Pixels, Point, Styled as _, Window, anchored, canvas,
    deferred, div, point, px, relative,
};
use monocode_ui::widgets::{POPOVER_GAP, POPOVER_PADDING, popover_frame};
use monocode_ui::{Theme, u};

/// Why a popover closed. `Escape` restores focus to the composer through the
/// owner's close callback; `Outside` leaves focus where the click put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DismissReason {
    Outside,
    Escape,
}

/// The side of the trigger a popover opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Right,
}

/// An element's bounds from its last prepaint, for hit tests that span
/// several surfaces (a trigger and the menus it opened).
#[derive(Clone, Default)]
pub struct BoundsCell(Rc<Cell<Option<Bounds<Pixels>>>>);

impl BoundsCell {
    pub fn get(&self) -> Option<Bounds<Pixels>> {
        self.0.get()
    }

    pub fn contains(&self, position: Point<Pixels>) -> bool {
        self.0
            .get()
            .is_some_and(|bounds| bounds.contains(&position))
    }

    pub fn clear(&self) {
        self.0.set(None);
    }

    /// An absolute, full-size probe to put inside the measured element.
    pub fn probe(&self) -> impl IntoElement + use<> {
        let cell = self.0.clone();
        canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }
}

/// Places `content` against the edge of its parent, which must be
/// `relative()`. `gap` is in CSS px; a negative gap overlaps, as flyouts do.
pub fn anchored_popover(
    side: Side,
    gap: f32,
    layer: usize,
    window: &Window,
    content: impl IntoElement,
) -> AnyElement {
    let gap = u(gap).to_pixels(window.rem_size());
    let (corner, offset) = match side {
        Side::Top => (Anchor::BottomLeft, point(px(0.), -gap)),
        Side::Bottom => (Anchor::TopLeft, point(px(0.), gap)),
        Side::Right => (Anchor::TopLeft, point(gap, px(0.))),
    };
    let slot = div().absolute().size_0();
    let slot = match side {
        Side::Top => slot.top_0().left_0(),
        Side::Bottom => slot.top(relative(1.)).left_0(),
        Side::Right => slot.top_0().left(relative(1.)),
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

/// Popover.tsx's default gap.
pub const DEFAULT_GAP: f32 = POPOVER_GAP;

/// The glass popover frame with a fixed width, blocking the pointer from
/// whatever is beneath it. `on_outside` runs for a press outside the frame.
pub fn popover_surface(
    id: impl Into<ElementId>,
    width: Option<f32>,
    max_height: Option<f32>,
    on_outside: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    content: impl IntoElement,
) -> gpui::Div {
    let mut frame = popover_frame(id);
    if let Some(width) = width {
        frame = frame.width(width);
    }
    if let Some(height) = max_height {
        frame = frame.max_height(height);
    }
    div()
        .occlude()
        .on_mouse_down_out(on_outside)
        .child(frame.child(content))
}

/// `LAYER.popover`, `LAYER.submenu`, and `LAYER.dialogPopover`.
pub fn popover_layer(cx: &App) -> usize {
    Theme::of(cx).layer.popover
}

pub fn submenu_layer(cx: &App) -> usize {
    Theme::of(cx).layer.submenu
}
