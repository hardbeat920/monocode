//! Native provider transports, ported from Monocode's TS adapters.
//! Blocking work belongs to a worker; never call `run` from a render/input handler.
use crate::agent::{AccessMode, Approval, Control, Decision, Settings, TurnMetrics};
use crate::tool::{ToolCall, ToolState};
use crate::{process, transcript::snapshot_remainder};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

mod acp;
mod commands;
pub use commands::{
    compact, discover_commands, native_invocation, native_prompt, prepare_prompt, supports_compact,
    NativeCommand, NativeSubcommand,
};
mod attachments;
mod compaction;
pub(crate) mod devin;
mod discovery;
mod follow_up;
mod kind;
mod lifecycle;
mod limits;
mod opencode;
mod parked;
mod pi;
pub fn configure_idle_agents(limit: usize) {
    parked::configure(limit);
    opencode::configure(limit);
}
pub fn keep_idle_agents(ids: Vec<i64>) {
    parked::keep(ids);
}
pub fn discard_worktree(path: &Path) {
    parked::discard_worktree(path);
    opencode::discard_worktree(path);
}
pub fn forget_session(kind: Kind, provider_id: &str, account_id: &str, cwd: &Path) {
    parked::forget_session(kind, provider_id, account_id, cwd);
    if kind == Kind::OpenCode {
        opencode::forget_session(provider_id, account_id, cwd);
    }
}
mod questions;
mod rewind;
mod tasks;
pub use rewind::{rewind_last_turn, supports_rewind, RewindError};
#[cfg(all(test, unix))]
mod text_tests;
pub use kind::Kind;

const BATCH_BYTES: usize = 64 * 1024;
const TICK: Duration = Duration::from_millis(16);

#[derive(Clone, Debug)]
pub struct Request {
    pub kind: Kind,
    pub binary: PathBuf,
    pub cwd: PathBuf,
    pub prompt: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub settings: Settings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Ready,
    Commands(Vec<NativeCommand>),
    FollowUp(crate::follow_up::Message),
    FollowUpFailed {
        id: String,
        error: String,
    },
    SessionId(String),
    TurnId(String),
    Model(String),
    Context {
        used: Option<u64>,
        window: Option<u64>,
    },
    Metrics(TurnMetrics),
    Approval(Approval),
    ApprovalResolved(String),
    Question(crate::agent::Question),
    QuestionResolved(String),
    BeginAssistant,
    Text(String),
    Reasoning(String),
    Tool(ToolCall),
    Status(String),
    /// What the turn still waits on after the provider yielded; empty clears it.
    Background(Vec<BackgroundTask>),
    UsageLimited(crate::usage::Limit),
    Interjection {
        text: String,
        custom_type: String,
        severity: Option<String>,
    },
    ConfigChanged {
        model: Option<String>,
        model_settings: HashMap<String, String>,
    },
    Done,
}

/// A command or subagent left running after the provider yielded its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundTask {
    pub id: String,
    pub description: String,
    pub agent: bool,
    /// The shell command behind a backgrounded Bash call, when known.
    pub command: Option<String>,
    /// Unix milliseconds.
    pub started_at: u64,
}

fn codex_error(error: &Value, fallback: &str) -> String {
    let message = error["message"].as_str().unwrap_or(fallback);
    if error["codexErrorInfo"] == "usageLimitExceeded" {
        format!("Usage limit reached: {message}")
    } else {
        message.into()
    }
}

/// GUI apps may not inherit the login shell PATH. Try the same common user and
/// system install locations as harness.rs, plus Claude's versioned native install.
/// An explicit override wins and errors at launch rather than silently falling back.
pub fn resolve(kind: Kind) -> Option<PathBuf> {
    if let Some(path) = crate::cli::configured(kind) {
        return Some(path);
    }
    resolve_auto(kind)
}
pub fn resolve_auto(kind: Kind) -> Option<PathBuf> {
    let key = format!("MONOCODE_{}_BINARY", kind.id().to_uppercase());
    if let Some(path) = std::env::var_os(key).filter(|s| !s.is_empty()) {
        return Some(path.into());
    }
    let mut candidates: Vec<PathBuf> = Some(std::ffi::OsString::from(
        crate::scm::shell::gui_search_path(),
    ))
    .into_iter()
    .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
    .filter(|p| !p.as_os_str().is_empty())
    .map(|p| p.join(kind.binary_name()))
    .collect();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        for dir in kind.install_dirs().iter().copied().chain([
            ".local/bin",
            ".bun/bin",
            ".npm-global/bin",
            ".cargo/bin",
            "n/bin",
        ]) {
            candidates.push(home.join(dir).join(kind.binary_name()));
        }
        if kind == Kind::Claude {
            candidates.push(home.join(".claude/local/claude"));
            let dir = home.join(".local/share/claude/versions");
            let mut versions: Vec<_> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            versions.sort_by_key(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .split('.')
                    .map(|s| s.parse::<u32>().unwrap_or(0))
                    .collect::<Vec<_>>()
            });
            candidates.extend(versions.into_iter().rev());
        }
        #[cfg(windows)]
        candidates.push(home.join("AppData/Roaming/npm").join(kind.binary_name()));
    }
    for dir in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/snap/bin",
    ] {
        candidates.push(Path::new(dir).join(kind.binary_name()));
    }
    if matches!(kind, Kind::Cursor | Kind::Pi) {
        let alias = if kind == Kind::Cursor {
            "agent"
        } else {
            "pi-coding-agent"
        };
        let paths: Vec<_> = candidates
            .iter()
            .filter_map(|p| p.parent().map(|dir| dir.join(alias)))
            .collect();
        candidates.extend(paths);
    }
    candidates.into_iter().find_map(|p| {
        #[cfg(windows)]
        for ext in ["exe", "cmd", "bat", "com"] {
            let candidate = p.with_extension(ext);
            if executable(&candidate) && discovery::matches(kind, &candidate) {
                return Some(candidate);
            }
        }
        (executable(&p) && discovery::matches(kind, &p)).then_some(p)
    })
}
fn executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn write_json(out: &mut impl Write, value: Value) -> Result<(), String> {
    let mut frame = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    frame.push(b'\n');
    out.write_all(&frame)
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}

/// State machine is deliberately testable without spawning a provider.
struct Protocol {
    initialized: bool,
    compact: bool,
    command_probe: bool,
    compact_confirmed: bool,
    active_turn: Option<String>,
    previous_turn: Option<String>,
    interrupt_id: Option<String>,
    claude_prompt_sent: bool,
    claude_result_seen: bool,
    claude_usage_limit: Option<crate::usage::Limit>,
    claude_resume_at: Option<Instant>,
    follow_up_ready: bool,
    steering: HashMap<String, (crate::follow_up::Message, Instant)>,
    request: Request,
    emitted: HashMap<String, String>,
    claude_message: usize,
    begun: bool,
    done: bool,
    tools: HashMap<String, ToolCall>,
    active_compaction: Option<String>,
    activities: tasks::Activities,
    /// Streaming tool id and its partial input JSON, by content block index.
    claude_tools: HashMap<u64, (String, String)>,
    text_key: Option<String>,
    approvals: HashMap<String, (Value, String, Value)>,
    metrics: TurnMetrics,
    usage_total: u64,
    acp: acp::State,
    pi: pi::State,
    probe: bool,
    catalog_result: Option<Value>,
    questions: HashMap<String, (Value, String, Value, crate::agent::Question)>,
    question_order: std::collections::VecDeque<String>,
    visible_question: Option<String>,
    async_question_items: std::collections::HashSet<String>,
    async_answers: HashMap<String, (Value, String, Value, crate::agent::Question)>,
}
impl Protocol {
    fn new(request: Request) -> Self {
        Self {
            request,
            initialized: false,
            compact: false,
            command_probe: false,
            compact_confirmed: false,
            active_turn: None,
            previous_turn: None,
            interrupt_id: None,
            claude_prompt_sent: false,
            claude_result_seen: false,
            claude_usage_limit: None,
            claude_resume_at: None,
            follow_up_ready: false,
            steering: HashMap::new(),
            emitted: HashMap::new(),
            claude_message: 0,
            begun: false,
            done: false,
            tools: HashMap::new(),
            active_compaction: None,
            activities: tasks::Activities::default(),
            claude_tools: HashMap::new(),
            text_key: None,
            approvals: HashMap::new(),
            metrics: TurnMetrics::default(),
            usage_total: 0,
            acp: Default::default(),
            pi: Default::default(),
            probe: false,
            catalog_result: None,
            questions: HashMap::new(),
            question_order: Default::default(),
            visible_question: None,
            async_question_items: Default::default(),
            async_answers: HashMap::new(),
        }
    }
    fn text(&mut self, key: &str, text: &str, snapshot: bool, events: &mut Vec<Event>) {
        let already = self.emitted.entry(key.to_owned()).or_default();
        let text = if snapshot {
            snapshot_remainder(already, text)
        } else {
            text
        };
        if text.is_empty() {
            return;
        }
        already.push_str(text);
        let text = text.to_owned();
        if self.text_key.as_deref() != Some(key) {
            events.push(Event::BeginAssistant);
            self.begun = true;
            self.text_key = Some(key.to_owned());
        }
        events.push(Event::Text(text));
    }
    fn frame(&mut self, v: Value, stdin: &mut impl Write) -> Result<Vec<Event>, String> {
        if self.request.kind == Kind::Codex {
            // Codex requests its clock before thread/start may have replied.
            if v["method"] == "currentTime/read" && v.get("id").is_some() {
                write_json(
                    stdin,
                    json!({"id":v["id"],"result":{"currentTimeAt":crate::time::now_ms()/1000}}),
                )?;
                return Ok(Vec::new());
            }
            let turn = v["params"]["turnId"]
                .as_str()
                .or_else(|| v["params"]["turn"]["id"].as_str());
            if turn.is_some() && turn == self.previous_turn.as_deref() {
                return Ok(Vec::new());
            }
            if let Some(id) = v["id"].as_str() {
                if let Some((wire, kind, request, question)) = self.async_answers.remove(id) {
                    if let Some(error) = v.get("error").filter(|e| !e.is_null()) {
                        let message = codex_error(error, "Could not send your answer to Codex");
                        self.questions
                            .insert(question.id.clone(), (wire, kind, request, question.clone()));
                        self.visible_question = Some(question.id.clone());
                        return Ok(vec![Event::Status(message), Event::Question(question)]);
                    }
                    return Ok(vec![Event::QuestionResolved(question.id)]);
                }
                if id.starts_with("native-answer-") || id.starts_with("native-interrupt-") {
                    return Ok(Vec::new());
                }
            }
            let method = v["method"].as_str().unwrap_or("");
            if self.previous_turn.is_some()
                && self.active_turn.is_none()
                && method != "turn/started"
                && (method.starts_with("item/") || method.starts_with("turn/") || method == "error")
            {
                return Ok(Vec::new());
            }
        }
        if self.request.kind == Kind::Claude {
            if v["type"] == "control_cancel_request" {
                let id = v["request_id"].to_string();
                let mut events = Vec::new();
                if self.approvals.remove(&id).is_some() {
                    events.push(Event::ApprovalResolved(id.clone()));
                }
                if self.questions.remove(&id).is_some() {
                    if self.visible_question.as_deref() == Some(id.as_str()) {
                        self.visible_question = None;
                    }
                    events.push(Event::QuestionResolved(id));
                    events.extend(self.show_next_question());
                }
                return Ok(events);
            }
            if v["type"] == "control_response"
                || (v["type"] == "system"
                    && matches!(v["subtype"].as_str(), Some("init" | "initialized")))
            {
                self.claude_prompt(stdin)?;
            }
            let starts_message = v["parent_tool_use_id"].is_null()
                && (v["type"] == "assistant"
                    || (v["type"] == "stream_event" && v["event"]["type"] == "message_start")
                    || (v["type"] == "system" && v["subtype"] == "init"));
            if starts_message && self.claude_result_seen {
                self.claude_result_seen = false;
                self.claude_resume_at = None;
                self.text_key = None;
            }
        }
        let mut activity_events = Vec::new();
        if self.activity(&v, &mut activity_events) {
            return Ok(activity_events);
        }
        if let Some(events) = self.steer_response(&v) {
            return Ok(events);
        }
        let mut started_turn = None;
        if self.request.kind == Kind::Codex {
            if v["id"] == 2 {
                if let Some(id) = v["result"]["thread"]["id"].as_str() {
                    self.request.provider_id = Some(id.into());
                }
            }
            if v["method"] == "turn/started" || v["id"] == 3 {
                let turn = if v["id"] == 3 {
                    &v["result"]["turn"]
                } else {
                    &v["params"]["turn"]
                };
                if let Some(id) = turn["id"].as_str() {
                    if self.active_turn.as_deref() != Some(id) {
                        started_turn = Some(id.to_owned());
                    }
                    self.active_turn = Some(id.into());
                }
            }
        }
        if (self.request.kind == Kind::Claude && v["type"] == "system" && v["subtype"] == "init")
            || (matches!(self.request.kind, Kind::Pi | Kind::Omp) && v["type"] == "agent_start")
        {
            self.follow_up_ready = true;
        }
        let mut events = activity_events;
        if let Some(id) = started_turn {
            events.push(Event::TurnId(id));
        }
        if self.request.kind == Kind::Claude && v["type"] == "system" {
            if v["status"] == "compacting" {
                self.compaction(None, ToolState::Running, &mut events);
            } else if v["subtype"]
                .as_str()
                .is_some_and(|subtype| subtype.starts_with("compact"))
            {
                self.compaction(None, ToolState::Completed, &mut events);
            }
        }
        if let Some(commands) = self.command_updates(&v) {
            events.push(Event::Commands(commands));
        }
        if self.request.kind == Kind::Codex {
            self.codex_async_questions(&v, &mut events, stdin)?;
        }
        if self.compact
            && self.request.kind == Kind::Claude
            && v["type"] == "system"
            && v["subtype"]
                .as_str()
                .is_some_and(|s| s.starts_with("compact"))
        {
            self.compact_confirmed = true;
        }
        match self.request.kind {
            Kind::Codex => {
                if let Some(error) = v.get("error").filter(|e| !e.is_null()) {
                    let message = codex_error(error, "RPC request failed");
                    if v["id"] == 2
                        && self.request.provider_id.is_some()
                        && lifecycle::recoverable_codex_resume(&message)
                    {
                        if self.request.settings.mono.is_some() {
                            return Err("The saved Mono Codex context could not be resumed. Retry to keep its saved context.".into());
                        }
                        self.request.provider_id = None;
                        return self.frame(json!({"id":1,"result":{}}), stdin);
                    }
                    return Err(message);
                }
                match v["id"].as_u64() {
                    Some(1) if v.get("result").is_some() => {
                        if !self.initialized {
                            write_json(stdin, json!({"method":"initialized"}))?;
                            self.initialized = true;
                        }
                        let (policy, sandbox, sandbox_policy, reviewer) = self.codex_access();
                        let mut params = json!({"cwd":self.request.cwd,"approvalPolicy":policy,"sandbox":sandbox,"sandboxPolicy":sandbox_policy,"approvalsReviewer":reviewer,
                            "developerInstructions":format!("You are the coding agent in Monocode. Use the available tools to inspect and work in the supplied cwd. Do not ask the user to paste files or git output you can inspect yourself. Respect the selected access policy and request approval when required. {}", if self.request.settings.plan { "This is a planning turn: inspect and propose a plan without changing files." } else { "" })});
                        if let Some(instructions) = crate::visual_bridge::instructions(
                            &self.request.settings.environment,
                            false,
                        ) {
                            params["developerInstructions"] = format!(
                                "{}\n\n{instructions}",
                                params["developerInstructions"].as_str().unwrap_or_default()
                            )
                            .into();
                        }
                        if self.request.settings.text_only {
                            params["ephemeral"] = true.into();
                            params["developerInstructions"] = "Answer the supplied text request directly. Do not call tools or inspect the workspace.".into();
                        }
                        if let Some(model) = &self.request.model {
                            params["model"] = model.clone().into();
                        }
                        if let Some(tier) = self
                            .request
                            .settings
                            .service_tier
                            .as_deref()
                            .filter(|tier| *tier != "default")
                        {
                            params["serviceTier"] = tier.into();
                        }
                        let method = if let Some(id) = &self.request.provider_id {
                            params["threadId"] = id.clone().into();
                            // The app already owns its transcript. Resume the
                            // provider context without serializing its history.
                            params["excludeTurns"] = true.into();
                            "thread/resume"
                        } else {
                            "thread/start"
                        };
                        write_json(stdin, json!({"id":2,"method":method,"params":params}))?;
                    }
                    Some(2) if v.get("result").is_some() => {
                        let id = v["result"]["thread"]["id"]
                            .as_str()
                            .ok_or("Codex returned no thread id")?;
                        events.push(Event::SessionId(id.into()));
                        if let Some(model) = v["result"]["model"].as_str() {
                            events.push(Event::Model(model.into()));
                        }
                        if self.compact {
                            write_json(
                                stdin,
                                json!({"id":3,"method":"thread/compact/start","params":{"threadId":id}}),
                            )?;
                            return Ok(events);
                        }
                        let (policy, _, sandbox, reviewer) = self.codex_access();
                        let mut params = json!({"threadId":id,"input":self.request.codex_input(),
                            "approvalPolicy":policy,"sandboxPolicy":sandbox,"approvalsReviewer":reviewer});
                        if let Some(effort) = &self.request.settings.effort {
                            params["effort"] = effort.clone().into();
                        }
                        if self.request.settings.fast {
                            params["serviceTier"] = "fast".into();
                        }
                        if let Some(tier) = self
                            .request
                            .settings
                            .service_tier
                            .as_deref()
                            .filter(|tier| *tier != "default")
                        {
                            params["serviceTier"] = tier.into();
                        }
                        if self.request.settings.plan {
                            params["sandboxPolicy"] = json!({"type":"readOnly"});
                            if self.request.settings.operator
                                || self.request.settings.operator_enabled
                                || self.request.settings.orchestrator
                            {
                                params["sandboxPolicy"]["networkAccess"] = true.into();
                            }
                            params["approvalPolicy"] = "never".into();
                        }
                        if let Some(model) = self
                            .request
                            .model
                            .as_deref()
                            .or_else(|| v["result"]["model"].as_str())
                        {
                            params["collaborationMode"] = json!({"mode": if self.request.settings.plan { "plan" } else { "default" }, "settings":{"model":model,"reasoning_effort":self.request.settings.effort,"developer_instructions":null}});
                        }
                        if let Some(model) = &self.request.model {
                            params["model"] = model.clone().into();
                        }
                        write_json(stdin, json!({"id":3,"method":"turn/start","params":params}))?;
                    }
                    _ => {}
                }
                let p = &v["params"];
                match v["method"].as_str().unwrap_or("") {
                    "thread/tokenUsage/updated" => {
                        let usage = &p["tokenUsage"];
                        events.push(Event::Context {
                            used: usage["last"]["totalTokens"].as_u64(),
                            window: usage["modelContextWindow"].as_u64().filter(|w| *w > 0),
                        });
                        let total = usage["total"]["totalTokens"].as_u64().unwrap_or(0);
                        if usage["last"].is_object() && (total == 0 || total != self.usage_total) {
                            self.usage_total = total;
                            let last = &usage["last"];
                            self.metrics.input_tokens += last["inputTokens"].as_u64().unwrap_or(0);
                            self.metrics.output_tokens +=
                                last["outputTokens"].as_u64().unwrap_or(0);
                            self.metrics.cache_read_tokens +=
                                last["cachedInputTokens"].as_u64().unwrap_or(0);
                            self.metrics.cache_write_tokens +=
                                last["cacheWriteInputTokens"].as_u64().unwrap_or(0);
                            if (last.get("cachedInputTokens").is_some()
                                || last.get("cacheWriteInputTokens").is_some())
                                && self.metrics.input_tokens > 0
                            {
                                self.metrics.cache_hit_percent = Some(
                                    self.metrics.cache_read_tokens as f64
                                        / (self.metrics.input_tokens
                                            + self.metrics.cache_write_tokens)
                                            as f64
                                        * 100.,
                                );
                            }
                            if self.metrics.reported() {
                                events.push(Event::Metrics(self.metrics));
                            }
                        }
                    }
                    "item/agentMessage/delta" => {
                        self.text(
                            p["itemId"].as_str().unwrap_or("assistant"),
                            p["delta"].as_str().unwrap_or(""),
                            false,
                            &mut events,
                        );
                    }
                    "item/completed" if p["item"]["type"] == "agentMessage" => {
                        self.text(
                            p["item"]["id"].as_str().unwrap_or("assistant"),
                            p["item"]["text"].as_str().unwrap_or(""),
                            true,
                            &mut events,
                        );
                    }
                    "item/started" if p["item"]["type"] == "agentMessage" => {}
                    "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                        events.push(Event::Reasoning(p["delta"].as_str().unwrap_or("").into()));
                        events.push(Event::Status("Thinking…".into()));
                    }
                    "item/started" | "item/completed" => {
                        self.codex_tool(&p["item"], v["method"] == "item/completed", &mut events)
                    }
                    "thread/compacted" if self.active_compaction.is_some() => {
                        self.compaction(None, ToolState::Completed, &mut events);
                    }
                    "item/commandExecution/outputDelta" => {
                        let id = p["itemId"].as_str().unwrap_or("");
                        let tool = self
                            .tools
                            .entry(id.into())
                            .or_insert_with(|| ToolCall::new(id, "commandExecution"));
                        tool.append_output(p["delta"].as_str().unwrap_or(""));
                        events.push(Event::Tool(tool.clone()));
                    }
                    "turn/completed" | "turn/aborted" => {
                        let status = p["turn"]["status"].as_str().unwrap_or("completed");
                        if status == "failed" {
                            return Err(codex_error(&p["turn"]["error"], "Codex turn failed"));
                        }
                        let interrupted =
                            matches!(status, "interrupted" | "cancelled" | "canceled")
                                || v["method"] == "turn/aborted";
                        for item in p["turn"]["items"].as_array().into_iter().flatten() {
                            if interrupted && item["type"] == "contextCompaction" {
                                // An aborted turn can include its unfinished
                                // compaction item in the final snapshot.
                                continue;
                            }
                            self.codex_tool(item, true, &mut events);
                        }
                        if interrupted {
                            for tool in self
                                .tools
                                .values_mut()
                                .filter(|tool| tool.state == ToolState::Running)
                            {
                                tool.state = ToolState::Cancelled;
                                events.push(Event::Tool(tool.clone()));
                            }
                        }
                        events.push(Event::Done);
                        self.done = true;
                    }
                    "error" if p["willRetry"] != true => {
                        return Err(codex_error(&p["error"], "Codex stream error"));
                    }
                    "error" => {
                        events.push(Event::Status(codex_error(
                            &p["error"],
                            "Codex is reconnecting…",
                        )));
                    }
                    "mcpServer/elicitation/request" => {
                        self.codex_elicitation(v["id"].clone(), p.clone(), &mut events, stdin)?
                    }
                    "item/tool/requestUserInput" => {
                        self.question(v["id"].clone(), "codex", p.clone(), &mut events, stdin)?
                    }
                    method
                        if v.get("id").is_some()
                            && (method.ends_with("/requestApproval")
                                || matches!(
                                    method,
                                    "execCommandApproval" | "applyPatchApproval"
                                )) =>
                    {
                        let detail = p["command"]
                            .as_str()
                            .or(p["reason"].as_str())
                            .map(str::to_owned)
                            .unwrap_or_default();
                        self.approval(
                            v["id"].clone(),
                            method,
                            p.clone(),
                            detail,
                            stdin,
                            &mut events,
                        )?;
                    }
                    method if v.get("id").is_some() && !method.is_empty() => {
                        // No approval UX in this gate: resolve every server request promptly.
                        write_json(
                            stdin,
                            json!({"id":v["id"],"error":{"code":-32601,"message":format!("Unsupported method: {method}")}}),
                        )?;
                        events.push(Event::Status(format!(
                            "Unsupported Codex request: {method}"
                        )));
                    }
                    _ => {}
                }
            }
            Kind::Claude => {
                if let Some(id) = v["session_id"].as_str() {
                    events.push(Event::SessionId(id.into()));
                }
                if let Some(model) = v["model"].as_str().filter(|_| v["type"] == "system") {
                    events.push(Event::Model(model.into()));
                }
                match v["type"].as_str().unwrap_or("") {
                    "rate_limit_event" if v["parent_tool_use_id"].is_null() => {
                        self.claude_usage_limit =
                            crate::usage::Limit::from_claude_info(&v["rate_limit_info"]);
                    }
                    "control_response" if v["response"]["request_id"] == "native-init" => {
                        self.claude_prompt(stdin)?;
                    }
                    "stream_event" => {
                        let e = &v["event"];
                        if e["type"] == "message_start" {
                            self.claude_message += 1;
                            self.claude_tools.clear();
                        }
                        if e["type"] == "content_block_start"
                            && e["content_block"]["type"] == "tool_use"
                        {
                            let block = &e["content_block"];
                            let id = block["id"].as_str().unwrap_or("");
                            let tool = ToolCall::new(id, block["name"].as_str().unwrap_or("Tool"));
                            self.claude_tools.insert(
                                e["index"].as_u64().unwrap_or(0),
                                (id.into(), String::new()),
                            );
                            self.tools.insert(id.into(), tool.clone());
                            events.push(Event::Tool(tool));
                        }
                        if e["type"] == "content_block_delta"
                            && e["delta"]["type"] == "input_json_delta"
                        {
                            // Like the desktop adapter, keep the last input that
                            // parses; a half-streamed object is not shown.
                            if let Some((id, partial)) =
                                self.claude_tools.get_mut(&e["index"].as_u64().unwrap_or(0))
                            {
                                partial.push_str(e["delta"]["partial_json"].as_str().unwrap_or(""));
                                let parsed = serde_json::from_str::<Value>(partial)
                                    .is_ok_and(|value| value.is_object());
                                if let Some(tool) =
                                    self.tools.get_mut(id.as_str()).filter(|_| parsed)
                                {
                                    tool.input = partial.clone();
                                    events.push(Event::Tool(tool.clone()));
                                }
                            }
                        }
                        if e["type"] == "content_block_delta"
                            && e["delta"]["type"] == "thinking_delta"
                        {
                            events.push(Event::Reasoning(
                                e["delta"]["thinking"].as_str().unwrap_or("").into(),
                            ));
                        }
                        if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta"
                        {
                            self.text(
                                &self.claude_message.to_string(),
                                e["delta"]["text"].as_str().unwrap_or(""),
                                false,
                                &mut events,
                            );
                        }
                    }
                    "assistant" => {
                        if let Some(error) = v["error"].as_str() {
                            return Err(self.claude_error(error, &v));
                        }
                        if v["parent_tool_use_id"].is_null() {
                            if let Some(used) = crate::context::claude_used(&v["message"]["usage"])
                            {
                                events.push(Event::Context {
                                    used: Some(used),
                                    window: None,
                                });
                            }
                        }
                        for block in v["message"]["content"].as_array().into_iter().flatten() {
                            if block["type"] == "tool_use" {
                                let id = block["id"].as_str().unwrap_or("");
                                let tool = self.tools.entry(id.into()).or_insert_with(|| {
                                    ToolCall::new(id, block["name"].as_str().unwrap_or("Tool"))
                                });
                                tool.input = serde_json::to_string_pretty(&block["input"])
                                    .unwrap_or_default();
                                tool.refresh_agent_input();
                                events.push(Event::Tool(tool.clone()));
                            }
                        }
                        let text = v["message"]["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<String>();
                        self.text(&self.claude_message.to_string(), &text, true, &mut events);
                    }
                    "user" => {
                        for block in v["message"]["content"].as_array().into_iter().flatten() {
                            if block["type"] == "tool_result" {
                                // A background Agent's launch receipt is not its final report.
                                if block["is_error"] != true
                                    && block["tool_use_id"].as_str().is_some_and(|id| {
                                        self.activities.background_calls.contains(id)
                                    })
                                {
                                    continue;
                                }
                                if let Some(tool) = block["tool_use_id"]
                                    .as_str()
                                    .and_then(|id| self.tools.get_mut(id))
                                {
                                    tool.output = block["content"]
                                        .as_str()
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| {
                                            block["content"]
                                                .as_array()
                                                .into_iter()
                                                .flatten()
                                                .filter_map(|b| b["text"].as_str())
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        });
                                    tool.bound_output();
                                    tool.state = if block["is_error"] == true {
                                        ToolState::Failed
                                    } else {
                                        ToolState::Completed
                                    };
                                    events.push(Event::Tool(tool.clone()));
                                }
                            }
                        }
                    }
                    "result" => {
                        if self.compact && !self.compact_confirmed && v["is_error"] != true {
                            return Err("Claude Code did not confirm context compaction".into());
                        }
                        let usage = &v["usage"];
                        let input = usage["input_tokens"].as_u64().unwrap_or(0);
                        let output = usage["output_tokens"].as_u64().unwrap_or(0);
                        let read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                        let write = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                        let metrics = TurnMetrics {
                            input_tokens: input,
                            output_tokens: output,
                            cache_read_tokens: read,
                            cache_write_tokens: write,
                            cache_hit_percent: ((usage.get("cache_read_input_tokens").is_some()
                                || usage.get("cache_creation_input_tokens").is_some())
                                && input + read + write > 0)
                                .then(|| read as f64 / (input + read + write) as f64 * 100.),
                        };
                        if metrics.reported() {
                            events.push(Event::Metrics(metrics));
                        }
                        let last = v["usage"]["iterations"]
                            .as_array()
                            .and_then(|iterations| iterations.last())
                            .unwrap_or(&v["usage"]);
                        let used = crate::context::claude_used(last);
                        let window = v["modelUsage"]
                            .as_object()
                            .into_iter()
                            .flat_map(|models| models.values())
                            .filter_map(|model| model["contextWindow"].as_u64())
                            .filter(|w| *w > 0)
                            .max();
                        if used.is_some() || window.is_some() {
                            events.push(Event::Context { used, window });
                        }
                        if v["is_error"] == true {
                            let error = v["result"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v["errors"].to_string());
                            return Err(self.claude_error(&error, &v));
                        }
                        // Some versions omit partial/assistant frames in simple turns.
                        if !self.begun {
                            self.text(
                                &self.claude_message.to_string(),
                                v["result"].as_str().unwrap_or(""),
                                true,
                                &mut events,
                            );
                        }
                        self.claude_result_seen = true;
                        if self.activities.claude_background.is_empty()
                            && self.activities.claude_pending.is_empty()
                            && self.claude_resume_at.is_none()
                        {
                            events.push(Event::Done);
                            self.done = true;
                        } else {
                            events.push(Event::Status("Waiting for background tasks…".into()));
                        }
                    }
                    "control_request" => {
                        let request = &v["request"];
                        if request["subtype"] == "can_use_tool" {
                            if request["tool_name"] == "AskUserQuestion" {
                                self.question(
                                    v["request_id"].clone(),
                                    "claude",
                                    request.clone(),
                                    &mut events,
                                    stdin,
                                )?;
                                return Ok(events);
                            }
                            let detail =
                                serde_json::to_string_pretty(&request["input"]).unwrap_or_default();
                            self.approval(
                                v["request_id"].clone(),
                                "claude",
                                request.clone(),
                                detail,
                                stdin,
                                &mut events,
                            )?;
                        } else {
                            write_json(
                                stdin,
                                json!({"type":"control_response","response":{"subtype":"error","request_id":v["request_id"],"error":"Unsupported host request"}}),
                            )?;
                        }
                    }
                    _ => {}
                }
            }
            kind if kind.is_pi() => events.extend(self.pi_frame(v, stdin)?),
            _ => events.extend(self.acp_frame(v, stdin)?),
        }
        Ok(events)
    }
    fn approval(
        &mut self,
        wire: Value,
        kind: &str,
        request: Value,
        detail: String,
        stdin: &mut impl Write,
        events: &mut Vec<Event>,
    ) -> Result<(), String> {
        let id = wire.to_string();
        let title = if kind == "claude" {
            format!("Allow {}?", request["tool_name"].as_str().unwrap_or("tool"))
        } else if kind.contains("fileChange") || kind == "applyPatchApproval" {
            "Allow file changes?".into()
        } else if kind.contains("permissions") {
            "Allow additional permissions?".into()
        } else {
            "Allow this command?".into()
        };
        let mut detail =
            if let Some(tool) = request["itemId"].as_str().and_then(|id| self.tools.get(id)) {
                let detail = if detail.is_empty() {
                    tool.summary().1
                } else {
                    detail
                };
                if tool.output.is_empty() {
                    detail
                } else {
                    format!("{detail}\n\n{}", tool.output)
                }
            } else {
                detail
            };
        if detail.is_empty() {
            detail = request
                .get("permissions")
                .and_then(|p| serde_json::to_string_pretty(p).ok())
                .unwrap_or_else(|| "The agent is requesting permission to continue.".into());
        }
        self.approvals
            .insert(id.clone(), (wire, kind.into(), request));
        if self.request.settings.plan || self.request.settings.access == AccessMode::ReadOnly {
            return self.respond(
                Control {
                    id,
                    decision: Decision::Deny,
                },
                stdin,
            );
        }
        let file_change = kind.contains("fileChange") || kind == "applyPatchApproval";
        let permissions = kind.contains("permissions");
        if self.request.settings.access == AccessMode::FullAccess
            || (self.request.kind == Kind::Codex
                && self.request.settings.access == AccessMode::AutoAcceptEdits
                && file_change)
            || (self.request.kind == Kind::Codex
                && permissions
                && matches!(
                    self.request.settings.access,
                    AccessMode::Auto | AccessMode::AutoAcceptEdits
                ))
        {
            return self.respond(
                Control {
                    id,
                    decision: Decision::Allow,
                },
                stdin,
            );
        }
        events.push(Event::Approval(Approval {
            id,
            title,
            detail,
            can_remember: self.request.kind != Kind::Codex && !file_change,
        }));
        Ok(())
    }
    fn respond(&mut self, control: Control, stdin: &mut impl Write) -> Result<(), String> {
        if matches!(control.decision, Decision::KeepOpen) {
            if let Some((_, _, _, question)) = self.questions.get_mut(&control.id) {
                question.auto_resolve_at = None;
            }
            return Ok(());
        }
        if let Some((wire, kind, request, mut question)) = self.questions.remove(&control.id) {
            if self.visible_question.as_deref() == Some(control.id.as_str()) {
                self.visible_question = None;
            }
            if kind == "codex-async" {
                if let Decision::Answer(answers) = &control.decision {
                    let text = question
                        .items
                        .iter()
                        .zip(answers)
                        .filter(|(_, a)| !a.is_empty())
                        .map(|(q, a)| format!("{}\n{}", q.prompt, a.join("\n")))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if !text.is_empty() && request["turnId"].as_str() == self.active_turn.as_deref()
                    {
                        question.auto_resolve_at = None;
                        let id = format!("native-answer-{}-{}", question.id, crate::time::now_ms());
                        write_json(
                            stdin,
                            json!({"id":id,"method":"turn/steer","params":{"threadId":request["threadId"],"expectedTurnId":request["turnId"],"input":[{"type":"text","text":text}]}}),
                        )?;
                        self.async_answers
                            .insert(id, (wire, kind, request, question));
                    }
                }
                return Ok(());
            }
            return questions::respond(stdin, wire, &kind, &request, &question, control.decision);
        }
        let Some((wire, kind, request)) = self.approvals.remove(&control.id) else {
            return Ok(());
        };
        let allow = matches!(control.decision, Decision::Allow | Decision::Session);
        let session = matches!(control.decision, Decision::Session);
        if kind == "acp" {
            return acp::respond(stdin, wire, &request, control.decision);
        }
        if kind == "codex-mcp" {
            return write_json(
                stdin,
                json!({"id":wire,"result":{"action":if allow {"accept"} else {"decline"},"content":if allow {request["nativeContent"].clone()} else {Value::Null},"_meta":null}}),
            );
        }
        if kind == "pi" {
            return write_json(
                stdin,
                json!({"type":"extension_ui_response","id":wire,"confirmed":allow,"cancelled":!allow}),
            );
        }
        if kind == "claude" {
            let mut response = if allow {
                json!({"behavior":"allow","updatedInput":request["input"]})
            } else {
                json!({"behavior":"deny","message":"Denied by the user"})
            };
            if session && allow {
                if let Some(suggestions) = request.get("permission_suggestions") {
                    response["updatedPermissions"] = suggestions.clone();
                }
            }
            write_json(
                stdin,
                json!({"type":"control_response","response":{"subtype":"success","request_id":wire,"response":response}}),
            )
        } else {
            let result = if kind.contains("permissions") {
                json!({"permissions": if allow { request["permissions"].clone() } else { json!({}) }, "scope":if self.request.settings.access == AccessMode::FullAccess { "session" } else { "turn" }})
            } else if kind == "execCommandApproval" || kind == "applyPatchApproval" {
                json!({"decision":if !allow { "denied" } else if session { "approved_for_session" } else { "approved" }})
            } else {
                json!({"decision":if !allow { "decline" } else { "accept" }})
            };
            write_json(stdin, json!({"id":wire,"result":result}))
        }
    }
    fn codex_tool(&mut self, item: &Value, completed: bool, events: &mut Vec<Event>) {
        let name = item["type"].as_str().unwrap_or("");
        if name == "contextCompaction" {
            self.compaction(
                item["id"].as_str(),
                if completed {
                    ToolState::Completed
                } else {
                    ToolState::Running
                },
                events,
            );
            return;
        }
        if !matches!(
            name,
            "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch" | "dynamicToolCall"
        ) {
            return;
        }
        let id = item["id"].as_str().unwrap_or("");
        let tool = self
            .tools
            .entry(id.into())
            .or_insert_with(|| ToolCall::new(id, name));
        tool.name = item["tool"].as_str().unwrap_or(name).to_owned();
        if name == "mcpToolCall" {
            tool.name = format!("{}:{}", item["server"].as_str().unwrap_or("mcp"), tool.name);
        }
        let previous_input = tool.input.clone();
        tool.input = item["command"]
            .as_str()
            .or_else(|| item["query"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                serde_json::to_string_pretty(
                    item.get("arguments")
                        .or_else(|| item.get("changes"))
                        .unwrap_or(&Value::Null),
                )
                .unwrap_or_default()
            });
        if name == "commandExecution" {
            // Keep argv and parsed provider metadata intact. Presentation unwraps
            // recognized launchers without changing the recorded execution.
            let previous = serde_json::from_str::<Value>(&previous_input).ok();
            let mut input = previous
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            for field in [
                "command",
                "commandActions",
                "parsed_cmd",
                "parsedCmd",
                "cwd",
                "exitCode",
                "durationMs",
            ] {
                if let Some(value) = item.get(field) {
                    input[field] = value.clone();
                }
            }
            tool.input = input.to_string();
        }
        if let Some(changes) = item["changes"].as_array().filter(|_| name == "fileChange") {
            tool.input = json!({
                "path": changes.first().and_then(|c| c["path"].as_str()).unwrap_or("Files"),
                "paths": changes.iter().filter_map(|c| c["path"].as_str()).collect::<Vec<_>>(),
            })
            .to_string();
            tool.output = changes
                .iter()
                .map(|c| {
                    format!(
                        "{}\n{}",
                        c["path"].as_str().unwrap_or(""),
                        c["diff"].as_str().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        if let Some(output) = item["aggregatedOutput"]
            .as_str()
            .or_else(|| item["output"].as_str())
        {
            tool.output = output.to_owned();
        } else if let Some(result) = item.get("result") {
            tool.output = serde_json::to_string_pretty(result).unwrap_or_default();
        }
        tool.bound_output();
        tool.state = match item["status"].as_str().unwrap_or(if completed {
            "completed"
        } else {
            "inProgress"
        }) {
            "completed" => ToolState::Completed,
            "failed" | "declined" => ToolState::Failed,
            "cancelled" | "canceled" | "interrupted" | "aborted" => ToolState::Cancelled,
            _ => ToolState::Running,
        };
        if item["exitCode"].as_i64().is_some_and(|code| code != 0) {
            tool.state = ToolState::Failed;
        }
        events.push(Event::Tool(tool.clone()));
    }
}

/// Stream at bounded memory cost. `emit` implements backpressure and must observe
/// cancellation while waiting for the viewport. Errors are returned after reaping.
pub fn run(
    request: Request,
    cancel: Arc<AtomicBool>,
    emit: impl FnMut(Vec<Event>) -> Result<(), String>,
) -> Result<(), String> {
    run_with_controls(request, cancel, None, emit)
}
pub fn run_with_controls(
    request: Request,
    cancel: Arc<AtomicBool>,
    controls: Option<mpsc::Receiver<Control>>,
    emit: impl FnMut(Vec<Event>) -> Result<(), String>,
) -> Result<(), String> {
    run_process(request, cancel, controls, false, emit).map(|_| ())
}

/// A bounded, separate conversation for titles and other background text work.
/// Never resume the user's conversation or inherit its app capabilities/tools.
pub fn run_text(mut request: Request, cancel: Arc<AtomicBool>) -> Result<String, String> {
    request.provider_id = None;
    request.settings = Settings {
        account: request.settings.account.clone(),
        text_only: true,
        access: AccessMode::ReadOnly,
        effort: (request.kind == Kind::Codex).then(|| "low".into()),
        ..Default::default()
    };
    let mut output = String::new();
    run(request, cancel, |events| {
        for event in events {
            match event {
                Event::Text(text) if output.len() + text.len() <= 64 * 1024 => {
                    output.push_str(&text)
                }
                Event::Text(_) => return Err("Background text response exceeded its limit".into()),
                Event::Tool(tool) if tool.is_compaction() => {}
                Event::Tool(_) | Event::Approval(_) | Event::Question(_) => {
                    return Err(
                        "Background text request attempted to use tools or request input".into(),
                    );
                }
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(output)
}

pub(crate) fn catalog_for(kind: Kind, account: crate::accounts::Profile) -> Result<Value, String> {
    let binary = resolve(kind).ok_or_else(|| kind.unavailable_hint())?;
    let cwd = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?);
    run_process(
        Request {
            kind,
            binary,
            cwd,
            prompt: String::new(),
            provider_id: None,
            model: None,
            settings: Settings {
                account,
                access: AccessMode::ReadOnly,
                ..Default::default()
            },
        },
        Arc::new(AtomicBool::new(false)),
        None,
        true,
        |_| Ok(()),
    )?
    .ok_or_else(|| format!("{} returned no model catalog", kind.label()))
}

fn run_process(
    request: Request,
    cancel: Arc<AtomicBool>,
    controls: Option<mpsc::Receiver<Control>>,
    probe: bool,
    emit: impl FnMut(Vec<Event>) -> Result<(), String>,
) -> Result<Option<Value>, String> {
    run_operation(request, cancel, controls, probe, false, false, emit)
}

fn run_operation(
    request: Request,
    cancel: Arc<AtomicBool>,
    controls: Option<mpsc::Receiver<Control>>,
    probe: bool,
    compact: bool,
    command_probe: bool,
    mut emit: impl FnMut(Vec<Event>) -> Result<(), String>,
) -> Result<Option<Value>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    if request.kind == Kind::OpenCode {
        return opencode::run(request, cancel, controls, probe, compact, emit);
    }
    let claude_new_session = (request.kind == Kind::Claude
        && request.provider_id.is_none()
        && !request.settings.text_only)
        .then(lifecycle::claude_session_id);
    #[cfg(unix)]
    if std::env::var_os("MONOCODE_NATIVE_TRACE").is_some() {
        unsafe {
            let mut signals = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_SETMASK, std::ptr::null(), &mut signals);
            eprintln!(
                "provider-launch sigchld_blocked={}",
                libc::sigismember(&signals, libc::SIGCHLD)
            );
        }
    }
    let mut command = Command::new(&request.binary);
    crate::scm::shell::apply_gui_env(&mut command);
    command.envs(&request.settings.environment);
    request.settings.account.apply(request.kind, &mut command)?;
    match request.kind {
        Kind::Codex => {
            command.arg("app-server");
            crate::visual_bridge::configure_codex(&mut command, &request.settings.environment);
            if let Some(mono) = &request.settings.mono {
                if let Some(home) =
                    crate::mono::codex_home(&mono.root, &request.settings.account.id)?
                {
                    let sqlite_home = serde_json::to_string(&home).map_err(|e| e.to_string())?;
                    command
                        .env("CODEX_HOME", &home)
                        .env("CODEX_SQLITE_HOME", &home)
                        .args(["-c", &format!("sqlite_home={sqlite_home}")]);
                }
            }
        }
        Kind::Claude => {
            command.args([
                "--verbose",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
                "--permission-prompt-tool",
                "stdio",
            ]);
            let settings = &request.settings;
            let mode = if settings.plan {
                "plan"
            } else {
                settings.access.claude()
            };
            command.args(["--permission-mode", mode]);
            if settings.text_only {
                command.args([
                    "--tools",
                    "",
                    "--max-turns",
                    "1",
                    "--no-session-persistence",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "{\"mcpServers\":{}}",
                ]);
            }
            let visual_server = (!settings.text_only)
                .then(|| crate::visual_bridge::mcp_server(&settings.environment))
                .flatten();
            let mut allowed = Vec::new();
            if !settings.text_only && settings.access == AccessMode::ReadOnly {
                command.args(["--tools", "Read,Glob,Grep,Bash"]);
                allowed.push("Read,Glob,Grep,Bash(git status *),Bash(git diff *),Bash(git log *),Bash(git show *),Bash(pwd),Bash(ls *),Bash(rg *)");
            }
            if let Some(server) = visual_server {
                command.args([
                    "--mcp-config",
                    &json!({"mcpServers":{"monocode_visual":server}}).to_string(),
                ]);
                allowed.push(crate::visual_bridge::CLAUDE_ALLOWED_TOOLS);
                if let Some(instructions) =
                    crate::visual_bridge::instructions(&settings.environment, true)
                {
                    command.args(["--append-system-prompt", &instructions]);
                }
            }
            if !allowed.is_empty() {
                command.args(["--allowedTools", &allowed.join(",")]);
            }
            if mode == "bypassPermissions" {
                command.arg("--allow-dangerously-skip-permissions");
            }
            let mut config = json!({});
            if settings.disable_claude_hooks || settings.text_only {
                config["disableAllHooks"] = true.into();
            }
            if settings.fast {
                config["fastMode"] = true.into();
            }
            if settings.thinking == Some(true) {
                config["alwaysThinkingEnabled"] = true.into();
            }
            if let Some(effort) = settings.effort.as_deref().filter(|e| *e != "ultrathink") {
                let effort = if effort == "ultracode" {
                    config["ultracode"] = true.into();
                    "xhigh"
                } else if effort == "xhigh"
                    && request.model.as_deref().is_some_and(|m| {
                        matches!(
                            m,
                            "claude-opus-4-6"
                                | "claude-sonnet-4-6"
                                | "claude-opus-4-5"
                                | "claude-haiku-4-5"
                                | "claude-opus-4-1"
                                | "claude-opus-4-0"
                                | "claude-sonnet-4-0"
                                | "claude-sonnet-4-5"
                        )
                    })
                {
                    "max"
                } else if effort == "max" && request.model.as_deref() == Some("claude-sonnet-4-6") {
                    "high"
                } else {
                    effort
                };
                command.args(["--effort", effort]);
            }
            command.args(["--settings", &config.to_string()]);
            if !settings.text_only {
                command.arg("--setting-sources=user,project,local");
            }
            if let Some(id) = &request.provider_id {
                command.args(["--resume", id]);
            } else if let Some(id) = &claude_new_session {
                command.args(["--session-id", id]);
            }
            if let Some(model) = &request.model {
                let model = if settings
                    .model_settings
                    .get("context")
                    .is_some_and(|value| value == "1m")
                    && !model.ends_with("[1m]")
                {
                    format!("{model}[1m]")
                } else {
                    model.clone()
                };
                command.args(["--model", &model]);
            }
        }
        kind if kind.is_pi() => pi::configure(&mut command, &request, probe || command_probe),
        _ => acp::configure(&mut command, &request)?,
    }
    command
        .current_dir(if request.kind == Kind::Antigravity {
            request.binary.parent().unwrap_or(&request.cwd)
        } else {
            &request.cwd
        })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::isolate(&mut command);
    if cancel.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    let eligible = !probe && !command_probe && !request.settings.text_only;
    let cached = eligible
        .then(|| parked::take(&parked::key(&request)))
        .flatten();
    let (mut wire, mut protocol, warm) = if let Some(cached) = cached {
        let mut protocol = Protocol::new(request);
        protocol.acp = cached.protocol.acp;
        protocol.pi = cached.protocol.pi;
        protocol.previous_turn = cached.protocol.active_turn;
        (cached.wire, protocol, true)
    } else {
        if eligible {
            // A new process supersedes older warm launch configurations and
            // any cancelled process still draining for this provider binding.
            parked::discard_session(&request);
        }
        (
            parked::Wire::spawn(&mut command, &request)?,
            Protocol::new(request),
            false,
        )
    };
    let rx = &wire.rx;
    let tail = &wire.tail;
    let mut stdin = &mut wire.stdin;
    let mut pending = Vec::new();
    if let Some(id) = claude_new_session {
        protocol.request.provider_id = Some(id.clone());
        pending.push(Event::SessionId(id));
    }
    let mut stop_confirmed = false;
    let result = (|| {
        protocol.probe = probe;
        protocol.compact = compact;
        protocol.command_probe = command_probe;
        if warm {
            if protocol.request.kind.is_acp() {
                protocol.acp_resume_warm(&mut stdin)?;
            } else if protocol.request.kind.is_pi() {
                protocol.pi_start(&mut stdin)?;
            } else if protocol.request.kind == Kind::Codex {
                protocol.initialized = true;
                protocol.frame(json!({"id":1,"result":{}}), &mut stdin)?;
            } else {
                protocol.follow_up_ready = true;
                protocol.frame(
                    json!({"type":"control_response","response":{"request_id":"native-init"}}),
                    &mut stdin,
                )?;
            }
        } else if protocol.request.kind.is_acp() {
            protocol.acp_start(&mut stdin)?;
        } else if protocol.request.kind.is_pi() {
            protocol.pi_start(&mut stdin)?;
        } else if protocol.request.kind == Kind::Codex {
            write_json(
                &mut stdin,
                json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"monocode_native","title":"MonoCode Native","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}}),
            )?;
        } else {
            write_json(
                &mut stdin,
                json!({"type":"control_request","request_id":"native-init","request":{"subtype":"initialize"}}),
            )?;
        }
        let mut pending_bytes = 0;
        let mut last_emit = Instant::now();
        let started = Instant::now();
        let mut ready_emitted = false;
        let mut interrupted = false;
        loop {
            if cancel.load(Ordering::Relaxed) && !interrupted {
                interrupted = true;
                if !protocol.interrupt(&mut stdin)? {
                    return Err("Stopped".into());
                }
                if protocol.request.kind == Kind::Claude {
                    return Err("Stopped".into());
                }
            }
            if !interrupted {
                if let Some(controls) = &controls {
                    while let Ok(control) = controls.try_recv() {
                        if let Decision::Steer(message) = control.decision {
                            let id = message.id.clone();
                            match protocol.steer(message, &mut stdin) {
                                Ok(events) => pending.extend(events),
                                Err(error) => pending.push(Event::FollowUpFailed { id, error }),
                            }
                        } else {
                            protocol.respond(control, &mut stdin)?;
                        }
                    }
                }
                pending.extend(protocol.expire_steering());
                pending.extend(protocol.lifecycle_tick(started, &mut stdin)?);
                if protocol.ready() && !ready_emitted {
                    ready_emitted = true;
                    pending.push(Event::Ready);
                }
            }
            if interrupted && protocol.request.kind.is_pi() {
                let _ = protocol.pi_tick(&mut stdin);
            }
            if interrupted && protocol.done {
                stop_confirmed = true;
                return Err("Stopped".into());
            }
            if protocol.request.settings.text_only && started.elapsed() >= Duration::from_secs(45) {
                return Err("Background text request timed out".into());
            }
            match rx.recv_timeout(TICK) {
                Ok(Ok(line)) => {
                    let value: Value = match serde_json::from_slice(&line) {
                        Ok(value) => value,
                        // Some CLIs print startup banners before their NDJSON stream.
                        Err(_)
                            if !line
                                .iter()
                                .copied()
                                .find(|b| !b.is_ascii_whitespace())
                                .is_some_and(|b| b == b'{') =>
                        {
                            continue;
                        }
                        Err(e) => return Err(format!("Invalid provider JSON: {e}")),
                    };
                    if std::env::var_os("MONOCODE_NATIVE_TRACE").is_some() {
                        let method = value["method"].as_str().unwrap_or("");
                        let item = &value["params"]["item"];
                        if !method.is_empty()
                            && !method.ends_with("Delta")
                            && !method.ends_with("/delta")
                        {
                            eprintln!(
                                "provider-frame method={method} type={} status={} id={} output_bytes={} keys={:?} legacy={}",
                                item["type"],
                                item["status"],
                                item["id"],
                                item["aggregatedOutput"].as_str().map_or(0, str::len),
                                value["params"]
                                    .as_object()
                                    .map(|o| o.keys().collect::<Vec<_>>()),
                                value["params"]["msg"]["type"]
                            );
                        }
                    }
                    if interrupted {
                        let reply_id = value["id"].as_str();
                        let turn_id = value["params"]["turnId"]
                            .as_str()
                            .or_else(|| value["params"]["turn"]["id"].as_str());
                        let terminal = matches!(
                            value["method"].as_str(),
                            Some("turn/completed" | "turn/aborted")
                        ) && (turn_id.is_some()
                            && turn_id == protocol.active_turn.as_deref()
                            || turn_id.is_none() && protocol.previous_turn.is_none());
                        let codex_ack = protocol.request.kind == Kind::Codex
                            && (reply_id.is_some() && reply_id == protocol.interrupt_id.as_deref()
                                || terminal);
                        let claude_end =
                            protocol.request.kind == Kind::Claude && value["type"] == "result";
                        if codex_ack || claude_end {
                            stop_confirmed = value.get("error").is_none_or(Value::is_null);
                            if terminal || claude_end {
                                protocol.done = true;
                            }
                            return Err("Stopped".into());
                        }
                    }
                    let events = match protocol.frame(value, &mut stdin) {
                        Ok(events) => events,
                        Err(_) if interrupted => return Err("Stopped".into()),
                        Err(error) => return Err(error),
                    };
                    for event in events {
                        if controls.is_none() || interrupted {
                            if let Event::Approval(approval) = &event {
                                protocol.respond(
                                    Control {
                                        id: approval.id.clone(),
                                        decision: Decision::Deny,
                                    },
                                    &mut stdin,
                                )?;
                            }
                            if let Event::Question(question) = &event {
                                protocol.respond(
                                    Control {
                                        id: question.id.clone(),
                                        decision: Decision::Deny,
                                    },
                                    &mut stdin,
                                )?;
                            }
                        }
                        if interrupted {
                            continue;
                        }
                        pending_bytes += match &event {
                            Event::Text(text) | Event::Reasoning(text) | Event::Status(text) => {
                                text.len()
                            }
                            Event::Tool(tool) => tool.input.len() + tool.output.len(),
                            Event::Approval(approval) => approval.detail.len(),
                            Event::Question(question) => {
                                question.items.iter().map(|q| q.prompt.len()).sum()
                            }
                            _ => 64,
                        };
                        if let Event::SessionId(id) = &event {
                            protocol.request.provider_id = Some(id.clone());
                        }
                        pending.push(event);
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if interrupted {
                        return Err("Stopped".into());
                    }
                    return Err(format!(
                        "Provider exited before completing the turn. {}",
                        String::from_utf8_lossy(&tail.lock().unwrap())
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if !pending.is_empty()
                && (last_emit.elapsed() >= TICK || pending_bytes >= BATCH_BYTES || protocol.done)
            {
                emit(std::mem::take(&mut pending))?;
                pending_bytes = 0;
                last_emit = Instant::now();
            }
            // A visual publication may arrive while the provider is waiting
            // for its tool result and therefore emits no stream events.
            if crate::visual_bridge::configured(&protocol.request.settings.environment) {
                emit(Vec::new())?;
            }
            if protocol.done {
                if interrupted {
                    stop_confirmed = true;
                    return Err("Stopped".into());
                }
                return Ok(protocol.catalog_result.take());
            }
            if command_probe && started.elapsed() > Duration::from_secs(45) {
                return Err("Command discovery timed out".into());
            }
            let discovery_timeout = match protocol.request.kind {
                Kind::Cursor | Kind::Grok => Some(15),
                Kind::Hermes => Some(30),
                Kind::Pi | Kind::Omp => Some(45),
                Kind::Antigravity => None,
                _ => Some(60),
            };
            if probe
                && discovery_timeout
                    .is_some_and(|seconds| started.elapsed() > Duration::from_secs(seconds))
            {
                return Err("Model discovery timed out".into());
            }
        }
    })();
    // Cancellation can arrive while publishing an event batch. Its callback
    // then returns Stopped before the next receive-loop iteration can interrupt.
    if cancel.load(Ordering::Relaxed) && protocol.interrupt_id.is_none() {
        let _ = protocol.interrupt(&mut wire.stdin);
    }
    if cancel.load(Ordering::Relaxed) && protocol.done {
        stop_confirmed = true;
    }
    if result.is_err() && !cancel.load(Ordering::Relaxed) {
        if let Some(limit) = protocol.claude_usage_limit.take() {
            pending.push(Event::UsageLimited(limit));
        }
    }
    if !pending.is_empty() {
        let _ = emit(std::mem::take(&mut pending));
    }
    if eligible
        && cancel.load(Ordering::Relaxed)
        && !protocol.done
        && protocol.request.kind != Kind::Antigravity
        && protocol.interrupt_id.is_some()
    {
        parked::settle_cancelled(wire, protocol);
    } else if eligible
        && ((result.is_ok() && !cancel.load(Ordering::Relaxed))
            || (stop_confirmed && protocol.request.kind != Kind::Antigravity))
    {
        parked::put(wire, protocol);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn protocol(kind: Kind) -> Protocol {
        Protocol::new(Request {
            kind,
            binary: "unused".into(),
            cwd: "/tmp".into(),
            prompt: "Hello".into(),
            provider_id: None,
            model: None,
            settings: Default::default(),
        })
    }
    #[test]
    fn codex_file_changes_retain_every_edited_path() {
        let mut protocol = protocol(Kind::Codex);
        let mut events = Vec::new();
        protocol.codex_tool(&json!({
            "type":"fileChange", "id":"edit", "status":"completed",
            "changes":[{"path":"/repo/a.rs", "diff":"first"}, {"path":"/repo/b.rs", "diff":"second"}]
        }), true, &mut events);
        let Event::Tool(tool) = &events[0] else {
            panic!("missing edit tool")
        };
        let input: Value = serde_json::from_str(&tool.input).unwrap();
        assert_eq!(tool.state, ToolState::Completed);
        assert_eq!(input["path"], "/repo/a.rs");
        assert_eq!(input["paths"], json!(["/repo/a.rs", "/repo/b.rs"]));
    }
    #[test]
    fn operator_thread_access_keeps_the_codex_app_bridge_reachable_on_later_turns() {
        for plan in [false, true] {
            let mut p = protocol(Kind::Codex);
            p.request.settings.operator_enabled = true;
            p.request.settings.plan = plan;
            assert!(!p.request.settings.operator);
            assert_eq!(p.codex_access().2["networkAccess"], true);
            let mut out = Vec::new();
            p.frame(json!({"id":1,"result":{}}), &mut out).unwrap();
            p.frame(json!({"id":2,"result":{"thread":{"id":"saved"}}}), &mut out)
                .unwrap();
            let started: Value = String::from_utf8_lossy(&out)
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|message| message["method"] == "turn/start")
                .unwrap();
            assert_eq!(started["params"]["sandboxPolicy"]["networkAccess"], true);
            if plan {
                assert_eq!(started["params"]["sandboxPolicy"]["type"], "readOnly");
            }
        }
    }
    #[test]
    fn context_reports_use_the_latest_request_and_include_cached_tokens() {
        let mut codex = protocol(Kind::Codex);
        let events = codex.frame(json!({"method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"totalTokens":4200},"total":{"totalTokens":99000},"modelContextWindow":272000}}}),&mut Vec::new()).unwrap();
        assert_eq!(
            events,
            vec![Event::Context {
                used: Some(4200),
                window: Some(272000)
            }]
        );
        let mut claude = protocol(Kind::Claude);
        let events = claude.frame(json!({"type":"assistant","message":{"content":[],"usage":{"input_tokens":100,"cache_read_input_tokens":9000,"cache_creation_input_tokens":500,"output_tokens":25}}}),&mut Vec::new()).unwrap();
        assert_eq!(
            events,
            vec![Event::Context {
                used: Some(9625),
                window: None
            }]
        );
        assert!(claude.frame(json!({"type":"assistant","parent_tool_use_id":"subagent","message":{"content":[],"usage":{"input_tokens":999999}}}),&mut Vec::new()).unwrap().is_empty());
        let events = claude.frame(json!({"type":"result","usage":{"input_tokens":90000,"iterations":[{"input_tokens":1000},{"input_tokens":2000,"output_tokens":50}]},"modelUsage":{"model":{"contextWindow":200000}}}),&mut Vec::new()).unwrap();
        assert!(events.contains(&Event::Context {
            used: Some(2050),
            window: Some(200000)
        }));
    }
    #[test]
    fn codex_handshake_stream_snapshot_and_resume() {
        let mut p = protocol(Kind::Codex);
        p.request.provider_id = Some("saved".into());
        let mut out = Vec::new();
        p.frame(json!({"id":1,"result":{}}), &mut out).unwrap();
        assert!(String::from_utf8_lossy(&out).contains("thread/resume"));
        let resumed: Value = String::from_utf8_lossy(&out)
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|message| message["method"] == "thread/resume")
            .unwrap();
        assert_eq!(resumed["params"]["threadId"], "saved");
        assert_eq!(resumed["params"]["excludeTurns"], true);
        p.frame(json!({"id":2,"result":{"thread":{"id":"saved"}}}), &mut out)
            .unwrap();
        let e = p
            .frame(
                json!({"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"Hi\n"}}),
                &mut out,
            )
            .unwrap();
        assert_eq!(e, vec![Event::BeginAssistant, Event::Text("Hi\n".into())]);
        assert_eq!(p.frame(json!({"method":"item/completed","params":{"item":{"type":"agentMessage","id":"a","text":"Hi\n\n"}}}), &mut out).unwrap(),vec![Event::Text("\n".into())]);
        assert_eq!(
            p.frame(
                json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}),
                &mut out
            )
            .unwrap(),
            vec![Event::Done]
        );
    }
    #[test]
    fn claude_stream_and_completed_snapshot_do_not_duplicate() {
        let mut p = protocol(Kind::Claude);
        let mut out = Vec::new();
        p.frame(
            json!({"type":"stream_event","event":{"type":"message_start"}}),
            &mut out,
        )
        .unwrap();
        p.frame(json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello 🙂"}}}), &mut out).unwrap();
        assert!(p.frame(json!({"type":"assistant","message":{"content":[{"type":"text","text":"Hello 🙂"}]}}), &mut out).unwrap().is_empty());
        assert!(p
            .frame(
                json!({"type":"result","is_error":true,"result":"Login required"}),
                &mut out
            )
            .unwrap_err()
            .contains("Login"));
    }
    #[test]
    fn unknown_server_requests_are_resolved_and_errors_surface() {
        let mut p = protocol(Kind::Codex);
        let mut out = Vec::new();
        p.frame(
            json!({"id":"req-1","method":"unsupported/request","params":{}}),
            &mut out,
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&out).contains("req-1"));
        assert!(p
            .frame(
                json!({"id":2,"error":{"message":"Resume failed"}}),
                &mut out
            )
            .unwrap_err()
            .contains("Resume failed"));
    }
    #[test]
    fn approvals_wait_for_a_decision_and_keep_the_requested_input() {
        let mut codex = protocol(Kind::Codex);
        let mut out = Vec::new();
        let events = codex.frame(json!({"id":"permission-1","method":"item/commandExecution/requestApproval","params":{"command":"cargo build","itemId":"c1"}}), &mut out).unwrap();
        let Event::Approval(approval) = &events[0] else {
            panic!("expected an approval")
        };
        assert!(
            out.is_empty(),
            "never answer an approval before a native decision"
        );
        codex
            .respond(
                Control {
                    id: approval.id.clone(),
                    decision: Decision::Deny,
                },
                &mut out,
            )
            .unwrap();
        let response: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(response["id"], "permission-1");
        assert_eq!(response["result"]["decision"], "decline");
        let mut claude = protocol(Kind::Claude);
        out.clear();
        let input = json!({"file_path":"/work/example.rs","content":"approved"});
        let events = claude.frame(json!({"type":"control_request","request_id":"permission-2","request":{"subtype":"can_use_tool","tool_name":"Write","input":input}}), &mut out).unwrap();
        let Event::Approval(approval) = &events[0] else {
            panic!("expected an approval")
        };
        assert!(out.is_empty());
        claude
            .respond(
                Control {
                    id: approval.id.clone(),
                    decision: Decision::Allow,
                },
                &mut out,
            )
            .unwrap();
        let response: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(response["response"]["request_id"], "permission-2");
        assert_eq!(response["response"]["response"]["behavior"], "allow");
        assert_eq!(response["response"]["response"]["updatedInput"], input);
    }
    #[test]
    fn selected_model_effort_and_access_reach_the_codex_turn() {
        let mut p = protocol(Kind::Codex);
        p.request.model = Some("selected-model".into());
        p.request.settings.effort = Some("high".into());
        p.request.settings.access = AccessMode::AutoAcceptEdits;
        let mut out = Vec::new();
        p.frame(json!({"id":1,"result":{}}), &mut out).unwrap();
        let rows: Vec<Value> = String::from_utf8_lossy(&out)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows[1]["params"]["sandbox"], "workspace-write");
        out.clear();
        p.frame(
            json!({"id":2,"result":{"thread":{"id":"thread"},"model":"selected-model"}}),
            &mut out,
        )
        .unwrap();
        let row: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(row["params"]["model"], "selected-model");
        assert_eq!(row["params"]["effort"], "high");
        assert_eq!(row["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(row["params"]["approvalsReviewer"], "user");
    }
    #[test]
    fn codex_tool_deltas_settle_to_one_completed_snapshot() {
        let mut p = protocol(Kind::Codex);
        let mut out = Vec::new();
        let started = p.frame(json!({"method":"item/started","params":{"item":{"id":"cmd-1","type":"commandExecution","command":"cargo check","status":"inProgress"}}}), &mut out).unwrap();
        assert!(
            matches!(&started[0], Event::Tool(t) if t.state == ToolState::Running && t.summary() == ("Run".into(), "cargo check".into()))
        );
        p.frame(json!({"method":"item/commandExecution/outputDelta","params":{"itemId":"cmd-1","delta":"Checking 🙂"}}), &mut out).unwrap();
        let completed = p.frame(json!({"method":"item/completed","params":{"item":{"id":"cmd-1","type":"commandExecution","command":"cargo check","status":"completed","aggregatedOutput":"Checking 🙂\nFinished"}}}), &mut out).unwrap();
        assert!(
            matches!(&completed[0], Event::Tool(t) if t.id == "cmd-1" && t.state == ToolState::Completed && t.output == "Checking 🙂\nFinished")
        );
        assert_eq!(p.tools.len(), 1);
    }
    #[test]
    fn codex_live_tool_preserves_argv_and_semantic_rollout_metadata() {
        let mut protocol = protocol(Kind::Codex);
        let mut out = Vec::new();
        let command = json!(["/bin/zsh", "-lc", "sed -n '8,20p' 'src/my file.rs'"]);
        let events = protocol.frame(json!({"method":"item/started","params":{"item":{"id":"read","type":"commandExecution","command":command,"parsed_cmd":[{"type":"read","path":"/repo/src/my file.rs"}],"cwd":"/repo","status":"inProgress"}}}), &mut out).unwrap();
        let Event::Tool(call) = &events[0] else {
            panic!()
        };
        assert_eq!(call.summary(), ("Read".into(), "src/my file.rs".into()));
        assert_eq!(
            call.presentation().command.as_deref(),
            Some("sed -n '8,20p' 'src/my file.rs'")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&call.input).unwrap()["command"],
            command
        );
        let events = protocol.frame(json!({"method":"item/completed","params":{"item":{"id":"read","type":"commandExecution","status":"failed","aggregatedOutput":"permission denied","exitCode":1}}}), &mut out).unwrap();
        let Event::Tool(call) = &events[0] else {
            panic!()
        };
        assert_eq!(call.summary().0, "Read");
        assert_eq!(call.output, "permission denied");
        assert_eq!(call.state, ToolState::Failed);
        assert_eq!(
            serde_json::from_str::<Value>(&call.input).unwrap()["command"],
            command
        );
    }
    #[test]
    fn claude_tool_input_and_result_keep_identity_and_failure() {
        let mut p = protocol(Kind::Claude);
        let mut out = Vec::new();
        p.frame(json!({"type":"stream_event","event":{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"read-1","name":"Read"}}}), &mut out).unwrap();
        p.frame(json!({"type":"stream_event","event":{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"file_path\":\"src/main.rs\"}"}}}), &mut out).unwrap();
        let input = p.tools["read-1"].summary();
        assert_eq!(input, ("Read".into(), "src/main.rs".into()));
        p.frame(json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"read-1","name":"Read","input":{"file_path":"src/main.rs"}}]}}), &mut out).unwrap();
        let events = p.frame(json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"read-1","is_error":true,"content":[{"type":"text","text":"File missing"}]}]}}), &mut out).unwrap();
        assert!(
            matches!(&events[0], Event::Tool(t) if t.id == "read-1" && t.state == ToolState::Failed && t.output == "File missing")
        );
        assert_eq!(p.tools.len(), 1);
    }
    #[test]
    fn claude_partial_tool_input_waits_until_it_parses() {
        let mut p = protocol(Kind::Claude);
        let mut out = Vec::new();
        p.frame(json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"sh","name":"Bash"}}}), &mut out).unwrap();
        let events = p.frame(json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\": \"git status"}}}), &mut out).unwrap();
        assert!(events.is_empty());
        assert_eq!(p.tools["sh"].input, "");
        let events = p.frame(json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":" --short\"}"}}}), &mut out).unwrap();
        let Event::Tool(call) = &events[0] else {
            panic!()
        };
        assert_eq!(call.input, r#"{"command": "git status --short"}"#);
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture(script: &str) -> (tempfile::TempDir, Request) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("provider");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let request = Request {
            kind: Kind::Codex,
            binary: path,
            cwd: tmp.path().into(),
            prompt: "hello".into(),
            provider_id: None,
            model: None,
            settings: Default::default(),
        };
        (tmp, request)
    }
    #[test]
    fn cancellation_reaps_child_and_descendant_promptly() {
        for kind in Kind::ALL {
            let (_tmp, mut request) =
                fixture("#!/bin/sh\nsleep 30 &\nprintf ready > ready\nwait\n");
            request.kind = kind;
            request.settings.access = AccessMode::ReadOnly;
            let ready = request.cwd.join("ready");
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            let timer = thread::spawn(move || {
                // Wait until the descendant exists, then measure cancellation
                // independently of provider startup and test-runner load.
                let deadline = Instant::now() + Duration::from_secs(10);
                while !ready.exists() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                let started = ready.exists();
                let stopped_at = Instant::now();
                stop.store(true, Ordering::Relaxed);
                (started, stopped_at)
            });
            assert_eq!(run(request, cancel, |_| Ok(())).unwrap_err(), "Stopped");
            let (started, stopped_at) = timer.join().unwrap();
            assert!(started, "{} fixture did not start", kind.label());
            assert!(
                stopped_at.elapsed() < Duration::from_secs(2),
                "{} cancellation must not wait on provider output",
                kind.label()
            );
        }
    }
    #[test]
    fn crash_preserves_coalesced_partial_text() {
        // Read initialization before exiting so startup cannot race a closed stdin.
        let (_tmp, request) = fixture(
            "#!/bin/sh\nIFS= read -r init\nprintf '%s\\n' '{\"method\":\"item/agentMessage/delta\",\"params\":{\"itemId\":\"a\",\"delta\":\"partial\"}}'\n",
        );
        let mut text = String::new();
        let result = run(request, Arc::new(AtomicBool::new(false)), |events| {
            for event in events {
                if let Event::Text(t) = event {
                    text.push_str(&t);
                }
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("exited"));
        assert_eq!(text, "partial");
    }
    #[test]
    fn large_history_and_tool_frames_complete_the_turn() {
        let (_tmp, mut request) = fixture(
            r#"#!/usr/bin/env python3
import json, sys
payload = 'x' * (3 * 1024 * 1024) + 'end'
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    value = json.loads(line)
    if value.get('id') == 1:
        send({'id': 1, 'result': {}})
    elif value.get('id') == 2:
        assert value['method'] == 'thread/resume'
        send({'id': 2, 'result': {'thread': {'id': 'large-thread', 'turns': [
            {'items': [{'id': 'history', 'type': 'agentMessage', 'text': payload}]}
        ]}}})
    elif value.get('id') == 3:
        send({'method': 'item/started', 'params': {'item': {
            'id': 'large-tool', 'type': 'commandExecution', 'command': 'large output', 'status': 'inProgress'
        }}})
        send({'method': 'item/completed', 'params': {'item': {
            'id': 'large-tool', 'type': 'commandExecution', 'status': 'completed', 'aggregatedOutput': payload
        }}})
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'answer', 'delta': 'Complete'}})
        send({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
"#,
        );
        request.provider_id = Some("large-thread".into());
        let mut text = String::new();
        let mut output = String::new();
        let mut done = false;
        run(request, Arc::new(AtomicBool::new(false)), |events| {
            for event in events {
                match event {
                    Event::Text(delta) => text.push_str(&delta),
                    Event::Tool(tool)
                        if tool.id == "large-tool" && tool.state == ToolState::Completed =>
                    {
                        output = tool.output;
                    }
                    Event::Done => done = true,
                    _ => {}
                }
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(text, "Complete");
        assert!(output.ends_with("end"));
        assert!(done);
    }

    #[test]
    fn malformed_provider_json_fails() {
        // Exercise JSON parsing after startup, rather than a broken initialization pipe.
        let (_tmp, request) = fixture("#!/bin/sh\nIFS= read -r init\nprintf '{invalid json\\n'\n");
        let error = run(request, Arc::new(AtomicBool::new(false)), |_| Ok(())).unwrap_err();
        assert!(error.starts_with("Invalid provider JSON:"), "{error}");
    }
}
