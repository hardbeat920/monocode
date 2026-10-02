//! Port of the appearance settings in src/features/settings/model/appearance.ts
//! and src/features/settings/model/uiScale.ts.
//!
//! Only the values and their clamps live here. Loading and saving them is the
//! settings store's job; it hands an [`AppearanceSettings`] to
//! [`crate::theme::set_appearance`].

/// The resolved color scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    Dark,
    Light,
}

/// What the user picked: a fixed scheme or the OS appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePreference {
    #[default]
    Dark,
    Light,
    System,
}

impl ThemePreference {
    /// Parses a stored value. Unknown values fall back to the default.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("dark") => Self::Dark,
            Some("light") => Self::Light,
            Some("system") => Self::System,
            _ => THEME_PREFERENCE_DEFAULT,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }
}

/// `resolveColorScheme`: "system" follows the OS, explicit picks win.
pub fn resolve_color_scheme(value: ThemePreference, system: ColorScheme) -> ColorScheme {
    match value {
        ThemePreference::System => system,
        ThemePreference::Dark => ColorScheme::Dark,
        ThemePreference::Light => ColorScheme::Light,
    }
}

pub const THEME_PREFERENCE_DEFAULT: ThemePreference = ThemePreference::Dark;

pub const THEME_HUE_MIN: f64 = 0.0;
pub const THEME_HUE_MAX: f64 = 360.0;
pub const THEME_HUE_DEFAULT: f64 = 240.0;

pub const THEME_SATURATION_MIN: f64 = 0.0;
pub const THEME_SATURATION_MAX: f64 = 100.0;
pub const THEME_SATURATION_DEFAULT: f64 = 0.0;

pub const THEME_DARK_LIGHTNESS_MIN: f64 = 0.0;
pub const THEME_DARK_LIGHTNESS_MAX: f64 = 30.0;
pub const THEME_DARK_LIGHTNESS_DEFAULT: f64 = 9.0;

pub const SIDEBAR_OPACITY_MIN: f32 = 0.15;
pub const SIDEBAR_OPACITY_MAX: f32 = 1.0;
pub const SIDEBAR_OPACITY_DEFAULT: f32 = 0.85;

pub const MAIN_OPACITY_MIN: f32 = 0.15;
pub const MAIN_OPACITY_MAX: f32 = 1.0;
pub const MAIN_OPACITY_DEFAULT: f32 = 0.85;

pub const SIDEBAR_BLUR_MIN: f32 = 1.0;
pub const SIDEBAR_BLUR_MAX: f32 = 64.0;
pub const SIDEBAR_BLUR_DEFAULT: f32 = 24.0;

pub const PROJECT_RAIL_WIDTH_MIN: f32 = 180.0;
pub const PROJECT_RAIL_WIDTH_MAX: f32 = 360.0;
pub const PROJECT_RAIL_WIDTH_DEFAULT: f32 = 200.0;

/// Sidebar.tsx `MIN_WIDTH`, `MAX_WIDTH`, `DEFAULT_WIDTH`.
pub const SESSION_SIDEBAR_WIDTH_MIN: f32 = 260.0;
pub const SESSION_SIDEBAR_WIDTH_MAX: f32 = 560.0;
pub const SESSION_SIDEBAR_WIDTH_DEFAULT: f32 = 260.0;

pub const BODY_GLASS_DEFAULT: bool = !cfg!(target_os = "linux");

pub const CHAT_BACKGROUND_OPACITY_MIN: f32 = 0.05;
pub const CHAT_BACKGROUND_OPACITY_MAX: f32 = 0.65;
pub const CHAT_BACKGROUND_OPACITY_DEFAULT: f32 = 0.24;
pub const CHAT_BACKGROUND_BLUR_MIN: f32 = 0.0;
pub const CHAT_BACKGROUND_BLUR_MAX: f32 = 40.0;
pub const CHAT_BACKGROUND_BLUR_DEFAULT: f32 = 0.0;

pub const UI_SCALE_DEFAULT: f32 = 1.0;
pub const UI_SCALE_MIN: f32 = 0.5;
pub const UI_SCALE_MAX: f32 = 2.0;
pub const UI_SCALE_STEP: f32 = 0.1;

/// Every UI scale step as a whole percent, `UI_SCALE_PERCENTS` in uiScale.ts.
pub fn ui_scale_percents() -> Vec<u32> {
    let steps = ((UI_SCALE_MAX - UI_SCALE_MIN) / UI_SCALE_STEP).round() as u32 + 1;
    (0..steps)
        .map(|i| ((UI_SCALE_MIN + i as f32 * UI_SCALE_STEP) * 100.0).round() as u32)
        .collect()
}

fn round_step(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// `normalizeUiScale`: clamp and round to one decimal; junk becomes 1.
pub fn normalize_ui_scale(value: f32) -> f32 {
    if !value.is_finite() {
        return UI_SCALE_DEFAULT;
    }
    round_step(value.clamp(UI_SCALE_MIN, UI_SCALE_MAX))
}

pub fn zoom_in_ui_scale(current: f32) -> f32 {
    normalize_ui_scale(current + UI_SCALE_STEP)
}

pub fn zoom_out_ui_scale(current: f32) -> f32 {
    normalize_ui_scale(current - UI_SCALE_STEP)
}

/// Every appearance value the theme reads. Values are kept normalized:
/// build one with [`AppearanceSettings::default`] and the `with_*` setters,
/// or call [`AppearanceSettings::normalized`] after filling fields directly.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceSettings {
    pub theme_preference: ThemePreference,
    /// Hue in degrees, 0..=360.
    pub theme_hue: f64,
    /// Saturation in percent, 0..=100.
    pub theme_saturation: f64,
    /// Dark scheme background lightness in percent, 0..=30.
    pub theme_dark_lightness: f64,
    /// User accent as `#rrggbb`. `None` keeps the neutral look.
    pub accent_color: Option<String>,
    pub sidebar_opacity: f32,
    pub main_opacity: f32,
    /// Window background blur radius in points.
    pub sidebar_blur: f32,
    /// Lets the main pane show the window glass (`glass-body`).
    pub body_glass: bool,
    pub chat_background_empty_opacity: f32,
    pub chat_background_session_opacity: f32,
    pub chat_background_blur: f32,
    pub ui_scale: f32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_preference: THEME_PREFERENCE_DEFAULT,
            theme_hue: THEME_HUE_DEFAULT,
            theme_saturation: THEME_SATURATION_DEFAULT,
            theme_dark_lightness: THEME_DARK_LIGHTNESS_DEFAULT,
            accent_color: None,
            sidebar_opacity: SIDEBAR_OPACITY_DEFAULT,
            main_opacity: MAIN_OPACITY_DEFAULT,
            sidebar_blur: SIDEBAR_BLUR_DEFAULT,
            body_glass: BODY_GLASS_DEFAULT,
            chat_background_empty_opacity: CHAT_BACKGROUND_OPACITY_DEFAULT,
            chat_background_session_opacity: CHAT_BACKGROUND_OPACITY_DEFAULT,
            chat_background_blur: CHAT_BACKGROUND_BLUR_DEFAULT,
            ui_scale: UI_SCALE_DEFAULT,
        }
    }
}

impl AppearanceSettings {
    /// Applies the same clamps and rounding the TypeScript loaders apply.
    pub fn normalized(mut self) -> Self {
        self.theme_hue = self.theme_hue.clamp(THEME_HUE_MIN, THEME_HUE_MAX).round();
        self.theme_saturation = self
            .theme_saturation
            .clamp(THEME_SATURATION_MIN, THEME_SATURATION_MAX)
            .round();
        self.theme_dark_lightness = self
            .theme_dark_lightness
            .clamp(THEME_DARK_LIGHTNESS_MIN, THEME_DARK_LIGHTNESS_MAX)
            .round();
        self.accent_color = normalize_accent_color(self.accent_color.as_deref());
        self.sidebar_opacity = self
            .sidebar_opacity
            .clamp(SIDEBAR_OPACITY_MIN, SIDEBAR_OPACITY_MAX);
        self.main_opacity = self.main_opacity.clamp(MAIN_OPACITY_MIN, MAIN_OPACITY_MAX);
        self.sidebar_blur = self
            .sidebar_blur
            .clamp(SIDEBAR_BLUR_MIN, SIDEBAR_BLUR_MAX)
            .round();
        self.chat_background_empty_opacity = self
            .chat_background_empty_opacity
            .clamp(CHAT_BACKGROUND_OPACITY_MIN, CHAT_BACKGROUND_OPACITY_MAX);
        self.chat_background_session_opacity = self
            .chat_background_session_opacity
            .clamp(CHAT_BACKGROUND_OPACITY_MIN, CHAT_BACKGROUND_OPACITY_MAX);
        self.chat_background_blur = self
            .chat_background_blur
            .clamp(CHAT_BACKGROUND_BLUR_MIN, CHAT_BACKGROUND_BLUR_MAX)
            .round();
        self.ui_scale = normalize_ui_scale(self.ui_scale);
        self
    }

    /// The scheme in effect, given the OS appearance for "system".
    pub fn color_scheme(&self, system: ColorScheme) -> ColorScheme {
        resolve_color_scheme(self.theme_preference, system)
    }

    /// `syncNativeGlass`: glass is on in dark mode, and on Linux only when the
    /// body glass setting is on.
    pub fn native_glass(&self, scheme: ColorScheme) -> bool {
        scheme == ColorScheme::Dark && (!cfg!(target_os = "linux") || self.body_glass)
    }
}

/// `normalizeAccentColor`: lowercase `#rrggbb`, anything else is no accent.
pub fn normalize_accent_color(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| crate::color::is_hex_color(value))
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_color_defaults_to_the_original_neutral_appearance() {
        assert_eq!(AppearanceSettings::default().accent_color, None);
    }

    #[test]
    fn accent_color_normalizes_hex_and_clears_invalid_values() {
        assert_eq!(
            normalize_accent_color(Some("#AABBCC")).as_deref(),
            Some("#aabbcc")
        );
        assert_eq!(normalize_accent_color(None), None);
        assert_eq!(normalize_accent_color(Some("tomato")), None);
    }

    #[test]
    fn theme_preference_defaults_to_dark() {
        assert_eq!(THEME_PREFERENCE_DEFAULT, ThemePreference::Dark);
        assert_eq!(ThemePreference::parse(None), ThemePreference::Dark);
    }

    #[test]
    fn theme_preference_ignores_unknown_stored_values() {
        assert_eq!(ThemePreference::parse(Some("sepia")), ThemePreference::Dark);
        for value in [
            ThemePreference::Dark,
            ThemePreference::Light,
            ThemePreference::System,
        ] {
            assert_eq!(ThemePreference::parse(Some(value.as_str())), value);
        }
    }

    #[test]
    fn theme_preference_resolves_system_against_the_os_appearance() {
        assert_eq!(
            resolve_color_scheme(ThemePreference::System, ColorScheme::Light),
            ColorScheme::Light
        );
        assert_eq!(
            resolve_color_scheme(ThemePreference::System, ColorScheme::Dark),
            ColorScheme::Dark
        );
    }

    #[test]
    fn theme_preference_keeps_explicit_picks_regardless_of_the_os_appearance() {
        assert_eq!(
            resolve_color_scheme(ThemePreference::Dark, ColorScheme::Light),
            ColorScheme::Dark
        );
        assert_eq!(
            resolve_color_scheme(ThemePreference::Light, ColorScheme::Dark),
            ColorScheme::Light
        );
    }

    #[test]
    fn dark_theme_lightness_defaults_and_clamps() {
        assert_eq!(AppearanceSettings::default().theme_dark_lightness, 9.0);
        let black = AppearanceSettings {
            theme_dark_lightness: 0.0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(black.theme_dark_lightness, 0.0);
        let light = AppearanceSettings {
            theme_dark_lightness: 80.0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(light.theme_dark_lightness, THEME_DARK_LIGHTNESS_MAX);
    }

    #[test]
    fn chat_background_visibility_defaults_and_clamps() {
        let settings = AppearanceSettings {
            chat_background_empty_opacity: 0.9,
            chat_background_session_opacity: 0.0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(
            settings.chat_background_empty_opacity,
            CHAT_BACKGROUND_OPACITY_MAX
        );
        assert_eq!(
            settings.chat_background_session_opacity,
            CHAT_BACKGROUND_OPACITY_MIN
        );
        assert_eq!(
            AppearanceSettings::default().chat_background_empty_opacity,
            CHAT_BACKGROUND_OPACITY_DEFAULT
        );
    }

    #[test]
    fn ui_scale_clamps_to_the_supported_range_and_rounds_to_one_decimal() {
        assert_eq!(normalize_ui_scale(1.0), 1.0);
        assert_eq!(normalize_ui_scale(1.05), 1.1);
        assert_eq!(normalize_ui_scale(0.0), UI_SCALE_MIN);
        assert_eq!(normalize_ui_scale(99.0), UI_SCALE_MAX);
        assert_eq!(normalize_ui_scale(f32::NAN), UI_SCALE_DEFAULT);
    }

    #[test]
    fn ui_scale_steps_in_and_out_without_float_drift() {
        assert_eq!(zoom_in_ui_scale(1.0), 1.1);
        assert_eq!(zoom_out_ui_scale(1.1), 1.0);
        let mut scale = 1.0;
        for _ in 0..20 {
            scale = zoom_in_ui_scale(scale);
        }
        assert_eq!(scale, UI_SCALE_MAX);
        assert_eq!(zoom_in_ui_scale(UI_SCALE_MAX), UI_SCALE_MAX);
        assert_eq!(zoom_out_ui_scale(UI_SCALE_MIN), UI_SCALE_MIN);
    }

    #[test]
    fn ui_scale_lists_every_supported_percent_step() {
        let percents = ui_scale_percents();
        assert_eq!(percents[0], 50);
        assert!(percents.contains(&100));
        assert_eq!(*percents.last().unwrap(), 200);
        assert_eq!(percents.len(), 16);
    }

    #[test]
    fn native_glass_is_dark_only() {
        let settings = AppearanceSettings::default();
        assert!(!settings.native_glass(ColorScheme::Light));
        if !cfg!(target_os = "linux") {
            assert!(settings.native_glass(ColorScheme::Dark));
        }
    }
}
