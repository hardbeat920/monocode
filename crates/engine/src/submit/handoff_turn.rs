//! Port of src/features/sessions/model/handoffTurn.ts: ask the outgoing
//! provider for a short recap before a switch. Approvals are denied and
//! questions skipped, and the turn is cancelled after 45 seconds.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use monocode_core::harness_event::{ApprovalDecision, HarnessSessionInput, SendTurnInput};
use monocode_core::reducer::join_stream_text;
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{HarnessEvent, HarnessId, ModelSettings, RuntimeMode, js};
use monocode_harness::core::registry::{HarnessRegistry, event_sink};
use parking_lot::Mutex;

use super::handoff::build_outgoing_handoff_prompt;

/// `HANDOFF_TIMEOUT_MS`.
pub const HANDOFF_TIMEOUT: Duration = Duration::from_millis(45_000);

/// `requestOutgoingHandoff` input.
#[derive(Debug, Clone)]
pub struct OutgoingHandoffInput {
    pub harness: HarnessId,
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub model_settings: Option<ModelSettings>,
    pub provider_account_id: Option<String>,
    pub user_request: String,
}

/// `requestOutgoingHandoff`: the outgoing agent's recap, or an empty string
/// when the turn fails. The caller falls back to the deterministic recap.
pub async fn request_outgoing_handoff(
    registry: &HarnessRegistry,
    input: OutgoingHandoffInput,
) -> String {
    request_outgoing_handoff_with_timeout(registry, input, HANDOFF_TIMEOUT).await
}

/// [`request_outgoing_handoff`] with a different timeout, for tests.
pub async fn request_outgoing_handoff_with_timeout(
    registry: &HarnessRegistry,
    input: OutgoingHandoffInput,
    timeout: Duration,
) -> String {
    let brief = Arc::new(Mutex::new(String::new()));
    let sink = {
        let brief = brief.clone();
        let registry = registry.clone();
        let harness = input.harness;
        let session_id = input.session_id.clone();
        event_sink(move |event| match event {
            HarnessEvent::MessageDelta { text } => {
                let mut brief = brief.lock();
                *brief = join_stream_text(&brief, &text);
            }
            HarnessEvent::ApprovalRequested { request_id, .. } => {
                registry.respond_harness_approval(
                    harness,
                    &session_id,
                    request_id,
                    ApprovalDecision::Deny,
                );
            }
            HarnessEvent::QuestionAsked { request_id, .. } => {
                registry.respond_harness_question(
                    harness,
                    &session_id,
                    request_id,
                    UserQuestionReply::Skipped,
                );
            }
            _ => {}
        })
    };
    let send = registry.send_harness_turn(
        input.harness,
        SendTurnInput {
            session: HarnessSessionInput {
                session_id: input.session_id.clone(),
                cwd: input.cwd.clone(),
                model: input.model.clone(),
                model_settings: input.model_settings.clone(),
                provider_account_id: input.provider_account_id.clone(),
                runtime_mode: RuntimeMode::Supervised,
                intent: None,
                controls_agents: None,
                app_access: None,
            },
            text: build_outgoing_handoff_prompt(&input.user_request),
            attachments: None,
        },
        sink,
        None,
    );
    let mut send = send.fuse();
    let mut timer = smol::Timer::after(timeout).fuse();
    loop {
        futures::select_biased! {
            // Errors fall back to the deterministic packet.
            _ = send => break,
            _ = timer => {
                let _ = registry.cancel_harness_turn(input.harness, &input.session_id).await;
            }
        }
    }
    js::trim(&brief.lock()).to_string()
}
