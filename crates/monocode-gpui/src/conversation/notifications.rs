//! Read-only notification snapshots and request-specific approval actions.

use super::*;
use crate::notifications::approvals::{InputKind, PendingInput};

impl Conversation {
    pub(crate) fn notification_was_cancelled(&self) -> bool {
        self.notification_cancelled
    }
    pub(crate) fn pending_notification_inputs(&self, cx: &gpui::App) -> Vec<PendingInput> {
        self.approvals
            .iter()
            .map(|approval| PendingInput {
                request_id: approval.id.clone(),
                kind: InputKind::Approval,
                label: approval.title.clone(),
            })
            .chain(self.questions.iter().map(|(card, _)| {
                let question = card.read(cx);
                PendingInput {
                    request_id: question.id().to_owned(),
                    kind: InputKind::Question,
                    label: question.notification_label().to_owned(),
                }
            }))
            .collect()
    }

    /// The host also checks the active tab and pane before suppressing a notice.
    pub(crate) fn composer_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.input.focus_handle(cx).is_focused(window)
    }

    pub(crate) fn notification_reply_body(&self, cx: &gpui::App) -> String {
        self.transcript.read(cx).notification_reply_body()
    }

    /// Resolve the exact approval represented by the card, even if a newer
    /// request arrived or an older one was resolved while the card was visible.
    /// Questions must open their answer form; they cannot be approved generically.
    pub(crate) fn decide_notification_request(
        &mut self,
        request_id: &str,
        decision: Decision,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self
            .approvals
            .iter()
            .position(|approval| approval.id == request_id)
        else {
            return false;
        };
        if !matches!(
            decision,
            Decision::Allow | Decision::Session | Decision::Always | Decision::Deny
        ) || (matches!(decision, Decision::Session) && !self.approvals[index].can_remember)
            || (matches!(decision, Decision::Always)
                && self.approvals[index].remember_scope.is_none())
            || self.approval_ui.saving.contains(request_id)
        {
            return false;
        }
        let Some(controls) = &self.controls else {
            self.status = "The provider is no longer waiting for this approval".into();
            cx.notify();
            return false;
        };
        let remembering = matches!(decision, Decision::Always);
        if controls
            .send(Control {
                id: request_id.to_owned(),
                decision,
            })
            .is_err()
        {
            self.status = "The provider is no longer waiting for this approval".into();
            cx.notify();
            return false;
        }
        if remembering {
            self.approval_ui.saving.insert(request_id.to_owned());
        } else {
            self.approvals.remove(index);
        }
        self.approval_ui.errors.remove(request_id);
        self.status = if remembering {
            "Saving permission…"
        } else if !self.approvals.is_empty() {
            "Waiting for approval"
        } else if !self.questions.is_empty() {
            "Waiting for your answer"
        } else {
            "Working…"
        }
        .into();
        // Request-level observers need an update even if another request remains.
        cx.emit(SessionChanged);
        cx.notify();
        true
    }
}
