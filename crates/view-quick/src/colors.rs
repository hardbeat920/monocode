//! Colors the quick composer's React views wrote inline from the Tailwind
//! palette (`text-yellow-200/90`, `bg-amber-400/20`). The theme has no
//! tokens for them, so they live here, picked per color scheme the way the
//! `dark:` variants did. Everything else comes from [`Theme`].

use gpui::Hsla;
use monocode_ui::Theme;
use monocode_ui::color::{hex, with_alpha};
use monocode_view_composer::composer::model::mode_commands::Mode;

const YELLOW_200: u32 = 0xfef08a;
const YELLOW_300: u32 = 0xfde047;
const YELLOW_700: u32 = 0xa16207;
const SKY_200: u32 = 0xbae6fd;
const SKY_300: u32 = 0x7dd3fc;
const SKY_700: u32 = 0x0369a1;
const FUCHSIA_200: u32 = 0xf5d0fe;
const FUCHSIA_300: u32 = 0xf0abfc;
const FUCHSIA_700: u32 = 0xa21caf;
const EMERALD_200: u32 = 0xa7f3d0;
const EMERALD_700: u32 = 0x047857;
const WHITE: u32 = 0xffffff;
const BLACK: u32 = 0x000000;

fn tw(value: u32, alpha: f32) -> Hsla {
    with_alpha(hex(value), alpha)
}

/// `MODE_COMMAND_STYLES[name].className`: the command's color in the prompt.
pub fn mode_text(mode: Mode, theme: &Theme) -> Hsla {
    let dark = theme.is_dark();
    match mode {
        Mode::Plan if dark => tw(YELLOW_200, 0.9),
        Mode::Plan => tw(YELLOW_700, 1.0),
        Mode::Operator if dark => tw(SKY_200, 0.9),
        Mode::Operator => tw(SKY_700, 1.0),
        Mode::Orchestrator if dark => tw(FUCHSIA_200, 0.9),
        Mode::Orchestrator => tw(FUCHSIA_700, 1.0),
        Mode::Draft => theme.content(0.70),
        Mode::Btw if dark => tw(EMERALD_200, 0.9),
        Mode::Btw => tw(EMERALD_700, 1.0),
    }
}

/// `menu.iconClassName`: a command row's icon.
pub fn mode_menu_icon(mode: Mode, theme: &Theme) -> Hsla {
    match mode {
        Mode::Plan => tw(YELLOW_300, 0.80),
        Mode::Operator => tw(SKY_300, 0.80),
        Mode::Orchestrator => tw(FUCHSIA_300, 0.65),
        Mode::Draft | Mode::Btw => theme.content(0.60),
    }
}

/// `text-white`.
pub fn white() -> Hsla {
    hex(WHITE)
}

/// `bg-white/45`: the reasoning slider's filled dots.
pub fn white_alpha(alpha: f32) -> Hsla {
    tw(WHITE, alpha)
}

/// `rgb(0 0 0 / a)`: the slider thumb's border and shadow.
pub fn black_alpha(alpha: f32) -> Hsla {
    tw(BLACK, alpha)
}

/// `text-amber-400` at an alpha (`bg-amber-400/20`).
pub fn amber(theme: &Theme, alpha: f32) -> Hsla {
    with_alpha(theme.colors.warning, alpha)
}

/// `text-red-400` at an alpha.
pub fn red(theme: &Theme, alpha: f32) -> Hsla {
    with_alpha(theme.colors.danger, alpha)
}
