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
        let prompt = if self.request.settings.effort.as_deref() == Some("ultrathink") {
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
