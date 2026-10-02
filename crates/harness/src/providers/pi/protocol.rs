//! Port of src/integrations/harness/providers/pi/piProtocol.ts: the pure
//! frame parsers and command builders for `pi --mode rpc` and `omp --mode rpc`.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::LazyLock;

use monocode_core::attachment::{Attachment, AttachmentKind, attachment_path_text, prompt_text};
use monocode_core::block::{ToolPreview, TurnMetrics};
use monocode_core::context_usage::ContextReading;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};
use monocode_core::task_list::is_task_list_tool_name;
use regex::Regex;
use serde_json::{Value, json};

use super::deps::{Rec, extract_tool_preview, stream_text_delta, title_from_tool_input};
use super::flavor::PiFlavor;

/// `SUPPORTED_PI_IMAGE_MIME_TYPES`: images Pi RPC accepts on `prompt` and `steer`.
pub const SUPPORTED_PI_IMAGE_MIME_TYPES: [&str; 4] =
    ["image/gif", "image/jpeg", "image/png", "image/webp"];

/// `PI_THINKING_LEVELS`.
pub const PI_THINKING_LEVELS: [&str; 7] =
    ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// `PiModelRef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiModelRef {
    pub provider: String,
    pub model_id: String,
}

/// The `method` of an `extension_ui_request` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiUiMethod {
    Select,
    Confirm,
    Input,
    Editor,
    Notify,
    SetStatus,
    SetWidget,
    SetTitle,
    SetEditorText,
}

impl PiUiMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            PiUiMethod::Select => "select",
            PiUiMethod::Confirm => "confirm",
            PiUiMethod::Input => "input",
            PiUiMethod::Editor => "editor",
            PiUiMethod::Notify => "notify",
            PiUiMethod::SetStatus => "setStatus",
            PiUiMethod::SetWidget => "setWidget",
            PiUiMethod::SetTitle => "setTitle",
            PiUiMethod::SetEditorText => "set_editor_text",
        }
    }
}

/// `PiExtensionUiRequest`. The TypeScript union becomes one struct:
/// `title` is always set for select, confirm, input, and editor; `message`
/// is set for confirm; `options` is filled for select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiExtensionUiRequest {
    pub id: String,
    pub method: PiUiMethod,
    pub title: Option<String>,
    pub message: Option<String>,
    pub options: Vec<String>,
}

impl PiExtensionUiRequest {
    /// A `notify` request, which the family builds to strip ANSI from a label.
    pub fn notify(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            method: PiUiMethod::Notify,
            title: Some(title.to_string()),
            message: None,
            options: Vec::new(),
        }
    }
}

/// `PiRpcResponse`.
#[derive(Debug, Clone, PartialEq)]
pub struct PiRpcResponse {
    pub id: Option<String>,
    pub command: String,
    pub success: bool,
    pub error: Option<String>,
    pub data: Option<Value>,
}

/// `asRecord`.
pub fn as_record(value: Option<&Value>) -> Option<&Rec> {
    value?.as_object()
}

/// `stringField`: a string with something other than whitespace in it.
pub fn string_field<'a>(rec: Option<&'a Rec>, key: &str) -> Option<&'a str> {
    match rec?.get(key)? {
        Value::String(value) if !js::trim(value).is_empty() => Some(value),
        _ => None,
    }
}

/// `numberField`. serde_json numbers are always finite.
pub fn number_field(rec: Option<&Rec>, key: &str) -> Option<f64> {
    rec?.get(key)?.as_f64()
}

fn tool_args_from_event(rec: Option<&Rec>) -> Rec {
    let Some(rec) = rec else {
        return Rec::new();
    };
    parse_arg_bag(rec.get("args"))
        .or_else(|| parse_arg_bag(rec.get("arguments")))
        .or_else(|| parse_arg_bag(rec.get("input")))
        .unwrap_or_default()
}

fn parse_arg_bag(value: Option<&Value>) -> Option<Rec> {
    if let Some(Value::String(text)) = value
        && !js::trim(text).is_empty()
    {
        return try_parse_json_record(text);
    }
    as_record(value).cloned()
}

/// `mergeToolInput`: later execution updates can be partial, so keep the
/// keys we already have.
pub fn merge_tool_input(current: &Rec, next: &Rec) -> Rec {
    if next.is_empty() {
        return current.clone();
    }
    let mut merged = current.clone();
    for (key, value) in next {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

/// `parseJsonLine`.
pub fn parse_json_line(line: &str) -> Option<Rec> {
    let trimmed = js::trim(line);
    if !trimmed.starts_with('{') {
        return None;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// `parsePiVersion`.
pub fn parse_pi_version(output: &str) -> Option<String> {
    static VERSION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[0-9]+\.[0-9]+\.[0-9]+").expect("version regex"));
    VERSION.find(output).map(|found| found.as_str().to_string())
}

/// The options of `buildPiSpawnArgs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PiSpawnOptions {
    pub model: Option<String>,
    pub resume: Option<String>,
    /// Catalog probes and isolated jobs: do not write a session file.
    pub no_session: bool,
    /// Catalog probes and throwaway text jobs, never for live chat.
    pub no_extensions: bool,
    /// Titles and other one-shot prompts: no tools, skills, or project context.
    pub isolated: bool,
    pub plan: bool,
}

/// `buildPiSpawnArgs`: spawn args for a session. Live sessions leave out
/// `--no-extensions` so the user's global Pi packages (todos, subagents,
/// custom tools) still load. Project-local `.pi` resources follow Pi's saved
/// trust.json, and RPC mode never prompts.
pub fn build_pi_spawn_args(flavor: &PiFlavor, input: &PiSpawnOptions) -> Vec<String> {
    let mut args: Vec<String> = vec!["--mode".into(), "rpc".into()];
    if input.isolated || input.no_session {
        args.push("--no-session".into());
    }
    if input.isolated || input.no_extensions {
        args.push("--no-extensions".into());
    }
    if input.isolated {
        args.extend(flavor.isolate_flags.iter().map(|flag| flag.to_string()));
    } else if input.plan {
        args.push("--tools".into());
        args.push(flavor.plan_tools.join(","));
    }
    if let Some(resume) = input.resume.as_deref().map(js::trim)
        && !resume.is_empty()
    {
        args.push(flavor.resume_flag.into());
        args.push(resume.into());
    }
    if let Some(model) = input.model.as_deref().map(js::trim)
        && !model.is_empty()
    {
        args.push("--model".into());
        args.push(model.into());
    }
    args
}

/// `parsePiModelRef`.
pub fn parse_pi_model_ref(native_id: Option<&str>) -> Option<PiModelRef> {
    let trimmed = js::trim(native_id?);
    if trimmed.is_empty() {
        return None;
    }
    let separator = trimmed.find('/')?;
    if separator == 0 || separator == trimmed.len() - 1 {
        return None;
    }
    Some(PiModelRef {
        provider: trimmed[..separator].to_string(),
        model_id: trimmed[separator + 1..].to_string(),
    })
}

/// `piNativeId`.
pub fn pi_native_id(provider: &str, model_id: &str) -> String {
    format!("{provider}/{model_id}")
}

fn pi_prompt_content(text: &str, attachments: &[Attachment]) -> Result<Rec, String> {
    let mut images: Vec<Value> = Vec::new();
    let body = prompt_text(text, attachments);
    let mut parts: Vec<String> = if body.is_empty() {
        Vec::new()
    } else {
        vec![body]
    };
    for attachment in attachments {
        let mime_type = js::trim(&attachment.mime_type).to_lowercase();
        match attachment.data.as_deref() {
            Some(data)
                if attachment.kind == AttachmentKind::Image
                    && !data.is_empty()
                    && SUPPORTED_PI_IMAGE_MIME_TYPES.contains(&mime_type.as_str()) =>
            {
                images.push(json!({ "type": "image", "data": data, "mimeType": mime_type }));
            }
            _ => parts.push(attachment_path_text(attachment)?),
        }
    }
    let mut content = Rec::new();
    content.insert("message".into(), Value::String(parts.join("\n\n")));
    if !images.is_empty() {
        content.insert("images".into(), Value::Array(images));
    }
    Ok(content)
}

/// `buildPiPrompt`. Fails when an attachment has no deliverable source.
pub fn build_pi_prompt(
    text: &str,
    attachments: &[Attachment],
    streaming: bool,
) -> Result<Rec, String> {
    let mut command = Rec::new();
    command.insert("type".into(), "prompt".into());
    command.extend(pi_prompt_content(text, attachments)?);
    if streaming {
        command.insert("streamingBehavior".into(), "steer".into());
    }
    Ok(command)
}

/// `PiForkMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiForkMessage {
    pub entry_id: String,
    pub text: String,
}

/// `forkMessagesFromRpcData`.
pub fn fork_messages_from_rpc_data(data: Option<&Value>) -> Vec<PiForkMessage> {
    let rec = as_record(data);
    let Some(messages) = rec.and_then(|rec| rec.get("messages")?.as_array()) else {
        return Vec::new();
    };
    messages
        .iter()
        .filter_map(|item| {
            let row = item.as_object();
            let entry_id = string_field(row, "entryId")?;
            Some(PiForkMessage {
                entry_id: entry_id.to_string(),
                text: string_field(row, "text").unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// `buildPiSteer`.
pub fn build_pi_steer(text: &str, attachments: &[Attachment]) -> Result<Rec, String> {
    let mut command = Rec::new();
    command.insert("type".into(), "steer".into());
    command.extend(pi_prompt_content(text, attachments)?);
    Ok(command)
}

/// `parseRpcResponse`.
pub fn parse_rpc_response(rec: &Rec) -> Option<PiRpcResponse> {
    let rec = Some(rec);
    if string_field(rec, "type") != Some("response") {
        return None;
    }
    let raw = rec?;
    Some(PiRpcResponse {
        id: string_field(rec, "id").map(str::to_string),
        command: string_field(rec, "command")
            .unwrap_or("unknown")
            .to_string(),
        success: raw.get("success") == Some(&Value::Bool(true)),
        error: string_field(rec, "error").map(str::to_string),
        data: raw.get("data").cloned(),
    })
}

/// `parseExtensionUiRequest`.
pub fn parse_extension_ui_request(rec: &Rec) -> Option<PiExtensionUiRequest> {
    let raw = rec;
    let rec = Some(rec);
    if string_field(rec, "type") != Some("extension_ui_request") {
        return None;
    }
    let id = string_field(rec, "id")?.to_string();
    let method = string_field(rec, "method")?;
    let request = |method, title: Option<&str>| PiExtensionUiRequest {
        id: id.clone(),
        method,
        title: title.map(str::to_string),
        message: None,
        options: Vec::new(),
    };
    match method {
        "select" => {
            let options = raw
                .get("options")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Some(PiExtensionUiRequest {
                options,
                ..request(
                    PiUiMethod::Select,
                    Some(string_field(rec, "title").unwrap_or("Choose an option")),
                )
            })
        }
        "confirm" => Some(PiExtensionUiRequest {
            message: Some(string_field(rec, "message").unwrap_or("").to_string()),
            ..request(
                PiUiMethod::Confirm,
                Some(string_field(rec, "title").unwrap_or("Confirm")),
            )
        }),
        "input" | "editor" => {
            let kind = if method == "input" {
                PiUiMethod::Input
            } else {
                PiUiMethod::Editor
            };
            Some(request(
                kind,
                Some(string_field(rec, "title").unwrap_or(method)),
            ))
        }
        "notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text" => {
            let kind = match method {
                "notify" => PiUiMethod::Notify,
                "setStatus" => PiUiMethod::SetStatus,
                "setWidget" => PiUiMethod::SetWidget,
                "setTitle" => PiUiMethod::SetTitle,
                _ => PiUiMethod::SetEditorText,
            };
            let title = string_field(rec, "message")
                .or_else(|| string_field(rec, "statusText"))
                .or_else(|| string_field(rec, "title"))
                .or_else(|| string_field(rec, "text"));
            Some(request(kind, title))
        }
        _ => None,
    }
}

/// `extensionUiResponse`.
pub fn extension_ui_response(request: &PiExtensionUiRequest, decision: ApprovalDecision) -> Value {
    if decision == ApprovalDecision::Deny {
        return json!({ "type": "extension_ui_response", "id": request.id, "cancelled": true });
    }
    match request.method {
        PiUiMethod::Confirm => {
            json!({ "type": "extension_ui_response", "id": request.id, "confirmed": true })
        }
        PiUiMethod::Select => {
            let value = request.options.first().cloned().unwrap_or_default();
            json!({ "type": "extension_ui_response", "id": request.id, "value": value })
        }
        _ => json!({ "type": "extension_ui_response", "id": request.id, "cancelled": true }),
    }
}

static OSC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\x1b\]|\x{9d})[^\x07\x1b\x{9c}]*(?:\x07|\x1b\\|\x{9c})").expect("osc regex")
});
static CSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\x1b\[|\x{9b})[0-?]*[ -/]*[@-~]").expect("csi regex"));

/// `extensionUiTitle`. Pi's theme helpers emit ANSI even in RPC mode (for
/// example Ponytail's setStatus). These labels use native styling, so CSI and
/// OSC sequences are stripped here, at the display boundary. Select replies
/// must keep the original option.
pub fn extension_ui_title(request: &PiExtensionUiRequest) -> String {
    let text = if request.method == PiUiMethod::Confirm {
        [request.title.as_deref(), request.message.as_deref()]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" — ")
    } else {
        request
            .title
            .clone()
            .unwrap_or_else(|| "Pi extension".to_string())
    };
    let text = OSC.replace_all(&text, "");
    CSI.replace_all(&text, "").into_owned()
}

/// `needsExtensionUiReply`.
pub fn needs_extension_ui_reply(request: &PiExtensionUiRequest) -> bool {
    matches!(
        request.method,
        PiUiMethod::Confirm | PiUiMethod::Select | PiUiMethod::Input | PiUiMethod::Editor
    )
}

/// What `sessionFromState` reads from `get_state` data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PiSessionState {
    pub session_id: Option<String>,
    pub session_file: Option<String>,
    pub context_window: Option<i64>,
}

/// `sessionFromState`.
pub fn session_from_state(data: Option<&Value>) -> PiSessionState {
    let rec = as_record(data);
    let model = as_record(rec.and_then(|rec| rec.get("model")));
    let window = number_field(model, "contextWindow");
    PiSessionState {
        session_id: string_field(rec, "sessionId").map(str::to_string),
        session_file: string_field(rec, "sessionFile").map(str::to_string),
        context_window: window
            .filter(|window| *window > 0.0)
            .map(|window| window as i64),
    }
}

/// `providerSessionIdFromState`. Session-store ids may only hold ASCII
/// letters, digits, `-`, and `_`. Pi's `sessionFile` is a filesystem path,
/// which persist would reject (and the sidebar would never get a git
/// snapshot). `--session` also accepts the UUID `sessionId`, so bind that.
pub fn provider_session_id_from_state(data: Option<&Value>) -> Option<String> {
    let session_id = session_from_state(data).session_id?;
    let session_id = js::trim(&session_id);
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(session_id.to_string())
}

fn usage_record(rec: &Rec) -> Option<&Rec> {
    as_record(rec.get("usage"))
        .or_else(|| assistant_message_usage(rec))
        .or_else(|| {
            let event = as_record(rec.get("assistantMessageEvent"))?;
            let partial = as_record(event.get("partial"))?;
            as_record(partial.get("usage"))
        })
}

/// `contextFromUsage`. Current Pi puts streaming usage on the frame. 0.80.x
/// put a finished assistant total on `message` and a live total on
/// `assistantMessageEvent.partial`. Tool-result messages can carry nested LLM
/// usage for a sub-call, which is not the context-window level, so only
/// assistant `message.usage` counts.
pub fn context_from_usage(rec: &Rec, window: Option<i64>) -> Option<ContextReading> {
    let usage = Some(usage_record(rec)?);
    let used = match number_field(usage, "totalTokens") {
        Some(total) if total != 0.0 => total,
        _ => {
            number_field(usage, "input").unwrap_or(0.0)
                + number_field(usage, "output").unwrap_or(0.0)
                + number_field(usage, "cacheRead").unwrap_or(0.0)
                + number_field(usage, "cacheWrite").unwrap_or(0.0)
        }
    };
    let window = window.filter(|window| *window > 0);
    if used == 0.0 {
        return window.map(|window| ContextReading {
            used: None,
            window: Some(window),
        });
    }
    Some(ContextReading {
        used: Some(used as i64),
        window,
    })
}

/// `turnMetricsFromUsage`.
pub fn turn_metrics_from_usage(rec: &Rec) -> Option<TurnMetrics> {
    let usage = usage_record(rec)?;
    let field = |key| number_field(Some(usage), key).unwrap_or(0.0);
    let input_tokens = field("input");
    let output_tokens = field("output");
    let cache_read_tokens = field("cacheRead");
    let cache_write_tokens = field("cacheWrite");
    let cache_reported = usage.contains_key("cacheRead") || usage.contains_key("cacheWrite");
    let cacheable_input = input_tokens + cache_read_tokens + cache_write_tokens;
    if input_tokens == 0.0 && output_tokens == 0.0 && cacheable_input == 0.0 {
        return None;
    }
    let nonzero = |value: f64| (value != 0.0).then_some(value as i64);
    Some(TurnMetrics {
        input_tokens: nonzero(input_tokens),
        output_tokens: nonzero(output_tokens),
        cache_read_tokens: nonzero(cache_read_tokens),
        cache_write_tokens: nonzero(cache_write_tokens),
        cache_hit_percent: (cache_reported && cacheable_input != 0.0)
            .then(|| (cache_read_tokens / cacheable_input) * 100.0),
        extra: Default::default(),
    })
}

/// `contextFromSessionStats`.
pub fn context_from_session_stats(data: Option<&Value>) -> Option<ContextReading> {
    let rec = as_record(data);
    let usage = Some(as_record(rec?.get("contextUsage"))?);
    let used = number_field(usage, "tokens").filter(|used| *used != 0.0);
    let window = number_field(usage, "contextWindow").filter(|window| *window != 0.0);
    if used.is_none() && window.is_none() {
        return None;
    }
    Some(ContextReading {
        used: used.map(|used| used as i64),
        window: window
            .filter(|window| *window > 0.0)
            .map(|window| window as i64),
    })
}

/// Whether a streamed assistant delta is answer text or thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiDeltaKind {
    Text,
    Thinking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiDelta {
    pub kind: PiDeltaKind,
    pub text: String,
}

fn message_update_event(rec: &Rec) -> Option<&Rec> {
    if string_field(Some(rec), "type") != Some("message_update") {
        return None;
    }
    as_record(rec.get("assistantMessageEvent"))
}

/// `assistantDeltaFromEvent`.
pub fn assistant_delta_from_event(rec: &Rec) -> Option<PiDelta> {
    if string_field(Some(rec), "type") != Some("message_update") {
        return None;
    }
    let event = as_record(rec.get("assistantMessageEvent"));
    let kind = string_field(event, "type");
    let delta = stream_text_delta(event.and_then(|event| event.get("delta")));
    if delta.is_empty() {
        return None;
    }
    match kind {
        Some("text_delta") => Some(PiDelta {
            kind: PiDeltaKind::Text,
            text: delta,
        }),
        Some("thinking_delta") => Some(PiDelta {
            kind: PiDeltaKind::Thinking,
            text: delta,
        }),
        _ => None,
    }
}

/// What `toolCallStartFromEvent` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiToolCallStart {
    pub id: String,
    pub name: String,
    pub index: i64,
}

/// `toolCallStartFromEvent`.
pub fn tool_call_start_from_event(rec: &Rec) -> Option<PiToolCallStart> {
    let event = Some(message_update_event(rec)?);
    if string_field(event, "type") != Some("toolcall_start") {
        return None;
    }
    let id = string_field(event, "id")?;
    let name = string_field(event, "toolName").or_else(|| string_field(event, "name"))?;
    Some(PiToolCallStart {
        id: id.to_string(),
        name: name.to_string(),
        index: number_field(event, "contentIndex").map_or(-1, |index| index as i64),
    })
}

/// What `toolCallDeltaFromEvent` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiToolCallDelta {
    pub index: i64,
    pub delta: String,
}

/// `toolCallDeltaFromEvent`.
pub fn tool_call_delta_from_event(rec: &Rec) -> Option<PiToolCallDelta> {
    let event = message_update_event(rec)?;
    if string_field(Some(event), "type") != Some("toolcall_delta") {
        return None;
    }
    let delta = stream_text_delta(event.get("delta"));
    if delta.is_empty() {
        return None;
    }
    Some(PiToolCallDelta {
        index: number_field(Some(event), "contentIndex").map_or(-1, |index| index as i64),
        delta,
    })
}

/// A tool call with its parsed input, from `toolcall_end` or
/// `tool_execution_start`.
#[derive(Debug, Clone, PartialEq)]
pub struct PiToolCall {
    pub id: String,
    pub name: String,
    pub input: Rec,
}

/// `toolCallEndFromEvent`.
pub fn tool_call_end_from_event(rec: &Rec) -> Option<PiToolCall> {
    let event = message_update_event(rec)?;
    if string_field(Some(event), "type") != Some("toolcall_end") {
        return None;
    }
    let call = as_record(event.get("toolCall")).unwrap_or(event);
    let id = string_field(Some(call), "id").or_else(|| string_field(Some(event), "id"))?;
    let name = string_field(Some(call), "name")
        .or_else(|| string_field(Some(call), "toolName"))
        .or_else(|| string_field(Some(event), "toolName"))?;
    let input = parse_arg_bag(call.get("arguments"))
        .or_else(|| parse_arg_bag(call.get("args")))
        .or_else(|| parse_arg_bag(event.get("arguments")))
        .unwrap_or_else(|| tool_args_from_event(Some(call)));
    Some(PiToolCall {
        id: id.to_string(),
        name: name.to_string(),
        input,
    })
}

/// `toolExecutionStartFromEvent`.
pub fn tool_execution_start_from_event(rec: &Rec) -> Option<PiToolCall> {
    let frame = Some(rec);
    if string_field(frame, "type") != Some("tool_execution_start") {
        return None;
    }
    let id = string_field(frame, "toolCallId")?;
    Some(PiToolCall {
        id: id.to_string(),
        name: string_field(frame, "toolName")
            .unwrap_or("tool")
            .to_string(),
        input: tool_args_from_event(frame),
    })
}

/// What `toolExecutionUpdateFromEvent` returns.
#[derive(Debug, Clone, PartialEq)]
pub struct PiToolUpdate {
    pub id: String,
    pub name: Option<String>,
    pub detail: Option<String>,
    pub input: Rec,
}

/// `toolExecutionUpdateFromEvent`.
pub fn tool_execution_update_from_event(rec: &Rec) -> Option<PiToolUpdate> {
    let frame = Some(rec);
    if string_field(frame, "type") != Some("tool_execution_update") {
        return None;
    }
    let id = string_field(frame, "toolCallId")?;
    let partial = as_record(rec.get("partialResult"));
    let detail = text_from_content(partial.and_then(|partial| partial.get("content")));
    Some(PiToolUpdate {
        id: id.to_string(),
        name: string_field(frame, "toolName").map(str::to_string),
        detail: (!detail.is_empty()).then_some(detail),
        input: tool_args_from_event(frame),
    })
}

/// What `toolExecutionEndFromEvent` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiToolEnd {
    pub id: String,
    pub name: Option<String>,
    pub detail: Option<String>,
    pub is_error: bool,
}

/// `toolExecutionEndFromEvent`.
pub fn tool_execution_end_from_event(rec: &Rec) -> Option<PiToolEnd> {
    let frame = Some(rec);
    if string_field(frame, "type") != Some("tool_execution_end") {
        return None;
    }
    let id = string_field(frame, "toolCallId")?;
    let result = as_record(rec.get("result"));
    let detail = text_from_content(result.and_then(|result| result.get("content")));
    Some(PiToolEnd {
        id: id.to_string(),
        name: string_field(frame, "toolName").map(str::to_string),
        detail: (!detail.is_empty()).then_some(detail),
        is_error: rec.get("isError") == Some(&Value::Bool(true)),
    })
}

/// `turnErrorFromEvent`. A failed turn arrives inside the assistant message,
/// not as an error frame: `stopReason: "error"` with the reason in
/// `errorMessage`. An empty string means it failed without a reason.
pub fn turn_error_from_event(rec: &Rec) -> Option<String> {
    if string_field(Some(rec), "type") != Some("message_end") {
        return None;
    }
    let message = as_record(rec.get("message"));
    if string_field(message, "role") != Some("assistant") {
        return None;
    }
    if string_field(message, "stopReason") != Some("error") {
        return None;
    }
    Some(
        string_field(message, "errorMessage")
            .unwrap_or("")
            .to_string(),
    )
}

/// `isAgentSettled`.
pub fn is_agent_settled(rec: &Rec) -> bool {
    string_field(Some(rec), "type") == Some("agent_settled")
}

/// `agentEndWillRetry`: `None` for frames other than `agent_end`.
pub fn agent_end_will_retry(rec: &Rec) -> Option<bool> {
    if string_field(Some(rec), "type") != Some("agent_end") {
        return None;
    }
    Some(rec.get("willRetry") == Some(&Value::Bool(true)))
}

/// `statusFromPiEvent`.
pub fn status_from_pi_event(rec: &Rec) -> Option<String> {
    let frame = Some(rec);
    match string_field(frame, "type")? {
        "compaction_start" => Some("Compacting context…".into()),
        "auto_retry_start" => {
            let attempt = number_field(frame, "attempt").filter(|value| *value != 0.0);
            let max = number_field(frame, "maxAttempts").filter(|value| *value != 0.0);
            match (attempt, max) {
                (Some(attempt), Some(max)) => Some(format!(
                    "Retrying ({}/{})…",
                    js::number_to_string(attempt),
                    js::number_to_string(max)
                )),
                _ => Some("Retrying…".into()),
            }
        }
        "extension_error" => Some(
            string_field(frame, "error")
                .unwrap_or("Pi extension error")
                .to_string(),
        ),
        _ => None,
    }
}

/// `tryParseJsonRecord`.
pub fn try_parse_json_record(partial: &str) -> Option<Rec> {
    match serde_json::from_str::<Value>(partial) {
        Ok(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// `textFromContent`.
pub fn text_from_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.as_str()),
                Value::Object(rec) => string_field(Some(rec), "text"),
                _ => None,
            })
            .collect(),
        _ => String::new(),
    }
}

/// `toolKindFromName`.
pub fn tool_kind_from_name(tool_name: &str) -> String {
    let normalized = tool_name.to_lowercase();
    let has = |part: &str| normalized.contains(part);
    if is_task_list_tool_name(tool_name) {
        return "tasks".into();
    }
    if has("bash") || has("command") || has("shell") {
        return "execute".into();
    }
    if has("edit") || has("write") || has("patch") || has("replace") {
        return "edit".into();
    }
    if normalized == "read" || has("read") {
        return "read".into();
    }
    if has("grep") || has("glob") || has("search") || has("find") || normalized == "ls" {
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

/// `toolTitle`.
pub fn tool_title(name: &str, input: &Rec) -> String {
    title_from_tool_input(name, &tool_kind_from_name(name), input)
}

/// `previewFromTool`.
pub fn preview_from_tool(name: &str, input: &Rec, output: Option<&str>) -> Option<ToolPreview> {
    let kind = tool_kind_from_name(name);
    let mut update = Rec::new();
    update.insert("title".into(), name.into());
    update.insert("name".into(), name.into());
    update.insert("kind".into(), kind.clone().into());
    update.insert("input".into(), Value::Object(input.clone()));
    update.insert("rawInput".into(), Value::Object(input.clone()));
    if let Some(output) = output {
        update.insert("content".into(), output.into());
    }
    let mut tool = Rec::new();
    tool.insert("title".into(), name.into());
    tool.insert("name".into(), name.into());
    tool.insert("kind".into(), kind.into());
    tool.insert("rawInput".into(), Value::Object(input.clone()));
    extract_tool_preview(&update, &tool)
}

/// `summarizeToolRequest`.
pub fn summarize_tool_request(tool_name: &str, input: &Rec) -> String {
    let rec = Some(input);
    if let Some(command) = string_field(rec, "command").or_else(|| string_field(rec, "cmd")) {
        return format!("{tool_name}: {}", js::slice_prefix(command, 400));
    }
    if let Some(path) = string_field(rec, "path")
        .or_else(|| string_field(rec, "file_path"))
        .or_else(|| string_field(rec, "filePath"))
    {
        return format!("{tool_name}: {path}");
    }
    let Ok(serialized) = serde_json::to_string(input) else {
        return tool_name.to_string();
    };
    if js::len(&serialized) <= 400 {
        return format!("{tool_name}: {serialized}");
    }
    format!("{tool_name}: {}...", js::slice_prefix(&serialized, 397))
}

/// Approximation of `String.prototype.localeCompare`: case-insensitive
/// first, then lowercase before uppercase.
// TODO(port): localeCompare uses ICU collation, which also orders
// punctuation before digits. This matches it for ASCII model names.
fn locale_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// `modelsFromRpcData`: flatten a `get_available_models` payload.
pub fn models_from_rpc_data(flavor: &PiFlavor, data: Option<&Value>) -> Vec<AgentModel> {
    let rec = as_record(data);
    let list: &[Value] = match (rec.and_then(|rec| rec.get("models")), data) {
        (Some(Value::Array(models)), _) => models,
        (_, Some(Value::Array(models))) => models,
        _ => &[],
    };
    let mut models: Vec<AgentModel> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for item in list {
        let Some(model) = item.as_object() else {
            continue;
        };
        let model_rec = Some(model);
        let (Some(model_id), Some(provider)) = (
            string_field(model_rec, "id"),
            string_field(model_rec, "provider"),
        ) else {
            continue;
        };
        let native_id = pi_native_id(provider, model_id);
        if !seen.insert(native_id.clone()) {
            continue;
        }
        let name = match string_field(model_rec, "name") {
            Some(name) if !name.is_empty() => name,
            _ => model_id,
        };
        let context_window = number_field(model_rec, "contextWindow");
        let settings: Vec<ModelSetting> = [
            thinking_setting(model.get("reasoning") == Some(&Value::Bool(true))),
            flavor.is_omp().then(fast_mode_setting),
        ]
        .into_iter()
        .flatten()
        .collect();
        models.push(AgentModel {
            id: format!("{}:{native_id}", flavor.id),
            harness: flavor.id,
            name: name.to_string(),
            native_id: Some(native_id),
            provider: None,
            settings: (!settings.is_empty()).then_some(settings),
            context_window: context_window
                .filter(|window| *window > 0.0)
                .map(|window| window as i64),
        });
    }
    models.sort_by(|left, right| locale_compare(&left.name, &right.name));
    models
}

/// `thinkingSetting`.
pub fn thinking_setting(reasoning: bool) -> Option<ModelSetting> {
    if !reasoning {
        return None;
    }
    Some(ModelSetting {
        id: "thinking".into(),
        label: "Thinking".into(),
        kind: ModelSettingKind::Select,
        value: "medium".into(),
        options: PI_THINKING_LEVELS
            .iter()
            .map(|value| ModelSettingChoice {
                value: (*value).into(),
                label: thinking_label(value),
            })
            .collect(),
        description: None,
    })
}

/// `fastModeSetting`.
pub fn fast_mode_setting() -> ModelSetting {
    ModelSetting {
        id: "fast".into(),
        label: "Fast".into(),
        description: Some("Use priority processing when the current model supports it".into()),
        kind: ModelSettingKind::Toggle,
        value: "false".into(),
        options: vec![
            ModelSettingChoice {
                value: "true".into(),
                label: "On".into(),
            },
            ModelSettingChoice {
                value: "false".into(),
                label: "Off".into(),
            },
        ],
    }
}

/// `isPiThinkingLevel`.
pub fn is_pi_thinking_level(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && PI_THINKING_LEVELS.contains(&value))
}

fn thinking_label(level: &str) -> String {
    match level {
        "xhigh" => "Extra High".into(),
        "off" => "Off".into(),
        _ => {
            let mut chars = level.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

fn assistant_message_usage(rec: &Rec) -> Option<&Rec> {
    let message = as_record(rec.get("message"));
    if string_field(message, "role") != Some("assistant") {
        return None;
    }
    as_record(message?.get("usage"))
}

#[cfg(test)]
mod tests {
    use super::super::flavor::{OMP_FLAVOR, PI_FLAVOR};
    use super::*;

    fn rec(value: Value) -> Rec {
        match value {
            Value::Object(rec) => rec,
            other => panic!("not an object: {other}"),
        }
    }

    fn args(flavor: &PiFlavor, options: PiSpawnOptions) -> Vec<String> {
        build_pi_spawn_args(flavor, &options)
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    mod build_pi_spawn_args {
        use super::*;

        #[test]
        fn starts_rpc_without_stripping_the_users_extensions() {
            assert_eq!(
                args(&PI_FLAVOR, PiSpawnOptions::default()),
                strings(&["--mode", "rpc"])
            );
            assert_eq!(
                args(
                    &PI_FLAVOR,
                    PiSpawnOptions {
                        model: Some("anthropic/claude-sonnet-4".into()),
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--model", "anthropic/claude-sonnet-4"])
            );
            assert_eq!(
                args(
                    &PI_FLAVOR,
                    PiSpawnOptions {
                        resume: Some("abc123".into()),
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--session", "abc123"])
            );
        }

        #[test]
        fn can_skip_session_files_without_disabling_extensions() {
            assert_eq!(
                args(
                    &PI_FLAVOR,
                    PiSpawnOptions {
                        no_session: true,
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--no-session"])
            );
        }

        #[test]
        fn strips_extensions_for_throwaway_catalog_probes() {
            assert_eq!(
                args(
                    &PI_FLAVOR,
                    PiSpawnOptions {
                        no_session: true,
                        no_extensions: true,
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--no-session", "--no-extensions"])
            );
        }

        #[test]
        fn isolates_throwaway_text_jobs_from_tools_and_project_context() {
            assert_eq!(
                args(
                    &PI_FLAVOR,
                    PiSpawnOptions {
                        isolated: true,
                        ..Default::default()
                    }
                ),
                strings(&[
                    "--mode",
                    "rpc",
                    "--no-session",
                    "--no-extensions",
                    "--no-tools",
                    "--no-skills",
                    "--no-context-files",
                ])
            );
        }

        #[test]
        fn limits_plan_turns_to_each_flavors_read_only_tools() {
            assert_eq!(
                args(
                    &PI_FLAVOR,
                    PiSpawnOptions {
                        plan: true,
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--tools", "read,grep,find,ls"])
            );
            assert_eq!(
                args(
                    &OMP_FLAVOR,
                    PiSpawnOptions {
                        plan: true,
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--tools", "read,grep,glob,lsp"])
            );
        }

        #[test]
        fn uses_omps_renamed_resume_and_context_flags() {
            assert_eq!(
                args(
                    &OMP_FLAVOR,
                    PiSpawnOptions {
                        resume: Some("abc123".into()),
                        ..Default::default()
                    }
                ),
                strings(&["--mode", "rpc", "--resume", "abc123"])
            );
            assert_eq!(
                args(
                    &OMP_FLAVOR,
                    PiSpawnOptions {
                        isolated: true,
                        ..Default::default()
                    }
                ),
                strings(&[
                    "--mode",
                    "rpc",
                    "--no-session",
                    "--no-extensions",
                    "--no-tools",
                    "--no-skills",
                    "--no-rules",
                ])
            );
        }
    }

    #[test]
    fn parse_pi_model_ref_splits_provider_model_ids() {
        assert_eq!(
            parse_pi_model_ref(Some("anthropic/claude-sonnet-4-20250514")),
            Some(PiModelRef {
                provider: "anthropic".into(),
                model_id: "claude-sonnet-4-20250514".into()
            })
        );
        assert_eq!(parse_pi_model_ref(Some("")), None);
        assert_eq!(parse_pi_model_ref(Some("sonnet")), None);
    }

    #[test]
    fn parse_pi_version_reads_a_semver_from_cli_output() {
        assert_eq!(parse_pi_version("0.49.2").as_deref(), Some("0.49.2"));
        assert_eq!(
            parse_pi_version("@earendil-works/pi-coding-agent/0.30.1").as_deref(),
            Some("0.30.1")
        );
    }

    mod build_pi_prompt {
        use super::*;

        #[test]
        fn attaches_vision_images_and_steers_while_streaming() {
            let image = Attachment {
                id: "a".into(),
                name: "shot.png".into(),
                mime_type: "image/png".into(),
                kind: AttachmentKind::Image,
                size: 12,
                data: Some("abc".into()),
                ..Default::default()
            };
            let prompt = build_pi_prompt("look", &[image], true).unwrap();
            assert_eq!(
                Value::Object(prompt),
                json!({
                    "type": "prompt",
                    "message": "look",
                    "streamingBehavior": "steer",
                    "images": [{ "type": "image", "data": "abc", "mimeType": "image/png" }],
                })
            );
        }

        #[test]
        fn builds_a_steer_command() {
            assert_eq!(
                Value::Object(build_pi_steer("stop", &[]).unwrap()),
                json!({ "type": "steer", "message": "stop" })
            );
        }

        #[test]
        fn parses_forkable_user_messages_from_rpc_data() {
            let data = json!({
                "messages": [{ "entryId": "u1", "text": "first" }, { "entryId": "u2", "text": "second" }]
            });
            assert_eq!(
                fork_messages_from_rpc_data(Some(&data)),
                vec![
                    PiForkMessage {
                        entry_id: "u1".into(),
                        text: "first".into()
                    },
                    PiForkMessage {
                        entry_id: "u2".into(),
                        text: "second".into()
                    },
                ]
            );
        }
    }

    mod rpc_frames {
        use super::*;

        #[test]
        fn displays_colored_extension_labels_without_changing_rpc_values() {
            let option = "\u{1b}[32mProceed\u{1b}[39m";
            let request = parse_extension_ui_request(&rec(json!({
                "type": "extension_ui_request",
                "id": "colored-select",
                "method": "select",
                "title": "\u{1b}[1mChoose\u{1b}[22m",
                "options": [option],
            })))
            .unwrap();
            assert_eq!(extension_ui_title(&request), "Choose");
            assert_eq!(
                extension_ui_response(&request, ApprovalDecision::Allow),
                json!({ "type": "extension_ui_response", "id": "colored-select", "value": option })
            );
        }

        #[test]
        fn removes_terminal_styling_and_hyperlink_controls_from_confirmation_text() {
            let request = parse_extension_ui_request(&rec(json!({
                "type": "extension_ui_request",
                "id": "colored-confirm",
                "method": "confirm",
                "title": "\u{1b}[38;2;100;150;200mReview\u{1b}[0m",
                "message": "Open \u{1b}]8;;https://example.com\u{1b}\\docs\u{1b}]8;;\u{7}?\n\t[1, 2]",
            })))
            .unwrap();
            assert_eq!(
                extension_ui_title(&request),
                "Review — Open docs?\n\t[1, 2]"
            );
        }

        #[test]
        fn parses_responses_events_and_extension_ui() {
            assert_eq!(parse_json_line("not json"), None);
            let response = parse_rpc_response(&rec(json!({
                "type": "response", "command": "prompt", "success": true, "id": "req-1"
            })));
            assert_eq!(
                response,
                Some(PiRpcResponse {
                    id: Some("req-1".into()),
                    command: "prompt".into(),
                    success: true,
                    error: None,
                    data: None,
                })
            );
            assert_eq!(
                parse_rpc_response(&rec(json!({
                    "type": "response", "command": "set_model", "success": false, "error": "missing"
                })))
                .and_then(|response| response.error)
                .as_deref(),
                Some("missing")
            );

            let confirm = parse_extension_ui_request(&rec(json!({
                "type": "extension_ui_request",
                "id": "ui-1",
                "method": "confirm",
                "title": "Dangerous",
                "message": "Allow rm?",
            })))
            .unwrap();
            assert_eq!(
                confirm,
                PiExtensionUiRequest {
                    id: "ui-1".into(),
                    method: PiUiMethod::Confirm,
                    title: Some("Dangerous".into()),
                    message: Some("Allow rm?".into()),
                    options: Vec::new(),
                }
            );
            assert!(needs_extension_ui_reply(&confirm));
            assert_eq!(extension_ui_title(&confirm), "Dangerous — Allow rm?");
            assert_eq!(
                extension_ui_response(&confirm, ApprovalDecision::Allow),
                json!({ "type": "extension_ui_response", "id": "ui-1", "confirmed": true })
            );
            assert_eq!(
                extension_ui_response(&confirm, ApprovalDecision::Deny),
                json!({ "type": "extension_ui_response", "id": "ui-1", "cancelled": true })
            );

            let select = parse_extension_ui_request(&rec(json!({
                "type": "extension_ui_request",
                "id": "ui-2",
                "method": "select",
                "title": "Pick",
                "options": ["a", "b"],
            })))
            .unwrap();
            assert_eq!(
                extension_ui_response(&select, ApprovalDecision::Allow),
                json!({ "type": "extension_ui_response", "id": "ui-2", "value": "a" })
            );

            let notify = parse_extension_ui_request(&rec(json!({
                "type": "extension_ui_request",
                "id": "ui-3",
                "method": "notify",
                "message": "loaded",
            })))
            .unwrap();
            assert!(!needs_extension_ui_reply(&notify));
        }
    }

    #[test]
    fn streaming_events_map_text_thinking_tools_and_turn_completion() {
        let delta = |kind: &str, text: &str| {
            assistant_delta_from_event(&rec(json!({
                "type": "message_update",
                "assistantMessageEvent": { "type": kind, "delta": text },
            })))
        };
        assert_eq!(
            delta("text_delta", "Hello"),
            Some(PiDelta {
                kind: PiDeltaKind::Text,
                text: "Hello".into()
            })
        );
        assert_eq!(
            delta("thinking_delta", "hmm"),
            Some(PiDelta {
                kind: PiDeltaKind::Thinking,
                text: "hmm".into()
            })
        );
        assert_eq!(
            delta("text_delta", "\n\n"),
            Some(PiDelta {
                kind: PiDeltaKind::Text,
                text: "\n\n".into()
            })
        );

        assert_eq!(
            tool_call_start_from_event(&rec(json!({
                "type": "message_update",
                "assistantMessageEvent": {
                    "type": "toolcall_start", "contentIndex": 1, "id": "call_1", "toolName": "write"
                },
            }))),
            Some(PiToolCallStart {
                id: "call_1".into(),
                name: "write".into(),
                index: 1
            })
        );

        assert_eq!(
            tool_execution_start_from_event(&rec(json!({
                "type": "tool_execution_start",
                "toolCallId": "call_1",
                "toolName": "bash",
                "args": { "command": "ls -la" },
            }))),
            Some(PiToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                input: rec(json!({ "command": "ls -la" })),
            })
        );
        assert_eq!(
            tool_execution_start_from_event(&rec(json!({
                "type": "tool_execution_start",
                "toolCallId": "call_2",
                "toolName": "bash",
                "args": "{\"command\":\"pwd\"}",
            }))),
            Some(PiToolCall {
                id: "call_2".into(),
                name: "bash".into(),
                input: rec(json!({ "command": "pwd" })),
            })
        );

        assert_eq!(
            tool_execution_end_from_event(&rec(json!({
                "type": "tool_execution_end",
                "toolCallId": "call_1",
                "toolName": "bash",
                "isError": false,
                "result": { "content": [{ "type": "text", "text": "ok" }] },
            }))),
            Some(PiToolEnd {
                id: "call_1".into(),
                name: Some("bash".into()),
                detail: Some("ok".into()),
                is_error: false,
            })
        );

        assert!(is_agent_settled(&rec(json!({ "type": "agent_settled" }))));
        assert_eq!(
            agent_end_will_retry(&rec(json!({ "type": "agent_end", "willRetry": true }))),
            Some(true)
        );
        assert_eq!(
            agent_end_will_retry(&rec(json!({ "type": "agent_end" }))),
            Some(false)
        );
    }

    mod tools_and_models {
        use super::*;

        #[test]
        fn titles_built_in_pi_tools() {
            assert_eq!(tool_kind_from_name("bash"), "execute");
            assert_eq!(tool_kind_from_name("edit"), "edit");
            assert_eq!(tool_kind_from_name("todo_write"), "tasks");
        }

        #[test]
        fn titles_built_in_pi_tools_with_previews() {
            assert_eq!(
                tool_title("bash", &rec(json!({ "command": "git status -s" }))),
                "git status -s"
            );
            assert!(tool_title("read", &rec(json!({ "path": "src/a.ts" }))).contains("src/a.ts"));
            assert_eq!(
                preview_from_tool("write", &rec(json!({ "path": "src/a.ts" })), None)
                    .map(|preview| preview.kind),
                Some(monocode_core::block::ToolPreviewKind::Write)
            );
        }

        #[test]
        fn keeps_earlier_tool_args_when_a_later_update_is_partial() {
            assert_eq!(
                merge_tool_input(
                    &rec(json!({ "command": "git status -s" })),
                    &rec(json!({ "timeout": 30 }))
                ),
                rec(json!({ "command": "git status -s", "timeout": 30 }))
            );
            assert_eq!(
                merge_tool_input(&rec(json!({ "command": "ls" })), &Rec::new()),
                rec(json!({ "command": "ls" }))
            );
        }

        #[test]
        fn flattens_get_available_models_payloads() {
            let data = json!({
                "models": [
                    {
                        "id": "claude-sonnet-4-20250514",
                        "name": "Claude Sonnet 4",
                        "provider": "anthropic",
                        "reasoning": true,
                        "contextWindow": 200000,
                    },
                    { "id": "gpt-4o", "name": "GPT-4o", "provider": "openai", "reasoning": false },
                ],
            });
            let models = models_from_rpc_data(&PI_FLAVOR, Some(&data));
            assert_eq!(
                models
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>(),
                ["pi:anthropic/claude-sonnet-4-20250514", "pi:openai/gpt-4o"]
            );
            assert_eq!(models[0].settings.as_ref().unwrap()[0].id, "thinking");
            assert_eq!(models[0].context_window, Some(200000));
            assert_eq!(models[1].settings, None);
        }

        #[test]
        fn adds_fast_mode_to_omp_models_without_exposing_it_for_pi() {
            let data = json!({
                "models": [{
                    "id": "claude-opus-4-1",
                    "name": "Claude Opus 4.1",
                    "provider": "anthropic",
                    "reasoning": true,
                }],
            });
            let omp = models_from_rpc_data(&OMP_FLAVOR, Some(&data)).remove(0);
            let pi = models_from_rpc_data(&PI_FLAVOR, Some(&data)).remove(0);
            let omp_settings = omp.settings.unwrap();
            assert_eq!(
                omp_settings
                    .iter()
                    .map(|setting| setting.id.as_str())
                    .collect::<Vec<_>>(),
                ["thinking", "fast"]
            );
            let fast = omp_settings
                .iter()
                .find(|setting| setting.id == "fast")
                .unwrap();
            assert_eq!(fast.kind, ModelSettingKind::Toggle);
            assert_eq!(fast.value, "false");
            assert!(
                !pi.settings
                    .unwrap()
                    .iter()
                    .any(|setting| setting.id == "fast")
            );
        }

        #[test]
        fn reads_session_and_context_stats() {
            assert_eq!(
                provider_session_id_from_state(Some(&json!({
                    "sessionId": "abc",
                    "sessionFile": "/tmp/session.jsonl",
                    "model": { "contextWindow": 1000 },
                })))
                .as_deref(),
                Some("abc")
            );
            assert_eq!(
                provider_session_id_from_state(Some(
                    &json!({ "sessionFile": "/tmp/session.jsonl" })
                )),
                None
            );
            let reading = |used, window| ContextReading { used, window };
            assert_eq!(
                context_from_usage(&rec(json!({ "usage": { "totalTokens": 120 } })), Some(200)),
                Some(reading(Some(120), Some(200)))
            );
            // Live 0.80.x shapes: finished total on the assistant message,
            // streaming total on the nested partial. Tool-result usage is a
            // nested LLM call, not the context-window level.
            assert_eq!(
                context_from_usage(
                    &rec(json!({
                        "type": "message_end",
                        "message": { "role": "assistant", "usage": { "totalTokens": 18014 } },
                    })),
                    Some(200000)
                ),
                Some(reading(Some(18014), Some(200000)))
            );
            assert_eq!(
                context_from_usage(
                    &rec(json!({
                        "type": "message_update",
                        "assistantMessageEvent": {
                            "type": "text_delta",
                            "partial": { "usage": { "input": 3, "output": 1, "cacheWrite": 18007 } },
                        },
                    })),
                    Some(200000)
                ),
                Some(reading(Some(18011), Some(200000)))
            );
            assert_eq!(
                context_from_usage(
                    &rec(json!({ "type": "message_end", "message": { "role": "user" } })),
                    Some(200000)
                ),
                None
            );
            assert_eq!(
                context_from_usage(
                    &rec(json!({
                        "type": "message_end",
                        "message": { "role": "toolResult", "usage": { "totalTokens": 150 } },
                    })),
                    Some(200000)
                ),
                None
            );
            assert_eq!(
                context_from_session_stats(Some(&json!({
                    "contextUsage": { "tokens": 60, "contextWindow": 200000, "percent": 30 }
                }))),
                Some(reading(Some(60), Some(200000)))
            );
        }

        #[test]
        fn normalizes_cache_usage_from_assistant_frames() {
            assert_eq!(
                turn_metrics_from_usage(&rec(json!({
                    "usage": { "input": 100, "output": 20, "cacheRead": 300, "cacheWrite": 50 }
                }))),
                Some(TurnMetrics {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                    cache_read_tokens: Some(300),
                    cache_write_tokens: Some(50),
                    cache_hit_percent: Some((300.0 / 450.0) * 100.0),
                    extra: Default::default(),
                })
            );
        }
    }

    mod turn_error_from_event {
        use super::*;

        #[test]
        fn reads_the_reason_a_turn_failed_with_no_content() {
            assert_eq!(
                turn_error_from_event(&rec(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [],
                        "stopReason": "error",
                        "errorMessage": "No API key for provider: openai-codex",
                    },
                })))
                .as_deref(),
                Some("No API key for provider: openai-codex")
            );
        }

        #[test]
        fn still_reports_a_failure_that_carries_no_reason() {
            assert_eq!(
                turn_error_from_event(&rec(json!({
                    "type": "message_end",
                    "message": { "role": "assistant", "stopReason": "error" },
                })))
                .as_deref(),
                Some("")
            );
        }

        #[test]
        fn ignores_healthy_messages_other_roles_and_other_frames() {
            assert_eq!(
                turn_error_from_event(&rec(json!({
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "stopReason": "end_turn",
                        "content": [{ "type": "text", "text": "hi" }],
                    },
                }))),
                None
            );
            assert_eq!(
                turn_error_from_event(&rec(json!({
                    "type": "message_end",
                    "message": { "role": "toolResult", "stopReason": "error", "errorMessage": "tool failed" },
                }))),
                None
            );
            assert_eq!(
                turn_error_from_event(&rec(json!({
                    "type": "turn_end",
                    "message": { "role": "assistant", "stopReason": "error", "errorMessage": "boom" },
                }))),
                None
            );
        }
    }

    /// The Pi cases of src/integrations/harness/core/fileAttachments.test.ts.
    mod file_attachments {
        use super::*;
        use monocode_core::attachment::ATTACHMENT_ONLY_PROMPT;

        type Build = fn(&str, &[Attachment]) -> Result<Rec, String>;

        fn builders() -> [Build; 2] {
            [
                |text, files| build_pi_prompt(text, files, false),
                build_pi_steer,
            ]
        }

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

        fn message(command: &Rec) -> &str {
            command.get("message").and_then(Value::as_str).unwrap()
        }

        #[test]
        fn tells_the_model_to_read_the_attachments_in_the_light_of_the_conversation() {
            for build in builders() {
                let command = build("", &[document()]).unwrap();
                assert!(message(&command).contains(ATTACHMENT_ONLY_PROMPT));
            }
        }

        #[test]
        fn delivers_files_in_pi_and_omp_prompts_and_steering() {
            for (name, mime_type, kind) in [
                ("report.pdf", "application/pdf", AttachmentKind::File),
                ("transcript.md", "text/markdown", AttachmentKind::File),
                ("server.log", "text/plain", AttachmentKind::File),
                ("recording.wav", "audio/wav", AttachmentKind::Audio),
                ("archive.zip", "application/zip", AttachmentKind::File),
            ] {
                let path = format!("/tmp/{name}");
                let file = Attachment {
                    name: name.into(),
                    mime_type: mime_type.into(),
                    kind,
                    path: Some(path.clone()),
                    ..document()
                };
                let expected = format!(
                    "Attached file (read from disk): {}",
                    serde_json::to_string(&path).unwrap()
                );
                for build in builders() {
                    let files = std::slice::from_ref(&file);
                    assert_eq!(
                        message(&build("", files).unwrap()),
                        format!("{ATTACHMENT_ONLY_PROMPT}\n\n{expected}")
                    );
                    assert_eq!(
                        message(&build("Review", files).unwrap()),
                        format!("Review\n\n{expected}")
                    );
                }
            }
        }

        #[test]
        fn preserves_native_images_alongside_documents_without_duplicate_path_inputs() {
            for build in builders() {
                let command = build("Review", &[image(), document()]).unwrap();
                assert_eq!(
                    message(&command),
                    "Review\n\nAttached file (read from disk): \"/tmp/report.pdf\""
                );
                assert_eq!(
                    command.get("images"),
                    Some(&json!([{ "type": "image", "mimeType": "image/png", "data": "YWJj" }]))
                );
            }
        }

        #[test]
        fn falls_back_for_an_image_that_cannot_be_embedded() {
            let large = Attachment {
                data: None,
                size: 21 * 1024 * 1024,
                ..image()
            };
            let svg = Attachment {
                name: "drawing.svg".into(),
                mime_type: "image/svg+xml".into(),
                path: Some("/tmp/drawing.svg".into()),
                ..image()
            };
            for file in [large, svg] {
                let path_text = attachment_path_text(&file).unwrap();
                for build in builders() {
                    let command = build("", std::slice::from_ref(&file)).unwrap();
                    assert_eq!(
                        message(&command),
                        format!("{ATTACHMENT_ONLY_PROMPT}\n\n{path_text}")
                    );
                    assert_eq!(command.get("images"), None);
                }
            }
        }

        #[test]
        fn reports_a_missing_source_instead_of_silently_dropping_an_attachment() {
            let files = [Attachment {
                path: None,
                ..document()
            }];
            for build in builders() {
                let error = build("Review", &files).unwrap_err();
                assert!(error.contains("report.pdf"), "{error}");
                assert!(error.contains("no local file path"), "{error}");
            }
        }
    }
}
