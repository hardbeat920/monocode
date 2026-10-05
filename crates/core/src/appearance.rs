//! Port of the settings shapes, defaults, and value ranges in
//! src/features/settings/model/appearance.ts and uiScale.ts.
//!
//! Applying a value (CSS variables, native window glass, webview zoom) is the
//! UI's job. This module keeps what the values are, how the TypeScript read
//! them from storage, and how it clamped them. Each field of
//! `AppearanceSettings` names the localStorage key it came from.

use serde::{Deserialize, Serialize};

use crate::js::{self, clamp};
use crate::platform::Platform;
use crate::settings::{read_flag, read_number, string_enum};

// localStorage keys, one per setting.
pub const ACCENT_COLOR_KEY: &str = "monocode.accentColor";
pub const THEME_HUE_KEY: &str = "monocode.themeHue";
pub const THEME_SATURATION_KEY: &str = "monocode.themeSaturation";
pub const THEME_DARK_LIGHTNESS_KEY: &str = "monocode.themeDarkLightness";
/// Sidebar opacity. Before `MAIN_OPACITY_KEY` existed the main pane used it too.
pub const OPACITY_KEY: &str = "monocode.sidebarOpacity";
pub const MAIN_OPACITY_KEY: &str = "monocode.mainOpacity";
/// The window blur radius. The key name predates the main pane glass.
pub const BLUR_KEY: &str = "monocode.sidebarBlur";
pub const PROJECT_RAIL_OPEN_KEY: &str = "monocode.projectRailOpen";
pub const SESSION_SIDEBAR_OPEN_KEY: &str = "monocode.sessionSidebarOpen";
pub const BODY_KEY: &str = "monocode.bodyGlass";
/// The theme preference (dark, light, or system).
pub const SCHEME_KEY: &str = "monocode.colorScheme";
pub const SIDEBAR_TAB_ORDER_KEY: &str = "monocode.sidebarTabOrder";
pub const PROJECT_RAIL_WIDTH_KEY: &str = "monocode.projectRailWidth";
pub const TRANSCRIPT_LAYOUT_KEY: &str = "monocode.transcriptLayout";
pub const TRANSCRIPT_ANCHOR_KEY: &str = "monocode.transcriptAnchor";
pub const CHAT_BACKGROUND_PATH_KEY: &str = "monocode.chatBackgroundPath";
/// Legacy shared opacity. Both per-scope opacities fall back to it.
pub const CHAT_BACKGROUND_OPACITY_KEY: &str = "monocode.chatBackgroundOpacity";
pub const CHAT_BACKGROUND_EMPTY_OPACITY_KEY: &str = "monocode.chatBackgroundEmptyOpacity";
pub const CHAT_BACKGROUND_SESSION_OPACITY_KEY: &str = "monocode.chatBackgroundSessionOpacity";
pub const CHAT_BACKGROUND_SCOPE_KEY: &str = "monocode.chatBackgroundScope";
pub const CHAT_BACKGROUND_BLUR_KEY: &str = "monocode.chatBackgroundBlur";
pub const NEW_THREAD_BACKGROUND_EFFECT_KEY: &str = "monocode.newThreadBackgroundEffect";
pub const CHANGES_VIEW_KEY: &str = "monocode.changesView";
pub const DIFF_PALETTE_KEY: &str = "monocode.diffPalette";
pub const SHOW_EXCLUDED_FILES_KEY: &str = "monocode.showExcludedFiles";
/// From uiScale.ts.
pub const UI_SCALE_KEY: &str = "monocode.uiScale";

string_enum! {
    /// `ColorScheme`: the resolved scheme.
    ColorScheme {
        #[default]
        Dark = "dark",
        Light = "light",
    }
    default Dark
}

string_enum! {
    /// `ThemePreference`.
    ThemePreference {
        #[default]
        Dark = "dark",
        Light = "light",
        System = "system",
    }
    default Dark
}

string_enum! {
    /// `TranscriptLayout`.
    TranscriptLayout {
        Full = "full",
        #[default]
        Chat = "chat",
    }
    default Chat
}

string_enum! {
    /// `ChatBackgroundScope`: show the background on empty sessions only, or always.
    ChatBackgroundScope {
        Empty = "empty",
        #[default]
        All = "all",
    }
    default All
}

string_enum! {
    /// `NewThreadBackgroundEffect`.
    NewThreadBackgroundEffect {
        #[default]
        None = "none",
        Dither = "dither",
        Ascii = "ascii",
        Halftone = "halftone",
        Scanlines = "scanlines",
        GradientBlur = "gradient-blur",
    }
    default None
}

string_enum! {
    /// `ChangesView`: the git changes panel layout.
    ChangesView {
        #[default]
        List = "list",
        Tree = "tree",
    }
    default List
}

string_enum! {
    /// `DiffPalette`: the colors for added and removed lines.
    DiffPalette {
        #[default]
        Default = "default",
        Colorblind = "colorblind",
        HighContrast = "high-contrast",
    }
    default Default
}

string_enum! {
    /// `SidebarTabId`.
    SidebarTabId {
        Files = "files",
        #[default]
        Sessions = "sessions",
        Changes = "changes",
        Inbox = "inbox",
    }
    default Sessions
}

impl NewThreadBackgroundEffect {
    /// `NEW_THREAD_BACKGROUND_EFFECT_LABELS`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Dither => "Dither",
            Self::Ascii => "ASCII",
            Self::Halftone => "Halftone",
            Self::Scanlines => "Scanlines",
            Self::GradientBlur => "Haze",
        }
    }

    /// `NEW_THREAD_BACKGROUND_EFFECT_DESCRIPTIONS`.
    pub const fn description(self) -> &'static str {
        match self {
            Self::None => "Shows the original artwork.",
            Self::Dither => "Rebuilds the artwork with a dithered color palette.",
            Self::Ascii => "Recreates the artwork with colored characters on black.",
            Self::Halftone => "Recreates the artwork with colored print dots on black.",
            Self::Scanlines => "Adds a pronounced horizontal display-line texture.",
            Self::GradientBlur => "Blurs and fades the artwork into the background below.",
        }
    }
}

pub const THEME_PREFERENCE_DEFAULT: ThemePreference = ThemePreference::Dark;
/// `ACCENT_COLOR_DEFAULT`: no user accent, the original neutral appearance.
pub const ACCENT_COLOR_DEFAULT: Option<&str> = None;
pub const TRANSCRIPT_LAYOUT_DEFAULT: TranscriptLayout = TranscriptLayout::Chat;
pub const CHANGES_VIEW_DEFAULT: ChangesView = ChangesView::List;
pub const DIFF_PALETTE_DEFAULT: DiffPalette = DiffPalette::Default;
pub const TRANSCRIPT_ANCHOR_DEFAULT: bool = true;
pub const SHOW_EXCLUDED_FILES_DEFAULT: bool = false;
pub const NEW_THREAD_BACKGROUND_EFFECT_DEFAULT: NewThreadBackgroundEffect =
    NewThreadBackgroundEffect::None;

/// `DEFAULT_SIDEBAR_TAB_ORDER`.
pub const DEFAULT_SIDEBAR_TAB_ORDER: [SidebarTabId; 4] = [
    SidebarTabId::Sessions,
    SidebarTabId::Inbox,
    SidebarTabId::Files,
    SidebarTabId::Changes,
];

pub const THEME_HUE_MIN: f64 = 0.0;
pub const THEME_HUE_MAX: f64 = 360.0;
pub const THEME_HUE_DEFAULT: f64 = 240.0;

pub const THEME_SATURATION_MIN: f64 = 0.0;
pub const THEME_SATURATION_MAX: f64 = 100.0;
pub const THEME_SATURATION_DEFAULT: f64 = 0.0;

pub const THEME_DARK_LIGHTNESS_MIN: f64 = 0.0;
pub const THEME_DARK_LIGHTNESS_MAX: f64 = 30.0;
pub const THEME_DARK_LIGHTNESS_DEFAULT: f64 = 9.0;

pub const SIDEBAR_OPACITY_MIN: f64 = 0.15;
pub const SIDEBAR_OPACITY_MAX: f64 = 1.0;
pub const SIDEBAR_OPACITY_DEFAULT: f64 = 0.85;

pub const MAIN_OPACITY_MIN: f64 = 0.15;
pub const MAIN_OPACITY_MAX: f64 = 1.0;
pub const MAIN_OPACITY_DEFAULT: f64 = 0.85;

pub const SIDEBAR_BLUR_MIN: f64 = 1.0;
pub const SIDEBAR_BLUR_MAX: f64 = 64.0;
pub const SIDEBAR_BLUR_DEFAULT: f64 = 24.0;

pub const PROJECT_RAIL_WIDTH_MIN: f64 = 180.0;
pub const PROJECT_RAIL_WIDTH_MAX: f64 = 360.0;
pub const PROJECT_RAIL_WIDTH_DEFAULT: f64 = 200.0;

pub const CHAT_BACKGROUND_OPACITY_MIN: f64 = 0.05;
pub const CHAT_BACKGROUND_OPACITY_MAX: f64 = 0.65;
pub const CHAT_BACKGROUND_OPACITY_DEFAULT: f64 = 0.24;
pub const CHAT_BACKGROUND_BLUR_MIN: f64 = 0.0;
pub const CHAT_BACKGROUND_BLUR_MAX: f64 = 40.0;
pub const CHAT_BACKGROUND_BLUR_DEFAULT: f64 = 0.0;
pub const CHAT_BACKGROUND_EMPTY_OPACITY_DEFAULT: f64 = CHAT_BACKGROUND_OPACITY_DEFAULT;
pub const CHAT_BACKGROUND_SESSION_OPACITY_DEFAULT: f64 = CHAT_BACKGROUND_OPACITY_DEFAULT;
pub const CHAT_BACKGROUND_SCOPE_DEFAULT: ChatBackgroundScope = ChatBackgroundScope::All;

pub const PROJECT_RAIL_OPEN_DEFAULT: bool = true;
pub const SESSION_SIDEBAR_OPEN_DEFAULT: bool = true;

/// `BODY_GLASS_DEFAULT`: main pane glass is on except on Linux.
pub const fn body_glass_default(platform: Platform) -> bool {
    !platform.is_linux()
}

pub const UI_SCALE_DEFAULT: f64 = 1.0;
pub const UI_SCALE_MIN: f64 = 0.5;
pub const UI_SCALE_MAX: f64 = 2.0;
pub const UI_SCALE_STEP: f64 = 0.1;

/// `UI_SCALE_PERCENTS`: every supported scale as a whole percent, 50 to 200.
pub fn ui_scale_percents() -> Vec<i64> {
    let steps = js::round((UI_SCALE_MAX - UI_SCALE_MIN) / UI_SCALE_STEP) as i64 + 1;
    (0..steps)
        .map(|i| js::round((UI_SCALE_MIN + i as f64 * UI_SCALE_STEP) * 100.0) as i64)
        .collect()
}

fn rounded(value: f64, min: f64, max: f64) -> i64 {
    js::round(clamp(value, min, max)) as i64
}

/// Hue as stored and applied: rounded and clamped to 0..=360.
pub fn clamp_theme_hue(value: f64) -> i64 {
    rounded(value, THEME_HUE_MIN, THEME_HUE_MAX)
}

/// Saturation as stored and applied: rounded and clamped to 0..=100.
pub fn clamp_theme_saturation(value: f64) -> i64 {
    rounded(value, THEME_SATURATION_MIN, THEME_SATURATION_MAX)
}

/// Dark-mode background lightness: rounded and clamped to 0..=30.
pub fn clamp_theme_dark_lightness(value: f64) -> i64 {
    rounded(value, THEME_DARK_LIGHTNESS_MIN, THEME_DARK_LIGHTNESS_MAX)
}

/// Sidebar opacity, clamped to 0.15..=1.
pub fn clamp_sidebar_opacity(value: f64) -> f64 {
    clamp(value, SIDEBAR_OPACITY_MIN, SIDEBAR_OPACITY_MAX)
}

/// Main pane opacity, clamped to 0.15..=1.
pub fn clamp_main_opacity(value: f64) -> f64 {
    clamp(value, MAIN_OPACITY_MIN, MAIN_OPACITY_MAX)
}

/// Window blur radius: rounded and clamped to 1..=64.
pub fn clamp_sidebar_blur(value: f64) -> i64 {
    rounded(value, SIDEBAR_BLUR_MIN, SIDEBAR_BLUR_MAX)
}

/// Project rail width: rounded and clamped to 180..=360.
pub fn clamp_project_rail_width(value: f64) -> i64 {
    rounded(value, PROJECT_RAIL_WIDTH_MIN, PROJECT_RAIL_WIDTH_MAX)
}

/// Chat background opacity, clamped to 0.05..=0.65.
pub fn clamp_chat_background_opacity(value: f64) -> f64 {
    clamp(
        value,
        CHAT_BACKGROUND_OPACITY_MIN,
        CHAT_BACKGROUND_OPACITY_MAX,
    )
}

/// Chat background blur: rounded and clamped to 0..=40. Zero means no filter.
pub fn clamp_chat_background_blur(value: f64) -> i64 {
    rounded(value, CHAT_BACKGROUND_BLUR_MIN, CHAT_BACKGROUND_BLUR_MAX)
}

/// `isHexColor`: `#rrggbb`.
pub fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// `normalizeAccentColor`: a lowercase `#rrggbb`, or `None` for the default.
pub fn normalize_accent_color(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| is_hex_color(value))
        .map(str::to_lowercase)
}

/// `accentForeground`: black or white text, whichever reads on `color`.
pub fn accent_foreground(color: &str) -> &'static str {
    let channel = |offset: usize| {
        let value = u8::from_str_radix(color.get(offset..offset + 2).unwrap_or("00"), 16)
            .unwrap_or(0) as f64
            / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
    if luminance > 0.179 {
        "#000000"
    } else {
        "#ffffff"
    }
}

/// `resolveColorScheme`: `system` follows the OS appearance.
pub fn resolve_color_scheme(value: ThemePreference, system_is_light: bool) -> ColorScheme {
    match value {
        ThemePreference::Dark => ColorScheme::Dark,
        ThemePreference::Light => ColorScheme::Light,
        ThemePreference::System if system_is_light => ColorScheme::Light,
        ThemePreference::System => ColorScheme::Dark,
    }
}

/// `loadSidebarTabOrder`: known tabs in stored order, then any missing ones.
/// Anything malformed reads as the default order.
pub fn parse_sidebar_tab_order(raw: Option<&str>) -> Vec<SidebarTabId> {
    let default = DEFAULT_SIDEBAR_TAB_ORDER.to_vec();
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return default;
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return default;
    };
    let mut next: Vec<SidebarTabId> = items
        .iter()
        .filter_map(|item| item.as_str().and_then(SidebarTabId::from_str_opt))
        .collect();
    for id in DEFAULT_SIDEBAR_TAB_ORDER {
        if !next.contains(&id) {
            next.push(id);
        }
    }
    if next.len() == DEFAULT_SIDEBAR_TAB_ORDER.len() {
        next
    } else {
        default
    }
}

/// `loadChatBackgroundPath`: the trimmed path, or `None` when empty.
pub fn parse_chat_background_path(raw: Option<&str>) -> Option<String> {
    raw.map(js::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

/// `normalizeUiScale`: clamp to 0.5..=2 and round to one decimal. Anything
/// that is not a finite number reads as 1.
pub fn normalize_ui_scale(value: f64) -> f64 {
    if !value.is_finite() {
        return UI_SCALE_DEFAULT;
    }
    js::round(clamp(value, UI_SCALE_MIN, UI_SCALE_MAX) * 10.0) / 10.0
}

/// `loadUiScale`.
pub fn parse_ui_scale(raw: Option<&str>) -> f64 {
    match raw {
        None => UI_SCALE_DEFAULT,
        Some(raw) => normalize_ui_scale(js::parse_number(raw).unwrap_or(f64::NAN)),
    }
}

/// `zoomInUiScale`.
pub fn zoom_in_ui_scale(current: f64) -> f64 {
    normalize_ui_scale(current + UI_SCALE_STEP)
}

/// `zoomOutUiScale`.
pub fn zoom_out_ui_scale(current: f64) -> f64 {
    normalize_ui_scale(current - UI_SCALE_STEP)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiScaleCommand {
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// `uiScaleCommand`: the browser-standard zoom keys. `+` arrives as `=` or
/// `+` depending on layout, `-` as `-` or `_`, and numpad keys report by code.
pub fn ui_scale_command(key: &str, code: &str) -> Option<UiScaleCommand> {
    if code == "NumpadAdd" || key == "+" || key == "=" {
        return Some(UiScaleCommand::ZoomIn);
    }
    if code == "NumpadSubtract" || key == "-" || key == "_" {
        return Some(UiScaleCommand::ZoomOut);
    }
    if code == "Numpad0" || key == "0" {
        return Some(UiScaleCommand::ZoomReset);
    }
    None
}

/// Appearance settings. Each field names its storage key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    /// `ACCENT_COLOR_KEY`: lowercase `#rrggbb`, or `None` for no accent.
    pub accent_color: Option<String>,
    /// `THEME_HUE_KEY`.
    pub theme_hue: i64,
    /// `THEME_SATURATION_KEY`.
    pub theme_saturation: i64,
    /// `THEME_DARK_LIGHTNESS_KEY`.
    pub theme_dark_lightness: i64,
    /// `OPACITY_KEY`.
    pub sidebar_opacity: f64,
    /// `MAIN_OPACITY_KEY`, falling back to `OPACITY_KEY`.
    pub main_opacity: f64,
    /// `BLUR_KEY`: the window blur radius.
    pub sidebar_blur: i64,
    /// `PROJECT_RAIL_OPEN_KEY`.
    pub project_rail_open: bool,
    /// `SESSION_SIDEBAR_OPEN_KEY`.
    pub session_sidebar_open: bool,
    /// `BODY_KEY`: main pane glass.
    pub body_glass: bool,
    /// `SCHEME_KEY`.
    pub theme_preference: ThemePreference,
    /// `SIDEBAR_TAB_ORDER_KEY`.
    pub sidebar_tab_order: Vec<SidebarTabId>,
    /// `PROJECT_RAIL_WIDTH_KEY`.
    pub project_rail_width: i64,
    /// `TRANSCRIPT_LAYOUT_KEY`.
    pub transcript_layout: TranscriptLayout,
    /// `TRANSCRIPT_ANCHOR_KEY`: anchor prompts to the top.
    pub transcript_anchor: bool,
    /// `CHAT_BACKGROUND_PATH_KEY`: the app-owned background image.
    pub chat_background_path: Option<String>,
    /// `CHAT_BACKGROUND_EMPTY_OPACITY_KEY`, falling back to `CHAT_BACKGROUND_OPACITY_KEY`.
    pub chat_background_empty_opacity: f64,
    /// `CHAT_BACKGROUND_SESSION_OPACITY_KEY`, falling back to `CHAT_BACKGROUND_OPACITY_KEY`.
    pub chat_background_session_opacity: f64,
    /// `CHAT_BACKGROUND_SCOPE_KEY`.
    pub chat_background_scope: ChatBackgroundScope,
    /// `CHAT_BACKGROUND_BLUR_KEY`.
    pub chat_background_blur: i64,
    /// `NEW_THREAD_BACKGROUND_EFFECT_KEY`.
    pub new_thread_background_effect: NewThreadBackgroundEffect,
    /// `CHANGES_VIEW_KEY`.
    pub changes_view: ChangesView,
    /// `DIFF_PALETTE_KEY`.
    pub diff_palette: DiffPalette,
    /// `SHOW_EXCLUDED_FILES_KEY`.
    pub show_excluded_files: bool,
    /// `UI_SCALE_KEY`: the interface scale, 0.5 to 2 in steps of 0.1.
    pub ui_scale: f64,
}

impl AppearanceSettings {
    /// The defaults for `platform`. Only main pane glass differs by platform.
    pub fn defaults_for(platform: Platform) -> Self {
        Self {
            accent_color: None,
            theme_hue: clamp_theme_hue(THEME_HUE_DEFAULT),
            theme_saturation: clamp_theme_saturation(THEME_SATURATION_DEFAULT),
            theme_dark_lightness: clamp_theme_dark_lightness(THEME_DARK_LIGHTNESS_DEFAULT),
            sidebar_opacity: SIDEBAR_OPACITY_DEFAULT,
            main_opacity: MAIN_OPACITY_DEFAULT,
            sidebar_blur: clamp_sidebar_blur(SIDEBAR_BLUR_DEFAULT),
            project_rail_open: PROJECT_RAIL_OPEN_DEFAULT,
            session_sidebar_open: SESSION_SIDEBAR_OPEN_DEFAULT,
            body_glass: body_glass_default(platform),
            theme_preference: THEME_PREFERENCE_DEFAULT,
            sidebar_tab_order: DEFAULT_SIDEBAR_TAB_ORDER.to_vec(),
            project_rail_width: clamp_project_rail_width(PROJECT_RAIL_WIDTH_DEFAULT),
            transcript_layout: TRANSCRIPT_LAYOUT_DEFAULT,
            transcript_anchor: TRANSCRIPT_ANCHOR_DEFAULT,
            chat_background_path: None,
            chat_background_empty_opacity: CHAT_BACKGROUND_EMPTY_OPACITY_DEFAULT,
            chat_background_session_opacity: CHAT_BACKGROUND_SESSION_OPACITY_DEFAULT,
            chat_background_scope: CHAT_BACKGROUND_SCOPE_DEFAULT,
            chat_background_blur: clamp_chat_background_blur(CHAT_BACKGROUND_BLUR_DEFAULT),
            new_thread_background_effect: NEW_THREAD_BACKGROUND_EFFECT_DEFAULT,
            changes_view: CHANGES_VIEW_DEFAULT,
            diff_palette: DIFF_PALETTE_DEFAULT,
            show_excluded_files: SHOW_EXCLUDED_FILES_DEFAULT,
            ui_scale: UI_SCALE_DEFAULT,
        }
    }

    /// Read every appearance setting from the old localStorage values, the
    /// way each `load*` function did.
    pub fn from_local_storage(get: impl Fn(&str) -> Option<String>, platform: Platform) -> Self {
        let number = |key: &str| read_number(get(key).as_deref());
        let flag = |key: &str, default: bool| read_flag(get(key).as_deref()).unwrap_or(default);
        let background_opacity = |key: &str| {
            clamp_chat_background_opacity(
                number(key)
                    .or_else(|| number(CHAT_BACKGROUND_OPACITY_KEY))
                    .unwrap_or(CHAT_BACKGROUND_OPACITY_DEFAULT),
            )
        };
        Self {
            accent_color: normalize_accent_color(get(ACCENT_COLOR_KEY).as_deref()),
            theme_hue: clamp_theme_hue(number(THEME_HUE_KEY).unwrap_or(THEME_HUE_DEFAULT)),
            theme_saturation: clamp_theme_saturation(
                number(THEME_SATURATION_KEY).unwrap_or(THEME_SATURATION_DEFAULT),
            ),
            theme_dark_lightness: clamp_theme_dark_lightness(
                number(THEME_DARK_LIGHTNESS_KEY).unwrap_or(THEME_DARK_LIGHTNESS_DEFAULT),
            ),
            sidebar_opacity: clamp_sidebar_opacity(
                number(OPACITY_KEY).unwrap_or(SIDEBAR_OPACITY_DEFAULT),
            ),
            main_opacity: clamp_main_opacity(
                number(MAIN_OPACITY_KEY)
                    .or_else(|| number(OPACITY_KEY))
                    .unwrap_or(MAIN_OPACITY_DEFAULT),
            ),
            sidebar_blur: clamp_sidebar_blur(number(BLUR_KEY).unwrap_or(SIDEBAR_BLUR_DEFAULT)),
            project_rail_open: flag(PROJECT_RAIL_OPEN_KEY, PROJECT_RAIL_OPEN_DEFAULT),
            session_sidebar_open: flag(SESSION_SIDEBAR_OPEN_KEY, SESSION_SIDEBAR_OPEN_DEFAULT),
            body_glass: flag(BODY_KEY, body_glass_default(platform)),
            theme_preference: ThemePreference::parse(get(SCHEME_KEY).as_deref()),
            sidebar_tab_order: parse_sidebar_tab_order(get(SIDEBAR_TAB_ORDER_KEY).as_deref()),
            project_rail_width: clamp_project_rail_width(
                number(PROJECT_RAIL_WIDTH_KEY).unwrap_or(PROJECT_RAIL_WIDTH_DEFAULT),
            ),
            transcript_layout: TranscriptLayout::parse(get(TRANSCRIPT_LAYOUT_KEY).as_deref()),
            transcript_anchor: flag(TRANSCRIPT_ANCHOR_KEY, TRANSCRIPT_ANCHOR_DEFAULT),
            chat_background_path: parse_chat_background_path(
                get(CHAT_BACKGROUND_PATH_KEY).as_deref(),
            ),
            chat_background_empty_opacity: background_opacity(CHAT_BACKGROUND_EMPTY_OPACITY_KEY),
            chat_background_session_opacity: background_opacity(
                CHAT_BACKGROUND_SESSION_OPACITY_KEY,
            ),
            chat_background_scope: ChatBackgroundScope::parse(
                get(CHAT_BACKGROUND_SCOPE_KEY).as_deref(),
            ),
            chat_background_blur: clamp_chat_background_blur(
                number(CHAT_BACKGROUND_BLUR_KEY).unwrap_or(CHAT_BACKGROUND_BLUR_DEFAULT),
            ),
            new_thread_background_effect: NewThreadBackgroundEffect::parse(
                get(NEW_THREAD_BACKGROUND_EFFECT_KEY).as_deref(),
            ),
            changes_view: ChangesView::parse(get(CHANGES_VIEW_KEY).as_deref()),
            diff_palette: DiffPalette::parse(get(DIFF_PALETTE_KEY).as_deref()),
            show_excluded_files: flag(SHOW_EXCLUDED_FILES_KEY, SHOW_EXCLUDED_FILES_DEFAULT),
            ui_scale: parse_ui_scale(get(UI_SCALE_KEY).as_deref()),
        }
    }

    /// `saveChatBackgroundOpacity`: one value for both scopes.
    pub fn set_chat_background_opacity(&mut self, value: f64) {
        let next = clamp_chat_background_opacity(value);
        self.chat_background_empty_opacity = next;
        self.chat_background_session_opacity = next;
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self::defaults_for(Platform::current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn stored(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    fn load(pairs: &[(&str, &str)]) -> AppearanceSettings {
        AppearanceSettings::from_local_storage(stored(pairs), Platform::Mac)
    }

    // accent color setting
    #[test]
    fn accent_defaults_to_the_original_neutral_appearance() {
        assert_eq!(ACCENT_COLOR_DEFAULT, None);
        assert_eq!(load(&[]).accent_color, None);
    }

    #[test]
    fn persists_normalized_hex_colors_and_clears_default_or_invalid_values() {
        assert_eq!(
            normalize_accent_color(Some("#AABBCC")).as_deref(),
            Some("#aabbcc")
        );
        assert_eq!(
            load(&[(ACCENT_COLOR_KEY, "#AABBCC")])
                .accent_color
                .as_deref(),
            Some("#aabbcc")
        );
        assert_eq!(normalize_accent_color(None), None);
        assert_eq!(normalize_accent_color(Some("tomato")), None);
        assert_eq!(load(&[(ACCENT_COLOR_KEY, "tomato")]).accent_color, None);
        assert_eq!(accent_foreground("#ffffff"), "#000000");
        assert_eq!(accent_foreground("#1a1a40"), "#ffffff");
    }

    // transcript layout setting
    #[test]
    fn transcript_layout_defaults_persists_and_ignores_unknown_values() {
        assert_eq!(TRANSCRIPT_LAYOUT_DEFAULT, TranscriptLayout::Chat);
        assert_eq!(load(&[]).transcript_layout, TranscriptLayout::Chat);
        assert_eq!(
            load(&[(TRANSCRIPT_LAYOUT_KEY, "full")]).transcript_layout,
            TranscriptLayout::Full
        );
        assert_eq!(
            load(&[(TRANSCRIPT_LAYOUT_KEY, "bubbles")]).transcript_layout,
            TranscriptLayout::Chat
        );
    }

    // transcript prompt-to-top and show excluded files settings
    #[test]
    fn switches_default_and_persist() {
        assert!(load(&[]).transcript_anchor);
        assert!(!load(&[(TRANSCRIPT_ANCHOR_KEY, "0")]).transcript_anchor);
        assert!(!load(&[]).show_excluded_files);
        assert!(load(&[(SHOW_EXCLUDED_FILES_KEY, "1")]).show_excluded_files);
        assert!(load(&[]).project_rail_open);
        assert!(load(&[]).session_sidebar_open);
        assert!(AppearanceSettings::from_local_storage(stored(&[]), Platform::Mac).body_glass);
        assert!(!AppearanceSettings::from_local_storage(stored(&[]), Platform::Linux).body_glass);
        assert!(
            AppearanceSettings::from_local_storage(stored(&[(BODY_KEY, "1")]), Platform::Linux)
                .body_glass
        );
    }

    // chat background setting
    #[test]
    fn stores_and_clears_the_app_owned_background_path() {
        assert_eq!(load(&[]).chat_background_path, None);
        let path = "/app-data/backgrounds/chat-background.webp";
        assert_eq!(
            load(&[(CHAT_BACKGROUND_PATH_KEY, path)])
                .chat_background_path
                .as_deref(),
            Some(path)
        );
        assert_eq!(
            load(&[(CHAT_BACKGROUND_PATH_KEY, "  ")]).chat_background_path,
            None
        );
    }

    #[test]
    fn defaults_and_clamps_background_visibility() {
        assert_eq!(
            load(&[]).chat_background_empty_opacity,
            CHAT_BACKGROUND_OPACITY_DEFAULT
        );
        let mut settings = load(&[]);
        settings.set_chat_background_opacity(1.0);
        assert_eq!(settings.chat_background_empty_opacity, 0.65);
        settings.set_chat_background_opacity(0.0);
        assert_eq!(settings.chat_background_session_opacity, 0.05);
        let legacy = load(&[
            (CHAT_BACKGROUND_OPACITY_KEY, "0.4"),
            (CHAT_BACKGROUND_SESSION_OPACITY_KEY, "0.3"),
        ]);
        assert_eq!(legacy.chat_background_empty_opacity, 0.4);
        assert_eq!(legacy.chat_background_session_opacity, 0.3);
    }

    #[test]
    fn persists_where_the_background_is_shown() {
        assert_eq!(
            load(&[]).chat_background_scope,
            CHAT_BACKGROUND_SCOPE_DEFAULT
        );
        assert_eq!(
            load(&[(CHAT_BACKGROUND_SCOPE_KEY, "empty")]).chat_background_scope,
            ChatBackgroundScope::Empty
        );
        assert_eq!(
            load(&[(CHAT_BACKGROUND_SCOPE_KEY, "all")]).chat_background_scope,
            ChatBackgroundScope::All
        );
        assert_eq!(
            load(&[(CHAT_BACKGROUND_SCOPE_KEY, "transcript")]).chat_background_scope,
            CHAT_BACKGROUND_SCOPE_DEFAULT
        );
    }

    #[test]
    fn defaults_persists_and_validates_the_new_thread_background_effect() {
        assert_eq!(
            load(&[]).new_thread_background_effect,
            NEW_THREAD_BACKGROUND_EFFECT_DEFAULT
        );
        for effect in NewThreadBackgroundEffect::ALL {
            assert_eq!(
                load(&[(NEW_THREAD_BACKGROUND_EFFECT_KEY, effect.as_str())])
                    .new_thread_background_effect,
                *effect
            );
        }
        assert_eq!(
            load(&[(NEW_THREAD_BACKGROUND_EFFECT_KEY, "blur")]).new_thread_background_effect,
            NEW_THREAD_BACKGROUND_EFFECT_DEFAULT
        );
        assert_eq!(NewThreadBackgroundEffect::GradientBlur.label(), "Haze");
    }

    // diff palette setting
    #[test]
    fn defaults_persists_and_validates_the_diff_palette() {
        assert_eq!(DIFF_PALETTE_DEFAULT, DiffPalette::Default);
        assert_eq!(load(&[]).diff_palette, DiffPalette::Default);
        for (stored, palette) in [
            ("colorblind", DiffPalette::Colorblind),
            ("high-contrast", DiffPalette::HighContrast),
            ("default", DiffPalette::Default),
        ] {
            assert_eq!(load(&[(DIFF_PALETTE_KEY, stored)]).diff_palette, palette);
            assert_eq!(palette.as_str(), stored);
        }
        assert_eq!(
            load(&[(DIFF_PALETTE_KEY, "rainbow")]).diff_palette,
            DIFF_PALETTE_DEFAULT
        );
    }

    // theme preference setting
    #[test]
    fn theme_defaults_to_dark_and_ignores_unknown_values() {
        assert_eq!(THEME_PREFERENCE_DEFAULT, ThemePreference::Dark);
        assert_eq!(load(&[]).theme_preference, ThemePreference::Dark);
        for value in ThemePreference::ALL {
            assert_eq!(
                load(&[(SCHEME_KEY, value.as_str())]).theme_preference,
                *value
            );
        }
        assert_eq!(
            load(&[(SCHEME_KEY, "solarized")]).theme_preference,
            THEME_PREFERENCE_DEFAULT
        );
    }

    #[test]
    fn resolves_system_against_the_os_appearance() {
        assert_eq!(
            resolve_color_scheme(ThemePreference::System, true),
            ColorScheme::Light
        );
        assert_eq!(
            resolve_color_scheme(ThemePreference::System, false),
            ColorScheme::Dark
        );
        assert_eq!(
            resolve_color_scheme(ThemePreference::Dark, true),
            ColorScheme::Dark
        );
        assert_eq!(
            resolve_color_scheme(ThemePreference::Light, false),
            ColorScheme::Light
        );
    }

    // dark theme lightness setting
    #[test]
    fn defaults_to_the_existing_dark_background_lightness() {
        assert_eq!(THEME_DARK_LIGHTNESS_DEFAULT, 9.0);
        assert_eq!(load(&[]).theme_dark_lightness, 9);
    }

    #[test]
    fn persists_true_black_and_clamps_overly_light_values() {
        assert_eq!(
            load(&[(THEME_DARK_LIGHTNESS_KEY, "0")]).theme_dark_lightness,
            0
        );
        assert_eq!(
            load(&[(THEME_DARK_LIGHTNESS_KEY, "100")]).theme_dark_lightness,
            30
        );
        assert_eq!(clamp_theme_dark_lightness(100.0), 30);
    }

    #[test]
    fn main_opacity_falls_back_to_the_sidebar_value() {
        assert_eq!(load(&[(OPACITY_KEY, "0.5")]).main_opacity, 0.5);
        assert_eq!(
            load(&[(OPACITY_KEY, "0.5"), (MAIN_OPACITY_KEY, "0.7")]).main_opacity,
            0.7
        );
        assert_eq!(
            load(&[(MAIN_OPACITY_KEY, "junk")]).main_opacity,
            MAIN_OPACITY_DEFAULT
        );
        assert_eq!(load(&[(BLUR_KEY, "100")]).sidebar_blur, 64);
        assert_eq!(load(&[(THEME_HUE_KEY, "12.6")]).theme_hue, 13);
    }

    #[test]
    fn reads_the_sidebar_tab_order() {
        use SidebarTabId::*;
        assert_eq!(
            parse_sidebar_tab_order(None),
            [Sessions, Inbox, Files, Changes]
        );
        assert_eq!(
            parse_sidebar_tab_order(Some(r#"["files","x","inbox"]"#)),
            [Files, Inbox, Sessions, Changes]
        );
        assert_eq!(
            parse_sidebar_tab_order(Some(r#"["files","files"]"#)),
            [Sessions, Inbox, Files, Changes]
        );
        assert_eq!(
            parse_sidebar_tab_order(Some("{}")),
            [Sessions, Inbox, Files, Changes]
        );
    }

    // ui scale
    #[test]
    fn clamps_to_the_supported_range_and_rounds_to_one_decimal() {
        assert_eq!(normalize_ui_scale(1.0), 1.0);
        assert_eq!(normalize_ui_scale(1.05), 1.1);
        assert_eq!(normalize_ui_scale(0.0), UI_SCALE_MIN);
        assert_eq!(normalize_ui_scale(99.0), UI_SCALE_MAX);
        assert_eq!(normalize_ui_scale(f64::NAN), UI_SCALE_DEFAULT);
        assert_eq!(parse_ui_scale(Some("junk")), UI_SCALE_DEFAULT);
        assert_eq!(parse_ui_scale(None), UI_SCALE_DEFAULT);
    }

    #[test]
    fn steps_in_and_out_without_float_drift() {
        assert_eq!(zoom_in_ui_scale(1.0), 1.1);
        assert_eq!(zoom_out_ui_scale(1.1), 1.0);
        let mut scale = 1.0;
        for _ in 0..10 {
            scale = zoom_in_ui_scale(scale);
        }
        assert_eq!(scale, UI_SCALE_MAX);
        assert_eq!(zoom_in_ui_scale(UI_SCALE_MAX), UI_SCALE_MAX);
        assert_eq!(zoom_out_ui_scale(UI_SCALE_MIN), UI_SCALE_MIN);
    }

    #[test]
    fn maps_browser_standard_zoom_keys() {
        use UiScaleCommand::*;
        assert_eq!(ui_scale_command("+", "Equal"), Some(ZoomIn));
        assert_eq!(ui_scale_command("=", "Equal"), Some(ZoomIn));
        assert_eq!(ui_scale_command("Add", "NumpadAdd"), Some(ZoomIn));
        assert_eq!(ui_scale_command("-", "Minus"), Some(ZoomOut));
        assert_eq!(ui_scale_command("_", "Minus"), Some(ZoomOut));
        assert_eq!(
            ui_scale_command("Subtract", "NumpadSubtract"),
            Some(ZoomOut)
        );
        assert_eq!(ui_scale_command("0", "Digit0"), Some(ZoomReset));
        assert_eq!(ui_scale_command("0", "Numpad0"), Some(ZoomReset));
        assert_eq!(ui_scale_command("p", "KeyP"), None);
    }

    #[test]
    fn lists_every_supported_percent_step() {
        let percents = ui_scale_percents();
        assert_eq!(percents[0], 50);
        assert!(percents.contains(&100));
        assert_eq!(percents.last(), Some(&200));
        assert_eq!(percents.len(), 16);
    }

    #[test]
    fn defaults_serialize_as_camel_case() {
        let json = serde_json::to_value(AppearanceSettings::defaults_for(Platform::Mac)).unwrap();
        assert_eq!(json["themeHue"], 240);
        assert_eq!(json["newThreadBackgroundEffect"], "none");
        assert_eq!(json["sidebarTabOrder"][0], "sessions");
        let back: AppearanceSettings =
            serde_json::from_value(serde_json::json!({ "themeHue": 10 })).unwrap();
        assert_eq!(back.theme_hue, 10);
        assert_eq!(back.ui_scale, UI_SCALE_DEFAULT);
    }
}
