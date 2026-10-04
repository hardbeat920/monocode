//! The window glass: the user's appearance settings as theme values, and
//! the desktop blur behind the window. Port of `syncNativeGlass` and the
//! blur radius call in src/features/settings/model/appearance.ts.
//!
//! On macOS the Tauri app drew glass with a transparent NSWindow and
//! `CGSSetWindowBackgroundBlurRadius` at the user's radius. GPUI's
//! `Blurred` background uses a visual-effect view with a fixed blur, so
//! here the window is `Transparent` and `monocode_platform::macos` sets the
//! radius on the GPUI window's raw handle.

use gpui::{App, Window, WindowBackgroundAppearance};
use monocode_core::appearance::AppearanceSettings as StoredAppearance;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};

/// The per-window key `monocode_platform::macos` files glass state under.
#[cfg(target_os = "macos")]
pub const WINDOW_KEY: &str = "main";

/// The stored appearance settings as the theme's values.
pub fn appearance_from_settings(stored: &StoredAppearance) -> AppearanceSettings {
    AppearanceSettings {
        theme_preference: ThemePreference::parse(Some(stored.theme_preference.as_str())),
        theme_hue: stored.theme_hue as f64,
        theme_saturation: stored.theme_saturation as f64,
        theme_dark_lightness: stored.theme_dark_lightness as f64,
        accent_color: stored.accent_color.clone(),
        sidebar_opacity: stored.sidebar_opacity as f32,
        main_opacity: stored.main_opacity as f32,
        sidebar_blur: stored.sidebar_blur as f32,
        body_glass: stored.body_glass,
        chat_background_empty_opacity: stored.chat_background_empty_opacity as f32,
        chat_background_session_opacity: stored.chat_background_session_opacity as f32,
        chat_background_blur: stored.chat_background_blur as f32,
        ui_scale: stored.ui_scale as f32,
        diff_palette: monocode_ui::DiffPalette::parse(Some(stored.diff_palette.as_str())),
    }
    .normalized()
}

/// `monocode_ui::sync_window` plus the native blur at the user's radius.
/// Call when the window opens and after every theme change.
pub fn sync_window(window: &mut Window, cx: &mut App) {
    monocode_ui::sync_window(window, cx);
    let theme = Theme::of(cx);
    let glass = theme.window_background() == WindowBackgroundAppearance::Blurred;
    let radius = theme.blur_radius();
    #[cfg(target_os = "macos")]
    {
        use monocode_platform::macos;
        if glass {
            window.set_background_appearance(WindowBackgroundAppearance::Transparent);
            macos::set_glass_enabled(WINDOW_KEY, true);
            let radius = radius
                .round()
                .clamp(macos::BLUR_MIN as f32, macos::BLUR_MAX as f32);
            macos::set_background_blur_radius(window, WINDOW_KEY, radius as u8);
        } else {
            macos::set_glass_enabled(WINDOW_KEY, false);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (glass, radius);
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::platform::Platform;

    #[test]
    fn stored_opacity_and_blur_reach_the_theme() {
        let mut stored = StoredAppearance::defaults_for(Platform::Mac);
        stored.sidebar_opacity = 0.7;
        stored.main_opacity = 0.4;
        stored.sidebar_blur = 30;
        let appearance = appearance_from_settings(&stored);
        assert!((appearance.sidebar_opacity - 0.7).abs() < 1e-6);
        assert!((appearance.main_opacity - 0.4).abs() < 1e-6);
        assert_eq!(appearance.sidebar_blur, 30.0);
    }
}
