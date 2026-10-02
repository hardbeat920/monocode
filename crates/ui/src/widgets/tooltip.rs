//! Tooltips. The React app relies on native `title` tooltips; this is the
//! GPUI stand-in: a small glass label with an optional shortcut.

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div,
};

use crate::styled::{UiStyled as _, glass_backdrop};
use crate::widgets::kbd::kbd;
use crate::{Theme, u};

pub struct Tooltip {
    text: SharedString,
    shortcut: Option<SharedString>,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
        }
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Builds the tooltip view, for `.tooltip(move |window, cx| ...)`.
    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}

/// A tooltip builder for `StatefulInteractiveElement::tooltip`.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

/// A tooltip builder with a keyboard shortcut.
pub fn tooltip_with_shortcut(
    text: impl Into<SharedString>,
    shortcut: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    let shortcut = shortcut.into();
    move |window, cx| {
        Tooltip::new(text.clone())
            .shortcut(shortcut.clone())
            .build(window, cx)
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let tint = if theme.is_dark() {
            crate::color::with_alpha(theme.colors.background_base, 0.85)
        } else {
            theme.colors.background_base
        };
        // Offset from the pointer the way gpui-component's tooltip sits.
        div().pl(u(10.)).pt(u(18.)).child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(u(8.))
                .px(u(8.))
                .py(u(4.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.colors.popover_border)
                .shadow_lg()
                .text_px(theme.text.label)
                .leading(theme.leading.snug)
                .text_color(theme.colors.content)
                .child(glass_backdrop(theme.radius.md, 24., tint))
                .child(div().relative().child(self.text.clone()))
                .children(
                    self.shortcut
                        .clone()
                        .map(|shortcut| div().relative().child(kbd(shortcut))),
                ),
        )
    }
}
