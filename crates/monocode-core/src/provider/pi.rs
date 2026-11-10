//! Pi/omp's NDJSON RPC. Kept separate from JSON-RPC: acknowledgements are not
//! turn completion, and retries/compaction can outlive an individual agent_end.
use super::*;

#[derive(Default)]
pub(super) struct State {
    message: usize,
    window: Option<u64>,
    retrying: bool,
    compacting: bool,
    finishing: bool,
    error: Option<String>,
    deadlines: HashMap<&'static str, Instant>,
    cancelling: bool,
    generation: u64,
    pending: HashMap<String, &'static str>,
    prompt_id: String,
    catalog_started: Option<Instant>,
    catalog_changed: Option<Instant>,
    catalog_poll: Option<Instant>,
    catalog_signature: String,
    catalog_models: HashMap<String, Value>,
}

pub(super) fn configure(command: &mut Command, request: &Request, probe: bool) {
    command.args(["--mode", "rpc"]);
    if probe || request.settings.text_only {
        command.arg("--no-session");
    }
    if request.settings.text_only {
        command.args(["--no-extensions", "--no-tools", "--no-skills"]);
        command.arg(if request.kind == Kind::Omp {
            "--no-rules"
        } else {
            "--no-context-files"
        });
    } else if request.settings.plan || request.settings.access == AccessMode::ReadOnly {
        command.args([
            "--tools",
            if request.kind == Kind::Omp {
                "read,grep,glob,lsp"
            } else {
                "read,grep,find,ls"
            },
        ]);
    }
    if let Some(id) = &request.provider_id {
        command.args([
            if request.kind == Kind::Omp {
                "--resume"
            } else {
                "--session"
            },
            id,
        ]);
    }
    if let Some(model) = request.model.as_ref().filter(|model| {
        model
            .split_once('/')
            .is_some_and(|(provider, id)| !provider.is_empty() && !id.is_empty())
    }) {
        command.args(["--model", model]);
    }
}
impl Protocol {
    fn pi_send(
        &mut self,
        id: &'static str,
        mut request: Value,
        seconds: u64,
        stdin: &mut impl Write,
    ) -> Result<(), String> {
        let wire = if self.pi.generation <= 1 {
            id.into()
        } else {
            format!("{id}:{}", self.pi.generation)
        };
        request["id"] = wire.clone().into();
        self.pi.pending.retain(|_, pending| *pending != id);
        self.pi.pending.insert(wire.clone(), id);
        if id == "native-prompt" {
            self.pi.prompt_id = wire;
        }
        self.pi
            .deadlines
            .insert(id, Instant::now() + Duration::from_secs(seconds));
        write_json(stdin, request)
    }
    pub(super) fn pi_cancel(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        self.pi.cancelling = true;
        self.pi.deadlines.clear();
        self.pi.pending.clear();
        for id in self
            .approvals
            .keys()
            .chain(self.questions.keys())
            .cloned()
            .collect::<Vec<_>>()
        {
            self.respond(
                Control {
                    id,
                    decision: Decision::Deny,
                },
                stdin,
            )?;
        }
        self.pi_send("native-abort", json!({"type":"abort"}), 5, stdin)
    }
    pub(super) fn pi_tick(&mut self, stdin: &mut impl Write) -> Result<Vec<Event>, String> {
        if self.probe && self.request.kind == Kind::Pi {
            if self
                .pi
                .catalog_started
                .is_some_and(|started| started.elapsed() >= Duration::from_secs(45))
            {
                if self.pi.catalog_models.is_empty() {
                    return Err("Pi model discovery timed out".into());
                }
                self.pi_catalog_finish();
                return Ok(Vec::new());
            }
            if self
                .pi
                .catalog_poll
                .is_some_and(|poll| Instant::now() >= poll)
            {
                self.pi.catalog_poll = None;
                self.pi_send(
                    "native-models",
                    json!({"type":"get_available_models"}),
                    45,
                    stdin,
                )?;
            }
        }
        let expired: Vec<_> = self
            .pi
            .deadlines
            .iter()
            .filter(|(_, deadline)| Instant::now() >= **deadline)
            .map(|(id, _)| *id)
            .collect();
        let mut events = Vec::new();
        for id in expired {
            self.pi.deadlines.remove(id);
            self.pi.pending.retain(|_, pending| *pending != id);
            if self.done {
                break;
            }
            events.extend(self.pi_response_failure(id, "RPC request timed out", stdin)?);
        }
        Ok(events)
    }
    fn pi_finish(&mut self) -> Result<Vec<Event>, String> {
        self.pi.deadlines.clear();
        self.pi.pending.clear();
        if !self.pi.cancelling {
            if let Some(error) = self.pi.error.take() {
                return Err(error);
            }
        }
        self.done = true;
        Ok(vec![Event::Done])
    }
    fn pi_response_failure(
        &mut self,
        id: &str,
        error: &str,
        stdin: &mut impl Write,
    ) -> Result<Vec<Event>, String> {
        match id {
            "native-commands" if !self.command_probe => Ok(Vec::new()),
            "native-thinking" => {
                self.pi_fast(stdin)?;
                Ok(Vec::new())
            }
            "native-fast" => {
                self.pi_prompt(stdin)?;
                Ok(if self.request.settings.fast {
                    vec![Event::Status(error.into())]
                } else {
                    Vec::new()
                })
            }
            // Stats improve the context meter, but cannot fail a completed turn.
            "native-stats" | "native-abort" => self.pi_finish(),
            _ => Err(format!("{}: {error}", self.request.kind.label())),
        }
    }
    pub(super) fn pi_start(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        let generation = self.pi.generation + 1;
        self.pi = State {
            generation,
            ..State::default()
        };
        if self.command_probe {
            self.pi_send(
                "native-commands", json!({"type":if self.request.kind == Kind::Omp { "get_available_commands" } else { "get_commands" }}), 15, stdin,
            )
        } else if self.probe {
            self.pi.catalog_started = Some(Instant::now());
            self.pi.catalog_changed = self.pi.catalog_started;
            self.pi_send(
                "native-models",
                json!({"type":"get_available_models"}),
                45,
                stdin,
            )
        } else {
            self.pi_send("native-state", json!({"type":"get_state"}), 45, stdin)
        }
    }
    fn pi_catalog_finish(&mut self) {
        self.catalog_result =
            Some(json!({"models":self.pi.catalog_models.values().collect::<Vec<_>>()}));
        self.pi.deadlines.clear();
        self.pi.pending.clear();
        self.done = true;
    }
    fn pi_prompt(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        if self.compact {
            return self.pi_send("native-compact", json!({"type":"compact"}), 30 * 60, stdin);
        }
        self.pi_send(
            "native-prompt", json!({"type":"prompt","message":self.request.prompt,"images":self.request.pi_images()?}),
            if self.request.kind == Kind::Omp {30 * 60} else {15}, stdin,
        )
    }
    fn pi_fast(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        if self.request.kind == Kind::Omp {
            self.pi_send(
                "native-fast",
                json!({"type":"set_fast_mode","enabled":self.request.settings.fast}),
                15,
                stdin,
            )
        } else {
            self.pi_prompt(stdin)
        }
    }
    fn pi_thinking(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        if let Some(level) = self
            .request
            .settings
            .model_settings
            .get("thinking")
            .or(self.request.settings.effort.as_ref())
            .filter(|level| {
                matches!(
                    level.as_str(),
                    "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                )
            })
        {
            self.pi_send(
                "native-thinking",
                json!({"type":"set_thinking_level","level":level}),
                15,
                stdin,
            )
        } else {
            self.pi_fast(stdin)
        }
    }
    fn pi_settle(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        self.pi.finishing = true;
        // A provider may finish before its prompt acknowledgement arrives.
        self.pi.deadlines.remove("native-prompt");
        self.pi_send(
            "native-stats",
            json!({"type":"get_session_stats"}),
            4,
            stdin,
        )
    }
    pub(super) fn pi_frame(
        &mut self,
        v: Value,
        stdin: &mut impl Write,
    ) -> Result<Vec<Event>, String> {
        let mut events = Vec::new();
        let kind = v["type"].as_str().unwrap_or("");
        if kind == "response" {
            let wire = v["id"].as_str().unwrap_or("");
            if self.request.kind == Kind::Omp
                && v["command"] == "prompt"
                && wire == self.pi.prompt_id
                && v["success"] == false
            {
                return Err(v["error"].as_str().unwrap_or("OMP command failed").into());
            }
            let id = match self.pi.pending.remove(wire) {
                Some(id) => id,
                None if self.pi.generation == 0 => wire,
                None => return Ok(events),
            };
            // A timed-out optional control can answer after the prompt began.
            // Ignore that stale response rather than submit the prompt twice.
            let tracked = self.pi.deadlines.remove(id).is_some();
            if !tracked && matches!(id, "native-thinking" | "native-fast") {
                return Ok(events);
            }
            if v["success"] == false {
                return self.pi_response_failure(
                    id,
                    v["error"].as_str().unwrap_or("RPC request failed"),
                    stdin,
                );
            }
            match id {
                "native-models" => {
                    if self.request.kind == Kind::Omp {
                        self.catalog_result = Some(v["data"].clone());
                        self.done = true;
                    } else {
                        let data = v["data"].get("models").unwrap_or(&v["data"]);
                        let signature = data.to_string();
                        if signature != self.pi.catalog_signature {
                            self.pi.catalog_signature = signature;
                            self.pi.catalog_changed = Some(Instant::now());
                        }
                        for model in data.as_array().into_iter().flatten() {
                            if let (Some(provider), Some(id)) =
                                (model["provider"].as_str(), model["id"].as_str())
                            {
                                self.pi
                                    .catalog_models
                                    .insert(format!("{provider}/{id}"), model.clone());
                            }
                        }
                        if self
                            .pi
                            .catalog_started
                            .is_some_and(|started| started.elapsed() >= Duration::from_secs(5))
                            && self
                                .pi
                                .catalog_changed
                                .is_some_and(|changed| changed.elapsed() >= Duration::from_secs(1))
                        {
                            self.pi_catalog_finish();
                        } else {
                            self.pi.catalog_poll =
                                Some(Instant::now() + Duration::from_millis(500));
                        }
                    }
                }
                "native-commands" => {
                    if self.command_probe {
                        if !v["data"]["commands"].is_array() {
                            return Err(
                                "Provider command catalog returned no commands array".into()
                            );
                        }
                        self.done = true;
                    }
                    events.push(Event::Commands(super::commands::parse_commands(
                        self.request.kind,
                        &v["data"]["commands"],
                    )));
                }
                "native-compact" => {
                    events.push(Event::Context {
                        used: v["data"]["estimatedTokensAfter"].as_u64(),
                        window: self.pi.window,
                    });
                    events.push(Event::Done);
                    self.done = true;
                }
                "native-state" => {
                    if !self.compact && !self.request.settings.text_only {
                        self.pi_send(
                            "native-commands", json!({"type":if self.request.kind == Kind::Omp {"get_available_commands"} else {"get_commands"}}), 15, stdin,
                        )?;
                    }
                    if let Some(id) = v["data"]["sessionId"]
                        .as_str()
                        .or_else(|| v["data"]["session_id"].as_str())
                    {
                        events.push(Event::SessionId(id.into()));
                    }
                    let model = &v["data"]["model"];
                    self.pi.window = model["contextWindow"].as_u64();
                    if let (Some(provider), Some(id)) =
                        (model["provider"].as_str(), model["id"].as_str())
                    {
                        events.push(Event::Model(format!("{provider}/{id}")));
                    }
                    let selected = self
                        .request
                        .model
                        .as_deref()
                        .and_then(|model| model.split_once('/'));
                    if self.request.kind == Kind::Pi
                        && selected.is_some_and(|(provider, id)| {
                            !provider.is_empty()
                                && !id.is_empty()
                                && (model["provider"].as_str() != Some(provider)
                                    || model["id"].as_str() != Some(id))
                        })
                    {
                        let (provider, id) = selected.unwrap();
                        self.pi_send(
                            "native-model",
                            json!({"type":"set_model","provider":provider,"modelId":id}),
                            15,
                            stdin,
                        )?;
                    } else {
                        self.pi_thinking(stdin)?;
                    }
                }
                "native-model" => {
                    self.pi.window = v["data"]["contextWindow"].as_u64().or(self.pi.window);
                    if let Some(model) = &self.request.model {
                        events.push(Event::Model(model.clone()));
                    }
                    self.pi_thinking(stdin)?;
                }
                "native-thinking" => self.pi_fast(stdin)?,
                "native-fast" => self.pi_prompt(stdin)?,
                "native-stats" => {
                    let usage = &v["data"]["contextUsage"];
                    events.push(Event::Context {
                        used: usage["tokens"].as_u64(),
                        window: usage["contextWindow"].as_u64().or(self.pi.window),
                    });
                    events.extend(self.pi_finish()?);
                }
                "native-abort" => events.extend(self.pi_finish()?),
                "native-prompt"
                    if self.request.kind == Kind::Omp && v["data"]["agentInvoked"] == false =>
                {
                    events.extend(self.pi_finish()?)
                }
                _ => {}
            }
            return Ok(events);
        }
        if self.probe {
            return Ok(events);
        }
        match kind {
            "message_start" | "message_end"
                if self.request.kind == Kind::Omp && v["message"]["role"] == "custom" =>
            {
                if kind == "message_start" && v["message"]["display"] == true {
                    let message = &v["message"];
                    let custom_type = message["customType"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("custom");
                    let mut text = custom_message_text(&message["content"]);
                    let mut severity = None;
                    if custom_type == "advisor" {
                        let mut notes = Vec::new();
                        for note in message["details"]["notes"].as_array().into_iter().flatten() {
                            let Some(body) =
                                note["note"].as_str().filter(|body| !body.trim().is_empty())
                            else {
                                continue;
                            };
                            notes.push(body);
                            match note["severity"].as_str() {
                                Some("blocker") => severity = Some("blocker".into()),
                                Some("concern") if severity.as_deref() != Some("blocker") => {
                                    severity = Some("concern".into())
                                }
                                Some("nit") if severity.is_none() => severity = Some("nit".into()),
                                _ => {}
                            }
                        }
                        if !notes.is_empty() {
                            text = notes.join("\n\n");
                        }
                    }
                    events.push(Event::Interjection {
                        text,
                        custom_type: custom_type.into(),
                        severity,
                    });
                    self.text_key = None;
                }
            }
            "message_start" if v["message"]["role"] == "assistant" => self.pi.message += 1,
            "message_update" => {
                let event = &v["assistantMessageEvent"];
                let delta = event["delta"]
                    .as_str()
                    .or_else(|| event["delta"]["text"].as_str())
                    .unwrap_or("");
                match event["type"].as_str() {
                    Some("text_delta") => self.text(
                        &format!("pi-{}", self.pi.message),
                        delta,
                        false,
                        &mut events,
                    ),
                    Some("thinking_delta") => events.push(Event::Reasoning(delta.into())),
                    _ => {}
                }
            }
            "message_end" if v["message"]["role"] == "assistant" => {
                if v["message"]["stopReason"] == "error" {
                    self.pi.error = Some(
                        v["message"]["errorMessage"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("Provider turn failed")
                            .into(),
                    );
                }
                let text = v["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| c["type"] == "text")
                    .filter_map(|c| c["text"].as_str())
                    .collect::<String>();
                self.text(&format!("pi-{}", self.pi.message), &text, true, &mut events);
                let usage = &v["message"]["usage"];
                let input = usage["input"].as_u64().unwrap_or(0);
                let output = usage["output"].as_u64().unwrap_or(0);
                let read = usage["cacheRead"].as_u64().unwrap_or(0);
                let write = usage["cacheWrite"].as_u64().unwrap_or(0);
                self.metrics.input_tokens += input;
                self.metrics.output_tokens += output;
                self.metrics.cache_read_tokens += read;
                self.metrics.cache_write_tokens += write;
                let total = self.metrics.input_tokens
                    + self.metrics.cache_read_tokens
                    + self.metrics.cache_write_tokens;
                if total > 0
                    && (usage.get("cacheRead").is_some() || usage.get("cacheWrite").is_some())
                {
                    self.metrics.cache_hit_percent =
                        Some(self.metrics.cache_read_tokens as f64 / total as f64 * 100.);
                }
                if self.metrics.reported() {
                    events.push(Event::Metrics(self.metrics));
                }
                events.push(Event::Context {
                    used: Some(input + output + read + write),
                    window: self.pi.window,
                });
            }
            "tool_execution_start" | "tool_execution_update" | "tool_execution_end" => {
                let Some(id) = v["toolCallId"].as_str() else {
                    return Ok(events);
                };
                let tool = self
                    .tools
                    .entry(id.into())
                    .or_insert_with(|| ToolCall::new(id, v["toolName"].as_str().unwrap_or("Tool")));
                // OMP can emit an update after a terminal tool event.
                if kind == "tool_execution_update" && tool.state != ToolState::Running {
                    return Ok(events);
                }
                if let Some(input) = v.get("args").or_else(|| v.get("arguments")) {
                    let input = input
                        .as_str()
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .unwrap_or_else(|| input.clone());
                    let mut previous: Value =
                        serde_json::from_str(&tool.input).unwrap_or_else(|_| json!({}));
                    if let (Some(previous), Some(next)) =
                        (previous.as_object_mut(), input.as_object())
                    {
                        previous.extend(next.clone());
                    }
                    tool.input = serde_json::to_string_pretty(&previous).unwrap_or_default();
                }
                let output = acp::content_text(
                    &v[if kind == "tool_execution_end" {
                        "result"
                    } else {
                        "partialResult"
                    }]["content"],
                );
                if !output.is_empty() {
                    tool.output = output;
                }
                tool.bound_output();
                if kind == "tool_execution_end" {
                    tool.state = if v["isCancelled"] == true || v["isCanceled"] == true {
                        ToolState::Cancelled
                    } else if v["isError"] == true {
                        ToolState::Failed
                    } else {
                        ToolState::Completed
                    };
                }
                events.push(Event::Tool(tool.clone()));
            }
            "extension_ui_request" => {
                let id = v["id"].to_string();
                let method = v["method"].as_str().unwrap_or("");
                if self.request.kind == Kind::Omp && method == "cancel" {
                    if self.questions.contains_key(&id) {
                        self.respond(
                            Control {
                                id: id.clone(),
                                decision: Decision::Deny,
                            },
                            stdin,
                        )?;
                        events.push(Event::QuestionResolved(id));
                    }
                } else if method == "confirm" {
                    self.approvals
                        .insert(id.clone(), (v["id"].clone(), "pi".into(), v.clone()));
                    if self.pi.cancelling {
                        self.respond(
                            Control {
                                id,
                                decision: Decision::Deny,
                            },
                            stdin,
                        )?;
                        return Ok(events);
                    }
                    events.push(Event::Approval(Approval {
                        id,
                        title: v["title"]
                            .as_str()
                            .unwrap_or("Provider confirmation")
                            .into(),
                        detail: v["message"].as_str().unwrap_or("").into(),
                        can_remember: false,
                    }));
                } else if matches!(method, "select" | "input" | "editor") {
                    self.question(v["id"].clone(), "pi", v.clone(), &mut events, stdin)?;
                    if self.pi.cancelling {
                        self.respond(
                            Control {
                                id,
                                decision: Decision::Deny,
                            },
                            stdin,
                        )?;
                        events.clear();
                    }
                } else if let Some(text) = v["message"].as_str().or_else(|| v["title"].as_str()) {
                    events.push(Event::Status(text.into()));
                }
            }
            "auto_retry_start" => {
                self.pi.retrying = true;
                self.pi.error = None;
                events.push(Event::Status("Retrying…".into()));
            }
            "auto_retry_end" => self.pi.retrying = false,
            "compaction_start" => {
                self.pi.compacting = true;
                self.compaction(None, ToolState::Running, &mut events);
            }
            "compaction_end" => {
                self.pi.compacting = false;
                let state = if v["aborted"] == true {
                    ToolState::Cancelled
                } else if v["error"].is_string() || v["isError"] == true {
                    ToolState::Failed
                } else {
                    ToolState::Completed
                };
                self.compaction(None, state, &mut events);
            }
            "agent_end" if v["willRetry"] == true => self.pi.error = None,
            "agent_settled" if !self.pi.finishing => self.pi_settle(stdin)?,
            "agent_end"
                if !self.pi.finishing
                    && !self.pi.retrying
                    && !self.pi.compacting
                    && v["isTerminal"] != false =>
            {
                self.pi_settle(stdin)?;
            }
            "prompt_result"
                if self.request.kind == Kind::Omp
                    && v["agentInvoked"] == false
                    && v["id"].as_str() == Some(self.pi.prompt_id.as_str()) =>
            {
                events.extend(self.pi_finish()?)
            }
            "prompt_result" if v["isError"] == true => {
                return Err(v["error"]
                    .as_str()
                    .unwrap_or("Provider prompt failed")
                    .into());
            }
            "extension_error" => events.push(Event::Status(
                v["error"].as_str().unwrap_or("Extension error").into(),
            )),
            "session_info_update" if self.request.kind == Kind::Omp => {
                if let Some(id) = v["sessionId"].as_str().filter(|id| !id.is_empty()) {
                    events.push(Event::SessionId(id.into()));
                }
            }
            "config_update" if self.request.kind == Kind::Omp => {
                let mut model = None;
                if let (Some(provider), Some(id)) =
                    (v["model"]["provider"].as_str(), v["model"]["id"].as_str())
                {
                    model = Some(format!("{provider}/{id}"));
                }
                self.pi.window = v["model"]["contextWindow"].as_u64().or(self.pi.window);
                let mut model_settings = HashMap::new();
                if let Some(thinking) = v["thinkingLevel"].as_str().filter(|level| {
                    matches!(
                        *level,
                        "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                    )
                }) {
                    model_settings.insert("thinking".into(), thinking.into());
                }
                if let Some(fast) = v["fastModeEnabled"].as_bool() {
                    model_settings.insert("fast".into(), fast.to_string());
                }
                events.push(Event::ConfigChanged {
                    model,
                    model_settings,
                });
            }
            "available_commands_update" if self.request.kind == Kind::Omp => {
                if v["commands"].is_array() {
                    events.push(Event::Commands(super::commands::parse_commands(
                        self.request.kind,
                        &v["commands"],
                    )));
                }
            }
            "command_output" if self.request.kind == Kind::Omp => {
                if let Some(text) = v["text"].as_str().filter(|text| !text.is_empty()) {
                    events.push(Event::Status(text.into()));
                }
            }
            "advisor_yielded" if self.request.kind == Kind::Omp => {
                events.push(Event::Status("Advisor reviewed this turn".into()))
            }
            _ => {}
        }
        Ok(events)
    }
}

fn custom_message_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.into();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["type"] == "text")
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
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
    fn prompt_acknowledgements_do_not_finish_a_turn_and_retries_are_kept_alive() {
        for kind in [Kind::Pi, Kind::Omp] {
            let mut p = protocol(kind);
            let mut out = Vec::new();
            p.pi_start(&mut out).unwrap();
            assert!(p.frame(json!({"type":"response","id":"native-state","success":true,"data":{"sessionId":"saved","model":{"provider":"openai","id":"model","contextWindow":200000}}}),&mut out).unwrap().contains(&Event::SessionId("saved".into())));
            p.frame(
                json!({"type":"response","id":"native-prompt","success":true}),
                &mut out,
            )
            .unwrap();
            assert!(!p.done);
            p.frame(json!({"type":"auto_retry_start"}), &mut out)
                .unwrap();
            p.frame(json!({"type":"agent_end","willRetry":true}), &mut out)
                .unwrap();
            assert!(!p.pi.finishing);
            p.frame(json!({"type":"auto_retry_end"}), &mut out).unwrap();
            p.frame(json!({"type":"agent_end"}), &mut out).unwrap();
            assert!(p.pi.finishing);
            assert!(!p.done);
            let events = p.frame(json!({"type":"response","id":"native-stats","success":true,"data":{"contextUsage":{"tokens":50,"contextWindow":200000}}}),&mut out).unwrap();
            assert!(events.contains(&Event::Done));
            assert!(events.contains(&Event::Context {
                used: Some(50),
                window: Some(200000)
            }));
        }
    }
    #[test]
    fn streamed_text_and_completed_snapshots_do_not_duplicate_or_count_usage_twice() {
        let mut p = protocol(Kind::Pi);
        let mut out = Vec::new();
        p.frame(
            json!({"type":"message_start","message":{"role":"assistant"}}),
            &mut out,
        )
        .unwrap();
        p.frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Hello"}}),&mut out).unwrap();
        let events = p.frame(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Hello!"}],"usage":{"input":100,"output":20,"cacheRead":900}}}),&mut out).unwrap();
        assert!(events.contains(&Event::Text("!".into())));
        assert_eq!(p.metrics.input_tokens, 100);
        p.frame(json!({"type":"turn_end","message":{"usage":{"input":100,"output":20,"cacheRead":900}}}),&mut out).unwrap();
        assert_eq!(p.metrics.input_tokens, 100);
    }
    #[test]
    fn tool_events_merge_input_and_keep_terminal_errors() {
        let mut p = protocol(Kind::Omp);
        let mut out = Vec::new();
        p.frame(json!({"type":"tool_execution_start","toolCallId":"t","toolName":"read","args":{"path":"file"}}),&mut out).unwrap();
        p.frame(json!({"type":"tool_execution_update","toolCallId":"t","args":{"offset":2},"partialResult":{"content":[{"text":"partial"}]}}),&mut out).unwrap();
        p.frame(json!({"type":"tool_execution_end","toolCallId":"t","isError":true,"result":{"content":[{"text":"failed"}]}}),&mut out).unwrap();
        let tool = &p.tools["t"];
        assert!(tool.input.contains("file") && tool.input.contains("offset"));
        assert_eq!(tool.output, "failed");
        assert_eq!(tool.state, ToolState::Failed);
        p.frame(json!({"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"Unauthorized"}}),&mut out).unwrap();
        assert!(p
            .frame(
                json!({"type":"response","id":"native-stats","success":true,"data":{}}),
                &mut out
            )
            .unwrap_err()
            .contains("Unauthorized"));
    }
}
