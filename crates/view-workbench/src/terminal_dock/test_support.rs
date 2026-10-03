//! Shared setup for the terminal dock's view tests.

use gpui::{App, VisualTestContext};

/// Installs gpui-component and the theme.
pub fn init(cx: &mut App) {
    if cx.has_global::<monocode_ui::Theme>() {
        return;
    }
    gpui_component::init(cx);
    monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
}

/// Draws twice so measured bounds settle.
pub fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}
