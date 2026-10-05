use std::collections::{HashMap, HashSet, VecDeque};

use monocode_core::block::{AgentStepKind, TurnMetrics};
use monocode_core::harness_event::HarnessEvent;
use monocode_core::task_list::task_list_from_tool_input;
use serde_json::{Value, json};

use super::super::deps::{
    ComposeToolTitle, compose_tool_title, extract_shell_command, extract_skill_name,
};
use super::super::protocol::{
    OpenCodePart, detail_from_tool_part, preview_from_tool_part, session_error_message,
    tool_kind_from_name,
};

pub struct Decoder {
    pub session: String,
    pub planning: bool,
    pub context_window: Option<i64>,
    pub sequence: HashMap<String, u64>,
    children: HashMap<String, String>,
    parents: HashMap<String, String>,
    pending: VecDeque<Value>,
    text: HashMap<String, String>,
    tools: HashMap<String, OpenCodePart>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    metrics: HashMap<String, TurnMetrics>,
}

pub struct Decoded {
    pub events: Vec<HarnessEvent>,
    pub ended: Option<Result<(), String>>,
}

impl Decoder {
    pub fn new(session: String) -> Self {
        Self {
            session,
            planning: false,
            context_window: None,
            sequence: HashMap::new(),
            children: HashMap::new(),
            parents: HashMap::new(),
            pending: VecDeque::new(),
            text: HashMap::new(),
            tools: HashMap::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            metrics: HashMap::new(),
        }
    }

    pub fn begin(&mut self, planning: bool) {
        self.planning = planning;
        self.text.clear();
        self.tools.clear();
        self.metrics.clear();
        self.pending.clear();
    }

    pub fn sessions(&self) -> Vec<String> {
        std::iter::once(self.session.clone())
            .chain(self.children.keys().cloned())
            .collect()
    }

    pub fn belongs(&self, session: &str) -> bool {
        session == self.session
            || self.children.contains_key(session)
            || self
                .parents
                .get(session)
                .is_some_and(|parent| self.belongs(parent))
    }

    pub fn checkpoint(&mut self, event: &Value) {
        if let (Some(session), Some(seq)) = (
            event.pointer("/data/sessionID").and_then(Value::as_str),
            event.pointer("/durable/seq").and_then(Value::as_u64),
        ) {
            self.sequence
                .entry(session.into())
                .and_modify(|value| *value = (*value).max(seq))
                .or_insert(seq);
        }
    }

    /// Full text boundaries also repair deltas lost by the volatile stream.
    pub fn decode(&mut self, event: &Value) -> Decoded {
        let mut result = Decoded {
            events: Vec::new(),
            ended: None,
        };
        let data = &event["data"];
        let kind = event["type"].as_str().unwrap_or_default();
        let session = data["sessionID"].as_str().unwrap_or_default();
        if kind == "session.created"
            && let Some(parent) = data["parentID"].as_str().filter(|_| !session.is_empty())
            && session != parent
            && self.belongs(parent)
        {
            self.parents.insert(session.into(), parent.into());
        }
        if session != self.session && !self.children.contains_key(session) {
            if self.belongs(session) && kind.starts_with("session.") {
                if self.pending.len() == 256 {
                    self.pending.pop_front();
                }
                self.pending.push_back(event.clone());
            }
            return result;
        }
        if let Some(id) = event["id"].as_str() {
            if !self.seen.insert(id.into()) {
                return result;
            }
            self.seen_order.push_back(id.into());
            if self.seen_order.len() > 4096
                && let Some(id) = self.seen_order.pop_front()
            {
                self.seen.remove(&id);
            }
        }
        if let Some(seq) = event.pointer("/durable/seq").and_then(Value::as_u64)
            && self
                .sequence
                .get(session)
                .is_some_and(|current| seq <= *current)
        {
            return result;
        }
        self.checkpoint(event);
        let child_call = self.children.get(session).cloned();
        let message = data["assistantMessageID"].as_str().unwrap_or_default();
        let key = format!(
            "{session}/{message}/{}/{}",
            data["ordinal"],
            if kind.contains("reasoning") {
                "reasoning"
            } else {
                "text"
            }
        );
        match kind {
            "session.text.delta"
            | "session.reasoning.delta"
            | "session.text.ended"
            | "session.reasoning.ended" => {
                let ended = kind.ends_with(".ended");
                let reasoning = kind.contains("reasoning");
                let full = self.text.entry(key.clone()).or_default();
                let delta = if ended {
                    let value = data["text"].as_str().unwrap_or_default();
                    let delta = value
                        .strip_prefix(full.as_str())
                        .unwrap_or_default()
                        .to_string();
                    *full = value.into();
                    delta
                } else {
                    let delta = data["delta"].as_str().unwrap_or_default().to_string();
                    full.push_str(&delta);
                    delta
                };
                if let Some(call) = child_call {
                    result.events.push(HarnessEvent::AgentStep {
                        call_id: call,
                        step_id: key,
                        kind: if reasoning {
                            AgentStepKind::Reasoning
                        } else {
                            AgentStepKind::Message
                        },
                        text: full.clone(),
                        tool_kind: None,
                        status: ended.then(|| "completed".into()),
                        detail: None,
                        preview: None,
                        agent_name: None,
                        agent_type: None,
                    });
                } else if reasoning {
                    if !delta.is_empty() {
                        result
                            .events
                            .push(HarnessEvent::ReasoningDelta { text: delta });
                    }
                    if ended {
                        result.events.push(HarnessEvent::ReasoningCompleted);
                    }
                } else if self.planning {
                    result.events.push(HarnessEvent::Plan {
                        text: full.clone(),
                        key: Some(key),
                        append: Some(false),
                        streaming: Some(!ended),
                    });
                } else {
                    if !delta.is_empty() {
                        result
                            .events
                            .push(HarnessEvent::MessageDelta { text: delta });
                    }
                    if ended {
                        result.events.push(HarnessEvent::MessageCompleted);
                    }
                }
            }
            "session.tool.input.started"
            | "session.tool.input.delta"
            | "session.tool.input.ended"
            | "session.tool.called"
            | "session.tool.progress"
            | "session.tool.success"
            | "session.tool.failed" => {
                self.tool(kind, session, data, child_call, &mut result.events);
            }
            "session.step.ended" => {
                let tokens = &data["tokens"];
                let metric = metrics(tokens);
                self.metrics.insert(message.into(), metric);
                if child_call.is_none() {
                    result.events.push(HarnessEvent::Context {
                        used: Some(
                            token(tokens, "input")
                                + token(tokens, "output")
                                + token(tokens, "reasoning")
                                + token(tokens, "/cache/read")
                                + token(tokens, "/cache/write"),
                        ),
                        window: self.context_window,
                    });
                    result
                        .events
                        .push(HarnessEvent::TurnMetrics(self.aggregate_metrics()));
                }
            }
            "session.retry.scheduled" if child_call.is_none() => {
                result.events.push(HarnessEvent::Status {
                    text: format!("OpenCode is retrying, attempt {}", data["attempt"]),
                });
            }
            "session.compaction.started" if child_call.is_none() => {
                result.events.push(HarnessEvent::Status {
                    text: "Compacting context...".into(),
                })
            }
            "session.compaction.ended" if child_call.is_none() => {
                result.events.push(HarnessEvent::Status {
                    text: "Context compacted".into(),
                })
            }
            "session.compaction.failed" | "session.execution.failed" if child_call.is_none() => {
                result.ended = Some(Err(session_error_message(data.get("error"))))
            }
            "session.execution.succeeded" | "session.execution.interrupted"
                if child_call.is_none() =>
            {
                result.events.push(HarnessEvent::MessageCompleted);
                result.events.push(HarnessEvent::ReasoningCompleted);
                result.ended = Some(Ok(()));
            }
            "session.model.selected" if child_call.is_none() => {
                let model = &data["model"];
                let settings = model["variant"]
                    .as_str()
                    .map(|variant| [("variant".into(), variant.into())].into());
                result.events.push(HarnessEvent::SessionConfigChanged {
                    model: model["providerID"]
                        .as_str()
                        .zip(model["id"].as_str())
                        .map(|(provider, id)| format!("opencode:{provider}/{id}")),
                    model_settings: settings,
                });
            }
            _ => {}
        }
        result
    }

    fn tool(
        &mut self,
        kind: &str,
        session: &str,
        data: &Value,
        child_call: Option<String>,
        events: &mut Vec<HarnessEvent>,
    ) {
        let id = data["id"].as_str().unwrap_or_default();
        if id.is_empty() {
            return;
        }
        let part = self
            .tools
            .entry(format!("{session}/{id}"))
            .or_insert_with(|| OpenCodePart {
                id: id.into(),
                call_id: Some(id.into()),
                part_type: "tool".into(),
                state: Some(
                    json!({"input":{},"metadata":{}})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                ..Default::default()
            });
        if let Some(name) = data["name"].as_str() {
            part.tool = Some(name.into());
        }
        let state = part.state.as_mut().unwrap();
        if let Some(input) = data.get("input").filter(|input| input.is_object()) {
            state.insert("input".into(), input.clone());
        }
        if kind == "session.tool.input.ended"
            && let Some(input) = data["text"]
                .as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .filter(Value::is_object)
        {
            state.insert("input".into(), input);
        }
        if let Some(metadata) = data.get("metadata") {
            state.insert("metadata".into(), metadata.clone());
        }
        if let Some(content) = data["content"].as_array() {
            state.insert(
                "output".into(),
                content
                    .iter()
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into(),
            );
        }
        if let Some(error) = data.get("error") {
            state.insert("error".into(), session_error_message(Some(error)).into());
        }
        let status = match kind {
            "session.tool.success" => "completed",
            "session.tool.failed" => "error",
            _ => "running",
        };
        state.insert("status".into(), status.into());
        let name = part.tool.as_deref().unwrap_or("tool");
        let tool_kind = tool_kind_from_name(name);
        let input = state.get("input").cloned().unwrap_or_else(|| json!({}));
        let title = compose_tool_title(&ComposeToolTitle {
            kind: Some(&tool_kind),
            title: Some(name),
            path: input["filePath"]
                .as_str()
                .or_else(|| input["path"].as_str()),
            query: input["pattern"]
                .as_str()
                .or_else(|| input["query"].as_str()),
            command: extract_shell_command(Some(&input)).as_deref(),
            skill: extract_skill_name(Some(&input)).as_deref(),
            ..Default::default()
        });
        let preview = preview_from_tool_part(part);
        let detail = detail_from_tool_part(part);
        if let Some(call_id) = child_call {
            events.push(HarnessEvent::AgentStep {
                call_id,
                step_id: format!("{session}/{id}"),
                kind: AgentStepKind::Tool,
                text: title,
                tool_kind: Some(tool_kind),
                status: Some(status.into()),
                detail,
                preview,
                agent_name: None,
                agent_type: None,
            });
        } else {
            if kind == "session.tool.input.started" {
                events.push(HarnessEvent::ToolStarted {
                    agent_model: None,
                    call_id: id.into(),
                    title,
                    kind: Some(tool_kind),
                    status: Some(status.into()),
                    background: None,
                    preview,
                    paths: None,
                });
            } else {
                events.push(HarnessEvent::ToolUpdated {
                    agent_model: None,
                    call_id: id.into(),
                    title: Some(title),
                    kind: Some(tool_kind),
                    status: Some(status.into()),
                    detail,
                    preview,
                    paths: None,
                });
            }
            if let Some(items) = task_list_from_tool_input(name, &input) {
                events.push(HarnessEvent::TasksUpdated {
                    key: Some("opencode-todos".into()),
                    explanation: None,
                    merge: Some(false),
                    authoritative: Some(true),
                    provider_session_id: Some(self.session.clone()),
                    items,
                });
            }
        }
        if let Some(child) = part
            .state
            .as_ref()
            .and_then(|state| state.get("metadata"))
            .and_then(|meta| meta.get("sessionId").or_else(|| meta.get("sessionID")))
            .and_then(Value::as_str)
            .filter(|child| *child != self.session)
        {
            self.children.insert(child.into(), id.into());
            let pending: Vec<_> = self.pending.drain(..).collect();
            for event in pending {
                events.extend(self.decode(&event).events);
            }
        }
    }

    fn aggregate_metrics(&self) -> TurnMetrics {
        let mut result = TurnMetrics {
            input_tokens: Some(0),
            output_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            ..Default::default()
        };
        for value in self.metrics.values() {
            *result.input_tokens.as_mut().unwrap() += value.input_tokens.unwrap_or_default();
            *result.output_tokens.as_mut().unwrap() += value.output_tokens.unwrap_or_default();
            *result.cache_read_tokens.as_mut().unwrap() +=
                value.cache_read_tokens.unwrap_or_default();
            *result.cache_write_tokens.as_mut().unwrap() +=
                value.cache_write_tokens.unwrap_or_default();
        }
        let input = result.input_tokens.unwrap_or_default()
            + result.cache_read_tokens.unwrap_or_default()
            + result.cache_write_tokens.unwrap_or_default();
        result.cache_hit_percent = (input > 0)
            .then(|| 100.0 * result.cache_read_tokens.unwrap_or_default() as f64 / input as f64);
        result
    }
}

fn token(value: &Value, field: &str) -> i64 {
    let value = if field.starts_with('/') {
        value.pointer(field)
    } else {
        value.get(field)
    };
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or_default() as i64
}

fn metrics(value: &Value) -> TurnMetrics {
    TurnMetrics {
        input_tokens: Some(token(value, "input")),
        output_tokens: Some(token(value, "output") + token(value, "reasoning")),
        cache_read_tokens: Some(token(value, "/cache/read")),
        cache_write_tokens: Some(token(value, "/cache/write")),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_usage_reports_catalog_capacity_and_aggregates_reasoning_and_cache() {
        let mut decoder = Decoder::new("ses_owned".into());
        decoder.context_window = Some(128_000);
        let result = decoder.decode(&json!({"id":"evt_step","type":"session.step.ended","data":{"sessionID":"ses_owned","assistantMessageID":"msg_owned","tokens":{"input":10,"output":5,"reasoning":2,"cache":{"read":3,"write":4}}}}));
        assert!(result.events.contains(&HarnessEvent::Context {
            used: Some(24),
            window: Some(128_000)
        }));
        assert!(result.events.iter().any(|event| matches!(event, HarnessEvent::TurnMetrics(value) if value.input_tokens == Some(10) && value.output_tokens == Some(7) && value.cache_read_tokens == Some(3) && value.cache_write_tokens == Some(4))));
    }

    #[test]
    fn full_boundaries_recover_lost_deltas_without_repeating_delivered_text() {
        let mut decoder = Decoder::new("ses_owned".into());
        let first = decoder.decode(&json!({"id":"evt_delta","type":"session.text.delta","data":{"sessionID":"ses_owned","assistantMessageID":"msg_owned","ordinal":0,"delta":"hel"}}));
        assert_eq!(
            first.events,
            vec![HarnessEvent::MessageDelta { text: "hel".into() }]
        );
        let full = json!({"id":"evt_full","type":"session.text.ended","durable":{"seq":4},"data":{"sessionID":"ses_owned","assistantMessageID":"msg_owned","ordinal":0,"text":"hello"}});
        assert_eq!(
            decoder.decode(&full).events,
            vec![
                HarnessEvent::MessageDelta { text: "lo".into() },
                HarnessEvent::MessageCompleted
            ]
        );
        assert!(decoder.decode(&full).events.is_empty());
        assert_eq!(decoder.sequence["ses_owned"], 4);
        assert!(
            decoder
                .decode(
                    &json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_other"}})
                )
                .ended
                .is_none()
        );
    }

    #[test]
    fn child_text_waits_for_the_explicit_task_binding() {
        let mut decoder = Decoder::new("ses_owned".into());
        decoder.decode(&json!({"type":"session.created","data":{"sessionID":"ses_child","parentID":"ses_owned"}}));
        assert!(decoder.decode(&json!({"type":"session.text.ended","data":{"sessionID":"ses_child","assistantMessageID":"msg_child","ordinal":0,"text":"child answer"}})).events.is_empty());
        decoder.decode(&json!({"type":"session.tool.input.started","data":{"sessionID":"ses_owned","id":"task_owned","name":"task"}}));
        let events = decoder.decode(&json!({"type":"session.tool.progress","data":{"sessionID":"ses_owned","id":"task_owned","metadata":{"sessionID":"ses_child"}}})).events;
        assert!(events.iter().any(|event| matches!(event, HarnessEvent::AgentStep { call_id, text, .. } if call_id == "task_owned" && text == "child answer")));
    }
}
