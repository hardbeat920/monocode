//! Submit's side of the attention package's `AttentionSubmit` hook: the
//! message queue, Steer, Resume, and the usage limit resume send their turns
//! through [`Submit::on_submit`]. Built only with the `attention` feature.

use std::rc::Rc;

use gpui::App;

use super::pipeline::{Submit, SubmitOptions};
use crate::attention::Attention;
use crate::attention::hooks::{AttentionSubmit, SubmitRequest};

/// `onSubmit` for attention.
pub struct SubmitForAttention;

impl AttentionSubmit for SubmitForAttention {
    fn submit(&self, request: SubmitRequest, cx: &mut App) {
        let Some(submit) = Submit::try_global(cx) else {
            return;
        };
        // Queue rows pass their cards explicitly, even when they have none
        // (`noteCard: head.noteCard`); other callers leave the session's.
        let explicit = request.queued_message_id.is_some();
        let options = SubmitOptions {
            follow_up_behavior: request.follow_up_behavior,
            queued_message_id: request.queued_message_id,
            note_card: explicit.then_some(request.note_card),
            handoff_card: explicit.then_some(request.handoff_card),
            intent: request.intent,
            ..SubmitOptions::default()
        };
        submit.update(cx, |submit, cx| {
            submit.on_submit(
                &request.session_id,
                &request.text,
                request.attachments,
                options,
                cx,
            );
        });
    }
}

/// Install [`SubmitForAttention`] as attention's submit hook. Call after both
/// `Submit::init` and the attention init.
pub fn install_attention_submit(cx: &mut App) {
    if cx.has_global::<Attention>() {
        Attention::set_submit(cx, Rc::new(SubmitForAttention));
    }
}
