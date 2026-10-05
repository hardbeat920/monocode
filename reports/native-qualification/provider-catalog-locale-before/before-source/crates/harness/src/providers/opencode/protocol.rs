//! Port of src/integrations/harness/providers/opencode/opencodeProtocol.ts:
//! the pure half of the OpenCode provider. It reads server output, CLI
//! versions, SSE event payloads, and message parts, and builds permission
//! rules and prompt parts.
//!
//! TypeScript passed `Record<string, unknown>` around. Here a record is a
//! `serde_json::Map`, and functions that read one take `Option<&Record>` the
//! way the TypeScript took `Record | null | undefined`.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::sync::LazyLock;

use monocode_core::attachment::{
    Attachment, attachment_path, attachment_path_text, is_vision_image, prompt_text,
};
use monocode_core::block::{ToolPreview, TurnMetrics};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent};
use monocode_core::js;
use monocode_core::task_list::is_task_list_tool_name;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::deps::extract_tool_preview;

/// A JSON object, the `Record<string, unknown>` of the TypeScript.
pub type Record = Map<String, Value>;

/// The oldest OpenCode whose server API this adapter speaks.
pub const MINIMUM_OPENCODE_VERSION: &str = "1.14.19";
pub const OPENCODE_SERVER_READY_PREFIX: &str = "opencode server listening";
/// Agents OpenCode runs for its own bookkeeping. Their text never reaches the
/// transcript.
pub const KNOWN_HIDDEN_AGENTS: [&str; 3] = ["compaction", "summary", "title"];

/// `KNOWN_HIDDEN_AGENTS.has(name)`.
pub fn is_known_hidden_agent(name: &str) -> bool {
    KNOWN_HIDDEN_AGENTS.contains(&name)
}

static OPENCODE_DEFAULT_TITLE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(New session - |Child session - )[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z$",
    )
    .unwrap()
});
static LISTENING_ON_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)on\s+(https?://\S+)").unwrap());
static ANY_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(https?://\S+)").unwrap());
static TRAILING_PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.,;]+$").unwrap());
static SEMVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]+\.[0-9]+\.[0-9]+").unwrap());
static SLUG_SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_/]+").unwrap());

/// `ParsedOpenCodeModelSlug`, in the shape OpenCode's API takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedOpenCodeModelSlug {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

/// `OpenCodePermissionRule["action"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionAction {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

/// `OpenCodePermissionRule`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenCodePermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: PermissionAction,
}

impl OpenCodePermissionRule {
    fn new(permission: &str, pattern: &str, action: PermissionAction) -> Self {
        Self {
            permission: permission.into(),
            pattern: pattern.into(),
            action,
        }
    }
}

/// The reply values `/permission/{id}/reply` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionReply {
    #[serde(rename = "once")]
    Once,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "reject")]
    Reject,
}

impl PermissionReply {
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionReply::Once => "once",
            PermissionReply::Always => "always",
            PermissionReply::Reject => "reject",
        }
    }
}

/// `OpenCodePart["time"]`. JavaScript numbers, kept as `f64` because only
/// their presence matters.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PartTime {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

/// `OpenCodePart`: one message part as the adapter tracks it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenCodePart {
    pub id: String,
    /// `type`: "text", "reasoning", "tool", "file", and others.
    pub part_type: String,
    pub message_id: Option<String>,
    pub call_id: Option<String>,
    pub tool: Option<String>,
    pub text: Option<String>,
    pub time: Option<PartTime>,
    pub state: Option<Record>,
}

impl OpenCodePart {
    fn state(&self) -> Option<&Record> {
        self.state.as_ref()
    }
}

/// `asRecord`: the value as a JSON object, or `None`.
pub fn as_record(value: Option<&Value>) -> Option<&Record> {
    value.and_then(Value::as_object)
}

/// `rec?.[key]`.
pub fn field<'a>(rec: Option<&'a Record>, key: &str) -> Option<&'a Value> {
    rec.and_then(|rec| rec.get(key))
}

/// `asRecord(rec?.[key])`.
pub fn record_field<'a>(rec: Option<&'a Record>, key: &str) -> Option<&'a Record> {
    as_record(field(rec, key))
}

/// JavaScript truthiness for a parsed JSON value.
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `stringField`: a string that is not blank. The value keeps its spaces.
pub fn string_field<'a>(rec: Option<&'a Record>, key: &str) -> Option<&'a str> {
    field(rec, key)
        .and_then(Value::as_str)
        .filter(|value| !js::trim(value).is_empty())
}

/// `parseOpenCodeModelSlug`: `provider/model` into its two halves.
pub fn parse_open_code_model_slug(slug: Option<&str>) -> Option<ParsedOpenCodeModelSlug> {
    let trimmed = js::trim(slug?);
    let separator = trimmed.find('/')?;
    if separator == 0 || separator == trimmed.len() - 1 {
        return None;
    }
    Some(ParsedOpenCodeModelSlug {
        provider_id: trimmed[..separator].to_string(),
        model_id: trimmed[separator + 1..].to_string(),
    })
}

/// `parseServerUrlFromOutput`: the URL `opencode serve` says it listens on.
pub fn parse_server_url_from_output(output: &str) -> Option<String> {
    for line in output.split('\n') {
        let trimmed = js::trim(line);
        if !trimmed.to_lowercase().contains("listening") {
            continue;
        }
        if let Some(found) = LISTENING_ON_URL.captures(trimmed).and_then(|m| m.get(1)) {
            return Some(
                TRAILING_PUNCTUATION
                    .replace(found.as_str(), "")
                    .into_owned(),
            );
        }
        if trimmed.starts_with(OPENCODE_SERVER_READY_PREFIX)
            && let Some(found) = ANY_URL.captures(trimmed).and_then(|m| m.get(1))
        {
            return Some(
                TRAILING_PUNCTUATION
                    .replace(found.as_str(), "")
                    .into_owned(),
            );
        }
    }
    None
}

/// `parseOpenCodeVersion`: the first `x.y.z` in `opencode --version` output.
pub fn parse_open_code_version(output: &str) -> Option<String> {
    SEMVER.find(output).map(|found| found.as_str().to_string())
}

/// `Number.parseInt(part, 10)`. `None` where JavaScript yields `NaN`.
fn parse_int(part: &str) -> Option<i64> {
    let text = part.trim_start_matches(js::is_space);
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|value| sign * value)
}

/// `compareSemver`: negative, zero, or positive, over the first three parts.
pub fn compare_semver(left: &str, right: &str) -> i64 {
    let parts = |value: &str| -> Vec<i64> {
        value
            .split('.')
            .map(|part| parse_int(part).unwrap_or(0))
            .collect()
    };
    let a = parts(left);
    let b = parts(right);
    for index in 0..3 {
        let delta = a.get(index).copied().unwrap_or(0) - b.get(index).copied().unwrap_or(0);
        if delta != 0 {
            return delta;
        }
    }
    0
}

/// `isOpenCodeDefaultTitle`: the placeholder title OpenCode gives a session.
pub fn is_open_code_default_title(title: &str) -> bool {
    OPENCODE_DEFAULT_TITLE_PATTERN.is_match(title)
}

/// `isOpenCodeNotFound`: a 404 status, or a `NotFoundError` with no status,
/// anywhere in the error's `cause`, `body`, `error`, or `data` chain.
pub fn is_open_code_not_found(cause: &Value) -> bool {
    let mut queue: VecDeque<&Value> = VecDeque::from([cause]);
    let mut steps = 0;
    while steps < 32 {
        let Some(node) = queue.pop_front() else {
            break;
        };
        steps += 1;
        let Some(record) = node.as_object() else {
            continue;
        };
        let response = record.get("response").and_then(Value::as_object);
        let statuses: Vec<f64> = [
            record.get("status"),
            record.get("statusCode"),
            response.and_then(|response| response.get("status")),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
        if statuses.contains(&404.0) {
            return true;
        }
        if !statuses.is_empty() {
            continue;
        }
        if record
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name.to_lowercase() == "notfounderror")
        {
            return true;
        }
        for key in ["cause", "body", "error", "data"] {
            if let Some(next) = record.get(key) {
                queue.push_back(next);
            }
        }
    }
    false
}

/// `buildOpenCodePermissionRules`: the session permission rules for an
/// access mode. Questions are always allowed so the agent can ask.
pub fn build_open_code_permission_rules(runtime_mode: RuntimeMode) -> Vec<OpenCodePermissionRule> {
    use PermissionAction::*;
    if runtime_mode == RuntimeMode::FullAccess {
        return vec![OpenCodePermissionRule::new("*", "*", Allow)];
    }
    let mut rules = vec![
        OpenCodePermissionRule::new("*", "*", Ask),
        OpenCodePermissionRule::new("question", "*", Allow),
    ];
    if matches!(
        runtime_mode,
        RuntimeMode::AutoAcceptEdits | RuntimeMode::Auto
    ) {
        rules.push(OpenCodePermissionRule::new("edit", "*", Allow));
    }
    if runtime_mode == RuntimeMode::Auto {
        rules.push(OpenCodePermissionRule::new("read", "*", Allow));
    }
    rules
}

/// `toOpenCodePermissionReply`.
pub fn to_open_code_permission_reply(decision: ApprovalDecision) -> PermissionReply {
    match decision {
        ApprovalDecision::Allow => PermissionReply::Once,
        ApprovalDecision::Deny => PermissionReply::Reject,
    }
}

/// `toFileUrl`: a `file://` URL with each path segment percent-encoded.
pub fn to_file_url(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let abs = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    };
    let encoded: Vec<String> = abs.split('/').map(js::encode_uri_component).collect();
    format!("file://{}", encoded.join("/"))
}

/// `OpenCodePromptPart`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OpenCodePromptPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "file")]
    File {
        mime: String,
        filename: String,
        url: String,
    },
}

/// `toOpenCodePromptParts`.
///
/// OpenCode forwards native file parts to the selected model provider. Keep
/// those parts to formats its provider adapters consistently support; local
/// files of every other type remain available to the agent through their path.
///
/// The error is the TypeScript's thrown message for an attachment with no
/// local path.
pub fn to_open_code_prompt_parts(
    text: &str,
    attachments: &[Attachment],
) -> Result<Vec<OpenCodePromptPart>, String> {
    let body = prompt_text(text, attachments);
    let mut text_parts: Vec<String> = Vec::new();
    if !body.is_empty() {
        text_parts.push(body);
    }
    let mut parts: Vec<OpenCodePromptPart> = Vec::new();
    for attachment in attachments {
        let mime = js::trim(&attachment.mime_type).to_lowercase();
        if !mime.starts_with("text/") && !is_vision_image(&mime) {
            text_parts.push(attachment_path_text(attachment)?);
            continue;
        }
        let has_path = attachment
            .path
            .as_deref()
            .is_some_and(|path| !path.is_empty());
        let url = match attachment.data.as_deref().filter(|data| !data.is_empty()) {
            Some(data) if !has_path => format!("data:{};base64,{data}", attachment.mime_type),
            _ => to_file_url(attachment_path(attachment)?),
        };
        parts.push(OpenCodePromptPart::File {
            mime: attachment.mime_type.clone(),
            filename: attachment.name.clone(),
            url,
        });
    }
    let mut out = Vec::with_capacity(parts.len() + 1);
    if !text_parts.is_empty() {
        out.push(OpenCodePromptPart::Text {
            text: text_parts.join("\n\n"),
        });
    }
    out.extend(parts);
    Ok(out)
}

/// What `mergeOpenCodeAssistantText` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedText {
    pub latest_text: String,
    pub delta_to_emit: String,
}

/// `mergeOpenCodeAssistantText`: fold a part snapshot into the text already
/// shown. A shorter snapshot that the shown text starts with is stale and
/// keeps the longer text.
pub fn merge_open_code_assistant_text(previous_text: Option<&str>, next_text: &str) -> MergedText {
    let latest_text = match previous_text {
        Some(previous)
            if !previous.is_empty()
                && previous.len() > next_text.len()
                && previous.starts_with(next_text) =>
        {
            previous.to_string()
        }
        _ => next_text.to_string(),
    };
    let prefix = common_prefix_length(previous_text.unwrap_or_default(), &latest_text);
    MergedText {
        delta_to_emit: latest_text[prefix..].to_string(),
        latest_text,
    }
}

/// What `appendOpenCodeAssistantTextDelta` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendedText {
    pub next_text: String,
    pub delta_to_emit: String,
}

/// `appendOpenCodeAssistantTextDelta`.
pub fn append_open_code_assistant_text_delta(previous_text: &str, delta: &str) -> AppendedText {
    AppendedText {
        next_text: format!("{previous_text}{delta}"),
        delta_to_emit: delta.to_string(),
    }
}

/// Byte length of the shared prefix, measured in whole characters.
// TODO(port): the TypeScript compares UTF-16 code units, so two strings that
// differ only in the low half of a surrogate pair split the pair. A Rust
// string cannot hold half a pair, so this stops before the whole character.
fn common_prefix_length(left: &str, right: &str) -> usize {
    left.char_indices()
        .zip(right.chars())
        .find(|((_, a), b)| a != b)
        .map(|((index, _), _)| index)
        .unwrap_or_else(|| left.len().min(right.len()))
}

/// `titleCaseSlug`: `acme-cloud` becomes `Acme Cloud`.
pub fn title_case_slug(value: &str) -> String {
    SLUG_SEPARATORS
        .split(value)
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `inferDefaultVariant`.
pub fn infer_default_variant(provider_id: &str, variants: &[String]) -> Option<String> {
    if variants.len() == 1 {
        return Some(variants[0].clone());
    }
    let has = |value: &str| variants.iter().any(|variant| variant == value);
    if provider_id == "anthropic" || provider_id.starts_with("google") {
        return has("high").then(|| "high".to_string());
    }
    // Variants are reasoning levels on every provider (e.g. minimal/low/medium
    // /high/xhigh), so prefer medium, then high, regardless of provider.
    if has("medium") {
        return Some("medium".into());
    }
    if has("high") {
        return Some("high".into());
    }
    None
}

const VARIANT_LABELS: [(&str, &str); 9] = [
    ("none", "None"),
    ("minimal", "Minimal"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra High"),
    ("extra-high", "Extra High"),
    ("max", "Max"),
    ("ultra", "Ultra"),
];

const VARIANT_ORDER: [&str; 9] = [
    "none",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "extra-high",
    "max",
    "ultra",
];

fn variant_label(value: &str) -> Option<&'static str> {
    VARIANT_LABELS
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, label)| *label)
}

/// `openCodeVariantLabel`: human label for an OpenCode variant value,
/// matching Codex and Cursor effort labels.
pub fn open_code_variant_label(value: &str) -> String {
    variant_label(value)
        .or_else(|| variant_label(&value.to_lowercase()))
        .map(str::to_string)
        .unwrap_or_else(|| title_case_slug(value))
}

/// Approximation of `String.prototype.localeCompare`: case-insensitive
/// first, then lowercase before uppercase.
// TODO(port): localeCompare uses ICU collation, which also orders
// punctuation before digits. This matches it for ASCII names.
pub(crate) fn locale_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// `sortOpenCodeVariants`: lowest to highest effort; unknown values sort last.
pub fn sort_open_code_variants(values: &[String]) -> Vec<String> {
    let rank = |value: &str| -> usize {
        let lower = value.to_lowercase();
        VARIANT_ORDER
            .iter()
            .position(|known| *known == lower)
            .unwrap_or(usize::MAX)
    };
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| {
        rank(left)
            .cmp(&rank(right))
            .then_with(|| locale_compare(left, right))
    });
    sorted
}

/// `inferDefaultAgent`: `build` when present, else the first agent.
pub fn infer_default_agent<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let names: Vec<&str> = names.into_iter().collect();
    names
        .iter()
        .find(|name| **name == "build")
        .or_else(|| names.first())
        .map(|name| name.to_string())
}

/// `toolKindFromName`: OpenCode tool names to the kinds the transcript uses.
pub fn tool_kind_from_name(tool_name: &str) -> String {
    let normalized = tool_name.to_lowercase();
    if is_task_list_tool_name(tool_name) {
        return "tasks".into();
    }
    if normalized.contains("bash") || normalized.contains("command") || normalized.contains("shell")
    {
        return "shell".into();
    }
    if normalized.contains("edit")
        || normalized.contains("write")
        || normalized.contains("patch")
        || normalized.contains("multiedit")
    {
        return "edit".into();
    }
    if normalized.contains("read") {
        return "read".into();
    }
    if normalized.contains("grep")
        || normalized.contains("glob")
        || normalized.contains("search")
        || normalized.contains("find")
    {
        return "search".into();
    }
    if normalized == "skill" || normalized == "skills" {
        return "skill".into();
    }
    if normalized == "agent" || normalized == "task" || normalized == "subagent" {
        return "agent".into();
    }
    tool_name.to_string()
}

/// `previewFromToolPart`.
pub fn preview_from_tool_part(part: &OpenCodePart) -> Option<ToolPreview> {
    let tool = part.tool.as_deref().unwrap_or("tool");
    let state = part.state();
    let kind = tool_kind_from_name(tool);
    let input = field(state, "input");
    let title = field(state, "title")
        .and_then(Value::as_str)
        .unwrap_or(tool);
    let content = field(state, "output")
        .filter(|value| !value.is_null())
        .or_else(|| field(state, "metadata"));

    let mut update = Record::new();
    update.insert("title".into(), title.into());
    update.insert("name".into(), tool.into());
    update.insert("kind".into(), kind.clone().into());
    if let Some(input) = input {
        update.insert("input".into(), input.clone());
        update.insert("rawInput".into(), input.clone());
    }
    if let Some(content) = content {
        update.insert("content".into(), content.clone());
    }

    let mut tool_rec = Record::new();
    tool_rec.insert("title".into(), tool.into());
    tool_rec.insert("name".into(), tool.into());
    tool_rec.insert("kind".into(), kind.into());
    if let Some(input) = input {
        tool_rec.insert("rawInput".into(), input.clone());
    }
    extract_tool_preview(&update, &tool_rec)
}

/// `detailFromToolPart`: the output, error, or running title worth showing.
pub fn detail_from_tool_part(part: &OpenCodePart) -> Option<String> {
    let state = part.state();
    let status = field(state, "status").and_then(Value::as_str).unwrap_or("");
    if status == "completed"
        && let Some(output) = field(state, "output").and_then(Value::as_str)
    {
        return Some(output.to_string());
    }
    if status == "error" {
        if let Some(error) = field(state, "error").and_then(Value::as_str) {
            return Some(error.to_string());
        }
        let error = record_field(state, "error");
        let data = record_field(error, "data");
        return string_field(data, "message")
            .or_else(|| string_field(error, "message"))
            .or_else(|| string_field(record_field(error, "error"), "message"))
            .map(str::to_string);
    }
    if status == "running"
        && let Some(title) = field(state, "title").and_then(Value::as_str)
    {
        return Some(title.to_string());
    }
    None
}

/// `permissionTitle`: the approval row title for a permission request.
pub fn permission_title(permission: &str, patterns: &[String]) -> String {
    let detail = if patterns.is_empty() {
        permission.to_string()
    } else {
        patterns.join("\n")
    };
    match permission {
        "bash" if !detail.is_empty() => format!("Run {detail}"),
        "bash" => "Run command".into(),
        "edit" if !detail.is_empty() => format!("Edit {detail}"),
        "edit" => "Edit file".into(),
        "read" if !detail.is_empty() => format!("Read {detail}"),
        "read" => "Read file".into(),
        _ if !detail.is_empty() => detail,
        _ => permission.to_string(),
    }
}

/// `sessionErrorMessage`: the message inside a `session.error` payload.
pub fn session_error_message(error: Option<&Value>) -> String {
    let Some(Value::Object(rec)) = error else {
        return "OpenCode session failed.".into();
    };
    let rec = Some(rec);
    string_field(record_field(rec, "data"), "message")
        .or_else(|| string_field(rec, "message"))
        .or_else(|| string_field(record_field(rec, "error"), "message"))
        .unwrap_or("OpenCode session failed.")
        .to_string()
}

/// `num(rec, key)`: a finite number, else 0.
fn num(rec: Option<&Record>, key: &str) -> f64 {
    field(rec, key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// `contextUsedFromMessageInfo`: context level from an OpenCode assistant
/// message `info`.
///
/// Cached reads still occupy the window, so they count alongside fresh input;
/// output counts because it carries into the next request.
pub fn context_used_from_message_info(info: Option<&Record>) -> Option<i64> {
    let tokens = record_field(info, "tokens")?;
    let tokens = Some(tokens);
    let cache = record_field(tokens, "cache");
    let used = num(tokens, "input")
        + num(tokens, "output")
        + num(tokens, "reasoning")
        + num(cache, "read")
        + num(cache, "write");
    (used > 0.0).then_some(used as i64)
}

/// `turnMetricsFromMessageInfo`.
pub fn turn_metrics_from_message_info(info: Option<&Record>) -> Option<TurnMetrics> {
    let tokens = record_field(info, "tokens")?;
    let tokens = Some(tokens);
    let cache = record_field(tokens, "cache");
    let input_tokens = num(tokens, "input");
    let output_tokens = num(tokens, "output") + num(tokens, "reasoning");
    let cache_read_tokens = num(cache, "read");
    let cache_write_tokens = num(cache, "write");
    let cache_reported = cache.is_some();
    let cacheable_input = input_tokens + cache_read_tokens + cache_write_tokens;
    if input_tokens == 0.0 && output_tokens == 0.0 && cacheable_input == 0.0 {
        return None;
    }
    let count = |value: f64| (value != 0.0).then_some(value as i64);
    Some(TurnMetrics {
        input_tokens: count(input_tokens),
        output_tokens: count(output_tokens),
        cache_read_tokens: count(cache_read_tokens),
        cache_write_tokens: count(cache_write_tokens),
        cache_hit_percent: (cache_reported && cacheable_input != 0.0)
            .then(|| (cache_read_tokens / cacheable_input) * 100.0),
        extra: Default::default(),
    })
}

/// `openCodeChildSessionId`: the session a `task` tool spawned, when
/// OpenCode names it on the call. A subagent runs as its own session, so this
/// is what ties the child's stream back to the row that started it.
pub fn open_code_child_session_id(part: &OpenCodePart) -> Option<String> {
    let state = part.state();
    let metadata = record_field(state, "metadata");
    let input = record_field(state, "input");
    for source in [metadata, state, input] {
        let id = [
            "sessionID",
            "sessionId",
            "session_id",
            "childSessionID",
            "subSessionID",
        ]
        .into_iter()
        .find_map(|key| string_field(source, key));
        if let Some(id) = id {
            return Some(id.to_string());
        }
    }
    None
}

/// `eventSessionId`: the session an SSE event belongs to.
pub fn event_session_id(event: &Record) -> Option<String> {
    let properties = record_field(Some(event), "properties")?;
    let properties = Some(properties);
    if let Some(session_id) = string_field(properties, "sessionID") {
        return Some(session_id.to_string());
    }
    let info = record_field(properties, "info");
    string_field(info, "sessionID")
        .or_else(|| string_field(record_field(properties, "part"), "sessionID"))
        .or_else(|| {
            let is_session_event = event
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.starts_with("session."));
            if is_session_event {
                string_field(info, "id")
            } else {
                None
            }
        })
        .map(str::to_string)
}

/// `textDeltaEvent`.
pub fn text_delta_event(part: &OpenCodePart, text: &str) -> Option<HarnessEvent> {
    if text.is_empty() {
        return None;
    }
    if part.part_type == "reasoning" {
        return Some(HarnessEvent::ReasoningDelta { text: text.into() });
    }
    Some(HarnessEvent::MessageDelta { text: text.into() })
}

/// `Map<string, OpenCodePart>`: parts by id, iterated in insertion order. A
/// part set again keeps its first position, as in a JavaScript `Map`.
#[derive(Debug, Clone, Default)]
pub struct PartStore {
    index: std::collections::HashMap<String, usize>,
    parts: Vec<OpenCodePart>,
}

impl PartStore {
    pub fn get(&self, id: &str) -> Option<&OpenCodePart> {
        self.index.get(id).map(|index| &self.parts[*index])
    }

    pub fn set(&mut self, part: OpenCodePart) {
        match self.index.get(&part.id) {
            Some(index) => self.parts[*index] = part,
            None => {
                self.index.insert(part.id.clone(), self.parts.len());
                self.parts.push(part);
            }
        }
    }

    /// Parts of one message, in insertion order.
    pub fn of_message(&self, message_id: &str) -> Vec<OpenCodePart> {
        self.parts
            .iter()
            .filter(|part| part.message_id.as_deref() == Some(message_id))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::attachment::{ATTACHMENT_ONLY_PROMPT, AttachmentKind};
    use serde_json::json;

    fn rec(value: Value) -> Record {
        value.as_object().cloned().unwrap()
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    // describe("eventSessionId")

    #[test]
    fn event_session_id_extracts_the_owning_session_for_each_type() {
        for event in [
            json!({ "type": "permission.asked", "properties": { "id": "permission_1", "sessionID": "session_1" } }),
            json!({ "type": "session.created", "properties": { "info": { "id": "session_1", "parentID": "session_parent" } } }),
            json!({ "type": "message.updated", "properties": { "info": { "id": "message_1", "sessionID": "session_1" } } }),
            json!({ "type": "message.part.updated", "properties": { "part": { "id": "part_1", "sessionID": "session_1" } } }),
            json!({ "type": "message.part.delta", "properties": { "sessionID": "session_1", "partID": "part_1" } }),
        ] {
            assert_eq!(
                event_session_id(&rec(event.clone())).as_deref(),
                Some("session_1"),
                "{event}"
            );
        }
    }

    #[test]
    fn event_session_id_does_not_mistake_message_ids_for_session_ids() {
        assert_eq!(
            event_session_id(&rec(json!({
                "type": "message.updated",
                "properties": { "info": { "id": "message_1" } },
            }))),
            None
        );
    }

    // describe("parseOpenCodeModelSlug")

    #[test]
    fn parse_model_slug_splits_provider_and_model() {
        assert_eq!(
            parse_open_code_model_slug(Some("anthropic/claude-sonnet-4-6")),
            Some(ParsedOpenCodeModelSlug {
                provider_id: "anthropic".into(),
                model_id: "claude-sonnet-4-6".into(),
            })
        );
        assert_eq!(
            serde_json::to_value(parse_open_code_model_slug(Some("a/b/c")).unwrap()).unwrap(),
            json!({ "providerID": "a", "modelID": "b/c" })
        );
    }

    #[test]
    fn parse_model_slug_rejects_bare_ids() {
        assert_eq!(parse_open_code_model_slug(Some("glm-5")), None);
        assert_eq!(parse_open_code_model_slug(Some("/model")), None);
        assert_eq!(parse_open_code_model_slug(Some("provider/")), None);
        assert_eq!(parse_open_code_model_slug(None), None);
    }

    // describe("tool kinds")

    #[test]
    fn classifies_todo_writes_as_internal_task_activity() {
        assert_eq!(tool_kind_from_name("todowrite"), "tasks");
    }

    #[test]
    fn classifies_common_tool_names() {
        assert_eq!(tool_kind_from_name("bash"), "shell");
        assert_eq!(tool_kind_from_name("multiedit"), "edit");
        assert_eq!(tool_kind_from_name("read"), "read");
        assert_eq!(tool_kind_from_name("glob"), "search");
        assert_eq!(tool_kind_from_name("skill"), "skill");
        assert_eq!(tool_kind_from_name("task"), "agent");
        assert_eq!(
            tool_kind_from_name("external_directory"),
            "external_directory"
        );
    }

    // describe("tool failure details")

    #[test]
    fn extracts_nested_provider_errors_instead_of_dropping_them() {
        let part = OpenCodePart {
            id: "agent-1".into(),
            part_type: "tool".into(),
            tool: Some("task".into()),
            state: Some(rec(json!({
                "status": "error",
                "error": { "data": { "message": "worker disconnected" } },
            }))),
            ..Default::default()
        };
        assert_eq!(
            detail_from_tool_part(&part).as_deref(),
            Some("worker disconnected")
        );
    }

    // describe("parseServerUrlFromOutput")

    #[test]
    fn reads_the_listening_url_from_server_output() {
        assert_eq!(
            parse_server_url_from_output("opencode server listening on http://127.0.0.1:4096")
                .as_deref(),
            Some("http://127.0.0.1:4096")
        );
    }

    #[test]
    fn reads_the_listening_url_that_opencode_2_prints() {
        assert_eq!(
            parse_server_url_from_output(
                "server listening on http://127.0.0.1:47123\nserver password abc"
            )
            .as_deref(),
            Some("http://127.0.0.1:47123")
        );
        assert_eq!(parse_server_url_from_output("starting up"), None);
    }

    // describe("parseOpenCodeVersion / compareSemver")

    #[test]
    fn extracts_a_semver_and_gates_1_14_19() {
        assert_eq!(
            parse_open_code_version("1.14.19").as_deref(),
            Some("1.14.19")
        );
        assert_eq!(
            parse_open_code_version("opencode 1.15.0").as_deref(),
            Some("1.15.0")
        );
        assert_eq!(
            parse_open_code_version("opencode v2.0.20").as_deref(),
            Some("2.0.20")
        );
        assert!(compare_semver("1.14.18", "1.14.19") < 0);
        assert_eq!(compare_semver("1.14.19", "1.14.19"), 0);
        assert!(compare_semver("1.15.0", "1.14.19") > 0);
        assert!(compare_semver("1.14", "1.14.0") == 0);
        assert!(compare_semver("1.x.2", "1.0.1") > 0);
    }

    // describe("buildOpenCodePermissionRules")

    #[test]
    fn allows_everything_in_full_access() {
        assert_eq!(
            serde_json::to_value(build_open_code_permission_rules(RuntimeMode::FullAccess))
                .unwrap(),
            json!([{ "permission": "*", "pattern": "*", "action": "allow" }])
        );
    }

    #[test]
    fn asks_by_default_and_allows_edits_in_auto_accept_edits() {
        let rules = build_open_code_permission_rules(RuntimeMode::AutoAcceptEdits);
        assert!(rules.contains(&OpenCodePermissionRule::new(
            "edit",
            "*",
            PermissionAction::Allow
        )));
        assert_eq!(
            rules[0],
            OpenCodePermissionRule::new("*", "*", PermissionAction::Ask)
        );
        let auto = build_open_code_permission_rules(RuntimeMode::Auto);
        assert!(auto.contains(&OpenCodePermissionRule::new(
            "read",
            "*",
            PermissionAction::Allow
        )));
    }

    #[test]
    fn maps_allow_and_deny_onto_opencode_reply_values() {
        assert_eq!(
            to_open_code_permission_reply(ApprovalDecision::Allow).as_str(),
            "once"
        );
        assert_eq!(
            to_open_code_permission_reply(ApprovalDecision::Deny).as_str(),
            "reject"
        );
    }

    // describe("mergeOpenCodeAssistantText")

    #[test]
    fn merge_emits_only_the_new_suffix() {
        assert_eq!(
            merge_open_code_assistant_text(Some("Hel"), "Hello"),
            MergedText {
                latest_text: "Hello".into(),
                delta_to_emit: "lo".into(),
            }
        );
    }

    #[test]
    fn merge_keeps_a_longer_snapshot_if_the_next_update_shrinks() {
        assert_eq!(
            merge_open_code_assistant_text(Some("Hello world"), "Hello"),
            MergedText {
                latest_text: "Hello world".into(),
                delta_to_emit: "".into(),
            }
        );
    }

    #[test]
    fn merge_slices_at_character_boundaries() {
        assert_eq!(
            merge_open_code_assistant_text(Some("caf"), "café!").delta_to_emit,
            "é!"
        );
        assert_eq!(
            merge_open_code_assistant_text(None, "naïve").delta_to_emit,
            "naïve"
        );
    }

    // describe("OpenCode helpers")

    #[test]
    fn ignores_opencode_placeholder_titles() {
        assert!(is_open_code_default_title(
            "New session - 2026-08-16T07:24:01.000Z"
        ));
        assert!(!is_open_code_default_title("Fix login timeout"));
    }

    #[test]
    fn detects_404_and_not_found_error() {
        assert!(is_open_code_not_found(&json!({ "status": 404 })));
        assert!(is_open_code_not_found(&json!({ "name": "NotFoundError" })));
        assert!(!is_open_code_not_found(
            &json!({ "status": 500, "name": "NotFoundError" })
        ));
        assert!(is_open_code_not_found(
            &json!({ "cause": { "response": { "status": 404 } } })
        ));
    }

    #[test]
    fn infers_default_variant_and_agent() {
        assert_eq!(
            infer_default_variant("anthropic", &strings(&["low", "high"])).as_deref(),
            Some("high")
        );
        assert_eq!(
            infer_default_variant("openai", &strings(&["low", "medium", "high"])).as_deref(),
            Some("medium")
        );
        assert_eq!(
            infer_default_agent(["plan", "build"]).as_deref(),
            Some("build")
        );
    }

    #[test]
    fn prefers_medium_then_high_variants_on_any_provider() {
        assert_eq!(
            infer_default_variant("some-cloud", &strings(&["low", "medium", "high"])).as_deref(),
            Some("medium")
        );
        assert_eq!(
            infer_default_variant("some-cloud", &strings(&["low", "high"])).as_deref(),
            Some("high")
        );
        assert_eq!(
            infer_default_variant("some-cloud", &strings(&["low", "xhigh"])),
            None
        );
    }

    #[test]
    fn labels_variants_like_codex_and_cursor_effort_levels() {
        assert_eq!(open_code_variant_label("xhigh"), "Extra High");
        assert_eq!(open_code_variant_label("extra-high"), "Extra High");
        assert_eq!(open_code_variant_label("minimal"), "Minimal");
        assert_eq!(open_code_variant_label("high"), "High");
        assert_eq!(open_code_variant_label("HIGH"), "High");
        assert_eq!(open_code_variant_label("turbo_mode"), "Turbo Mode");
    }

    #[test]
    fn sorts_variants_from_lowest_to_highest_effort() {
        assert_eq!(
            sort_open_code_variants(&strings(&["high", "minimal", "xhigh", "low", "medium"])),
            strings(&["minimal", "low", "medium", "high", "xhigh"])
        );
    }

    #[test]
    fn catalog_locale_preserves_effort_rank_and_collates_unknown_variants() {
        assert_eq!(
            sort_open_code_variants(&strings(&[
                "Zebra",
                "éclair",
                "e\u{301}clair",
                "xhigh",
                "high"
            ])),
            strings(&["high", "xhigh", "éclair", "e\u{301}clair", "Zebra"])
        );
    }

    #[test]
    fn catalog_locale_keeps_equivalent_model_names_in_provider_order() {
        use super::super::catalog::{flatten_open_code_models, parse_models_cli_output};

        let output = [
            "anthropic/composed",
            r#"{"id":"composed","name":"éclair"}"#,
            "anthropic/decomposed",
            r#"{"id":"decomposed","name":"e\u0301clair"}"#,
            "anthropic/zebra",
            r#"{"id":"zebra","name":"Zebra"}"#,
        ]
        .join("\n");
        let models = flatten_open_code_models(&parse_models_cli_output(&output), &[]);
        assert_eq!(
            models
                .iter()
                .map(|model| model.native_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            [
                "anthropic/composed",
                "anthropic/decomposed",
                "anthropic/zebra"
            ]
        );
    }

    // describe("contextUsedFromMessageInfo")

    #[test]
    fn counts_cache_reads_and_writes_alongside_input_and_output() {
        let info = rec(json!({
            "role": "assistant",
            "modelID": "big-pickle",
            "providerID": "opencode",
            "tokens": {
                "input": 1_200,
                "output": 800,
                "reasoning": 200,
                "cache": { "read": 40_000, "write": 5_000 },
            },
        }));
        assert_eq!(context_used_from_message_info(Some(&info)), Some(47_200));
    }

    #[test]
    fn ignores_a_message_that_carries_no_token_block() {
        assert_eq!(
            context_used_from_message_info(Some(&rec(json!({ "role": "assistant" })))),
            None
        );
        assert_eq!(context_used_from_message_info(None), None);
    }

    #[test]
    fn treats_an_all_zero_reading_as_nothing_to_report() {
        let info = rec(json!({
            "tokens": {
                "input": 0,
                "output": 0,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 },
            },
        }));
        assert_eq!(context_used_from_message_info(Some(&info)), None);
        assert_eq!(turn_metrics_from_message_info(Some(&info)), None);
    }

    #[test]
    fn normalizes_cache_usage_for_a_turn_tooltip() {
        let info = rec(json!({
            "tokens": {
                "input": 1_200,
                "output": 800,
                "reasoning": 200,
                "cache": { "read": 40_000, "write": 5_000 },
            },
        }));
        assert_eq!(
            serde_json::to_value(turn_metrics_from_message_info(Some(&info)).unwrap()).unwrap(),
            json!({
                "inputTokens": 1_200,
                "outputTokens": 1_000,
                "cacheReadTokens": 40_000,
                "cacheWriteTokens": 5_000,
                "cacheHitPercent": (40_000.0 / 46_200.0) * 100.0,
            })
        );
    }

    // Other helpers the adapter leans on.

    #[test]
    fn titles_permission_requests() {
        assert_eq!(
            permission_title("bash", &strings(&["ls -la"])),
            "Run ls -la"
        );
        assert_eq!(permission_title("edit", &[]), "Edit edit");
        assert_eq!(
            permission_title("external_directory", &strings(&["/home/user/*"])),
            "/home/user/*"
        );
        assert_eq!(permission_title("", &[]), "");
    }

    #[test]
    fn reads_session_error_messages() {
        assert_eq!(
            session_error_message(Some(&json!({ "data": { "message": "Rate limited" } }))),
            "Rate limited"
        );
        assert_eq!(
            session_error_message(Some(&json!({ "error": { "message": "Boom" } }))),
            "Boom"
        );
        assert_eq!(
            session_error_message(Some(&json!("oops"))),
            "OpenCode session failed."
        );
        assert_eq!(session_error_message(None), "OpenCode session failed.");
    }

    #[test]
    fn names_the_child_session_a_task_spawned() {
        let part = OpenCodePart {
            id: "p".into(),
            part_type: "tool".into(),
            state: Some(rec(json!({
                "input": { "sessionID": "from_input" },
                "metadata": { "sessionId": "from_metadata" },
            }))),
            ..Default::default()
        };
        assert_eq!(
            open_code_child_session_id(&part).as_deref(),
            Some("from_metadata")
        );
    }

    #[test]
    fn maps_text_and_reasoning_deltas() {
        let mut part = OpenCodePart {
            id: "p".into(),
            part_type: "text".into(),
            ..Default::default()
        };
        assert_eq!(text_delta_event(&part, ""), None);
        assert_eq!(
            text_delta_event(&part, "hi"),
            Some(HarnessEvent::MessageDelta { text: "hi".into() })
        );
        part.part_type = "reasoning".into();
        assert_eq!(
            text_delta_event(&part, "hm"),
            Some(HarnessEvent::ReasoningDelta { text: "hm".into() })
        );
    }

    #[test]
    fn encodes_file_urls() {
        assert_eq!(to_file_url("/tmp/a b/c.md"), "file:///tmp/a%20b/c.md");
        assert_eq!(to_file_url("C:\\x\\y.md"), "file:///C%3A/x/y.md");
    }

    // fileAttachments.test.ts, the OpenCode cases.

    fn document() -> Attachment {
        Attachment {
            id: "document".into(),
            name: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            kind: AttachmentKind::File,
            size: 100,
            path: Some("/tmp/report.pdf".into()),
            ..Default::default()
        }
    }

    fn image() -> Attachment {
        Attachment {
            id: "image".into(),
            name: "screenshot.png".into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 3,
            data: Some("YWJj".into()),
            path: Some("/tmp/screenshot.png".into()),
            ..Default::default()
        }
    }

    fn folder() -> Attachment {
        Attachment {
            id: "folder".into(),
            name: "reports".into(),
            mime_type: "inode/directory".into(),
            kind: AttachmentKind::File,
            size: 4096,
            path: Some("/tmp/reports".into()),
            ..Default::default()
        }
    }

    #[test]
    fn gives_a_folder_to_opencode_as_a_path_it_can_read() {
        // OpenCode joins the prompt and the path into one text part.
        let parts = to_open_code_prompt_parts("look", &[folder()]).unwrap();
        assert_eq!(parts.len(), 1);
        match &parts[0] {
            OpenCodePromptPart::Text { text } => assert!(text.contains("Attached folder")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn keeps_opencode_native_file_parts_for_supported_text_and_images() {
        let text = Attachment {
            name: "notes.md".into(),
            mime_type: "text/markdown".into(),
            path: Some("/tmp/notes.md".into()),
            ..document()
        };
        let pasted = Attachment {
            path: None,
            ..image()
        };
        assert_eq!(
            serde_json::to_value(to_open_code_prompt_parts("", &[text, pasted]).unwrap()).unwrap(),
            json!([
                { "type": "text", "text": ATTACHMENT_ONLY_PROMPT },
                {
                    "type": "file",
                    "mime": "text/markdown",
                    "filename": "notes.md",
                    "url": "file:///tmp/notes.md",
                },
                {
                    "type": "file",
                    "mime": "image/png",
                    "filename": "screenshot.png",
                    "url": "data:image/png;base64,YWJj",
                },
            ])
        );
    }

    #[test]
    fn gives_opencode_unsupported_and_provider_dependent_files_as_local_paths() {
        let plist = Attachment {
            name: "Info.plist".into(),
            mime_type: "application/octet-stream".into(),
            path: Some("/tmp/Info.plist".into()),
            ..document()
        };
        assert_eq!(
            to_open_code_prompt_parts("Inspect these", &[plist, document()]).unwrap(),
            vec![OpenCodePromptPart::Text {
                text: [
                    "Inspect these",
                    "Attached file (read from disk): \"/tmp/Info.plist\"",
                    "Attached file (read from disk): \"/tmp/report.pdf\"",
                ]
                .join("\n\n"),
            }]
        );
    }

    #[test]
    fn joins_the_attachment_only_stand_in_and_the_path_into_one_text_part() {
        let parts = to_open_code_prompt_parts("", &[document()]).unwrap();
        match &parts[0] {
            OpenCodePromptPart::Text { text } => assert!(text.contains(ATTACHMENT_ONLY_PROMPT)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_bare_turn_has_no_parts_and_a_pathless_file_is_an_error() {
        assert_eq!(to_open_code_prompt_parts("   ", &[]).unwrap(), vec![]);
        let pathless = Attachment {
            path: None,
            ..document()
        };
        assert!(
            to_open_code_prompt_parts("x", &[pathless])
                .unwrap_err()
                .contains("no local file path")
        );
    }
}
