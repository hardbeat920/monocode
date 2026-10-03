//! `SubmitOptions` from App.tsx: `ComposerTurnOptions` plus what managed
//! callers (the queue, CI repair, second opinions, automations, the
//! orchestrator, the app CLI) pass.

use std::rc::Rc;

use gpui::App;
use monocode_core::block::{PlanBuildTarget, SecondOpinionMeta, TurnIntent};
use monocode_core::handoff::HandoffComposerCard;
use monocode_core::notes::NoteComposerCard;
use monocode_core::orchestration::OrchestrationProposal;
use monocode_core::session::EditedResendRejection;
use monocode_core::settings::FollowUpBehavior;

use crate::submit::acceptance::OnSettled;
use crate::submit::ci_repair::CiRepairRequest;

/// `onResendRejected`: the provider did not take an edited resend.
pub type OnResendRejected = Rc<dyn Fn(EditedResendRejection, &mut App)>;

/// `SubmitOptions`.
#[derive(Clone, Default)]
pub struct SubmitOptions {
    /// `intent`; `None` is `default`.
    pub intent: Option<TurnIntent>,
    /// Replace the last user turn instead of appending one.
    pub resend_edited: bool,
    /// Promote an existing unsent transcript block instead of appending a turn.
    pub draft_block_id: Option<String>,
    pub on_resend_rejected: Option<OnResendRejected>,
    pub ci_repair: Option<CiRepairRequest>,
    /// Saved alongside the user turn; does not replace the submitted prompt.
    pub ci_context: Option<String>,
    pub second_opinion: Option<SecondOpinionMeta>,
    pub follow_up_behavior: Option<FollowUpBehavior>,
    /// `"noteCard" in options`: `Some(None)` sends without the session's chip.
    pub note_card: Option<Option<NoteComposerCard>>,
    /// `"handoffCard" in options`, like `note_card`.
    pub handoff_card: Option<Option<HandoffComposerCard>>,
    pub queued_message_id: Option<String>,
    pub plan_block_id: Option<String>,
    pub build_target: Option<PlanBuildTarget>,
    /// The orchestrator wrote this turn; it is hidden from the transcript.
    pub managed: bool,
    pub orchestration_retry: Option<OrchestrationProposal>,
    pub app_request_id: Option<String>,
    pub on_settled: Option<OnSettled>,
    /// Generate a fresh title even when this is not the session's first turn.
    pub refresh_title: bool,
    /// Internal guard for the retry after resolving a renamed project.
    pub project_location_ready: bool,
}

impl std::fmt::Debug for SubmitOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmitOptions")
            .field("intent", &self.intent)
            .field("resend_edited", &self.resend_edited)
            .field("draft_block_id", &self.draft_block_id)
            .field(
                "ci_repair",
                &self.ci_repair.as_ref().map(|repair| &repair.text),
            )
            .field("ci_context", &self.ci_context.is_some())
            .field("second_opinion", &self.second_opinion)
            .field("follow_up_behavior", &self.follow_up_behavior)
            .field("note_card", &self.note_card)
            .field("handoff_card", &self.handoff_card)
            .field("queued_message_id", &self.queued_message_id)
            .field("plan_block_id", &self.plan_block_id)
            .field("build_target", &self.build_target)
            .field("managed", &self.managed)
            .field("orchestration_retry", &self.orchestration_retry.is_some())
            .field("app_request_id", &self.app_request_id)
            .field("on_settled", &self.on_settled.is_some())
            .field("refresh_title", &self.refresh_title)
            .field("project_location_ready", &self.project_location_ready)
            .finish()
    }
}
