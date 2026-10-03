//! Entity tests: ports of the inbox `*.test.ts` files that render the
//! views. Assertions read the views' state and the fakes' recorded calls
//! where the TypeScript read the DOM.

mod checks;
mod detail;
mod dialog;
mod list;
mod notice;
mod notification;
mod repair;

use gpui::{App, VisualTestContext};

/// The theme, the component keys, and this crate's keys.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
    monocode_editor::init(cx);
    crate::init(cx);
    crate::set_autofocus(false);
}

/// Lets effects settle, then draws the window, so render code runs.
pub fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    cx.run_until_parked();
}
