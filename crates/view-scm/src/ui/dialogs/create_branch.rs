//! Port of src/features/source-control/ui/CreateBranchDialog.tsx.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_ui::widgets::{ModalSize, modal};
use monocode_ui::{Theme, UiStyled as _, u};

use crate::ui::common::field_input;
use crate::ui::dialogs::{DialogButton, dialog_button, error_line};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateBranchEvent {
    Create(String),
    Cancel,
}

pub struct CreateBranchDialog {
    name: Entity<InputState>,
    busy: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CreateBranchEvent> for CreateBranchDialog {}

impl CreateBranchDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-branch"));
        name.update(cx, |state, cx| state.focus(window, cx));
        let subscriptions = vec![cx.subscribe_in(
            &name,
            window,
            |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        Self {
            name,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn name_input(&self) -> &Entity<InputState> {
        &self.name
    }

    pub fn trimmed(&self, cx: &gpui::App) -> String {
        self.name.read(cx).value().trim().to_string()
    }

    /// The owner runs the create; it reports progress and failure here.
    pub fn set_state(
        &mut self,
        busy: bool,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let failed = error.is_some() && self.error != error;
        self.busy = busy;
        self.error = error;
        self.name
            .update(cx, |state, cx| state.set_disabled(busy, cx));
        if failed {
            self.name.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.trimmed(cx);
        if name.is_empty() || self.busy {
            return;
        }
        cx.emit(CreateBranchEvent::Create(name));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(CreateBranchEvent::Cancel);
        }
    }
}

impl Render for CreateBranchDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let this = cx.entity().downgrade();
        let can_submit = !self.trimmed(cx).is_empty() && !self.busy;
        let field = field_input(&self.name, theme.content(0.05), window, cx)
            .when(self.busy, |el| el.opacity(0.5));
        let mut form = div().flex().flex_col().gap(u(16.)).p(u(16.)).child(
            div()
                .flex()
                .flex_col()
                .gap(u(6.))
                .child(
                    div()
                        .text_px(12.)
                        .medium()
                        .text_color(theme.content(0.70))
                        .child("Branch name"),
                )
                .child(field),
        );
        if let Some(error) = &self.error {
            form = form.child(error_line(error.clone(), cx));
        }
        let cancel = {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| this.cancel(cx));
            }
        };
        let submit = {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| this.submit(cx));
            }
        };
        form = form.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(dialog_button(
                    "cancel",
                    "Cancel",
                    DialogButton::Ghost,
                    self.busy,
                    false,
                    cancel,
                    cx,
                ))
                .child(dialog_button(
                    "create",
                    "Create branch",
                    DialogButton::Primary,
                    !can_submit,
                    self.busy,
                    submit,
                    cx,
                )),
        );
        let close = this.clone();
        modal("create-branch-dialog", "New branch")
            .description("Create and check out a branch in this project.")
            .size(ModalSize::Sm)
            .on_close(move |_, cx| {
                let _ = close.update(cx, |this, cx| this.cancel(cx));
            })
            .child(form)
    }
}
