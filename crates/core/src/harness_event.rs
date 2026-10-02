//! Port of src/integrations/harness/core/types.ts: the events every provider
//! adapter emits and the inputs it receives.
//!
//! The JavaScript callbacks on the inputs (`onEvent`, `onAccepted`) are not
//! data, so they are not here. Adapters take them as separate arguments.

use serde::{Deserialize, Serialize};

use crate::attachment::Attachment;
use crate::block::{
    AgentStepKind, ApprovalDecided, InterjectionSeverity, ModelSettings, TaskListItem, ToolPreview,
    TurnIntent, TurnMetrics,
};
use crate::harness::RuntimeMode;
use crate::user_question::UserQuestion;

/// One thing a harness reported. Tagged by `type`, as in TypeScript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HarnessEvent {
    #[serde(rename = "session.started")]
    SessionStarted,
    #[serde(rename = "session.ended")]
    SessionEnded {
        /// Exit code. JSON `null` and a missing field both read as `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i64>,
    },
    #[serde(rename = "session.error")]
    SessionError { message: String },
    #[serde(rename = "session.providerBound", rename_all = "camelCase")]
    SessionProviderBound { provider_session_id: String },
    #[serde(rename = "turn.started", rename_all = "camelCase")]
    TurnStarted { provider_turn_id: String },
    #[serde(rename = "session.configChanged", rename_all = "camelCase")]
    SessionConfigChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_settings: Option<ModelSettings>,
    },
    #[serde(rename = "status")]
    Status { text: String },
    /// The provider refused the turn until its usage window resets (epoch ms).
    #[serde(rename = "usage.limited", rename_all = "camelCase")]
    UsageLimited {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resets_at: Option<i64>,
    },
    /// The agent yielded but the turn is not over: work it started is still
    /// running and will wake it again. Empty once it is back at work.
    #[serde(rename = "background.updated")]
    BackgroundUpdated { tasks: Vec<String> },
    #[serde(rename = "interjection", rename_all = "camelCase")]
    Interjection {
        text: String,
        custom_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<InterjectionSeverity>,
    },
    #[serde(rename = "message.delta")]
    MessageDelta { text: String },
    #[serde(rename = "message.completed")]
    MessageCompleted,
    /// `image.generated` has two shapes: inline base64 data, or a file on disk.
    #[serde(rename = "image.generated")]
    ImageGenerated(GeneratedImage),
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { text: String },
    #[serde(rename = "reasoning.completed")]
    ReasoningCompleted,
    #[serde(rename = "tool.started", rename_all = "camelCase")]
    ToolStarted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_model: Option<String>,
        call_id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        /// Work the agent left running when it yielded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        background: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<ToolPreview>,
        /// Every path affected when one structured edit changes several files.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        paths: Option<Vec<String>>,
    },
    #[serde(rename = "tool.updated", rename_all = "camelCase")]
    ToolUpdated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_model: Option<String>,
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<ToolPreview>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        paths: Option<Vec<String>>,
    },
    /// Something a subagent did, mirrored onto its parent Agent tool call.
    #[serde(rename = "agent.step", rename_all = "camelCase")]
    AgentStep {
        /// Tool call id of the parent Agent or Task call.
        call_id: String,
        /// Provider step identity. Repeats merge onto the same row.
        step_id: String,
        kind: AgentStepKind,
        text: String,
        /// Tool kind for a tool step, so it gets the right icon.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<ToolPreview>,
        /// The subagent's own name, when the provider only reveals it here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
    },
    #[serde(rename = "approval.requested", rename_all = "camelCase")]
    ApprovalRequested {
        request_id: i64,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<ToolPreview>,
    },
    #[serde(rename = "approval.resolved", rename_all = "camelCase")]
    ApprovalResolved {
        request_id: i64,
        /// `Cancelled` means a PermissionRequest hook decided before the user could.
        decision: ApprovalDecided,
    },
    #[serde(rename = "question.asked", rename_all = "camelCase")]
    QuestionAsked {
        request_id: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        questions: Vec<UserQuestion>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_resolve_at: Option<i64>,
    },
    #[serde(rename = "question.updated", rename_all = "camelCase")]
    QuestionUpdated {
        request_id: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_resolve_at: Option<i64>,
    },
    #[serde(rename = "question.resolved", rename_all = "camelCase")]
    QuestionResolved {
        request_id: i64,
        decision: QuestionDecision,
    },
    #[serde(rename = "tasks.updated", rename_all = "camelCase")]
    TasksUpdated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        explanation: Option<String>,
        /// Merge changed items into the existing list instead of replacing it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        merge: Option<bool>,
        /// This snapshot owns its labels, so a changed item text is a rename.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authoritative: Option<bool>,
        /// Provider conversation that owns these items.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_session_id: Option<String>,
        items: Vec<TaskListItem>,
    },
    #[serde(rename = "plan")]
    Plan {
        text: String,
        /// Merge identity for deltas and the authoritative completed item.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        /// Append a stream delta instead of replacing the current snapshot.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append: Option<bool>,
        /// `Some(false)` marks the plan ready for review.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streaming: Option<bool>,
    },
    /// Context-window level after the harness's latest request.
    #[serde(rename = "context")]
    Context {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        used: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<i64>,
    },
    /// Provider token accounting for the active user turn.
    #[serde(rename = "turn.metrics")]
    TurnMetrics(TurnMetrics),
}

/// The two shapes of `image.generated`.
///
/// Serde tries the variants in order, so the file shape (which needs `path`,
/// `mimeType`, and `size`) comes before the inline shape (which needs `data`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GeneratedImage {
    #[serde(rename_all = "camelCase")]
    File {
        item_id: String,
        path: String,
        name: String,
        mime_type: String,
        size: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Inline {
        item_id: String,
        data: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
}

impl GeneratedImage {
    pub fn item_id(&self) -> &str {
        match self {
            GeneratedImage::File { item_id, .. } | GeneratedImage::Inline { item_id, .. } => {
                item_id
            }
        }
    }
}

/// How a clarifying question ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuestionDecision {
    #[serde(rename = "answered")]
    Answered,
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// `ApprovalDecision`: what the user answers to an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApprovalDecision {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
}

/// `HarnessSessionInput`, without `onEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSessionInput {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_account_id: Option<String>,
    pub runtime_mode: RuntimeMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<TurnIntent>,
    /// This session drives MonoCode's control CLI, which reaches the app over
    /// loopback. Sandboxes deny network by default, so a lead that cannot open
    /// that socket cannot supervise its agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controls_agents: Option<bool>,
    /// Grants this normal turn access to MonoCode's scoped app CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_access: Option<bool>,
}

/// `CompactContextInput`.
pub type CompactContextInput = HarnessSessionInput;

/// `SendTurnInput`, without `onAccepted`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTurnInput {
    #[serde(flatten)]
    pub session: HarnessSessionInput,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
}

/// `SteerTurnInput`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteerTurnInput {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
}

/// `RewindLastTurnInput`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindLastTurnInput {
    #[serde(flatten)]
    pub session: HarnessSessionInput,
    /// Provider turn boundary for the visible user message, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_turn_id: Option<String>,
    /// When set, Cursor may resend through `session/edit_prompt` in one call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
}

/// `RewindLastTurnResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindLastTurnResult {
    /// The harness already ran the replacement turn.
    pub submitted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn round_trip(value: Value) -> HarnessEvent {
        let event: HarnessEvent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), value);
        event
    }

    #[test]
    fn reads_both_image_shapes() {
        let file = round_trip(json!({
            "type": "image.generated", "itemId": "i1", "path": "/tmp/a.png",
            "name": "a.png", "mimeType": "image/png", "size": 12
        }));
        assert!(matches!(
            file,
            HarnessEvent::ImageGenerated(GeneratedImage::File { .. })
        ));
        let inline = round_trip(json!({
            "type": "image.generated", "itemId": "i2", "data": "AAAA", "name": "b.png", "alt": "chart"
        }));
        match inline {
            HarnessEvent::ImageGenerated(image @ GeneratedImage::Inline { .. }) => {
                assert_eq!(image.item_id(), "i2")
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn flattens_interjection_and_metrics_fields() {
        round_trip(
            json!({ "type": "interjection", "text": "note", "customType": "advisor", "severity": "concern" }),
        );
        let metrics = round_trip(
            json!({ "type": "turn.metrics", "inputTokens": 5, "cacheHitPercent": 40.5 }),
        );
        match metrics {
            HarnessEvent::TurnMetrics(m) => assert_eq!(m.input_tokens, Some(5)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn round_trips_every_event_kind() {
        for value in [
            json!({ "type": "session.started" }),
            json!({ "type": "session.ended", "code": 1 }),
            json!({ "type": "session.ended" }),
            json!({ "type": "session.error", "message": "boom" }),
            json!({ "type": "session.providerBound", "providerSessionId": "p" }),
            json!({ "type": "turn.started", "providerTurnId": "t" }),
            json!({ "type": "session.configChanged", "model": "m", "modelSettings": { "effort": "high" } }),
            json!({ "type": "status", "text": "Working" }),
            json!({ "type": "usage.limited", "resetsAt": 1000 }),
            json!({ "type": "background.updated", "tasks": ["build"] }),
            json!({ "type": "message.delta", "text": "hi" }),
            json!({ "type": "message.completed" }),
            json!({ "type": "reasoning.delta", "text": "hm" }),
            json!({ "type": "reasoning.completed" }),
            json!({ "type": "tool.started", "callId": "c", "title": "Read", "kind": "read",
                    "preview": { "kind": "read", "path": "/a" }, "paths": ["/a"] }),
            json!({ "type": "tool.updated", "callId": "c", "status": "completed", "detail": "ok" }),
            json!({ "type": "agent.step", "callId": "c", "stepId": "s", "kind": "tool", "text": "Grep", "toolKind": "search" }),
            json!({ "type": "approval.requested", "requestId": 7, "title": "Run ls?" }),
            json!({ "type": "approval.resolved", "requestId": 7, "decision": "cancelled" }),
            json!({ "type": "question.asked", "requestId": 8, "questions": [
                { "id": "q", "prompt": "Which?", "multiSelect": false, "allowCustom": true, "options": [] }
            ], "autoResolveAt": 5 }),
            json!({ "type": "question.updated", "requestId": 8 }),
            json!({ "type": "question.resolved", "requestId": 8, "decision": "skipped" }),
            json!({ "type": "tasks.updated", "merge": true, "items": [{ "text": "a", "status": "pending" }] }),
            json!({ "type": "plan", "text": "# P", "append": true, "streaming": false }),
            json!({ "type": "context", "used": 10, "window": 100 }),
        ] {
            round_trip(value);
        }
    }

    #[test]
    fn reads_a_null_exit_code() {
        let event: HarnessEvent =
            serde_json::from_value(json!({ "type": "session.ended", "code": null })).unwrap();
        assert_eq!(event, HarnessEvent::SessionEnded { code: None });
    }

    #[test]
    fn flattens_session_input_into_turn_input() {
        let value = json!({
            "sessionId": "s", "cwd": "/r", "model": "m", "runtimeMode": "auto",
            "text": "go", "attachments": []
        });
        let input: SendTurnInput = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(input.session.runtime_mode, RuntimeMode::Auto);
        assert_eq!(serde_json::to_value(&input).unwrap(), value);
    }
}
