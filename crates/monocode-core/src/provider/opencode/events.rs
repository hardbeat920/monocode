//! OpenCode v1 snapshots and v2 durable events share one native transcript.
use super::*;
use crate::agent::{Question, QuestionItem, QuestionOption};
use crate::tool::tasks::{AgentRun, AgentStep, Panel};
use std::collections::HashSet;

#[derive(Clone)]
enum Outcome {
    Succeeded,
    Interrupted,
    Failed(String),
}
impl Outcome {
    fn result(&self, compact: bool) -> Result<(), String> {
        match self {
            Self::Succeeded => Ok(()),
            Self::Interrupted if compact => Err("OpenCode compaction was interrupted".into()),
            Self::Interrupted => Ok(()),
            Self::Failed(error) => Err(error.clone()),
        }
    }
}
#[derive(Default)]
struct Inbox {
    delivered: HashSet<String>,
    outcomes: HashMap<String, Outcome>,
    order: VecDeque<String>,
    running_compaction: Option<String>,
    reports_delivery: bool,
}
impl Inbox {
    fn settle(&mut self, id: String, outcome: Outcome) {
        if self.outcomes.contains_key(&id) {
            return;
        }
        self.order.push_back(id.clone());
        self.outcomes.insert(id, outcome);
        while self.order.len() > 512 {
            if let Some(id) = self.order.pop_front() {
                self.outcomes.remove(&id);
            }
        }
    }
}
struct CachedInbox {
    key: String,
    session: String,
    account: String,
    cwd: PathBuf,
    inbox: Arc<Mutex<Inbox>>,
}
fn inboxes() -> &'static Mutex<Vec<CachedInbox>> {
    static INBOXES: OnceLock<Mutex<Vec<CachedInbox>>> = OnceLock::new();
    INBOXES.get_or_init(Default::default)
}
fn session_inbox(request: &Request, session: &str) -> Arc<Mutex<Inbox>> {
    let key = json!([
        process::owner(),
        request.settings.account,
        request.cwd,
        session
    ])
    .to_string();
    let mut pool = inboxes().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(index) = pool.iter().position(|entry| entry.key == key) {
        let entry = pool.remove(index);
        let inbox = entry.inbox.clone();
        pool.push(entry);
        return inbox;
    }
    let inbox = Arc::new(Mutex::new(Inbox::default()));
    pool.push(CachedInbox {
        key,
        session: session.into(),
        account: request.settings.account.id.clone(),
        cwd: request.cwd.clone(),
        inbox: inbox.clone(),
    });
    if pool.len() > 128 {
        pool.remove(0);
    }
    inbox
}
pub(super) fn forget_session(session: &str, account: &str, cwd: &Path) {
    inboxes()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|entry| {
            !(entry.session == session && entry.account == account && entry.cwd == cwd)
        });
}

pub(super) struct Pending {
    pub session: String,
    pub wire: String,
    pub form: Option<Value>,
}
pub(super) struct State {
    pub protocol: Protocol,
    pub session: String,
    pub approvals: HashMap<String, Pending>,
    pub questions: HashMap<String, Pending>,
    parts: HashMap<String, Value>,
    roles: HashMap<String, String>,
    parents: HashMap<String, Option<String>>,
    agents: HashMap<String, String>,
    pending_agents: HashMap<String, Vec<Value>>,
    metrics: HashMap<String, TurnMetrics>,
    inbox: Arc<Mutex<Inbox>>,
    pub awaited: Option<String>,
    pub admitted: bool,
    pub terminal: Option<Result<(), String>>,
    early: Option<Result<(), String>>,
    pub compact: bool,
}
impl State {
    pub fn new(request: Request, session: String, compact: bool) -> Self {
        let inbox = session_inbox(&request, &session);
        Self {
            protocol: Protocol::new(request),
            session,
            approvals: HashMap::new(),
            questions: HashMap::new(),
            parts: HashMap::new(),
            roles: HashMap::new(),
            parents: HashMap::new(),
            agents: HashMap::new(),
            pending_agents: HashMap::new(),
            metrics: HashMap::new(),
            inbox,
            awaited: None,
            admitted: false,
            terminal: None,
            early: None,
            compact,
        }
    }
    pub fn admit(&mut self, id: Option<String>) {
        self.awaited = id;
        self.admitted = true;
        let inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(outcome) = self.awaited.as_ref().and_then(|id| inbox.outcomes.get(id)) {
            self.terminal = Some(outcome.result(self.compact));
        } else if !inbox.reports_delivery || self.awaited.is_none() {
            self.terminal = self.early.take();
        }
    }
    fn finish(&mut self, outcome: Outcome, v2: bool, execution: bool) {
        let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
        if v2 && execution {
            for id in std::mem::take(&mut inbox.delivered) {
                inbox.settle(id, outcome.clone());
            }
            inbox.running_compaction = None;
        }
        let result = outcome.result(self.compact);
        if !self.admitted {
            self.early = Some(result);
            return;
        }
        if v2 && inbox.reports_delivery {
            if let Some(id) = &self.awaited {
                self.terminal = inbox
                    .outcomes
                    .get(id)
                    .map(|outcome| outcome.result(self.compact));
                return;
            }
        }
        self.terminal = Some(result);
    }
    fn compact_finish(&mut self, outcome: Outcome, input: Option<&str>) {
        let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
        let result = outcome.result(true);
        let id = input
            .map(str::to_owned)
            .or_else(|| inbox.running_compaction.take());
        if let Some(id) = id {
            inbox.settle(id.clone(), outcome);
            if self.awaited.as_deref() == Some(id.as_str()) {
                self.terminal = Some(result);
            }
        } else if !inbox.reports_delivery || self.awaited.is_none() {
            if self.admitted {
                self.terminal = Some(result);
            } else {
                self.early = Some(result);
            }
        }
    }
    fn descendant(&mut self, client: &Client, id: &str) -> Result<bool, String> {
        let mut seen = HashSet::new();
        let mut at = Some(id.to_owned());
        while let Some(id) = at {
            if id == self.session {
                return Ok(true);
            }
            if !seen.insert(id.clone()) {
                break;
            }
            if !self.parents.contains_key(&id) {
                let value = client.request(
                    "GET",
                    &client.path(&format!("/session/{}", client::encode(&id))),
                    None,
                )?;
                self.parents
                    .insert(id.clone(), value["parentID"].as_str().map(str::to_owned));
            }
            at = self.parents.get(&id).cloned().flatten();
        }
        Ok(false)
    }
    fn known_descendant(&self, id: &str) -> bool {
        let mut seen = HashSet::new();
        let mut at = Some(id);
        while let Some(id) = at {
            if id == self.session || self.agents.contains_key(id) {
                return true;
            }
            if !seen.insert(id) {
                break;
            }
            at = self.parents.get(id).and_then(|parent| parent.as_deref());
        }
        false
    }
    fn agent_owner(&self, session: &str) -> Option<String> {
        let mut seen = HashSet::new();
        let mut at = Some(session);
        while let Some(id) = at {
            if let Some(owner) = self.agents.get(id) {
                return Some(owner.clone());
            }
            if !seen.insert(id) {
                break;
            }
            at = self.parents.get(id).and_then(|parent| parent.as_deref());
        }
        None
    }
    pub fn frame(
        &mut self,
        client: &Client,
        event: Value,
        interactive: bool,
    ) -> Result<Vec<Event>, String> {
        let kind = event["type"].as_str().unwrap_or("");
        let p = event
            .get("properties")
            .or_else(|| event.get("data"))
            .unwrap_or(&Value::Null)
            .clone();
        let mut events = Vec::new();
        if matches!(kind, "session.created" | "session.updated") {
            let info = p.get("info").unwrap_or(&p);
            if let Some(id) = info["id"].as_str().or_else(|| p["sessionID"].as_str()) {
                self.parents
                    .insert(id.into(), info["parentID"].as_str().map(str::to_owned));
            }
            return Ok(events);
        }
        let session = p["sessionID"]
            .as_str()
            .or_else(|| p["form"]["sessionID"].as_str())
            .or_else(|| p["request"]["sessionID"].as_str())
            .or_else(|| p["info"]["sessionID"].as_str())
            .or_else(|| p["part"]["sessionID"].as_str())
            .unwrap_or(&self.session)
            .to_owned();
        let child = session != self.session;
        let interaction = matches!(kind, "permission.asked" | "question.asked" | "form.created");
        if child {
            // The service broadcasts unrelated sessions too. Only blocking
            // interactions need an HTTP ancestry lookup; a foreign transcript
            // or terminal event must never fail or finish this conversation.
            let owned = if interaction {
                self.descendant(client, &session)?
            } else {
                self.known_descendant(&session)
            };
            if !owned {
                return Ok(events);
            }
        }
        if child
            && !interaction
            && (kind.starts_with("session.execution.")
                || matches!(kind, "session.idle" | "session.status" | "session.error"))
        {
            return Ok(events);
        }
        match kind {
            "session.inbox.delivered" if !child => {
                if let Some(id) = p["inboxID"].as_str() {
                    let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
                    inbox.reports_delivery = true;
                    inbox.delivered.insert(id.into());
                }
            }
            "session.compaction.started" if !child => {
                self.protocol
                    .compaction(None, ToolState::Running, &mut events);
                self.inbox
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .running_compaction = p["inputID"].as_str().map(str::to_owned)
            }
            "session.compaction.ended" if !child => {
                self.protocol
                    .compaction(None, ToolState::Completed, &mut events);
                if self.compact {
                    self.compact_finish(Outcome::Succeeded, None);
                }
            }
            "session.compaction.failed" if !child => {
                self.protocol
                    .compaction(None, ToolState::Failed, &mut events);
                if self.compact {
                    self.compact_finish(
                        Outcome::Failed(error_message(&p["error"])),
                        p["inputID"].as_str(),
                    );
                }
            }
            "session.execution.succeeded" if !child => {
                self.finish(Outcome::Succeeded, client.v2, true)
            }
            "session.execution.interrupted" if !child => {
                self.finish(Outcome::Interrupted, client.v2, true)
            }
            "session.execution.failed" if !child => {
                self.finish(Outcome::Failed(error_message(&p["error"])), client.v2, true)
            }
            "session.idle" if client.v2 && !child => self.finish(Outcome::Succeeded, true, false),
            "session.status" if !child => {
                if p["status"]["type"] == "idle" {
                    self.finish(Outcome::Succeeded, client.v2, false);
                } else if let Some(message) = p["status"]["message"].as_str() {
                    events.push(Event::Status(message.into()));
                }
            }
            "session.retry.scheduled" if !child => {
                let attempt = p["attempt"]
                    .as_u64()
                    .map(|n| format!(" (attempt {n})"))
                    .unwrap_or_default();
                events.push(Event::Status(format!(
                    "Retrying{attempt}: {}",
                    error_message(&p["error"])
                )));
            }
            "session.error" if !child => self.terminal = Some(Err(error_message(&p["error"]))),
            "message.updated" => {
                let info = p.get("info").unwrap_or(&p);
                self.message(info, child, &mut events);
            }
            "session.step.started" | "session.step.ended" | "session.step.failed" => {
                let info = json!({"id":p["assistantMessageID"],"role":"assistant","agent":p["agent"],"modelID":p["model"]["id"],"providerID":p["model"]["providerID"],"tokens":p["tokens"]});
                self.message(&info, child, &mut events);
            }
            "message.part.updated" => self.part(&p["part"], &session, child, &mut events),
            "message.part.delta" => {
                if let (Some(id), Some(delta)) = (p["partID"].as_str(), p["delta"].as_str()) {
                    if let Some(mut part) = self.parts.get(id).cloned() {
                        part["text"] =
                            format!("{}{}", part["text"].as_str().unwrap_or(""), delta).into();
                        self.part(&part, &session, child, &mut events);
                    }
                }
            }
            "session.text.started"
            | "session.text.delta"
            | "session.text.ended"
            | "session.reasoning.started"
            | "session.reasoning.delta"
            | "session.reasoning.ended" => {
                if let Some(message) = p["assistantMessageID"].as_str() {
                    let part_kind = if kind.starts_with("session.reasoning.") {
                        "reasoning"
                    } else {
                        "text"
                    };
                    let id = format!(
                        "{message}:{part_kind}:{}",
                        p["ordinal"].as_u64().unwrap_or(0)
                    );
                    let mut part = self.parts.get(&id).cloned().unwrap_or_else(
                        || json!({"id":id,"type":part_kind,"messageID":message,"text":""}),
                    );
                    if kind.ends_with(".delta") {
                        part["text"] = format!(
                            "{}{}",
                            part["text"].as_str().unwrap_or(""),
                            p["delta"].as_str().unwrap_or("")
                        )
                        .into();
                    } else if let Some(text) = p["text"].as_str() {
                        part["text"] = text.into();
                    }
                    self.part(&part, &session, child, &mut events);
                }
            }
            "session.tool.input.started"
            | "session.tool.called"
            | "session.tool.progress"
            | "session.tool.success"
            | "session.tool.failed" => {
                if let Some(id) = p["id"].as_str() {
                    let previous = self.parts.get(id);
                    let name = p["name"]
                        .as_str()
                        .or_else(|| previous.and_then(|part| part["tool"].as_str()))
                        .unwrap_or("tool");
                    let input = p
                        .get("input")
                        .cloned()
                        .or_else(|| previous.map(|part| part["state"]["input"].clone()))
                        .unwrap_or_else(|| json!({}));
                    let status = match kind {
                        "session.tool.input.started" => "pending",
                        "session.tool.success" => "completed",
                        "session.tool.failed" => "error",
                        _ => "running",
                    };
                    let part = json!({"id":id,"callID":id,"type":"tool","tool":name,"messageID":p["assistantMessageID"],"state":{"status":status,"input":input,"metadata":p["metadata"],"output":content_text(&p["content"]),"error":p["error"]}});
                    self.part(&part, &session, child, &mut events);
                }
            }
            "permission.asked" => {
                let permission = p.get("request").unwrap_or(&p);
                let id = permission["id"]
                    .as_str()
                    .or_else(|| permission["requestID"].as_str());
                if let Some(id) = id {
                    if self.approvals.values().any(|pending| pending.wire == id) {
                        return Ok(events);
                    }
                    let action = permission["permission"]
                        .as_str()
                        .or_else(|| permission["action"].as_str())
                        .unwrap_or("tool");
                    let planning = self.protocol.request.settings.plan
                        || self.protocol.request.settings.access == AccessMode::ReadOnly;
                    if planning
                        || self.protocol.request.settings.access == AccessMode::FullAccess
                        || !interactive
                    {
                        let action = action.to_lowercase();
                        let writing = ["bash", "command", "shell", "edit", "write", "patch"]
                            .iter()
                            .any(|kind| action.contains(kind));
                        let reading = ["read", "grep", "glob", "search", "find"]
                            .iter()
                            .any(|kind| action.contains(kind));
                        let allow = if planning {
                            !writing && reading
                        } else {
                            interactive
                                && self.protocol.request.settings.access == AccessMode::FullAccess
                        };
                        client.permission(&session, id, allow)?;
                    } else {
                        let ui = format!("opencode-permission-{id}");
                        self.approvals.insert(
                            ui.clone(),
                            Pending {
                                session: session.clone(),
                                wire: id.into(),
                                form: None,
                            },
                        );
                        events.push(Event::Approval(Approval {
                            id: ui,
                            title: action.into(),
                            detail: permission.to_string(),
                            can_remember: false,
                        }));
                    }
                }
            }
            "question.asked" | "form.created" => {
                let raw = p.get("form").unwrap_or(&p);
                if let Some(id) = raw["id"].as_str().or_else(|| raw["requestID"].as_str()) {
                    if self.questions.values().any(|pending| pending.wire == id) {
                        return Ok(events);
                    }
                    let form = if kind == "form.created" {
                        Some(raw.clone())
                    } else {
                        None
                    };
                    let ui = format!("opencode-question-{id}");
                    let question = question(&ui, raw, form.is_some());
                    self.questions.insert(
                        ui.clone(),
                        Pending {
                            session: session.clone(),
                            wire: id.into(),
                            form,
                        },
                    );
                    if interactive {
                        events.push(Event::Question(question));
                    } else {
                        self.respond(
                            client,
                            Control {
                                id: ui,
                                decision: Decision::Deny,
                            },
                        )?;
                    }
                }
            }
            _ => {}
        }
        Ok(events)
    }
    fn message(&mut self, info: &Value, child: bool, events: &mut Vec<Event>) {
        let id = info["id"].as_str().unwrap_or("");
        let role = info["role"]
            .as_str()
            .or_else(|| info["type"].as_str())
            .unwrap_or("assistant");
        let hidden = matches!(
            info["agent"].as_str(),
            Some("compaction" | "summary" | "title")
        );
        if !id.is_empty() {
            self.roles
                .insert(id.into(), if hidden { "hidden" } else { role }.into());
        }
        if child || hidden || role != "assistant" {
            return;
        }
        if let (Some(provider), Some(model)) =
            (info["providerID"].as_str(), info["modelID"].as_str())
        {
            events.push(Event::Model(format!("{provider}/{model}")));
        }
        if !info["tokens"].is_object() {
            return;
        }
        let tokens = &info["tokens"];
        let metrics = TurnMetrics {
            input_tokens: tokens["input"].as_u64().unwrap_or(0),
            output_tokens: tokens["output"].as_u64().unwrap_or(0)
                + tokens["reasoning"].as_u64().unwrap_or(0),
            cache_read_tokens: tokens["cache"]["read"].as_u64().unwrap_or(0),
            cache_write_tokens: tokens["cache"]["write"].as_u64().unwrap_or(0),
            cache_hit_percent: None,
            ..Default::default()
        };
        if metrics.reported() {
            events.push(Event::Context {
                used: Some(
                    metrics.input_tokens
                        + metrics.output_tokens
                        + metrics.cache_read_tokens
                        + metrics.cache_write_tokens,
                ),
                window: None,
            });
            self.metrics.insert(id.into(), metrics);
            let mut total = TurnMetrics::default();
            for metrics in self.metrics.values() {
                total.input_tokens += metrics.input_tokens;
                total.output_tokens += metrics.output_tokens;
                total.cache_read_tokens += metrics.cache_read_tokens;
                total.cache_write_tokens += metrics.cache_write_tokens;
            }
            let input = total.input_tokens + total.cache_read_tokens + total.cache_write_tokens;
            if input > 0 {
                total.cache_hit_percent =
                    Some(total.cache_read_tokens as f64 / input as f64 * 100.);
            }
            events.push(Event::Metrics(total));
        }
    }
    fn part(&mut self, part: &Value, session: &str, child: bool, events: &mut Vec<Event>) {
        let Some(id) = part["id"].as_str() else {
            return;
        };
        self.parts.insert(id.into(), part.clone());
        let kind = part["type"].as_str().unwrap_or("");
        let hidden = part["messageID"]
            .as_str()
            .and_then(|message| self.roles.get(message))
            .is_some_and(|role| role != "assistant");
        if hidden {
            return;
        }
        if child {
            if let Some(owner) = self.agent_owner(session) {
                let step = if kind == "tool" {
                    let state = &part["state"];
                    let mut call = ToolCall::new(id, part["tool"].as_str().unwrap_or("tool"));
                    if !state["input"].is_null() {
                        call.input = state["input"].to_string();
                    }
                    call.state = match state["status"].as_str() {
                        Some("completed") => ToolState::Completed,
                        Some("error" | "failed") => ToolState::Failed,
                        Some("cancelled" | "canceled" | "aborted") => ToolState::Cancelled,
                        _ => ToolState::Running,
                    };
                    call.output =
                        state["output"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                if call.state == ToolState::Failed {
                                    error_message(&state["error"])
                                } else {
                                    String::new()
                                }
                            });
                    if matches!(call.name.as_str(), "task" | "agent" | "subagent") {
                        call.panel = Some(Panel::Agent(AgentRun::from_input(&state["input"])));
                    }
                    AgentStep::from_tool_call(&call)
                } else if matches!(kind, "text" | "reasoning") {
                    AgentStep {
                        id: id.into(),
                        kind: if kind == "reasoning" {
                            "reasoning"
                        } else {
                            "message"
                        }
                        .into(),
                        text: part["text"].as_str().unwrap_or("").into(),
                        ..AgentStep::default()
                    }
                } else {
                    return;
                };
                if let Some(tool) = self.protocol.tools.get_mut(&owner) {
                    if let Some(Panel::Agent(run)) = tool.panel.as_mut() {
                        run.upsert(step);
                        events.push(Event::Tool(tool.clone()));
                    }
                }
            } else {
                let pending = self.pending_agents.entry(session.into()).or_default();
                if let Some(previous) = pending.iter_mut().find(|row| row["id"] == id) {
                    *previous = part.clone();
                } else if pending.len() < 64 {
                    pending.push(part.clone());
                }
            }
            return;
        }
        if kind == "text" {
            self.protocol
                .text(id, part["text"].as_str().unwrap_or(""), true, events);
        } else if kind == "reasoning" {
            let key = format!("reasoning-{id}");
            let previous = self.protocol.emitted.entry(key).or_default();
            let delta = snapshot_remainder(previous, part["text"].as_str().unwrap_or(""));
            if !delta.is_empty() {
                let delta = delta.to_owned();
                previous.push_str(&delta);
                events.push(Event::Reasoning(delta));
            }
        } else if kind == "tool" {
            let call = part["callID"].as_str().unwrap_or(id);
            let tool =
                self.protocol.tools.entry(call.into()).or_insert_with(|| {
                    ToolCall::new(call, part["tool"].as_str().unwrap_or("tool"))
                });
            let state = &part["state"];
            if !state["input"].is_null() {
                tool.input = state["input"].to_string();
            }
            tool.state = match state["status"].as_str() {
                Some("completed") => ToolState::Completed,
                Some("error") => ToolState::Failed,
                Some("cancelled" | "canceled" | "aborted") => ToolState::Cancelled,
                _ => ToolState::Running,
            };
            tool.output = state["output"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if tool.state == ToolState::Failed {
                        error_message(&state["error"])
                    } else {
                        String::new()
                    }
                });
            tool.bound_output();
            let mut bound_child = None;
            if matches!(tool.name.as_str(), "task" | "agent" | "subagent") {
                let child = [&state["metadata"], state, &state["input"]]
                    .into_iter()
                    .find_map(|source| {
                        [
                            "sessionID",
                            "sessionId",
                            "session_id",
                            "childSessionID",
                            "subSessionID",
                        ]
                        .into_iter()
                        .find_map(|key| source[key].as_str())
                    });
                if let Some(child) = child {
                    self.agents.insert(child.into(), call.into());
                    if tool.panel.is_none() {
                        let mut run = AgentRun::from_input(&state["input"]);
                        run.ids.push(child.into());
                        tool.panel = Some(Panel::Agent(run));
                    }
                    bound_child = Some(child.to_owned());
                }
            }
            events.push(Event::Tool(tool.clone()));
            if let Some(child) = bound_child {
                for pending in self.pending_agents.remove(&child).unwrap_or_default() {
                    self.part(&pending, &child, true, events);
                }
            }
        }
    }
    pub fn respond(&mut self, client: &Client, control: Control) -> Result<Vec<Event>, String> {
        if matches!(control.decision, Decision::KeepOpen) {
            return Ok(Vec::new());
        }
        if let Some(pending) = self.approvals.remove(&control.id) {
            client.permission(
                &pending.session,
                &pending.wire,
                matches!(control.decision, Decision::Allow | Decision::Session),
            )?;
            return Ok(Vec::new());
        }
        if let Some(pending) = self.questions.remove(&control.id) {
            let answers = if let Decision::Answer(answers) = control.decision {
                Some(answers)
            } else {
                None
            };
            let base = if client.v2 {
                client.path(&format!(
                    "/session/{}/form/{}",
                    client::encode(&pending.session),
                    client::encode(&pending.wire)
                ))
            } else {
                format!("/question/{}", client::encode(&pending.wire))
            };
            if let Some(answers) = answers {
                let body = if let Some(form) = pending.form {
                    let mut answer = serde_json::Map::new();
                    for (index, field) in
                        form["fields"].as_array().into_iter().flatten().enumerate()
                    {
                        let key = field["key"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("field_{index}"));
                        let values: Vec<_> = answers
                            .get(index)
                            .into_iter()
                            .flatten()
                            .map(|value| {
                                field["options"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .find(|option| option["label"] == *value)
                                    .and_then(|option| option["value"].as_str())
                                    .unwrap_or(value)
                                    .to_owned()
                            })
                            .collect();
                        let value = match field["type"].as_str() {
                            Some("multiselect") => json!(values),
                            Some("boolean") => json!(values.first().is_some_and(|v| matches!(
                                v.trim().to_lowercase().as_str(),
                                "yes" | "true"
                            ))),
                            Some("number" | "integer") => {
                                let text = values.first().map(String::as_str).unwrap_or("").trim();
                                if text.is_empty() {
                                    continue;
                                }
                                text.parse::<f64>()
                                    .ok()
                                    .filter(|n| n.is_finite())
                                    .map(|n| json!(n))
                                    .unwrap_or_else(|| json!(text))
                            }
                            _ if values.len() <= 1 => {
                                json!(values.first().cloned().unwrap_or_default())
                            }
                            _ => json!(values),
                        };
                        answer.insert(key, value);
                    }
                    json!({"answer":answer})
                } else {
                    json!({"answers":answers})
                };
                client.request("POST", &format!("{base}/reply"), Some(body))?;
            } else if client.v2 {
                client.request("DELETE", &base, None)?;
            } else {
                client.request("POST", &format!("{base}/reject"), Some(json!({})))?;
            }
            return Ok(vec![Event::QuestionResolved(control.id)]);
        }
        Ok(Vec::new())
    }
    pub fn cancel(&mut self, client: &Client) {
        for (_, pending) in self.approvals.drain() {
            let _ = client.permission(&pending.session, &pending.wire, false);
        }
        let questions: Vec<_> = self.questions.keys().cloned().collect();
        for id in questions {
            let _ = self.respond(
                client,
                Control {
                    id,
                    decision: Decision::Deny,
                },
            );
        }
        client.abort(&self.session);
    }
}
fn question(ui: &str, raw: &Value, form: bool) -> Question {
    let source = if form {
        &raw["fields"]
    } else {
        &raw["questions"]
    };
    let items = source
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, row)| {
            let id = row["id"]
                .as_str()
                .or_else(|| row["key"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("field_{index}"));
            let mut options: Vec<_> = row["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let label = option
                        .as_str()
                        .or_else(|| option["label"].as_str())
                        .or_else(|| option["value"].as_str())?;
                    Some(QuestionOption {
                        id: label.into(),
                        label: label.into(),
                        description: option["description"].as_str().unwrap_or("").into(),
                    })
                })
                .collect();
            if row["type"] == "boolean" && options.is_empty() {
                options = ["Yes", "No"]
                    .map(|value| QuestionOption {
                        id: value.into(),
                        label: value.into(),
                        description: String::new(),
                    })
                    .to_vec();
            }
            let allow_custom = options.is_empty() || row["custom"].as_bool().unwrap_or(!form);
            QuestionItem {
                id: id.clone(),
                prompt: row["question"]
                    .as_str()
                    .or_else(|| row["prompt"].as_str())
                    .or_else(|| row["title"].as_str())
                    .or_else(|| row["header"].as_str())
                    .unwrap_or(&id)
                    .into(),
                multiple: row["multiple"] == true || row["type"] == "multiselect",
                allow_custom,
                options,
            }
        })
        .collect();
    Question {
        id: ui.into(),
        title: "OpenCode has a question".into(),
        items,
        auto_resolve_at: None,
    }
}
fn content_text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| {
                if block["type"] == "text" {
                    block["text"].as_str()
                } else {
                    None
                }
            })
            .collect()
    })
}
pub(super) fn error_message(value: &Value) -> String {
    value
        .as_str()
        .or_else(|| value["message"].as_str())
        .or_else(|| value["data"]["message"].as_str())
        .or_else(|| value["error"]["message"].as_str())
        .unwrap_or("OpenCode session failed.")
        .into()
}
