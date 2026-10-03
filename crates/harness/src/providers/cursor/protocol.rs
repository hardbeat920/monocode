//! Port of the pure helpers in
//! src/integrations/harness/providers/cursor/cursor.ts: session config
//! options, permission option picking, question replies, subagent and
//! enrichment checks, and the permission request reader. The live session
//! state machine is in `cursor.rs`.

use std::sync::LazyLock;

use monocode_core::block::{ToolPreview, ToolPreviewKind};
use monocode_core::harness::RuntimeMode;
use monocode_core::js;
use monocode_core::task_list::is_task_list_tool_name;
use monocode_core::user_question::{CUSTOM_OPTION_ID, UserQuestion, UserQuestionReply};
use regex::Regex;
use serde_json::{Value, json};

use crate::core::json_text::js_string;

use super::labels::{number_field, string_field, tool_label};
use monocode_core::reducer::{
    ToolTitleInput, agent_tool_title, compose_tool_title, extract_search_query,
    extract_shell_command, extract_skill_name, extract_tool_preview, is_agent_tool_name,
    is_weak_tool_title, merge_tool_preview,
};

use super::json::{EMPTY, Rec, as_record, object, present};
use super::subagents::cursor_agent_label;

static UPDATE_TODOS_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^update todos\b").unwrap());
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[_-]+").unwrap());

/// `TOOL_ENRICH_MAX_ATTEMPTS`.
pub const TOOL_ENRICH_MAX_ATTEMPTS: i64 = 20;

/// `CLIENT_CAPABILITIES`, shared by the live session, the text runner, and
/// the catalog probe.
pub fn client_capabilities() -> Value {
    json!({
        "fs": { "readTextFile": false, "writeTextFile": false },
        "terminal": false,
        "_meta": { "parameterizedModelPicker": true },
    })
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

/// `SessionConfigOption`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfigOption {
    pub id: String,
    pub category: Option<String>,
    pub current_value: Option<ConfigValue>,
}

/// JavaScript `String(value ?? "")`.
pub fn js_string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value) => js_string(value),
    }
}

/// `rec?.id ?? rec?.configId`, nullish-aware.
fn option_id(rec: &Rec) -> Option<&Value> {
    rec.get("id")
        .filter(|value| !value.is_null())
        .or_else(|| rec.get("configId").filter(|value| !value.is_null()))
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
            let id = js::trim(&js_string_or_empty(rec.and_then(option_id))).to_string();
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
                current_value: match rec.get("currentValue") {
                    Some(Value::String(value)) => Some(ConfigValue::String(value.clone())),
                    Some(Value::Bool(value)) => Some(ConfigValue::Bool(*value)),
                    _ => None,
                },
            })
        })
        .collect()
}

/// `extractModelConfigId(setup)`: the model option's id, else "model".
pub fn extract_model_config_id(config_options: Option<&Value>) -> String {
    read_config_options(config_options)
        .into_iter()
        .find(|option| option.category.as_deref() == Some("model") || option.id == "model")
        .map(|option| option.id)
        .unwrap_or_else(|| "model".into())
}

/// `resolveSettingConfigId`: map a MonoCode model setting to Cursor's config id.
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
    let find = |pred: &dyn Fn(&SessionConfigOption) -> bool| {
        options
            .iter()
            .find(|option| pred(option))
            .map(|option| option.id.clone())
    };
    match needle.as_str() {
        "effort" | "reasoning" => find(&|option| {
            option.id == "effort"
                || option.id == "reasoning"
                || (option.category.as_deref() == Some("thought_level") && option.id != "thinking")
        }),
        "fast" | "fastmode" => {
            find(&|option| option.id == "fast" || option.id.to_lowercase().contains("fast"))
        }
        "thinking" => find(&|option| option.id == "thinking"),
        "context" | "contextwindow" => {
            find(&|option| option.id == "context" || option.id == "context_size")
        }
        _ => None,
    }
}

/// `pickOption`: the first preferred id the request offers.
pub fn pick_option(option_ids: &[String], preferred: &[&str]) -> Option<String> {
    preferred
        .iter()
        .find(|id| option_ids.iter().any(|option| option == *id))
        .map(|id| id.to_string())
}

/// `pickAutoOption`: the option the runtime mode answers without asking.
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
    if runtime_mode == RuntimeMode::AutoAcceptEdits && (tool == "execute" || tool == "other") {
        return None;
    }
    if runtime_mode == RuntimeMode::FullAccess {
        return pick_option(
            option_ids,
            &["allow-always", "allow_always", "allow-once", "allow_once"],
        );
    }
    pick_option(
        option_ids,
        &["allow-once", "allow_once", "allow-always", "allow_always"],
    )
}

/// The option ids a planning turn answers with: allow reads and searches,
/// reject the rest.
pub fn planning_option(
    preview_kind: Option<ToolPreviewKind>,
    kind: Option<&str>,
    option_ids: &[String],
) -> String {
    let normalized = match preview_kind {
        Some(ToolPreviewKind::Read) => "read".to_string(),
        Some(ToolPreviewKind::Write) => "write".to_string(),
        Some(ToolPreviewKind::Shell) => "shell".to_string(),
        Some(ToolPreviewKind::Search) => "search".to_string(),
        None => kind.unwrap_or("").to_lowercase(),
    };
    let read_only = normalized == "read" || normalized == "search";
    let picked = if read_only {
        pick_option(option_ids, &["allow-once", "allow_once", "allow"])
    } else {
        pick_option(option_ids, &["reject-once", "reject_once", "reject-always"])
    };
    picked.unwrap_or_else(|| {
        if read_only {
            "allow-once"
        } else {
            "reject-once"
        }
        .into()
    })
}

/// The option id for the user's approval decision.
pub fn decision_option(allow: bool, option_ids: &[String]) -> String {
    let picked = if allow {
        pick_option(
            option_ids,
            &["allow-once", "allow_once", "allow-always", "allow_always"],
        )
    } else {
        pick_option(option_ids, &["reject-once", "reject_once", "reject-always"])
    };
    picked.unwrap_or_else(|| if allow { "allow-once" } else { "reject-once" }.into())
}

/// `cursorAskQuestionResponse`.
pub fn cursor_ask_question_response(
    reply: &UserQuestionReply,
    questions: &[UserQuestion],
) -> Value {
    let UserQuestionReply::Answered { answers, .. } = reply else {
        return json!({ "outcome": { "outcome": "skipped", "reason": "User skipped" } });
    };
    let answers: Vec<Value> = questions
        .iter()
        .map(|question| {
            let selected: Vec<&String> = answers
                .get(&question.id)
                .map(|ids| ids.iter().filter(|id| *id != CUSTOM_OPTION_ID).collect())
                .unwrap_or_default();
            json!({ "questionId": question.id, "selectedOptionIds": selected })
        })
        .collect();
    json!({ "outcome": { "outcome": "answered", "answers": answers } })
}

/// `isCursorTodoUpdate`.
pub fn is_cursor_todo_update(method: &str) -> bool {
    method == "cursor/update_todos" || method == "_cursor/update_todos"
}

/// `cursorSubagentDetail`: "explore subagent" from a task's subagent type.
pub fn cursor_subagent_detail(task: &Rec) -> Option<String> {
    let kind = task
        .get("subagentType")
        .filter(|value| !value.is_null())
        .or_else(|| task.get("subagent_type"));
    if let Some(Value::String(kind)) = kind
        && !kind.is_empty()
        && kind != "unspecified"
    {
        return Some(format!("{} subagent", SEPARATORS.replace_all(kind, " ")));
    }
    if let Some(Value::String(custom)) = as_record(kind).and_then(|rec| rec.get("custom"))
        && !js::trim(custom).is_empty()
    {
        return Some(format!(
            "{} subagent",
            SEPARATORS.replace_all(js::trim(custom), " ")
        ));
    }
    None
}

/// `cursorToolOutputIsBackground`.
pub fn cursor_tool_output_is_background(update: &Rec, tool: &Rec) -> bool {
    [
        update.get("rawOutput"),
        tool.get("rawOutput"),
        update.get("raw_output"),
        tool.get("raw_output"),
    ]
    .into_iter()
    .any(|value| {
        as_record(value).is_some_and(|output| {
            output.get("isBackground") == Some(&Value::Bool(true))
                || output.get("is_background") == Some(&Value::Bool(true))
        })
    })
}

/// `needsCursorToolEnrichment`: whether Cursor's own store should be read for
/// a better label.
pub fn needs_cursor_tool_enrichment(
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> bool {
    let key = kind.unwrap_or("").to_lowercase();
    if key == "agent" {
        return cursor_agent_label(title).is_none();
    }
    if matches!(key.as_str(), "execute" | "think" | "fetch" | "skill") {
        return false;
    }
    if preview.is_some_and(|preview| {
        preview.path.as_deref().is_some_and(|path| !path.is_empty())
            || preview
                .query
                .as_deref()
                .is_some_and(|query| !query.is_empty())
    }) {
        return false;
    }
    if matches!(key.as_str(), "read" | "search" | "edit" | "write") {
        return true;
    }
    title.is_none_or(|title| title.is_empty() || is_weak_tool_title(title))
}

/// The tool name a Cursor raw input carries.
fn input_tool_name(input: &Rec) -> Option<&str> {
    string_field(input, "_toolName")
        .or_else(|| string_field(input, "toolName"))
        .or_else(|| string_field(input, "tool_name"))
        .or_else(|| string_field(input, "name"))
}

/// `isCursorAgentInput`.
pub fn is_cursor_agent_input(value: Option<&Value>) -> bool {
    as_record(value)
        .and_then(input_tool_name)
        .is_some_and(is_agent_tool_name)
}

/// `isCursorTaskListInput`.
pub fn is_cursor_task_list_input(value: Option<&Value>, title: Option<&str>) -> bool {
    let normalized = as_record(value).and_then(input_tool_name).map(|name| {
        name.chars()
            .filter(|c| !(js::is_space(*c) || *c == '_' || *c == '-'))
            .collect::<String>()
            .to_lowercase()
    });
    normalized.is_some_and(|name| !name.is_empty() && is_task_list_tool_name(&name))
        || UPDATE_TODOS_TITLE.is_match(title.unwrap_or(""))
}

/// `cursorAgentTitle`: the task description, else a known name, else what
/// the input implies, else "Subagent".
pub fn cursor_agent_title(
    raw_input: Option<&Value>,
    raw_title: Option<&str>,
    existing: Option<&str>,
) -> String {
    let input = as_record(raw_input);
    if let Some(description) =
        input.and_then(|input| cursor_agent_label(string_field(input, "description")))
    {
        return description;
    }
    if let Some(name) = cursor_agent_label(existing).or_else(|| cursor_agent_label(raw_title)) {
        return name;
    }
    let inferred = input.map(|input| agent_tool_title(input, "Subagent"));
    cursor_agent_label(inferred.as_deref()).unwrap_or_else(|| "Subagent".into())
}

/// `toolEnrichmentDelay`: back off as attempts pile up.
pub fn tool_enrichment_delay(attempts: impl IntoIterator<Item = i64>) -> i64 {
    let fewest = attempts.into_iter().min();
    match fewest {
        Some(attempts) if attempts < 4 => 100,
        Some(attempts) if attempts < 12 => 300,
        _ => 1_000,
    }
}

/// What `handlePermission` reads from a `session/request_permission` request.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorPermissionRequest {
    pub title: String,
    pub kind: Option<String>,
    pub call_id: Option<String>,
    pub preview: Option<ToolPreview>,
    pub option_ids: Vec<String>,
}

/// The permission request half of `handlePermission`.
pub fn cursor_permission_request(params: &Value) -> CursorPermissionRequest {
    let rec = params.as_object();
    let field = |key: &str| rec.and_then(|rec| rec.get(key));
    let subject_value = object(field("subject"));
    let subject = as_record(subject_value);
    let tool_value = object(field("toolCall"))
        .or_else(|| object(subject.and_then(|subject| subject.get("toolCall"))))
        .or(subject_value)
        .or(rec.map(|_| params))
        .unwrap_or(&EMPTY);
    let tool = tool_value.as_object().expect("an object value");
    let empty = Rec::new();
    let subject_or_empty = subject.unwrap_or(&empty);
    let command = string_field(subject_or_empty, "command");
    let kind = string_field(tool, "kind").or_else(|| string_field(subject_or_empty, "kind"));
    let subject_preview = subject.and_then(|subject| extract_tool_preview(subject, subject));
    let preview = merge_tool_preview(
        extract_tool_preview(tool, tool).as_ref(),
        subject_preview.as_ref(),
    );
    let label = tool_label(tool, subject.unwrap_or(tool))
        .or_else(|| command.map(str::to_string))
        .or_else(|| {
            rec.and_then(|rec| string_field(rec, "title"))
                .map(str::to_string)
        });
    let inputs = present(&[
        tool.get("rawInput"),
        tool.get("raw_input"),
        tool.get("input"),
        subject_value,
    ]);
    let shell_command = command
        .map(str::to_string)
        .or_else(|| extract_shell_command(&inputs));
    let skill = extract_skill_name(&inputs);
    let query = preview
        .as_ref()
        .and_then(|preview| preview.query.clone())
        .or_else(|| extract_search_query(tool_value))
        .or_else(|| subject_value.and_then(extract_search_query));
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
    let call_id = string_field(tool, "toolCallId")
        .or_else(|| string_field(tool, "tool_call_id"))
        .or_else(|| rec.and_then(|rec| string_field(rec, "toolCallId")))
        .or_else(|| string_field(subject_or_empty, "toolCallId"))
        .map(str::to_string);
    let option_ids = match field("options") {
        Some(Value::Array(options)) => options
            .iter()
            .filter_map(|item| {
                item.get("optionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect(),
        _ => Vec::new(),
    };
    CursorPermissionRequest {
        title: if title.is_empty() {
            "Permission".into()
        } else {
            title
        },
        kind: kind.map(str::to_string),
        call_id,
        preview,
        option_ids,
    }
}

/// The text of a `cursor/ask_question` title, if the request carries one.
pub fn ask_question_title(params: &Value) -> Option<String> {
    match params.get("title") {
        Some(Value::String(title)) if !js::trim(title).is_empty() => {
            Some(js::trim(title).to_string())
        }
        _ => None,
    }
}

/// `toolCallId ?? tool_call_id` on a request record, for ask_question.
/// Unlike [`string_field`], any string counts.
pub fn ask_question_call_id(params: &Value) -> Option<String> {
    let rec = params.as_object()?;
    match (rec.get("toolCallId"), rec.get("tool_call_id")) {
        (Some(Value::String(id)), _) => Some(id.clone()),
        (_, Some(Value::String(id))) => Some(id.clone()),
        _ => None,
    }
}

/// `toolCallId ?? tool_call_id` through [`string_field`].
pub fn call_id_field(rec: &Rec) -> Option<String> {
    string_field(rec, "toolCallId")
        .or_else(|| string_field(rec, "tool_call_id"))
        .map(str::to_string)
}

/// `numberField(task, "durationMs") ?? numberField(task, "duration_ms")`.
pub fn task_duration(task: &Rec) -> Option<f64> {
    number_field(task, "durationMs").or_else(|| number_field(task, "duration_ms"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn reads_config_options_and_the_model_id() {
        let raw = json!([
            null,
            {},
            { "configId": "effort", "category": "thought_level", "currentValue": "high" },
            { "id": "picker", "category": "model", "currentValue": "composer-2.5" },
            { "id": "fast", "currentValue": true },
        ]);
        let options = read_config_options(Some(&raw));
        assert_eq!(options.len(), 3);
        assert_eq!(options[2].current_value, Some(ConfigValue::Bool(true)));
        assert_eq!(extract_model_config_id(Some(&raw)), "picker");
        assert_eq!(extract_model_config_id(None), "model");
        assert_eq!(
            resolve_setting_config_id(&options, "reasoning").as_deref(),
            Some("effort")
        );
        assert_eq!(
            resolve_setting_config_id(&options, "fastMode").as_deref(),
            Some("fast")
        );
        assert_eq!(resolve_setting_config_id(&options, "context"), None);
    }

    #[test]
    fn picks_permission_options_by_runtime_mode() {
        let offered = ids(&["allow-once", "allow-always", "reject-once"]);
        assert_eq!(
            pick_auto_option(RuntimeMode::Supervised, Some("edit"), &offered),
            None
        );
        assert_eq!(
            pick_auto_option(RuntimeMode::AutoAcceptEdits, Some("execute"), &offered),
            None
        );
        assert_eq!(
            pick_auto_option(RuntimeMode::AutoAcceptEdits, Some("edit"), &offered).as_deref(),
            Some("allow-once")
        );
        assert_eq!(
            pick_auto_option(RuntimeMode::FullAccess, Some("execute"), &offered).as_deref(),
            Some("allow-always")
        );
        assert_eq!(pick_auto_option(RuntimeMode::FullAccess, None, &[]), None);
        assert_eq!(
            decision_option(false, &ids(&["reject_once"])),
            "reject_once"
        );
        assert_eq!(decision_option(true, &[]), "allow-once");
        assert_eq!(planning_option(None, Some("read"), &offered), "allow-once");
        assert_eq!(
            planning_option(Some(ToolPreviewKind::Write), Some("read"), &[]),
            "reject-once"
        );
    }

    #[test]
    fn answers_questions_without_custom_ids() {
        let question: UserQuestion = serde_json::from_value(json!({
            "id": "q1", "prompt": "Which?", "multiSelect": false, "allowCustom": true, "options": []
        }))
        .unwrap();
        let mut answers = BTreeMap::new();
        answers.insert(
            "q1".to_string(),
            vec!["a".to_string(), CUSTOM_OPTION_ID.to_string()],
        );
        let reply = UserQuestionReply::Answered {
            answers,
            custom: None,
        };
        assert_eq!(
            cursor_ask_question_response(&reply, std::slice::from_ref(&question)),
            json!({ "outcome": { "outcome": "answered", "answers": [{ "questionId": "q1", "selectedOptionIds": ["a"] }] } })
        );
        assert_eq!(
            cursor_ask_question_response(&UserQuestionReply::Skipped, &[question]),
            json!({ "outcome": { "outcome": "skipped", "reason": "User skipped" } })
        );
    }

    #[test]
    fn describes_subagent_types() {
        assert_eq!(
            cursor_subagent_detail(
                json!({ "subagentType": "code_review" })
                    .as_object()
                    .unwrap()
            )
            .as_deref(),
            Some("code review subagent")
        );
        assert_eq!(
            cursor_subagent_detail(
                json!({ "subagentType": "unspecified" })
                    .as_object()
                    .unwrap()
            ),
            None
        );
        assert_eq!(
            cursor_subagent_detail(
                json!({ "subagent_type": { "custom": " my-agent " } })
                    .as_object()
                    .unwrap()
            )
            .as_deref(),
            Some("my agent subagent")
        );
    }

    #[test]
    fn detects_agent_and_todo_inputs() {
        assert!(is_cursor_agent_input(Some(&json!({ "_toolName": "task" }))));
        assert!(!is_cursor_agent_input(Some(
            &json!({ "_toolName": "read" })
        )));
        assert!(is_cursor_task_list_input(
            Some(&json!({ "_toolName": "updateTodos" })),
            None
        ));
        assert!(is_cursor_task_list_input(None, Some("Update TODOs")));
        assert!(!is_cursor_task_list_input(
            Some(&json!({ "name": "read" })),
            Some("Read")
        ));
        assert!(cursor_tool_output_is_background(
            json!({ "rawOutput": { "isBackground": true } })
                .as_object()
                .unwrap(),
            &Rec::new()
        ));
    }

    #[test]
    fn titles_agents_from_description_name_or_type() {
        assert_eq!(
            cursor_agent_title(
                Some(&json!({ "description": "Explore auth" })),
                Some("Task: x"),
                None
            ),
            "Explore auth"
        );
        assert_eq!(
            cursor_agent_title(
                Some(&json!({ "_toolName": "task" })),
                Some("Task: Subagent task"),
                None
            ),
            "Subagent"
        );
        assert_eq!(
            cursor_agent_title(None, Some("Task: Review"), Some("Known")),
            "Known"
        );
        assert_eq!(
            cursor_agent_title(Some(&json!({ "subagentType": "explore" })), None, None),
            "Explore subagent"
        );
    }

    #[test]
    fn decides_which_tools_need_enrichment() {
        assert!(needs_cursor_tool_enrichment(
            Some("agent"),
            Some("Subagent"),
            None
        ));
        assert!(!needs_cursor_tool_enrichment(
            Some("agent"),
            Some("Review auth"),
            None
        ));
        assert!(!needs_cursor_tool_enrichment(Some("execute"), None, None));
        assert!(needs_cursor_tool_enrichment(
            Some("read"),
            Some("Read a.ts"),
            None
        ));
        let mut preview = ToolPreview::new(ToolPreviewKind::Read);
        preview.path = Some("a.ts".into());
        assert!(!needs_cursor_tool_enrichment(
            Some("read"),
            Some("Read"),
            Some(&preview)
        ));
        assert!(needs_cursor_tool_enrichment(
            Some("other"),
            Some("Tool"),
            None
        ));
        assert!(!needs_cursor_tool_enrichment(
            Some("other"),
            Some("Fetch docs"),
            None
        ));
        assert_eq!(tool_enrichment_delay([5, 2]), 100);
        assert_eq!(tool_enrichment_delay([7]), 300);
        assert_eq!(tool_enrichment_delay([]), 1_000);
    }

    #[test]
    fn reads_a_permission_request() {
        let request = cursor_permission_request(&json!({
            "toolCall": { "toolCallId": "t1", "kind": "read", "rawInput": { "path": "/repo/src/a.ts" } },
            "options": [{ "optionId": "allow-once" }, { "optionId": "reject-once" }, { "name": "x" }],
        }));
        assert_eq!(request.call_id.as_deref(), Some("t1"));
        assert_eq!(request.kind.as_deref(), Some("read"));
        assert_eq!(request.title, "Read /repo/src/a.ts");
        assert_eq!(request.option_ids, ids(&["allow-once", "reject-once"]));
        let bare = cursor_permission_request(&json!({}));
        assert_eq!(bare.title, "Permission");
        let shell = cursor_permission_request(
            &json!({ "subject": { "command": "rm -rf build", "kind": "execute" } }),
        );
        assert_eq!(shell.title, "rm -rf build");
    }
}
