//! Tailwind palette colors the card components name directly. Theme tokens
//! come from `monocode_ui::Theme`; these are the fixed classes such as
//! `text-violet-400/90` that are not tokens. Hex values are Tailwind v4's.

use gpui::Hsla;
use monocode_ui::color::{hex, with_alpha};

pub use crate::transcript::view::style::palette::*;

/// `violet-400` at `alpha`: pull request chips and merged work items.
pub fn violet_400(alpha: f32) -> Hsla {
    with_alpha(hex(0xa684ff), alpha)
}

/// `emerald-400` at `alpha`: issue chips and open work items.
pub fn emerald_400(alpha: f32) -> Hsla {
    with_alpha(hex(0x00d492), alpha)
}

/// `rose-400` at `alpha`: closed work items.
pub fn rose(alpha: f32) -> Hsla {
    with_alpha(hex(0xff637e), alpha)
}

/// `red-400`: alerts.
pub fn red_400() -> Hsla {
    hex(0xff6467)
}

/// `amber-400`: the approval toast's attention mark.
pub fn amber_400() -> Hsla {
    hex(0xffb900)
}

/// `sky-400` at `alpha`: generic link chips.
pub fn sky_400(alpha: f32) -> Hsla {
    with_alpha(hex(0x00bcff), alpha)
}

/// `sky-400/60`: the focus ring on a diff chip.
pub fn sky_ring() -> Hsla {
    sky_400(0.6)
}
