//! Structured task and delegated-run presentation. Never infer work from prose.
use super::{ToolCall, ToolState};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Panel {
    Tasks(TaskList),
    Agent(AgentRun),
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    pub items: Vec<Task>,
    #[serde(default)]
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    pub status: TaskStatus,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}
impl TaskStatus {
    pub fn parse(value: &str) -> Self {
        match value.to_lowercase().replace([' ', '-', '_'], "").as_str() {
            "completed" | "complete" | "done" => Self::Completed,
            "inprogress" | "active" | "running" => Self::InProgress,
            "cancelled" | "canceled" | "skipped" => Self::Cancelled,
            _ => Self::Pending,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "In progress",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
        }
    }
}
impl TaskList {
    pub fn progress(&self) -> String {
        let done = self
            .items
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let total = self
            .items
            .iter()
            .filter(|t| t.status != TaskStatus::Cancelled)
            .count();
        if total > 0 && done == total {
            "Complete".into()
        } else {
            format!(
                "{done} of {}",
                if total == 0 { self.items.len() } else { total }
            )
        }
    }
    pub fn from_value(value: &Value) -> Option<Self> {
        let rows = value
            .get("todos")
            .or_else(|| value.get("plan"))
            .or_else(|| value.get("items"))?
            .as_array()?;
        let items = rows
            .iter()
            .filter_map(|row| {
                let text = ["content", "activeForm", "text", "step"]
                    .iter()
                    .find_map(|key| string(&row[*key]))?;
                let id = string(&row["id"]).or_else(|| row["id"].as_i64().map(|id| id.to_string()));
                Some(Task {
                    id,
                    text,
                    status: TaskStatus::parse(row["status"].as_str().unwrap_or("pending")),
                })
            })
            .collect();
        Some(Self {
            items,
            explanation: string(&value["explanation"]).unwrap_or_default(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    /// Provider-reported thread/task identities, never a generated display ID.
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub steps: Vec<AgentStep>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStep {
    pub id: String,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub state: Option<ToolState>,
    #[serde(default)]
    pub detail: String,
    /// Original tool classification, separate from the step's tool/prose kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub detail_omitted: bool,
    /// Provider arguments and display metadata needed by normal tool rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Value>,
}
impl AgentStep {
    /// Mirror a child call without reducing its file/command metadata to prose.
    pub fn from_tool_call(call: &ToolCall) -> Self {
        let (action, target) = call.summary();
        let mut display = call
            .display
            .clone()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        display["nativeName"] = Value::String(call.name.clone());
        display["nativeInput"] = Value::String(call.input.clone());
        if let Some(panel) = &call.panel {
            display["nativePanel"] = serde_json::to_value(panel).unwrap_or(Value::Null);
        }
        let text = format!("{action} {target}").trim().to_owned();
        let tool_kind = string(&display["kind"])
            .filter(|kind| kind != "other")
            .or_else(|| matches!(call.panel(), Some(Panel::Agent(_))).then(|| "agent".into()))
            .unwrap_or_else(|| call.name.clone());
        let detail = if call.output.is_empty() {
            desktop_detail(&display)
        } else {
            call.output.clone()
        };
        let detail_omitted = detail.is_empty() && display["detailOmitted"] == true;
        Self {
            id: call.id.clone(),
            kind: "tool".into(),
            text,
            state: Some(call.state.clone()),
            detail,
            tool_kind: Some(tool_kind),
            preview: display
                .get("preview")
                .filter(|value| value.is_object())
                .cloned(),
            detail_omitted,
            display: Some(display),
        }
    }

    /// Use the same labels, file chips, previews and disclosures as parent tools.
    pub fn to_tool_call(&self) -> ToolCall {
        let mut display = self
            .display
            .clone()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        display["title"] = Value::String(self.text.clone());
        if let Some(kind) = &self.tool_kind {
            display["kind"] = Value::String(kind.clone());
        }
        if let Some(preview) = &self.preview {
            display["preview"] = preview.clone();
        }
        let name = string(&display["nativeName"])
            .or_else(|| {
                self.tool_kind
                    .as_deref()
                    .map(|kind| match kind.to_lowercase().as_str() {
                        "read" => "Read".into(),
                        "write" => "Write".into(),
                        "edit" => "Edit".into(),
                        "search" => "Grep".into(),
                        "execute" | "shell" | "bash" => "Bash".into(),
                        "agent" => "Agent".into(),
                        "task" => "Task".into(),
                        _ => kind.to_owned(),
                    })
            })
            .unwrap_or_else(|| self.text.clone());
        let input = display["nativeInput"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                display
                    .get("rawInput")
                    .or_else(|| display.get("raw_input"))
                    .or_else(|| display.get("input"))
                    .filter(|value| !value.is_null())
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    })
            })
            .unwrap_or_else(|| "{}".into());
        let mut call = ToolCall::new(&self.id, &name);
        call.input = input;
        call.output = if self.detail.is_empty() {
            desktop_detail(&display)
        } else {
            self.detail.clone()
        };
        display["detailOmitted"] = Value::Bool(self.detail_omitted && call.output.is_empty());
        call.state = self.state.clone().unwrap_or(ToolState::Completed);
        call.panel = display
            .get("nativePanel")
            .and_then(|panel| serde_json::from_value(panel.clone()).ok());
        call.display = Some(display);
        if let Some(run) = call.agent_mut() {
            if run.name.is_none() && run.brief.is_empty() {
                run.name = Some(self.text.clone());
            }
        }
        call
    }
}
impl AgentRun {
    pub fn from_input(input: &Value) -> Self {
        Self {
            name: string(&input["name"])
                .or_else(|| string(&input["agentName"]))
                .or_else(|| agent_path_name(input)),
            model: string(&input["model"]),
            agent_type: string(&input["subagent_type"]),
            brief: string(&input["description"])
                .or_else(|| string(&input["prompt"]))
                .unwrap_or_default(),
            ..Self::default()
        }
    }
    pub fn title(&self) -> &str {
        self.name
            .as_deref()
            .or_else(|| self.brief.lines().next().filter(|s| !s.is_empty()))
            .unwrap_or("Subagent")
    }
    /// The entire brief belongs on the row's tooltip, as in the React transcript.
    pub fn display_brief(&self) -> String {
        let brief = self
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.brief)
            .trim();
        let brief = if let Some(end) = agent_prefix_end(brief) {
            brief[end..]
                .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '·' | '-'))
        } else {
            brief
        };
        if brief.is_empty() {
            "Subagent".into()
        } else {
            brief.into()
        }
    }
    pub fn display_name(&self) -> String {
        const LIMIT: usize = 56;
        let brief = self.display_brief();
        let sentence_end = brief
            .char_indices()
            .find(|(_, ch)| matches!(ch, '.' | '!' | '?'))
            .map(|(at, ch)| at + ch.len_utf8())
            .unwrap_or(brief.len());
        let name = brief[..sentence_end].trim();
        if name.encode_utf16().count() <= LIMIT {
            return name.into();
        }
        // Keep React's UTF-16 length while never cutting a surrogate pair in half.
        let mut units = 0;
        let cut: String = name
            .chars()
            .take_while(|ch| {
                units += ch.len_utf16();
                units <= LIMIT
            })
            .collect();
        let trimmed = cut
            .rfind(' ')
            .filter(|at| cut[..*at].encode_utf16().count() > LIMIT / 2)
            .map(|at| &cut[..at])
            .unwrap_or(&cut)
            .trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | ':'));
        format!("{trimmed}…")
    }
    pub fn status_line(&self, state: &ToolState) -> String {
        if matches!(state, ToolState::Failed | ToolState::Cancelled) {
            return "failed".into();
        }
        let tools = self.steps.iter().filter(|step| step.kind == "tool");
        let count = tools.clone().count();
        if count == 0 {
            return String::new();
        }
        let failed = tools
            .filter(|step| matches!(step.state, Some(ToolState::Failed | ToolState::Cancelled)))
            .count();
        let label = format!("{count} {}", if count == 1 { "step" } else { "steps" });
        if failed == 0 {
            label
        } else {
            format!("{label}, {failed} failed")
        }
    }
    pub fn model_name(&self, provider: crate::provider::Kind) -> Option<String> {
        let id = self.model.as_deref()?.trim();
        if id.is_empty()
            || ["auto", "default", "inherit", "unspecified"]
                .iter()
                .any(|value| id.eq_ignore_ascii_case(value))
        {
            return None;
        }
        crate::catalog::cached_name(provider, id)
            .or_else(|| desktop_model_name(id).map(str::to_owned))
            .or_else(|| Some(id.into()))
    }
    pub fn upsert(&mut self, mut step: AgentStep) {
        // Bound child detail without claiming the retained window is the whole run.
        bound(&mut step.text, 16 * 1024);
        bound(&mut step.detail, 16 * 1024);
        if let Some(old) = self.steps.iter_mut().find(|old| old.id == step.id) {
            // A replayed start/progress event must not erase a finished result.
            if step.state == Some(ToolState::Running)
                && matches!(
                    old.state,
                    Some(ToolState::Completed | ToolState::Failed | ToolState::Cancelled)
                )
            {
                step.state = old.state.clone();
            }
            if step.detail.is_empty() {
                step.detail.clone_from(&old.detail);
            }
            if step.text.is_empty()
                || (step.kind == "tool" && step.display.is_none() && old.display.is_some())
            {
                step.text.clone_from(&old.text);
            }
            if step.tool_kind.is_none() {
                step.tool_kind.clone_from(&old.tool_kind);
            }
            if step.state.is_none() {
                step.state.clone_from(&old.state);
            }
            step.preview = merge_metadata(old.preview.as_ref(), step.preview.as_ref());
            step.display = merge_metadata(old.display.as_ref(), step.display.as_ref());
            step.detail_omitted =
                step.detail.is_empty() && (step.detail_omitted || old.detail_omitted);
            *old = step;
        } else {
            self.steps.push(step);
        }
    }
}

fn merge_metadata(previous: Option<&Value>, update: Option<&Value>) -> Option<Value> {
    match (previous, update) {
        (Some(Value::Object(previous)), Some(Value::Object(update))) => {
            let mut merged = previous.clone();
            for (key, value) in update {
                if matches!(key.as_str(), "nativeInput" | "nativeName")
                    && value.as_str().is_some_and(str::is_empty)
                {
                    continue;
                }
                merged.insert(key.clone(), value.clone());
            }
            Some(Value::Object(merged))
        }
        (_, Some(update)) => Some(update.clone()),
        (previous, None) => previous.cloned(),
    }
}
fn is_false(value: &bool) -> bool {
    !*value
}

// The React model picker's built-in labels; live catalogs take precedence.
fn desktop_model_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "claude:sonnet-5" | "claude-sonnet-5" => "Claude Sonnet 5",
        "claude:opus-5" | "claude-opus-5" => "Claude Opus 5",
        "claude:opus-5-5" | "claude-opus-5-5" => "Claude Opus 5.5",
        "claude:fable-5" | "claude-fable-5" => "Claude Fable 5",
        "claude:opus-4.6" | "claude-opus-4-6" => "Opus 4.6",
        "claude:sonnet-4.6" | "cursor:claude-sonnet-4-6" | "claude-sonnet-4-6" => "Sonnet 4.6",
        "claude:haiku-4.5" | "claude-haiku-4-5" => "Haiku 4.5",
        "claude:opus-4.5" | "claude-opus-4-5" => "Opus 4.5",
        "cursor:composer-2.5" | "composer-2.5" => "Composer 2.5",
        "cursor:gpt-5.4" | "opencode:gpt-5.4" | "gpt-5.4" => "GPT-5.4",
        "cursor:grok-4.6" => "Cursor Grok 4.6",
        "grok:grok-4.6" | "grok-4.6" => "Grok 4.6",
        "grok:grok-4.5" | "opencode:grok-4.5" | "grok-4.5" => "Grok 4.5",
        "opencode:glm-5" => "GLM 5",
        "opencode:minimax-m2.5" => "MiniMax M2.5",
        "opencode:kimi-k2.5" => "Kimi K2.5",
        "opencode:deepseek-v4-flash" => "DeepSeek V4 Flash",
        "opencode:qwen-3.5" => "Qwen 3.5",
        "opencode:claude-sonnet-4.6" => "Claude Sonnet 4.6",
        "pi:default" | "omp:default" => "Default",
        "fx:zai/glm-5.2-fast" | "zai/glm-5.2-fast" => "GLM 5.2 Fast",
        "hermes:default" => "Configured model",
        "antigravity:gemini-3.8-flash-high" | "gemini-3.8-flash-high" => "Gemini 3.8 Flash (High)",
        "devin:adaptive" | "adaptive" => "Adaptive",
        _ => return None,
    })
}
pub fn bound(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut at = text.len() - limit;
        while !text.is_char_boundary(at) {
            at += 1;
        }
        *text = format!("… earlier content omitted …\n{}", &text[at..]);
    }
}
fn string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Codex reports task names as paths in both collaboration item formats.
/// Match the retained Tauri adapter's leaf formatting and "subagent" suffix.
fn agent_path_name(input: &Value) -> Option<String> {
    let path = string(&input["agentPath"]).or_else(|| string(&input["agent_path"]))?;
    let leaf = path.split(['/', '\\']).rfind(|part| !part.is_empty())?;
    static SEPARATORS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[_-]+").unwrap());
    static INITIALS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?-u:\b[a-z])").unwrap());
    let label = SEPARATORS.replace_all(leaf.trim(), " ");
    let label = INITIALS.replace_all(&label, |caps: &regex::Captures<'_>| {
        caps[0].to_ascii_uppercase()
    });
    Some(format!("{label} subagent"))
}

fn agent_prefix_end(title: &str) -> Option<usize> {
    ["agent", "task", "subagent"]
        .into_iter()
        .find_map(|prefix| {
            let end = prefix.len();
            (title
                .get(..end)
                .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
                && title[end..]
                    .chars()
                    .next()
                    .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_'))
            .then_some(end)
        })
}

/// Match the React transcript's delegated-tool recognition, including old rows.
fn is_agent_tool(kind: Option<&str>, title: &str) -> bool {
    match kind.unwrap_or("").trim().to_lowercase().as_str() {
        "agent" | "task" | "subagent" => true,
        "" | "other" => agent_prefix_end(title.trim()).is_some(),
        _ => false,
    }
}

/// Recover the same output sources for parent calls and mirrored child steps.
pub(crate) fn desktop_detail(meta: &Value) -> String {
    if let Some(detail) = meta["detail"].as_str().filter(|value| !value.is_empty()) {
        return detail.into();
    }
    let preview = &meta["preview"];
    if let Some(output) = preview["output"].as_str().filter(|value| !value.is_empty()) {
        return output.into();
    }
    if let Some(raw) = meta
        .get("rawOutput")
        .or_else(|| meta.get("raw_output"))
        .filter(|raw| !raw.is_null())
    {
        return raw
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| serde_json::to_string_pretty(raw).unwrap_or_default());
    }
    if let Some(display) = meta
        .get("nativePresentation")
        .filter(|display| display.is_object())
    {
        let detail = desktop_detail(display);
        if !detail.is_empty() {
            return detail;
        }
    }
    preview["lines"]
        .as_array()
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| {
                    line["text"]
                        .as_str()
                        .map(|text| match line["kind"].as_str() {
                            Some("add") => format!("+ {text}"),
                            Some("del") => format!("- {text}"),
                            _ => line["number"]
                                .as_u64()
                                .map_or_else(|| text.into(), |number| format!("{number}  {text}")),
                        })
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

impl ToolCall {
    pub fn agent_report(&self) -> Option<&str> {
        (self.state != ToolState::Running)
            .then(|| self.output.trim())
            .filter(|report| !report.is_empty())
    }
    pub fn panel(&self) -> Option<Panel> {
        if let Some(panel) = &self.panel {
            return Some(panel.clone());
        }
        let input: Value = serde_json::from_str(&self.input).unwrap_or_default();
        let name = self.name.to_lowercase().replace([' ', '-', '_'], "");
        if ["todowrite", "writetodos", "updatetodos", "updateplan"]
            .iter()
            .any(|suffix| name == *suffix || name.ends_with(&format!(":{suffix}")))
        {
            return TaskList::from_value(&input).map(Panel::Tasks);
        }
        let display = self.display.as_ref().unwrap_or(&input["__desktop"]);
        let title = display["title"].as_str().unwrap_or(&self.name);
        let kind = display["kind"].as_str();
        let native_agent = matches!(
            kind.unwrap_or("").trim().to_lowercase().as_str(),
            "" | "other"
        ) && matches!(
            self.name.trim().to_lowercase().as_str(),
            "agent" | "task" | "subagent"
        );
        if is_agent_tool(kind, title) || native_agent {
            let mut run = AgentRun::from_input(&input);
            if run.name.is_none() && run.brief.is_empty() {
                run.brief = title.into();
            }
            return Some(Panel::Agent(run));
        }
        None
    }
    pub fn refresh_agent_input(&mut self) {
        if let Ok(value) = serde_json::from_str::<Value>(&self.input) {
            let input = AgentRun::from_input(&value);
            if let Some(run) = self.agent_mut() {
                if input.name.is_some() {
                    run.name = input.name;
                }
                if input.model.is_some() {
                    run.model = input.model;
                }
                if input.agent_type.is_some() {
                    run.agent_type = input.agent_type;
                }
                if !input.brief.is_empty() {
                    run.brief = input.brief;
                }
            }
        }
    }
    pub fn agent_mut(&mut self) -> Option<&mut AgentRun> {
        if self.panel.is_none() {
            self.panel = self.panel();
        }
        match self.panel.as_mut() {
            Some(Panel::Agent(run)) => Some(run),
            _ => None,
        }
    }
}

/// Adapt desktop metadata and recognize legacy delegated-tool captions.
pub fn from_desktop(block: &Value) -> Option<Panel> {
    if block["role"] == "tasks" {
        return TaskList::from_value(&block["taskList"]).map(Panel::Tasks);
    }
    let agent = &block["agentRun"];
    let title = string(&block["text"])
        .or_else(|| string(&block["tool"]["title"]))
        .unwrap_or_default();
    if !agent.is_object() && !is_agent_tool(block["tool"]["kind"].as_str(), &title) {
        return None;
    }
    let mut name = agent.get("name").and_then(string);
    if name
        .as_deref()
        .is_none_or(|name| name.eq_ignore_ascii_case("Subagent"))
    {
        // Older native builds saved a placeholder but retained the Codex item.
        // Recover its title on read without rewriting the stored transcript.
        if let Some(input) = block["tool"]["nativeInput"]
            .as_str()
            .and_then(|input| serde_json::from_str::<Value>(input).ok())
            .filter(|input| {
                matches!(
                    input["type"].as_str(),
                    Some("collabAgentToolCall" | "subAgentActivity")
                )
            })
        {
            name = AgentRun::from_input(&input).name.or(name);
        }
    }
    let brief = agent
        .get("nativeBrief")
        .and_then(string)
        .unwrap_or_else(|| if name.is_none() { title } else { String::new() });
    let mut run = AgentRun {
        ids: agent
            .get("nativeIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(string)
            .collect(),
        name,
        model: agent.get("model").and_then(string),
        agent_type: agent.get("agentType").and_then(string),
        brief,
        ..AgentRun::default()
    };
    for step in agent
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = string(&step["id"]) else {
            continue;
        };
        let mut display = step
            .get("nativePresentation")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        for key in [
            "nativeName",
            "nativeInput",
            "rawInput",
            "raw_input",
            "input",
            "locations",
            "location",
        ] {
            if let Some(value) = step.get(key) {
                display[key] = value.clone();
            }
        }
        let detail = desktop_detail(step);
        let detail_omitted = detail.is_empty() && step["detailOmitted"] == true;
        // Source history already contains the full payload. Do not truncate a
        // stored result while projecting it into the native transcript.
        run.steps.push(AgentStep {
            id,
            kind: string(&step["kind"]).unwrap_or_else(|| "tool".into()),
            text: string(&step["text"]).unwrap_or_default(),
            state: step["status"].as_str().map(|status| match status {
                "completed" | "success" => ToolState::Completed,
                "failed" | "error" => ToolState::Failed,
                "cancelled" | "canceled" | "aborted" => ToolState::Cancelled,
                _ => ToolState::Running,
            }),
            detail,
            tool_kind: string(&step["toolKind"]).or_else(|| string(&display["kind"])),
            preview: step
                .get("preview")
                .filter(|value| value.is_object())
                .or_else(|| display.get("preview").filter(|value| value.is_object()))
                .cloned(),
            detail_omitted,
            display: display
                .as_object()
                .is_some_and(|fields| !fields.is_empty())
                .then_some(display),
        });
    }
    Some(Panel::Agent(run))
}

pub fn desktop_agent(run: &AgentRun) -> Value {
    serde_json::json!({
        "name":run.name.as_deref().filter(|name| !name.trim().is_empty()).unwrap_or_else(|| if run.brief.is_empty() { "Subagent" } else { &run.brief }),
        "model":run.model, "agentType":run.agent_type, "nativeIds":run.ids, "nativeBrief":run.brief,
        "steps":run.steps.iter().map(|step| {
            let mut value = serde_json::json!({
                "id":step.id,"kind":step.kind,"text":step.text,"detail":step.detail,
                "status":step.state.as_ref().map(|state| match state { ToolState::Running=>"in_progress", ToolState::Completed=>"completed", ToolState::Failed=>"failed", ToolState::Cancelled=>"cancelled" })
            });
            if let Some(kind) = &step.tool_kind { value["toolKind"] = Value::String(kind.clone()); }
            if let Some(preview) = &step.preview { value["preview"] = preview.clone(); }
            value["detailOmitted"] = Value::Bool(step.detail_omitted);
            if let Some(display) = &step.display { value["nativePresentation"] = display.clone(); }
            value
        }).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn codex_saved_placeholders_recover_path_titles_without_losing_steps() {
        let mut block = json!({
            "role":"tool", "text":"collabAgentToolCall",
            "tool":{"kind":"agent", "nativeInput":json!({
                "type":"collabAgentToolCall", "agentPath":"/root/explore-auth"
            }).to_string()},
            "agentRun":{"name":"Subagent", "model":"child-model", "steps":[{
                "id":"read", "kind":"tool", "text":"Read source.rs", "status":"completed"
            }]}
        });
        let original = block.clone();
        let Some(Panel::Agent(run)) = from_desktop(&block) else {
            panic!()
        };
        assert_eq!(run.display_name(), "Explore Auth subagent");
        assert_eq!(run.model.as_deref(), Some("child-model"));
        assert_eq!(run.steps.len(), 1);
        assert_eq!(block, original);

        block["agentRun"]["name"] = json!("Correctness review");
        let Some(Panel::Agent(run)) = from_desktop(&block) else {
            panic!()
        };
        assert_eq!(run.display_name(), "Correctness review");
        block["agentRun"]["name"] = json!("Subagent");
        block["tool"]["nativeInput"] = json!("invalid input");
        let Some(Panel::Agent(run)) = from_desktop(&block) else {
            panic!()
        };
        assert_eq!(run.display_name(), "Subagent");
    }
    #[test]
    fn agent_input_names_keep_explicit_names_and_prompt_fallbacks() {
        for (input, expected) in [
            (
                json!({"name":"Reviewer", "agentPath":"/root/explore-auth"}),
                "Reviewer",
            ),
            (
                json!({"agentName":"Reviewer", "agentPath":"/root/explore-auth"}),
                "Reviewer",
            ),
            (
                json!({"prompt":"Correctness review", "agentPath":"/root/explore-auth"}),
                "Explore Auth subagent",
            ),
            (json!({"prompt":"Correctness review"}), "Correctness review"),
            (json!({"agentPath":"/"}), "Subagent"),
            (json!({}), "Subagent"),
        ] {
            assert_eq!(AgentRun::from_input(&input).display_name(), expected);
        }
    }
    #[test]
    fn task_snapshots_keep_identity_cancelled_denominator_and_empty_clear() {
        let list = TaskList::from_value(&json!({"todos":[{"id":7,"content":"Ship","status":"completed"},{"id":"skip","activeForm":"Skip","status":"cancelled"},{"text":"Inspect 🙂","status":"in_progress"},{"status":"completed"}]})).unwrap();
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.items[0].id.as_deref(), Some("7"));
        assert_eq!(list.progress(), "1 of 2");
        assert_eq!(
            TaskList::from_value(&json!({"todos":[]}))
                .unwrap()
                .items
                .len(),
            0
        );
    }
    #[test]
    fn arbitrary_tool_and_prose_are_not_interpreted_as_tasks() {
        let mut tool = ToolCall::new("x", "Read");
        tool.input = json!({"todos":[{"content":"not a task list"}]}).to_string();
        assert_eq!(tool.panel(), None);
        tool.name = "TodoWrite".into();
        tool.input = "[x] Not structured".into();
        assert_eq!(tool.panel(), None);
    }
    #[test]
    fn panels_roundtrip_and_old_calls_deserialize() {
        let old = r#"{"id":"x","name":"Read","input":"{}","output":"","state":"completed"}"#;
        assert!(serde_json::from_str::<ToolCall>(old)
            .unwrap()
            .panel
            .is_none());
        let mut call = ToolCall::new("actual-tool-id", "Agent");
        call.input = json!({"description":"Review","model":"actual-model"}).to_string();
        let run = call.agent_mut().unwrap();
        run.ids.push("actual-child-id".into());
        for (text, state) in [
            ("Reading", ToolState::Running),
            ("Read", ToolState::Completed),
        ] {
            run.upsert(AgentStep {
                id: "real-step".into(),
                kind: "tool".into(),
                text: text.into(),
                state: Some(state),
                detail: String::new(),
                ..AgentStep::default()
            });
        }
        assert_eq!(run.steps.len(), 1);
        assert_eq!(
            serde_json::from_str::<ToolCall>(&serde_json::to_string(&call).unwrap()).unwrap(),
            call
        );
    }
}
