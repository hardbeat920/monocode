//! Stationary notification glass, matching GlassBackdrop and the native popover.
use crate::theme::*;
use gpui::{Div, canvas, div, prelude::*, px};

pub(crate) fn backdrop(radius: f32) -> impl IntoElement {
    let (light, opaque) = (is_light(), floating_fill());
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let quad = if light {
                gpui::fill(bounds, opaque)
            } else {
                gpui::fill(bounds, content(0.02))
                    .border_color(gpui::backdrop_blur_marker(24. * window.scale_factor()))
            };
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = (light, opaque);
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let quad = gpui::fill(bounds, pane_fill());
            window.paint_quad(quad.corner_radii(px(radius)));
        },
    )
    .absolute()
    .size_full()
}
pub(crate) fn glass_frame() -> Div {
    div()
        .relative()
        .w(px(340.))
        .max_w_full()
        .overflow_hidden()
        .rounded(px(12.))
        .border_1()
        .border_color(content(0.1))
        .text_color(content(1.))
        .shadow_lg()
        .child(backdrop(12.))
}

/// Shared shell for the compact cards in the top-right notification stack.
pub(crate) fn toast_frame() -> Div {
    glass_frame()
        .w_full()
        .occlude()
        .border_dashed()
        .border_color(content(0.2))
        .bg(content(0.1))
}
