//! Port of src/integrations/harness/providers/hermes/hermesProtocol.ts: the
//! pure ACP mapping for Hermes Agent. Event and permission mapping come from
//! the Grok protocol, as in TypeScript.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use monocode_core::attachment::{Attachment, PromptContentBlock, prompt_blocks};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::js;
use monocode_core::models::AgentModel;

use crate::providers::grok::protocol::{Rec, as_record, upper_word_starts};

/// `HermesBackgroundDispatch`: detached subagents a `delegate_task` call left
/// running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesBackgroundDispatch {
    pub call_id: String,
    pub delegation_id: String,
    pub transcripts: Vec<String>,
}

pub const HERMES_AUTH_HELP: &str =
    "Configure Hermes with `hermes model`, then verify it with `hermes acp --check`.";

static NOISE_LEVEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\[(?:debug|info|warning)\]").unwrap());
static LOG_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\d{4}-\d{2}-\d{2}[^\[]*\[(?:error|critical)\]\s*(?:[A-Za-z0-9_.]+:\s*)?")
        .unwrap()
});
static AUTH_FAILURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:not authenticated|authentication (?:required|failed)|unauthorized|(?:missing|invalid|expired|revoked) (?:api key|credential|token)|no (?:llm )?provider (?:is )?configured|provider.+(?:not configured|requires authentication))",
    )
    .unwrap()
});

/// `hermesStderrAuthError`. Hermes writes ordinary provider-health warnings
/// to stderr. Only explicit authentication failures reach the conversation; a
/// broad match on words such as `credential` also catches logger names like
/// `agent.credential_pool` and makes successful turns look broken.
pub fn hermes_stderr_auth_error(line: &str) -> Option<String> {
    let detail = js::trim(line);
    if detail.is_empty() || NOISE_LEVEL.is_match(detail) {
        return None;
    }
    let message = LOG_PREFIX.replace(detail, "");
    if !AUTH_FAILURE.is_match(&message) {
        return None;
    }
    Some(format!("{message}\n\n{HERMES_AUTH_HELP}"))
}

/// `hermesPromptBlocks`: Hermes accepts the standard ACP text, image, and
/// resource-link blocks.
pub fn hermes_prompt_blocks(
    text: &str,
    attachments: &[Attachment],
) -> Result<Vec<PromptContentBlock>, String> {
    prompt_blocks(text, attachments)
}

/// `hermesModeId`: MonoCode access levels as Hermes edit-approval modes.
pub fn hermes_mode_id(runtime_mode: RuntimeMode, planning: bool) -> &'static str {
    if planning || runtime_mode == RuntimeMode::Supervised {
        return "default";
    }
    if runtime_mode == RuntimeMode::AutoAcceptEdits {
        return "accept_edits";
    }
    "dont_ask"
}

static SETUP_DETAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)auth|credential|api key|provider|configure|setup").unwrap());
static TIMED_OUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)timed out").unwrap());

/// `hermesStartupError`.
pub fn hermes_startup_error(detail: &str) -> anyhow::Error {
    if SETUP_DETAIL.is_match(detail) {
        return anyhow::anyhow!("{}\n\n{HERMES_AUTH_HELP}", js::trim(detail));
    }
    if TIMED_OUT.is_match(detail) {
        return anyhow::anyhow!("Hermes Agent did not start. {HERMES_AUTH_HELP}");
    }
    anyhow::anyhow!("Hermes Agent did not start. {detail}")
}

/// `hermesSessionId`.
pub fn hermes_session_id(result: &Value) -> Option<String> {
    let rec = result.as_object()?;
    trimmed_string(
        field(rec, "sessionId")
            .or_else(|| field(rec, "session_id"))
            .or_else(|| field(rec, "id")),
    )
}

/// `hermesCurrentModelId`.
pub fn hermes_current_model_id(result: &Value) -> Option<String> {
    let models = as_record(result.as_object().and_then(|rec| field(rec, "models")))?;
    trimmed_string(field(models, "currentModelId").or_else(|| field(models, "current_model_id")))
}

/// `hermesBackgroundDispatch`. Hermes' ACP adapter completes `delegate_task`
/// as soon as detached children are dispatched. The JSON handle is wrapped in
/// the tool call's text content, so recover it before the shared ACP parser
/// caps the detail for display.
pub fn hermes_background_dispatch(params: &Value) -> Option<HermesBackgroundDispatch> {
    let envelope = params.as_object();
    let update = as_record(envelope.and_then(|rec| field(rec, "update"))).or(envelope)?;
    let tool = as_record(field(update, "toolCall"))
        .or_else(|| as_record(field(update, "tool_call")))
        .unwrap_or(update);
    let call_id = js_string_or_empty(
        field(tool, "toolCallId")
            .or_else(|| field(tool, "tool_call_id"))
            .or_else(|| field(update, "toolCallId"))
            .or_else(|| field(update, "tool_call_id")),
    );
    let call_id = js::trim(&call_id).to_string();
    if call_id.is_empty() {
        return None;
    }

    let mut candidates: Vec<&Value> = [
        update.get("rawOutput"),
        update.get("raw_output"),
        tool.get("rawOutput"),
        tool.get("raw_output"),
    ]
    .into_iter()
    .flatten()
    .collect();
    collect_content(update.get("content"), &mut candidates);
    if !std::ptr::eq(tool, update) {
        collect_content(tool.get("content"), &mut candidates);
    }

    for candidate in candidates {
        let Some(dispatch) = dispatch_record(candidate) else {
            continue;
        };
        let delegation_id =
            match field(&dispatch, "delegation_id").or_else(|| field(&dispatch, "delegationId")) {
                Some(value) => crate::core::json_text::js_string(value),
                None => call_id.clone(),
            };
        let delegation_id = js::trim(&delegation_id).to_string();
        let transcripts: Vec<String> = match field(&dispatch, "live_transcripts")
            .or_else(|| field(&dispatch, "liveTranscripts"))
        {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .filter(|path| !js::trim(path).is_empty())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        if delegation_id.is_empty() || transcripts.is_empty() {
            continue;
        }
        return Some(HermesBackgroundDispatch {
            call_id,
            delegation_id,
            transcripts,
        });
    }
    None
}

/// `modelsFromHermesSession`: the standard ACP SessionModelState from Hermes
/// `session/new`, with the model active in Hermes first.
pub fn models_from_hermes_session(result: &Value) -> Vec<AgentModel> {
    let state = as_record(result.as_object().and_then(|rec| field(rec, "models")));
    let raw = state.and_then(|state| {
        field(state, "availableModels").or_else(|| field(state, "available_models"))
    });
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };

    let current = hermes_current_model_id(result);
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for item in items {
        let Some(model) = item.as_object() else {
            continue;
        };
        let native = js_string_or_empty(
            field(model, "modelId")
                .or_else(|| field(model, "model_id"))
                .or_else(|| field(model, "value"))
                .or_else(|| field(model, "id")),
        );
        let native = js::trim(&native).to_string();
        if native.is_empty() || !seen.insert(native.clone()) {
            continue;
        }
        let name = match field(model, "name").or_else(|| field(model, "title")) {
            Some(value) => crate::core::json_text::js_string(value),
            None => native.clone(),
        };
        let name = js::trim(&name);
        let name = if name.is_empty() {
            display_name(&native)
        } else {
            name.to_string()
        };
        models.push(
            AgentModel::new(&format!("hermes:{native}"), HarnessId::Hermes, &name)
                .with_native_id(&native),
        );
    }

    // The model active in Hermes is the right default for a newly selected
    // provider. `set_harness_models` picks the first model for generic catalogs.
    if let Some(current) = current
        && let Some(index) = models
            .iter()
            .position(|model| model.native_id.as_deref() == Some(current.as_str()))
        && index > 0
    {
        let model = models.remove(index);
        models.insert(0, model);
    }
    models
}

static DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_]+").unwrap());

fn display_name(native: &str) -> String {
    let slug = match native.rfind(':') {
        Some(colon) => &native[colon + 1..],
        None => native,
    };
    upper_word_starts(&DASHES.replace_all(slug, " "))
}

fn collect_content<'a>(value: Option<&'a Value>, output: &mut Vec<&'a Value>) {
    match value {
        Some(text @ Value::String(_)) => output.push(text),
        Some(Value::Array(items)) => {
            for item in items {
                collect_content(Some(item), output);
            }
        }
        Some(Value::Object(rec)) => {
            if let Some(text) = field(rec, "text") {
                collect_content(Some(text), output);
            }
            if let Some(content) = field(rec, "content") {
                collect_content(Some(content), output);
            }
        }
        _ => {}
    }
}

fn dispatch_record(value: &Value) -> Option<Rec> {
    let rec = match value {
        Value::Object(rec) => rec.clone(),
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(rec)) => rec,
            _ => return None,
        },
        _ => return None,
    };
    let is = |key: &str, expected: &str| rec.get(key).and_then(Value::as_str) == Some(expected);
    (is("status", "dispatched") && is("mode", "background")).then_some(rec)
}

fn field<'a>(rec: &'a Rec, key: &str) -> Option<&'a Value> {
    rec.get(key).filter(|value| !value.is_null())
}

fn trimmed_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !js::trim(text).is_empty() => Some(js::trim(text).to_string()),
        _ => None,
    }
}

fn js_string_or_empty(value: Option<&Value>) -> String {
    value
        .map(crate::core::json_text::js_string)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
