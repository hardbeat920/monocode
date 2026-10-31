//! The one hover tooltip every button shares.
use crate::theme::{content, shell_fill};
use gpui::{
    Context, IntoElement, Render, SharedString, StatefulInteractiveElement, Window, div,
    prelude::*, px,
};

pub(crate) struct Tooltip {
    label: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(shell_fill())
            .border_1()
            .border_color(content(0.12))
            .whitespace_nowrap()
            .text_size(px(11.))
            .line_height(px(16.))
            .text_color(content(0.85))
            .child(self.label.clone())
    }
}

pub(crate) trait Tip: Sized {
    fn tip(self, label: impl Into<SharedString>) -> Self;
}

impl<E: StatefulInteractiveElement> Tip for E {
    fn tip(self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.tooltip(move |_, cx| {
            let label = label.clone();
            cx.new(|_| Tooltip { label }).into()
        })
    }
}
