//! Port of src/integrations/harness/providers/antigravity/antigravityProtocol.ts:
//! the pure half of the Antigravity ACP adapter. It maps runtime modes,
//! picks permission options, reads config options and models, and turns
//! `session/update` notifications into harness events.

use std::collections::HashMap;

use monocode_core::attachment::{Attachment, PromptContentBlock, prompt_blocks};
use monocode_core::block::{TaskListItem, ToolPreview, TurnMetrics};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent};
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};
use monocode_core::reducer::{
    ToolTitleInput, compose_tool_title, extract_search_query, extract_shell_command,
    extract_skill_name, extract_tool_preview,
};
use monocode_core::task_list::normalize_task_list_status;
use serde_json::{Map, Value};

use crate::core::acp_subagents::{AcpAgentInfo, acp_agent_info};
use crate::core::json_text::js_string;

/// `Record<string, unknown>`.
pub type Rec = Map<String, Value>;

/// `AntigravityModeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AntigravityModeId {
    Default,
    AutoEdit,
    Yolo,
}

impl AntigravityModeId {
    pub const fn as_str(self) -> &'static str {
        match self {
            AntigravityModeId::Default => "default",
            AntigravityModeId::AutoEdit => "auto_edit",
            AntigravityModeId::Yolo => "yolo",
        }
    }
}

/// `AntigravityPermissionRequest`.
#[derive(Debug, Clone, PartialEq)]
pub struct AntigravityPermissionRequest {
    pub title: String,
    pub kind: Option<String>,
    pub call_id: Option<String>,
    pub preview: Option<ToolPreview>,
    pub option_ids: Vec<String>,
    pub option_kinds: HashMap<String, String>,
}

/// `currentValue` of a session config option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValue {
    String(String),
    Bool(bool),
}

impl ConfigValue {
    /// `String(currentValue)`.
    pub fn as_js_string(&self) -> String {
        match self {
            ConfigValue::String(value) => value.clone(),
            ConfigValue::Bool(value) => value.to_string(),
        }
    }
}

/// `SessionConfigOption`. `kind` is the TypeScript `type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfigOption {
    pub id: String,
    pub category: Option<String>,
    pub kind: Option<String>,
    pub current_value: Option<ConfigValue>,
}

/// `asRecord`.
pub fn as_record(value: Option<&Value>) -> Option<&Rec> {
    match value {
        Some(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// An object value, kept as `&Value` for the reducer's `unknown` helpers.
fn object(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| value.is_object())
}

/// `stringField`: a string with something besides whitespace, untrimmed.
pub fn string_field<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    match rec.get(key) {
        Some(Value::String(value)) if !js::trim(value).is_empty() => Some(value),
        _ => None,
    }
}

/// `numberField`: a finite number.
fn number_field(rec: &Rec, key: &str) -> Option<f64> {
    rec.get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

/// `rec[key]`, with JSON `null` read as missing, as `??` does.
fn nn<'a>(rec: &'a Rec, key: &str) -> Option<&'a Value> {
    rec.get(key).filter(|value| !value.is_null())
}

/// `String(value ?? "")`.
fn js_string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value) => js_string(value),
    }
}

/// The present values of a list, for helpers that took `...values: unknown[]`.
fn present<'a>(values: &[Option<&'a Value>]) -> Vec<&'a Value> {
    values
        .iter()
        .filter_map(|value| value.filter(|value| !value.is_null()))
        .collect()
}

/// `antigravityPromptBlocks`.
pub fn antigravity_prompt_blocks(
    text: &str,
    attachments: &[Attachment],
) -> Result<Vec<PromptContentBlock>, String> {
    prompt_blocks(text, attachments)
}

/// `antigravitySpawnCwd`: the raw `.par` finds sibling resources relative to
/// its process directory.
pub fn antigravity_spawn_cwd(binary: &str, fallback: &str) -> String {
    match binary.rfind(['/', '\\']) {
        Some(separator) => binary[..=separator].to_string(),
        None => fallback.to_string(),
    }
}

/// `antigravityModeId`. There is no native plan mode: planning uses default
/// and the client denies non-read permissions.
pub fn antigravity_mode_id(runtime_mode: RuntimeMode, planning: bool) -> AntigravityModeId {
    if planning {
        return AntigravityModeId::Default;
    }
    match runtime_mode {
        RuntimeMode::FullAccess => AntigravityModeId::Yolo,
        RuntimeMode::AutoAcceptEdits => AntigravityModeId::AutoEdit,
        _ => AntigravityModeId::Default,
    }
}

/// `autoPermissionOption`.
pub fn auto_permission_option(
    runtime_mode: RuntimeMode,
    kind: Option<&str>,
    option_ids: &[String],
    option_kinds: &HashMap<String, String>,
) -> Option<String> {
    if runtime_mode != RuntimeMode::FullAccess
        && !(runtime_mode == RuntimeMode::AutoAcceptEdits && kind == Some("edit"))
    {
        return None;
    }
    permission_option_id(ApprovalDecision::Allow, option_ids, option_kinds)
}

/// `permissionOptionId`. ACP option ids are opaque, so the advertised
/// semantic kind wins over the id's spelling.
pub fn permission_option_id(
    decision: ApprovalDecision,
    option_ids: &[String],
    option_kinds: &HashMap<String, String>,
) -> Option<String> {
    let kinds: [&str; 2] = match decision {
        ApprovalDecision::Allow => ["allow_once", "allow_always"],
        ApprovalDecision::Deny => ["reject_once", "reject_always"],
    };
    for kind in kinds {
        if let Some(id) = option_ids
            .iter()
            .find(|id| option_kinds.get(id.as_str()).map(String::as_str) == Some(kind))
        {
            return Some(id.clone());
        }
    }
    let preferred: &[&str] = match decision {
        ApprovalDecision::Allow => &[
            "allow-once",
            "allow_once",
            "allow-always",
            "allow_always",
            "allow",
        ],
        ApprovalDecision::Deny => &[
            "reject-once",
            "reject_once",
            "reject-always",
            "reject_always",
            "reject",
            "deny",
        ],
    };
    pick_option(option_ids, preferred)
}

/// `pickOption`.
fn pick_option(option_ids: &[String], preferred: &[&str]) -> Option<String> {
    preferred
        .iter()
        .find(|id| option_ids.iter().any(|option| option == *id))
        .map(|id| id.to_string())
}

/// `optionId ?? option_id` on a permission option.
fn option_id_of(option: &Rec) -> Option<&Value> {
    nn(option, "optionId").or_else(|| nn(option, "option_id"))
}

/// `permissionRequestFromAcp`.
pub fn permission_request_from_acp(params: &Value) -> AntigravityPermissionRequest {
    static EMPTY: std::sync::LazyLock<Value> =
        std::sync::LazyLock::new(|| Value::Object(Map::new()));
    let rec = params.as_object();
    let field = |key: &str| rec.and_then(|rec| rec.get(key));
    let subject_value = object(field("subject"));
    let subject = as_record(subject_value);
    let tool_value = object(field("toolCall"))
        .or_else(|| object(field("tool_call")))
        .or_else(|| object(subject.and_then(|subject| subject.get("toolCall"))))
        .or(subject_value)
        .or(rec.map(|_| params))
        .unwrap_or(&EMPTY);
    let tool = tool_value.as_object().expect("an object value");
    let empty = Rec::new();
    let subject_or_empty = subject.unwrap_or(&empty);
    let command = string_field(subject_or_empty, "command");
    let kind = string_field(tool, "kind").or_else(|| string_field(subject_or_empty, "kind"));
    let preview = extract_tool_preview(tool, tool);
    let label = tool_label(tool)
        .or_else(|| command.map(str::to_string))
        .or_else(|| {
            rec.and_then(|rec| string_field(rec, "title"))
                .map(str::to_string)
        });
    let shell_command = command
        .map(str::to_string)
        .or_else(|| extract_shell_command(&[tool_value]));
    let skill = extract_skill_name(&[tool_value]);
    let query = preview
        .as_ref()
        .and_then(|preview| preview.query.clone())
        .or_else(|| extract_search_query(tool_value));
    let title = compose_tool_title(&ToolTitleInput {
        kind,
        title: label.as_deref(),
        command: shell_command.as_deref(),
        skill: skill.as_deref(),
        path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
        query: query.as_deref(),
        preview_kind: preview.as_ref().map(|preview| preview.kind),
        cwd: None,
    });
    let options: &[Value] = match field("options") {
        Some(Value::Array(options)) => options,
        _ => &[],
    };
    let option_ids = options
        .iter()
        .filter_map(|item| {
            item.as_object()
                .and_then(option_id_of)?
                .as_str()
                .map(str::to_string)
        })
        .collect();
    let mut option_kinds = HashMap::new();
    for option in options.iter().filter_map(Value::as_object) {
        if let (Some(Value::String(id)), Some(Value::String(kind))) =
            (option_id_of(option), option.get("kind"))
        {
            option_kinds.insert(id.clone(), kind.clone());
        }
    }
    AntigravityPermissionRequest {
        title: if title.is_empty() {
            "Permission".into()
        } else {
            title
        },
        kind: kind.map(str::to_string),
        call_id: string_field(tool, "toolCallId")
            .or_else(|| string_field(tool, "tool_call_id"))
            .or_else(|| rec.and_then(|rec| string_field(rec, "toolCallId")))
            .or_else(|| string_field(subject_or_empty, "toolCallId"))
            .map(str::to_string),
        preview,
        option_ids,
        option_kinds,
    }
}

/// `eventsFromAcpUpdate`.
pub fn events_from_acp_update(params: &Value) -> Vec<HarnessEvent> {
    let rec = params.as_object();
    let update_value = object(rec.and_then(|rec| rec.get("update"))).or(rec.map(|_| params));
    let Some(update_value) = update_value else {
        return Vec::new();
    };
    let update = update_value.as_object().expect("an object value");
    let kind = ["sessionUpdate", "session_update", "type"]
        .iter()
        .find_map(|key| nn(update, key))
        .map(|value| js_string_or_empty(Some(value)))
        .unwrap_or_default();
    let body = || nn(update, "content").or_else(|| nn(update, "text"));

    if kind == "agent_message_chunk" || kind == "agent_message" {
        let text = text_from_content(body(), if kind == "agent_message" { "\n" } else { "" });
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![HarnessEvent::MessageDelta { text }]
        };
    }

    if kind == "agent_thought_chunk" || kind == "agent_thought" {
        let text = text_from_content(body(), if kind == "agent_thought" { "\n" } else { "" });
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![HarnessEvent::ReasoningDelta { text }]
        };
    }

    if kind == "tool_call" || kind == "tool_call_update" || kind == "tool_call_content_chunk" {
        let tool_value = object(update.get("toolCall"))
            .or_else(|| object(update.get("tool_call")))
            .unwrap_or(update_value);
        let tool = tool_value.as_object().expect("an object value");
        let call_id = [
            nn(tool, "toolCallId"),
            nn(tool, "tool_call_id"),
            nn(update, "toolCallId"),
            nn(update, "tool_call_id"),
        ]
        .into_iter()
        .flatten()
        .next()
        .map(|value| js_string_or_empty(Some(value)))
        .unwrap_or_default();
        if call_id.is_empty() {
            return Vec::new();
        }
        let status = string_field(update, "status").or_else(|| string_field(tool, "status"));
        let tool_kind = string_field(update, "kind").or_else(|| string_field(tool, "kind"));
        let preview = extract_tool_preview(update, tool);
        let inputs = present(&[
            update.get("rawInput"),
            tool.get("rawInput"),
            update.get("raw_input"),
            tool.get("raw_input"),
            update.get("input"),
            tool.get("input"),
            Some(update_value),
        ]);
        let first_input = ["rawInput", "raw_input", "input"]
            .iter()
            .find_map(|key| nn(update, key).or_else(|| nn(tool, key)));
        let label = tool_label(update).or_else(|| tool_label(tool));
        let query = preview
            .as_ref()
            .and_then(|preview| preview.query.clone())
            .or_else(|| first_input.and_then(extract_search_query));
        let composed = compose_tool_title(&ToolTitleInput {
            kind: tool_kind,
            title: label.as_deref(),
            command: extract_shell_command(&inputs).as_deref(),
            skill: extract_skill_name(&inputs).as_deref(),
            path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
            query: query.as_deref(),
            preview_kind: preview.as_ref().map(|preview| preview.kind),
            cwd: None,
        });
        let title = if composed.is_empty() {
            label
        } else {
            Some(composed)
        };
        let agent = acp_agent_info(update, tool, tool_kind, title.as_deref(), None);
        let (title, kind, agent_model) = match agent {
            Some(AcpAgentInfo { title, agent_model }) => (
                Some(title),
                Some(AcpAgentInfo::KIND.to_string()),
                agent_model,
            ),
            None => (title, tool_kind.map(str::to_string), None),
        };
        return vec![HarnessEvent::ToolUpdated {
            agent_model,
            call_id,
            title,
            kind,
            status: status.map(str::to_string),
            detail: tool_detail(update, tool),
            preview,
            paths: None,
        }];
    }

    if kind == "plan" || kind == "current_plan" {
        return plan_event(update).into_iter().collect();
    }

    usage_from_update(update)
}

/// `modelsFromSessionNew`: models and reasoning controls from the standard
/// ACP session config.
pub fn models_from_session_new(raw: &Value) -> Vec<AgentModel> {
    let rec = raw.as_object();
    let options: &[Value] = match rec.and_then(|rec| rec.get("configOptions")) {
        Some(Value::Array(options)) => options,
        _ => &[],
    };
    let model_id =
        extract_model_config_id(&read_config_options(Some(&Value::Array(options.to_vec()))));
    let model_option = options
        .iter()
        .filter_map(Value::as_object)
        .find(|item| item.get("id").and_then(Value::as_str) == Some(model_id.as_str()));
    let choices = config_choices(model_option.and_then(|option| option.get("options")));
    let mut settings = Vec::new();
    for option in options.iter().filter_map(Value::as_object) {
        if option.get("category").and_then(Value::as_str) != Some("thought_level") {
            continue;
        }
        let values = config_choices(option.get("options"));
        if values.len() < 2 {
            continue;
        }
        let current = js_string_or_empty(option.get("currentValue"));
        settings.push(ModelSetting {
            id: "effort".into(),
            label: "Effort".into(),
            kind: ModelSettingKind::Select,
            value: if values.iter().any(|choice| choice.value == current) {
                current
            } else {
                values[0].value.clone()
            },
            options: values,
            description: None,
        });
    }
    let models = if !choices.is_empty() {
        choices
    } else {
        let available: &[Value] = match rec
            .and_then(|rec| rec.get("models"))
            .and_then(|models| models.get("availableModels"))
        {
            Some(Value::Array(available)) => available,
            _ => &[],
        };
        available
            .iter()
            .filter_map(|item| {
                let model = item.as_object();
                let empty = Rec::new();
                let value = string_field(model.unwrap_or(&empty), "modelId")?.to_string();
                let label = match model.and_then(|model| nn(model, "name")) {
                    Some(name) => js_string(name),
                    None => value.clone(),
                };
                Some(ModelSettingChoice { value, label })
            })
            .collect()
    };
    // `new Map(models.map(...))`: a repeated value keeps its first place and
    // its last label.
    let mut unique: Vec<ModelSettingChoice> = Vec::new();
    for choice in models {
        match unique
            .iter_mut()
            .find(|existing| existing.value == choice.value)
        {
            Some(existing) => *existing = choice,
            None => unique.push(choice),
        }
    }
    unique
        .into_iter()
        .map(|choice| {
            let mut model = AgentModel::new(
                &format!("antigravity:{}", choice.value),
                HarnessId::Antigravity,
                &choice.label,
            );
            model.native_id = Some(choice.value);
            if !settings.is_empty() {
                model.settings = Some(settings.clone());
            }
            model
        })
        .collect()
}

/// `configChoices`: select values, with grouped options flattened.
fn config_choices(raw: Option<&Value>) -> Vec<ModelSettingChoice> {
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    items
        .iter()
        .flat_map(|item| {
            let rec = item.as_object();
            if let Some(Value::Array(_)) = rec.and_then(|rec| rec.get("options")) {
                return config_choices(rec.and_then(|rec| rec.get("options")));
            }
            let empty = Rec::new();
            let Some(value) = string_field(rec.unwrap_or(&empty), "value") else {
                return Vec::new();
            };
            let label = match rec.and_then(|rec| nn(rec, "name").or_else(|| nn(rec, "label"))) {
                Some(label) => js_string(label),
                None => value.to_string(),
            };
            vec![ModelSettingChoice {
                value: value.to_string(),
                label,
            }]
        })
        .collect()
}

/// `readConfigOptions`.
pub fn read_config_options(raw: Option<&Value>) -> Vec<SessionConfigOption> {
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let rec = item.as_object();
            let id = rec.and_then(|rec| nn(rec, "id").or_else(|| nn(rec, "configId")));
            let id = js::trim(&js_string_or_empty(id)).to_string();
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
                kind: rec.get("type").and_then(Value::as_str).map(str::to_string),
                current_value: match rec.get("currentValue") {
                    Some(Value::String(value)) => Some(ConfigValue::String(value.clone())),
                    Some(Value::Bool(value)) => Some(ConfigValue::Bool(*value)),
                    _ => None,
                },
            })
        })
        .collect()
}

/// `extractModelConfigId`. A model selector is never a boolean option, and a
/// provider selector is not the model picker.
pub fn extract_model_config_id(options: &[SessionConfigOption]) -> String {
    let selectable = |option: &SessionConfigOption| option.kind.as_deref() != Some("boolean");
    if let Some(exact) = options
        .iter()
        .find(|option| option.id == "model" && selectable(option))
    {
        return exact.id.clone();
    }
    options
        .iter()
        .find(|option| {
            option.category.as_deref() == Some("model")
                && option.id != "provider"
                && selectable(option)
        })
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
    let found = match needle.as_str() {
        "effort" | "reasoning" => options.iter().find(|option| {
            option.id == "effort"
                || option.id == "reasoning"
                || option.category.as_deref() == Some("thought_level")
        }),
        "fast" | "fastmode" | "fast_mode" => options.iter().find(|option| {
            option.id == "fast"
                || option.id == "fast_mode"
                || option.id.to_lowercase().contains("fast")
        }),
        _ => None,
    };
    found.map(|option| option.id.clone())
}

/// `sessionIdFromResult`.
pub fn session_id_from_result(result: &Value) -> Option<String> {
    let rec = result.as_object()?;
    let id = nn(rec, "sessionId")
        .or_else(|| nn(rec, "session_id"))
        .or_else(|| nn(rec, "id"))?;
    let id = js::trim(id.as_str()?);
    (!id.is_empty()).then(|| id.to_string())
}

const USAGE_FIELDS: [&str; 17] = [
    "used",
    "usedTokens",
    "used_tokens",
    "inputTokens",
    "input_tokens",
    "outputTokens",
    "output_tokens",
    "cacheReadTokens",
    "cache_read_input_tokens",
    "cacheWriteTokens",
    "cache_creation_input_tokens",
    "window",
    "size",
    "contextWindow",
    "context_window",
    "maxTokens",
    "max_tokens",
];

fn first_number(rec: &Rec, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| number_field(rec, key))
}

/// `sumNumbers`.
fn sum_numbers(rec: &Rec, keys: &[&str]) -> Option<f64> {
    let values: Vec<f64> = keys
        .iter()
        .filter_map(|key| number_field(rec, key))
        .collect();
    (!values.is_empty()).then(|| values.iter().sum())
}

/// `usageFromUpdate`: context level and token metrics.
fn usage_from_update(update: &Rec) -> Vec<HarnessEvent> {
    let usage = as_record(update.get("usage"))
        .or_else(|| as_record(update.get("tokenUsage")))
        .or_else(|| as_record(update.get("token_usage")))
        .or_else(|| {
            USAGE_FIELDS
                .iter()
                .any(|key| number_field(update, key).is_some())
                .then_some(update)
        });
    let Some(usage) = usage else {
        return Vec::new();
    };
    let used = first_number(usage, &["used", "usedTokens", "used_tokens"]).or_else(|| {
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
    let window = first_number(
        usage,
        &[
            "window",
            "size",
            "contextWindow",
            "context_window",
            "maxTokens",
            "max_tokens",
        ],
    );
    let mut events = Vec::new();
    if used.is_some() || window.is_some() {
        events.push(HarnessEvent::Context {
            used: used.map(|value| value as i64),
            window: window.map(|value| value as i64),
        });
    }
    let input_tokens = first_number(usage, &["inputTokens", "input_tokens"]);
    let output_tokens = first_number(usage, &["outputTokens", "output_tokens"]);
    let cache_read_tokens = first_number(usage, &["cacheReadTokens", "cache_read_input_tokens"]);
    let cache_write_tokens =
        first_number(usage, &["cacheWriteTokens", "cache_creation_input_tokens"]);
    let cache_reported = cache_read_tokens.is_some() || cache_write_tokens.is_some();
    if input_tokens.is_some() || output_tokens.is_some() || cache_reported {
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
            extra: Default::default(),
        }));
    }
    events
}

/// `planEvent`: a task list from plan entries, else plan text.
fn plan_event(update: &Rec) -> Option<HarnessEvent> {
    let entries = nn(update, "entries").or_else(|| nn(update, "plan"));
    if let Some(Value::Array(entries)) = entries {
        let items = entries
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|rec| {
                let content = js_string_or_empty(
                    nn(rec, "content")
                        .or_else(|| nn(rec, "text"))
                        .or_else(|| nn(rec, "title")),
                );
                let content = js::trim(&content);
                (!content.is_empty()).then(|| TaskListItem {
                    id: None,
                    text: content.to_string(),
                    status: normalize_task_list_status(rec.get("status")),
                    extra: Default::default(),
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
    if let Some(Value::String(text)) = update.get("text")
        && !js::trim(text).is_empty()
    {
        return Some(HarnessEvent::Plan {
            text: text.clone(),
            key: None,
            append: None,
            streaming: None,
        });
    }
    None
}

/// `toolLabel`.
fn tool_label(rec: &Rec) -> Option<String> {
    ["title", "name", "toolName", "tool_name"]
        .iter()
        .find_map(|key| human_field(rec, key))
        .map(str::to_string)
}

/// `toolDetail`.
fn tool_detail(update: &Rec, tool: &Rec) -> Option<String> {
    let mut content = text_from_content(update.get("content"), "\n");
    if content.is_empty() {
        content = text_from_content(tool.get("content"), "\n");
    }
    if !js::trim(&content).is_empty() {
        return Some(cap(&content, 8_000));
    }
    let output = nn(update, "rawOutput").or_else(|| nn(tool, "rawOutput"));
    if let Some(Value::String(output)) = output
        && !js::trim(output).is_empty()
    {
        return Some(cap(output, 8_000));
    }
    let output_text = text_from_content(output, "");
    (!js::trim(&output_text).is_empty()).then(|| cap(&output_text, 8_000))
}

/// `cap`.
fn cap(value: &str, max: usize) -> String {
    let text = js::trim(value);
    if js::len(text) <= max {
        return text.to_string();
    }
    format!("{}\n…", js::slice_prefix(text, max))
}

/// `humanField`.
fn human_field<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    string_field(rec, key).filter(|value| !looks_like_call_id(value))
}

/// `looksLikeCallId`.
fn looks_like_call_id(value: &str) -> bool {
    static CALL_ID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(call[-_]?|tool[-_])[a-z0-9_-]+$").unwrap()
    });
    static UUID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
            .unwrap()
    });
    let text = js::trim(value);
    CALL_ID.is_match(text) || UUID.is_match(text)
}

/// `textFromContent` in antigravityProtocol.ts: array parts join with the
/// separator.
fn text_from_content(content: Option<&Value>, separator: &str) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(rec)) => {
            if let Some(Value::String(text)) = rec.get("text") {
                return text.clone();
            }
            match nn(rec, "content") {
                Some(nested) => text_from_content(Some(nested), separator),
                None => String::new(),
            }
        }
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text_from_content(Some(item), separator))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(separator),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::attachment::ATTACHMENT_ONLY_PROMPT;
    use monocode_core::harness::{RUNTIME_MODES, harness_supports_attachments};
    use serde_json::json;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn maps_every_runtime_mode_and_overrides_full_access_for_planning() {
        let modes: Vec<&str> = RUNTIME_MODES
            .iter()
            .map(|mode| antigravity_mode_id(*mode, false).as_str())
            .collect();
        assert_eq!(modes, ["default", "auto_edit", "default", "yolo"]);
        for mode in RUNTIME_MODES {
            assert_eq!(antigravity_mode_id(mode, true), AntigravityModeId::Default);
        }
    }

    #[test]
    fn delivers_image_only_prompts_and_leaves_attachments_enabled() {
        let image: Attachment = serde_json::from_value(json!({
            "id": "image", "name": "image.png", "kind": "image", "mimeType": "image/png",
            "size": 4, "data": "aGV5"
        }))
        .unwrap();
        assert!(harness_supports_attachments(HarnessId::Antigravity));
        assert_eq!(
            serde_json::to_value(antigravity_prompt_blocks("", &[image]).unwrap()).unwrap(),
            json!([
                { "type": "text", "text": ATTACHMENT_ONLY_PROMPT },
                { "type": "image", "mimeType": "image/png", "data": "aGV5" }
            ])
        );
        assert_eq!(
            serde_json::to_value(antigravity_prompt_blocks(" hi ", &[]).unwrap()).unwrap(),
            json!([{ "type": "text", "text": "hi" }])
        );
        assert!(antigravity_prompt_blocks("  ", &[]).unwrap().is_empty());
    }

    #[test]
    fn extracts_session_ids_and_rejects_malformed_ids() {
        for key in ["sessionId", "session_id", "id"] {
            assert_eq!(
                session_id_from_result(&json!({ key: " S1 " })).as_deref(),
                Some("S1")
            );
        }
        for raw in [
            json!(null),
            json!({}),
            json!({ "sessionId": " " }),
            json!({ "sessionId": 42 }),
        ] {
            assert_eq!(session_id_from_result(&raw), None);
        }
    }

    #[test]
    fn resolves_config_ids_by_category_without_selecting_the_provider() {
        let options = read_config_options(Some(&json!([
            null, {}, { "id": "provider", "category": "model" },
            { "id": "model_picker", "category": "model", "currentValue": "m1" },
            { "id": "thinking", "category": "thought_level", "currentValue": "high" },
        ])));
        assert_eq!(options.len(), 3);
        assert_eq!(extract_model_config_id(&options), "model_picker");
        assert_eq!(extract_model_config_id(&[]), "model");
        assert_eq!(
            resolve_setting_config_id(&options, "effort").as_deref(),
            Some("thinking")
        );
        assert_eq!(
            resolve_setting_config_id(&options, "reasoning").as_deref(),
            Some("thinking")
        );
        assert_eq!(
            resolve_setting_config_id(&options, "THINKING").as_deref(),
            Some("thinking")
        );
        assert_eq!(resolve_setting_config_id(&options, "missing"), None);
    }

    #[test]
    fn discovers_live_models_and_thinking_levels_including_grouped_choices() {
        let models = models_from_session_new(&json!({ "configOptions": [
            { "id": "model", "category": "model", "options": [
                { "group": "provider", "options": [
                    { "value": "m1", "name": "Model One" }, { "value": "m2", "name": "Model Two" },
                ] },
            ] },
            { "id": "thinking", "category": "thought_level", "currentValue": "high", "options": [
                { "value": "low", "name": "Low" }, { "value": "high", "name": "High" },
                { "value": "max", "name": "Max" },
            ] },
        ] }));
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["antigravity:m1", "antigravity:m2"]
        );
        let first = &models[0];
        assert_eq!(first.harness, HarnessId::Antigravity);
        assert_eq!(first.name, "Model One");
        assert_eq!(first.native_id.as_deref(), Some("m1"));
        let settings = first.settings.as_ref().unwrap();
        assert_eq!(settings[0].id, "effort");
        assert_eq!(settings[0].value, "high");
        assert_eq!(
            serde_json::to_value(&settings[0].options).unwrap(),
            json!([
                { "value": "low", "label": "Low" }, { "value": "high", "label": "High" },
                { "value": "max", "label": "Max" },
            ])
        );
        assert!(models_from_session_new(&Value::Null).is_empty());
    }

    #[test]
    fn falls_back_to_top_level_acp_models_without_inventing_reasoning_controls() {
        let models = models_from_session_new(&json!({ "models": { "availableModels": [
            { "modelId": "gemini-pro-agent", "name": "Gemini 3.1 Pro (High)" },
        ] } }));
        assert_eq!(
            serde_json::to_value(&models).unwrap(),
            json!([{
                "id": "antigravity:gemini-pro-agent", "harness": "antigravity", "nativeId": "gemini-pro-agent",
                "name": "Gemini 3.1 Pro (High)",
            }])
        );
    }

    #[test]
    fn uses_opaque_permission_ids_by_semantic_kind_and_never_fabricates_an_id() {
        let request = permission_request_from_acp(&json!({
            "toolCall": { "toolCallId": "t1", "title": "Write file", "kind": "edit" },
            "options": [
                { "optionId": "yes-7", "kind": "allow_once" },
                { "optionId": "no-9", "kind": "reject_once" },
            ],
        }));
        assert_eq!(request.call_id.as_deref(), Some("t1"));
        assert_eq!(request.kind.as_deref(), Some("edit"));
        let (option_ids, option_kinds) = (&request.option_ids, &request.option_kinds);
        let none = HashMap::new();
        assert_eq!(
            permission_option_id(ApprovalDecision::Allow, option_ids, option_kinds).as_deref(),
            Some("yes-7")
        );
        assert_eq!(
            permission_option_id(ApprovalDecision::Deny, option_ids, option_kinds).as_deref(),
            Some("no-9")
        );
        assert_eq!(
            permission_option_id(ApprovalDecision::Deny, &ids(&["allow_once"]), &none),
            None
        );
        assert_eq!(
            permission_option_id(ApprovalDecision::Allow, &[], &none),
            None
        );
        assert_eq!(
            auto_permission_option(
                RuntimeMode::Supervised,
                Some("edit"),
                option_ids,
                option_kinds
            ),
            None
        );
        assert_eq!(
            auto_permission_option(RuntimeMode::Auto, Some("execute"), option_ids, option_kinds),
            None
        );
        assert_eq!(
            auto_permission_option(
                RuntimeMode::AutoAcceptEdits,
                Some("execute"),
                option_ids,
                option_kinds
            ),
            None
        );
        assert_eq!(
            auto_permission_option(
                RuntimeMode::AutoAcceptEdits,
                Some("edit"),
                option_ids,
                option_kinds
            )
            .as_deref(),
            Some("yes-7")
        );
        assert_eq!(
            auto_permission_option(
                RuntimeMode::FullAccess,
                Some("execute"),
                option_ids,
                option_kinds
            )
            .as_deref(),
            Some("yes-7")
        );
    }

    #[test]
    fn maps_text_reasoning_tools_and_plans_without_fx_specific_result_decoding() {
        let parse = |update: Value| {
            events_from_acp_update(&json!({ "update": update }))
                .iter()
                .map(|event| serde_json::to_value(event).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            parse(
                json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "hi" } })
            ),
            [json!({ "type": "message.delta", "text": "hi" })]
        );
        assert_eq!(
            parse(
                json!({ "sessionUpdate": "agent_thought_chunk", "content": { "text": "think" } })
            ),
            [json!({ "type": "reasoning.delta", "text": "think" })]
        );
        let tools = parse(
            json!({ "sessionUpdate": "tool_call", "toolCallId": "t", "title": "Read file", "kind": "read", "status": "completed" }),
        );
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "tool.updated");
        assert_eq!(tools[0]["callId"], "t");
        assert_eq!(tools[0]["kind"], "read");
        assert_eq!(tools[0]["status"], "completed");
        let plans = parse(
            json!({ "sessionUpdate": "plan", "entries": [{ "content": "Check code", "status": "pending" }] }),
        );
        assert_eq!(plans[0]["type"], "tasks.updated");
        assert_eq!(plans[0]["items"][0]["text"], "Check code");
        assert!(
            parse(json!({ "sessionUpdate": "available_commands_update", "availableCommands": [] }))
                .is_empty()
        );
    }

    #[test]
    fn reads_usage_and_cache_metrics() {
        let events = events_from_acp_update(&json!({ "update": {
            "sessionUpdate": "usage_update", "used": 1200, "size": 200000,
            "inputTokens": 10, "cacheReadTokens": 30
        } }));
        assert_eq!(
            events
                .iter()
                .map(|event| serde_json::to_value(event).unwrap())
                .collect::<Vec<_>>(),
            [
                json!({ "type": "context", "used": 1200, "window": 200000 }),
                json!({ "type": "turn.metrics", "inputTokens": 10, "cacheReadTokens": 30, "cacheHitPercent": 75.0 }),
            ]
        );
        assert_eq!(
            antigravity_spawn_cwd("/fake/agy_acp_server.par", "/repo"),
            "/fake/"
        );
        assert_eq!(antigravity_spawn_cwd("agy", "/repo"), "/repo");
    }

    #[test]
    fn labels_delegation_as_an_agent_row() {
        let events = events_from_acp_update(&json!({ "update": {
            "sessionUpdate": "tool_call", "toolCallId": "a1", "title": "Task", "kind": "other",
            "rawInput": { "_toolName": "task", "description": "Explore auth", "model": "gemini" }
        } }));
        let event = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(event["kind"], "agent");
        assert_eq!(event["title"], "Explore auth");
        assert_eq!(event["agentModel"], "gemini");
    }
}
