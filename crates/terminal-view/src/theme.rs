//! Terminal colors and font.
//!
//! Port of `terminalTheme`, `ANSI_DARK`, `ANSI_LIGHT`, `monoFont`, and
//! `oscColors` in src/features/terminal/ui/TerminalView.tsx. The CSS variables
//! that file reads at runtime are resolved here to the values the default app
//! theme gives them (`src/styles/index.css`, hue 240, saturation 0%).

use gpui::{Hsla, Pixels, Rgba, SharedString, px, rgb, rgba};

/// Colors and font for one terminal. The host builds it from the app theme;
/// [`TerminalTheme::dark`] and [`TerminalTheme::light`] match TerminalView.tsx.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalTheme {
    /// ANSI colors 0 to 15: black, red, green, yellow, blue, magenta, cyan,
    /// white, then the bright versions in the same order.
    pub ansi: [Rgba; 16],
    /// Default text color (`--color-content`).
    pub foreground: Rgba,
    /// Fill behind the grid. TerminalView.tsx uses `#00000000` so the pane
    /// background shows through.
    pub background: Rgba,
    /// The opaque color the terminal sits on (`--color-background-base`).
    /// OSC 11 queries report it, because `background` is transparent.
    pub base_background: Rgba,
    /// Cursor color (`--color-accent`).
    pub cursor: Rgba,
    /// Text color drawn on top of a block cursor.
    pub cursor_accent: Rgba,
    /// Selection fill while the terminal has focus.
    pub selection: Rgba,
    /// Selection fill while the terminal does not have focus.
    pub selection_inactive: Rgba,
    /// First choice of font family. CSS generic names work: `ui-monospace`
    /// and `monospace` map to the platform's monospace font.
    pub font_family: SharedString,
    /// Families to try, in order, when `font_family` is not installed.
    pub font_fallbacks: Vec<SharedString>,
    /// Font size. TerminalView.tsx uses 13px.
    pub font_size: Pixels,
    /// Row height as a multiple of the font's natural line height. xterm.js
    /// `lineHeight: 1`.
    pub line_height: f32,
    /// Lines of scrollback history. xterm.js `scrollback: 5000`.
    pub scrollback: usize,
}

/// `ANSI_DARK` in TerminalView.tsx.
pub const ANSI_DARK: [u32; 16] = [
    0x1d2428, 0xf87171, 0x4ade80, 0xfbbf24, 0x60a5fa, 0xc084fc, 0x22d3ee, 0xe8eef2, 0x64748b,
    0xfca5a5, 0x86efac, 0xfde68a, 0x93c5fd, 0xd8b4fe, 0x67e8f9, 0xf8fafc,
];

/// `ANSI_LIGHT` in TerminalView.tsx, a One Light palette for a near-white
/// canvas.
pub const ANSI_LIGHT: [u32; 16] = [
    0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xfafafa, 0x7c8591,
    0xdf6b60, 0x68b567, 0xd19a2f, 0x5c89f5, 0xb54bb3, 0x1f9cc9, 0xffffff,
];

/// The `--font-mono` stack in `src/styles/index.css`.
const FONT_STACK: [&str; 8] = [
    "ui-monospace",
    "SFMono-Regular",
    "Menlo",
    "Monaco",
    "Consolas",
    "Liberation Mono",
    "Courier New",
    "monospace",
];

impl TerminalTheme {
    /// The dark theme. `--color-content` is `hsl(240 0% 92%)`, the accent is
    /// `hsl(211 92% 62%)`, and `--color-background-base` is `hsl(240 0% 9%)`.
    pub fn dark() -> Self {
        Self {
            ansi: ANSI_DARK.map(rgb),
            foreground: rgb(0xebebeb),
            background: rgba(0x00000000),
            base_background: rgb(0x171717),
            cursor: rgb(0x459bf7),
            cursor_accent: rgb(0x000000),
            selection: rgba(0xffffff2e),
            selection_inactive: rgba(0xffffff14),
            ..Self::shared()
        }
    }

    /// The light theme (`html.theme-light`): content lightness 18%,
    /// background lightness 97%.
    pub fn light() -> Self {
        Self {
            ansi: ANSI_LIGHT.map(rgb),
            foreground: rgb(0x2e2e2e),
            background: rgba(0x00000000),
            base_background: rgb(0xf7f7f7),
            cursor: rgb(0x459bf7),
            cursor_accent: rgb(0xffffff),
            selection: rgba(0x0000002e),
            selection_inactive: rgba(0x00000014),
            ..Self::shared()
        }
    }

    fn shared() -> Self {
        Self {
            ansi: [rgb(0); 16],
            foreground: rgb(0),
            background: rgb(0),
            base_background: rgb(0),
            cursor: rgb(0),
            cursor_accent: rgb(0),
            selection: rgb(0),
            selection_inactive: rgb(0),
            font_family: FONT_STACK[0].into(),
            font_fallbacks: FONT_STACK[1..].iter().map(|f| (*f).into()).collect(),
            font_size: px(13.0),
            line_height: 1.0,
            scrollback: 5000,
        }
    }

    /// The color of a 256-color palette index. 0 to 15 come from the theme,
    /// 16 to 231 are the xterm 6x6x6 cube, and 232 to 255 the grey ramp.
    pub fn indexed(&self, index: u8) -> Rgba {
        match index {
            0..=15 => self.ansi[index as usize],
            _ => {
                let (r, g, b) = xterm_256(index);
                rgb_u8(r, g, b)
            }
        }
    }

    /// `font_family` followed by `font_fallbacks`.
    pub fn font_families(&self) -> impl Iterator<Item = &SharedString> {
        std::iter::once(&self.font_family).chain(self.font_fallbacks.iter())
    }
}

impl Default for TerminalTheme {
    fn default() -> Self {
        Self::dark()
    }
}

/// xterm's RGB value for palette entries 16 to 255.
pub fn xterm_256(index: u8) -> (u8, u8, u8) {
    const LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    match index {
        0..=15 => (0, 0, 0),
        16..=231 => {
            let n = (index - 16) as usize;
            (LEVELS[n / 36], LEVELS[(n / 6) % 6], LEVELS[n % 6])
        }
        232..=255 => {
            let v = 8 + 10 * (index - 232);
            (v, v, v)
        }
    }
}

/// An opaque color from 8-bit channels.
pub fn rgb_u8(r: u8, g: u8, b: u8) -> Rgba {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

/// 8-bit channels of a color, dropping alpha.
pub fn to_u8(color: Rgba) -> (u8, u8, u8) {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (channel(color.r), channel(color.g), channel(color.b))
}

/// The same color with alpha 1. xterm.js paints inverse text whose color is
/// the default background with `color.opaque(background)`.
pub fn opaque(color: Rgba) -> Rgba {
    Rgba { a: 1.0, ..color }
}

/// Paint color for GPUI.
pub fn hsla(color: Rgba) -> Hsla {
    color.into()
}

/// Map a CSS font family name to the name the platform's text system knows.
/// Returns `None` for names that have no meaning on this platform.
pub fn platform_family_name(name: &str) -> Option<&str> {
    match name {
        // WebKit draws `ui-monospace` with SF Mono, which CoreText exposes
        // only under this hidden family name.
        "ui-monospace" | "SFMono-Regular" | "SF Mono" => {
            cfg!(target_os = "macos").then_some(".AppleSystemUIFontMonospaced")
        }
        "monospace" => Some(if cfg!(target_os = "macos") {
            "Menlo"
        } else if cfg!(target_os = "windows") {
            "Consolas"
        } else {
            "DejaVu Sans Mono"
        }),
        other => Some(other),
    }
}

/// Pick the first family in `families` that the platform has. `installed`
/// is the text system's family list. Hidden macOS system families (names that
/// start with a dot) never appear in that list, so they count as installed.
pub fn resolve_font_family<'a>(
    families: impl IntoIterator<Item = &'a SharedString>,
    installed: &[String],
) -> SharedString {
    for family in families {
        let Some(name) = platform_family_name(family) else {
            continue;
        };
        if name.starts_with('.') || installed.iter().any(|f| f == name) {
            return SharedString::from(name.to_string());
        }
    }
    SharedString::from(
        platform_family_name("monospace")
            .unwrap_or("Menlo")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_defaults_match_terminal_view_tsx() {
        let theme = TerminalTheme::default();
        assert_eq!(to_u8(theme.ansi[0]), (0x1d, 0x24, 0x28));
        assert_eq!(to_u8(theme.ansi[1]), (0xf8, 0x71, 0x71));
        assert_eq!(to_u8(theme.ansi[15]), (0xf8, 0xfa, 0xfc));
        assert_eq!(to_u8(theme.foreground), (0xeb, 0xeb, 0xeb));
        assert_eq!(theme.background.a, 0.0);
        assert_eq!(to_u8(theme.base_background), (0x17, 0x17, 0x17));
        assert_eq!(to_u8(theme.cursor), (0x45, 0x9b, 0xf7));
        assert!((theme.selection.a - 0.18).abs() < 0.01);
        assert!((theme.selection_inactive.a - 0.08).abs() < 0.01);
        assert_eq!(theme.font_family.as_ref(), "ui-monospace");
        assert_eq!(theme.font_fallbacks.last().unwrap().as_ref(), "monospace");
        assert_eq!(theme.font_size, px(13.0));
        assert_eq!(theme.scrollback, 5000);
    }

    #[test]
    fn light_palette_is_one_light() {
        let theme = TerminalTheme::light();
        assert_eq!(to_u8(theme.ansi[4]), (0x40, 0x78, 0xf2));
        assert_eq!(to_u8(theme.foreground), (0x2e, 0x2e, 0x2e));
        assert_eq!(to_u8(theme.base_background), (0xf7, 0xf7, 0xf7));
        assert_eq!(to_u8(theme.cursor_accent), (0xff, 0xff, 0xff));
    }

    #[test]
    fn xterm_cube_and_grey_ramp() {
        assert_eq!(xterm_256(16), (0, 0, 0));
        assert_eq!(xterm_256(196), (0xff, 0, 0));
        assert_eq!(xterm_256(21), (0, 0, 0xff));
        assert_eq!(xterm_256(231), (0xff, 0xff, 0xff));
        assert_eq!(xterm_256(232), (8, 8, 8));
        assert_eq!(xterm_256(255), (238, 238, 238));
        let theme = TerminalTheme::dark();
        assert_eq!(theme.indexed(3), theme.ansi[3]);
        assert_eq!(to_u8(theme.indexed(196)), (0xff, 0, 0));
    }

    #[test]
    fn font_stack_resolution_skips_missing_families() {
        let theme = TerminalTheme::dark();
        let installed = vec!["Menlo".to_string(), "Courier New".to_string()];
        let family = resolve_font_family(theme.font_families(), &installed);
        if cfg!(target_os = "macos") {
            assert_eq!(family.as_ref(), ".AppleSystemUIFontMonospaced");
        } else {
            assert_eq!(family.as_ref(), "Menlo");
        }
        let only_generic: Vec<SharedString> = vec!["Nope".into(), "monospace".into()];
        let family = resolve_font_family(&only_generic, &[]);
        assert_eq!(Some(family.as_ref()), platform_family_name("monospace"));
    }
}
