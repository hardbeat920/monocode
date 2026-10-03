//! Calls from attention into other packages, and attention's side of the
//! runtime hooks.
//!
//! `runtime::hooks` has no hook for sending a turn or for answering an
//! approval yet (see NEEDS.md), so attention defines the two traits it needs
//! here, with no-op defaults. The package that owns the call fills them in
//! with `Attention::set_submit` and `Attention::set_approval_router`.

use std::collections::HashSet;

use gpui::{App, WeakEntity};
use monocode_core::HarnessId;
use monocode_core::attachment::Attachment;
use monocode_core::block::TurnIntent;
use monocode_core::handoff::HandoffComposerCard;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::notes::NoteComposerCard;
use monocode_core::session::Session;
use monocode_core::settings::FollowUpBehavior;
use monocode_core::user_question::UserQuestionReply;
use monocode_settings::Kv;

use super::notifier::Notifier;
use crate::runtime::hooks::AttentionHooks;

/// One `onSubmit(sessionId, text, attachments, options)` call, with the
/// options the queue and usage limit flows pass.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitRequest {
    pub session_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub follow_up_behavior: Option<FollowUpBehavior>,
    pub queued_message_id: Option<String>,
    pub note_card: Option<NoteComposerCard>,
    pub handoff_card: Option<HandoffComposerCard>,
    pub intent: Option<TurnIntent>,
}

impl SubmitRequest {
    /// A plain prompt with no options.
    pub fn text(session_id: &str, text: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            text: text.to_string(),
            attachments: Vec::new(),
            follow_up_behavior: None,
            queued_message_id: None,
            note_card: None,
            handoff_card: None,
            intent: None,
        }
    }
}

/// The submit pipeline, as the queue and the usage limit resume call it.
pub trait AttentionSubmit {
    /// `onSubmit`. Called outside `Sessions`.
    fn submit(&self, _request: SubmitRequest, _cx: &mut App) {}
}

/// Where approval and question answers go, and how the app opens the
/// session behind a notice. Every method is called outside `Sessions`.
pub trait ApprovalRouter {
    /// `remoteProjectFor(session.cwd)`: the session runs on a remote host.
    fn is_remote(&self, _session: &Session, _cx: &App) -> bool {
        false
    }

    /// `respondHarnessApproval`.
    fn respond_approval(
        &self,
        _harness: HarnessId,
        _session_id: &str,
        _request_id: i64,
        _decision: ApprovalDecision,
        _cx: &mut App,
    ) {
    }

    /// `respondHarnessQuestion`.
    fn respond_question(
        &self,
        _harness: HarnessId,
        _session_id: &str,
        _request_id: i64,
        _reply: &UserQuestionReply,
        _cx: &mut App,
    ) {
    }

    /// `keepHarnessQuestionOpen`: the user started answering, so the
    /// harness must not skip the question on its deadline.
    fn keep_question_open(
        &self,
        _harness: HarnessId,
        _session_id: &str,
        _request_id: i64,
        _cx: &mut App,
    ) {
    }

    /// `remoteSessionActions(sessionId)?.approve`.
    fn remote_approve(
        &self,
        _session_id: &str,
        _request_id: i64,
        _decision: ApprovalDecision,
        _cx: &mut App,
    ) {
    }

    /// `remoteSessionActions(sessionId)?.answer`.
    fn remote_answer(
        &self,
        _session_id: &str,
        _request_id: i64,
        _reply: &UserQuestionReply,
        _cx: &mut App,
    ) {
    }

    /// `orchestrator.forSession(sessionId)?.leadId`.
    fn orchestration_lead_for(&self, _session_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `setInspectedWorkerId`: show this worker in its lead's panel.
    fn inspect_worker(&self, _session_id: &str, _cx: &mut App) {}

    /// `focusOpenSession`: focus the tab showing the session. `false` when
    /// no tab shows it.
    fn focus_open_session(&self, _session_id: &str, _cx: &mut App) -> bool {
        false
    }

    /// `onSelectHistorySession`: open a stored session in a tab.
    fn open_history_session(&self, _session_id: &str, _cx: &mut App) {}
}

/// The defaults for both traits.
pub struct NoopAttentionHooks;

impl AttentionSubmit for NoopAttentionHooks {}
impl ApprovalRouter for NoopAttentionHooks {}

/// Attention's `runtime::hooks::AttentionHooks`. The runtime calls these
/// while it holds `Sessions`, so they read the `Notifier` and the settings,
/// never `Sessions`.
pub struct RuntimeAttentionHooks {
    pub(crate) notifier: WeakEntity<Notifier>,
    pub(crate) kv: Kv,
}

impl AttentionHooks for RuntimeAttentionHooks {
    fn sync_dock_badge(&self, sessions: &[Session], cx: &mut App) {
        if let Some(notifier) = self.notifier.upgrade() {
            notifier.update(cx, |notifier, _| notifier.sync_dock_badge(sessions));
        }
    }

    fn unseen_finished_ids(&self, cx: &App) -> HashSet<String> {
        self.notifier
            .upgrade()
            .map(|notifier| notifier.read(cx).unseen_finished_ids().clone())
            .unwrap_or_default()
    }

    fn live_agents_enabled(&self, _cx: &App) -> bool {
        monocode_settings::settings_store::load_live_agents_enabled(&self.kv)
    }
}
