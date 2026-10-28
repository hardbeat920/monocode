//! File-action failures appear alongside the shell's other error cards.
use crate::theme::*;
use gpui::{Context, EventEmitter, Render, Window, div, prelude::*};

pub(crate) enum FileActionErrorEvent {
    Dismiss,
}
#[derive(Default)]
pub(crate) struct FileActionError {
    message: Option<String>,
}
impl EventEmitter<FileActionErrorEvent> for FileActionError {}
impl FileActionError {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn set_message(&mut self, message: Option<String>, cx: &mut Context<Self>) {
        if self.message != message {
            self.message = message;
            cx.notify();
        }
    }
    pub(crate) fn is_visible(&self) -> bool {
        self.message.is_some()
    }
}
impl Render for FileActionError {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(message) = &self.message else {
            return div().into_any_element();
        };
        super::errors::card(
            "file-action-error",
            message.clone(),
            icon_button(
                "dismiss-file-action-error",
                "Dismiss file action error",
                "close",
            )
            .debug_selector(|| "dismiss-file-action-error".into())
            .on_click(cx.listener(|_, _, _, cx| cx.emit(FileActionErrorEvent::Dismiss))),
        )
        .into_any_element()
    }
}
