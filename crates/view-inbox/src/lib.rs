//! Inbox views. Port of src/features/inbox/ui: the Inbox page with its list,
//! filters, and item detail; the linked work item panel; pull request
//! checks, CI repair, comments, and diffs; the linked activity notice; and
//! the link dialog from src/features/sessions/ui.
//!
//! The views read and change inbox data through the traits in [`data`]. The
//! app implements them over the engine's inbox package.

pub mod data;
pub mod fixtures;
pub mod list;
pub mod model;
pub mod pr;
pub mod style;

/// Binds the markdown keys the item bodies and comments use. Call once,
/// after `monocode_ui::init`.
pub fn init(cx: &mut gpui::App) {
    monocode_markdown::init(cx);
}

thread_local! {
    static AUTOFOCUS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Whether views move focus into their fields when they open (the search
/// field of the fix picker, the link dialog's URL, the reply field). Hidden
/// headless windows have no native view for a focused text field, so the
/// gallery and the tests turn this off.
pub fn set_autofocus(autofocus: bool) {
    AUTOFOCUS.with(|cell| cell.set(autofocus));
}

pub(crate) fn autofocus() -> bool {
    AUTOFOCUS.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests;
