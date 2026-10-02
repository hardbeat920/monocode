//! Port of src/integrations/harness/providers/codex/codexProtocol.ts.
//!
//! Pure mapping between the Codex app-server protocol and MonoCode's harness
//! events: thread and turn parameters, notifications, tool items, diff
//! previews, subagent steps, and server approval requests.

use serde::Serialize;
use serde_json::{Value, json};

use monocode_core::attachment::{
    Attachment, attachment_path, attachment_path_text, is_vision_image, normalize_image_mime,
    prompt_text,
};
use monocode_core::block::{
    AgentStepKind, TaskListItem, TaskListItemStatus, ToolPreview, ToolPreviewKind, TurnIntent,
    TurnMetrics,
};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{ApprovalDecision, GeneratedImage, HarnessEvent};
use monocode_core::js;
use monocode_core::paths::display_path;
use monocode_core::reducer::{
    ToolTitleInput, compose_tool_title, extract_tool_preview, format_agent_type,
    format_shell_intent, infer_shell_intent,
};
use monocode_core::task_list::normalize_task_list_status_str;

pub use super::json::{Record, as_record, string_field};

/// Codex `approvalPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ApprovalPolicy {
    #[serde(rename = "untrusted")]
    Untrusted,
    #[serde(rename = "on-request")]
    OnRequest,
    #[serde(rename = "never")]
    Never,
}

/// Codex `sandbox` for thread/start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SandboxMode {
    #[serde(rename = "read-only")]
    ReadOnly,
    #[serde(rename = "workspace-write")]
    WorkspaceWrite,
    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
}

/// Codex `approvalsReviewer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ApprovalsReviewer {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "auto_review")]
    AutoReview,
}

/// Codex `sandboxPolicy` for thread/start and turn/start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum SandboxPolicy {
    #[serde(rename = "readOnly", rename_all = "camelCase")]
    ReadOnly {
        #[serde(skip_serializing_if = "Option::is_none")]
        network_access: Option<bool>,
    },
    #[serde(rename = "workspaceWrite", rename_all = "camelCase")]
    WorkspaceWrite {
        #[serde(skip_serializing_if = "Option::is_none")]
        network_access: Option<bool>,
    },
    #[serde(rename = "dangerFullAccess")]
    DangerFullAccess,
}

/// `CodexThreadConfig`: approval and sandbox settings for thread/start and
/// turn/start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThreadConfig {
    pub approval_policy: ApprovalPolicy,
    pub sandbox: SandboxMode,
    pub approvals_reviewer: ApprovalsReviewer,
    pub sandbox_policy: SandboxPolicy,
}

/// `withNetwork`. `readOnly` and `workspaceWrite` both default to
/// `networkAccess: false`, which blocks loopback too. An orchestration lead or
/// a thread with app access needs the local CLI socket, so its turns enable
/// network access.
fn with_network(config: CodexThreadConfig, controls_agents: bool) -> CodexThreadConfig {
    if !controls_agents || config.sandbox_policy == SandboxPolicy::DangerFullAccess {
        return config;
    }
    let sandbox_policy = match config.sandbox_policy {
        SandboxPolicy::ReadOnly { .. } => SandboxPolicy::ReadOnly {
            network_access: Some(true),
        },
        SandboxPolicy::WorkspaceWrite { .. } => SandboxPolicy::WorkspaceWrite {
            network_access: Some(true),
        },
        other => other,
    };
    CodexThreadConfig {
        sandbox_policy,
        ..config
    }
}

/// `runtimeModeToCodexConfig`.
pub fn runtime_mode_to_codex_config(mode: RuntimeMode, controls_agents: bool) -> CodexThreadConfig {
    with_network(base_codex_config(mode), controls_agents)
}

fn base_codex_config(mode: RuntimeMode) -> CodexThreadConfig {
    match mode {
        RuntimeMode::Supervised => CodexThreadConfig {
            approval_policy: ApprovalPolicy::Untrusted,
            sandbox: SandboxMode::ReadOnly,
            approvals_reviewer: ApprovalsReviewer::User,
            sandbox_policy: SandboxPolicy::ReadOnly {
                network_access: None,
            },
        },
        RuntimeMode::AutoAcceptEdits => CodexThreadConfig {
            approval_policy: ApprovalPolicy::OnRequest,
            sandbox: SandboxMode::WorkspaceWrite,
            approvals_reviewer: ApprovalsReviewer::User,
            sandbox_policy: SandboxPolicy::WorkspaceWrite {
                network_access: None,
            },
        },
        RuntimeMode::Auto => CodexThreadConfig {
            approval_policy: ApprovalPolicy::OnRequest,
            sandbox: SandboxMode::WorkspaceWrite,
            approvals_reviewer: ApprovalsReviewer::AutoReview,
            sandbox_policy: SandboxPolicy::WorkspaceWrite {
                network_access: None,
            },
        },
        RuntimeMode::FullAccess => CodexThreadConfig {
            // Explicit escalations still need an approval round trip. "never"
            // rejects them before the client's full-access handler can allow them.
            approval_policy: ApprovalPolicy::OnRequest,
            sandbox: SandboxMode::DangerFullAccess,
            approvals_reviewer: ApprovalsReviewer::User,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
        },
    }
}

/// `streamTextDelta`, owned.
fn delta_text(value: Option<&Value>) -> String {
    monocode_core::reducer::stream_text_delta(value).to_string()
}

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Input for `build_thread_start_params`.
#[derive(Debug, Clone, Default)]
pub struct ThreadStartInput<'a> {
    pub cwd: &'a str,
    pub runtime_mode: RuntimeMode,
    pub controls_agents: bool,
    pub model: Option<&'a str>,
    pub service_tier: Option<&'a str>,
}

/// `buildThreadStartParams`.
pub fn build_thread_start_params(input: &ThreadStartInput<'_>) -> Record {
    let config = runtime_mode_to_codex_config(input.runtime_mode, input.controls_agents);
    let mut params = Record::new();
    params.insert("cwd".into(), json!(input.cwd));
    params.insert("approvalPolicy".into(), to_json(&config.approval_policy));
    params.insert("sandbox".into(), to_json(&config.sandbox));
    params.insert("sandboxPolicy".into(), to_json(&config.sandbox_policy));
    params.insert(
        "approvalsReviewer".into(),
        to_json(&config.approvals_reviewer),
    );
    if let Some(model) = input.model.filter(|model| !model.is_empty()) {
        params.insert("model".into(), json!(model));
    }
    if let Some(tier) = input
        .service_tier
        .filter(|tier| !tier.is_empty() && *tier != "default")
    {
        params.insert("serviceTier".into(), json!(tier));
    }
    params
}

/// Input for `build_turn_steer_params`.
#[derive(Debug, Clone, Default)]
pub struct TurnSteerInput<'a> {
    pub thread_id: &'a str,
    pub expected_turn_id: &'a str,
    pub prompt: Option<&'a str>,
    pub attachments: &'a [Attachment],
}

/// `buildTurnSteerParams`. Fails when an attachment has no deliverable source.
pub fn build_turn_steer_params(input: &TurnSteerInput<'_>) -> Result<Record, String> {
    let mut params = Record::new();
    params.insert("threadId".into(), json!(input.thread_id));
    params.insert("expectedTurnId".into(), json!(input.expected_turn_id));
    params.insert(
        "input".into(),
        Value::Array(codex_input(input.prompt, input.attachments)?),
    );
    Ok(params)
}

/// Input for `build_turn_start_params`.
#[derive(Debug, Clone, Default)]
pub struct TurnStartInput<'a> {
    pub thread_id: &'a str,
    pub runtime_mode: RuntimeMode,
    pub controls_agents: bool,
    pub prompt: Option<&'a str>,
    pub attachments: &'a [Attachment],
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub service_tier: Option<&'a str>,
    pub intent: Option<TurnIntent>,
}

/// `buildTurnStartParams`. Fails when an attachment has no deliverable source.
pub fn build_turn_start_params(input: &TurnStartInput<'_>) -> Result<Record, String> {
    let runtime_config = runtime_mode_to_codex_config(input.runtime_mode, input.controls_agents);
    let planning = input.intent == Some(TurnIntent::Plan);
    let config = if planning {
        with_network(
            CodexThreadConfig {
                approval_policy: ApprovalPolicy::Never,
                sandbox: SandboxMode::ReadOnly,
                approvals_reviewer: runtime_config.approvals_reviewer,
                sandbox_policy: SandboxPolicy::ReadOnly {
                    network_access: None,
                },
            },
            input.controls_agents,
        )
    } else {
        runtime_config
    };
    let mut params = Record::new();
    params.insert("threadId".into(), json!(input.thread_id));
    params.insert(
        "input".into(),
        Value::Array(codex_input(input.prompt, input.attachments)?),
    );
    params.insert("approvalPolicy".into(), to_json(&config.approval_policy));
    params.insert(
        "approvalsReviewer".into(),
        to_json(&config.approvals_reviewer),
    );
    params.insert("sandboxPolicy".into(), to_json(&config.sandbox_policy));
    params.insert(
        "collaborationMode".into(),
        json!({
            "mode": if planning { "plan" } else { "default" },
            "settings": {
                "model": input.model,
                "reasoning_effort": input.effort,
                "developer_instructions": null,
            },
        }),
    );
    if let Some(model) = input.model.filter(|model| !model.is_empty()) {
        params.insert("model".into(), json!(model));
    }
    if let Some(effort) = input.effort.filter(|effort| !effort.is_empty()) {
        params.insert("effort".into(), json!(effort));
    }
    if let Some(tier) = input
        .service_tier
        .filter(|tier| !tier.is_empty() && *tier != "default")
    {
        params.insert("serviceTier".into(), json!(tier));
    }
    Ok(params)
}

/// `codexInput`. The app-server accepts image inputs, but documents need a
/// path in text.
fn codex_input(prompt: Option<&str>, attachments: &[Attachment]) -> Result<Vec<Value>, String> {
    let mut input = Vec::new();
    let body = prompt_text(prompt.unwrap_or(""), attachments);
    if !body.is_empty() {
        input.push(json!({ "type": "text", "text": body }));
    }
    for file in attachments {
        if is_vision_image(&file.mime_type) {
            match file.data.as_deref().filter(|data| !data.is_empty()) {
                Some(data) => input.push(json!({
                    "type": "image",
                    "url": format!("data:{};base64,{}", normalize_image_mime(&file.mime_type), data),
                })),
                None => input.push(json!({ "type": "localImage", "path": attachment_path(file)? })),
            }
        } else {
            input.push(json!({ "type": "text", "text": attachment_path_text(file)? }));
        }
    }
    Ok(input)
}

/// `isRecoverableThreadResumeError`: the thread is gone, so start a new one.
pub fn is_recoverable_thread_resume_error(message: &str) -> bool {
    let message = message.to_lowercase();
    if !message.contains("thread") {
        return false;
    }
    [
        "not found",
        "unknown thread",
        "no such thread",
        "does not exist",
        "missing thread",
        "thread id",
    ]
    .iter()
    .any(|snippet| message.contains(snippet))
}

/// `numberField`: a finite number, or 0.
fn number_field(rec: Option<&Record>, key: &str) -> f64 {
    rec.and_then(|rec| rec.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// `CodexApprovalKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodexApprovalKind {
    Command,
    FileChange,
    Permissions,
}

/// `toCodexApprovalDecision`: the wire decision for a UI decision.
pub fn to_codex_approval_decision(
    decision: ApprovalDecision,
    kind: CodexApprovalKind,
) -> &'static str {
    // Prefer one-shot accept. Session-scoped grants can be added later.
    let _ = kind;
    match decision {
        ApprovalDecision::Deny => "decline",
        ApprovalDecision::Allow => "accept",
    }
}

/// How a Codex turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurnStatus {
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

/// `turnCompleted` on a mapped notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCompleted {
    pub status: TurnStatus,
    pub error: Option<String>,
}

/// `MappedCodexNotification`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MappedCodexNotification {
    pub events: Vec<HarnessEvent>,
    /// Provider diagnostics for debug logs, kept out of the transcript.
    pub diagnostic: Option<String>,
    /// Set when the active turn finished.
    pub turn_completed: Option<TurnCompleted>,
    /// `None` leaves the active turn alone. `Some(None)` clears it.
    pub active_turn_id: Option<Option<String>>,
    /// Codex refused the turn because the account's usage limit is spent.
    pub usage_limited: bool,
    /// A sparse `account/rateLimits/updated` snapshot.
    pub rate_limits: Option<Record>,
}

impl MappedCodexNotification {
    fn events(events: Vec<HarnessEvent>) -> Self {
        Self {
            events,
            ..Self::default()
        }
    }

    fn none() -> Self {
        Self::default()
    }
}

/// `mapCodexNotification`: translate a Codex app-server notification into
/// harness events. Unknown methods map to no events.
pub fn map_codex_notification(method: &str, params: &Value) -> MappedCodexNotification {
    let Some(rec) = as_record(Some(params)) else {
        return MappedCodexNotification::none();
    };

    match method {
        "item/agentMessage/delta" => {
            let delta = delta_text(rec.get("delta"));
            if delta.is_empty() {
                return MappedCodexNotification::none();
            }
            MappedCodexNotification::events(vec![HarnessEvent::MessageDelta { text: delta }])
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            let delta = delta_text(rec.get("delta"));
            if delta.is_empty() {
                return MappedCodexNotification::none();
            }
            MappedCodexNotification::events(vec![HarnessEvent::ReasoningDelta { text: delta }])
        }
        "item/plan/delta" => {
            let delta = delta_text(rec.get("delta"));
            if delta.is_empty() {
                return MappedCodexNotification::none();
            }
            MappedCodexNotification::events(vec![HarnessEvent::Plan {
                text: delta,
                key: string_field(Some(rec), "itemId").map(str::to_string),
                append: Some(true),
                streaming: Some(true),
            }])
        }
        "turn/plan/updated" => {
            let Some(Value::Array(plan)) = rec.get("plan") else {
                return MappedCodexNotification::none();
            };
            let items = plan
                .iter()
                .filter_map(|step| {
                    let row = as_record(Some(step));
                    let body = string_field(row, "step").unwrap_or("");
                    if body.is_empty() {
                        return None;
                    }
                    Some(TaskListItem {
                        id: None,
                        text: body.to_string(),
                        status: string_field(row, "status")
                            .map(normalize_task_list_status_str)
                            .unwrap_or(TaskListItemStatus::Pending),
                        extra: Default::default(),
                    })
                })
                .collect();
            MappedCodexNotification::events(vec![HarnessEvent::TasksUpdated {
                key: string_field(Some(rec), "turnId").map(str::to_string),
                explanation: string_field(Some(rec), "explanation").map(str::to_string),
                merge: None,
                authoritative: None,
                provider_session_id: None,
                items,
            }])
        }
        "item/started" | "item/completed" => map_item_lifecycle(method, rec),
        "item/commandExecution/outputDelta" => {
            let item_id = string_field(Some(rec), "itemId").unwrap_or("");
            let delta = delta_text(rec.get("delta"));
            if item_id.is_empty() || delta.is_empty() {
                return MappedCodexNotification::none();
            }
            MappedCodexNotification::events(vec![tool_updated(
                item_id,
                None,
                Some("execute"),
                Some("in_progress".into()),
                Some(delta),
                None,
                None,
            )])
        }
        "item/fileChange/patchUpdated" => map_file_change_patch(rec),
        "thread/tokenUsage/updated" => map_token_usage(rec),
        "turn/started" => {
            let turn = as_record(rec.get("turn"));
            MappedCodexNotification {
                active_turn_id: string_field(turn, "id").map(|id| Some(id.to_string())),
                ..MappedCodexNotification::none()
            }
        }
        "turn/completed" | "turn/aborted" => map_turn_terminal(method, rec),
        "error" => {
            let error_obj = as_record(rec.get("error"));
            let message = string_field(error_obj, "message")
                .or_else(|| string_field(Some(rec), "message"))
                .unwrap_or("Codex error")
                .to_string();
            if rec.get("willRetry") == Some(&Value::Bool(true)) {
                // Codex owns retrying the request. A status event would persist
                // a row for every attempt and interrupt any streaming block.
                return MappedCodexNotification {
                    diagnostic: Some(message),
                    ..MappedCodexNotification::none()
                };
            }
            MappedCodexNotification {
                events: vec![HarnessEvent::SessionError { message }],
                usage_limited: is_usage_limit_error(error_obj),
                ..MappedCodexNotification::none()
            }
        }
        "account/rateLimits/updated" => MappedCodexNotification {
            rate_limits: as_record(rec.get("rateLimits")).cloned(),
            ..MappedCodexNotification::none()
        },
        "configWarning" | "warning" => {
            let Some(message) = string_field(Some(rec), "summary")
                .or_else(|| string_field(Some(rec), "message"))
                .or_else(|| string_field(Some(rec), "details"))
            else {
                return MappedCodexNotification::none();
            };
            // Runtime warnings have no structured code. Match only Codex's known
            // transport fallback notice. Configuration and other warnings stay
            // visible.
            if method == "warning" && is_transport_fallback(message) {
                return MappedCodexNotification {
                    diagnostic: Some(message.to_string()),
                    ..MappedCodexNotification::none()
                };
            }
            MappedCodexNotification::events(vec![HarnessEvent::Status {
                text: message.to_string(),
            }])
        }
        _ => MappedCodexNotification::none(),
    }
}

/// `/^Falling back from WebSockets to HTTPS transport(?:[.:]|$)/` on the
/// message with leading whitespace removed.
fn is_transport_fallback(message: &str) -> bool {
    const PREFIX: &str = "Falling back from WebSockets to HTTPS transport";
    let trimmed = message.trim_start_matches(js::is_space);
    match trimmed.strip_prefix(PREFIX) {
        Some(rest) => rest.is_empty() || rest.starts_with('.') || rest.starts_with(':'),
        None => false,
    }
}

/// Codex thread items MonoCode already renders elsewhere, or internal metadata.
const SILENT_ITEM_TYPES: [&str; 3] = ["userMessage", "contextCompaction", "enteredReviewMode"];

/// `mapTokenUsage`. Codex reports both `last` (the most recent request) and
/// `total` (cumulative thread spend). Only `last` describes the context
/// window. `total` keeps climbing across compactions and would run past 100%.
fn map_token_usage(rec: &Record) -> MappedCodexNotification {
    let usage = as_record(rec.get("tokenUsage"));
    let Some(last) = as_record(usage.and_then(|usage| usage.get("last"))) else {
        return MappedCodexNotification::none();
    };
    let used = number_field(Some(last), "totalTokens");
    let window = number_field(usage, "modelContextWindow");
    let input_tokens = number_field(Some(last), "inputTokens");
    let cache_read_tokens = number_field(Some(last), "cachedInputTokens");
    let cache_write_tokens = number_field(Some(last), "cacheWriteInputTokens");
    let output_tokens = number_field(Some(last), "outputTokens");
    let cache_reported =
        last.contains_key("cachedInputTokens") || last.contains_key("cacheWriteInputTokens");
    let nonzero = |value: f64| (value != 0.0).then_some(value as i64);
    let metrics = TurnMetrics {
        input_tokens: nonzero(input_tokens),
        output_tokens: nonzero(output_tokens),
        cache_read_tokens: nonzero(cache_read_tokens),
        cache_write_tokens: nonzero(cache_write_tokens),
        cache_hit_percent: (cache_reported && input_tokens > 0.0)
            .then(|| (cache_read_tokens / (input_tokens + cache_write_tokens)) * 100.0),
        extra: Default::default(),
    };
    let has_metrics = metrics.input_tokens.is_some()
        || metrics.output_tokens.is_some()
        || metrics.cache_read_tokens.is_some()
        || metrics.cache_write_tokens.is_some()
        || metrics.cache_hit_percent.is_some();
    if used == 0.0 && window == 0.0 && !has_metrics {
        return MappedCodexNotification::none();
    }
    let mut events = Vec::new();
    if used != 0.0 || window != 0.0 {
        events.push(HarnessEvent::Context {
            used: (used > 0.0).then_some(used as i64),
            window: (window > 0.0).then_some(window as i64),
        });
    }
    if has_metrics {
        events.push(HarnessEvent::TurnMetrics(metrics));
    }
    MappedCodexNotification::events(events)
}

fn map_turn_terminal(method: &str, rec: &Record) -> MappedCodexNotification {
    let turn = as_record(rec.get("turn"));
    let status_raw = string_field(turn, "status").unwrap_or(if method == "turn/aborted" {
        "interrupted"
    } else {
        "completed"
    });
    let error_obj = as_record(turn.and_then(|turn| turn.get("error")));
    let error = string_field(error_obj, "message").map(str::to_string);
    let status = match status_raw {
        "failed" => TurnStatus::Failed,
        "cancelled" => TurnStatus::Cancelled,
        "interrupted" => TurnStatus::Interrupted,
        _ => TurnStatus::Completed,
    };
    let mut events = vec![
        HarnessEvent::MessageCompleted,
        HarnessEvent::ReasoningCompleted,
    ];
    if status == TurnStatus::Failed {
        events.push(HarnessEvent::SessionError {
            message: error.clone().unwrap_or_else(|| "Codex turn failed.".into()),
        });
    }
    MappedCodexNotification {
        events,
        turn_completed: Some(TurnCompleted { status, error }),
        active_turn_id: Some(None),
        usage_limited: status == TurnStatus::Failed && is_usage_limit_error(error_obj),
        ..MappedCodexNotification::none()
    }
}

fn is_usage_limit_error(error: Option<&Record>) -> bool {
    error.and_then(|error| error.get("codexErrorInfo")) == Some(&json!("usageLimitExceeded"))
}

fn map_item_lifecycle(method: &str, rec: &Record) -> MappedCodexNotification {
    let Some(item) = as_record(rec.get("item")) else {
        return MappedCodexNotification::none();
    };
    let call_id = string_field(Some(item), "id").unwrap_or("");
    if call_id.is_empty() {
        return MappedCodexNotification::none();
    }
    let item_type = string_field(Some(item), "type").unwrap_or("");
    let completed = method == "item/completed";

    if SILENT_ITEM_TYPES.contains(&item_type) {
        return MappedCodexNotification::none();
    }

    if item_type == "exitedReviewMode" && completed {
        return match string_field(Some(item), "review") {
            Some(review) => MappedCodexNotification::events(vec![
                HarnessEvent::MessageDelta {
                    text: review.to_string(),
                },
                HarnessEvent::MessageCompleted,
            ]),
            None => MappedCodexNotification::none(),
        };
    }

    if item_type == "agentMessage" {
        // Prefer deltas. A completed agent message may carry the full text for
        // non-streaming. A turn can still run after this item (Codex often
        // sends a short message, then tools, then another message), so this is
        // not turn completion.
        if completed {
            let text = delta_text(item.get("text"));
            let mut events = Vec::new();
            if !text.is_empty() {
                events.push(HarnessEvent::MessageDelta { text });
                events.push(HarnessEvent::MessageCompleted);
            }
            return MappedCodexNotification::events(events);
        }
        return MappedCodexNotification::none();
    }

    if item_type == "imageGeneration" {
        if !completed {
            return MappedCodexNotification::none();
        }
        let Some(result) = string_field(Some(item), "result").map(js::trim) else {
            return MappedCodexNotification::none();
        };
        if result.is_empty() {
            return MappedCodexNotification::none();
        }
        let prompt = string_field(Some(item), "revisedPrompt")
            .map(js::trim)
            .filter(|prompt| !prompt.is_empty());
        return MappedCodexNotification::events(vec![HarnessEvent::ImageGenerated(
            GeneratedImage::Inline {
                item_id: call_id.to_string(),
                data: result.to_string(),
                name: "generated-image".into(),
                alt: prompt.map(str::to_string),
            },
        )]);
    }

    if item_type == "reasoning" {
        if completed {
            if let Some(Value::Array(summary)) = item.get("summary") {
                let text = summary
                    .iter()
                    .map(|part| match part {
                        Value::String(text) => text.as_str(),
                        other => string_field(as_record(Some(other)), "text").unwrap_or(""),
                    })
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    return MappedCodexNotification::events(vec![
                        HarnessEvent::ReasoningDelta { text },
                        HarnessEvent::ReasoningCompleted,
                    ]);
                }
            }
            return MappedCodexNotification::events(vec![HarnessEvent::ReasoningCompleted]);
        }
        return MappedCodexNotification::none();
    }

    if item_type == "plan" {
        return match string_field(Some(item), "text") {
            Some(text) => MappedCodexNotification::events(vec![HarnessEvent::Plan {
                text: text.to_string(),
                key: string_field(Some(item), "id").map(str::to_string),
                append: None,
                streaming: Some(false),
            }]),
            None => MappedCodexNotification::none(),
        };
    }

    match map_tool_item(item, item_type, completed) {
        Some(event) => MappedCodexNotification::events(vec![event]),
        None => MappedCodexNotification::none(),
    }
}

/// A `tool.started` event with the usual fields.
fn tool_started(
    call_id: &str,
    title: String,
    kind: &str,
    status: Option<String>,
    preview: Option<ToolPreview>,
    paths: Option<Vec<String>>,
) -> HarnessEvent {
    HarnessEvent::ToolStarted {
        agent_model: None,
        call_id: call_id.to_string(),
        title,
        kind: Some(kind.to_string()),
        status,
        background: None,
        preview,
        paths,
    }
}

/// A `tool.updated` event with the usual fields.
fn tool_updated(
    call_id: &str,
    title: Option<String>,
    kind: Option<&str>,
    status: Option<String>,
    detail: Option<String>,
    preview: Option<ToolPreview>,
    paths: Option<Vec<String>>,
) -> HarnessEvent {
    HarnessEvent::ToolUpdated {
        agent_model: None,
        call_id: call_id.to_string(),
        title,
        kind: kind.map(str::to_string),
        status,
        detail,
        preview,
        paths,
    }
}

/// `mapToolItem`.
pub fn map_tool_item(item: &Record, item_type: &str, completed: bool) -> Option<HarnessEvent> {
    let call_id = string_field(Some(item), "id").unwrap_or("");
    if call_id.is_empty() {
        return None;
    }

    match item_type {
        "commandExecution" => {
            let command = string_field(Some(item), "command").unwrap_or("Shell");
            let status = map_item_status(string_field(Some(item), "status"), completed);
            let output = string_field(Some(item), "aggregatedOutput")
                .or_else(|| string_field(Some(item), "output"));
            let presentation = codex_command_presentation(item, command);
            if !completed {
                return Some(tool_started(
                    call_id,
                    presentation.title,
                    "execute",
                    Some(status),
                    presentation.preview,
                    None,
                ));
            }
            Some(tool_updated(
                call_id,
                Some(presentation.title),
                Some("execute"),
                Some(status),
                output.map(str::to_string),
                presentation.preview,
                None,
            ))
        }
        "fileChange" => Some(map_file_change_item(item, call_id, completed)),
        "webSearch" => {
            let query = string_field(Some(item), "query").unwrap_or("Search");
            let status = map_item_status(string_field(Some(item), "status"), completed);
            let title = compose_tool_title(&ToolTitleInput {
                kind: Some("search"),
                title: Some(query),
                query: Some(query),
                preview_kind: Some(ToolPreviewKind::Search),
                ..Default::default()
            });
            let mut preview = ToolPreview::new(ToolPreviewKind::Search);
            preview.query = Some(query.to_string());
            if !completed {
                return Some(tool_started(
                    call_id,
                    title,
                    "search",
                    Some(status),
                    Some(preview),
                    None,
                ));
            }
            Some(tool_updated(
                call_id,
                Some(title),
                Some("search"),
                Some(status),
                None,
                Some(preview),
                None,
            ))
        }
        "mcpToolCall" => {
            let server = string_field(Some(item), "server").unwrap_or("mcp");
            let tool = string_field(Some(item), "tool").unwrap_or("tool");
            let title = format!("{server}:{tool}");
            let status = map_item_status(string_field(Some(item), "status"), completed);
            let mut fake = Record::new();
            fake.insert("kind".into(), json!("other"));
            fake.insert("title".into(), json!(title));
            if let Some(args) = item.get("arguments") {
                fake.insert("rawInput".into(), args.clone());
            }
            let preview = extract_tool_preview(&fake, &fake);
            if !completed {
                return Some(tool_started(
                    call_id,
                    title,
                    "other",
                    Some(status),
                    preview,
                    None,
                ));
            }
            Some(tool_updated(
                call_id,
                Some(title),
                Some("other"),
                Some(status),
                None,
                preview,
                None,
            ))
        }
        "subAgentActivity" => Some(map_sub_agent_activity(item, call_id, completed)),
        "collabAgentToolCall" => Some(map_collab_agent_tool_call(item, call_id, completed)),
        // Unknown item types are ignored. Codex may add new internal kinds.
        _ => None,
    }
}

struct CommandPresentation {
    title: String,
    preview: Option<ToolPreview>,
}

/// `codexCommandPresentation`: prefer Codex's own best-effort command
/// parsing, then the shared shell intent fallback.
fn codex_command_presentation(item: &Record, command: &str) -> CommandPresentation {
    let cwd = string_field(Some(item), "cwd");
    let actions: Vec<&Record> = match item.get("commandActions") {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| as_record(Some(value)))
            .collect(),
        _ => Vec::new(),
    };

    // The last meaningful stage usually describes the pipeline's visible goal
    // (`cat file | grep term` is a Find). Unknown filters are ignored.
    for action in actions.iter().rev() {
        let action = Some(*action);
        let kind = string_field(action, "type");
        let path = string_field(action, "path");
        let shown_path = path.map(|path| display_path(path, cwd));
        match kind {
            Some("search") => {
                let Some(query) = string_field(action, "query") else {
                    continue;
                };
                return CommandPresentation {
                    title: format!("Find {query}"),
                    preview: Some(shell_command_preview(command, path, Some(query), None)),
                };
            }
            Some("read") if path.is_some() => {
                return CommandPresentation {
                    title: format!("Read {}", shown_path.unwrap_or_default()),
                    preview: Some(shell_command_preview(command, path, None, None)),
                };
            }
            Some("listFiles") => {
                return CommandPresentation {
                    title: match shown_path {
                        Some(shown) => format!("List {shown}"),
                        None => "List".into(),
                    },
                    preview: Some(shell_command_preview(command, path, None, None)),
                };
            }
            _ => {}
        }
    }

    let Some(inferred) = infer_shell_intent(command) else {
        return CommandPresentation {
            title: command.to_string(),
            preview: None,
        };
    };
    let path = inferred.path.as_deref().filter(|path| !path.is_empty());
    let shown_path = path.map(|path| display_path(path, cwd));
    CommandPresentation {
        title: format_shell_intent(&inferred, shown_path.as_deref(), inferred.query.as_deref())
            .unwrap_or_else(|| command.to_string()),
        preview: Some(shell_command_preview(
            command,
            path,
            inferred.query.as_deref(),
            inferred.start_line,
        )),
    }
}

/// `shellCommandPreview`.
fn shell_command_preview(
    command: &str,
    path: Option<&str>,
    query: Option<&str>,
    start_line: Option<i64>,
) -> ToolPreview {
    let mut preview = ToolPreview::new(ToolPreviewKind::Shell);
    preview.title = Some(command.to_string());
    if let Some(path) = path.filter(|path| !path.is_empty()) {
        preview.path = Some(path.to_string());
        preview.file_name = Some(last_segment(path.trim_end_matches(['/', '\\'])).to_string());
    }
    if let Some(query) = query.filter(|query| !query.is_empty()) {
        preview.query = Some(query.to_string());
    }
    if let Some(line) = start_line.filter(|line| *line != 0) {
        preview.start_line = Some(line);
    }
    preview
}

/// `path.split(/[/\\]/).pop()`.
fn last_segment(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The last non-empty segment, `path.split(/[/\\]/).filter(Boolean).pop()`.
fn leaf_segment(path: &str) -> Option<&str> {
    path.split(['/', '\\']).rfind(|part| !part.is_empty())
}

fn map_sub_agent_activity(item: &Record, call_id: &str, completed: bool) -> HarnessEvent {
    let kind = string_field(Some(item), "kind")
        .unwrap_or("")
        .to_lowercase();
    let path =
        string_field(Some(item), "agentPath").or_else(|| string_field(Some(item), "agent_path"));
    let title = match path.and_then(leaf_segment) {
        Some(leaf) => format!("{} subagent", format_agent_type(leaf)),
        None => "Subagent".into(),
    };
    if kind == "interrupted" {
        return tool_updated(
            call_id,
            Some(title),
            Some("agent"),
            Some("failed".into()),
            Some("Subagent interrupted.".into()),
            None,
            None,
        );
    }
    if kind == "interacted" {
        if completed {
            return tool_updated(
                call_id,
                Some(title),
                Some("agent"),
                Some("in_progress".into()),
                None,
                None,
                None,
            );
        }
        return tool_started(
            call_id,
            title,
            "agent",
            Some("in_progress".into()),
            None,
            None,
        );
    }
    // `started` items are completion-only in app-server v2: the spawn
    // finished, but the child agent is still running.
    tool_started(
        call_id,
        title,
        "agent",
        Some("in_progress".into()),
        None,
        None,
    )
}

/// `receiverThreadIds` (or `receiver_thread_ids`) as non-empty strings.
fn receiver_ids(item: &Record) -> Vec<&str> {
    match item
        .get("receiverThreadIds")
        .filter(|value| !value.is_null())
        .or_else(|| item.get("receiver_thread_ids"))
    {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| !value.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// `agentsStates` (or `agents_states`) as a record.
fn agents_states(item: &Record) -> Option<&Record> {
    as_record(item.get("agentsStates")).or_else(|| as_record(item.get("agents_states")))
}

/// `mapCollabAgentToolCall`: the app-server v2 representation for spawn,
/// send, wait, and close calls.
fn map_collab_agent_tool_call(item: &Record, call_id: &str, completed: bool) -> HarnessEvent {
    let tool = string_field(Some(item), "tool").unwrap_or("");
    let receivers = receiver_ids(item);
    let fallback_title = match tool {
        "spawnAgent" => "Spawn subagent".to_string(),
        "sendInput" => "Message subagent".to_string(),
        "resumeAgent" => "Resume subagent".to_string(),
        "wait" if receivers.len() > 1 => format!("Wait for {} subagents", receivers.len()),
        "wait" => "Wait for subagent".to_string(),
        "closeAgent" => "Close subagent".to_string(),
        _ => "Subagent".to_string(),
    };
    // A spawn's brief is the only name the agent gets. Its first line is what
    // the model wrote the run for, so "Spawn subagent" is a last resort.
    let title = match agent_brief(item) {
        Some(brief) if tool == "spawnAgent" => brief,
        _ => fallback_title,
    };
    let detail = collab_agent_failure_detail(item);
    let failed = string_field(Some(item), "status") == Some("failed") || detail.is_some();
    // Spawning or resuming an agent is that agent's row. Waiting on one,
    // messaging it, and closing it are bookkeeping against a row that already
    // exists. Given their own agent rows they read as extra subagents that
    // never do anything.
    let spawns = tool == "spawnAgent" || tool == "resumeAgent";
    // Completing a spawn means the call returned, not that the agent it
    // started has finished. The child runs on its own thread for as long as
    // it needs. Only its reported state settles the row, so a running agent
    // is never captioned as done.
    let settled = completed && (!spawns || failed);
    let status = if settled {
        if failed { "failed" } else { "completed" }
    } else {
        "in_progress"
    };
    let kind = if spawns { "agent" } else { "other" };
    let agent_model = string_field(Some(item), "model")
        .filter(|_| spawns)
        .map(str::to_string);
    if completed {
        HarnessEvent::ToolUpdated {
            agent_model,
            call_id: call_id.to_string(),
            title: Some(title),
            kind: Some(kind.into()),
            status: Some(status.into()),
            detail,
            preview: None,
            paths: None,
        }
    } else {
        HarnessEvent::ToolStarted {
            agent_model,
            call_id: call_id.to_string(),
            title,
            kind: Some(kind.into()),
            status: Some(status.into()),
            background: None,
            preview: None,
            paths: None,
        }
    }
}

const TERMINAL_AGENT_STATES: [&str; 14] = [
    "completed",
    "complete",
    "done",
    "finished",
    "errored",
    "error",
    "failed",
    "notfound",
    "not_found",
    "closed",
    "interrupted",
    "stopped",
    "cancelled",
    "canceled",
];

const FAILED_AGENT_STATES: [&str; 6] = [
    "errored",
    "error",
    "failed",
    "notfound",
    "not_found",
    "interrupted",
];

/// One child thread's settled state from `codexSubagentStates`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentState {
    pub thread_id: String,
    /// `"failed"` or `"completed"`.
    pub status: &'static str,
    pub message: Option<String>,
}

/// `codexSubagentStates`: per-agent state a collab item reports, keyed by
/// child thread. This is what settles a spawned agent's row, since the spawn
/// call returns long before the agent it started is finished.
pub fn codex_subagent_states(item: &Record) -> Vec<SubagentState> {
    let Some(states) = agents_states(item) else {
        return Vec::new();
    };
    states
        .iter()
        .filter_map(|(thread_id, value)| {
            let state = as_record(Some(value));
            let status = string_field(state, "status").unwrap_or("").to_lowercase();
            if thread_id.is_empty() || !TERMINAL_AGENT_STATES.contains(&status.as_str()) {
                return None;
            }
            let message = string_field(state, "message")
                .map(js::trim)
                .filter(|message| !message.is_empty());
            Some(SubagentState {
                thread_id: thread_id.clone(),
                status: if FAILED_AGENT_STATES.contains(&status.as_str()) {
                    "failed"
                } else {
                    "completed"
                },
                message: message.map(str::to_string),
            })
        })
        .collect()
}

/// `codexSubagentThreadIds`: thread ids a collab item ties to an agent row,
/// so the child thread's own notifications can be mirrored back onto it.
/// Codex runs each subagent as a separate thread on the same connection.
pub fn codex_subagent_thread_ids(item: &Record) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut add = |id: &str| {
        if !id.is_empty() && !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
    };
    for key in ["agentThreadId", "agent_thread_id"] {
        if let Some(value) = string_field(Some(item), key) {
            add(value);
        }
    }
    for value in receiver_ids(item) {
        add(value);
    }
    if let Some(states) = agents_states(item) {
        for key in states.keys() {
            add(key);
        }
    }
    ids
}

/// `mapCodexSubagentSteps`: a child thread's own notification, mirrored onto
/// the agent row that spawned it. Only settled items are mirrored: the deltas
/// that stream inside a child thread carry no item identity, so they cannot
/// merge onto a step without stacking the same sentence up on every chunk.
pub fn map_codex_subagent_steps(call_id: &str, method: &str, params: &Value) -> Vec<HarnessEvent> {
    if method == "thread/started" {
        let thread = as_record(as_record(Some(params)).and_then(|rec| rec.get("thread")));
        return match string_field(thread, "model") {
            Some(model) => vec![HarnessEvent::ToolUpdated {
                agent_model: Some(model.to_string()),
                call_id: call_id.to_string(),
                title: None,
                kind: Some("agent".into()),
                status: None,
                detail: None,
                preview: None,
                paths: None,
            }],
            None => Vec::new(),
        };
    }
    if method != "item/started" && method != "item/completed" {
        return Vec::new();
    }
    let rec = as_record(Some(params));
    let item = as_record(rec.and_then(|rec| rec.get("item")));
    let Some(item_id) = string_field(item, "id") else {
        return Vec::new();
    };
    if rec.is_none() {
        return Vec::new();
    }
    map_codex_notification(method, params)
        .events
        .into_iter()
        .filter_map(|event| match event {
            HarnessEvent::ToolStarted {
                call_id: step_id,
                title,
                kind,
                status,
                preview,
                ..
            } => Some(agent_tool_step(
                call_id,
                step_id,
                Some(title),
                kind,
                status,
                None,
                preview,
            )),
            HarnessEvent::ToolUpdated {
                call_id: step_id,
                title,
                kind,
                status,
                detail,
                preview,
                ..
            } => {
                // Only a failure earns detail: a settled result already rides
                // in the preview, and a long one would weigh the run down.
                let detail = detail.filter(|_| status.as_deref() == Some("failed"));
                Some(agent_tool_step(
                    call_id, step_id, title, kind, status, detail, preview,
                ))
            }
            HarnessEvent::MessageDelta { text } => Some(HarnessEvent::AgentStep {
                call_id: call_id.to_string(),
                step_id: format!("{item_id}:text"),
                kind: AgentStepKind::Message,
                text,
                tool_kind: None,
                status: None,
                detail: None,
                preview: None,
                agent_name: None,
                agent_type: None,
            }),
            HarnessEvent::ReasoningDelta { text } => Some(HarnessEvent::AgentStep {
                call_id: call_id.to_string(),
                step_id: format!("{item_id}:reasoning"),
                kind: AgentStepKind::Reasoning,
                text,
                tool_kind: None,
                status: None,
                detail: None,
                preview: None,
                agent_name: None,
                agent_type: None,
            }),
            _ => None,
        })
        .collect()
}

fn agent_tool_step(
    call_id: &str,
    step_id: String,
    title: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    detail: Option<String>,
    preview: Option<ToolPreview>,
) -> HarnessEvent {
    HarnessEvent::AgentStep {
        call_id: call_id.to_string(),
        step_id,
        kind: AgentStepKind::Tool,
        text: title.unwrap_or_default(),
        tool_kind: kind.filter(|kind| !kind.is_empty()),
        status: status.filter(|status| !status.is_empty()),
        detail: detail.filter(|detail| !detail.is_empty()),
        preview,
        agent_name: None,
        agent_type: None,
    }
}

/// `agentBrief`: the first line of a spawn's prompt, short enough to sit on
/// a row.
fn agent_brief(item: &Record) -> Option<String> {
    let path =
        string_field(Some(item), "agentPath").or_else(|| string_field(Some(item), "agent_path"));
    if let Some(leaf) = path.and_then(leaf_segment) {
        return Some(format!("{} subagent", format_agent_type(leaf)));
    }
    let prompt = string_field(Some(item), "prompt")?;
    let line = prompt
        .split('\n')
        .map(js::trim)
        .find(|part| !part.is_empty())?;
    if js::len(line) <= 160 {
        return Some(line.to_string());
    }
    Some(format!("{}\u{2026}", js::slice_prefix(line, 159)))
}

fn collab_agent_failure_detail(item: &Record) -> Option<String> {
    let mut errors: Vec<&str> = Vec::new();
    if let Some(states) = agents_states(item) {
        for value in states.values() {
            let state = as_record(Some(value));
            let status = string_field(state, "status").unwrap_or("").to_lowercase();
            if status != "errored" && status != "notfound" && status != "not_found" {
                continue;
            }
            let message = string_field(state, "message").unwrap_or("Subagent failed.");
            if !errors.contains(&message) {
                errors.push(message);
            }
        }
    }
    if !errors.is_empty() {
        return Some(errors.join("\n"));
    }
    (string_field(Some(item), "status") == Some("failed"))
        .then(|| "Subagent operation failed.".into())
}

/// `changes[].path` for every change that names one.
fn change_paths(changes: &[Value]) -> Vec<String> {
    changes
        .iter()
        .filter_map(|change| string_field(as_record(Some(change)), "path"))
        .map(str::to_string)
        .collect()
}

fn edit_title(path: Option<&str>) -> String {
    let fallback = match path {
        Some(path) => format!("Edit {path}"),
        None => "Edit".into(),
    };
    let title = compose_tool_title(&ToolTitleInput {
        kind: Some("edit"),
        title: Some(&fallback),
        path,
        preview_kind: Some(ToolPreviewKind::Write),
        ..Default::default()
    });
    if title.is_empty() {
        "Edit".into()
    } else {
        title
    }
}

fn map_file_change_item(item: &Record, call_id: &str, completed: bool) -> HarnessEvent {
    let changes: &[Value] = match item.get("changes") {
        Some(Value::Array(changes)) => changes,
        _ => &[],
    };
    let paths = change_paths(changes);
    let first = as_record(changes.first());
    let path = string_field(first, "path");
    let diff = string_field(first, "diff");
    let status = map_item_status(string_field(Some(item), "status"), completed);
    let preview = build_diff_preview(path, diff);
    let title = edit_title(path);
    let paths = (!paths.is_empty()).then_some(paths);
    if !completed {
        return tool_started(call_id, title, "edit", Some(status), preview, paths);
    }
    tool_updated(
        call_id,
        Some(title),
        Some("edit"),
        Some(status),
        None,
        preview,
        paths,
    )
}

fn map_file_change_patch(rec: &Record) -> MappedCodexNotification {
    let item_id = string_field(Some(rec), "itemId").unwrap_or("");
    if item_id.is_empty() {
        return MappedCodexNotification::none();
    }
    let changes: &[Value] = match rec.get("changes") {
        Some(Value::Array(changes)) => changes,
        _ => &[],
    };
    let paths = change_paths(changes);
    let first = as_record(changes.first());
    let path = string_field(first, "path");
    let diff = string_field(first, "diff").or_else(|| string_field(Some(rec), "diff"));
    let preview = build_diff_preview(path, diff);
    let title = edit_title(path);
    MappedCodexNotification::events(vec![tool_updated(
        item_id,
        Some(title),
        Some("edit"),
        Some("in_progress".into()),
        None,
        preview,
        (!paths.is_empty()).then_some(paths),
    )])
}

/// `buildDiffPreview`: an edit preview from Codex's unified diff.
pub fn build_diff_preview(path: Option<&str>, diff: Option<&str>) -> Option<ToolPreview> {
    if path.is_none() && diff.is_none() {
        return None;
    }
    let mut fake = Record::new();
    fake.insert("kind".into(), json!("edit"));
    fake.insert(
        "title".into(),
        json!(match path {
            Some(path) => format!("Edit {path}"),
            None => "Edit".into(),
        }),
    );
    let mut diff_block = Record::new();
    diff_block.insert("type".into(), json!("diff"));
    if let Some(path) = path {
        diff_block.insert("path".into(), json!(path));
    }
    if let Some(diff) = diff {
        diff_block.insert("patch".into(), json!(diff));
    }
    fake.insert("content".into(), json!([diff_block]));
    if let Some(path) = path {
        fake.insert("locations".into(), json!([{ "path": path }]));
    }
    extract_tool_preview(&fake, &fake).or_else(|| {
        path.map(|path| {
            let mut preview = ToolPreview::new(ToolPreviewKind::Write);
            preview.path = Some(path.to_string());
            preview.file_name = Some(last_segment(path).to_string());
            preview
        })
    })
}

fn map_item_status(status: Option<&str>, completed: bool) -> String {
    match status {
        Some("completed") => "completed".into(),
        Some("failed") | Some("declined") => "failed".into(),
        Some("inProgress") => "in_progress".into(),
        _ if completed => "completed".into(),
        _ => "in_progress".into(),
    }
}

/// A mapped server approval request.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedApproval {
    pub kind: CodexApprovalKind,
    /// Always `HarnessEvent::ApprovalRequested`.
    pub event: HarnessEvent,
}

/// `mapApprovalRequest`.
pub fn map_approval_request(
    method: &str,
    params: &Value,
    request_id: i64,
) -> Option<MappedApproval> {
    let rec = as_record(Some(params))?;
    let call_id = string_field(Some(rec), "itemId").map(str::to_string);

    match method {
        "item/commandExecution/requestApproval" => {
            let command = string_field(Some(rec), "command").unwrap_or("Shell");
            let reason = string_field(Some(rec), "reason");
            let presentation = codex_command_presentation(rec, command);
            let readable = presentation.title != command;
            let title = if readable {
                presentation.title
            } else if let Some(reason) = reason {
                format!("{command} \u{2014} {reason}")
            } else {
                command.to_string()
            };
            Some(MappedApproval {
                kind: CodexApprovalKind::Command,
                event: HarnessEvent::ApprovalRequested {
                    request_id,
                    title,
                    kind: Some("execute".into()),
                    call_id,
                    preview: presentation.preview,
                },
            })
        }
        "item/fileChange/requestApproval" => {
            let title = string_field(Some(rec), "reason").unwrap_or("Approve file changes");
            Some(MappedApproval {
                kind: CodexApprovalKind::FileChange,
                event: HarnessEvent::ApprovalRequested {
                    request_id,
                    title: title.to_string(),
                    kind: Some("edit".into()),
                    call_id,
                    preview: None,
                },
            })
        }
        "item/permissions/requestApproval" => {
            let title = string_field(Some(rec), "reason").unwrap_or("Approve permissions");
            Some(MappedApproval {
                kind: CodexApprovalKind::Permissions,
                event: HarnessEvent::ApprovalRequested {
                    request_id,
                    title: title.to_string(),
                    kind: Some("other".into()),
                    call_id,
                    preview: None,
                },
            })
        }
        _ => None,
    }
}
