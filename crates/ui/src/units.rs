//! Lengths that follow the interface scale.
//!
//! The React app zooms the whole webview for `monocode.uiScale`. GPUI has no
//! page zoom, so MonoCode sizes everything in rems and sets the window's rem
//! size to `16px * scale` (see [`crate::theme::sync_window`]). Write CSS px
//! from the React source through [`u`]: `h-10` is `u(40.)`, `text-[13px]` is
//! `u(13.)`.

use gpui::{Rems, rems};

/// A CSS px value from the React source, scaled by the interface scale.
#[inline]
pub fn u(css_px: f32) -> Rems {
    rems(css_px / 16.0)
}
