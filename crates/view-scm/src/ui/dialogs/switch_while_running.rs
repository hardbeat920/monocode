//! Port of src/features/source-control/ui/SwitchWhileRunningDialog.tsx:
//! asks before changing a connected machine's branch under running
//! sessions.

use gpui::{
    Context, EventEmitter, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
};
use monocode_ui::widgets::{ModalSize, modal};
use monocode_ui::{UiStyled as _, u};

use crate::ui::dialogs::{DialogButton, dialog_button, error_line};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchWhileRunningEvent {
    Confirm,
    Cancel,
}

pub struct SwitchWhileRunningDialog {
    branch: String,
    creating: bool,
    /// The host's explanation, which names the running sessions.
    message: String,
    busy: bool,
    error: Option<String>,
}

impl EventEmitter<SwitchWhileRunningEvent> for SwitchWhileRunningDialog {}

impl SwitchWhileRunningDialog {
    pub fn new(branch: impl Into<String>, creating: bool, message: impl Into<String>) -> Self {
        Self {
            branch: branch.into(),
            creating,
            message: message.into(),
            busy: false,
            error: None,
        }
    }

    pub fn set_state(&mut self, busy: bool, error: Option<String>, cx: &mut Context<Self>) {
        self.busy = busy;
        self.error = error;
        cx.notify();
    }

    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(SwitchWhileRunningEvent::Confirm);
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(SwitchWhileRunningEvent::Cancel);
        }
    }
}

/// `message.replace(/^Host rejected request:\s*/, "")`.
pub fn host_message(message: &str) -> &str {
    message
        .strip_prefix("Host rejected request:")
        .map(str::trim_start)
        .unwrap_or(message)
}

impl Render for SwitchWhileRunningDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.creating {
            format!("Create {} anyway?", self.branch)
        } else {
            format!("Switch to {} anyway?", self.branch)
        };
        let this = cx.entity().downgrade();
        let on = |action: fn(&mut Self, &mut Context<Self>)| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, action);
            }
        };
        let mut body = div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .p(u(16.))
            .text_px(12.)
            .child(div().child(host_message(&self.message).to_string()));
        if let Some(error) = &self.error {
            body = body.child(error_line(error.clone(), cx));
        }
        body = body.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(dialog_button(
                    "running-cancel",
                    "Cancel",
                    DialogButton::Ghost,
                    self.busy,
                    false,
                    on(Self::cancel),
                    cx,
                ))
                .child(dialog_button(
                    "running-confirm",
                    if self.creating {
                        "Create and switch"
                    } else {
                        "Switch anyway"
                    },
                    DialogButton::Danger,
                    self.busy,
                    false,
                    on(Self::confirm),
                    cx,
                )),
        );
        let close = this.clone();
        modal("switch-while-running", title)
            .size(ModalSize::Sm)
            .on_close(move |_, cx| {
                let _ = close.update(cx, |this, cx| this.cancel(cx));
            })
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_host_prefix() {
        assert_eq!(
            host_message("Host rejected request:  Two sessions run here"),
            "Two sessions run here"
        );
        assert_eq!(host_message("Plain"), "Plain");
    }
}
