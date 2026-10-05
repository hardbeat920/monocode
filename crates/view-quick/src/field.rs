//! Text fields for the quick composer. Every field is the composer's
//! `PromptInput`, so the prompt, the search boxes, and the branch name share
//! one caret, selection, and IME implementation. Owners intercept Enter,
//! the arrows, and Escape with `capture_action`.

use gpui::{App, AppContext as _, Entity, Window};
use monocode_ui::Theme;
use monocode_view_composer::composer::prompt_input::{PromptColors, PromptInput};

/// The theme's caret, selection, and `placeholder` colors (`content/35` by
/// default).
pub fn field_colors(cx: &App, placeholder_alpha: f32) -> PromptColors {
    let theme = Theme::of(cx);
    PromptColors {
        selection: theme.accent(0.35),
        placeholder: theme.content(placeholder_alpha),
        caret: theme.colors.content,
    }
}

/// A one-line field without padding, for the search rows.
pub fn search_field(placeholder: &str, window: &mut Window, cx: &mut App) -> Entity<PromptInput> {
    let colors = field_colors(cx, 0.35);
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        let mut input = PromptInput::new(window, cx);
        input.set_padding([0., 0., 0., 0.], cx);
        input.set_colors(colors, cx);
        input.set_placeholder(placeholder, cx);
        input
    })
}
