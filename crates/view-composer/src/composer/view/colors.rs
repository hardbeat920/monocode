//! The Tailwind palette colors modeCommands.tsx and Composer.tsx write
//! inline (`text-yellow-200/90`, `bg-sky-400/10`). The theme has no tokens
//! for them, so they live here, picked per color scheme the way the React
//! `dark:` variants did.

use gpui::Hsla;
use monocode_ui::Theme;
use monocode_ui::color::{hex, with_alpha};

use super::super::model::mode_commands::Mode;

const YELLOW_200: u32 = 0xfef08a;
const YELLOW_300: u32 = 0xfde047;
const YELLOW_700: u32 = 0xa16207;
const SKY_200: u32 = 0xbae6fd;
const SKY_300: u32 = 0x7dd3fc;
const SKY_400: u32 = 0x38bdf8;
const SKY_500: u32 = 0x0ea5e9;
const SKY_700: u32 = 0x0369a1;
const FUCHSIA_200: u32 = 0xf5d0fe;
const FUCHSIA_300: u32 = 0xf0abfc;
const FUCHSIA_400: u32 = 0xe879f9;
const FUCHSIA_500: u32 = 0xd946ef;
const FUCHSIA_700: u32 = 0xa21caf;
const EMERALD_200: u32 = 0xa7f3d0;
const EMERALD_700: u32 = 0x047857;

fn tw(value: u32, alpha: f32) -> Hsla {
    with_alpha(hex(value), alpha)
}

/// `MODE_COMMAND_STYLES[name].className`: the command's text color.
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

/// A mode pill's fill, hover fill, and whether it has the dashed outline.
pub struct PillColors {
    pub bg: Hsla,
    pub hover: Hsla,
    pub text: Hsla,
    pub hover_text: Hsla,
    pub dashed_border: Option<Hsla>,
}

/// `pill.className` in `MODE_COMMAND_STYLES`.
pub fn mode_pill(mode: Mode, theme: &Theme) -> PillColors {
    let dark = theme.is_dark();
    let text = mode_text(mode, theme);
    match mode {
        Mode::Plan => PillColors {
            bg: tw(YELLOW_300, 0.12),
            hover: tw(YELLOW_300, 0.18),
            text,
            hover_text: text,
            dashed_border: None,
        },
        Mode::Operator => PillColors {
            bg: if dark {
                tw(SKY_400, 0.10)
            } else {
                tw(SKY_500, 0.15)
            },
            hover: if dark {
                tw(SKY_400, 0.15)
            } else {
                tw(SKY_500, 0.20)
            },
            text,
            hover_text: text,
            dashed_border: None,
        },
        Mode::Orchestrator => PillColors {
            bg: if dark {
                tw(FUCHSIA_400, 0.10)
            } else {
                tw(FUCHSIA_500, 0.15)
            },
            hover: if dark {
                tw(FUCHSIA_400, 0.15)
            } else {
                tw(FUCHSIA_500, 0.20)
            },
            text,
            hover_text: text,
            dashed_border: None,
        },
        Mode::Draft | Mode::Btw => PillColors {
            bg: theme.content(0.05),
            hover: theme.content(0.10),
            text: theme.content(0.70),
            hover_text: theme.colors.content,
            dashed_border: Some(theme.content(0.25)),
        },
    }
}

/// `menu.iconClassName`: the + menu row icon.
pub fn mode_menu_icon(mode: Mode, theme: &Theme) -> Hsla {
    match mode {
        Mode::Plan => tw(YELLOW_300, 0.80),
        Mode::Operator => tw(SKY_300, 0.80),
        Mode::Orchestrator => tw(FUCHSIA_300, 0.65),
        Mode::Draft | Mode::Btw => theme.content(0.60),
    }
}

/// The check mark beside an active + menu row.
pub fn mode_menu_check(mode: Mode, theme: &Theme) -> Hsla {
    match mode {
        Mode::Operator => tw(SKY_300, 0.80),
        Mode::Orchestrator => tw(FUCHSIA_300, 0.80),
        _ => theme.colors.accent,
    }
}

/// The Orchestrator row's `v1` badge: fill and ink.
pub fn orchestrator_badge() -> (Hsla, Hsla) {
    (tw(FUCHSIA_300, 0.10), tw(FUCHSIA_200, 0.55))
}

/// The stop button: `bg-white text-black hover:bg-white/90`.
pub fn stop_button() -> (Hsla, Hsla, Hsla) {
    (hex(0xffffff), with_alpha(hex(0xffffff), 0.9), hex(0x000000))
}

/// ComposerRunner's coin and star fills.
pub const COIN: u32 = 0xe8b923;
pub const STAR: u32 = 0xf4e27a;
