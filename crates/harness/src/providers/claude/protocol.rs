//! Port of src/integrations/harness/providers/claude/claudeProtocol.ts: the
//! pure half of the Claude Code provider. It builds spawn arguments, user
//! messages, and control frames, and reads stream-json lines.
//!
//! TypeScript passed `Record<string, unknown>` around. Here a record is a
//! `serde_json::Map`, and functions that read one take `&Record`.

use std::sync::LazyLock;

use monocode_core::attachment::{Attachment, AttachmentKind, attachment_path_text, prompt_text};
use monocode_core::block::{TaskListItem, ToolPreview, TurnMetrics};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::js;
use monocode_core::task_list::{
    is_task_list_tool_name, normalize_task_list_status_str, task_list_from_tool_input,
};
use monocode_core::user_question::{
    UserQuestionReply, question_prompt_title, questions_from_unknown, selected_answer_labels,
};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::shared::{
    OrderedMap, extract_tool_preview, is_agent_tool_name, stream_text_delta, title_from_tool_input,
};

/// A JSON object, the `Record<string, unknown>` of the TypeScript.
pub type Record = Map<String, Value>;

/// Claude Code versions that first ship Opus 5.5, Opus 5, Sonnet 5, Fable 5,
/// Opus 4.8, and Opus 4.7.
pub const MINIMUM_CLAUDE_OPUS_5_5_VERSION: &str = "2.1.280";
pub const MINIMUM_CLAUDE_OPUS_5_VERSION: &str = "2.1.219";
pub const MINIMUM_CLAUDE_SONNET_5_VERSION: &str = "2.1.197";
pub const MINIMUM_CLAUDE_FABLE_5_VERSION: &str = "2.1.169";
pub const MINIMUM_CLAUDE_OPUS_4_8_VERSION: &str = "2.1.154";
pub const MINIMUM_CLAUDE_OPUS_4_7_VERSION: &str = "2.1.111";

pub const CLAUDE_SETTING_SOURCES: &str = "user,project,local";

pub const SUPPORTED_CLAUDE_IMAGE_MIME_TYPES: [&str; 4] =
    ["image/gif", "image/jpeg", "image/png", "image/webp"];

/// `ClaudePermissionMode`: the values `--permission-mode` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaudePermissionMode {
    Default,
    Plan,
    AcceptEdits,
    Auto,
    BypassPermissions,
}

impl ClaudePermissionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ClaudePermissionMode::Default => "default",
            ClaudePermissionMode::Plan => "plan",
            ClaudePermissionMode::AcceptEdits => "acceptEdits",
            ClaudePermissionMode::Auto => "auto",
            ClaudePermissionMode::BypassPermissions => "bypassPermissions",
        }
    }
}

/// `ClaudeControlRequest`: a `control_request` the CLI sent us.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeControlRequest {
    pub request_id: String,
    pub subtype: String,
    pub tool_name: Option<String>,
    pub input: Record,
    pub tool_use_id: Option<String>,
}

/// `ClaudeCliSettings`: the JSON passed to `--settings`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCliSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_thinking_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ultracode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_all_hooks: Option<bool>,
}

impl ClaudeCliSettings {
    /// No field is set, the `Object.keys(settings).length === 0` check.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// `asRecord`.
pub fn as_record(value: &Value) -> Option<&Record> {
    value.as_object()
}

/// `asRecord(rec[key])`.
pub fn record_field<'a>(rec: Option<&'a Record>, key: &str) -> Option<&'a Record> {
    rec?.get(key).and_then(Value::as_object)
}

/// `stringField`: a string that is not blank. The value comes back untrimmed.
pub fn string_field<'a>(rec: Option<&'a Record>, key: &str) -> Option<&'a str> {
    let value = rec?.get(key)?.as_str()?;
    (!js::trim(value).is_empty()).then_some(value)
}

fn type_of(rec: &Record) -> Option<&str> {
    string_field(Some(rec), "type")
}

fn subtype_of(rec: &Record) -> Option<&str> {
    string_field(Some(rec), "subtype")
}

static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());

/// `parseClaudeVersion`.
pub fn parse_claude_version(output: &str) -> Option<String> {
    VERSION.find(output).map(|m| m.as_str().to_string())
}

/// `Number.parseInt(part, 10) || 0`: leading digits, with an optional sign.
fn parse_int_or_zero(part: &str) -> i64 {
    let text = js::trim(part);
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().map(|n| sign * n).unwrap_or(0)
}

/// `compareSemver`: negative, zero, or positive, like a sort comparator.
pub fn compare_semver(left: &str, right: &str) -> i64 {
    let a: Vec<i64> = left.split('.').map(parse_int_or_zero).collect();
    let b: Vec<i64> = right.split('.').map(parse_int_or_zero).collect();
    for i in 0..3 {
        let delta = a.get(i).copied().unwrap_or(0) - b.get(i).copied().unwrap_or(0);
        if delta != 0 {
            return delta;
        }
    }
    0
}

/// `runtimeModeToPermission`.
///
/// Supervised maps onto a flag like every other mode. Sending nothing left the
/// CLI free to fall back to `permissions.defaultMode` from the user's settings,
/// so a session the picker called Supervised could silently run as `auto`.
/// `default` is the value that asks; `manual` is its alias but needs CLI 2.1.200.
pub fn runtime_mode_to_permission(mode: RuntimeMode) -> ClaudePermissionMode {
    match mode {
        RuntimeMode::AutoAcceptEdits => ClaudePermissionMode::AcceptEdits,
        RuntimeMode::Auto => ClaudePermissionMode::Auto,
        RuntimeMode::FullAccess => ClaudePermissionMode::BypassPermissions,
        RuntimeMode::Supervised => ClaudePermissionMode::Default,
    }
}

/// Older model ids that reject `xhigh` and want `max` instead.
const LEGACY_XHIGH_AS_MAX: [&str; 8] = [
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5",
    "claude-haiku-4-5",
    "claude-opus-4-1",
    "claude-opus-4-0",
    "claude-sonnet-4-0",
    "claude-sonnet-4-5",
];

/// `normalizeClaudeCliEffort`: a resolved Claude effort for `--effort`.
/// `ultracode` pairs with `xhigh`; `ultrathink` is a prompt prefix, not a CLI effort.
pub fn normalize_claude_cli_effort(effort: Option<&str>, model: Option<&str>) -> Option<String> {
    let effort = effort.filter(|effort| !effort.is_empty())?;
    if effort == "ultrathink" {
        return None;
    }
    if effort == "ultracode" {
        return Some("xhigh".into());
    }
    let model = model.filter(|model| !model.is_empty());
    if effort == "xhigh" && model.is_some_and(|model| LEGACY_XHIGH_AS_MAX.contains(&model)) {
        return Some("max".into());
    }
    if effort == "max" && model == Some("claude-sonnet-4-6") {
        return Some("high".into());
    }
    Some(effort.to_string())
}

/// `isClaudeUltracodeEffort`.
pub fn is_claude_ultracode_effort(effort: Option<&str>) -> bool {
    effort == Some("ultracode")
}

/// `applyClaudePromptEffortPrefix`.
pub fn apply_claude_prompt_effort_prefix(text: &str, effort: Option<&str>) -> String {
    if effort != Some("ultrathink") {
        return text.to_string();
    }
    if text.is_empty() {
        return "Ultrathink:".into();
    }
    format!("Ultrathink:\n{text}")
}

/// `resolveClaudeApiModelId`: `[1m]` selects the 1M context window.
pub fn resolve_claude_api_model_id(model: &str, context: Option<&str>) -> String {
    if context == Some("1m") {
        return format!("{model}[1m]");
    }
    model.to_string()
}

/// `parseJsonLine`: one stream-json object, or `None` for anything else.
pub fn parse_json_line(line: &str) -> Option<Record> {
    let trimmed = js::trim(line);
    if !trimmed.starts_with('{') {
        return None;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// `buildClaudeUserMessage`. Fails like `attachmentPathText` when an
/// attachment has no local path.
pub fn build_claude_user_message(
    text: &str,
    attachments: &[Attachment],
    effort: Option<&str>,
) -> Result<Value, String> {
    let text = apply_claude_prompt_effort_prefix(&prompt_text(text, attachments), effort);
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(json!({ "type": "text", "text": text }));
    }
    for attachment in attachments {
        match image_content_block(attachment) {
            Some(block) => content.push(block),
            None => {
                content.push(json!({ "type": "text", "text": attachment_path_text(attachment)? }))
            }
        }
    }
    Ok(json!({
        "type": "user",
        "session_id": "",
        "parent_tool_use_id": null,
        "message": { "role": "user", "content": content },
    }))
}

/// `(message.message as { content: unknown[] }).content`.
pub fn user_message_content(message: &Value) -> &[Value] {
    message
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn image_content_block(attachment: &Attachment) -> Option<Value> {
    let data = attachment.data.as_deref().filter(|data| !data.is_empty())?;
    if attachment.kind != AttachmentKind::Image {
        return None;
    }
    let mime = normalize_image_mime(&attachment.mime_type);
    if !SUPPORTED_CLAUDE_IMAGE_MIME_TYPES.contains(&mime) {
        return None;
    }
    Some(json!({
        "type": "image",
        "source": { "type": "base64", "media_type": mime, "data": data },
    }))
}

/// This module's own `normalizeImageMime`, which only renames `image/jpg`.
fn normalize_image_mime(mime: &str) -> &str {
    if mime == "image/jpg" {
        "image/jpeg"
    } else {
        mime
    }
}

/// The input to `buildClaudeSpawnArgs`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeSpawnOptions {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<ClaudePermissionMode>,
    pub resume: Option<String>,
    pub session_id: Option<String>,
    pub settings: Option<ClaudeCliSettings>,
    /// `None` means on, as in TypeScript, where only `false` turned it off.
    pub include_partial_messages: Option<bool>,
    pub max_turns: Option<i64>,
    pub isolated: bool,
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

/// `buildClaudeSpawnArgs`.
pub fn build_claude_spawn_args(input: &ClaudeSpawnOptions) -> Vec<String> {
    let mut args: Vec<String> = [
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
    ]
    .map(String::from)
    .to_vec();
    if !input.isolated {
        args.extend(["--permission-prompt-tool".into(), "stdio".into()]);
    }
    if input.include_partial_messages != Some(false) {
        args.push("--include-partial-messages".into());
    }
    // Isolated spawns are MonoCode's own helper calls (titles, summaries); the
    // user's hooks have no business firing there. Interactive sessions inherit
    // whatever the caller decided so `~/.claude` hooks keep working.
    let mut settings = input.settings.clone().unwrap_or_default();
    if input.isolated {
        settings.disable_all_hooks = Some(true);
    }
    let settings = serde_json::to_string(&settings).unwrap_or_else(|_| "{}".into());
    if input.isolated {
        args.push("--no-session-persistence".into());
        args.push("--strict-mcp-config".into());
        args.push("--mcp-config".into());
        args.push(r#"{"mcpServers":{}}"#.into());
        args.push("--settings".into());
        args.push(settings);
    } else {
        args.push(format!("--setting-sources={CLAUDE_SETTING_SOURCES}"));
        args.push("--settings".into());
        args.push(settings);
    }
    if let Some(model) = non_empty(&input.model) {
        args.extend(["--model".into(), model.to_string()]);
    }
    if let Some(effort) = non_empty(&input.effort) {
        args.extend(["--effort".into(), effort.to_string()]);
    }
    if let Some(mode) = input.permission_mode {
        args.extend(["--permission-mode".into(), mode.as_str().to_string()]);
    }
    if input.permission_mode == Some(ClaudePermissionMode::BypassPermissions) {
        args.push("--allow-dangerously-skip-permissions".into());
    }
    if let Some(resume) = non_empty(&input.resume) {
        args.extend(["--resume".into(), resume.to_string()]);
    }
    if let Some(session_id) = non_empty(&input.session_id) {
        args.extend(["--session-id".into(), session_id.to_string()]);
    }
    if let Some(max_turns) = input.max_turns.filter(|turns| *turns != 0) {
        args.extend(["--max-turns".into(), max_turns.to_string()]);
    }
    args
}

/// `buildControlRequest`.
pub fn build_control_request(request_id: &str, request: Value) -> Value {
    json!({ "type": "control_request", "request_id": request_id, "request": request })
}

/// `buildControlResponse`.
pub fn build_control_response(request_id: &str, response: Value) -> Value {
    json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response },
    })
}

/// `ClaudeControlResponse`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeControlResponse {
    pub request_id: String,
    pub ok: bool,
    pub payload: Option<Record>,
    pub error: Option<String>,
}

/// `parseControlResponse`.
pub fn parse_control_response(rec: &Record) -> Option<ClaudeControlResponse> {
    if type_of(rec) != Some("control_response") {
        return None;
    }
    let nested = record_field(Some(rec), "response");
    let request_id = string_field(nested, "request_id")
        .or_else(|| string_field(Some(rec), "request_id"))
        .unwrap_or("");
    if request_id.is_empty() {
        return None;
    }
    let subtype = string_field(nested, "subtype").unwrap_or("");
    if subtype == "error" {
        return Some(ClaudeControlResponse {
            request_id: request_id.to_string(),
            ok: false,
            payload: None,
            error: Some(
                string_field(nested, "error")
                    .unwrap_or("control request failed")
                    .to_string(),
            ),
        });
    }
    if !subtype.is_empty() && subtype != "success" {
        return None;
    }
    Some(ClaudeControlResponse {
        request_id: request_id.to_string(),
        ok: true,
        payload: Some(
            record_field(nested, "response")
                .cloned()
                .unwrap_or_default(),
        ),
        error: None,
    })
}

/// `listModelsFromControlResponse`: rows from a `list_models` control
/// response, or `None` if this line is something else.
pub fn list_models_from_control_response(rec: &Record, request_id: &str) -> Option<Vec<Value>> {
    let parsed = parse_control_response(rec)?;
    if parsed.request_id != request_id {
        return None;
    }
    if !parsed.ok {
        return Some(Vec::new());
    }
    Some(
        parsed
            .payload
            .as_ref()
            .and_then(|payload| payload.get("models"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    )
}

/// `isClaudeInitMessage`.
pub fn is_claude_init_message(rec: &Record) -> bool {
    type_of(rec) == Some("system") && matches!(subtype_of(rec), Some("init" | "initialized"))
}

/// `toClaudePermissionResult`.
pub fn to_claude_permission_result(decision: ApprovalDecision, input: &Record) -> Value {
    match decision {
        ApprovalDecision::Allow => json!({ "behavior": "allow", "updatedInput": input }),
        ApprovalDecision::Deny => {
            json!({ "behavior": "deny", "message": "User declined tool execution." })
        }
    }
}

/// `parseControlRequest`.
pub fn parse_control_request(rec: &Record) -> Option<ClaudeControlRequest> {
    if !matches!(
        type_of(rec),
        Some("control_request" | "sdk_control_request")
    ) {
        return None;
    }
    let nested = record_field(Some(rec), "request");
    let request_id = string_field(Some(rec), "request_id")
        .or_else(|| string_field(nested, "request_id"))
        .unwrap_or("");
    let subtype = string_field(nested, "subtype")
        .or_else(|| string_field(Some(rec), "subtype"))
        .unwrap_or("");
    if request_id.is_empty() || subtype.is_empty() {
        return None;
    }
    let input = record_field(nested, "input")
        .or_else(|| record_field(nested, "tool_input"))
        .or_else(|| record_field(Some(rec), "input"))
        .cloned()
        .unwrap_or_default();
    Some(ClaudeControlRequest {
        request_id: request_id.to_string(),
        subtype: subtype.to_string(),
        tool_name: string_field(nested, "tool_name")
            .or_else(|| string_field(Some(rec), "tool_name"))
            .map(str::to_string),
        input,
        tool_use_id: string_field(nested, "tool_use_id")
            .or_else(|| string_field(nested, "toolUseID"))
            .or_else(|| string_field(Some(rec), "tool_use_id"))
            .map(str::to_string),
    })
}

/// `parseControlCancelId`.
pub fn parse_control_cancel_id(rec: &Record) -> Option<String> {
    if type_of(rec) != Some("control_cancel_request") {
        return None;
    }
    string_field(Some(rec), "request_id")
        .or_else(|| string_field(record_field(Some(rec), "request"), "request_id"))
        .map(str::to_string)
}

fn parent_tool_use_id(rec: &Record) -> Option<&str> {
    rec.get("parent_tool_use_id")
        .and_then(Value::as_str)
        .filter(|parent| !parent.is_empty())
}

/// `sessionIdFromMessage`.
pub fn session_id_from_message(rec: &Record) -> Option<String> {
    if type_of(rec) == Some("system") && subtype_of(rec).is_some_and(|s| s.starts_with("hook_")) {
        return None;
    }
    // Subagents can carry their own session id. Rebinding the parent to it
    // would drop resume for the conversation the user is actually in.
    if parent_tool_use_id(rec).is_some() {
        return None;
    }
    string_field(Some(rec), "session_id").map(str::to_string)
}

/// Claude Code pings `system/status` for every request lifecycle step
/// ("requesting", "responding", and so on). Codex and opencode only emit status
/// text for notable events (retries, warnings, compaction), so drop the
/// lifecycle chatter here and keep the transcript comparable across harnesses.
const LIFECYCLE_STATUSES: [&str; 18] = [
    "requesting",
    "request",
    "responding",
    "response",
    "streaming",
    "thinking",
    "working",
    "running",
    "pending",
    "queued",
    "waiting",
    "in_progress",
    "tool_use",
    "idle",
    "done",
    "completed",
    "status",
    "compact",
];

/// `statusTextFromSystem`.
pub fn status_text_from_system(rec: &Record) -> Option<String> {
    if type_of(rec) != Some("system") {
        return None;
    }
    let subtype = subtype_of(rec).unwrap_or("");
    let compact = subtype.starts_with("compact");
    if subtype != "status" && !compact {
        return None;
    }
    // Prose lives in `message`; `status` carries the bare lifecycle token.
    let text = js::trim(string_field(Some(rec), "message").unwrap_or(""));
    let lower = text.to_lowercase();
    let token = lower.trim_end_matches(|c: char| js::is_space(c) || c == '.' || c == '…');
    if !text.is_empty() && !LIFECYCLE_STATUSES.contains(&token) {
        return Some(text.to_string());
    }
    // Compaction is worth one row even when the CLI sends no prose with it.
    compact.then(|| "Compacted context".to_string())
}

/// How a Claude turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeTurnStatus {
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeTurnResult {
    pub status: ClaudeTurnStatus,
    pub error: Option<String>,
}

fn string_items(rec: &Record, key: &str) -> Vec<String> {
    rec.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `turnStatusFromResult`.
pub fn turn_status_from_result(rec: &Record) -> ClaudeTurnResult {
    let done = |status| ClaudeTurnResult {
        status,
        error: None,
    };
    if subtype_of(rec) == Some("success") {
        return done(ClaudeTurnStatus::Completed);
    }
    let errors = string_items(rec, "errors");
    let joined = errors.join(" ").to_lowercase();
    let terminal = string_field(Some(rec), "terminal_reason").unwrap_or("");
    if terminal == "aborted_tools"
        || terminal == "aborted_streaming"
        || joined.contains("interrupt")
    {
        return done(ClaudeTurnStatus::Interrupted);
    }
    if joined.contains("cancel") {
        return done(ClaudeTurnStatus::Cancelled);
    }
    let error = errors
        .into_iter()
        .find(|item| !item.starts_with("[ede_diagnostic]"));
    ClaudeTurnResult {
        status: ClaudeTurnStatus::Failed,
        error: Some(error.unwrap_or_else(|| "Claude turn failed.".into())),
    }
}

/// A refused usage window, with when it resets (epoch ms) when known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaudeUsageLimit {
    pub resets_at: Option<i64>,
}

/// `normalizeEpochMs` from rateLimits.ts.
fn normalize_epoch_ms(value: f64) -> Option<f64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    // 1e10 sits between seconds-epoch (<2286) and millisecond-epoch (>2001).
    Some(if value > 10_000_000_000.0 {
        value
    } else {
        value * 1000.0
    })
}

/// `parseResetTimestamp` from src/features/providers/model/rateLimits.ts.
pub fn parse_reset_timestamp(value: Option<&Value>) -> Option<i64> {
    let ms = match value? {
        Value::Number(n) => normalize_epoch_ms(n.as_f64()?),
        Value::String(text) => {
            if js::trim(text).is_empty() {
                return None;
            }
            match js::parse_number(text).filter(|n| n.is_finite()) {
                Some(numeric) => normalize_epoch_ms(numeric),
                None => parse_date(text),
            }
        }
        _ => None,
    }?;
    Some(ms as i64)
}

/// `Date.parse` for the ISO 8601 forms a reset time comes in.
// TODO(port): Date.parse also reads RFC 2822 and local-time forms; only RFC 3339
// and a bare date are read here.
fn parse_date(text: &str) -> Option<f64> {
    use time::format_description::well_known::Rfc3339;
    let text = js::trim(text);
    if let Ok(at) = time::OffsetDateTime::parse(text, &Rfc3339) {
        return Some((at.unix_timestamp_nanos() / 1_000_000) as f64);
    }
    let mut parts = text.splitn(3, '-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let month = time::Month::try_from(month.parse::<u8>().ok()?).ok()?;
    let date = time::Date::from_calendar_date(year.parse().ok()?, month, day.parse().ok()?).ok()?;
    Some((date.midnight().assume_utc().unix_timestamp() * 1000) as f64)
}

/// `usageLimitFromRateLimitEvent`: a `rate_limit_event` that refuses requests.
/// `None` once requests are allowed again, or while extra usage is paying for
/// them and the turn goes on.
pub fn usage_limit_from_rate_limit_event(rec: &Record) -> Option<ClaudeUsageLimit> {
    let info = record_field(Some(rec), "rate_limit_info");
    if string_field(info, "status") != Some("rejected") {
        return None;
    }
    if info.and_then(|info| info.get("isUsingOverage")) == Some(&Value::Bool(true)) {
        return None;
    }
    Some(ClaudeUsageLimit {
        resets_at: parse_reset_timestamp(info.and_then(|info| info.get("resetsAt"))),
    })
}

static USAGE_LIMIT_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)hit your (?:usage )?limit|usage limit reached").unwrap());

/// `isUsageLimitResult`: Claude also ends a limited turn with the limit as
/// its error text.
pub fn is_usage_limit_result(rec: &Record) -> bool {
    if rec.get("is_error") != Some(&Value::Bool(true)) {
        return false;
    }
    let result = string_field(Some(rec), "result").unwrap_or("").to_string();
    std::iter::once(result)
        .chain(string_items(rec, "errors"))
        .any(|text| USAGE_LIMIT_TEXT.is_match(&text))
}

/// Which stream a text delta belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeDeltaKind {
    Assistant,
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeStreamDelta {
    pub kind: ClaudeDeltaKind,
    pub text: String,
}

/// `streamDeltaFromEvent`.
pub fn stream_delta_from_event(rec: &Record) -> Option<ClaudeStreamDelta> {
    let event = record_field(Some(rec), "event")?;
    if type_of(event) != Some("content_block_delta") {
        return None;
    }
    let delta = record_field(Some(event), "delta");
    let (kind, key) = match string_field(delta, "type").unwrap_or("") {
        "text_delta" => (ClaudeDeltaKind::Assistant, "text"),
        "thinking_delta" => (ClaudeDeltaKind::Reasoning, "thinking"),
        _ => return None,
    };
    let text = stream_text_delta(delta.and_then(|delta| delta.get(key)));
    (!text.is_empty()).then(|| ClaudeStreamDelta {
        kind,
        text: text.to_string(),
    })
}

fn event_index(event: &Record) -> i64 {
    match event.get("index") {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().unwrap_or(-1.0) as i64),
        _ => -1,
    }
}

/// A tool call the stream opened.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeToolStart {
    pub index: i64,
    pub id: String,
    pub name: String,
    pub input: Record,
}

/// `toolStartFromEvent`.
pub fn tool_start_from_event(rec: &Record) -> Option<ClaudeToolStart> {
    let event = record_field(Some(rec), "event")?;
    if type_of(event) != Some("content_block_start") {
        return None;
    }
    let block = record_field(Some(event), "content_block")?;
    if !matches!(
        type_of(block).unwrap_or(""),
        "tool_use" | "server_tool_use" | "mcp_tool_use"
    ) {
        return None;
    }
    let id = string_field(Some(block), "id")?;
    let name = string_field(Some(block), "name")?;
    Some(ClaudeToolStart {
        index: event_index(event),
        id: id.to_string(),
        name: name.to_string(),
        input: record_field(Some(block), "input")
            .cloned()
            .unwrap_or_default(),
    })
}

/// A piece of a tool call's input JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeInputJsonDelta {
    pub index: i64,
    pub partial: String,
}

/// `inputJsonDeltaFromEvent`.
pub fn input_json_delta_from_event(rec: &Record) -> Option<ClaudeInputJsonDelta> {
    let event = record_field(Some(rec), "event")?;
    if type_of(event) != Some("content_block_delta") {
        return None;
    }
    let delta = record_field(Some(event), "delta");
    if string_field(delta, "type") != Some("input_json_delta") {
        return None;
    }
    let partial = delta
        .and_then(|delta| delta.get("partial_json"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if partial.is_empty() {
        return None;
    }
    Some(ClaudeInputJsonDelta {
        index: event_index(event),
        partial: partial.to_string(),
    })
}

/// `isSubagentMessage`.
pub fn is_subagent_message(rec: &Record) -> bool {
    parent_tool_use_id(rec).is_some()
}

/// `isAgentTaskType`.
pub fn is_agent_task_type(task_type: Option<&str>) -> bool {
    matches!(
        task_type.unwrap_or("").to_lowercase().as_str(),
        "local_agent" | "remote_agent"
    )
}

fn system_subtype(rec: &Record, subtype: &str) -> bool {
    type_of(rec) == Some("system") && subtype_of(rec) == Some(subtype)
}

fn owned(value: Option<&str>) -> Option<String> {
    value.map(str::to_string)
}

/// `ClaudeAgentTaskStarted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAgentTaskStarted {
    pub task_id: String,
    pub tool_use_id: Option<String>,
    pub description: String,
    pub task_type: String,
    pub backgrounded: bool,
    pub ambient: bool,
}

/// `parseTaskStarted`.
pub fn parse_task_started(rec: &Record) -> Option<ClaudeAgentTaskStarted> {
    if !system_subtype(rec, "task_started") {
        return None;
    }
    let rec_ref = Some(rec);
    Some(ClaudeAgentTaskStarted {
        task_id: string_field(rec_ref, "task_id")?.to_string(),
        tool_use_id: owned(string_field(rec_ref, "tool_use_id")),
        description: string_field(rec_ref, "description")
            .unwrap_or("Subagent")
            .to_string(),
        task_type: string_field(rec_ref, "task_type").unwrap_or("").to_string(),
        backgrounded: rec.get("is_backgrounded") == Some(&Value::Bool(true)),
        ambient: rec.get("ambient") == Some(&Value::Bool(true)),
    })
}

/// `ClaudeAgentTaskProgress`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAgentTaskProgress {
    pub task_id: String,
    pub tool_use_id: Option<String>,
    pub description: String,
    pub subagent_type: Option<String>,
    pub last_tool_name: Option<String>,
    pub summary: Option<String>,
}

/// `parseTaskProgress`.
pub fn parse_task_progress(rec: &Record) -> Option<ClaudeAgentTaskProgress> {
    if !system_subtype(rec, "task_progress") {
        return None;
    }
    let rec = Some(rec);
    Some(ClaudeAgentTaskProgress {
        task_id: string_field(rec, "task_id")?.to_string(),
        tool_use_id: owned(string_field(rec, "tool_use_id")),
        description: string_field(rec, "description")
            .unwrap_or("Subagent")
            .to_string(),
        subagent_type: owned(string_field(rec, "subagent_type")),
        last_tool_name: owned(string_field(rec, "last_tool_name")),
        summary: owned(string_field(rec, "summary")),
    })
}

/// `ClaudeAgentTaskUpdated`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAgentTaskUpdated {
    pub task_id: String,
    pub status: Option<String>,
    pub description: Option<String>,
    pub error: Option<String>,
    pub backgrounded: Option<bool>,
}

/// `parseTaskUpdated`.
pub fn parse_task_updated(rec: &Record) -> Option<ClaudeAgentTaskUpdated> {
    if !system_subtype(rec, "task_updated") {
        return None;
    }
    let task_id = string_field(Some(rec), "task_id")?;
    let empty = Record::new();
    let patch = record_field(Some(rec), "patch").unwrap_or(&empty);
    Some(ClaudeAgentTaskUpdated {
        task_id: task_id.to_string(),
        status: owned(string_field(Some(patch), "status")),
        description: owned(string_field(Some(patch), "description")),
        error: owned(string_field(Some(patch), "error")),
        backgrounded: patch.get("is_backgrounded").and_then(Value::as_bool),
    })
}

/// `ClaudeAgentTaskNotification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAgentTaskNotification {
    pub task_id: String,
    pub tool_use_id: Option<String>,
    pub status: String,
    pub summary: String,
    pub ambient: bool,
}

/// `parseTaskNotification`.
pub fn parse_task_notification(rec: &Record) -> Option<ClaudeAgentTaskNotification> {
    if !system_subtype(rec, "task_notification") {
        return None;
    }
    let rec_ref = Some(rec);
    Some(ClaudeAgentTaskNotification {
        task_id: string_field(rec_ref, "task_id")?.to_string(),
        tool_use_id: owned(string_field(rec_ref, "tool_use_id")),
        status: string_field(rec_ref, "status")
            .unwrap_or("completed")
            .to_string(),
        summary: string_field(rec_ref, "summary").unwrap_or("").to_string(),
        ambient: rec.get("ambient") == Some(&Value::Bool(true)),
    })
}

/// `ClaudeBackgroundTask`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeBackgroundTask {
    pub task_id: String,
    pub task_type: String,
    pub description: String,
}

/// `parseBackgroundTasks`: every task Claude is running for the session:
/// subagents, shells, monitors.
pub fn parse_background_tasks(rec: &Record) -> Option<Vec<ClaudeBackgroundTask>> {
    if !system_subtype(rec, "background_tasks_changed") {
        return None;
    }
    let tasks = rec.get("tasks").and_then(Value::as_array);
    Some(
        tasks
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let row = as_record(item)?;
                if row.get("ambient") == Some(&Value::Bool(true)) {
                    return None;
                }
                let row = Some(row);
                Some(ClaudeBackgroundTask {
                    task_id: string_field(row, "task_id")?.to_string(),
                    task_type: string_field(row, "task_type").unwrap_or("").to_string(),
                    description: string_field(row, "description")
                        .unwrap_or("Subagent")
                        .to_string(),
                })
            })
            .collect(),
    )
}

/// `ClaudeToolProgress`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolProgress {
    pub tool_use_id: String,
    pub parent_tool_use_id: Option<String>,
    pub tool_name: Option<String>,
    pub subagent_type: Option<String>,
}

/// `parseToolProgress`.
pub fn parse_tool_progress(rec: &Record) -> Option<ClaudeToolProgress> {
    if type_of(rec) != Some("tool_progress") {
        return None;
    }
    let rec = Some(rec);
    Some(ClaudeToolProgress {
        tool_use_id: string_field(rec, "tool_use_id")?.to_string(),
        parent_tool_use_id: owned(string_field(rec, "parent_tool_use_id")),
        tool_name: owned(string_field(rec, "tool_name")),
        subagent_type: owned(string_field(rec, "subagent_type")),
    })
}

/// `isTerminalAgentTaskStatus`.
pub fn is_terminal_agent_task_status(status: Option<&str>) -> bool {
    matches!(
        status.unwrap_or("").to_lowercase().as_str(),
        "completed" | "failed" | "killed" | "stopped"
    )
}

fn message_content(rec: &Record) -> &[Value] {
    record_field(Some(rec), "message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn content_strings(rec: &Record, block_type: &str, key: &str) -> Vec<String> {
    message_content(rec)
        .iter()
        .filter_map(|block| {
            let row = as_record(block)?;
            if type_of(row) != Some(block_type) {
                return None;
            }
            let text = row.get(key).and_then(Value::as_str).unwrap_or("");
            (!text.is_empty()).then(|| text.to_string())
        })
        .collect()
}

/// `assistantTextBlocks`.
pub fn assistant_text_blocks(rec: &Record) -> Vec<String> {
    content_strings(rec, "text", "text")
}

/// `assistantThinkingBlocks`: reasoning a message carries, used to mirror a
/// subagent's thinking.
pub fn assistant_thinking_blocks(rec: &Record) -> Vec<String> {
    content_strings(rec, "thinking", "thinking")
}

/// `assistantMessageId`: provider id of an assistant message, for keying
/// steps mirrored from it.
pub fn assistant_message_id(rec: &Record) -> Option<String> {
    owned(string_field(record_field(Some(rec), "message"), "id"))
}

/// One `tool_use` block of an assistant message.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeToolUse {
    pub id: String,
    pub name: String,
    pub input: Record,
}

/// `assistantToolUses`.
pub fn assistant_tool_uses(rec: &Record) -> Vec<ClaudeToolUse> {
    message_content(rec)
        .iter()
        .filter_map(|block| {
            let row = as_record(block)?;
            if type_of(row) != Some("tool_use") {
                return None;
            }
            Some(ClaudeToolUse {
                id: string_field(Some(row), "id")?.to_string(),
                name: string_field(Some(row), "name")?.to_string(),
                input: record_field(Some(row), "input")
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// One `tool_result` block of a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolResult {
    pub tool_use_id: String,
    pub is_error: bool,
    pub text: String,
}

/// `toolResultsFromUserMessage`.
pub fn tool_results_from_user_message(rec: &Record) -> Vec<ClaudeToolResult> {
    message_content(rec)
        .iter()
        .filter_map(|block| {
            let row = as_record(block)?;
            if type_of(row) != Some("tool_result") {
                return None;
            }
            Some(ClaudeToolResult {
                tool_use_id: string_field(Some(row), "tool_use_id")?.to_string(),
                is_error: row.get("is_error") == Some(&Value::Bool(true)),
                text: tool_result_text(row.get("content")),
            })
        })
        .collect()
}

fn tool_result_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block {
                Value::String(text) => Some(text.as_str()),
                Value::Object(row) if type_of(row) == Some("text") => {
                    row.get("text").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect(),
        _ => String::new(),
    }
}

/// `extractExitPlanModePlan`.
pub fn extract_exit_plan_mode_plan(value: &Value) -> Option<String> {
    owned(string_field(as_record(value), "plan"))
}

/// `extractAskUserQuestionTitle`.
pub fn extract_ask_user_question_title(input: &Record) -> String {
    let title = question_prompt_title(&questions_from_unknown(&Value::Object(input.clone())));
    if title.is_empty() {
        "Claude question".into()
    } else {
        title
    }
}

/// `askUserQuestionAllowInput`: the `updatedInput` that answers an
/// AskUserQuestion call with the selected options.
pub fn ask_user_question_allow_input(input: &Record, reply: Option<&UserQuestionReply>) -> Value {
    let questions = questions_from_unknown(&Value::Object(input.clone()));
    let mut answers = Record::new();
    if let Some(UserQuestionReply::Answered {
        answers: selected,
        custom,
    }) = reply
    {
        for question in &questions {
            let labels = selected_answer_labels(question, selected, custom.as_ref());
            if labels.is_empty() {
                continue;
            }
            let answer = if question.multi_select {
                labels.join(", ")
            } else {
                labels[0].clone()
            };
            answers.insert(question.prompt.clone(), Value::String(answer));
        }
    }
    let mut out = Record::new();
    // `{ questions: undefined }` drops the key in JSON.stringify.
    if let Some(questions) = input.get("questions") {
        out.insert("questions".into(), questions.clone());
    }
    out.insert("answers".into(), Value::Object(answers));
    Value::Object(out)
}

/// `tryParseJsonRecord`.
pub fn try_parse_json_record(value: &str) -> Option<Record> {
    match serde_json::from_str::<Value>(value) {
        Ok(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// `taskListFromTodos`.
pub fn task_list_from_todos(input: &Record) -> Option<Vec<TaskListItem>> {
    task_list_from_tool_input("TodoWrite", &Value::Object(input.clone()))
}

/// `isTodoTool`.
pub fn is_todo_tool(tool_name: &str) -> bool {
    is_task_list_tool_name(tool_name)
}

/// `isClaudeTaskTool`: newer Claude Code builds replace TodoWrite with
/// incremental task tools. TaskCreate adds one item and TaskUpdate changes
/// one item by id.
pub fn is_claude_task_tool(tool_name: &str) -> bool {
    matches!(
        js::trim(tool_name),
        "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet"
    )
}

/// The session's TaskCreate and TaskUpdate items by Claude task id, in
/// creation order, like the TypeScript `Map<string, TaskListItem>`.
pub type ClaudeTaskMap = OrderedMap<TaskListItem>;

static TASK_CREATED_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Task #([^\s:]+)").unwrap());

/// `applyClaudeTaskTool`: fold one successful TaskCreate or TaskUpdate call
/// into the session's task map. Returns true when the visible list changed.
/// TaskCreate only learns its id from the result text ("Task #3 created
/// successfully: ...").
pub fn apply_claude_task_tool(
    tasks: &mut ClaudeTaskMap,
    tool_name: &str,
    input: &Record,
    result_text: &str,
) -> bool {
    let name = js::trim(tool_name);
    if name == "TaskCreate" {
        let text = ["subject", "activeForm", "description"]
            .into_iter()
            .find_map(|key| {
                let value = input.get(key)?.as_str()?;
                let trimmed = js::trim(value);
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            });
        let id = TASK_CREATED_ID
            .captures(result_text)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string());
        let (Some(text), Some(id)) = (text, id) else {
            return false;
        };
        tasks.set(
            id.clone(),
            TaskListItem {
                id: Some(id),
                text,
                ..TaskListItem::default()
            },
        );
        return true;
    }
    if name == "TaskUpdate" {
        let id = match input.get("taskId") {
            Some(Value::Number(n)) => n.as_f64().map(js::number_to_string).unwrap_or_default(),
            Some(Value::String(raw)) => {
                let trimmed = js::trim(raw);
                trimmed.strip_prefix('#').unwrap_or(trimmed).to_string()
            }
            _ => String::new(),
        };
        let Some(current) = (!id.is_empty()).then(|| tasks.get(&id).cloned()).flatten() else {
            return false;
        };
        let status =
            string_field(Some(input), "status").map(|status| js::trim(status).to_lowercase());
        if status.as_deref() == Some("deleted") {
            tasks.delete(&id);
            return true;
        }
        let subject =
            string_field(Some(input), "subject").map(|subject| js::trim(subject).to_string());
        let mut next = current;
        if let Some(subject) = subject.filter(|subject| !subject.is_empty()) {
            next.text = subject;
        }
        if let Some(status) = status.filter(|status| !status.is_empty()) {
            next.status = normalize_task_list_status_str(&status);
        }
        tasks.set(id, next);
        return true;
    }
    false
}

/// `toolKindFromName`.
pub fn tool_kind_from_name(tool_name: &str) -> String {
    let normalized = tool_name.to_lowercase();
    let has = |needle: &str| normalized.contains(needle);
    if is_todo_tool(tool_name) || is_claude_task_tool(tool_name) {
        return "tasks".into();
    }
    if has("bash") || has("command") || has("shell") || has("terminal") {
        return "execute".into();
    }
    if has("edit") || has("write") || has("patch") || has("replace") || has("multiedit") {
        return "edit".into();
    }
    if normalized == "read" || has("read") {
        return "read".into();
    }
    if has("grep") || has("glob") || has("search") || has("websearch") {
        return "search".into();
    }
    if normalized == "skill" || normalized == "skills" {
        return "skill".into();
    }
    if is_agent_tool_name(tool_name) {
        return "agent".into();
    }
    tool_name.to_string()
}

/// `toolTitle`.
pub fn tool_title(name: &str, input: &Record) -> String {
    title_from_tool_input(name, &tool_kind_from_name(name), input)
}

/// `previewFromTool`.
pub fn preview_from_tool(name: &str, input: &Record, output: Option<&str>) -> Option<ToolPreview> {
    let kind = tool_kind_from_name(name);
    let mut tool = Record::new();
    tool.insert("title".into(), Value::String(name.into()));
    tool.insert("name".into(), Value::String(name.into()));
    tool.insert("kind".into(), Value::String(kind));
    tool.insert("rawInput".into(), Value::Object(input.clone()));
    let mut update = tool.clone();
    update.insert("input".into(), Value::Object(input.clone()));
    if let Some(output) = output {
        update.insert("content".into(), Value::String(output.to_string()));
    }
    extract_tool_preview(&update, &tool)
}

/// `summarizeToolRequest`.
// TODO(port): JSON.stringify kept the CLI's key order; serde_json sorts keys
// unless its `preserve_order` feature is on.
pub fn summarize_tool_request(tool_name: &str, input: &Record) -> String {
    let rec = Some(input);
    if let Some(command) = string_field(rec, "command").or_else(|| string_field(rec, "cmd")) {
        return format!("{tool_name}: {}", js::slice_prefix(command, 400));
    }
    if let Some(description) = string_field(rec, "description") {
        return description.to_string();
    }
    let Ok(serialized) = serde_json::to_string(input) else {
        return tool_name.to_string();
    };
    if js::len(&serialized) <= 400 {
        return format!("{tool_name}: {serialized}");
    }
    format!("{tool_name}: {}...", js::slice_prefix(&serialized, 397))
}

/// The input to `claudeSettingsKey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSettingsKeyInput<'a> {
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub fast: Option<&'a str>,
    pub thinking: Option<&'a str>,
    pub context: Option<&'a str>,
    pub runtime_mode: RuntimeMode,
    pub hooks: Option<bool>,
}

/// `claudeSettingsKey`: launch settings that need a fresh process when they change.
pub fn claude_settings_key(input: &ClaudeSettingsKeyInput<'_>) -> String {
    [
        input.model,
        input.effort.unwrap_or(""),
        input.fast.unwrap_or(""),
        input.thinking.unwrap_or(""),
        input.context.unwrap_or(""),
        input.runtime_mode.as_str(),
        if input.hooks == Some(false) {
            "nohooks"
        } else {
            "hooks"
        },
    ]
    .join("|")
}

fn number_field(rec: Option<&Record>, key: &str) -> f64 {
    rec.and_then(|rec| rec.get(key))
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}

/// Tokens occupying the window for one request.
///
/// Cached reads still take up window space, so they count the same as fresh
/// input; output counts because it carries into the next request.
fn context_used_from_usage(usage: Option<&Record>) -> f64 {
    if usage.is_none() {
        return 0.0;
    }
    number_field(usage, "input_tokens")
        + number_field(usage, "cache_creation_input_tokens")
        + number_field(usage, "cache_read_input_tokens")
        + number_field(usage, "output_tokens")
}

fn tokens(value: f64) -> Option<i64> {
    (value != 0.0).then_some(value as i64)
}

/// `turnMetricsFromResult`: aggregate token accounting for the completed turn.
pub fn turn_metrics_from_result(rec: &Record) -> Option<TurnMetrics> {
    let usage = record_field(Some(rec), "usage")?;
    let input_tokens = number_field(Some(usage), "input_tokens");
    let output_tokens = number_field(Some(usage), "output_tokens");
    let cache_read_tokens = number_field(Some(usage), "cache_read_input_tokens");
    let cache_write_tokens = number_field(Some(usage), "cache_creation_input_tokens");
    let cache_reported = usage.contains_key("cache_read_input_tokens")
        || usage.contains_key("cache_creation_input_tokens");
    let cacheable_input = input_tokens + cache_read_tokens + cache_write_tokens;
    if input_tokens == 0.0 && output_tokens == 0.0 && cacheable_input == 0.0 {
        return None;
    }
    Some(TurnMetrics {
        input_tokens: tokens(input_tokens),
        output_tokens: tokens(output_tokens),
        cache_read_tokens: tokens(cache_read_tokens),
        cache_write_tokens: tokens(cache_write_tokens),
        cache_hit_percent: (cache_reported && cacheable_input != 0.0)
            .then(|| (cache_read_tokens / cacheable_input) * 100.0),
        extra: Default::default(),
    })
}

/// `contextUsedFromAssistant`: context level from an `assistant` message.
/// Callers must skip subagent messages, which run their own window and would
/// make the reading jump.
pub fn context_used_from_assistant(rec: &Record) -> Option<i64> {
    let usage = record_field(record_field(Some(rec), "message"), "usage")?;
    let used = context_used_from_usage(Some(usage));
    (used > 0.0).then_some(used as i64)
}

/// Context level and window from a turn `result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaudeContextReading {
    pub used: Option<i64>,
    pub window: Option<i64>,
}

/// `contextFromResult`.
///
/// `usage` at the top level sums every iteration of the turn, so the last entry
/// of `usage.iterations` is what actually sits in the window. `modelUsage`
/// carries the window itself, which is why we let the CLI tell us rather than
/// keeping a model table in sync.
pub fn context_from_result(rec: &Record) -> Option<ClaudeContextReading> {
    let usage = record_field(Some(rec), "usage");
    let last = usage
        .and_then(|usage| usage.get("iterations"))
        .and_then(Value::as_array)
        .and_then(|iterations| iterations.last())
        .and_then(as_record);
    let used = context_used_from_usage(last.or(usage));

    let mut window: Option<f64> = None;
    if let Some(model_usage) = record_field(Some(rec), "modelUsage") {
        for entry in model_usage.values() {
            let context_window = number_field(as_record(entry), "contextWindow");
            if context_window > 0.0 {
                window = Some(window.unwrap_or(0.0).max(context_window));
            }
        }
    }

    if used == 0.0 && window.is_none() {
        return None;
    }
    Some(ClaudeContextReading {
        used: (used > 0.0).then_some(used as i64),
        window: window.map(|window| window as i64),
    })
}
