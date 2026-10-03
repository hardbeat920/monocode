//! Shared setup for the view tests.

use gpui::{App, VisualTestContext};

/// Installs gpui-component, the theme, and this crate's keys.
pub fn init(cx: &mut App) {
    if cx.has_global::<monocode_ui::Theme>() {
        return;
    }
    gpui_component::init(cx);
    monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
    super::init(cx);
}

/// Draws twice so window-wide listeners and measured bounds settle.
pub fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}
