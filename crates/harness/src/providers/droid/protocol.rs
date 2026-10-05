//! Port of src/integrations/harness/providers/droid/droidProtocol.ts: the pure
//! ACP mapping for Factory Droid. Event and permission mapping come from the
//! Grok protocol, as in TypeScript.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use monocode_core::attachment::{Attachment, PromptContentBlock, prompt_blocks};
use monocode_core::block::ModelSettings;
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};

use crate::core::json_rpc::RpcError;
use crate::providers::grok::protocol::{Rec, as_record};

pub const DROID_AUTH_HELP: &str = "Sign in by running `droid` once in Terminal and using /login, or set FACTORY_API_KEY, then retry.";

/// `droid exec --output-format acp` speaks standard ACP over stdio.
pub const DROID_ACP_ARGS: [&str; 3] = ["exec", "--output-format", "acp"];

/// `DroidModeId`: Droid's autonomy levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DroidModeId {
    Normal,
    Spec,
    AutoLow,
    AutoMedium,
    AutoHigh,
}

impl DroidModeId {
    pub const fn as_str(self) -> &'static str {
        match self {
            DroidModeId::Normal => "normal",
            DroidModeId::Spec => "spec",
            DroidModeId::AutoLow => "auto-low",
            DroidModeId::AutoMedium => "auto-medium",
            DroidModeId::AutoHigh => "auto-high",
        }
    }
}

/// One choice of a [`DroidConfigOption`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroidConfigChoice {
    pub value: String,
    pub name: String,
}

/// `DroidConfigOption`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DroidConfigOption {
    pub id: String,
    pub category: Option<String>,
    pub current_value: Option<String>,
    pub options: Vec<DroidConfigChoice>,
}

/// `droidModeId`. Anything the level does not auto-approve comes back as
/// `session/request_permission`, so MonoCode still gets the final say.
pub fn droid_mode_id(runtime_mode: RuntimeMode, planning: bool) -> DroidModeId {
    if planning {
        return DroidModeId::Spec;
    }
    match runtime_mode {
        RuntimeMode::Supervised => DroidModeId::Normal,
        RuntimeMode::AutoAcceptEdits => DroidModeId::AutoLow,
        RuntimeMode::Auto => DroidModeId::AutoMedium,
        RuntimeMode::FullAccess => DroidModeId::AutoHigh,
    }
}

/// `droidPromptBlocks`: Droid accepts the standard ACP text, image, and
/// resource-link blocks.
pub fn droid_prompt_blocks(
    text: &str,
    attachments: &[Attachment],
) -> Result<Vec<PromptContentBlock>, String> {
    prompt_blocks(text, attachments)
}

/// `droidSessionId`.
pub fn droid_session_id(result: &Value) -> Option<String> {
    let rec = result.as_object()?;
    let id = field(rec, "sessionId")
        .or_else(|| field(rec, "session_id"))
        .or_else(|| field(rec, "id"));
    trimmed_string(id)
}

/// `readDroidConfigOptions`.
pub fn read_droid_config_options(raw: &Value) -> Vec<DroidConfigOption> {
    let Some(items) = raw.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let rec = item.as_object()?;
            let id = match rec.get("id") {
                Some(Value::String(id)) if !id.is_empty() => id.clone(),
                _ => return None,
            };
            let options = match rec.get("options") {
                Some(Value::Array(entries)) => entries
                    .iter()
                    .filter_map(|entry| {
                        let option = entry.as_object()?;
                        let value = match option.get("value") {
                            Some(Value::String(value)) if !value.is_empty() => value.clone(),
                            _ => return None,
                        };
                        let name = match option.get("name") {
                            Some(Value::String(name)) => name.clone(),
                            _ => value.clone(),
                        };
                        Some(DroidConfigChoice { value, name })
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Some(DroidConfigOption {
                id,
                category: plain_string(rec.get("category")),
                current_value: plain_string(rec.get("currentValue")),
                options,
            })
        })
        .collect()
}

/// `droidConfigOptionsFrom`: config options from a `session/new` result or a
/// `config_option_update`. `None` when the value carries none.
pub fn droid_config_options_from(value: &Value) -> Option<Vec<DroidConfigOption>> {
    let rec = value.as_object();
    let update = as_record(rec.and_then(|rec| field(rec, "update"))).or(rec)?;
    let raw = field(update, "configOptions").or_else(|| field(update, "config_options"))?;
    raw.is_array().then(|| read_droid_config_options(raw))
}

/// `droidModelConfig`.
pub fn droid_model_config(options: &[DroidConfigOption]) -> Option<&DroidConfigOption> {
    options
        .iter()
        .find(|option| option.id == "model" || option.category.as_deref() == Some("model"))
}

/// `droidEffortConfig`.
pub fn droid_effort_config(options: &[DroidConfigOption]) -> Option<&DroidConfigOption> {
    options.iter().find(|option| {
        option.id == "reasoning_effort" || option.category.as_deref() == Some("thought_level")
    })
}

/// `droidCurrentModelId`.
pub fn droid_current_model_id(result: &Value) -> Option<String> {
    let rec = result.as_object();
    let models = as_record(rec.and_then(|rec| field(rec, "models")));
    let value = models
        .and_then(|models| {
            field(models, "currentModelId").or_else(|| field(models, "current_model_id"))
        })
        .cloned()
        .or_else(|| {
            let options = droid_config_options_from(result).unwrap_or_default();
            droid_model_config(&options)
                .and_then(|config| config.current_value.clone())
                .map(Value::String)
        });
    trimmed_string(value.as_ref())
}

/// `droidEffortSetting`. Droid's reasoning levels differ per model (`off` or
/// `none`, up to `max`). Expose them as MonoCode's standard `effort` select;
/// one choice is no choice.
pub fn droid_effort_setting(config: Option<&DroidConfigOption>) -> Option<ModelSetting> {
    let config = config.filter(|config| config.options.len() >= 2)?;
    let value = match config.current_value.as_deref() {
        Some(current)
            if !current.is_empty()
                && config.options.iter().any(|option| option.value == current) =>
        {
            current.to_string()
        }
        _ => config.options[0].value.clone(),
    };
    Some(ModelSetting {
        id: "effort".into(),
        label: "Reasoning".into(),
        kind: ModelSettingKind::Select,
        value,
        options: config
            .options
            .iter()
            .map(|option| ModelSettingChoice {
                value: option.value.clone(),
                label: option.name.clone(),
            })
            .collect(),
        description: None,
    })
}

/// `droidEffortValue`: MonoCode's effort value mapped onto what this Droid
/// model accepts.
pub fn droid_effort_value(
    config: Option<&DroidConfigOption>,
    settings: Option<&ModelSettings>,
) -> Option<String> {
    let wanted = settings
        .and_then(|settings| settings.get("effort").or_else(|| settings.get("reasoning")))
        .map(|value| js::trim(value))
        .filter(|value| !value.is_empty());
    let (config, wanted) = (config?, wanted?);
    let aliases: &[&str] = match wanted {
        "xhigh" | "extra-high" => &["xhigh", "extra-high"],
        "off" => &["off", "none"],
        "none" => &["none", "off"],
        other => return pick(config, &[other]),
    };
    pick(config, aliases)
}

fn pick(config: &DroidConfigOption, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| {
            config
                .options
                .iter()
                .any(|option| option.value == **candidate)
        })
        .map(|candidate| candidate.to_string())
}

/// `modelsFromDroidSession`: Droid's ACP SessionModelState as MonoCode
/// catalog entries, with Droid's current model first.
pub fn models_from_droid_session(
    result: &Value,
    effort_by_model: &HashMap<String, DroidConfigOption>,
) -> Vec<AgentModel> {
    let rec = result.as_object();
    let state = as_record(rec.and_then(|rec| field(rec, "models")));
    let mut raw: Option<Value> = state
        .and_then(|state| {
            field(state, "availableModels").or_else(|| field(state, "available_models"))
        })
        .cloned();
    if !raw.as_ref().is_some_and(Value::is_array) {
        let options = droid_config_options_from(result).unwrap_or_default();
        raw = droid_model_config(&options).map(|config| {
            Value::Array(
                config
                    .options
                    .iter()
                    .map(|option| json!({ "modelId": option.value, "name": option.name }))
                    .collect(),
            )
        });
    }
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for item in &items {
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
        let mut entry = AgentModel::new(
            &format!("droid:{native}"),
            HarnessId::Droid,
            if name.is_empty() { &native } else { name },
        )
        .with_native_id(&native);
        if let Some(effort) = droid_effort_setting(effort_by_model.get(&native)) {
            entry.settings = Some(vec![effort]);
        }
        models.push(entry);
    }

    // Droid's configured default is the right first pick for a new session.
    if let Some(current) = droid_current_model_id(result)
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

/// `droidErrorMessage`. Droid reports failures as a generic `Internal error`
/// with the useful text in `data`, often an HTTP status followed by a JSON
/// body with `detail`.
pub fn droid_error_message(error: &anyhow::Error) -> String {
    let message = error.to_string();
    let data = error
        .downcast_ref::<RpcError>()
        .and_then(|error| error.data.as_ref());
    let Some(Value::String(data)) = data else {
        return message;
    };
    if js::trim(data).is_empty() {
        return message;
    }
    if let Some(brace) = data.find('{')
        && let Ok(Value::Object(body)) = serde_json::from_str::<Value>(&data[brace..])
    {
        let detail = field(&body, "detail")
            .or_else(|| field(&body, "message"))
            .or_else(|| field(&body, "error"));
        if let Some(Value::String(detail)) = detail
            && !js::trim(detail).is_empty()
        {
            return js::trim(detail).to_string();
        }
    }
    js::trim(data).to_string()
}

static ERROR_ECHO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Error: \d{3} \{").unwrap());

/// `isDroidErrorEcho`. Droid also streams a failure as an
/// `Error: <status> {...}` message chunk. The prompt rejection already
/// reports it as a session error.
pub fn is_droid_error_echo(params: &Value) -> bool {
    let update = as_record(params.as_object().and_then(|rec| rec.get("update")));
    let Some(update) = update else {
        return false;
    };
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("agent_message_chunk") {
        return false;
    }
    match as_record(update.get("content")).and_then(|content| content.get("text")) {
        Some(Value::String(text)) => ERROR_ECHO.is_match(text),
        _ => false,
    }
}

static AUTH_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:auth(?:entication)? required|not (?:logged in|authenticated)|unauthori[sz]ed|invalid api key|FACTORY_API_KEY|please (?:log|sign) in)",
    )
    .unwrap()
});
static TIMED_OUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)timed out").unwrap());

/// `isDroidAuthError`.
pub fn is_droid_auth_error(detail: &str) -> bool {
    AUTH_ERROR.is_match(detail)
}

/// `droidStartupError`.
pub fn droid_startup_error(error: &anyhow::Error) -> anyhow::Error {
    let detail = droid_error_message(error);
    if is_droid_auth_error(&detail) {
        return anyhow::anyhow!("{}\n\n{DROID_AUTH_HELP}", js::trim(&detail));
    }
    if TIMED_OUT.is_match(&detail) {
        return anyhow::anyhow!("Factory Droid did not start. {DROID_AUTH_HELP}");
    }
    anyhow::anyhow!("Factory Droid did not start. {detail}")
}

static SPEC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)spec").unwrap());

/// `droidSpecPlan`: the plan Droid attaches when leaving spec mode
/// (ExitSpecMode).
pub fn droid_spec_plan(params: &Value) -> Option<String> {
    let rec = params.as_object();
    let tool = as_record(rec.and_then(|rec| field(rec, "toolCall")))
        .or_else(|| as_record(rec.and_then(|rec| field(rec, "tool_call"))))?;
    let raw = as_record(field(tool, "rawInput")).or_else(|| as_record(field(tool, "raw_input")));
    let title = js_string_or_empty(field(tool, "title"));
    let is_spec = tool.get("kind").and_then(Value::as_str) == Some("switch_mode")
        || SPEC.is_match(&title)
        || raw.is_some_and(|raw| field(raw, "plan").is_some());
    if !is_spec {
        return None;
    }
    for key in ["plan", "spec", "content", "markdown"] {
        if let Some(Value::String(value)) = raw.and_then(|raw| raw.get(key))
            && !js::trim(value).is_empty()
        {
            return Some(js::trim(value).to_string());
        }
    }
    let content: &[Value] = tool
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let text = content
        .iter()
        .map(|item| {
            let block = as_record(item.as_object().and_then(|item| item.get("content")))
                .or_else(|| item.as_object());
            match block.and_then(|block| block.get("text")) {
                Some(Value::String(text)) => text.clone(),
                _ => String::new(),
            }
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let text = js::trim(&text);
    (!text.is_empty()).then(|| text.to_string())
}

fn field<'a>(rec: &'a Rec, key: &str) -> Option<&'a Value> {
    rec.get(key).filter(|value| !value.is_null())
}

/// `typeof value === "string" ? value : undefined`.
fn plain_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// A string with something in it, trimmed.
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
