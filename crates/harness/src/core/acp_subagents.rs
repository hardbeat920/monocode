//! Port of src/integrations/harness/core/acpSubagents.ts: route explicitly
//! attributed ACP child activity onto its parent Agent tool call, without
//! mixing child prose into the parent's own text.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use monocode_core::block::{AgentStepKind, ToolPreview};
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use monocode_core::reducer::{
    agent_tool_title, is_agent_tool, is_agent_tool_name, merge_tool_preview,
};

/// `isFailedStatus` from src/features/sessions/model/transcriptActivity.ts.
pub fn is_failed_status(status: Option<&str>) -> bool {
    let value = status.unwrap_or("").to_lowercase();
    matches!(
        value.as_str(),
        "failed" | "error" | "cancelled" | "canceled"
    )
}

/// The fields of an `agent.step` event.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    call_id: String,
    step_id: String,
    kind: AgentStepKind,
    text: String,
    tool_kind: Option<String>,
    status: Option<String>,
    detail: Option<String>,
    preview: Option<ToolPreview>,
}

impl Step {
    fn into_event(self) -> HarnessEvent {
        HarnessEvent::AgentStep {
            call_id: self.call_id,
            step_id: self.step_id,
            kind: self.kind,
            text: self.text,
            tool_kind: self.tool_kind,
            status: self.status,
            detail: self.detail,
            preview: self.preview,
            agent_name: None,
            agent_type: None,
        }
    }

    /// `{ ...prev, ...next, text: next.text || prev.text, toolKind: next.toolKind
    /// ?? prev.toolKind, status: next.status ?? prev.status, preview:
    /// mergeToolPreview(next.preview, prev.preview) }`. `detail` is only on a
    /// step that carries one, so `prev`'s survives otherwise.
    fn merge(prev: &Step, next: Step) -> Step {
        Step {
            text: if next.text.is_empty() {
                prev.text.clone()
            } else {
                next.text
            },
            tool_kind: next.tool_kind.or_else(|| prev.tool_kind.clone()),
            status: next.status.or_else(|| prev.status.clone()),
            detail: next.detail.or_else(|| prev.detail.clone()),
            preview: merge_tool_preview(next.preview.as_ref(), prev.preview.as_ref()),
            call_id: next.call_id,
            step_id: next.step_id,
            kind: next.kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProseKind {
    Message,
    Reasoning,
}

impl ProseKind {
    fn as_str(self) -> &'static str {
        match self {
            ProseKind::Message => "message",
            ProseKind::Reasoning => "reasoning",
        }
    }

    fn step_kind(self) -> AgentStepKind {
        match self {
            ProseKind::Message => AgentStepKind::Message,
            ProseKind::Reasoning => AgentStepKind::Reasoning,
        }
    }
}

#[derive(Debug, Clone)]
struct Prose {
    id: u64,
    kind: ProseKind,
    text: String,
}

const MAX_PROSE: usize = 2_000;
const MAX_BACKLOG: usize = 64;
const MAX_PENDING_PARENTS: usize = 32;

/// `AcpSubagents`: one router per ACP connection.
#[derive(Debug, Default)]
pub struct AcpSubagents {
    tools: HashSet<String>,
    owners: HashMap<String, String>,
    /// Insertion order matters: the oldest parent is dropped first.
    pending: Vec<(String, Vec<Step>)>,
    prose: HashMap<String, Prose>,
    sequence: u64,
}

fn record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// `a ?? b ?? ...`: the first value that is present and not null.
fn first_present<'a>(values: impl IntoIterator<Item = Option<&'a Value>>) -> Option<&'a Value> {
    values.into_iter().flatten().find(|value| !value.is_null())
}

/// `text`: a string with something besides whitespace.
fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !js::trim(text).is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn field<'a>(map: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    map.and_then(|map| map.get(key))
}

impl AcpSubagents {
    pub fn new() -> Self {
        Self::default()
    }

    /// `isChild`.
    pub fn is_child(&self, params: &Value) -> bool {
        self.parent(params).is_some()
    }

    fn pending_index(&self, call_id: &str) -> Option<usize> {
        self.pending.iter().position(|(id, _)| id == call_id)
    }

    fn take_backlog(&mut self, call_id: &str) -> Vec<Step> {
        match self.pending_index(call_id) {
            Some(index) => self.pending.remove(index).1,
            None => Vec::new(),
        }
    }

    /// `route`: pass parent events through, turn child events into
    /// `agent.step` events on the parent, and hold child steps whose parent
    /// row does not exist yet.
    pub fn route(&mut self, params: &Value, events: Vec<HarnessEvent>) -> Vec<HarnessEvent> {
        let Some(parent) = self.parent(params) else {
            let mut out = Vec::with_capacity(events.len());
            for event in events {
                let call_id = match &event {
                    HarnessEvent::ToolStarted { call_id, .. }
                    | HarnessEvent::ToolUpdated { call_id, .. } => Some(call_id.clone()),
                    _ => None,
                };
                out.push(event);
                if let Some(call_id) = call_id {
                    self.tools.insert(call_id.clone());
                    out.extend(
                        self.take_backlog(&call_id)
                            .into_iter()
                            .map(Step::into_event),
                    );
                }
            }
            return out;
        };

        let mut output: Vec<Step> = Vec::new();
        for event in events {
            match event {
                HarnessEvent::ToolStarted {
                    call_id,
                    title,
                    kind,
                    status,
                    preview,
                    ..
                } => {
                    self.owners.insert(call_id.clone(), parent.clone());
                    self.prose.remove(&parent);
                    output.push(Step {
                        call_id: parent.clone(),
                        step_id: format!("tool:{call_id}"),
                        kind: AgentStepKind::Tool,
                        text: title,
                        tool_kind: kind,
                        status,
                        detail: None,
                        preview,
                    });
                }
                HarnessEvent::ToolUpdated {
                    call_id,
                    title,
                    kind,
                    status,
                    detail,
                    preview,
                    ..
                } => {
                    self.owners.insert(call_id.clone(), parent.clone());
                    self.prose.remove(&parent);
                    // Detail is what a red row opens, so only a call that
                    // failed carries one. A settled result is already in the
                    // preview, and storing every child's output would weigh
                    // the saved session down for nothing.
                    let detail = detail
                        .filter(|detail| !detail.is_empty())
                        .filter(|_| is_failed_status(status.as_deref()));
                    output.push(Step {
                        call_id: parent.clone(),
                        step_id: format!("tool:{call_id}"),
                        kind: AgentStepKind::Tool,
                        text: title.unwrap_or_default(),
                        tool_kind: kind,
                        status,
                        detail,
                        preview,
                    });
                }
                HarnessEvent::MessageDelta { text: delta } => {
                    output.push(self.prose_step(params, &parent, ProseKind::Message, &delta));
                }
                HarnessEvent::ReasoningDelta { text: delta } => {
                    output.push(self.prose_step(params, &parent, ProseKind::Reasoning, &delta));
                }
                // Child plans, context meters, and lifecycle notifications
                // belong to the child too. They must never replace or finish
                // the parent's own work.
                _ => {}
            }
        }

        if self.tools.contains(&parent) {
            return output.into_iter().map(Step::into_event).collect();
        }

        let index = match self.pending_index(&parent) {
            Some(index) => index,
            None => {
                self.pending.push((parent.clone(), Vec::new()));
                self.pending.len() - 1
            }
        };
        let backlog = &mut self.pending[index].1;
        for step in output {
            match backlog
                .iter()
                .position(|entry| entry.step_id == step.step_id)
            {
                None => backlog.push(step),
                Some(found) => {
                    let merged = Step::merge(&backlog[found], step);
                    backlog[found] = merged;
                }
            }
        }
        if backlog.len() > MAX_BACKLOG {
            let excess = backlog.len() - MAX_BACKLOG;
            backlog.drain(..excess);
        }
        if self.pending.len() > MAX_PENDING_PARENTS {
            self.pending.remove(0);
        }
        Vec::new()
    }

    fn prose_step(&mut self, params: &Value, parent: &str, kind: ProseKind, delta: &str) -> Step {
        let reuse = self
            .prose
            .get(parent)
            .is_some_and(|prose| prose.kind == kind);
        if !reuse {
            self.sequence += 1;
            self.prose.insert(
                parent.to_string(),
                Prose {
                    id: self.sequence,
                    kind,
                    text: String::new(),
                },
            );
        }
        let envelope = record(Some(params));
        let update = record(field(envelope, "update")).or(envelope);
        let update_type = first_present([
            field(update, "sessionUpdate"),
            field(update, "session_update"),
            field(update, "type"),
        ]);
        let snapshot = matches!(
            update_type.and_then(Value::as_str),
            Some("agent_message" | "agent_thought")
        );
        let prose = self.prose.get_mut(parent).expect("inserted above");
        let joined = if snapshot {
            delta.to_string()
        } else {
            format!("{}{delta}", prose.text)
        };
        prose.text = js::slice_prefix(&joined, MAX_PROSE).to_string();
        Step {
            call_id: parent.to_string(),
            step_id: format!("{}:{}", kind.as_str(), prose.id),
            kind: kind.step_kind(),
            text: prose.text.clone(),
            tool_kind: None,
            status: None,
            detail: None,
            preview: None,
        }
    }

    fn parent(&self, params: &Value) -> Option<String> {
        let envelope = record(Some(params));
        let update = record(field(envelope, "update")).or(envelope);
        let tool = record(field(update, "toolCall")).or_else(|| record(field(update, "tool_call")));
        let id = text(first_present([
            field(tool, "toolCallId"),
            field(tool, "tool_call_id"),
            field(update, "toolCallId"),
            field(update, "tool_call_id"),
        ]));
        let mut parent: Option<String> = None;
        'sources: for source in [tool, update, envelope] {
            let meta = record(field(source, "_meta"));
            for entry in [
                source,
                meta,
                record(field(meta, "cursor")),
                record(field(meta, "grok")),
                record(field(meta, "x.ai")),
                record(field(meta, "fx")),
            ] {
                parent = text(first_present([
                    field(entry, "parentToolCallId"),
                    field(entry, "parent_tool_call_id"),
                ]));
                if parent.is_some() {
                    break 'sources;
                }
            }
        }
        if parent.is_none() {
            parent = id.as_ref().and_then(|id| self.owners.get(id).cloned());
        }
        let mut parent = parent?;
        if Some(&parent) == id.as_ref() {
            return None;
        }
        let mut seen = HashSet::new();
        while let Some(owner) = self.owners.get(&parent) {
            if seen.contains(&parent) {
                break;
            }
            seen.insert(parent.clone());
            parent = owner.clone();
        }
        if seen.contains(&parent) {
            None
        } else {
            Some(parent)
        }
    }
}

/// The result of `acpAgentInfo`. `kind` is always `"agent"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentInfo {
    pub title: String,
    pub agent_model: Option<String>,
}

impl AcpAgentInfo {
    pub const KIND: &'static str = "agent";
}

/// `acpAgentInfo`: some ACP servers classify delegation as `other` and
/// identify it only in the tool input.
pub fn acp_agent_info(
    update: &Map<String, Value>,
    tool: &Map<String, Value>,
    kind: Option<&str>,
    title: Option<&str>,
    native_input: Option<&Value>,
) -> Option<AcpAgentInfo> {
    let input = record(first_present([
        native_input,
        update.get("rawInput"),
        tool.get("rawInput"),
        update.get("raw_input"),
        tool.get("raw_input"),
        update.get("input"),
        tool.get("input"),
    ]));
    let name = text(first_present([
        field(input, "_toolName"),
        field(input, "toolName"),
        update.get("name"),
        tool.get("name"),
    ]));
    let named_agent = name.as_deref().is_some_and(is_agent_tool_name);
    if !is_agent_tool(kind, title) && !named_agent {
        return None;
    }
    let model = text(field(input, "model"));
    let empty = Map::new();
    Some(AcpAgentInfo {
        title: agent_tool_title(input.unwrap_or(&empty), title.unwrap_or("Subagent")),
        agent_model: model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn delta(text: &str) -> HarnessEvent {
        HarnessEvent::MessageDelta { text: text.into() }
    }

    fn tool_started(call_id: &str, title: &str, kind: Option<&str>) -> HarnessEvent {
        HarnessEvent::ToolStarted {
            agent_model: None,
            call_id: call_id.into(),
            title: title.into(),
            kind: kind.map(str::to_string),
            status: None,
            background: None,
            preview: None,
            paths: None,
        }
    }

    fn tool_updated(call_id: &str, title: &str, kind: &str) -> HarnessEvent {
        HarnessEvent::ToolUpdated {
            agent_model: None,
            call_id: call_id.into(),
            title: Some(title.into()),
            kind: Some(kind.into()),
            status: None,
            detail: None,
            preview: None,
            paths: None,
        }
    }

    fn step_of(event: &HarnessEvent) -> (&str, &str, &str) {
        match event {
            HarnessEvent::AgentStep {
                call_id,
                step_id,
                text,
                ..
            } => (call_id, step_id, text),
            other => panic!("expected agent.step, got {other:?}"),
        }
    }

    #[test]
    fn buffers_children_until_the_matching_row_exists_even_with_simultaneous_spawns() {
        let mut router = AcpSubagents::new();
        let child = |router: &mut AcpSubagents, id: &str, text: &str| {
            router.route(
                &json!({ "update": { "_meta": { "cursor": { "parentToolCallId": id } } } }),
                vec![delta(text)],
            )
        };
        assert!(child(&mut router, "b", "Second child").is_empty());
        assert!(child(&mut router, "a", "First child").is_empty());
        let start = |router: &mut AcpSubagents, call_id: &str| {
            router.route(&json!({}), vec![tool_updated(call_id, call_id, "agent")])
        };
        let a = start(&mut router, "a");
        assert_eq!(a.len(), 2);
        assert!(matches!(&a[0], HarnessEvent::ToolUpdated { call_id, .. } if call_id == "a"));
        assert_eq!(step_of(&a[1]).0, "a");
        assert_eq!(step_of(&a[1]).2, "First child");
        let b = start(&mut router, "b");
        assert_eq!(step_of(&b[1]).0, "b");
        assert_eq!(step_of(&b[1]).2, "Second child");
    }

    #[test]
    fn keeps_nested_work_on_its_ancestor_and_suppresses_child_context_and_completion() {
        let mut router = AcpSubagents::new();
        router.route(
            &json!({}),
            vec![tool_started("root", "Explore", Some("agent"))],
        );
        router.route(
            &json!({ "parentToolCallId": "root" }),
            vec![tool_started("nested", "Task", Some("agent"))],
        );
        let params = json!({ "parentToolCallId": "nested" });
        let routed = router.route(&params, vec![delta("Nested answer")]);
        assert_eq!(step_of(&routed[0]).0, "root");
        assert!(
            router
                .route(
                    &params,
                    vec![
                        HarnessEvent::Context {
                            used: Some(999),
                            window: None
                        },
                        HarnessEvent::MessageCompleted,
                        HarnessEvent::SessionEnded { code: None },
                    ],
                )
                .is_empty()
        );
    }

    #[test]
    fn replaces_whole_prose_snapshots_and_starts_a_new_step_after_tools() {
        let mut router = AcpSubagents::new();
        router.route(&json!({}), vec![tool_started("a", "Explore", None)]);
        let params =
            json!({ "update": { "sessionUpdate": "agent_message", "parentToolCallId": "a" } });
        let first = router.route(&params, vec![delta("Hello")]).remove(0);
        let repeated = router.route(&params, vec![delta("Hello again")]).remove(0);
        assert_eq!(step_of(&repeated).1, step_of(&first).1);
        assert_eq!(step_of(&repeated).2, "Hello again");
        router.route(&params, vec![tool_started("t", "Read", None)]);
        let after = router.route(&params, vec![delta("Done")]).remove(0);
        assert_ne!(step_of(&after).1, step_of(&first).1);
    }

    #[test]
    fn keeps_failed_detail_and_drops_settled_detail() {
        let mut router = AcpSubagents::new();
        router.route(
            &json!({}),
            vec![tool_started("spawn", "Task", Some("agent"))],
        );
        let params = json!({ "update": { "_meta": { "parentToolCallId": "spawn" } } });
        let update = |status: &str, detail: &str| HarnessEvent::ToolUpdated {
            agent_model: None,
            call_id: "read".into(),
            title: None,
            kind: Some("read".into()),
            status: Some(status.into()),
            detail: Some(detail.into()),
            preview: None,
            paths: None,
        };
        let failed = router.route(&params, vec![update("failed", "File missing")]);
        assert!(
            matches!(&failed[0], HarnessEvent::AgentStep { detail: Some(d), .. } if d == "File missing")
        );
        let settled = router.route(
            &params,
            vec![update("completed", "export function auth() {}")],
        );
        assert!(matches!(
            &settled[0],
            HarnessEvent::AgentStep { detail: None, .. }
        ));
    }

    #[test]
    fn sparse_updates_find_their_parent_through_the_owner_map() {
        let mut router = AcpSubagents::new();
        router.route(
            &json!({}),
            vec![tool_started("spawn", "Task", Some("agent"))],
        );
        router.route(
            &json!({ "update": { "_meta": { "parentToolCallId": "spawn" }, "toolCallId": "read" } }),
            vec![tool_started("read", "Read auth.ts", Some("read"))],
        );
        let params =
            json!({ "update": { "sessionUpdate": "tool_call_update", "toolCallId": "read" } });
        assert!(router.is_child(&params));
        // A tool can never be its own parent.
        assert!(!router.is_child(
            &json!({ "update": { "toolCallId": "spawn", "parentToolCallId": "spawn" } })
        ));
    }

    #[test]
    fn names_delegation_from_tool_input() {
        let update = json!({
            "rawInput": { "_toolName": "task", "description": "Check auth", "model": "review-model" }
        });
        let info = acp_agent_info(
            update.as_object().unwrap(),
            &Map::new(),
            Some("other"),
            Some("Task"),
            None,
        )
        .unwrap();
        assert_eq!(info.title, "Check auth");
        assert_eq!(info.agent_model.as_deref(), Some("review-model"));
        assert_eq!(
            acp_agent_info(
                &Map::new(),
                &Map::new(),
                Some("read"),
                Some("Read file"),
                None
            ),
            None
        );
        assert!(is_failed_status(Some("Canceled")));
        assert!(!is_failed_status(None));
    }
}

/// The `describe.each(["fx", "grok"])` cases, which run each provider's real
/// ACP parser and the transcript reducer.
#[cfg(all(test, feature = "fx", feature = "grok"))]
mod provider_tests {
    use super::*;
    use monocode_core::block::{Block, BlockRole};
    use monocode_core::harness::HarnessId;
    use monocode_core::reducer::apply_harness_event;
    use monocode_core::session::Session;
    use serde_json::json;

    type Parse = fn(&Value) -> Vec<HarnessEvent>;

    fn providers() -> [(HarnessId, Parse); 2] {
        [
            (
                HarnessId::Fx,
                crate::providers::fx::protocol::events_from_acp_update,
            ),
            (
                HarnessId::Grok,
                crate::providers::grok::protocol::events_from_acp_update,
            ),
        ]
    }

    struct Run {
        router: AcpSubagents,
        session: Session,
        parse: Parse,
    }

    impl Run {
        fn new(harness: HarnessId, parse: Parse) -> Self {
            Self {
                router: AcpSubagents::new(),
                session: Session::blank("s", harness, format!("{harness}:default"), "/repo"),
                parse,
            }
        }

        fn push(&mut self, update: Value) {
            let params = json!({ "sessionId": "parent", "update": update });
            for event in self.router.route(&params, (self.parse)(&params)) {
                self.session = apply_harness_event(&self.session, &event);
            }
        }

        fn spawn_block(&self) -> &Block {
            self.session
                .blocks
                .iter()
                .find(|block| {
                    block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("spawn")
                })
                .expect("the spawn row")
        }
    }

    fn spawn(input: Value) -> Value {
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "spawn",
            "kind": "other",
            "title": "Task",
            "status": "in_progress",
            "rawInput": input,
        })
    }

    fn child_read(meta: &Value) -> Value {
        json!({
            "sessionUpdate": "tool_call",
            "_meta": meta,
            "toolCallId": "read",
            "kind": "read",
            "title": "Read auth.ts",
            "status": "in_progress",
        })
    }

    #[test]
    #[ignore = "fx and grok still call the stand-in acp_agent_info in providers/grok/deps.rs"]
    fn names_delegated_work_and_merges_child_tool_updates_without_leaking_prose() {
        for (harness, parse) in providers() {
            let mut run = Run::new(harness, parse);
            run.push(spawn(json!({
                "_toolName": "task", "description": "Check auth", "model": "review-model"
            })));
            let meta = json!({ "parentToolCallId": "spawn" });
            for text in ["Checking ", "auth."] {
                run.push(json!({
                    "sessionUpdate": "agent_message_chunk",
                    "_meta": meta,
                    "content": { "type": "text", "text": text },
                }));
            }
            run.push(child_read(&meta));
            // Sparse completions may omit the parent metadata entirely.
            run.push(json!({ "sessionUpdate": "tool_call_update", "toolCallId": "read", "status": "completed" }));
            run.push(json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "Parent answer" },
            }));

            let block = run.spawn_block();
            assert_eq!(
                block.tool.as_ref().unwrap().kind.as_deref(),
                Some("agent"),
                "{harness}"
            );
            let agent_run = block.agent_run.as_ref().unwrap();
            assert_eq!(
                agent_run.model.as_deref(),
                Some("review-model"),
                "{harness}"
            );
            assert_eq!(block.text, "Check auth", "{harness}");
            assert_eq!(agent_run.steps.len(), 2, "{harness}");
            assert_eq!(agent_run.steps[0].text, "Checking auth.", "{harness}");
            assert_eq!(
                agent_run.steps[1].status.as_deref(),
                Some("completed"),
                "{harness}"
            );
            let assistant: Vec<&str> = run
                .session
                .blocks
                .iter()
                .filter(|block| block.role == BlockRole::Assistant)
                .map(|block| block.text.as_str())
                .collect();
            assert_eq!(assistant, vec!["Parent answer"], "{harness}");
            assert!(
                !run.session.blocks.iter().any(|block| block
                    .tool
                    .as_ref()
                    .and_then(|tool| tool.call_id.as_deref())
                    == Some("read")),
                "{harness}"
            );
        }
    }

    #[test]
    fn keeps_a_failed_child_tools_output_on_its_step_where_it_can_be_read() {
        for (harness, parse) in providers() {
            let mut run = Run::new(harness, parse);
            run.push(spawn(
                json!({ "_toolName": "task", "description": "Check auth" }),
            ));
            let meta = json!({ "parentToolCallId": "spawn" });
            run.push(child_read(&meta));
            run.push(json!({
                "sessionUpdate": "tool_call_update",
                "_meta": meta,
                "toolCallId": "read",
                "status": "failed",
                "content": [{ "type": "text", "text": "File missing" }],
            }));
            let steps = &run.spawn_block().agent_run.as_ref().unwrap().steps;
            assert_eq!(steps.len(), 1, "{harness}");
            assert_eq!(steps[0].status.as_deref(), Some("failed"), "{harness}");
            assert_eq!(
                steps[0].detail.as_deref(),
                Some("File missing"),
                "{harness}"
            );
        }
    }

    #[test]
    fn leaves_a_settled_child_tools_result_off_its_step() {
        for (harness, parse) in providers() {
            let mut run = Run::new(harness, parse);
            run.push(spawn(
                json!({ "_toolName": "task", "description": "Check auth" }),
            ));
            let meta = json!({ "parentToolCallId": "spawn" });
            run.push(child_read(&meta));
            run.push(json!({
                "sessionUpdate": "tool_call_update",
                "_meta": meta,
                "toolCallId": "read",
                "status": "completed",
                "content": [{ "type": "text", "text": "export function auth() {}" }],
            }));
            let steps = &run.spawn_block().agent_run.as_ref().unwrap().steps;
            assert_eq!(steps.len(), 1, "{harness}");
            assert_eq!(steps[0].status.as_deref(), Some("completed"), "{harness}");
            assert_eq!(steps[0].detail, None, "{harness}");
        }
    }
}
