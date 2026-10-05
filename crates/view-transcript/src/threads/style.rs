//! Colors these views read that are not theme tokens: Tailwind palette
//! classes the React markup names directly, and the custom properties
//! index.css gives the btw burst and the orchestrator constellation.
//! Theme tokens come from `monocode_ui::Theme`.

use gpui::Hsla;
use monocode_ui::Theme;
use monocode_ui::color::{hex, mix, rgba8, with_alpha};

/// `amber-300`, a running side thread's dot.
pub fn amber_300() -> Hsla {
    hex(0xffd230)
}

/// `red-300`, a failed side thread's dot.
pub fn red_300() -> Hsla {
    hex(0xffa2a2)
}

/// `emerald-300`, a finished side thread's dot.
pub fn emerald_300() -> Hsla {
    hex(0x5ee9b5)
}

/// `red-200`, the btw error label.
pub fn red_200() -> Hsla {
    hex(0xffc9c9)
}

/// `red-100`, the btw error text.
pub fn red_100() -> Hsla {
    hex(0xffe2e2)
}

/// `red-50`, the hovered Retry label.
pub fn red_50() -> Hsla {
    hex(0xfef2f2)
}

/// `#f87171` in `.btw-error`.
pub fn btw_error_red() -> Hsla {
    hex(0xf87171)
}

/// The burst colors in `.btw-burst`: the accent lightened with white in
/// dark mode, the plain accent in light mode.
pub fn burst_colors(theme: &Theme) -> (Hsla, Hsla) {
    let accent = theme.user_accent_or_accent();
    if theme.is_dark() {
        (
            mix(accent, hex(0xffffff), 0.70),
            with_alpha(accent, 0.75 * accent.a),
        )
    } else {
        (accent, with_alpha(accent, 0.45 * accent.a))
    }
}

/// `--mode-color`, `--mode-glow`, and `--mode-spark` on
/// `.orchestrator-constellation`.
#[derive(Clone, Copy, Debug)]
pub struct ModeColors {
    pub color: Hsla,
    pub glow: Hsla,
    pub spark: Hsla,
}

pub fn orchestrator_colors(theme: &Theme) -> ModeColors {
    if theme.is_dark() {
        ModeColors {
            color: hex(0xf0abfc),
            glow: rgba8(232, 121, 249, 0.85),
            spark: hex(0xfdf4ff),
        }
    } else {
        ModeColors {
            color: hex(0xc026d3),
            glow: rgba8(192, 38, 211, 0.5),
            spark: hex(0xf5d0fe),
        }
    }
}
