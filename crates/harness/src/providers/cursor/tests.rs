//! Ports of cursorLive.test.ts and cursorText.test.ts on a scripted child
//! backend.

use std::collections::HashMap;
use std::sync::Arc;

use monocode_core::block::BlockRole;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{HarnessEvent, SendTurnInput};
use monocode_core::reducer::apply_harness_event;
use monocode_core::session::Session;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::core::catalog::SharedCatalog;
use crate::core::registry::{EventSink, TextPromptInput};
use crate::core::task::{BoxFuture, SmolSpawner};

use super::fake::{Wire, fake_children, flush, wait_for};
use super::session::CursorSessions;
use super::store::{CursorStore, StoreReader, StoredCursorSubagentRun, StoredCursorToolCall};
use super::text::{TEXT_CHILD_ID, TextRunner};

const THREAD: &str = "cursor-live";

/// One `subagent_runs` call: session id, tool call ids, known revisions.
type StoreCall = (String, Vec<String>, HashMap<String, String>);

/// `stores` in cursorLive.test.ts.
#[derive(Default)]
struct FakeStore {
    runs: Mutex<Vec<StoredCursorSubagentRun>>,
    /// A read waits on this, then returns what it receives.
    hold: Mutex<Option<async_channel::Receiver<Vec<StoredCursorSubagentRun>>>>,
    calls: Mutex<Vec<StoreCall>>,
}

impl CursorStore for FakeStore {
    fn subagent_runs(
        &self,
        session_id: String,
        tool_call_ids: Vec<String>,
        known_revisions: HashMap<String, String>,
    ) -> BoxFuture<'static, anyhow::Result<Vec<StoredCursorSubagentRun>>> {
        self.calls
            .lock()
            .push((session_id, tool_call_ids, known_revisions));
        let hold = self.hold.lock().clone();
        let runs = self.runs.lock().clone();
        Box::pin(async move {
            match hold {
                Some(hold) => Ok(hold.recv().await.unwrap_or_default()),
                None => Ok(runs),
            }
        })
    }

    fn tool_calls(
        &self,
        _: String,
        _: Vec<String>,
    ) -> BoxFuture<'static, anyhow::Result<Vec<StoredCursorToolCall>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct Harness {
    sessions: Arc<CursorSessions>,
    wire: Arc<Wire>,
    store: Arc<FakeStore>,
}

fn harness() -> Harness {
    let (children, wire) = fake_children("/fake/cursor-agent", None);
    let store = Arc::new(FakeStore::default());
    let sessions = CursorSessions::new(
        children,
        Arc::new(SmolSpawner),
        SharedCatalog::new(),
        StoreReader::new(store.clone(), false),
        Default::default(),
    );
    Harness {
        sessions,
        wire,
        store,
    }
}

type Events = Arc<Mutex<Vec<HarnessEvent>>>;

fn recorder() -> (Events, EventSink) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    (events, Arc::new(move |event| sink.lock().push(event)))
}

fn turn_input(session_id: &str) -> SendTurnInput {
    serde_json::from_value(json!({
        "sessionId": session_id,
        "cwd": "/repo",
        "model": "cursor:composer-2.5",
        "modelSettings": {},
        "runtimeMode": "supervised",
        "text": "explore the codebase",
        "attachments": [],
    }))
    .unwrap()
}

struct Turn {
    events: Events,
    prompt_id: Value,
    task: smol::Task<anyhow::Result<()>>,
}

fn id_of(wire: &Wire, method: &str) -> Value {
    wire.outbound(method)
        .and_then(|message| message.get("id").cloned())
        .unwrap()
}

/// `startTurn`.
async fn start_turn(h: &Harness) -> Turn {
    let (events, sink) = recorder();
    let sessions = h.sessions.clone();
    let task =
        smol::spawn(async move { sessions.send_cursor_turn(turn_input(THREAD), sink).await });
    let wire = &h.wire;
    wait_for(|| wire.outbound("initialize").is_some(), "initialize").await;
    wire.reply(THREAD, &id_of(wire, "initialize"), json!({}));
    wait_for(|| wire.outbound("authenticate").is_some(), "authenticate").await;
    wire.reply(THREAD, &id_of(wire, "authenticate"), json!({}));
    wait_for(|| wire.outbound("session/new").is_some(), "session/new").await;
    wire.reply(
        THREAD,
        &id_of(wire, "session/new"),
        json!({
            "sessionId": "cursor_1",
            "configOptions": [{ "id": "model", "category": "model", "currentValue": "composer-2.5" }],
        }),
    );
    wait_for(
        || wire.outbound("session/prompt").is_some(),
        "session/prompt",
    )
    .await;
    Turn {
        events,
        prompt_id: id_of(wire, "session/prompt"),
        task,
    }
}

/// `emitAgentStart`.
fn emit_agent_start(wire: &Wire) {
    wire.notify(
        THREAD,
        "session/update",
        json!({
            "sessionId": "cursor_1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "call_agent",
                "title": "Task: Explore auth",
                "kind": "other",
                "status": "pending",
                "rawInput": { "_toolName": "task", "description": "Explore auth", "subagentType": "explore" },
            },
        }),
    );
}

fn as_json(event: &HarnessEvent) -> Value {
    serde_json::to_value(event).unwrap()
}

/// `toMatchObject`: every field in `expected` matches `actual`.
fn matches(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected
            .iter()
            .all(|(key, value)| actual.get(key).is_some_and(|found| matches(found, value))),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(a, e)| matches(a, e))
        }
        _ => actual == expected,
    }
}

fn assert_matches(actual: Option<Value>, expected: Value) {
    let actual = actual.expect("an event");
    assert!(
        matches(&actual, &expected),
        "expected {expected} in {actual}"
    );
}

/// `agentEvents`: tool updates for the parent agent row.
fn agent_events(events: &Events) -> Vec<Value> {
    events
        .lock()
        .iter()
        .map(as_json)
        .filter(|event| event["type"] == "tool.updated" && event["callId"] == "call_agent")
        .collect()
}

fn of_type(events: &Events, kind: &str) -> Vec<Value> {
    events
        .lock()
        .iter()
        .map(as_json)
        .filter(|event| event["type"] == kind)
        .collect()
}

fn last(events: &Events) -> Option<Value> {
    events.lock().last().map(as_json)
}

fn run(value: Value) -> StoredCursorSubagentRun {
    serde_json::from_value(value).unwrap()
}

async fn finish(h: &Harness, turn: Turn) -> Events {
    h.wire
        .reply(THREAD, &turn.prompt_id, json!({ "stopReason": "end_turn" }));
    turn.task.await.unwrap();
    turn.events
}

async fn teardown(h: &Harness) {
    let _ = h.sessions.stop_cursor_session(THREAD).await;
}

#[test]
fn recovers_native_child_steps_without_parent_attributed_acp_events_and_enriches_foreground_names()
{
    smol::block_on(async {
        let h = harness();
        *h.store.runs.lock() = vec![run(json!({
            "agentId": "child_1",
            "toolCallId": "call_agent",
            "revision": "12",
            "prompt": "Perform a read-only, defect-first code review of ACP routing in /repo.",
            "steps": [
                { "id": "child_1:blob:0", "kind": "message", "text": "Checking routing." },
                {
                    "id": "child_1:tool:read", "kind": "tool", "text": "", "toolName": "Read",
                    "args": { "path": "/repo/acp.ts" }, "status": "completed",
                    "output": "export const route = true;"
                }
            ]
        }))];
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call", "toolCallId": "call_agent", "title": "Task: Subagent task",
                "kind": "other", "status": "pending", "rawInput": { "_toolName": "task" }
            } }),
        );
        wait_for(|| !agent_events(&events).is_empty(), "agent row").await;
        assert_matches(
            agent_events(&events).first().cloned(),
            json!({ "title": "Subagent" }),
        );
        wait_for(
            || !of_type(&events, "agent.step").is_empty(),
            "stored child steps",
        )
        .await;
        {
            let calls = h.store.calls.lock();
            assert_eq!(calls[0].0, "cursor_1");
            assert_eq!(calls[0].1, vec!["call_agent".to_string()]);
        }
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "call_agent", "status": "completed"
            } }),
        );
        h.wire.request(
            THREAD,
            101,
            "cursor/task",
            json!({ "toolCallId": "call_agent", "agentId": "child_1", "description": "Review ACP routing", "durationMs": 25 }),
        );
        wait_for(|| h.wire.response(101).is_some(), "cursor/task response").await;
        let events = finish(&h, turn).await;
        let session = events.lock().iter().fold(
            Session::blank("s", HarnessId::Cursor, "cursor:composer-2.5", "/repo"),
            |session, event| apply_harness_event(&session, event),
        );
        let row = session
            .blocks
            .iter()
            .find(|block| {
                block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("call_agent")
            })
            .unwrap();
        assert_eq!(row.text, "Review ACP routing");
        assert_eq!(
            row.tool.as_ref().unwrap().status.as_deref(),
            Some("completed")
        );
        let agent_run = row.agent_run.as_ref().unwrap();
        assert_eq!(agent_run.name, "Review ACP routing");
        assert_eq!(agent_run.steps.len(), 2);
        assert_matches(
            Some(serde_json::to_value(&agent_run.steps[1]).unwrap()),
            json!({ "toolKind": "read", "status": "completed", "preview": { "path": "/repo/acp.ts" } }),
        );
        assert!(
            session
                .blocks
                .iter()
                .all(|block| block.role != BlockRole::Assistant)
        );
        let mut revisions = HashMap::new();
        revisions.insert("child_1".to_string(), "12".to_string());
        let last_revisions = h.store.calls.lock().last().unwrap().2.clone();
        assert_eq!(last_revisions, revisions);
        teardown(&h).await;
    });
}

#[test]
fn keeps_a_task_description_received_before_its_placeholder_tool_row() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        h.wire.notify(
            THREAD,
            "cursor/task",
            json!({ "toolCallId": "call_agent", "description": "Review UI events", "agentId": "child_1" }),
        );
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "update": {
                "sessionUpdate": "tool_call", "toolCallId": "call_agent", "title": "Task: Subagent task",
                "rawInput": { "_toolName": "task" }, "status": "in_progress"
            } }),
        );
        wait_for(|| agent_events(&events).len() >= 2, "both agent updates").await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "title": "Review UI events" }),
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

fn ignores_an_in_flight_child_read_after(action: &str) {
    smol::block_on(async {
        let h = harness();
        let (release, hold) = async_channel::bounded(1);
        *h.store.hold.lock() = Some(hold);
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        wait_for(|| !h.store.calls.lock().is_empty(), "child read").await;
        match action {
            "cancel" => h.sessions.cancel_cursor_turn(THREAD).await.unwrap(),
            "stop" => h.sessions.stop_cursor_session(THREAD).await.unwrap(),
            _ => h.wire.exit(THREAD, 1),
        }
        flush().await;
        let count = events.lock().len();
        release
            .send(vec![run(json!({
                "agentId": "child_1", "toolCallId": "call_agent", "revision": "1",
                "steps": [{ "id": "late", "kind": "message", "text": "Late result" }]
            }))])
            .await
            .unwrap();
        // A stopped ACP client rejects its prompt; cancellation still receives a reply.
        if action == "cancel" {
            h.wire.reply(
                THREAD,
                &turn.prompt_id,
                json!({ "stopReason": "cancelled" }),
            );
        }
        let _ = turn.task.await;
        flush().await;
        let late: Vec<Value> = events.lock()[count..].iter().map(as_json).collect();
        assert!(
            !late
                .iter()
                .any(|event| event["type"] == "agent.step" || event["type"] == "message.completed"),
            "late events after {action}: {late:?}"
        );
        teardown(&h).await;
    });
}

#[test]
fn ignores_an_in_flight_child_read_after_cancel() {
    ignores_an_in_flight_child_read_after("cancel");
}

#[test]
fn ignores_an_in_flight_child_read_after_stop() {
    ignores_an_in_flight_child_read_after("stop");
}

#[test]
fn ignores_an_in_flight_child_read_after_exit() {
    ignores_an_in_flight_child_read_after("exit");
}

#[test]
fn routes_attributed_child_work_to_its_row_and_leaves_parent_narration_separate() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        let meta = json!({ "parentToolCallId": "call_agent" });
        let update = |update: Value| json!({ "sessionId": "cursor_1", "update": update });
        h.wire.notify(
            THREAD,
            "session/update",
            update(json!({ "sessionUpdate": "agent_message_chunk", "_meta": meta, "content": { "type": "text", "text": "Child narration" } })),
        );
        h.wire.notify(
            THREAD,
            "session/update",
            update(json!({ "sessionUpdate": "tool_call", "_meta": meta, "toolCallId": "child_read", "title": "Read auth.ts", "kind": "read", "status": "in_progress" })),
        );
        h.wire.notify(
            THREAD,
            "session/update",
            update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": "child_read", "status": "completed" })),
        );
        h.wire.notify(
            THREAD,
            "session/update",
            update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Parent narration" } })),
        );
        wait_for(
            || !of_type(&events, "message.delta").is_empty(),
            "parent narration",
        )
        .await;
        assert_eq!(
            of_type(&events, "message.delta"),
            vec![json!({ "type": "message.delta", "text": "Parent narration" })]
        );
        let steps = of_type(&events, "agent.step");
        assert_eq!(steps.len(), 3, "{steps:?}");
        assert_matches(
            steps.first().cloned(),
            json!({ "callId": "call_agent", "kind": "message", "text": "Child narration" }),
        );
        assert_matches(
            steps.get(1).cloned(),
            json!({ "callId": "call_agent", "stepId": "tool:child_read", "status": "in_progress" }),
        );
        assert_matches(
            steps.get(2).cloned(),
            json!({ "callId": "call_agent", "stepId": "tool:child_read", "status": "completed" }),
        );
        assert!(
            !events
                .lock()
                .iter()
                .map(as_json)
                .any(|event| event["type"] == "tool.updated" && event["callId"] == "child_read")
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn keeps_a_failed_child_tools_error_text_on_its_step() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        let meta = json!({ "parentToolCallId": "call_agent" });
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call", "_meta": meta, "toolCallId": "child_bash",
                "title": "npm test", "kind": "execute", "status": "in_progress"
            } }),
        );
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "child_bash", "status": "failed",
                "content": [{ "type": "text", "text": "Tests failed: assertion error" }]
            } }),
        );
        wait_for(|| of_type(&events, "agent.step").len() >= 2, "failed step").await;
        assert_matches(
            of_type(&events, "agent.step").last().cloned(),
            json!({ "callId": "call_agent", "stepId": "tool:child_bash", "status": "failed", "detail": "Tests failed: assertion error" }),
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn does_not_pass_a_child_tools_command_off_as_its_error_output() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        let meta = json!({ "parentToolCallId": "call_agent" });
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call", "_meta": meta, "toolCallId": "child_bash", "title": "npm test",
                "kind": "execute", "rawInput": { "command": "npm test" }, "status": "in_progress"
            } }),
        );
        // Failed with nothing to say, but the update still carries the call:
        // the command is the title, not the reason it failed.
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "child_bash", "status": "failed",
                "rawInput": { "command": "npm test" }
            } }),
        );
        wait_for(|| of_type(&events, "agent.step").len() >= 2, "failed step").await;
        let step = of_type(&events, "agent.step").last().cloned();
        assert_matches(
            step.clone(),
            json!({ "callId": "call_agent", "stepId": "tool:child_bash", "status": "failed" }),
        );
        assert!(step.unwrap().get("detail").is_none());
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn emits_request_shaped_cursor_todo_updates_as_structured_task_lists() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        h.wire.request(
            THREAD,
            71,
            "cursor/update_todos",
            json!({ "toolCallId": "call_todos", "todos": [
                { "content": "Inspect", "status": "completed" },
                { "content": "Implement", "status": "in_progress" }
            ] }),
        );
        wait_for(
            || h.wire.response(71).is_some(),
            "cursor/update_todos response",
        )
        .await;
        assert_eq!(
            last(&events),
            Some(json!({ "type": "tasks.updated", "items": [
                { "text": "Inspect", "status": "completed" },
                { "text": "Implement", "status": "in_progress" }
            ] }))
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn keeps_notification_shaped_cursor_todo_updates_compatible() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        let before = events.lock().len();
        h.wire.notify(
            THREAD,
            "_cursor/update_todos",
            json!({ "todos": [{ "content": "Inspect", "status": "pending" }] }),
        );
        wait_for(|| events.lock().len() > before, "task list").await;
        assert_eq!(
            last(&events),
            Some(
                json!({ "type": "tasks.updated", "items": [{ "text": "Inspect", "status": "pending" }] })
            )
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn preserves_cursors_partial_update_signal_and_task_identities() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        h.wire.request(
            THREAD,
            72,
            "cursor/update_todos",
            json!({ "toolCallId": "call_todos", "merge": true, "todos": [
                { "id": "2", "content": "Implementing the fix", "status": "completed" }
            ] }),
        );
        wait_for(
            || h.wire.response(72).is_some(),
            "partial cursor/update_todos response",
        )
        .await;
        assert_eq!(
            last(&events),
            Some(json!({ "type": "tasks.updated", "merge": true, "items": [
                { "id": "2", "text": "Implementing the fix", "status": "completed" }
            ] }))
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn marks_cursors_redundant_todo_tool_call_as_internal_task_activity() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        let before = events.lock().len();
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call", "toolCallId": "call_todos", "title": "Update TODOs",
                "kind": "other", "status": "pending", "rawInput": { "_toolName": "updateTodos", "todos": [] }
            } }),
        );
        wait_for(|| events.lock().len() > before, "todo row").await;
        assert_matches(
            last(&events),
            json!({ "type": "tool.updated", "callId": "call_todos", "kind": "tasks" }),
        );
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "call_todos", "kind": "other", "status": "completed"
            } }),
        );
        wait_for(|| events.lock().len() > before + 1, "todo update").await;
        assert_matches(
            last(&events),
            json!({ "type": "tool.updated", "callId": "call_todos", "kind": "tasks", "status": "completed" }),
        );
        finish(&h, turn).await;
        teardown(&h).await;
    });
}

#[test]
fn keeps_an_acp_background_task_active_until_the_prompt_completes() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "call_agent", "status": "completed",
                "rawOutput": { "isBackground": true }
            } }),
        );
        wait_for(|| agent_events(&events).len() >= 2, "background update").await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "type": "tool.updated", "title": "Explore auth", "kind": "agent", "status": "in_progress" }),
        );
        flush().await;
        assert!(!turn.task.is_finished());
        assert!(of_type(&events, "message.completed").is_empty());
        let events = finish(&h, turn).await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "type": "tool.updated", "kind": "agent", "status": "completed" }),
        );
        assert!(!of_type(&events, "message.completed").is_empty());
        teardown(&h).await;
    });
}

#[test]
fn uses_cursors_task_request_when_raw_acp_output_omits_the_background_flag() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "call_agent", "status": "completed"
            } }),
        );
        h.wire.request(
            THREAD,
            99,
            "cursor/task",
            json!({ "toolCallId": "call_agent", "description": "Explore auth", "subagentType": "explore", "agentId": "child_1" }),
        );
        wait_for(|| h.wire.response(99).is_some(), "cursor/task response").await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "type": "tool.updated", "title": "Explore auth", "kind": "agent", "status": "in_progress", "detail": "explore subagent" }),
        );
        let events = finish(&h, turn).await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "status": "completed" }),
        );
        teardown(&h).await;
    });
}

#[test]
fn does_not_reopen_a_foreground_task_after_cursor_reports_its_duration() {
    smol::block_on(async {
        let h = harness();
        let turn = start_turn(&h).await;
        let events = turn.events.clone();
        emit_agent_start(&h.wire);
        h.wire.notify(
            THREAD,
            "session/update",
            json!({ "sessionId": "cursor_1", "update": {
                "sessionUpdate": "tool_call_update", "toolCallId": "call_agent", "status": "completed",
                "rawOutput": { "isBackground": false, "durationMs": 25 }
            } }),
        );
        h.wire.request(
            THREAD,
            100,
            "cursor/task",
            json!({ "toolCallId": "call_agent", "description": "Explore auth", "subagentType": "explore", "agentId": "child_1", "durationMs": 25 }),
        );
        wait_for(|| h.wire.response(100).is_some(), "cursor/task response").await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "type": "tool.updated", "kind": "agent", "status": "completed" }),
        );
        let events = finish(&h, turn).await;
        assert_matches(
            agent_events(&events).last().cloned(),
            json!({ "status": "completed" }),
        );
        teardown(&h).await;
    });
}

#[test]
fn forwards_cursor_text_deltas_without_duplicating_snapshots() {
    smol::block_on(async {
        let (children, wire) = fake_children("/fake/cursor-agent", None);
        let runner = TextRunner::new(children, Arc::new(SmolSpawner));
        let (events, sink) = recorder();
        let task = smol::spawn({
            let runner = runner.clone();
            async move {
                runner
                    .run_cursor_text_prompt(TextPromptInput {
                        cwd: "/repo".into(),
                        prompt: "question".into(),
                        on_event: Some(sink),
                        ..Default::default()
                    })
                    .await
            }
        });
        let id = TEXT_CHILD_ID;
        for (method, result) in [
            ("initialize", json!({})),
            ("authenticate", json!({})),
            (
                "session/new",
                json!({ "sessionId": "cursor_text", "configOptions": [{ "id": "model", "category": "model" }] }),
            ),
            ("session/set_mode", json!({})),
            ("session/set_config_option", json!({})),
        ] {
            wait_for(|| wire.outbound(method).is_some(), method).await;
            wire.reply(id, &id_of(&wire, method), result);
        }
        wait_for(
            || wire.outbound("session/prompt").is_some(),
            "session/prompt",
        )
        .await;
        wire.notify(
            id,
            "session/update",
            json!({ "sessionId": "cursor_text", "update": { "sessionUpdate": "agent_message_chunk", "content": "Hel" } }),
        );
        wire.notify(
            id,
            "session/update",
            json!({ "sessionId": "cursor_text", "update": { "sessionUpdate": "agent_message", "content": "Hello" } }),
        );
        wire.reply(id, &id_of(&wire, "session/prompt"), json!({}));
        assert_eq!(task.await.unwrap(), "Hello");
        assert_eq!(
            events.lock().iter().map(as_json).collect::<Vec<_>>(),
            vec![
                json!({ "type": "message.delta", "text": "Hel" }),
                json!({ "type": "message.delta", "text": "lo" }),
            ]
        );
        runner.stop_cursor_text_prompt(None).await;
    });
}
