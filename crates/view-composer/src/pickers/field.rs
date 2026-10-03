//! Text fields for the pickers: gpui-base's unframed input, colored from
//! the MonoCode theme. The pickers draw their own field frames, so they
//! skip gpui-component's framed `Input`, which also needs a native window
//! handle that test windows lack.

use gpui::{AnyElement, App, Entity, IntoElement as _};
use gpui_base::input::{InputBaseState, InputEditorStyle, InputModeKind};
use monocode_ui::Theme;

/// The input with picker colors: content text, `content/40` placeholder,
/// and the accent selection gpui-component's inputs use.
pub fn plain_input<M: InputModeKind + 'static>(
    state: &Entity<InputBaseState<M>>,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let style = InputEditorStyle {
        foreground: theme.colors.content,
        muted_foreground: theme.content(0.40),
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        ..Default::default()
    };
    state.update(cx, |state, _| state.set_editor_style(style));
    state.clone().into_any_element()
}
