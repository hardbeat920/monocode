//! Port of `MessageQueue` in src/features/sessions/ui/Composer.tsx: the
//! follow-ups waiting for the running turn, with Steer, edit, and remove,
//! and the paused banner after an interrupt.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_core::session::{MessageQueueStatus, QueuedMessage};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{IconName, Theme, icon, u};

use super::super::model::chat_context::{
    chat_context_summary, compose_chat_context, split_chat_context,
};
use super::super::prompt_input::{Enter, Escape, PromptInput};
use super::{Composer, ComposerEvent};

/// The row being edited and its editor.
#[derive(Default)]
pub struct MessageQueueState {
    pub editing: Option<String>,
    pub editor: Option<Entity<PromptInput>>,
}

/// The row label: the typed text, else the context summary, else a count.
pub fn queued_label(message: &QueuedMessage) -> String {
    let split = split_chat_context(&message.text);
    let text = monocode_core::js::trim(&split.text).to_string();
    if !text.is_empty() {
        return text;
    }
    let summary = chat_context_summary(&split.items);
    if !summary.is_empty() {
        return summary;
    }
    let count = message.attachments.len();
    format!("{count} attachment{}", if count == 1 { "" } else { "s" })
}

impl Composer {
    fn start_queue_edit(
        &mut self,
        message: &QueuedMessage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = split_chat_context(&message.text).text;
        let editor = cx.new(|cx| {
            let mut editor = PromptInput::new(window, cx);
            editor.reset_text(text, cx);
            editor
        });
        let theme = Theme::of(cx).clone();
        editor.update(cx, |editor, cx| {
            editor.set_padding([2., 6., 2., 6.], cx);
            editor.set_max_height(Some(96.), cx);
            editor.set_colors(
                super::super::prompt_input::PromptColors {
                    selection: monocode_ui::color::with_alpha(theme.user_accent_or_accent(), 0.30),
                    placeholder: theme.content(0.40),
                    caret: theme.colors.content,
                },
                cx,
            );
            editor.focus(window, cx);
        });
        self.queue.editing = Some(message.id.clone());
        self.queue.editor = Some(editor);
        cx.emit(ComposerEvent::QueuedMessageEditing(Some(
            message.id.clone(),
        )));
        cx.notify();
    }

    fn cancel_queue_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.queue = MessageQueueState::default();
        cx.emit(ComposerEvent::QueuedMessageEditing(None));
        self.focus(window, cx);
        cx.notify();
    }

    /// The editor holds only the typed text; the message keeps its chips.
    fn save_queue_edit(
        &mut self,
        message: &QueuedMessage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.queue.editor.as_ref() else {
            return;
        };
        let draft = editor.read(cx).text().to_string();
        let items = split_chat_context(&message.text).items;
        if monocode_core::js::trim(&draft).is_empty()
            && message.attachments.is_empty()
            && items.is_empty()
        {
            return;
        }
        cx.emit(ComposerEvent::EditQueuedMessage {
            id: message.id.clone(),
            text: compose_chat_context(&draft, &items),
        });
        self.queue = MessageQueueState::default();
        self.focus(window, cx);
        cx.notify();
    }

    fn queue_button(
        &self,
        id: SharedString,
        name: IconName,
        label: Option<&'static str>,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let mut button = div()
            .id(id)
            .flex()
            .flex_none()
            .h(u(24.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.md))
            .hover(move |style| style.bg(hover).text_color(ink))
            .child(icon(name).size(u(14.)).text_color(theme.content(0.55)));
        button = match label {
            Some(label) => button.gap(u(6.)).px(u(6.)).child(label),
            None => button.w(u(24.)),
        };
        button
    }

    pub(crate) fn render_message_queue(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let messages = self.props.queued_messages.clone();
        if messages.is_empty() {
            return None;
        }
        let theme = Theme::of(cx).clone();
        let paused = self.props.queue_status == Some(MessageQueueStatus::Paused);
        let mut card = div()
            .relative()
            .rounded_t(u(10.))
            .border_1()
            .border_b_0()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.03))
            .px(u(8.))
            .py(u(4.));
        if paused {
            card = card.child(
                div()
                    .flex()
                    .h(u(28.))
                    .items_center()
                    .gap(u(8.))
                    .border_b_1()
                    .border_color(theme.colors.stroke)
                    .text_px(12.)
                    .child(
                        icon(IconName::Pause)
                            .size(u(14.))
                            .text_color(theme.content(0.55)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child("Queue paused because you interrupted"),
                    )
                    .child(
                        self.queue_button(
                            "queue-resume".into(),
                            IconName::Play,
                            Some("Resume"),
                            &theme,
                        )
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::ResumeQueue))),
                    ),
            );
        }
        for (index, message) in messages.iter().enumerate() {
            let editing = self.queue.editing.as_deref() == Some(message.id.as_str());
            let mut row = div()
                .flex()
                .min_h(u(28.))
                .items_center()
                .gap(u(8.))
                .text_px(12.)
                .when(index > 0, |el| {
                    el.border_t_1().border_color(theme.colors.stroke)
                })
                .child(
                    icon(IconName::ListEnd)
                        .size(u(14.))
                        .flex_none()
                        .text_color(theme.content(0.55)),
                );
            let id = message.id.clone();
            if editing && let Some(editor) = self.queue.editor.clone() {
                let save_message = message.clone();
                let enter_message = message.clone();
                let items_empty = split_chat_context(&message.text).items.is_empty();
                let can_save = !monocode_core::js::trim(editor.read(cx).text()).is_empty()
                    || !message.attachments.is_empty()
                    || !items_empty;
                row = row
                    .child(
                        div()
                            .id("queue-editor")
                            .min_w_0()
                            .flex_1()
                            .min_h(u(24.))
                            .rounded(u(theme.radius.md))
                            .border_1()
                            .border_color(theme.content(0.15))
                            .bg(theme.content(0.05))
                            .text_px(12.)
                            .line_height(u(18.))
                            .text_color(theme.colors.content)
                            .capture_action(cx.listener(move |this, _: &Enter, window, cx| {
                                cx.stop_propagation();
                                this.save_queue_edit(&enter_message, window, cx);
                            }))
                            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                                cx.stop_propagation();
                                this.cancel_queue_edit(window, cx);
                            }))
                            .child(editor),
                    )
                    .child(
                        self.queue_button(
                            format!("queue-save-{id}").into(),
                            IconName::Check,
                            None,
                            &theme,
                        )
                        .when(!can_save, |el| el.opacity(0.3))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.save_queue_edit(&save_message, window, cx);
                            },
                        )),
                    )
                    .child(
                        self.queue_button(
                            format!("queue-cancel-{id}").into(),
                            IconName::X,
                            None,
                            &theme,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.cancel_queue_edit(window, cx)),
                        ),
                    );
            } else {
                let steer = id.clone();
                let edit = message.clone();
                let delete = id.clone();
                row = row
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(theme.content(0.80))
                            .child(queued_label(message)),
                    )
                    .child(
                        self.queue_button(
                            format!("queue-steer-{id}").into(),
                            IconName::CornerDownRight,
                            Some("Steer"),
                            &theme,
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(ComposerEvent::SteerQueuedMessage(steer.clone()));
                        })),
                    )
                    .child(
                        self.queue_button(
                            format!("queue-edit-{id}").into(),
                            IconName::Pencil,
                            None,
                            &theme,
                        )
                        .tooltip(monocode_ui::widgets::tooltip("Edit queued message"))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.start_queue_edit(&edit, window, cx);
                            },
                        )),
                    )
                    .child(
                        self.queue_button(
                            format!("queue-delete-{id}").into(),
                            IconName::Trash2,
                            None,
                            &theme,
                        )
                        .tooltip(monocode_ui::widgets::tooltip("Remove queued message"))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(ComposerEvent::DeleteQueuedMessage(delete.clone()));
                        })),
                    );
            }
            card = card.child(row);
        }
        let _ = window;
        Some(
            div()
                .px(u(8.))
                .text_color(theme.content(0.55))
                .child(card)
                .into_any_element(),
        )
    }
}
