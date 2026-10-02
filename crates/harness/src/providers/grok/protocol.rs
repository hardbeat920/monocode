//! Port of src/integrations/harness/providers/grok/grokProtocol.ts: the pure
//! ACP mapping for Grok Build. Droid and Hermes reuse the event, permission,
//! and option helpers here.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use monocode_core::attachment::{Attachment, PromptContentBlock, prompt_blocks};
use monocode_core::block::{
    ModelSettings, TaskListItem, ToolPreview, ToolPreviewKind, TurnMetrics,
};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent};
use monocode_core::js;
use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};
use monocode_core::task_list::normalize_task_list_status;
use monocode_core::user_question::{
    UserQuestion, UserQuestionReply, questions_from_unknown, selected_answer_labels,
};

use monocode_core::reducer::{
    ToolTitleInput, compose_tool_title, extract_search_query, extract_shell_command,
    extract_skill_name, extract_tool_preview,
};

use crate::core::acp_subagents::acp_agent_info;

/// `Record<string, unknown>`.
pub type Rec = Map<String, Value>;

pub const AUTH_HELP: &str =
    "Grok Build is not signed in. Run `grok login` in a terminal, or set XAI_API_KEY.";

pub const TEXT_MODEL: &str = "grok-4.6";

/// `VARIANT_KIND`.
fn variant_kind(key: &str) -> Option<&'static str> {
    Some(match key {
        "readfile" | "read" | "listdir" | "list_dir" => "read",
        "write" | "edit" | "searchreplace" => "edit",
        "bash" | "execute" | "run_terminal_command" => "execute",
        "grep" | "search" | "websearch" | "web_search" => "search",
        "webfetch" | "web_fetch" => "fetch",
        "agent" | "task" | "subagent" => "agent",
        _ => return None,
    })
}

/// `EFFORT_LABELS`.
fn effort_label(value: &str) -> Option<&'static str> {
    Some(match value {
        "xhigh" => "Extra High",
        "high" => "High",
        "medium" => "Medium",
        "low" => "Low",
        _ => return None,
    })
}

/// `GrokPermissionRequest`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrokPermissionRequest {
    pub title: String,
    pub kind: Option<String>,
    pub call_id: Option<String>,
    pub preview: Option<ToolPreview>,
    pub option_ids: Vec<String>,
}

/// `askQuestionsFromAcp`.
pub fn ask_questions_from_acp(params: &Value) -> Vec<UserQuestion> {
    questions_from_unknown(params)
}

/// `askQuestionResponse`.
pub fn ask_question_response(reply: &UserQuestionReply, questions: &[UserQuestion]) -> Value {
    let UserQuestionReply::Answered { answers, custom } = reply else {
        return json!({ "outcome": "skip_interview" });
    };
    let mut out = Map::new();
    for question in questions {
        let labels = selected_answer_labels(question, answers, custom.as_ref());
        if labels.is_empty() {
            continue;
        }
        let value = if question.multi_select {
            json!(labels)
        } else {
            json!(labels.first().cloned().unwrap_or_default())
        };
        out.insert(question.prompt.clone(), value);
    }
    json!({ "outcome": "accepted", "answers": out })
}

/// `grokPromptBlocks`. Grok accepts ACP image blocks despite advertising
/// `image: false`.
pub fn grok_prompt_blocks(
    text: &str,
    attachments: &[Attachment],
) -> Result<Vec<PromptContentBlock>, String> {
    prompt_blocks(text, attachments)
}

/// The input of `grokSpawnArgs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GrokSpawnInput<'a> {
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub full_access: bool,
    pub plan: bool,
}

/// `grokSpawnArgs`: global flags before `agent`, and `stdio` last.
pub fn grok_spawn_args(input: GrokSpawnInput<'_>) -> Vec<String> {
    let mut args = vec!["--no-auto-update".to_string()];
    if input.plan {
        args.extend(["--permission-mode".into(), "plan".into()]);
    }
    args.extend(["agent".into(), "--no-leader".into()]);
    let native = native_id(input.model);
    if !native.is_empty() {
        args.extend(["--model".into(), native.to_string()]);
    }
    if let Some(effort) = input
        .effort
        .map(js::trim)
        .filter(|effort| !effort.is_empty())
    {
        args.extend(["--reasoning-effort".into(), effort.to_string()]);
    }
    if input.full_access {
        args.push("--always-approve".into());
    }
    args.push("stdio".into());
    args
}

/// `grokTextSpawnArgs`.
pub fn grok_text_spawn_args() -> Vec<String> {
    [
        "--no-auto-update",
        "--permission-mode",
        "dontAsk",
        "agent",
        "--no-leader",
        "--model",
        TEXT_MODEL,
        "--reasoning-effort",
        "low",
        "stdio",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// `grokSessionNewParams`: `yoloMode` for full access, `autoMode` for auto.
pub fn grok_session_new_params(cwd: &str, runtime_mode: RuntimeMode) -> Value {
    let mut params = json!({ "cwd": cwd, "mcpServers": [] });
    match runtime_mode {
        RuntimeMode::FullAccess => params["_meta"] = json!({ "yoloMode": true }),
        RuntimeMode::Auto => params["_meta"] = json!({ "autoMode": true }),
        _ => {}
    }
    params
}

/// `grokEffort`.
pub fn grok_effort(settings: Option<&ModelSettings>) -> Option<String> {
    let settings = settings?;
    ["effort", "reasoning"]
        .into_iter()
        .filter_map(|key| settings.get(key))
        .map(|value| js::trim(value))
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// `grokAuthMethodId`. Never pick `grok.com`: that starts a browser OAuth
/// flow with no headless completion path. Prefer an API key when the agent
/// advertised it (it saw XAI_API_KEY), otherwise the cached `grok login` token.
pub fn grok_auth_method_id(init: &Value) -> Option<String> {
    let rec = init.as_object();
    let methods: &[Value] = rec
        .and_then(|rec| rec.get("authMethods"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut ids: Vec<String> = Vec::new();
    for item in methods {
        let Some(Value::String(id)) = item.as_object().and_then(|item| item.get("id")) else {
            continue;
        };
        let trimmed = js::trim(id);
        if !trimmed.is_empty() && id != "grok.com" && !ids.iter().any(|seen| seen == trimmed) {
            ids.push(trimmed.to_string());
        }
    }
    let default_id = opt_string_field(
        as_record(rec.and_then(|rec| rec.get("_meta"))),
        "defaultAuthMethodId",
    );
    let has = |id: &str| ids.iter().any(|seen| seen == id);
    if has("xai.api_key") {
        return Some("xai.api_key".into());
    }
    if let Some(default_id) = default_id
        && has(default_id)
    {
        return Some(default_id.to_string());
    }
    if has("cached_token") {
        return Some("cached_token".into());
    }
    ids.into_iter().next()
}

static AUTH_DETAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)auth|login|credential|api key|XAI_API_KEY").unwrap());
static TIMED_OUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)timed out").unwrap());

/// True when an error message reads like a Grok sign-in failure.
pub fn is_grok_auth_detail(detail: &str) -> bool {
    AUTH_DETAIL.is_match(detail)
}

/// `grokAuthError`.
pub fn grok_auth_error(detail: &str) -> anyhow::Error {
    if AUTH_DETAIL.is_match(detail) {
        return anyhow::anyhow!("{}\n\n{AUTH_HELP}", js::trim(detail));
    }
    if TIMED_OUT.is_match(detail) {
        return anyhow::anyhow!("Grok Build did not start. {AUTH_HELP}");
    }
    anyhow::anyhow!("Grok Build did not start. {detail}")
}

/// `pickAutoOption`.
pub fn pick_auto_option(
    runtime_mode: RuntimeMode,
    kind: Option<&str>,
    option_ids: &[String],
) -> Option<String> {
    if option_ids.is_empty() {
        return None;
    }
    let tool = kind.unwrap_or("").to_lowercase();
    if runtime_mode == RuntimeMode::Supervised {
        return None;
    }
    if runtime_mode == RuntimeMode::AutoAcceptEdits
        && (tool == "execute" || tool == "other" || tool == "fetch")
    {
        return None;
    }
    if runtime_mode == RuntimeMode::FullAccess {
        return pick_option(
            option_ids,
            &[
                "allow-always",
                "allow_always",
                "allow-once",
                "allow_once",
                "allow",
            ],
        );
    }
    pick_option(
        option_ids,
        &[
            "allow-once",
            "allow_once",
            "allow-always",
            "allow_always",
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
pub fn permission_request_from_acp(params: &Value) -> GrokPermissionRequest {
    let rec = params.as_object();
    let subject = as_record(rec.and_then(|rec| field(rec, "subject")));
    let empty = Rec::new();
    let tool = as_record(rec.and_then(|rec| field(rec, "toolCall")))
        .or_else(|| as_record(rec.and_then(|rec| field(rec, "tool_call"))))
        .or_else(|| as_record(subject.and_then(|subject| field(subject, "toolCall"))))
        .or(subject)
        .or(rec)
        .unwrap_or(&empty);
    let grok = grok_tool_fields(tool, tool);
    let kind = grok
        .kind
        .clone()
        .or_else(|| string_field(tool, "kind").map(str::to_string))
        .or_else(|| opt_string_field(subject, "kind").map(str::to_string));
    let preview = extract_tool_preview(tool, tool);
    let tool_value = Value::Object(tool.clone());
    let command = grok
        .command
        .clone()
        .or_else(|| extract_shell_command(&[&tool_value]));
    let skill = extract_skill_name(&[&tool_value]);
    let label = tool_label(tool);
    let query = grok
        .query
        .clone()
        .or_else(|| preview.as_ref().and_then(|preview| preview.query.clone()))
        .or_else(|| extract_search_query(&tool_value));
    let path = grok
        .path
        .clone()
        .or_else(|| preview.as_ref().and_then(|preview| preview.path.clone()));
    let composed = compose_tool_title(&ToolTitleInput {
        kind: kind.as_deref(),
        title: grok.title.as_deref().or(label.as_deref()),
        command: command.as_deref(),
        skill: skill.as_deref(),
        path: path.as_deref(),
        query: query.as_deref(),
        preview_kind: preview.as_ref().map(|preview| preview.kind),
        cwd: None,
    });
    let title = Some(composed)
        .filter(|title| !title.is_empty())
        .or_else(|| grok.title.clone())
        .or(label)
        .unwrap_or_else(|| "Permission".into());
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
    let call_id = grok
        .call_id
        .clone()
        .or_else(|| string_field(tool, "toolCallId").map(str::to_string))
        .or_else(|| string_field(tool, "tool_call_id").map(str::to_string))
        .or_else(|| opt_string_field(rec, "toolCallId").map(str::to_string));
    GrokPermissionRequest {
        title,
        preview: merge_preview(
            preview,
            grok.path.as_deref(),
            grok.query.as_deref(),
            kind.as_deref(),
        ),
        kind,
        call_id,
        option_ids,
    }
}

/// `planFromExitPlan`.
pub fn plan_from_exit_plan(params: &Value) -> String {
    let rec = params.as_object();
    let nested = as_record(rec.and_then(|rec| field(rec, "input")));
    let text = rec
        .and_then(|rec| {
            field(rec, "planContent")
                .or_else(|| field(rec, "plan"))
                .or_else(|| field(rec, "content"))
        })
        .or_else(|| {
            nested.and_then(|nested| field(nested, "plan").or_else(|| field(nested, "planContent")))
        });
    match text {
        Some(Value::String(text)) => js::trim(text).to_string(),
        _ => String::new(),
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

    if kind == "tool_call_delta_chunk" {
        let call_id = string_field(update, "toolCallId")
            .or_else(|| string_field(update, "tool_call_id"))
            .unwrap_or("");
        if call_id.is_empty() {
            return Vec::new();
        }
        let name = string_field(update, "name").or_else(|| string_field(update, "title"));
        return vec![HarnessEvent::ToolUpdated {
            agent_model: None,
            call_id: call_id.to_string(),
            title: name.map(humanize_tool_name),
            kind: kind_from_name(name).map(str::to_string),
            status: Some("pending".into()),
            detail: None,
            preview: None,
            paths: None,
        }];
    }

    if kind == "tool_call" || kind == "tool_call_update" || kind == "tool_call_content_chunk" {
        let tool = as_record(field(update, "toolCall"))
            .or_else(|| as_record(field(update, "tool_call")))
            .unwrap_or(update);
        let grok = grok_tool_fields(update, tool);
        let call_id = match &grok.call_id {
            Some(id) => id.clone(),
            None => js_string_or_empty(
                field(tool, "toolCallId")
                    .or_else(|| field(tool, "tool_call_id"))
                    .or_else(|| field(update, "toolCallId"))
                    .or_else(|| field(update, "tool_call_id")),
            ),
        };
        if call_id.is_empty() {
            return Vec::new();
        }
        let tool_kind = grok
            .kind
            .clone()
            .or_else(|| string_field(update, "kind").map(str::to_string))
            .or_else(|| string_field(tool, "kind").map(str::to_string));
        let status = string_field(update, "status")
            .or_else(|| string_field(tool, "status"))
            .map(str::to_string);
        let preview = merge_preview(
            extract_tool_preview(update, tool),
            grok.path.as_deref(),
            grok.query.as_deref(),
            tool_kind.as_deref(),
        );
        let grok_input = grok.input.clone().map(Value::Object);
        let inputs = [
            field(update, "rawInput"),
            field(tool, "rawInput"),
            field(update, "raw_input"),
            field(tool, "raw_input"),
            field(update, "input"),
            field(tool, "input"),
        ];
        let present: Vec<&Value> = inputs.iter().flatten().copied().collect();
        let command = grok.command.clone().or_else(|| {
            let mut values = present.clone();
            values.extend(grok_input.as_ref());
            extract_shell_command(&values)
        });
        let skill = extract_skill_name(&present);
        let update_label = tool_label(update);
        let tool_label_value = tool_label(tool);
        let query = preview
            .as_ref()
            .and_then(|preview| preview.query.clone())
            .or_else(|| grok.query.clone());
        let composed = compose_tool_title(&ToolTitleInput {
            kind: tool_kind.as_deref(),
            title: grok
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
            .or_else(|| grok.title.clone())
            .or(update_label)
            .or(tool_label_value);
        let detail = Some(cap(&tool_detail(update, tool).unwrap_or_default()))
            .filter(|detail| !detail.is_empty());
        let agent = acp_agent_info(
            update,
            tool,
            tool_kind.as_deref(),
            title.as_deref(),
            grok_input.as_ref(),
        );
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

    if kind == "session_summary_generated" {
        return Vec::new();
    }

    usage_from_update(update)
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

/// `contextWindowFromSetup`.
pub fn context_window_from_setup(result: &Value) -> Option<i64> {
    let mut models = models_from_session_new(result);
    models.extend(models_from_initialize(result));
    let current = current_model_id(result);
    let found = models
        .iter()
        .find(|model| model.native_id.is_some() && model.native_id.as_deref() == current.as_deref())
        .or_else(|| models.first());
    found.and_then(|model| model.context_window)
}

/// `currentModelId`.
pub fn current_model_id(result: &Value) -> Option<String> {
    let rec = result.as_object();
    let models = as_record(rec.and_then(|rec| field(rec, "models")));
    let meta = as_record(rec.and_then(|rec| field(rec, "_meta")));
    let state = as_record(meta.and_then(|meta| field(meta, "modelState")));
    opt_string_field(models, "currentModelId")
        .or_else(|| opt_string_field(state, "currentModelId"))
        .or_else(|| opt_string_field(meta, "currentModelId"))
        .map(str::to_string)
}

/// `modelsFromInitialize`.
pub fn models_from_initialize(result: &Value) -> Vec<AgentModel> {
    let rec = result.as_object();
    let meta = as_record(rec.and_then(|rec| field(rec, "_meta")));
    let state = as_record(meta.and_then(|meta| field(meta, "modelState")));
    models_from_available(
        state
            .and_then(|state| field(state, "availableModels"))
            .or_else(|| rec.and_then(|rec| field(rec, "availableModels"))),
    )
}

/// `modelsFromSessionNew`.
pub fn models_from_session_new(result: &Value) -> Vec<AgentModel> {
    let rec = result.as_object();
    let models = as_record(rec.and_then(|rec| field(rec, "models")));
    models_from_available(
        models
            .and_then(|models| field(models, "availableModels"))
            .or_else(|| {
                as_record(rec.and_then(|rec| field(rec, "_meta")))
                    .and_then(|meta| field(meta, "availableModels"))
            }),
    )
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap());
static MODEL_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[*+\-]\s+(\S+)").unwrap());

/// `modelsFromGrokModelsOutput`: parse `grok models` text.
pub fn models_from_grok_models_output(stdout: &str) -> Vec<AgentModel> {
    let mut models = Vec::new();
    for raw in split_lines(stdout) {
        let stripped = ANSI.replace_all(raw, "");
        let line = js::trim(&stripped);
        let Some(captures) = MODEL_LINE.captures(line) else {
            continue;
        };
        let native = js::trim(&captures[1]);
        if native.is_empty() {
            continue;
        }
        models.push(model_from_native(native, &display_name(native), None));
    }
    unique_grok_models(models)
}

/// `fallbackGrokModels`.
pub fn fallback_grok_models() -> Vec<AgentModel> {
    let choice = |value: &str, label: &str, default: bool| EffortChoice {
        value: value.into(),
        label: label.into(),
        default,
    };
    vec![
        model_from_native(
            "grok-4.6",
            "Grok 4.6",
            Some(ModelExtra {
                context_window: Some(500_000.0),
                efforts: vec![
                    choice("xhigh", "Extra High", false),
                    choice("high", "High", true),
                    choice("medium", "Medium", false),
                    choice("low", "Low", false),
                ],
                default_effort: None,
            }),
        ),
        model_from_native(
            "grok-4.5",
            "Grok 4.5",
            Some(ModelExtra {
                context_window: Some(500_000.0),
                efforts: vec![
                    choice("high", "High", true),
                    choice("medium", "Medium", false),
                    choice("low", "Low", false),
                ],
                default_effort: None,
            }),
        ),
    ]
}

fn models_from_available(raw: Option<&Value>) -> Vec<AgentModel> {
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    let mut models = Vec::new();
    for item in items {
        let Some(rec) = item.as_object() else {
            continue;
        };
        let native = js_string_or_empty(
            field(rec, "modelId")
                .or_else(|| field(rec, "model_id"))
                .or_else(|| field(rec, "id"))
                .or_else(|| field(rec, "value")),
        );
        let native = js::trim(&native);
        if native.is_empty() {
            continue;
        }
        let name = match field(rec, "name").or_else(|| field(rec, "displayName")) {
            Some(value) => json_text_string(value),
            None => native.to_string(),
        };
        let name = js::trim(&name);
        let meta = as_record(field(rec, "_meta")).unwrap_or(rec);
        let window = number_field(meta, "totalContextTokens")
            .or_else(|| number_field(meta, "contextWindow"))
            .or_else(|| number_field(rec, "contextWindow"));
        let efforts = reasoning_efforts(meta);
        let name = if name.is_empty() {
            display_name(native)
        } else {
            name.to_string()
        };
        models.push(model_from_native(
            native,
            &name,
            Some(ModelExtra {
                context_window: window,
                efforts,
                default_effort: string_field(meta, "reasoningEffort").map(str::to_string),
            }),
        ));
    }
    unique_grok_models(models)
}

#[derive(Debug, Clone)]
struct EffortChoice {
    value: String,
    label: String,
    default: bool,
}

#[derive(Debug, Clone, Default)]
struct ModelExtra {
    context_window: Option<f64>,
    efforts: Vec<EffortChoice>,
    default_effort: Option<String>,
}

fn model_from_native(native: &str, name: &str, extra: Option<ModelExtra>) -> AgentModel {
    let extra = extra.unwrap_or_default();
    let settings = (!extra.efforts.is_empty()).then(|| {
        let value = extra
            .default_effort
            .clone()
            .or_else(|| {
                extra
                    .efforts
                    .iter()
                    .find(|item| item.default)
                    .map(|item| item.value.clone())
            })
            .or_else(|| extra.efforts.first().map(|item| item.value.clone()));
        vec![effort_setting(&extra.efforts, value.as_deref())]
    });
    let mut model =
        AgentModel::new(&format!("grok:{native}"), HarnessId::Grok, name).with_native_id(native);
    model.settings = settings;
    // `contextWindow` is set only when it is truthy.
    model.context_window = extra
        .context_window
        .filter(|window| *window != 0.0)
        .map(|window| window as i64);
    model
}

fn effort_setting(options: &[EffortChoice], value: Option<&str>) -> ModelSetting {
    let value = match value {
        Some(value) if !value.is_empty() && options.iter().any(|item| item.value == value) => {
            value.to_string()
        }
        _ => options
            .first()
            .map(|item| item.value.clone())
            .unwrap_or_else(|| "high".into()),
    };
    ModelSetting {
        id: "effort".into(),
        label: "Reasoning".into(),
        kind: ModelSettingKind::Select,
        value,
        options: options
            .iter()
            .map(|item| ModelSettingChoice {
                value: item.value.clone(),
                label: effort_label(&item.value)
                    .map(str::to_string)
                    .unwrap_or_else(|| item.label.clone()),
            })
            .collect(),
        description: None,
    }
}

static EFFORT_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+Effort$").unwrap());

fn reasoning_efforts(meta: &Rec) -> Vec<EffortChoice> {
    let Some(Value::Array(raw)) =
        field(meta, "reasoningEfforts").or_else(|| field(meta, "reasoning_efforts"))
    else {
        return Vec::new();
    };
    raw.iter()
        .filter_map(|item| {
            let rec = item.as_object();
            let value = js_string_or_empty(
                rec.and_then(|rec| field(rec, "value").or_else(|| field(rec, "id"))),
            );
            let value = js::trim(&value).to_string();
            if value.is_empty() {
                return None;
            }
            let label = match rec.and_then(|rec| field(rec, "label")) {
                Some(label) => json_text_string(label),
                None => effort_label(&value)
                    .map(str::to_string)
                    .unwrap_or_else(|| value.clone()),
            };
            let label = js::trim(&EFFORT_SUFFIX.replace(&label, "")).to_string();
            Some(EffortChoice {
                value,
                label,
                default: rec.and_then(|rec| rec.get("default")) == Some(&Value::Bool(true)),
            })
        })
        .collect()
}

/// What `grokToolFields` reads from Grok's own tool metadata.
#[derive(Debug, Clone, Default)]
struct GrokToolFields {
    kind: Option<String>,
    title: Option<String>,
    path: Option<String>,
    command: Option<String>,
    query: Option<String>,
    call_id: Option<String>,
    input: Option<Rec>,
}

static NON_ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

fn grok_tool_fields(update: &Rec, tool: &Rec) -> GrokToolFields {
    let meta = nested_meta(update, "x.ai/tool").or_else(|| nested_meta(tool, "x.ai/tool"));
    let input = as_record(meta.and_then(|meta| field(meta, "input")))
        .or_else(|| as_record(field(update, "rawInput")))
        .or_else(|| as_record(field(update, "raw_input")))
        .or_else(|| as_record(field(tool, "rawInput")))
        .or_else(|| as_record(field(tool, "raw_input")))
        .or_else(|| as_record(field(update, "input")))
        .or_else(|| as_record(field(tool, "input")));
    let variant = js_string_or_empty(
        input
            .and_then(|input| field(input, "variant"))
            .or_else(|| meta.and_then(|meta| field(meta, "name"))),
    )
    .to_lowercase();
    let squashed = NON_ALNUM.replace_all(&variant, "");
    let owned = |value: Option<&str>| value.map(str::to_string);
    GrokToolFields {
        kind: owned(opt_string_field(meta, "kind"))
            .or_else(|| variant_kind(&squashed).map(str::to_string))
            .or_else(|| variant_kind(&variant).map(str::to_string)),
        title: owned(opt_string_field(meta, "label"))
            .or_else(|| owned(string_field(update, "title")))
            .or_else(|| owned(string_field(tool, "title"))),
        path: owned(opt_string_field(input, "path"))
            .or_else(|| owned(opt_string_field(input, "absolute_path")))
            .or_else(|| owned(opt_string_field(input, "file_path"))),
        command: owned(opt_string_field(input, "command")),
        query: owned(opt_string_field(input, "query"))
            .or_else(|| owned(opt_string_field(input, "pattern")))
            .or_else(|| owned(opt_string_field(input, "search"))),
        call_id: owned(string_field(update, "toolCallId"))
            .or_else(|| owned(string_field(update, "tool_call_id")))
            .or_else(|| owned(string_field(tool, "toolCallId")))
            .or_else(|| owned(string_field(tool, "tool_call_id"))),
        input: input.cloned(),
    }
}

fn nested_meta<'a>(rec: &'a Rec, key: &str) -> Option<&'a Rec> {
    as_record(field(rec, "_meta")).and_then(|meta| as_record(field(meta, key)))
}

fn merge_preview(
    preview: Option<ToolPreview>,
    path: Option<&str>,
    query: Option<&str>,
    kind: Option<&str>,
) -> Option<ToolPreview> {
    if let Some(query) = query.filter(|query| !query.is_empty())
        && preview.as_ref().is_none_or(|preview| {
            preview.kind == ToolPreviewKind::Search
                || preview.path.as_deref().is_none_or(str::is_empty)
        })
    {
        let mut next = preview.unwrap_or_else(|| ToolPreview::new(ToolPreviewKind::Search));
        next.query = Some(query.to_string());
        return Some(next);
    }
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return preview;
    };
    let file_name = basename(path);
    if let Some(mut preview) = preview {
        preview.path.get_or_insert_with(|| path.to_string());
        preview.file_name.get_or_insert(file_name);
        return Some(preview);
    }
    let mut next = ToolPreview::new(preview_kind(kind));
    next.path = Some(path.to_string());
    next.file_name = Some(file_name);
    Some(next)
}

fn preview_kind(kind: Option<&str>) -> ToolPreviewKind {
    match kind.unwrap_or("").to_lowercase().as_str() {
        "execute" | "shell" => ToolPreviewKind::Shell,
        "search" | "fetch" => ToolPreviewKind::Search,
        "edit" | "write" => ToolPreviewKind::Write,
        _ => ToolPreviewKind::Read,
    }
}

const USAGE_FIELDS: [&str; 14] = [
    "used",
    "usedTokens",
    "used_tokens",
    "totalTokens",
    "inputTokens",
    "input_tokens",
    "outputTokens",
    "output_tokens",
    "window",
    "size",
    "contextWindow",
    "context_window",
    "maxTokens",
    "max_tokens",
];

fn usage_from_update(update: &Rec) -> Vec<HarnessEvent> {
    let usage = as_record(field(update, "usage"))
        .or_else(|| as_record(field(update, "tokenUsage")))
        .or_else(|| as_record(field(update, "token_usage")))
        .or_else(|| {
            USAGE_FIELDS
                .iter()
                .any(|key| number_field(update, key).is_some())
                .then_some(update)
        });
    let Some(usage) = usage else {
        return Vec::new();
    };
    let first = |keys: &[&str]| keys.iter().find_map(|key| number_field(usage, key));
    let used = first(&["totalTokens", "used", "usedTokens", "used_tokens"]).or_else(|| {
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
    usage_events(usage, used, window)
}

/// The `context` and `turn.metrics` events for one usage record.
fn usage_events(usage: &Rec, used: Option<f64>, window: Option<f64>) -> Vec<HarnessEvent> {
    let mut events = Vec::new();
    if used.is_some() || window.is_some() {
        events.push(HarnessEvent::Context {
            used: used.map(|value| value as i64),
            window: window.map(|value| value as i64),
        });
    }
    let first = |keys: &[&str]| keys.iter().find_map(|key| number_field(usage, key));
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

/// `planEvent`.
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

/// `toolLabel`.
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
    if !js::trim(&output_text).is_empty() {
        return Some(cap(&output_text));
    }
    opt_string_field(as_record(output), "content_concise").map(cap)
}

fn kind_from_name(name: Option<&str>) -> Option<&'static str> {
    let key = NON_ALNUM
        .replace_all(&name?.to_lowercase(), "")
        .into_owned();
    variant_kind(&key)
}

static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[_-]+").unwrap());
static DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_]+").unwrap());

fn humanize_tool_name(name: &str) -> String {
    let cleaned = SEPARATORS.replace_all(name, " ");
    let cleaned = js::trim(&cleaned);
    if cleaned.is_empty() {
        name.to_string()
    } else {
        upper_word_starts(cleaned)
    }
}

fn unique_grok_models(models: Vec<AgentModel>) -> Vec<AgentModel> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|model| seen.insert(model.id.clone()))
        .collect()
}

fn display_name(native: &str) -> String {
    upper_word_starts(&DASHES.replace_all(native, " "))
}

/// The `nativeId` helper: the slug after the first colon.
fn native_id(model: &str) -> &str {
    let trimmed = js::trim(model);
    match trimmed.find(':') {
        Some(colon) => &trimmed[colon + 1..],
        None => trimmed,
    }
}

/// `cap`: trim, and cut past 8,000 UTF-16 units with an ellipsis line.
pub(crate) fn cap(value: &str) -> String {
    const MAX: usize = 8_000;
    let text = js::trim(value);
    if js::len(text) <= MAX {
        return text.to_string();
    }
    format!("{}\n…", js::slice_prefix(text, MAX))
}

/// `pickOption`.
pub(crate) fn pick_option(option_ids: &[String], preferred: &[&str]) -> Option<String> {
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

/// `textFromContent`.
pub(crate) fn text_from_content(content: Option<&Value>, separator: &str) -> String {
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

fn basename(path: &str) -> String {
    match path.rsplit(['\\', '/']).next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => path.to_string(),
    }
}

/// `text.replace(/\b\w/g, (ch) => ch.toUpperCase())`. `\w` is ASCII only.
pub(crate) fn upper_word_starts(text: &str) -> String {
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

/// `text.split(/\r?\n/)`.
pub(crate) fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// A field read the way `??` sees it: a missing key and JSON `null` are both absent.
pub(crate) fn field<'a>(rec: &'a Rec, key: &str) -> Option<&'a Value> {
    rec.get(key).filter(|value| !value.is_null())
}

/// `asRecord`.
pub fn as_record(value: Option<&Value>) -> Option<&Rec> {
    value.and_then(Value::as_object)
}

/// `stringField`: a string with something besides whitespace in it.
pub fn string_field<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    match rec.get(key) {
        Some(Value::String(value)) if !js::trim(value).is_empty() => Some(value),
        _ => None,
    }
}

/// `stringField(rec ?? {}, key)`.
pub(crate) fn opt_string_field<'a>(rec: Option<&'a Rec>, key: &str) -> Option<&'a str> {
    rec.and_then(|rec| string_field(rec, key))
}

/// `numberField`. JSON numbers are always finite.
pub(crate) fn number_field(rec: &Rec, key: &str) -> Option<f64> {
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

/// `String(value ?? "")`.
pub(crate) fn js_string_or_empty(value: Option<&Value>) -> String {
    value.map(json_text_string).unwrap_or_default()
}

/// JavaScript `String(value)`.
pub(crate) fn json_text_string(value: &Value) -> String {
    crate::core::json_text::js_string(value)
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
