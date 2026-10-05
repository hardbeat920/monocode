//! `submit`, `completeSubmit`, and `restoreDraft` from Composer.tsx: the
//! commands Send runs locally, the text a turn leaves with, and what the
//! composer keeps when the host rejects it.

use std::collections::HashSet;

use gpui::{Context, Window};
use monocode_core::Attachment;
use monocode_core::block::TurnIntent;
use monocode_core::inbox::compose_inbox_message;
use monocode_core::session::{ComposerTurnOptions, EditedResendRejection};

use super::super::host::{ComposerSubmission, ResendTicket};
use super::super::model::chat_context::{compose_chat_context, split_chat_context};
use super::super::model::commands::{
    consume_btw_command, consume_draft_command, consume_operator_command,
    consume_orchestrator_command, consume_plan_command, consume_session_folder_command,
    is_compact_command, is_mcp_command,
};
use super::super::model::mcp::{mcp_context_text, tagged_mcp_servers};
use super::super::model::skills::leading_native_command;
use super::{Composer, PendingResend};

impl Composer {
    /// `submit`: the Send button and Enter. Waits for pastes still reading.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.props.disabled || self.props.worktree_removed || self.submit_waiting.is_some() {
            return;
        }
        if self.pastes_in_flight > 0 {
            self.submit_waiting = Some(self.paste_generation);
            return;
        }
        self.complete_submit(window, cx);
    }

    /// A paste finished. Runs a Send that was waiting for it, unless a reset
    /// or an earlier send retired the draft meanwhile.
    pub(crate) fn paste_settled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pastes_in_flight = self.pastes_in_flight.saturating_sub(1);
        if self.pastes_in_flight > 0 {
            return;
        }
        if let Some(generation) = self.submit_waiting.take()
            && generation == self.paste_generation
        {
            self.complete_submit(window, cx);
        }
    }

    /// Clears what a sent or saved message leaves behind.
    fn clear_after_send(&mut self, cx: &mut Context<Self>) {
        self.paste_generation += 1;
        self.set_prompt(String::new(), 0, cx);
        self.report_draft_text("", cx);
        self.plus_open = false;
        self.slash = None;
        self.mention = None;
        self.creating_skill = false;
        self.create_error = None;
    }

    /// `completeSubmit`.
    pub(crate) fn complete_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.prompt_text(cx);
        if self.props.disabled || self.props.worktree_removed {
            return;
        }
        let host = self.host.clone();
        if is_mcp_command(&value) {
            self.mcp_insert_at = Some(0);
            self.set_prompt(String::new(), 0, cx);
            self.report_draft_text("", cx);
            self.slash = None;
            self.sync_has_value();
            self.open_mcp_picker(window, cx);
            return;
        }
        let context = self.context_items.clone();
        let draft_command = if self.props.can_save_draft {
            consume_draft_command(&value)
        } else {
            super::super::model::commands::ConsumedCommand {
                text: value.clone(),
                matched: false,
            }
        };
        if (self.modes.draft || draft_command.matched) && self.props.can_save_draft {
            let files = self.attachments.clone();
            let text = draft_command.text;
            if monocode_core::js::trim(&text).is_empty() && files.is_empty() && context.is_empty() {
                return;
            }
            let composed = mcp_context_text(
                &tagged_mcp_servers(&text, &self.selected_mcp),
                &compose_chat_context(&text, &context),
            );
            if !host.save_draft(composed, files, window, cx) {
                return;
            }
            self.clear_after_send(cx);
            self.context_items.clear();
            self.attachments.clear();
            self.modes.draft = false;
            self.selected_mcp.clear();
            self.save_mcp_tags(cx);
            self.sync_has_value();
            cx.notify();
            return;
        }
        let folder_command = consume_session_folder_command(&value);
        let btw_command = consume_btw_command(&value);
        if btw_command.matched
            && self.props.btw_enabled
            && self.attachments.is_empty()
            && self.props.inbox_card.is_none()
            && self.props.note_card.is_none()
            && self.props.handoff_card.is_none()
        {
            if !host.btw(btw_command.text, false, window, cx) {
                return;
            }
            self.clear_after_send(cx);
            self.sync_has_value();
            cx.notify();
            return;
        }
        if folder_command.matched && self.props.folders_enabled && !self.session_folder_selected {
            self.open_session_folder_picker(window, cx);
            return;
        }
        if is_compact_command(&value) {
            if !host.compact_context(window, cx) {
                return;
            }
            self.clear_after_send(cx);
            self.sync_has_value();
            cx.notify();
            return;
        }

        let command =
            consume_plan_command(if folder_command.matched && self.session_folder_selected {
                &folder_command.text
            } else {
                &value
            });
        let orchestrator = if !self.remote() && !self.props.hide_top_bar && !command.planning {
            consume_orchestrator_command(&command.text)
        } else {
            super::super::model::commands::ConsumedCommand {
                text: command.text.clone(),
                matched: false,
            }
        };
        let native = self.host.raw_slash_commands(self.props.harness)
            && leading_native_command(&orchestrator.text);
        let body = if native {
            orchestrator.text.clone()
        } else {
            compose_inbox_message(self.props.inbox_card.as_ref(), &orchestrator.text)
        };
        let text = compose_chat_context(&body, &context);
        let submitted_text = if self.modes.operator && !consume_operator_command(&text).matched {
            format!("/operator {text}")
        } else {
            text.clone()
        };
        let files = self.attachments.clone();
        if text.is_empty()
            && files.is_empty()
            && self.props.note_card.is_none()
            && self.props.handoff_card.is_none()
        {
            return;
        }
        // Clear the parent draft before submitting. The app can remount the
        // composer when the first message leaves an empty session; a draft
        // still holding the sent text would come back as the initial draft.
        let resend = if self.resend_edited {
            self.next_resend += 1;
            let ticket = ResendTicket(self.next_resend);
            self.pending_resend = Some(PendingResend {
                ticket,
                revision: self.draft_revision,
                text: text.clone(),
                files: files.clone(),
                borrowed: self.borrowed_attachment_ids.clone(),
                mcp: self.selected_mcp.clone(),
            });
            Some(ticket)
        } else {
            None
        };
        self.report_draft_text("", cx);
        let intent = if self.modes.plan || command.planning {
            TurnIntent::Plan
        } else if self.modes.orchestration || orchestrator.matched {
            TurnIntent::Orchestrate
        } else {
            TurnIntent::Default
        };
        let submission = ComposerSubmission {
            text: mcp_context_text(
                &tagged_mcp_servers(&submitted_text, &self.selected_mcp),
                &submitted_text,
            ),
            attachments: files.clone(),
            options: ComposerTurnOptions {
                intent: Some(intent),
                resend_edited: self.resend_edited.then_some(true),
                ..ComposerTurnOptions::default()
            },
            resend,
        };
        // The app can reject a turn before it is recorded (for example while
        // an orchestration is paused). Keep the text, files, and mode so
        // resolving the blocker never destroys the user's work.
        if !host.submit(submission, window, cx) {
            self.pending_resend = None;
            self.restore_draft(&text, files, None, window, cx);
            return;
        }
        self.clear_after_send(cx);
        self.context_items.clear();
        self.borrowed_attachment_ids.clear();
        self.attachments.clear();
        self.selected_mcp.clear();
        self.save_mcp_tags(cx);
        self.set_resend_edited(false, cx);
        self.modes = Default::default();
        self.session_folder_selected = false;
        self.session_folder_open = false;
        self.paste_error = None;
        self.sync_has_value();
        cx.notify();
    }

    /// `onResendRejected`: an edited resend failed after it left. Restores
    /// it unless newer text replaced it.
    pub fn resend_rejected(
        &mut self,
        ticket: ResendTicket,
        rejection: EditedResendRejection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_resend.take() else {
            return;
        };
        if pending.ticket != ticket {
            self.pending_resend = Some(pending);
            return;
        }
        if self.draft_revision != pending.revision {
            return;
        }
        self.restore_draft(
            &pending.text,
            pending.files,
            Some(pending.borrowed),
            window,
            cx,
        );
        self.selected_mcp = pending.mcp;
        self.save_mcp_tags(cx);
        self.set_resend_edited(!rejection.provider_rewound, cx);
        self.prompt
            .update(cx, |prompt, cx| prompt.invalidate_decorations(cx));
        cx.notify();
    }

    /// `restoreDraft`: put a message back, chips and files included.
    /// `borrowed` names attachments the composer must not revoke.
    pub(crate) fn restore_draft(
        &mut self,
        text: &str,
        next_attachments: Vec<Attachment>,
        borrowed: Option<HashSet<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = split_chat_context(text);
        let end = message.text.len();
        self.set_prompt(message.text.clone(), end, cx);
        self.context_items = message.items;
        self.report_draft_text(text, cx);
        let borrowed = borrowed.unwrap_or_else(|| self.borrowed_attachment_ids.clone());
        let next_ids: HashSet<&str> = next_attachments
            .iter()
            .map(|file| file.id.as_str())
            .collect();
        for file in std::mem::take(&mut self.attachments) {
            if next_ids.contains(file.id.as_str()) || self.borrowed_attachment_ids.remove(&file.id)
            {
                continue;
            }
            self.host.revoke_attachment(&file, cx);
        }
        self.borrowed_attachment_ids = next_attachments
            .iter()
            .filter(|file| borrowed.contains(&file.id))
            .map(|file| file.id.clone())
            .collect();
        self.attachments = next_attachments;
        self.sync_has_value();
        self.focus(window, cx);
        cx.notify();
    }
}
