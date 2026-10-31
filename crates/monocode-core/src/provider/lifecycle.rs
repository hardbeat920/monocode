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
        if self.grants_apply() && self.request.settings.access == AccessMode::FullAccess {
            return write_json(
                stdin,
                json!({"id":wire,"result":{"action":"accept","content":content,"_meta":null}}),
            );
        }
        let subjects = self.mcp_subject(&request).into_iter().collect();
        request["nativeContent"] = content;
        let id = wire.to_string();
        self.approvals
            .insert(id.clone(), (wire, "codex-mcp".into(), request));
        if self.offer(&id, subjects, &title) {
            return self.respond(
                Control {
                    id,
                    decision: Decision::Allow,
                },
                stdin,
            );
        }
        events.push(Event::Approval(Approval {
            remember_scope: self.remember_scope(&id),
            id,
            title,
            detail: String::new(),
            can_remember: false,
        }));
        Ok(())
    }

    /// A Codex MCP confirmation without the fields that change on every ask.
    pub(super) fn mcp_subject(&self, request: &Value) -> Option<crate::permissions::Subject> {
        mcp_confirmation(request)?;
        let server = request["serverName"]
            .as_str()
            .filter(|s| !s.trim().is_empty())?;
        request["message"]
            .as_str()
            .filter(|s| !s.trim().is_empty())?;
        let mut confirmation = request.as_object()?.clone();
        for field in ["threadId", "turnId", "serverName", "nativeContent"] {
            confirmation.remove(field);
        }
        let mut confirmation = Value::Object(confirmation);
        crate::permissions::stable(&mut confirmation);
        Some(crate::permissions::Subject::McpConfirmation {
            server: server.into(),
            confirmation,
        })
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

#[cfg(test)]
mod permission_tests {
    use super::*;

    fn protocol(path: &Path) -> Protocol {
        let mut p = Protocol::new(Request {
            kind: Kind::Codex,
            binary: "unused".into(),
            cwd: "/tmp".into(),
            prompt: String::new(),
            provider_id: None,
            model: None,
            settings: Settings::default(),
        });
        p.permissions = crate::permissions::Store::new(path.into());
        p
    }

    fn request() -> Value {
        json!({"serverName":"cua_repl", "threadId":"first", "turnId":"turn-1",
            "mode":"form", "message":"Allow Computer Use to use \"Monocode Native\"?",
            "requestedSchema":{"type":"object", "properties":{"approved":{"type":"boolean"}}, "required":["approved"]}})
    }

    fn ask(p: &mut Protocol, request: Value, out: &mut Vec<u8>) -> Vec<Event> {
        let mut events = Vec::new();
        p.codex_elicitation(json!(17), request, &mut events, out)
            .unwrap();
        events
    }

    #[test]
    fn always_allow_survives_restart_and_revocation_restores_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut p = protocol(&path);
        let mut out = Vec::new();
        let events = ask(&mut p, request(), &mut out);
        let [Event::Approval(approval)] = events.as_slice() else {
            panic!("expected approval");
        };
        assert!(approval.remember_scope.is_some());
        assert!(out.is_empty());
        assert_eq!(
            p.remember_approval(&approval.id, &mut out).unwrap(),
            vec![Event::ApprovalResolved(approval.id.clone())]
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["result"],
            json!({"action":"accept", "content":{"approved":true}, "_meta":null})
        );
        assert!(p.approvals.is_empty());

        let mut restarted = protocol(&path);
        let mut next = request();
        next["threadId"] = json!("another-chat");
        next["turnId"] = json!("another-turn");
        out.clear();
        assert!(ask(&mut restarted, next, &mut out).is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["result"]["action"],
            "accept"
        );
        let grant = p.permissions.list().unwrap().pop().unwrap();
        p.permissions.remove(&grant.key).unwrap();
        out.clear();
        assert!(matches!(
            ask(&mut restarted, request(), &mut out).as_slice(),
            [Event::Approval(_)]
        ));
        assert!(out.is_empty());
    }

    #[test]
    fn always_allow_matches_a_later_call_with_different_call_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let with_meta = |call: &str, turn: &str| {
            let mut r = request();
            r["_meta"] = json!({"callId":call, "tool_name":"get_app_state",
                "x-codex-turn-metadata":{"turn_id":turn}});
            r
        };
        let mut p = protocol(&path);
        ask(&mut p, with_meta("call-1", "turn-1"), &mut Vec::new());
        p.remember_approval("17", &mut Vec::new()).unwrap();
        let mut later = protocol(&path);
        let mut out = Vec::new();
        assert!(ask(&mut later, with_meta("call-2", "turn-2"), &mut out).is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["result"]["action"],
            "accept"
        );
    }

    #[test]
    fn always_allow_survives_a_codex_standalone_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut first = protocol(&path);
        first.request.binary = "/opt/codex/standalone/releases/0.164.0/bin/codex".into();
        ask(&mut first, request(), &mut Vec::new());
        first.remember_approval("17", &mut Vec::new()).unwrap();
        let mut later = protocol(&path);
        later.request.binary = "/opt/codex/standalone/releases/0.165.0/bin/codex".into();
        let mut out = Vec::new();
        assert!(ask(&mut later, request(), &mut out).is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["result"]["action"],
            "accept"
        );
    }

    #[test]
    fn computer_use_grant_matches_actions_on_the_same_app_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let make = |tool: &str, app: &str| {
            let mut r = request();
            r["_meta"] = json!({"codex_approval_kind":"mcp_tool_call", "connector_id":"computer-use", "tool_name":tool, "tool_params":{"app":app}, "tool_params_display":[tool], "callId":tool});
            r
        };
        let mut p = protocol(&path);
        let events = ask(&mut p, make("get_app_state", "App A"), &mut Vec::new());
        assert!(
            matches!(&events[0], Event::Approval(a) if a.remember_scope.as_ref().unwrap().contains("actions for this app"))
        );
        p.remember_approval("17", &mut Vec::new()).unwrap();
        let mut later = protocol(&path);
        assert!(ask(&mut later, make("click", "App A"), &mut Vec::new()).is_empty());
        assert!(matches!(
            ask(&mut later, make("click", "App B"), &mut Vec::new()).as_slice(),
            [Event::Approval(_)]
        ));
    }

    #[test]
    fn saved_approval_matches_linked_worktrees_but_not_other_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let worktree = dir.path().join("worktree");
        let other = dir.path().join("other");
        std::fs::create_dir_all(main.join(".git/worktrees/test")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(other.join(".git")).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/test").display()),
        )
        .unwrap();
        std::fs::write(main.join(".git/worktrees/test/commondir"), "../..\n").unwrap();
        let path = dir.path().join("permissions.json");
        let mut p = protocol(&path);
        p.request.cwd = worktree;
        ask(&mut p, request(), &mut Vec::new());
        p.remember_approval("17", &mut Vec::new()).unwrap();
        let mut later = protocol(&path);
        later.request.cwd = main;
        assert!(ask(&mut later, request(), &mut Vec::new()).is_empty());
        later.request.cwd = other;
        assert!(matches!(
            ask(&mut later, request(), &mut Vec::new()).as_slice(),
            [Event::Approval(_)]
        ));
    }

    #[test]
    fn one_time_and_denied_decisions_do_not_create_grants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        for decision in [Decision::Allow, Decision::Deny] {
            let mut p = protocol(&path);
            ask(&mut p, request(), &mut Vec::new());
            p.respond(
                Control {
                    id: "17".into(),
                    decision,
                },
                &mut Vec::new(),
            )
            .unwrap();
            assert!(!path.exists());
            assert!(matches!(
                ask(&mut p, request(), &mut Vec::new()).as_slice(),
                [Event::Approval(_)]
            ));
        }
    }

    #[test]
    fn changed_targets_schemas_metadata_accounts_and_projects_prompt_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut original = protocol(&path);
        ask(&mut original, request(), &mut Vec::new());
        original.remember_approval("17", &mut Vec::new()).unwrap();
        for field in [
            "message",
            "requestedSchema",
            "_meta",
            "serverName",
            "resource",
        ] {
            let mut p = protocol(&path);
            let mut changed = request();
            changed[field] = if field == "requestedSchema" {
                json!({"type":"object", "properties":{}})
            } else {
                json!("different")
            };
            let mut out = Vec::new();
            assert!(
                matches!(
                    ask(&mut p, changed, &mut out).as_slice(),
                    [Event::Approval(_)]
                ),
                "{field}"
            );
            assert!(out.is_empty());
        }
        let mut p = protocol(&path);
        p.request.cwd = dir.path().into();
        assert!(matches!(
            ask(&mut p, request(), &mut Vec::new()).as_slice(),
            [Event::Approval(_)]
        ));
        let mut p = protocol(&path);
        p.request.settings.account = crate::accounts::Profile::new("other", dir.path());
        assert!(matches!(
            ask(&mut p, request(), &mut Vec::new()).as_slice(),
            [Event::Approval(_)]
        ));
        let mut p = protocol(&path);
        p.request
            .settings
            .environment
            .insert("CODEX_HOME".into(), dir.path().display().to_string());
        assert!(matches!(
            ask(&mut p, request(), &mut Vec::new()).as_slice(),
            [Event::Approval(_)]
        ));
    }

    #[test]
    fn restricted_modes_never_reuse_or_create_saved_grants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut p = protocol(&path);
        ask(&mut p, request(), &mut Vec::new());
        p.remember_approval("17", &mut Vec::new()).unwrap();
        for plan in [true, false] {
            let mut p = protocol(&path);
            p.request.settings.plan = plan;
            p.request.settings.access = if plan {
                AccessMode::FullAccess
            } else {
                AccessMode::ReadOnly
            };
            let mut out = Vec::new();
            let events = ask(&mut p, request(), &mut out);
            let [Event::Approval(approval)] = events.as_slice() else {
                panic!("expected prompt");
            };
            assert!(approval.remember_scope.is_none());
            assert!(matches!(
                p.remember_approval("17", &mut out).unwrap().as_slice(),
                [Event::ApprovalSaveFailed { .. }]
            ));
            assert!(out.is_empty());
        }
    }

    #[test]
    fn save_failure_keeps_request_open_and_stale_requests_cannot_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        std::fs::create_dir(&path).unwrap();
        let mut p = protocol(&path);
        let mut out = Vec::new();
        ask(&mut p, request(), &mut out);
        assert!(matches!(
            p.remember_approval("17", &mut out).unwrap().as_slice(),
            [Event::ApprovalSaveFailed { .. }]
        ));
        assert!(out.is_empty());
        assert!(p.approvals.contains_key("17"));
        p.respond(
            Control {
                id: "17".into(),
                decision: Decision::Deny,
            },
            &mut out,
        )
        .unwrap();
        std::fs::remove_dir(&path).unwrap();
        assert!(p.remember_approval("17", &mut out).unwrap().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn arbitrary_forms_and_missing_server_identity_cannot_be_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = protocol(&dir.path().join("permissions.json"));
        let mut form = request();
        form["requestedSchema"]["properties"]["secret"] = json!({"type":"string"});
        assert!(p.mcp_subject(&form).is_none());
        let mut missing = request();
        missing.as_object_mut().unwrap().remove("serverName");
        assert!(p.mcp_subject(&missing).is_none());
        assert!(ask(&mut p, form, &mut Vec::new())
            .iter()
            .all(|e| !matches!(e, Event::Approval(_))));
    }
}
