//! Port of src/integrations/harness/providers/fx/fxProtocol.ts: the pure ACP
//! mapping for fx.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use monocode_core::attachment::PromptContentBlock;
use monocode_core::block::{TaskListItem, ToolPreview, TurnMetrics};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent};
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};
use monocode_core::task_list::normalize_task_list_status;

use monocode_core::reducer::{
    ToolTitleInput, compose_tool_title, extract_search_query, extract_shell_command,
    extract_skill_name, extract_tool_preview,
};

use super::tool::{fx_tool_info, fx_tool_verb};
use crate::core::acp_subagents::acp_agent_info;

/// `Record<string, unknown>`.
pub type Rec = Map<String, Value>;

/// `FxModeId`.
pub const FX_MODE_ASK: &str = "ask";
pub const FX_MODE_CODE: &str = "code";

/// `FxPermissionRequest`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FxPermissionRequest {
    pub title: String,
    pub kind: Option<String>,
    pub call_id: Option<String>,
    pub preview: Option<ToolPreview>,
    pub option_ids: Vec<String>,
}

/// `SessionConfigOption`. `current_value` is a JSON string or boolean.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionConfigOption {
    pub id: String,
    pub category: Option<String>,
    pub current_value: Option<Value>,
}

impl SessionConfigOption {
    /// `String(option.currentValue ?? "")`.
    pub fn current_text(&self) -> String {
        match &self.current_value {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Bool(flag)) => flag.to_string(),
            _ => String::new(),
        }
    }
}

fn effort_label(value: &str) -> Option<&'static str> {
    Some(match value {
        "auto" => "Auto",
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        _ => return None,
    })
}

/// `fxPromptBlocks`: text only. fx accepts no image or audio blocks.
pub fn fx_prompt_blocks(text: &str) -> Vec<PromptContentBlock> {
    let trimmed = js::trim(text);
    if trimmed.is_empty() {
        return Vec::new();
    }
    vec![PromptContentBlock::Text {
        text: trimmed.to_string(),
    }]
}

/// `fxModeId`: fx always runs in its own `code` (auto) mode.
///
/// fx defaults to `ask`, which stops and requests permission for every read,
/// list, and command. MonoCode does not show those prompts, so a turn that
/// touched a single file would park forever with nothing on screen. fx has a
/// working auto mode, so MonoCode uses it and lets fx police itself.
pub fn fx_mode_id(_runtime_mode: RuntimeMode) -> &'static str {
    FX_MODE_CODE
}

/// `autoPermissionOption`: approve anything fx still asks about. `set_mode`
/// can fail, and fx keeps a few prompts even in code mode, so this backstop
/// keeps a turn from blocking on an approval nobody can see.
pub fn auto_permission_option(_runtime_mode: RuntimeMode, option_ids: &[String]) -> Option<String> {
    if option_ids.is_empty() {
        return None;
    }
    pick_option(
        option_ids,
        &[
            "allow-always",
            "allow_always",
            "allow-once",
            "allow_once",
            "allow",
        ],
    )
}

/// `permissionOptionId`.
pub fn permission_option_id(decision: ApprovalDecision, option_ids: &[String]) -> String {
    if decision == ApprovalDecision::Allow {
        return pick_option(
            option_ids,
            &[
                "allow-once",
                "allow_once",
                "allow-always",
                "allow_always",
                "allow",
            ],
        )
        .unwrap_or_else(|| "allow-once".into());
    }
    pick_option(
        option_ids,
        &[
            "reject-once",
            "reject_once",
            "reject-always",
            "reject_always",
            "reject",
            "deny",
        ],
    )
    .unwrap_or_else(|| "reject-once".into())
}

/// `permissionRequestFromAcp`.
pub fn permission_request_from_acp(params: &Value) -> FxPermissionRequest {
    let rec = params.as_object();
    let subject = as_record(rec.and_then(|rec| field(rec, "subject")));
    let empty = Rec::new();
    let tool = as_record(rec.and_then(|rec| field(rec, "toolCall")))
        .or_else(|| as_record(rec.and_then(|rec| field(rec, "tool_call"))))
        .or_else(|| as_record(subject.and_then(|subject| field(subject, "toolCall"))))
        .or(subject)
        .or(rec)
        .unwrap_or(&empty);
    let command = opt_string_field(subject, "command").map(str::to_string);
    let kind = string_field(tool, "kind")
        .or_else(|| opt_string_field(subject, "kind"))
        .map(str::to_string);
    let fx = fx_tool_info(tool, tool);
    let preview = if fx.resolved {
        fx.preview.clone()
    } else {
        fx.preview
            .clone()
            .or_else(|| extract_tool_preview(tool, tool))
    };
    let label_value = tool_label(tool);
    let label = fx
        .title
        .clone()
        .or_else(|| fx_tool_verb(label_value.as_deref()).map(str::to_string))
        .or_else(|| label_value.clone())
        .or_else(|| command.clone())
        .or_else(|| opt_string_field(rec, "title").map(str::to_string));
    let tool_value = Value::Object(tool.clone());
    let shell = command
        .clone()
        .or_else(|| extract_shell_command(&[&tool_value]));
    let skill = extract_skill_name(&[&tool_value]);
    let query = preview
        .as_ref()
        .and_then(|preview| preview.query.clone())
        .or_else(|| extract_search_query(&tool_value));
    let composed = compose_tool_title(&ToolTitleInput {
        kind: kind.as_deref(),
        title: label.as_deref(),
        command: shell.as_deref(),
        skill: skill.as_deref(),
        path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
        query: query.as_deref(),
        preview_kind: preview.as_ref().map(|preview| preview.kind),
        cwd: None,
    });
    let title = if composed.is_empty() {
        "Permission".to_string()
    } else {
        composed
    };
    let option_ids = rec
        .and_then(|rec| rec.get("options"))
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|item| {
                    let item = item.as_object()?;
                    match field(item, "optionId").or_else(|| field(item, "option_id")) {
                        Some(Value::String(id)) => Some(id.clone()),
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let call_id = string_field(tool, "toolCallId")
        .or_else(|| string_field(tool, "tool_call_id"))
        .or_else(|| opt_string_field(rec, "toolCallId"))
        .or_else(|| opt_string_field(subject, "toolCallId"))
        .map(str::to_string);
    FxPermissionRequest {
        title,
        kind,
        call_id,
        preview,
        option_ids,
    }
}

/// `eventsFromAcpUpdate`.
pub fn events_from_acp_update(params: &Value) -> Vec<HarnessEvent> {
    let rec = params.as_object();
    let Some(update) = as_record(rec.and_then(|rec| field(rec, "update"))).or(rec) else {
        return Vec::new();
    };
    let kind = js_string_or_empty(
        field(update, "sessionUpdate")
            .or_else(|| field(update, "session_update"))
            .or_else(|| field(update, "type")),
    );

    if kind == "agent_message_chunk" || kind == "agent_message" {
        let text = text_from_content(
            field(update, "content").or_else(|| field(update, "text")),
            if kind == "agent_message" { "\n" } else { "" },
        );
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![HarnessEvent::MessageDelta { text }]
        };
    }

    if kind == "agent_thought_chunk" || kind == "agent_thought" {
        let text = text_from_content(
            field(update, "content").or_else(|| field(update, "text")),
            if kind == "agent_thought" { "\n" } else { "" },
        );
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![HarnessEvent::ReasoningDelta { text }]
        };
    }

    if kind == "tool_call" || kind == "tool_call_update" || kind == "tool_call_content_chunk" {
        let tool = as_record(field(update, "toolCall"))
            .or_else(|| as_record(field(update, "tool_call")))
            .unwrap_or(update);
        let call_id = js_string_or_empty(
            field(tool, "toolCallId")
                .or_else(|| field(tool, "tool_call_id"))
                .or_else(|| field(update, "toolCallId"))
                .or_else(|| field(update, "tool_call_id")),
        );
        if call_id.is_empty() {
            return Vec::new();
        }
        let status = string_field(update, "status")
            .or_else(|| string_field(tool, "status"))
            .map(str::to_string);
        // fx sends no rawInput, locations, or diff, so the generic ACP
        // extraction has nothing to work with. Mine the result instead, and
        // fall back to the shared path only if fx starts sending structured
        // fields.
        let fx = fx_tool_info(update, tool);
        let tool_kind = fx
            .kind
            .clone()
            .or_else(|| string_field(update, "kind").map(str::to_string))
            .or_else(|| string_field(tool, "kind").map(str::to_string));
        let preview = if fx.resolved {
            fx.preview.clone()
        } else {
            fx.preview
                .clone()
                .or_else(|| extract_tool_preview(update, tool))
        };
        let update_value = Value::Object(update.clone());
        let inputs = [
            field(update, "rawInput"),
            field(tool, "rawInput"),
            field(update, "raw_input"),
            field(tool, "raw_input"),
            field(update, "input"),
            field(tool, "input"),
            Some(&update_value),
        ];
        let present: Vec<&Value> = inputs.iter().flatten().copied().collect();
        let command = extract_shell_command(&present);
        let skill = extract_skill_name(&present);
        let query = preview
            .as_ref()
            .and_then(|preview| preview.query.clone())
            .or_else(|| {
                inputs[..6]
                    .iter()
                    .flatten()
                    .next()
                    .and_then(|input| extract_search_query(input))
            });
        let update_label = tool_label(update);
        let tool_label_value = tool_label(tool);
        let composed = compose_tool_title(&ToolTitleInput {
            kind: tool_kind.as_deref(),
            title: fx
                .title
                .as_deref()
                .or(update_label.as_deref())
                .or(tool_label_value.as_deref()),
            command: command.as_deref(),
            skill: skill.as_deref(),
            path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
            query: query.as_deref(),
            preview_kind: preview.as_ref().map(|preview| preview.kind),
            cwd: None,
        });
        let title = Some(composed)
            .filter(|title| !title.is_empty())
            .or_else(|| fx.title.clone())
            .or(update_label)
            .or(tool_label_value);
        let detail = Some(cap(fx.detail.as_deref().unwrap_or("")))
            .filter(|detail| !detail.is_empty())
            .or_else(|| tool_detail(update, tool));
        let agent = acp_agent_info(update, tool, tool_kind.as_deref(), title.as_deref(), None);
        let (kind, title, agent_model) = match agent {
            Some(agent) => (
                Some("agent".to_string()),
                Some(agent.title),
                agent.agent_model,
            ),
            None => (tool_kind, title, None),
        };
        return vec![HarnessEvent::ToolUpdated {
            agent_model,
            call_id,
            title,
            kind,
            status,
            detail,
            preview,
            paths: None,
        }];
    }

    if kind == "plan" || kind == "current_plan" {
        return plan_event(update).into_iter().collect();
    }

    usage_from_update(update)
}

/// `modelsFromFxOutput`: `fx models --json`, or its text list as a fallback.
pub fn models_from_fx_output(stdout: &str) -> Vec<AgentModel> {
    let trimmed = js::trim(stdout);
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(raw) = serde_json::from_str::<Value>(trimmed) {
        return unique_fx_models(models_from_fx_json(&raw));
    }
    let start = trimmed.find('{');
    let array_start = trimmed.find('[');
    let json_at = match (start, array_start) {
        (Some(start), None) => Some(start),
        (Some(start), Some(array)) if start < array => Some(start),
        (_, array) => array,
    };
    if let Some(at) = json_at
        && let Ok(raw) = serde_json::from_str::<Value>(&trimmed[at..])
    {
        return unique_fx_models(models_from_fx_json(&raw));
    }
    unique_fx_models(models_from_fx_text(trimmed))
}

/// `modelFromFxStatusOutput`: fx can omit its active, TUI-selected model
/// from `models --json`.
pub fn model_from_fx_status_output(stdout: &str) -> Option<AgentModel> {
    let trimmed = js::trim(stdout);
    if trimmed.is_empty() {
        return None;
    }
    let raw = match serde_json::from_str::<Value>(trimmed) {
        Ok(raw) => raw,
        Err(_) => {
            let start = trimmed.find('{')?;
            serde_json::from_str::<Value>(&trimmed[start..]).ok()?
        }
    };
    let model = opt_string_field(raw.as_object(), "model")?;
    model_from_json(&Value::String(model.to_string()))
}

/// `mergeFxCatalogModels`.
pub fn merge_fx_catalog_models(
    models: Vec<AgentModel>,
    active: Option<AgentModel>,
) -> Vec<AgentModel> {
    let mut all = models;
    all.extend(active);
    unique_fx_models(all)
}

/// `modelsFromFxJson`.
pub fn models_from_fx_json(raw: &Value) -> Vec<AgentModel> {
    let rec = raw.as_object();
    let list: &[Value] = match raw {
        Value::Array(items) => items,
        _ => ["models", "ids", "data"]
            .into_iter()
            .find_map(|key| rec.and_then(|rec| rec.get(key)).and_then(Value::as_array))
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    };
    list.iter().filter_map(model_from_json).collect()
}

/// `readConfigOptions`.
pub fn read_config_options(raw: &Value) -> Vec<SessionConfigOption> {
    let Some(items) = raw.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let rec = item.as_object();
            let id = js_string_or_empty(
                rec.and_then(|rec| field(rec, "id").or_else(|| field(rec, "configId"))),
            );
            let id = js::trim(&id).to_string();
            if id.is_empty() {
                return None;
            }
            let rec = rec?;
            Some(SessionConfigOption {
                id,
                category: rec
                    .get("category")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                current_value: rec
                    .get("currentValue")
                    .filter(|value| value.is_string() || value.is_boolean())
                    .cloned(),
            })
        })
        .collect()
}

/// `extractModelConfigId`.
pub fn extract_model_config_id(options: &[SessionConfigOption]) -> String {
    if let Some(exact) = options.iter().find(|option| option.id == "model") {
        return exact.id.clone();
    }
    // fx lists provider first with category "model"; that is not the model picker.
    options
        .iter()
        .find(|option| option.category.as_deref() == Some("model") && option.id != "provider")
        .map(|option| option.id.clone())
        .unwrap_or_else(|| "model".into())
}

/// `resolveSettingConfigId`.
pub fn resolve_setting_config_id(
    options: &[SessionConfigOption],
    setting_id: &str,
) -> Option<String> {
    let needle = js::trim(setting_id).to_lowercase();
    if let Some(exact) = options
        .iter()
        .find(|option| option.id.to_lowercase() == needle)
    {
        return Some(exact.id.clone());
    }
    if needle == "effort" || needle == "reasoning" {
        return options
            .iter()
            .find(|option| {
                option.id == "effort"
                    || option.id == "reasoning"
                    || option.category.as_deref() == Some("thought_level")
            })
            .map(|option| option.id.clone());
    }
    if needle == "fast" || needle == "fastmode" || needle == "fast_mode" {
        return options
            .iter()
            .find(|option| {
                option.id == "fast"
                    || option.id == "fast_mode"
                    || option.id.to_lowercase().contains("fast")
            })
            .map(|option| option.id.clone());
    }
    None
}

/// `sessionIdFromResult`.
pub fn session_id_from_result(result: &Value) -> Option<String> {
    let rec = result.as_object()?;
    let id = field(rec, "sessionId")
        .or_else(|| field(rec, "session_id"))
        .or_else(|| field(rec, "id"));
    match id {
        Some(Value::String(id)) if !js::trim(id).is_empty() => Some(js::trim(id).to_string()),
        _ => None,
    }
}

fn model_from_json(item: &Value) -> Option<AgentModel> {
    let Some(rec) = item.as_object() else {
        let native = js::trim(item.as_str()?);
        if native.is_empty() {
            return None;
        }
        return Some(
            AgentModel::new(&format!("fx:{native}"), HarnessId::Fx, native).with_native_id(native),
        );
    };
    let native = js_string_or_empty(
        field(rec, "id")
            .or_else(|| field(rec, "modelId"))
            .or_else(|| field(rec, "model_id"))
            .or_else(|| field(rec, "model"))
            .or_else(|| field(rec, "value")),
    );
    let native = js::trim(&native);
    if native.is_empty() {
        return None;
    }
    let name = js_string_or_empty(
        field(rec, "name")
            .or_else(|| field(rec, "displayName"))
            .or_else(|| field(rec, "title")),
    );
    let name = js::trim(&name);
    let settings = settings_from_json(rec);
    let window = number_field(rec, "contextWindow")
        .or_else(|| number_field(rec, "context_window"))
        .or_else(|| number_field(rec, "window"));
    let name = if name.is_empty() {
        display_name(native)
    } else {
        name.to_string()
    };
    let mut model =
        AgentModel::new(&format!("fx:{native}"), HarnessId::Fx, &name).with_native_id(native);
    model.settings = (!settings.is_empty()).then_some(settings);
    model.context_window = window
        .filter(|window| *window != 0.0)
        .map(|window| window as i64);
    Some(model)
}

fn settings_from_json(rec: &Rec) -> Vec<ModelSetting> {
    let mut settings = Vec::new();
    let effort_options = effort_choices(rec);
    if effort_options.len() > 1 {
        settings.push(ModelSetting {
            id: "effort".into(),
            label: "Effort".into(),
            kind: ModelSettingKind::Select,
            value: string_field(rec, "effort")
                .map(str::to_string)
                .unwrap_or_else(|| effort_options[0].value.clone()),
            options: effort_options,
            description: None,
        });
    }
    let flag = |key: &str| rec.get(key) == Some(&Value::Bool(true));
    if flag("fast") || flag("fast_mode") || flag("supportsFast") {
        settings.push(ModelSetting {
            id: "fast".into(),
            label: "Fast".into(),
            kind: ModelSettingKind::Toggle,
            value: if flag("fast_mode") || flag("fast") {
                "true"
            } else {
                "false"
            }
            .into(),
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
            description: None,
        });
    }
    settings
}

fn effort_choices(rec: &Rec) -> Vec<ModelSettingChoice> {
    let raw = field(rec, "effortOptions")
        .or_else(|| field(rec, "effort_options"))
        .or_else(|| field(rec, "efforts"))
        .or_else(|| field(rec, "supportedEffort"));
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(text) if !js::trim(text).is_empty() => Some(js::trim(text).to_string()),
            Value::Object(nested) => string_field(nested, "value").map(str::to_string),
            _ => None,
        })
        .map(|value| ModelSettingChoice {
            label: effort_label(&value)
                .map(str::to_string)
                .unwrap_or_else(|| value.clone()),
            value,
        })
        .collect()
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap());
static TEXT_MODEL_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+)\s+[—-]\s+(.+)$").unwrap());
static DEFAULT_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\(default\)\s*$").unwrap());

fn models_from_fx_text(stdout: &str) -> Vec<AgentModel> {
    let mut models = Vec::new();
    for raw in stdout.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let stripped = ANSI.replace_all(raw, "");
        let line = js::trim(&stripped);
        let Some(captures) = TEXT_MODEL_LINE.captures(line) else {
            continue;
        };
        let native = &captures[1];
        let name = DEFAULT_SUFFIX.replace(&captures[2], "");
        let name = js::trim(&name);
        if native.is_empty() || name.is_empty() {
            continue;
        }
        models.push(
            AgentModel::new(&format!("fx:{native}"), HarnessId::Fx, name).with_native_id(native),
        );
    }
    models
}

fn unique_fx_models(models: Vec<AgentModel>) -> Vec<AgentModel> {
    let mut seen = HashSet::new();
    models
        .into_iter()
        .filter(|model| seen.insert(model.id.clone()))
        .collect()
}

static DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_]+").unwrap());

fn display_name(native: &str) -> String {
    let slug = match native.find('/') {
        Some(slash) => &native[slash + 1..],
        None => native,
    };
    upper_word_starts(&DASHES.replace_all(slug, " "))
}

fn usage_from_update(update: &Rec) -> Vec<HarnessEvent> {
    let has_usage_fields = ["used", "usedTokens", "inputTokens"]
        .iter()
        .any(|key| number_field(update, key).is_some());
    let usage = as_record(field(update, "usage"))
        .or_else(|| as_record(field(update, "tokenUsage")))
        .or_else(|| as_record(field(update, "token_usage")))
        .or_else(|| has_usage_fields.then_some(update));
    let Some(usage) = usage else {
        return Vec::new();
    };
    let first = |keys: &[&str]| keys.iter().find_map(|key| number_field(usage, key));
    let used = first(&["used", "usedTokens", "used_tokens"]).or_else(|| {
        sum_numbers(
            usage,
            &[
                "inputTokens",
                "outputTokens",
                "input_tokens",
                "output_tokens",
            ],
        )
    });
    let window = first(&[
        "window",
        "size",
        "contextWindow",
        "context_window",
        "maxTokens",
        "max_tokens",
    ]);
    let mut events = Vec::new();
    if used.is_some() || window.is_some() {
        events.push(HarnessEvent::Context {
            used: used.map(|value| value as i64),
            window: window.map(|value| value as i64),
        });
    }
    let input_tokens = first(&["inputTokens", "input_tokens"]);
    let output_tokens = first(&["outputTokens", "output_tokens"]);
    let cache_read_tokens = first(&["cacheReadTokens", "cache_read_input_tokens"]);
    let cache_write_tokens = first(&["cacheWriteTokens", "cache_creation_input_tokens"]);
    let cache_reported = cache_read_tokens.is_some() || cache_write_tokens.is_some();
    if input_tokens.is_some()
        || output_tokens.is_some()
        || cache_read_tokens.is_some()
        || cache_write_tokens.is_some()
    {
        let cacheable_input = input_tokens.unwrap_or(0.0)
            + cache_read_tokens.unwrap_or(0.0)
            + cache_write_tokens.unwrap_or(0.0);
        events.push(HarnessEvent::TurnMetrics(TurnMetrics {
            input_tokens: input_tokens.map(|value| value as i64),
            output_tokens: output_tokens.map(|value| value as i64),
            cache_read_tokens: cache_read_tokens.map(|value| value as i64),
            cache_write_tokens: cache_write_tokens.map(|value| value as i64),
            cache_hit_percent: (cache_reported && cacheable_input > 0.0)
                .then(|| cache_read_tokens.unwrap_or(0.0) / cacheable_input * 100.0),
            extra: Map::new(),
        }));
    }
    events
}

fn plan_event(update: &Rec) -> Option<HarnessEvent> {
    let entries = field(update, "entries").or_else(|| field(update, "plan"));
    if let Some(Value::Array(entries)) = entries {
        let items = entries
            .iter()
            .filter_map(|item| {
                let rec = item.as_object()?;
                let content = js_string_or_empty(
                    field(rec, "content")
                        .or_else(|| field(rec, "text"))
                        .or_else(|| field(rec, "title")),
                );
                let content = js::trim(&content);
                if content.is_empty() {
                    return None;
                }
                Some(TaskListItem {
                    id: None,
                    text: content.to_string(),
                    status: normalize_task_list_status(rec.get("status")),
                    extra: Map::new(),
                })
            })
            .collect();
        return Some(HarnessEvent::TasksUpdated {
            key: None,
            explanation: None,
            merge: None,
            authoritative: None,
            provider_session_id: None,
            items,
        });
    }
    match update.get("text") {
        Some(Value::String(text)) if !js::trim(text).is_empty() => Some(HarnessEvent::Plan {
            text: text.clone(),
            key: None,
            append: None,
            streaming: None,
        }),
        _ => None,
    }
}

fn tool_label(rec: &Rec) -> Option<String> {
    ["title", "name", "toolName", "tool_name"]
        .into_iter()
        .find_map(|key| human_field(rec, key))
}

fn tool_detail(update: &Rec, tool: &Rec) -> Option<String> {
    let mut content = text_from_content(field(update, "content"), "\n");
    if content.is_empty() {
        content = text_from_content(field(tool, "content"), "\n");
    }
    if !js::trim(&content).is_empty() {
        return Some(cap(&content));
    }
    let output = field(update, "rawOutput").or_else(|| field(tool, "rawOutput"));
    if let Some(Value::String(output)) = output
        && !js::trim(output).is_empty()
    {
        return Some(cap(output));
    }
    let output_text = text_from_content(output, "");
    (!js::trim(&output_text).is_empty()).then(|| cap(&output_text))
}

fn cap(value: &str) -> String {
    const MAX: usize = 8_000;
    let text = js::trim(value);
    if js::len(text) <= MAX {
        return text.to_string();
    }
    format!("{}\n…", js::slice_prefix(text, MAX))
}

fn pick_option(option_ids: &[String], preferred: &[&str]) -> Option<String> {
    preferred
        .iter()
        .find(|id| option_ids.iter().any(|option| option == *id))
        .map(|id| id.to_string())
}

fn human_field(rec: &Rec, key: &str) -> Option<String> {
    let value = string_field(rec, key)?;
    (!looks_like_call_id(value)).then(|| value.to_string())
}

static CALL_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(call[-_]?|tool[-_])[a-z0-9_-]+$").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap()
});

fn looks_like_call_id(value: &str) -> bool {
    let text = js::trim(value);
    CALL_ID.is_match(text) || UUID.is_match(text)
}

fn text_from_content(content: Option<&Value>, separator: &str) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(rec)) => match rec.get("text") {
            Some(Value::String(text)) => text.clone(),
            _ => match field(rec, "content") {
                Some(inner) => text_from_content(Some(inner), separator),
                None => String::new(),
            },
        },
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text_from_content(Some(item), separator))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(separator),
        _ => String::new(),
    }
}

/// `text.replace(/\b\w/g, (ch) => ch.toUpperCase())`. `\w` is ASCII only.
fn upper_word_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_word = false;
    for c in text.chars() {
        let word = c.is_ascii_alphanumeric() || c == '_';
        out.push(if word && !previous_word {
            c.to_ascii_uppercase()
        } else {
            c
        });
        previous_word = word;
    }
    out
}

fn field<'a>(rec: &'a Rec, key: &str) -> Option<&'a Value> {
    rec.get(key).filter(|value| !value.is_null())
}

/// `asRecord`.
pub fn as_record(value: Option<&Value>) -> Option<&Rec> {
    value.and_then(Value::as_object)
}

/// `stringField`.
pub fn string_field<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    match rec.get(key) {
        Some(Value::String(value)) if !js::trim(value).is_empty() => Some(value),
        _ => None,
    }
}

fn opt_string_field<'a>(rec: Option<&'a Rec>, key: &str) -> Option<&'a str> {
    rec.and_then(|rec| string_field(rec, key))
}

fn number_field(rec: &Rec, key: &str) -> Option<f64> {
    rec.get(key).and_then(Value::as_f64)
}

fn sum_numbers(rec: &Rec, keys: &[&str]) -> Option<f64> {
    let mut total = 0.0;
    let mut found = false;
    for key in keys {
        if let Some(value) = number_field(rec, key) {
            total += value;
            found = true;
        }
    }
    found.then_some(total)
}

fn js_string_or_empty(value: Option<&Value>) -> String {
    value
        .map(crate::core::json_text::js_string)
        .unwrap_or_default()
}

/// `{ outcome: { outcome: "selected", optionId } }`.
pub fn selected_outcome(option_id: &str) -> Value {
    json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
