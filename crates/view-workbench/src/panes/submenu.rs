//! Places a submenu beside the row that opened it, as Popover.tsx did for
//! `side="right"`: to the right of the row, or to its left when the window
//! has no room on the right.

use gpui::{
    Anchor, AnyElement, Bounds, IntoElement, ParentElement as _, Pixels, Window, anchored,
    deferred, point, px,
};
use monocode_ui::Theme;
use monocode_ui::widgets::POPOVER_PADDING;

/// `content` beside `row`, `gap` away, its top `lift` above the row's top.
/// `width` is the submenu's width in window pixels.
pub fn submenu_beside(
    row: Bounds<Pixels>,
    gap: Pixels,
    lift: Pixels,
    width: Pixels,
    content: impl IntoElement,
    window: &Window,
    theme: &Theme,
) -> AnyElement {
    let right = row.origin.x + row.size.width + gap;
    let room = window.viewport_size().width - px(POPOVER_PADDING);
    let (position, anchor) = if right + width <= room {
        (point(right, row.origin.y - lift), Anchor::TopLeft)
    } else {
        (
            point(row.origin.x - gap, row.origin.y - lift),
            Anchor::TopRight,
        )
    };
    deferred(
        anchored()
            .position(position)
            .anchor(anchor)
            .snap_to_window_with_margin(px(POPOVER_PADDING))
            .child(content),
    )
    .with_priority(theme.layer.submenu)
    .into_any_element()
}
