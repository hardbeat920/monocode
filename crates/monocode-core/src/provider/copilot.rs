//! GitHub Copilot's ACP configuration differs from the other ACP providers.
use super::*;

impl Protocol {
    pub(super) fn copilot_options(&mut self, setup: &Value) -> Result<(), String> {
        let planning =
            self.request.settings.plan || self.request.settings.access == AccessMode::ReadOnly;
        let mode = if planning { "plan" } else { "agent" };
        self.acp.options.push_back((
            "session/set_mode",
            json!({"sessionId":self.acp.session,"modeId":format!("https://agentclientprotocol.com/protocol/session-modes#{mode}")}),
        ));
        if let Some(model) = self
            .request
            .model
            .as_ref()
            .filter(|model| model.as_str() != "default")
            .filter(|model| {
                setup["models"]["currentModelId"].as_str() != Some(model.as_str())
                    && !setup["configOptions"].as_array().is_some_and(|options| {
                        options.iter().any(|option| {
                            option["id"] == "model"
                                && option["currentValue"].as_str() == Some(model.as_str())
                        })
                    })
            })
        {
            self.acp.options.push_back((
                "session/set_model",
                json!({"sessionId":self.acp.session,"modelId":model}),
            ));
        }
        // Reset on every turn, including warm resumes and after native commands.
        // A failure must abort the turn, or the CLI could bypass app approvals.
        self.acp.options.push_back((
            "session/set_config_option",
            json!({"sessionId":self.acp.session,"configId":"allow_all","value":"off"}),
        ));
        Ok(())
    }

    pub(super) fn copilot_settings(&mut self, setup: &Value) -> Result<(), String> {
        let option = setup["configOptions"].as_array().and_then(|options| {
            options
                .iter()
                .find(|option| option["id"] == "reasoning_effort")
        });
        let Some(option) = option else { return Ok(()) };
        let mut choices = Vec::new();
        config_choices(&option["options"], &mut choices);
        if choices.is_empty() {
            return Ok(());
        }
        let effort = self
            .request
            .settings
            .model_settings
            .get("reasoningEffort")
            .or_else(|| self.request.settings.model_settings.get("reasoning_effort"))
            .or(self.request.settings.effort.as_ref())
            .map(String::as_str)
            .or_else(|| option["currentValue"].as_str());
        if let Some(effort) = effort {
            if !choices.iter().any(|choice| choice == effort) {
                return Err(format!(
                    "Copilot does not support reasoning effort \"{effort}\" for this model"
                ));
            }
            self.acp.options.push_back((
                "session/set_config_option",
                json!({"sessionId":self.acp.session,"configId":"reasoning_effort","value":effort}),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn protocol() -> Protocol {
        Protocol::new(Request {
            kind: Kind::Copilot,
            binary: "copilot".into(),
            cwd: "/tmp".into(),
            prompt: "Hello".into(),
            provider_id: None,
            model: Some("gpt".into()),
            settings: Default::default(),
        })
    }
    fn last(out: &mut Vec<u8>) -> Value {
        let text = String::from_utf8(std::mem::take(out)).unwrap();
        serde_json::from_str(text.lines().last().unwrap()).unwrap()
    }
    fn reply(p: &mut Protocol, out: &mut Vec<u8>, result: Value) -> Value {
        p.frame(json!({"id":p.acp.sequence,"result":result}), out)
            .unwrap();
        last(out)
    }
    #[test]
    fn configures_mode_model_and_permissions_before_each_turn() {
        let mut p = protocol();
        let mut out = Vec::new();
        let mut command = Command::new("copilot");
        configure(&mut command, &p.request).unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--acp", "--stdio", "--no-auto-update"]
        );
        p.acp_start(&mut out).unwrap();
        assert_eq!(last(&mut out)["method"], "initialize");
        assert_eq!(reply(&mut p, &mut out, json!({}))["method"], "session/new");
        let mode = reply(
            &mut p,
            &mut out,
            json!({"sessionId":"s","configOptions":[{"id":"allow_all","currentValue":"off"}]}),
        );
        assert_eq!(
            mode["params"]["modeId"],
            "https://agentclientprotocol.com/protocol/session-modes#agent"
        );
        assert_eq!(
            reply(&mut p, &mut out, json!({}))["method"],
            "session/set_model"
        );
        assert_eq!(
            reply(&mut p, &mut out, json!({}))["params"]["configId"],
            "allow_all"
        );
        assert_eq!(
            reply(&mut p, &mut out, json!({}))["method"],
            "session/prompt"
        );
        p.acp_resume_warm(&mut out).unwrap();
        last(&mut out);
        reply(&mut p, &mut out, json!({}));
        assert_eq!(reply(&mut p, &mut out, json!({}))["params"]["value"], "off");
        let err = p
            .frame(
                json!({"id":p.acp.sequence,"error":{"message":"unsupported"}}),
                &mut out,
            )
            .unwrap_err();
        assert!(err.contains("unsupported"));
        assert!(out.is_empty());
    }
    #[test]
    fn resume_errors_never_create_a_replacement_session() {
        let mut p = protocol();
        p.request.provider_id = Some("existing".into());
        let mut out = Vec::new();
        p.acp_start(&mut out).unwrap();
        last(&mut out);
        assert!(p
            .frame(json!({"id":p.acp.sequence,"result":{}}), &mut out)
            .unwrap_err()
            .contains("cannot resume"));
        let load = reply(
            &mut p,
            &mut out,
            json!({"agentCapabilities":{"loadSession":true}}),
        );
        assert_eq!(load["method"], "session/load");
        assert!(p
            .frame(
                json!({"id":p.acp.sequence,"error":{"message":"session not found"}}),
                &mut out
            )
            .is_err());
        assert!(out.is_empty());
    }
    #[test]
    fn efforts_are_validated_against_the_selected_models_options() {
        let mut p = protocol();
        p.request.settings.effort = Some("high".into());
        p.copilot_settings(&json!({})).unwrap();
        assert!(p.acp.options.is_empty());
        let setup = json!({"configOptions":[{"id":"reasoning_effort","currentValue":"low","options":[{"value":"low"},{"value":"high"}]}]});
        p.copilot_settings(&setup).unwrap();
        assert_eq!(p.acp.options.pop_front().unwrap().1["value"], "high");
        p.request.settings.effort = Some("unsupported".into());
        assert!(p.copilot_settings(&setup).is_err());
    }
    #[test]
    fn approval_policy_handles_opaque_options_and_unknown_tool_kinds() {
        for (access, plan, kind, expected) in [
            (AccessMode::ReadOnly, false, "edit", Some("no")),
            (AccessMode::FullAccess, true, "read", Some("yes")),
            (AccessMode::FullAccess, true, "execute", Some("no")),
            (AccessMode::FullAccess, false, "execute", Some("yes")),
            (AccessMode::AutoAcceptEdits, false, "edit", Some("yes")),
            (AccessMode::AutoAcceptEdits, false, "unknown", None),
            (AccessMode::Auto, false, "execute", None),
        ] {
            let mut p = protocol();
            p.request.settings.access = access;
            p.request.settings.plan = plan;
            p.acp.stage = Stage::Prompt;
            let mut out = Vec::new();
            let events = p.frame(json!({"id":"permission","method":"session/request_permission","params":{"toolCall":{"kind":kind},"options":[{"kind":"allow_once","optionId":"yes"},{"kind":"reject_once","optionId":"no"}]}}), &mut out).unwrap();
            if let Some(expected) = expected {
                assert_eq!(last(&mut out)["result"]["outcome"]["optionId"], expected);
            } else {
                assert!(matches!(
                    &events[0],
                    Event::Approval(Approval {
                        can_remember: false,
                        ..
                    })
                ));
                assert!(out.is_empty());
            }
        }
    }
    #[test]
    fn command_probe_never_submits_a_prompt() {
        let mut p = protocol();
        p.command_probe = true;
        let mut out = Vec::new();
        p.acp_session(&mut out).unwrap();
        last(&mut out);
        p.frame(
            json!({"id":p.acp.sequence,"result":{"sessionId":"probe"}}),
            &mut out,
        )
        .unwrap();
        assert!(p.acp.stage == Stage::Commands);
        assert!(out.is_empty());
        let events = p.frame(json!({"method":"session/update","params":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"plan"}]}}}), &mut out).unwrap();
        assert!(
            matches!(&events[0], Event::Commands(commands) if commands[0].invocation == "copilot:plan")
        );
        p.acp.deadline = Some(Instant::now());
        p.acp_tick(&mut out).unwrap();
        assert!(p.done);
        assert!(out.is_empty());
    }
}

#[derive(Default)]
pub(super) struct Stream {
    searches: std::collections::HashSet<String>,
    pending: String,
    owners: HashMap<String, String>,
    steps: HashMap<String, Vec<crate::tool::tasks::AgentStep>>,
    prose: HashMap<String, (String, String)>,
    sequence: u64,
}
impl Stream {
    fn filter(&mut self, text: &str) -> String {
        const OPEN: &str = "<final_answer>";
        const CLOSE: &str = "</final_answer>";
        if self.pending.is_empty() && self.searches.is_empty() {
            return text.into();
        }
        let mut input = std::mem::take(&mut self.pending) + text;
        let mut output = String::new();
        static CITATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let citation = CITATION.get_or_init(|| {
            regex::Regex::new(r"^\s*(?:/|[A-Za-z]:[\\/]).+:\d+(?:-\d+)?\s*$").unwrap()
        });
        while !input.is_empty() {
            let Some(start) = input.find(OPEN) else {
                let suffix = (1..OPEN.len())
                    .rev()
                    .find(|n| input.ends_with(&OPEN[..*n]))
                    .unwrap_or(0);
                let split = input.len() - suffix;
                output.push_str(&input[..split]);
                self.pending = input[split..].into();
                return output;
            };
            output.push_str(&input[..start]);
            input = input[start..].into();
            let Some(end) = input.find(CLOSE) else {
                if input.len() > 64 * 1024 {
                    output.push_str(&input)
                } else {
                    self.pending = input
                }
                return output;
            };
            let body = input[OPEN.len()..end].trim();
            let lines: Vec<_> = body
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            if lines.is_empty() || !lines.iter().all(|line| citation.is_match(line)) {
                output.push_str(&input[..end + CLOSE.len()]);
            }
            input = input[end + CLOSE.len()..].into();
        }
        output
    }
}
impl Protocol {
    pub(super) fn copilot_update(&mut self, update: &Value, events: &mut Vec<Event>) {
        use crate::tool::tasks::{AgentRun, AgentStep, Panel};
        let id = update["toolCallId"].as_str().unwrap_or("");
        let parent = update["_meta"]["github.com/copilot"]["agentId"]
            .as_str()
            .or_else(|| update["_meta"]["parentToolCallId"].as_str())
            .or_else(|| self.acp.copilot.owners.get(id).map(String::as_str))
            .filter(|parent| !parent.is_empty() && *parent != id)
            .map(str::to_owned);
        if let Some(parent) = parent {
            if !id.is_empty() {
                self.acp.copilot.owners.insert(id.into(), parent.clone());
            }
            let kind = update["sessionUpdate"].as_str().unwrap_or("");
            let step = if matches!(
                kind,
                "agent_message_chunk" | "agent_message" | "agent_thought_chunk" | "agent_thought"
            ) {
                let kind = if kind.starts_with("agent_message") {
                    "message"
                } else {
                    "reasoning"
                };
                self.acp.copilot.sequence += 1;
                let sequence = self.acp.copilot.sequence;
                let prose = self
                    .acp
                    .copilot
                    .prose
                    .entry(parent.clone())
                    .or_insert_with(|| (format!("{kind}:{sequence}"), String::new()));
                if !prose.0.starts_with(kind) {
                    *prose = (format!("{kind}:{sequence}"), String::new());
                }
                let text = content_text(&update["content"]);
                if matches!(
                    update["sessionUpdate"].as_str(),
                    Some("agent_message" | "agent_thought")
                ) {
                    prose.1.clear();
                }
                prose.1.extend(
                    text.chars()
                        .take(2000_usize.saturating_sub(prose.1.chars().count())),
                );
                Some(AgentStep {
                    id: prose.0.clone(),
                    kind: kind.into(),
                    text: prose.1.clone(),
                    ..Default::default()
                })
            } else if matches!(
                kind,
                "tool_call" | "tool_call_update" | "tool_call_content_chunk"
            ) {
                self.acp.copilot.prose.remove(&parent);
                let mut child = Vec::new();
                self.acp_update_body(update, &mut child);
                child.into_iter().find_map(|event| {
                    if let Event::Tool(call) = event {
                        Some(AgentStep::from_tool_call(&call))
                    } else {
                        None
                    }
                })
            } else {
                None
            };
            if let Some(step) = step {
                let steps = self.acp.copilot.steps.entry(parent.clone()).or_default();
                if let Some(existing) = steps.iter_mut().find(|existing| existing.id == step.id) {
                    *existing = step
                } else {
                    steps.push(step);
                }
                if steps.len() > 64 {
                    steps.remove(0);
                }
            }
            if self.acp.copilot.steps.len() > 32 {
                self.acp.copilot.steps.retain(|key, _| key == &parent);
            }
            self.copilot_child_steps(&parent, events);
            return;
        }
        let mut update = update.clone();
        if update["title"] == "search_code_subagent" {
            self.acp.copilot.searches.insert(id.into());
            update["kind"] = "agent".into();
            update["title"] = "Search code".into();
        }
        let mut routed = Vec::new();
        self.acp_update_body(&update, &mut routed);
        if matches!(
            update["status"].as_str(),
            Some("completed" | "failed" | "cancelled")
        ) {
            self.acp.copilot.searches.remove(id);
        }
        for event in routed {
            match event {
                Event::Text(text) => {
                    let text = self.acp.copilot.filter(&text);
                    if !text.is_empty() {
                        events.push(Event::Text(text));
                    }
                }
                Event::Tool(mut call) => {
                    if self.acp.copilot.steps.contains_key(&call.id) && call.agent_mut().is_none() {
                        call.panel = Some(Panel::Agent(AgentRun {
                            brief: call.name.clone(),
                            ..Default::default()
                        }));
                        self.tools.insert(call.id.clone(), call.clone());
                    }
                    let id = call.id.clone();
                    events.push(Event::Tool(call));
                    self.copilot_child_steps(&id, events);
                }
                other => events.push(other),
            }
        }
    }
    fn copilot_child_steps(&mut self, parent: &str, events: &mut Vec<Event>) {
        use crate::tool::tasks::{AgentRun, Panel};
        if let Some(call) = self.tools.get_mut(parent) {
            if let Some(steps) = self.acp.copilot.steps.remove(parent) {
                if call.agent_mut().is_none() {
                    call.panel = Some(Panel::Agent(AgentRun {
                        brief: call.name.clone(),
                        ..Default::default()
                    }));
                }
                let run = call.agent_mut().unwrap();
                for step in steps {
                    run.upsert(step);
                }
                events.push(Event::Tool(call.clone()));
            }
        }
    }
    pub(super) fn copilot_flush(&mut self, events: &mut Vec<Event>) {
        let text = std::mem::take(&mut self.acp.copilot.pending);
        self.acp.copilot.searches.clear();
        self.acp.copilot.prose.clear();
        if !text.is_empty() {
            events.push(Event::Text(text));
        }
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    #[test]
    fn search_citations_are_filtered_across_chunks_but_real_answers_survive() {
        let mut stream = Stream::default();
        stream.searches.insert("search".into());
        assert_eq!(stream.filter("Before <final_"), "Before ");
        assert_eq!(
            stream.filter("answer>/src/main.rs:12-14\nC:\\src\\main.rs:5</final_answer>After"),
            "After"
        );
        assert_eq!(
            stream.filter("<final_answer>Actual answer</final_answer>"),
            "<final_answer>Actual answer</final_answer>"
        );
        assert_eq!(stream.filter("<final_answer>unfinished"), "");
        assert_eq!(stream.pending, "<final_answer>unfinished");
    }
    #[test]
    fn child_activity_is_attached_to_its_agent_and_never_becomes_parent_text() {
        let mut p = Protocol::new(Request {
            kind: Kind::Copilot,
            binary: "unused".into(),
            cwd: "/tmp".into(),
            prompt: "Hello".into(),
            provider_id: None,
            model: None,
            settings: Default::default(),
        });
        let mut events = Vec::new();
        p.acp_update(&json!({"sessionUpdate":"agent_message_chunk","_meta":{"github.com/copilot":{"agentId":"parent"}},"content":{"type":"text","text":"child answer"}}), &mut events);
        assert!(events.is_empty());
        p.acp_update(&json!({"sessionUpdate":"tool_call","toolCallId":"parent","title":"search_code_subagent"}), &mut events);
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::Text(_) | Event::Reasoning(_))));
        let run = p.tools.get_mut("parent").unwrap().agent_mut().unwrap();
        assert_eq!(run.steps[0].text, "child answer");
        events.clear();
        p.acp_update(&json!({"sessionUpdate":"tool_call","toolCallId":"child","kind":"read","title":"Read file","_meta":{"github.com/copilot":{"agentId":"parent"}}}), &mut events);
        assert!(events
            .iter()
            .all(|event| matches!(event, Event::Tool(call) if call.id == "parent")));
        let run = p.tools.get_mut("parent").unwrap().agent_mut().unwrap();
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[1].kind, "tool");
    }
}
