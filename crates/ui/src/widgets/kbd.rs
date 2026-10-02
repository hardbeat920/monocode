//! Keyboard shortcut chip, as QuickComposer.tsx draws it:
//! `rounded border border-content/12 px-1 text-[10px] text-content/55`.

use gpui::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
};

use crate::styled::UiStyled as _;
use crate::{Theme, u};

#[derive(IntoElement)]
pub struct Kbd {
    keys: SharedString,
}

pub fn kbd(keys: impl Into<SharedString>) -> Kbd {
    Kbd { keys: keys.into() }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex_none()
            .px(u(4.))
            .rounded(u(theme.radius.sm))
            .border_1()
            .border_color(theme.content(0.12))
            .text_px(theme.text.micro)
            .leading(theme.leading.label)
            .text_color(theme.content(0.55))
            .child(self.keys)
    }
}
