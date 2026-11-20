//! Lifecycle rules from the desktop adapters. An idle stream is not a failed turn.
use super::*;
static CONTROL_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Protocol {
    pub(super) fn codex_elicitation(
        &mut self,
        wire: Value,
        mut request: Value,
        events: &mut Vec<Event>,
        stdin: &mut impl Write,
    ) -> Result<(), String> {
        let confirmation = mcp_confirmation(&request);
        let Some((title, content)) = confirmation else {
            events.push(Event::Status("This MCP server requested a form or browser sign-in that MonoCode does not support yet. Complete it in the server's own interface.".into()));
            return write_json(
                stdin,
                json!({"id":wire,"result":{"action":"cancel","content":null,"_meta":null}}),
            );
        };
        if !self.request.settings.plan && self.request.settings.access == AccessMode::FullAccess {
            return write_json(
                stdin,
                json!({"id":wire,"result":{"action":"accept","content":content,"_meta":null}}),
            );
        }
        request["nativeContent"] = content;
        let id = wire.to_string();
        self.approvals
            .insert(id.clone(), (wire, "codex-mcp".into(), request));
        events.push(Event::Approval(Approval {
            id,
            title,
            detail: String::new(),
            can_remember: false,
        }));
        Ok(())
    }

    pub(super) fn claude_prompt(&mut self, stdin: &mut impl Write) -> Result<(), String> {
        if self.claude_prompt_sent {
            return Ok(());
        }
        let prompt = if let Some(argument) = &self.request.settings.mono_goal {
            if !self.claude_goal_supported {
                return Err("This Claude CLI did not advertise its built-in /goal command.".into());
            }
            format!("/goal {argument}").trim_end().to_owned()
        } else if self.request.settings.effort.as_deref() == Some("ultrathink") {
            if self.request.prompt.is_empty() {
                "Ultrathink:".into()
            } else {
                format!("Ultrathink:\n{}", self.request.prompt)
            }
        } else {
            self.request.prompt.clone()
        };
        write_json(
            stdin,
            json!({"type":"user","session_id":"","parent_tool_use_id":null,
                "message":{"role":"user","content":self.request.claude_content(&prompt)?}}),
        )?;
        self.claude_prompt_sent = true;
        self.follow_up_ready = true;
        Ok(())
    }

    pub(super) fn lifecycle_tick(
        &mut self,
        started: Instant,
        stdin: &mut impl Write,
    ) -> Result<Vec<Event>, String> {
        let mut events = self.expire_questions(stdin)?;
        if let Some(check) = self.goal_idle_check.as_mut().filter(|_| self.goal_ack) {
            if check.requested && check.since.elapsed() >= Duration::from_secs(5) {
                return Err("Codex did not answer the goal thread status request.".into());
            }
            if !check.requested && check.since.elapsed() >= Duration::from_secs(1) {
                write_json(
                    stdin,
                    json!({"id":format!("goal-idle-{}",check.turn),"method":"thread/read","params":{"threadId":self.request.provider_id}}),
                )?;
                check.requested = true;
                check.since = Instant::now();
            }
        }
        if self.request.kind.is_acp() {
            events.extend(self.acp_tick(stdin)?);
        } else if self.request.kind.is_pi() {
            events.extend(self.pi_tick(stdin)?);
        } else if self.request.kind == Kind::Claude {
            // The desktop adapter proceeds after eight seconds even when older
            // CLIs never acknowledge the initialize control request.
            if !self.claude_prompt_sent && started.elapsed() >= Duration::from_secs(8) {
                self.claude_prompt(stdin)?;
            }
            self.sync_background(&mut events);
            if self.claude_result_seen
                && self.activities.claude_background.is_empty()
                && self.activities.claude_pending.is_empty()
                && self.claude_resume_at.is_none_or(|at| Instant::now() >= at)
            {
                self.done = true;
                events.push(Event::Done);
            }
        }
        Ok(events)
    }

    pub(super) fn confirm_goal_pause(
        &mut self,
        stdin: &mut impl Write,
        replies: &mpsc::Receiver<Result<Vec<u8>, String>>,
        emit: &mut impl FnMut(Vec<Event>) -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.interrupt(stdin)? {
            return Err("Missing active goal thread.".into());
        }
        let binding = self.request.provider_id.clone();
        let thread = binding.as_deref();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut paused = false;
        let mut turn = self.active_turn.clone();
        while !paused || turn.is_some() {
            let timeout = if paused {
                "Goal pause confirmed, but waiting for the turn to finish timed out."
            } else {
                "Waiting for Codex to confirm the goal pause timed out."
            };
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(timeout)?;
            let line = replies.recv_timeout(remaining).map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => timeout,
                mpsc::RecvTimeoutError::Disconnected => {
                    "Codex disconnected before confirming Stop."
                }
            })??;
            let Ok(reply) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            let id = reply["id"].as_str();
            if id == self.interrupt_id.as_deref() {
                if let Some(error) = reply.get("error").filter(|v| !v.is_null()) {
                    return Err(codex_error(error, "Codex rejected the pause request."));
                }
                let goal = &reply["result"]["goal"];
                if goal["threadId"].as_str() != thread || goal["status"] != "paused" {
                    return Err("Codex did not return a paused goal for this thread.".into());
                }
                paused = true;
            } else if id
                .and_then(|id| id.strip_prefix("goal-turn-stop-"))
                .is_some_and(|interrupted| turn.as_deref() == Some(interrupted))
            {
                if let Some(error) = reply.get("error").filter(|v| !v.is_null()) {
                    return Err(codex_error(error, "Codex rejected the turn interrupt."));
                }
                if !reply["result"].is_object() {
                    return Err("Codex did not acknowledge the turn interrupt.".into());
                }
                // The interrupt reply acknowledges the request. The turn can still emit output.
            } else if reply["params"]["threadId"].as_str() == thread {
                let method = reply["method"].as_str().unwrap_or("");
                let incoming_turn = reply["params"]["turnId"]
                    .as_str()
                    .or_else(|| reply["params"]["turn"]["id"].as_str());
                let turn_output = turn.is_some()
                    && incoming_turn == turn.as_deref()
                    && (method.starts_with("item/")
                        || matches!(method, "turn/completed" | "turn/aborted"));
                if method == "turn/started" || turn_output {
                    let mut events = self.frame(reply.clone(), stdin)?;
                    events.retain(|event| !matches!(event, Event::Done));
                    if !events.is_empty() {
                        match emit(events) {
                            Err(error) if error != "Stopped" => return Err(error),
                            _ => {}
                        }
                    }
                }
                if let Some(started) = reply["params"]["turn"]["id"].as_str() {
                    match reply["method"].as_str() {
                        Some("turn/started") if turn.as_deref() != Some(started) => {
                            // A continuation may have started while the pause was in flight.
                            write_json(
                                stdin,
                                json!({"id":format!("goal-turn-stop-{started}"),"method":"turn/interrupt","params":{"threadId":thread,"turnId":started}}),
                            )?;
                            turn = Some(started.to_owned());
                        }
                        Some("turn/completed" | "turn/aborted")
                            if turn.as_deref() == Some(started) =>
                        {
                            turn = None
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// A user Stop uses the provider's cancellation API before transport teardown.
    /// Returns whether there is an active request whose acknowledgement can settle.
    pub(super) fn interrupt(&mut self, stdin: &mut impl Write) -> Result<bool, String> {
        let interrupt_id = format!(
            "native-interrupt-{}",
            CONTROL_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        self.interrupt_id = Some(interrupt_id.clone());
        match self.request.kind {
            Kind::Codex => {
                if let Some(thread) = self
                    .request
                    .provider_id
                    .as_ref()
                    .filter(|_| self.goal_active)
                {
                    write_json(
                        stdin,
                        json!({"id":interrupt_id,"method":"thread/goal/set","params":{"threadId":thread,"origin":"user","status":"paused"}}),
                    )?;
                    if let Some(turn) = &self.active_turn {
                        write_json(
                            stdin,
                            json!({"id":format!("goal-turn-stop-{turn}"),"method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}}),
                        )?;
                    }
                    return Ok(true);
                }
                self.approvals.clear();
                self.questions.clear();
                if let (Some(thread), Some(turn)) = (&self.request.provider_id, &self.active_turn) {
                    write_json(
                        stdin,
                        json!({"id":interrupt_id,"method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}}),
                    )?;
                    return Ok(true);
                }
                Ok(false)
            }
            Kind::Claude => {
                // Deny outstanding permissions and skip questions before interrupt,
                // exactly as cancelClaudeTurn does. Stop includes background tasks.
                for id in self.approvals.keys().cloned().collect::<Vec<_>>() {
                    self.respond(
                        Control {
                            id,
                            decision: Decision::Deny,
                        },
                        stdin,
                    )?;
                }
                for id in self.questions.keys().cloned().collect::<Vec<_>>() {
                    self.respond(
                        Control {
                            id,
                            decision: Decision::Deny,
                        },
                        stdin,
                    )?;
                }
                let tasks: Vec<_> = self.activities.claude_background.keys().cloned().collect();
                for (ix, task) in tasks.iter().enumerate() {
                    write_json(
                        stdin,
                        json!({"type":"control_request","request_id":format!("native-stop-task-{ix}"),"request":{"subtype":"stop_task","task_id":task}}),
                    )?;
                }
                write_json(
                    stdin,
                    json!({"type":"control_request","request_id":interrupt_id,"request":{"subtype":"interrupt"}}),
                )?;
                Ok(self.claude_prompt_sent)
            }
            kind if kind.is_pi() => {
                self.pi_cancel(stdin)?;
                Ok(self.follow_up_ready)
            }
            _ => {
                let active = self.acp_active_session().is_some();
                self.acp_cancel(stdin)?;
                Ok(active)
            }
        }
    }

    pub(super) fn codex_access(&self) -> (Value, &'static str, Value, &'static str) {
        let (policy, sandbox, mut sandbox_policy, reviewer) = self.request.settings.access.codex();
        if (self.request.settings.operator
            || self.request.settings.operator_enabled
            || self.request.settings.orchestrator)
            && sandbox_policy["type"] != "dangerFullAccess"
        {
            sandbox_policy["networkAccess"] = true.into();
        }
        (policy, sandbox, sandbox_policy, reviewer)
    }
}

fn mcp_confirmation(request: &Value) -> Option<(String, Value)> {
    if !matches!(
        request["mode"].as_str(),
        Some("form" | "openai/form" | "openaiForm")
    ) {
        return None;
    }
    let schema = request["requestedSchema"].as_object()?;
    if schema.get("type")?.as_str()? != "object"
        || schema.keys().any(|key| {
            ![
                "type",
                "properties",
                "required",
                "title",
                "description",
                "$schema",
                "additionalProperties",
            ]
            .contains(&key.as_str())
        })
    {
        return None;
    }
    let properties = schema.get("properties")?.as_object()?;
    if properties.len() > 1 {
        return None;
    }
    if let Some(required) = schema.get("required") {
        if required
            .as_array()?
            .iter()
            .any(|key| key.as_str().is_none_or(|key| !properties.contains_key(key)))
        {
            return None;
        }
    }
    let mut content = serde_json::Map::new();
    let mut detail = String::new();
    if let Some((key, field)) = properties.iter().next() {
        let field = field.as_object()?;
        if field.get("type")?.as_str()? != "boolean"
            || field
                .keys()
                .any(|key| !["type", "title", "description", "default"].contains(&key.as_str()))
        {
            return None;
        }
        detail = field
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_owned();
        if let Some(description) = field
            .get("description")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            detail.push_str(" — ");
            detail.push_str(description);
        }
        content.insert(key.clone(), true.into());
    }
    let mut title = format!(
        "{}: {}",
        request["serverName"].as_str().unwrap_or("MCP"),
        request["message"].as_str().unwrap_or("Approve request")
    );
    if !detail.is_empty() {
        title.push_str(" — ");
        title.push_str(&detail);
    }
    Some((title, content.into()))
}

pub(super) fn recoverable_codex_resume(error: &str) -> bool {
    let message = error.to_lowercase();
    message.contains("thread")
        && [
            "not found",
            "unknown thread",
            "no such thread",
            "does not exist",
            "missing thread",
            "thread id",
        ]
        .iter()
        .any(|part| message.contains(part))
}

pub(super) fn claude_session_id() -> String {
    let mut bytes = rand::random::<[u8; 16]>();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = format!("{:032x}", u128::from_be_bytes(bytes));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
