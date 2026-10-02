//! MonoCode's visual foundation: the theme ported from src/styles/index.css
//! and appearance.ts, the icon set, the asset source, and shared widgets.
//!
//! Call [`init`] once at startup, after `gpui_component::init`, and pass
//! [`Assets`] to `Application::with_assets`. Views read colors and sizes from
//! [`Theme::of`] and lengths through [`u`].

pub mod appearance;
pub mod assets;
pub mod color;
pub mod file_icons;
pub mod icons;
pub mod styled;
pub mod theme;
pub mod units;
pub mod widgets;

pub use appearance::{AppearanceSettings, ColorScheme, ThemePreference};
pub use assets::Assets;
pub use file_icons::{FileTypeIcon, file_type_icon, folder_type_icon};
pub use icons::{IconName, ProviderLogo, icon, provider_logo};
pub use styled::UiStyled;
pub use theme::{Theme, set_appearance, set_system_scheme, sync_window};
pub use units::u;

/// Installs the theme global, the gpui-component theme, and the widget state
/// MonoCode's views use.
pub fn init(appearance: AppearanceSettings, cx: &mut gpui::App) {
    theme::init(appearance, cx);
    widgets::init(cx);
}
