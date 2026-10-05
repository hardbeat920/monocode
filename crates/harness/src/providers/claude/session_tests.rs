//! Port of src/integrations/harness/providers/claude/claudeLive.test.ts.
//!
//! Despite its name, the TypeScript file never spawned Claude Code: it
//! replaced `../../core/child` with a mock and scripted the CLI's stream-json
//! lines. These tests do the same through a fake [`ClaudeChildIo`], so they
//! run by default. The real CLI is exercised by the `#[ignore]` test in
//! `live_tests.rs`.
//!
//! The TypeScript reduced events with `applyHarnessEvent` from a
//! `newSession("claude", "/repo")`; these use `monocode_core::reducer`. Its
//! checks on `groupTurnItems`, `foldableWork`, and `workSummaryLine`
//! (transcriptActivity.ts, not ported yet) are left out.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::BoxFuture;
use monocode_core::block::{
    AgentStepKind, BlockRole, ModelSettings, PlanStatus, TaskListItem, TaskListItemStatus,
    TaskListMeta, TurnIntent,
};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{
    ApprovalDecision, HarnessEvent, HarnessSessionInput, SendTurnInput,
};
use monocode_core::reducer::apply::apply_harness_event;
use monocode_core::session::Session;
use monocode_core::user_question::UserQuestionReply;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::core::child::ChildAccount;
use crate::core::registry::EventSink;
use crate::core::task::SmolSpawner;

use super::io::{ClaudeChildIo, ExitHandler, LineHandler};
use super::session::{ClaudeSessionOptions, ClaudeSessions};

/// The TypeScript's 15 s grace, shortened so the tests do not need fake timers.
const GRACE: Duration = Duration::from_millis(300);

#[derive(Default)]
struct FakeState {
    sent: Vec<String>,
    spawned: Vec<Vec<String>>,
    on_line: Option<LineHandler>,
    on_exit: Option<ExitHandler>,
    reject_writes: VecDeque<String>,
}

/// The `vi.mock("../../core/child")` of the TypeScript test.
#[derive(Default)]
struct FakeIo {
    state: Mutex<FakeState>,
}

impl ClaudeChildIo for FakeIo {
    fn resolve_claude_binary(&self) -> BoxFuture<'static, Result<String>> {
        async { Ok("/fake/claude".to_string()) }.boxed()
    }

    fn spawn_child(
        &self,
        _child_id: &str,
        _command: &str,
        args: Vec<String>,
        _cwd: &str,
        _account: Option<ChildAccount>,
    ) -> BoxFuture<'static, Result<()>> {
        self.state.lock().spawned.push(args);
        async { Ok(()) }.boxed()
    }

    fn watch_child(&self, _child_id: &str, on_line: LineHandler, on_exit: ExitHandler) {
        let mut state = self.state.lock();
        state.on_line = Some(on_line);
        state.on_exit = Some(on_exit);
    }

    fn unwatch_child(&self, _child_id: &str) {}

    fn write_child(&self, _child_id: &str, line: String) -> BoxFuture<'static, Result<()>> {
        let mut state = self.state.lock();
        if let Some(error) = state.reject_writes.pop_front() {
            return async move { Err(anyhow!(error)) }.boxed();
        }
        state.sent.push(line);
        async { Ok(()) }.boxed()
    }

    fn kill_child(&self, _child_id: &str) -> BoxFuture<'static, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn exec_child(
        &self,
        _command: &str,
        _args: Vec<String>,
        _cwd: Option<&str>,
    ) -> BoxFuture<'static, Result<String>> {
        async { Ok("2.1.300 (Claude Code)".to_string()) }.boxed()
    }

    fn home_dir(&self) -> BoxFuture<'static, Result<String>> {
        async { Ok("/home/user".to_string()) }.boxed()
    }
}

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<HarnessEvent>>>);

impl Events {
    fn sink(&self) -> EventSink {
        let events = self.0.clone();
        Arc::new(move |event| events.lock().push(event))
    }

    fn all(&self) -> Vec<HarnessEvent> {
        self.0.lock().clone()
    }

    fn any(&self, pred: impl Fn(&HarnessEvent) -> bool) -> bool {
        self.0.lock().iter().any(pred)
    }

    fn extend(&self, other: &Events) {
        let mut copied = other.all();
        self.0.lock().append(&mut copied);
    }

    /// `events.reduce(applyHarnessEvent, newSession("claude", "/repo"))`.
    fn reduce(&self) -> Session {
        let mut session = Session::blank("s", HarnessId::Claude, "claude:sonnet-5", "/repo");
        for event in self.all() {
            session = apply_harness_event(&session, &event);
        }
        session
    }

    fn background_updates(&self) -> Vec<Vec<String>> {
        self.all()
            .into_iter()
            .filter_map(|event| match event {
                HarnessEvent::BackgroundUpdated { tasks } => Some(tasks),
                _ => None,
            })
            .collect()
    }

    fn last_task_items(&self) -> Option<Vec<TaskListItem>> {
        self.reduce()
            .blocks
            .iter()
            .rev()
            .find(|block| block.role == BlockRole::Tasks)
            .and_then(|block| block.task_list.as_ref())
            .map(|list| list.items.clone())
    }
}

#[derive(Default)]
struct TurnOptions {
    runtime_mode: Option<RuntimeMode>,
    intent: Option<TurnIntent>,
    provider_account_id: Option<String>,
    model: Option<&'static str>,
    text: Option<&'static str>,
}

type Turn = smol::Task<Result<()>>;

struct Harness {
    io: Arc<FakeIo>,
    sessions: ClaudeSessions,
}

impl Harness {
    fn new() -> Self {
        let io = Arc::new(FakeIo::default());
        let options = ClaudeSessionOptions {
            init_timeout: Duration::from_secs(2),
            resume_grace: GRACE,
            ..Default::default()
        };
        let sessions = ClaudeSessions::new(io.clone(), Arc::new(SmolSpawner), options);
        Self { io, sessions }
    }

    fn emit(&self, rec: Value) {
        let on_line = self
            .io
            .state
            .lock()
            .on_line
            .clone()
            .expect("no watched child");
        on_line(rec.to_string());
    }

    fn exit(&self, code: Option<i64>) {
        let on_exit = self
            .io
            .state
            .lock()
            .on_exit
            .clone()
            .expect("no watched child");
        on_exit(code);
    }

    fn parse(&self) -> Vec<Value> {
        self.io
            .state
            .lock()
            .sent
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn clear_sent(&self) {
        self.io.state.lock().sent.clear();
    }

    fn spawned(&self) -> Vec<Vec<String>> {
        self.io.state.lock().spawned.clone()
    }

    fn reject_next_write(&self, error: &str) {
        self.io.state.lock().reject_writes.push_back(error.into());
    }

    fn user_count(&self) -> usize {
        self.parse().iter().filter(|m| m["type"] == "user").count()
    }

    fn response_for(&self, request_id: &str) -> Option<Value> {
        self.parse()
            .into_iter()
            .find(|m| m["response"]["request_id"] == request_id)
    }

    fn wait_for(&self, pred: impl Fn() -> bool, label: &str) {
        for _ in 0..400 {
            if pred() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for {label}; sent={:?}", self.parse());
    }

    fn send(&self, session_id: &str, options: TurnOptions) -> (Events, Turn) {
        let events = Events::default();
        let turn = self.send_with(session_id, options, events.sink());
        (events, turn)
    }

    fn send_with(&self, session_id: &str, options: TurnOptions, sink: EventSink) -> Turn {
        let model_settings = ModelSettings::new();
        let input = SendTurnInput {
            session: HarnessSessionInput {
                session_id: session_id.into(),
                cwd: "/repo".into(),
                model: options.model.unwrap_or("claude:claude-sonnet-5").into(),
                model_settings: Some(model_settings),
                provider_account_id: options.provider_account_id,
                runtime_mode: options.runtime_mode.unwrap_or(RuntimeMode::Supervised),
                intent: options.intent,
                controls_agents: None,
                app_access: None,
            },
            text: options.text.unwrap_or("explore the codebase").into(),
            attachments: Some(Vec::new()),
        };
        let sessions = self.sessions.clone();
        smol::spawn(async move { sessions.send_turn(input, sink).await })
    }

    /// `startTurn`: a first turn through the initialize handshake.
    fn start_turn(&self, session_id: &str, options: TurnOptions) -> (Events, Turn) {
        let (events, turn) = self.send(session_id, options);
        self.wait_for(
            || {
                self.parse()
                    .iter()
                    .any(|m| m["request"]["subtype"] == "initialize")
            },
            "initialize",
        );
        self.emit(json!({ "type": "system", "subtype": "init", "session_id": "sess_1" }));
        self.emit(json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": "monocode_1" },
        }));
        self.wait_for(|| self.user_count() > 0, "user prompt");
        (events, turn)
    }

    /// `restartedTurn`: a later turn that must launch a new Claude process.
    fn restarted_turn(&self, options: TurnOptions, provider_session_id: &str) -> (Events, Turn) {
        let spawn_count = self.spawned().len();
        let user_count = self.user_count();
        let (events, turn) = self.send(
            "s1",
            TurnOptions {
                text: Some("finish it"),
                ..options
            },
        );
        self.wait_for(
            || self.spawned().len() == spawn_count + 1,
            "replacement Claude process",
        );
        self.emit(
            json!({ "type": "system", "subtype": "init", "session_id": provider_session_id }),
        );
        self.wait_for(|| self.user_count() > user_count, "follow-up prompt");
        (events, turn)
    }

    /// `emitFollowUpTurn`: what Claude streams when a finished task wakes it
    /// for another turn.
    fn emit_follow_up_turn(&self, text: &str) {
        self.emit(json!({ "type": "system", "subtype": "init", "session_id": "sess_1" }));
        self.emit(json!({
            "type": "stream_event",
            "session_id": "sess_1",
            "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } },
        }));
        self.emit(json!({
            "type": "assistant",
            "session_id": "sess_1",
            "message": { "content": [{ "type": "text", "text": text }] },
        }));
        self.result("sess_1");
    }

    fn result(&self, session_id: &str) {
        self.emit(json!({ "type": "result", "subtype": "success", "session_id": session_id }));
    }

    /// `emitInlineSubagent`: a subagent Claude ran inline; its report comes
    /// back on the parent's stream.
    fn emit_inline_subagent(&self) {
        self.emit(json!({
            "type": "assistant",
            "session_id": "sess_1",
            "message": { "content": [{
                "type": "tool_use",
                "id": "toolu_agent",
                "name": "Task",
                "input": { "description": "Explore the auth module", "subagent_type": "explore" },
            }] },
        }));
        self.emit(json!({
            "type": "system",
            "subtype": "task_started",
            "task_id": "t1",
            "tool_use_id": "toolu_agent",
            "description": "Explore the auth module",
            "task_type": "local_agent",
        }));
        self.emit(json!({
            "type": "user",
            "session_id": "sess_1",
            "message": { "content": [{
                "type": "tool_result", "tool_use_id": "toolu_agent", "content": "Auth lives in src/auth.",
            }] },
        }));
    }

    fn emit_background_bash_launch(&self, task_id: &str) {
        self.emit(json!({
            "type": "assistant",
            "session_id": "sess_1",
            "message": { "content": [{
                "type": "tool_use",
                "id": "toolu_bash",
                "name": "Bash",
                "input": { "command": "sleep 30 && echo done", "run_in_background": true },
            }] },
        }));
        self.emit(json!({
            "type": "system",
            "subtype": "background_tasks_changed",
            "tasks": [{ "task_id": task_id, "task_type": "local_bash", "description": "Wait 30 seconds then print done" }],
        }));
        self.emit(json!({
            "type": "system",
            "subtype": "task_started",
            "task_id": task_id,
            "tool_use_id": "toolu_bash",
            "description": "Wait 30 seconds then print done",
            "task_type": "local_bash",
        }));
        self.emit(json!({
            "type": "user",
            "session_id": "sess_1",
            "message": { "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_bash",
                "content": format!("Command running in background with ID: {task_id}"),
            }] },
        }));
    }

    fn emit_background_bash(&self, task_id: &str) {
        self.emit_background_bash_launch(task_id);
        self.emit(json!({
            "type": "stream_event",
            "session_id": "sess_1",
            "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "waiting" } },
        }));
        self.emit(json!({
            "type": "assistant",
            "session_id": "sess_1",
            "message": { "content": [{ "type": "text", "text": "waiting" }] },
        }));
        self.result("sess_1");
    }

    fn emit_bash_finished(&self, task_id: &str) {
        self.emit(json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] }));
        self.emit(json!({
            "type": "system", "subtype": "task_updated", "task_id": task_id, "patch": { "status": "completed" },
        }));
        self.emit(json!({
            "type": "system",
            "subtype": "task_notification",
            "task_id": task_id,
            "tool_use_id": "toolu_bash",
            "status": "completed",
            "summary": "Background command \"sleep 30 && echo done\" completed (exit code 0)",
        }));
    }

    fn emit_task_tool(
        &self,
        id: &str,
        name: &str,
        input: Value,
        result: &str,
        provider_session_id: &str,
    ) {
        self.emit(json!({
            "type": "assistant",
            "session_id": provider_session_id,
            "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] },
        }));
        self.emit(json!({
            "type": "user",
            "session_id": provider_session_id,
            "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": result }] },
        }));
    }
}

/// `await turn`, with a deadline so a hung turn fails the test.
fn finish(turn: Turn) -> Result<()> {
    smol::block_on(async {
        crate::core::task::timeout(Duration::from_secs(5), turn)
            .await
            .expect("turn did not settle")
    })
}

fn settles_within(turn: &Turn, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if turn.is_finished() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    turn.is_finished()
}

fn item(id: &str, text: &str, status: TaskListItemStatus) -> TaskListItem {
    TaskListItem {
        id: Some(id.into()),
        text: text.into(),
        status,
        extra: Default::default(),
    }
}

fn contains_all(args: &[String], items: &[&str]) -> bool {
    items.iter().all(|item| args.iter().any(|arg| arg == item))
}

fn tasks_updates(events: &Events) -> Vec<HarnessEvent> {
    events
        .all()
        .into_iter()
        .filter(|event| matches!(event, HarnessEvent::TasksUpdated { .. }))
        .collect()
}

// describe("claude streamed tool inputs")

#[test]
fn replaces_an_empty_shell_row_with_the_complete_assistant_tool_input() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit(json!({
        "type": "stream_event",
        "session_id": "sess_1",
        "event": {
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "tool_use", "id": "toolu_shell", "name": "Bash", "input": {} },
        },
    }));
    h.emit(json!({
        "type": "stream_event",
        "session_id": "sess_1",
        "event": {
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": "{\"command\":\"git status" },
        },
    }));
    h.emit(json!({
        "type": "assistant",
        "session_id": "sess_1",
        "message": { "content": [{
            "type": "tool_use",
            "id": "toolu_shell",
            "name": "Bash",
            "input": { "command": "git status --short", "description": "Check changes" },
        }] },
    }));
    h.emit(json!({
        "type": "user",
        "session_id": "sess_1",
        "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_shell", "content": "clean" }] },
    }));
    h.result("sess_1");
    finish(turn).unwrap();

    assert!(events.any(|event| matches!(
        event,
        HarnessEvent::ToolUpdated { call_id, title: Some(title), status: Some(status), .. }
            if call_id == "toolu_shell" && title == "git status --short" && status == "pending"
    )));
    let session = events.reduce();
    let tool = session
        .blocks
        .iter()
        .find(|block| {
            block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("toolu_shell")
        })
        .unwrap();
    assert_eq!(tool.text, "git status --short");
    assert_eq!(
        tool.tool.as_ref().unwrap().status.as_deref(),
        Some("completed")
    );
}

// describe("claude task tools")

#[test]
fn builds_the_task_list_from_task_create_and_task_update_not_subagent_rows() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests", "description": "Cover the parser" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_c2",
        "TaskCreate",
        json!({ "subject": "Ship it", "description": "Open the PR" }),
        "Task #2 created successfully: Ship it",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "in_progress" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_u2",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    let session = events.reduce();
    let lists: Vec<_> = session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::Tasks)
        .collect();
    assert_eq!(lists.len(), 1);
    assert_eq!(
        lists[0].task_list.as_ref().unwrap().items,
        [
            item("1", "Write tests", TaskListItemStatus::Completed),
            item("2", "Ship it", TaskListItemStatus::Pending),
        ]
    );
    assert!(!session.blocks.iter().any(|block| {
        block.tool.as_ref().and_then(|tool| tool.kind.as_deref()) == Some("agent")
            || block.agent_run.is_some()
    }));
}

#[test]
fn keeps_earlier_tasks_updatable_after_a_restart_resumes_the_conversation() {
    let h = Harness::new();
    let (first, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests", "description": "Cover the parser" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    let events = Events::default();
    events.extend(&first);
    let (second, turn) = h.restarted_turn(
        TurnOptions {
            model: Some("claude:opus-5"),
            ..Default::default()
        },
        "sess_1",
    );
    assert!(contains_all(&h.spawned()[1], &["--resume", "sess_1"]));
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    events.extend(&second);

    assert_eq!(
        events.last_task_items(),
        Some(vec![item(
            "1",
            "Write tests",
            TaskListItemStatus::Completed
        )])
    );
}

#[test]
fn shows_a_task_update_subject_rename_in_the_panel() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests", "description": "Cover the parser" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "subject": "Write parser tests", "status": "in_progress" }),
        "Updated task #1 subject, status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    assert_eq!(
        events.last_task_items(),
        Some(vec![item(
            "1",
            "Write parser tests",
            TaskListItemStatus::InProgress
        )])
    );
}

#[test]
fn keeps_earlier_tasks_across_a_plan_to_build_restart() {
    let h = Harness::new();
    let (first, turn) = h.start_turn(
        "s1",
        TurnOptions {
            intent: Some(TurnIntent::Plan),
            ..Default::default()
        },
    );
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    let events = Events::default();
    events.extend(&first);
    let (second, turn) = h.restarted_turn(
        TurnOptions {
            intent: Some(TurnIntent::Build),
            ..Default::default()
        },
        "sess_1",
    );
    assert!(contains_all(&h.spawned()[1], &["--resume", "sess_1"]));
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    events.extend(&second);

    assert_eq!(
        events.last_task_items(),
        Some(vec![item(
            "1",
            "Write tests",
            TaskListItemStatus::Completed
        )])
    );
}

#[test]
fn keeps_earlier_tasks_after_the_claude_child_exits() {
    let h = Harness::new();
    let (first, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    h.exit(Some(1));

    let events = Events::default();
    events.extend(&first);
    let (second, turn) = h.restarted_turn(TurnOptions::default(), "sess_1");
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    events.extend(&second);

    assert_eq!(
        events.last_task_items(),
        Some(vec![item(
            "1",
            "Write tests",
            TaskListItemStatus::Completed
        )])
    );
}

fn task_lists(session: &Session) -> Vec<TaskListMeta> {
    session
        .blocks
        .iter()
        .filter_map(|block| block.task_list.clone())
        .collect()
}

#[test]
fn rehydrates_tasks_from_the_persisted_panel_after_an_app_restart() {
    let h = Harness::new();
    let (first, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_c2",
        "TaskCreate",
        json!({ "subject": "Ship it" }),
        "Task #2 created successfully: Ship it",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    let restored = first.reduce();

    // App restart: all module state is gone; only the saved transcript remains.
    smol::block_on(h.sessions.stop_session("s1")).unwrap();
    h.sessions.reset_for_tests();
    h.sessions.bind_session("s1", "sess_1", "/repo", None);
    h.sessions.restore_task_lists("s1", &task_lists(&restored));

    let events = Events::default();
    events.extend(&first);
    let (second, turn) = h.restarted_turn(TurnOptions::default(), "sess_1");
    assert!(contains_all(
        h.spawned().last().unwrap(),
        &["--resume", "sess_1"]
    ));
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_c3",
        "TaskCreate",
        json!({ "subject": "Tag release" }),
        "Task #3 created successfully: Tag release",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    events.extend(&second);

    assert_eq!(
        events.last_task_items(),
        Some(vec![
            item("1", "Write tests", TaskListItemStatus::Completed),
            item("2", "Ship it", TaskListItemStatus::Pending),
            item("3", "Tag release", TaskListItemStatus::Pending),
        ])
    );
}

/// Conversation A: tasks #1 and #2 in sess_1, reduced like the saved transcript.
fn conversation_with_tasks(h: &Harness) -> Session {
    let (first, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.emit_task_tool(
        "toolu_c2",
        "TaskCreate",
        json!({ "subject": "Ship it" }),
        "Task #2 created successfully: Ship it",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();
    first.reduce()
}

/// Conversation B updates A's #1, then creates its own #1.
fn emit_conversation_b(h: &Harness) {
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_2",
    );
    h.emit_task_tool(
        "toolu_c3",
        "TaskCreate",
        json!({ "subject": "Fresh task" }),
        "Task #1 created successfully: Fresh task",
        "sess_2",
    );
    h.result("sess_2");
}

fn is_fresh_sess_2_list(event: &HarnessEvent) -> bool {
    matches!(
        event,
        HarnessEvent::TasksUpdated { provider_session_id: Some(id), items, .. }
            if id == "sess_2" && items == &[item("1", "Fresh task", TaskListItemStatus::Pending)]
    )
}

#[test]
fn rehydrates_the_bound_conversations_panel_past_a_later_conversations_panel() {
    let h = Harness::new();
    let restored = conversation_with_tasks(&h);
    let mut lists = task_lists(&restored);
    lists.push(TaskListMeta {
        key: Some("claude-tasks".into()),
        provider_session_id: Some("sess_2".into()),
        explanation: None,
        items: vec![item("1", "Other conversation", TaskListItemStatus::Pending)],
        extra: Default::default(),
    });

    smol::block_on(h.sessions.stop_session("s1")).unwrap();
    h.sessions.reset_for_tests();
    h.sessions.bind_session("s1", "sess_1", "/repo", None);
    h.sessions.restore_task_lists("s1", &lists);

    let (events, turn) = h.restarted_turn(TurnOptions::default(), "sess_1");
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "2", "status": "completed" }),
        "Updated task #2 status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    assert_eq!(
        events.last_task_items(),
        Some(vec![
            item("1", "Write tests", TaskListItemStatus::Pending),
            item("2", "Ship it", TaskListItemStatus::Completed),
        ])
    );
}

#[test]
fn does_not_carry_tasks_to_another_conversation_bound_to_the_same_thread() {
    let h = Harness::new();
    let restored = conversation_with_tasks(&h);
    let first_list = restored
        .blocks
        .iter()
        .find(|block| block.role == BlockRole::Tasks)
        .and_then(|block| block.task_list.as_ref())
        .unwrap();
    assert_eq!(first_list.provider_session_id.as_deref(), Some("sess_1"));

    smol::block_on(h.sessions.stop_session("s1")).unwrap();
    h.sessions.bind_session("s1", "sess_2", "/repo", None);
    h.sessions.restore_task_lists("s1", &task_lists(&restored));

    let (events, turn) = h.restarted_turn(TurnOptions::default(), "sess_2");
    assert!(contains_all(
        h.spawned().last().unwrap(),
        &["--resume", "sess_2"]
    ));
    emit_conversation_b(&h);
    finish(turn).unwrap();

    let updates = tasks_updates(&events);
    assert_eq!(updates.len(), 1);
    assert!(is_fresh_sess_2_list(&updates[0]));
}

#[test]
fn starts_a_clean_task_map_when_claude_reports_a_different_conversation() {
    let h = Harness::new();
    conversation_with_tasks(&h);

    let (events, turn) = h.send(
        "s1",
        TurnOptions {
            text: Some("keep going"),
            ..Default::default()
        },
    );
    h.wait_for(|| h.user_count() > 1, "follow-up prompt");
    emit_conversation_b(&h);
    finish(turn).unwrap();

    assert!(is_fresh_sess_2_list(tasks_updates(&events).last().unwrap()));
}

#[test]
fn drops_the_task_map_when_the_conversation_cannot_resume() {
    let h = Harness::new();
    let (_, turn) = h.start_turn(
        "s1",
        TurnOptions {
            provider_account_id: Some("work".into()),
            ..Default::default()
        },
    );
    h.emit_task_tool(
        "toolu_c1",
        "TaskCreate",
        json!({ "subject": "Write tests" }),
        "Task #1 created successfully: Write tests",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    let (events, turn) = h.restarted_turn(
        TurnOptions {
            provider_account_id: Some("home".into()),
            ..Default::default()
        },
        "sess_1",
    );
    assert!(
        !h.spawned()
            .last()
            .unwrap()
            .iter()
            .any(|arg| arg == "--resume")
    );
    h.emit_task_tool(
        "toolu_u1",
        "TaskUpdate",
        json!({ "taskId": "1", "status": "completed" }),
        "Updated task #1 status",
        "sess_1",
    );
    h.result("sess_1");
    finish(turn).unwrap();

    assert!(tasks_updates(&events).is_empty());
}

// describe("claude assistant message boundaries")

#[test]
fn keeps_a_follow_up_paragraph_separate_and_does_not_replay_its_snapshot() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    let progress = "- update the notes and commit";
    let update = "Connect returned an empty file for one image on one post. The catch-up skips it and carries on, and I'll include it in the final tally.";
    for text in [progress, update] {
        h.emit(json!({
            "type": "stream_event",
            "session_id": "sess_1",
            "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } },
        }));
        h.emit(json!({
            "type": "assistant",
            "session_id": "sess_1",
            "message": { "content": [{ "type": "text", "text": text }] },
        }));
    }
    h.result("sess_1");
    finish(turn).unwrap();

    let texts: Vec<String> = events
        .reduce()
        .blocks
        .iter()
        .map(|block| block.text.clone())
        .collect();
    assert_eq!(texts, [progress, update]);
    let deltas: Vec<HarnessEvent> = events
        .all()
        .into_iter()
        .filter(|event| matches!(event, HarnessEvent::MessageDelta { .. }))
        .collect();
    assert_eq!(
        deltas,
        [
            HarnessEvent::MessageDelta {
                text: progress.into()
            },
            HarnessEvent::MessageDelta {
                text: update.into()
            },
        ]
    );
}

// describe("claude model switching")

#[test]
fn restarts_a_named_account_with_the_new_model_while_resuming_the_provider_conversation() {
    let h = Harness::new();
    let (_, turn) = h.start_turn(
        "s1",
        TurnOptions {
            provider_account_id: Some("account-work".into()),
            ..Default::default()
        },
    );
    h.result("sess_1");
    finish(turn).unwrap();

    let user_count = h.user_count();
    let (_, second) = h.send(
        "s1",
        TurnOptions {
            model: Some("claude:opus-5"),
            provider_account_id: Some("account-work".into()),
            text: Some("what did I ask before?"),
            ..Default::default()
        },
    );
    h.wait_for(|| h.spawned().len() == 2, "replacement Claude process");
    let args = &h.spawned()[1];
    assert!(contains_all(
        args,
        &["--model", "claude-opus-5", "--resume", "sess_1"]
    ));
    assert!(!args.iter().any(|arg| arg == "--session-id"));

    h.emit(json!({ "type": "system", "subtype": "init", "session_id": "sess_1" }));
    h.wait_for(|| h.user_count() > user_count, "follow-up prompt");
    h.result("sess_1");
    finish(second).unwrap();
}

// describe("claude legacy account resume")

#[test]
fn resumes_a_legacy_thread_when_the_missing_account_resolves_to_default() {
    let h = Harness::new();
    h.sessions
        .bind_session("s1", "legacy-session", "/repo", None);
    let (_, turn) = h.start_turn(
        "s1",
        TurnOptions {
            provider_account_id: Some("default".into()),
            ..Default::default()
        },
    );
    let args = &h.spawned()[0];
    assert!(contains_all(args, &["--resume", "legacy-session"]));
    assert!(!args.iter().any(|arg| arg == "--session-id"));
    h.result("legacy-session");
    finish(turn).unwrap();
}

#[test]
fn does_not_resume_a_legacy_default_thread_under_a_named_account() {
    let h = Harness::new();
    h.sessions
        .bind_session("s1", "legacy-session", "/repo", None);
    let (_, turn) = h.start_turn(
        "s1",
        TurnOptions {
            provider_account_id: Some("account-work".into()),
            ..Default::default()
        },
    );
    let args = &h.spawned()[0];
    assert!(!args.iter().any(|arg| arg == "--resume"));
    assert!(args.iter().any(|arg| arg == "--session-id"));
    h.result("sess_1");
    finish(turn).unwrap();
}

// describe("claude subagents")

fn approval_request_id(events: &Events) -> (i64, Option<String>) {
    events
        .all()
        .into_iter()
        .find_map(|event| match event {
            HarnessEvent::ApprovalRequested {
                request_id,
                call_id,
                ..
            } => Some((request_id, call_id)),
            _ => None,
        })
        .expect("no approval.requested")
}

#[test]
fn routes_a_child_permission_decision() {
    for decision in [ApprovalDecision::Allow, ApprovalDecision::Deny] {
        let h = Harness::new();
        let (events, turn) = h.start_turn("s1", TurnOptions::default());
        h.emit(json!({
            "type": "control_request",
            "request_id": "child_permission",
            "session_id": "sess_child",
            "parent_tool_use_id": "toolu_agent",
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Read",
                "tool_use_id": "child_read",
                "input": { "file_path": "/home/user/.gitconfig" },
            },
        }));
        let (request_id, call_id) = approval_request_id(&events);
        assert_eq!(call_id.as_deref(), Some("child_read"));
        h.sessions.respond_approval("s1", request_id, decision);
        h.wait_for(
            || h.response_for("child_permission").is_some(),
            "child decision",
        );
        let response = h.response_for("child_permission").unwrap();
        assert_eq!(response["type"], "control_response");
        let expected = match decision {
            ApprovalDecision::Allow => "allow",
            ApprovalDecision::Deny => "deny",
        };
        assert_eq!(response["response"]["response"]["behavior"], expected);
        let bound = events
            .all()
            .into_iter()
            .rev()
            .find_map(|event| match event {
                HarnessEvent::SessionProviderBound {
                    provider_session_id,
                } => Some(provider_session_id),
                _ => None,
            });
        assert_eq!(bound.as_deref(), Some("sess_1"));
        h.result("sess_1");
        finish(turn).unwrap();
    }
}

fn emit_child_questions(h: &Harness, prompt: impl Fn(&str) -> String) {
    for id in ["child_a", "child_b"] {
        h.emit(json!({
            "type": "control_request",
            "request_id": id,
            "parent_tool_use_id": format!("agent_{id}"),
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "AskUserQuestion",
                "input": { "questions": [{ "question": prompt(id), "options": [{ "label": "Proceed" }] }] },
            },
        }));
    }
}

#[test]
fn keeps_simultaneous_child_questions_reachable_in_the_single_question_ui() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_child_questions(&h, |id| format!("Question from {id}"));
    let asked = events
        .all()
        .into_iter()
        .filter(|event| matches!(event, HarnessEvent::QuestionAsked { .. }))
        .count();
    assert_eq!(asked, 1);
    for id in ["child_a", "child_b"] {
        h.wait_for(
            || {
                events.reduce().pending_question.is_some_and(|request| {
                    request.questions[0].prompt == format!("Question from {id}")
                })
            },
            "pending question",
        );
        let request = events.reduce().pending_question.unwrap();
        let question = &request.questions[0];
        let reply = if id == "child_a" {
            UserQuestionReply::Answered {
                answers: std::collections::BTreeMap::from([(
                    question.id.clone(),
                    vec![question.options[0].id.clone()],
                )]),
                custom: None,
            }
        } else {
            UserQuestionReply::Skipped
        };
        h.sessions.respond_question("s1", request.request_id, reply);
        h.wait_for(|| h.response_for(id).is_some(), "question response");
        let behavior = if id == "child_a" { "allow" } else { "deny" };
        assert_eq!(
            h.response_for(id).unwrap()["response"]["response"]["behavior"],
            behavior
        );
    }
    h.wait_for(
        || events.reduce().pending_question.is_none(),
        "no pending question",
    );
    h.result("sess_1");
    finish(turn).unwrap();
}

#[test]
fn preserves_the_remaining_question_when_one_is_cancelled_by_the_server() {
    for cancelled in ["child_a", "child_b"] {
        let h = Harness::new();
        let (events, turn) = h.start_turn("s1", TurnOptions::default());
        emit_child_questions(&h, str::to_string);
        h.emit(json!({ "type": "control_cancel_request", "request_id": cancelled }));
        h.wait_for(
            || events.any(|event| matches!(event, HarnessEvent::QuestionResolved { .. })),
            "cancelled question",
        );
        let remaining = if cancelled == "child_a" {
            "child_b"
        } else {
            "child_a"
        };
        h.wait_for(
            || {
                events
                    .reduce()
                    .pending_question
                    .is_some_and(|request| request.questions[0].prompt == remaining)
            },
            "remaining question",
        );
        let request = events.reduce().pending_question.unwrap();
        h.sessions
            .respond_question("s1", request.request_id, UserQuestionReply::Skipped);
        h.wait_for(
            || h.response_for(remaining).is_some(),
            "remaining question response",
        );
        assert!(h.response_for(cancelled).is_none());
        h.result("sess_1");
        finish(turn).unwrap();
    }
}

#[test]
fn fails_the_active_turn_if_a_child_permission_reply_cannot_be_delivered() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit(json!({
        "type": "control_request",
        "request_id": "child_permission",
        "parent_tool_use_id": "toolu_agent",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "Read",
            "input": { "file_path": "/home/user/.gitconfig" },
        },
    }));
    let (request_id, _) = approval_request_id(&events);
    h.reject_next_write("Broken pipe");
    h.sessions
        .respond_approval("s1", request_id, ApprovalDecision::Allow);
    let error = finish(turn).unwrap_err();
    assert_eq!(error.to_string(), "Broken pipe");
    assert!(events.any(|event| event
        == &HarnessEvent::SessionError {
            message: "Broken pipe".into()
        }));
}

fn tool_of(session: &Session, call_id: &str) -> monocode_core::block::BlockTool {
    session
        .blocks
        .iter()
        .find_map(|block| {
            block
                .tool
                .as_ref()
                .filter(|tool| tool.call_id.as_deref() == Some(call_id))
                .cloned()
        })
        .unwrap_or_else(|| panic!("no tool block {call_id}"))
}

#[test]
fn shows_one_row_per_background_subagent_when_the_task_list_comes_first() {
    for descriptions in [
        ["Explore the auth module", "Review the tests"],
        ["Explore the auth module", "Explore the auth module"],
    ] {
        let h = Harness::new();
        let (events, turn) = h.start_turn("s1", TurnOptions::default());
        let agents = [
            ("toolu_a", "t1", descriptions[0]),
            ("toolu_b", "t2", descriptions[1]),
        ];
        h.emit(json!({
            "type": "assistant",
            "session_id": "sess_1",
            "message": { "content": agents.iter().map(|(id, _, description)| json!({
                "type": "tool_use",
                "id": id,
                "name": "Agent",
                "input": { "description": description, "subagent_type": "explore", "run_in_background": true },
            })).collect::<Vec<_>>() },
        }));
        // Claude lists the tasks, with no tool_use_id, before it announces them.
        h.emit(json!({
            "type": "system",
            "subtype": "background_tasks_changed",
            "tasks": agents.iter().map(|(_, task, description)| json!({
                "task_id": task, "task_type": "local_agent", "description": description,
            })).collect::<Vec<_>>(),
        }));
        // Each listed task must claim a different call, even before
        // task_started supplies the authoritative ids for agents with
        // identical descriptions.
        let mut updated: Vec<String> = events
            .all()
            .into_iter()
            .filter_map(|event| match event {
                HarnessEvent::ToolUpdated { call_id, .. } => Some(call_id),
                _ => None,
            })
            .collect();
        updated.sort();
        updated.dedup();
        assert_eq!(updated, ["toolu_a", "toolu_b"]);
        for (id, task, description) in agents {
            h.emit(json!({
                "type": "system",
                "subtype": "task_started",
                "task_id": task,
                "tool_use_id": id,
                "description": description,
                "task_type": "local_agent",
                "is_backgrounded": true,
            }));
        }
        let rows: Vec<String> = events
            .all()
            .into_iter()
            .filter_map(|event| match event {
                HarnessEvent::ToolStarted {
                    call_id,
                    kind: Some(kind),
                    ..
                } if kind == "agent" => Some(call_id),
                _ => None,
            })
            .collect();
        assert_eq!(rows, ["toolu_a", "toolu_b"]);

        h.emit(json!({
            "type": "system",
            "subtype": "task_notification",
            "task_id": "t1",
            "tool_use_id": "toolu_a",
            "status": "completed",
            "summary": "First agent finished",
        }));
        let session = events.reduce();
        let first = tool_of(&session, "toolu_a");
        assert_eq!(first.status.as_deref(), Some("completed"));
        assert_eq!(first.detail.as_deref(), Some("First agent finished"));
        assert_eq!(
            tool_of(&session, "toolu_b").status.as_deref(),
            Some("in_progress")
        );
        h.emit(json!({
            "type": "system",
            "subtype": "task_notification",
            "task_id": "t2",
            "tool_use_id": "toolu_b",
            "status": "completed",
            "summary": "Second agent finished",
        }));
        // Both agents finished before the result, so the turn waits for the
        // follow-up Claude takes to read them, which this fixture never sends.
        h.result("sess_1");
        finish(turn).unwrap();
    }
}

#[test]
fn keeps_an_unmatched_background_subagent_visible_until_it_finishes() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    let description = "Explore the auth module";
    // A task can be listed without an Agent call in the parent transcript.
    for _ in 0..2 {
        h.emit(json!({
            "type": "system",
            "subtype": "background_tasks_changed",
            "tasks": [{ "task_id": "t1", "task_type": "local_agent", "description": description }],
        }));
    }
    h.emit(json!({
        "type": "system",
        "subtype": "task_progress",
        "task_id": "t1",
        "description": description,
        "summary": "Reading the auth module",
    }));
    let call_id = format!("agent:{description}");
    let session = events.reduce();
    let agent_rows = |session: &Session| {
        session
            .blocks
            .iter()
            .filter(|block| {
                block.tool.as_ref().and_then(|tool| tool.kind.as_deref()) == Some("agent")
            })
            .count()
    };
    assert_eq!(agent_rows(&session), 1);
    let row = tool_of(&session, &call_id);
    assert_eq!(row.status.as_deref(), Some("in_progress"));
    assert_eq!(row.detail.as_deref(), Some("Reading the auth module"));

    h.emit(json!({
        "type": "system",
        "subtype": "task_notification",
        "task_id": "t1",
        "status": "completed",
        "summary": "Found the auth entry points",
    }));
    // The task finished before the result, so the turn waits for the
    // follow-up Claude takes to read it, which this fixture never sends.
    h.result("sess_1");
    finish(turn).unwrap();

    let finished = events.reduce();
    assert_eq!(agent_rows(&finished), 1);
    let row = tool_of(&finished, &call_id);
    assert_eq!(row.status.as_deref(), Some("completed"));
    assert_eq!(row.detail.as_deref(), Some("Found the auth entry points"));
}

fn emit_agent_call(h: &Harness, input: Value) {
    h.emit(json!({
        "type": "assistant",
        "session_id": "sess_1",
        "message": { "content": [{ "type": "tool_use", "id": "toolu_agent", "name": "Agent", "input": input }] },
    }));
}

fn has_message_completed(events: &Events) -> bool {
    events.any(|event| event == &HarnessEvent::MessageCompleted)
}

#[test]
fn stays_busy_after_a_parent_result_while_a_background_subagent_is_running() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_agent_call(
        &h,
        json!({ "description": "Explore the auth module", "subagent_type": "explore" }),
    );
    h.emit(json!({
        "type": "system",
        "subtype": "task_started",
        "task_id": "t1",
        "tool_use_id": "toolu_agent",
        "description": "Explore the auth module",
        "task_type": "local_agent",
        "is_backgrounded": true,
    }));
    h.emit(json!({
        "type": "user",
        "session_id": "sess_1",
        "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_agent", "content": "Backgrounded" }] },
    }));
    h.result("sess_1");

    assert!(!settles_within(&turn, Duration::from_millis(30)));
    assert!(events.any(|event| matches!(
        event,
        HarnessEvent::ToolStarted { kind: Some(kind), title, .. }
            if kind == "agent" && title == "Explore the auth module"
    )));
    assert!(!has_message_completed(&events));

    h.emit(json!({
        "type": "system",
        "subtype": "task_notification",
        "task_id": "t1",
        "tool_use_id": "toolu_agent",
        "status": "completed",
        "summary": "Found the tokens",
    }));
    // The notification wakes Claude for a follow-up turn; that turn's result
    // is what ends the MonoCode turn.
    assert!(!settles_within(&turn, Duration::from_millis(30)));
    h.emit_follow_up_turn("The explorer found the tokens.");
    finish(turn).unwrap();
    assert!(has_message_completed(&events));
}

#[test]
fn does_not_end_the_turn_on_a_subagent_result() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_agent_call(
        &h,
        json!({ "description": "Explore", "subagent_type": "explore" }),
    );
    h.emit(json!({
        "type": "result",
        "subtype": "success",
        "session_id": "sess_sub",
        "parent_tool_use_id": "toolu_agent",
    }));
    assert!(!settles_within(&turn, Duration::from_millis(30)));
    assert!(!has_message_completed(&events));
    h.result("sess_1");
    finish(turn).unwrap();
}

#[test]
fn does_not_dump_subagent_assistant_text_into_the_parent_transcript() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_agent_call(
        &h,
        json!({ "description": "Explore", "subagent_type": "explore" }),
    );
    h.emit(json!({
        "type": "assistant",
        "parent_tool_use_id": "toolu_agent",
        "message": { "content": [{ "type": "text", "text": "I will grep for tokens" }] },
    }));
    h.result("sess_1");
    finish(turn).unwrap();
    assert!(!events.any(|event| matches!(
        event,
        HarnessEvent::MessageDelta { text } if text.contains("I will grep for tokens")
    )));
}

#[test]
fn mirrors_a_subagents_tools_thinking_and_prose_onto_its_own_row() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_agent_call(
        &h,
        json!({ "description": "Correctness review", "subagent_type": "explore" }),
    );
    h.emit(json!({
        "type": "assistant",
        "parent_tool_use_id": "toolu_agent",
        "message": {
            "id": "msg_sub_1",
            "model": "claude-haiku-4-5",
            "content": [
                { "type": "thinking", "thinking": "Start with the reducer." },
                { "type": "text", "text": "I will grep for tokens" },
                { "type": "tool_use", "id": "toolu_sub_read", "name": "Read", "input": { "file_path": "/repo/src/App.tsx" } },
            ],
        },
    }));
    h.emit(json!({
        "type": "user",
        "parent_tool_use_id": "toolu_agent",
        "message": { "content": [{
            "type": "tool_result", "tool_use_id": "toolu_sub_read", "content": "export function App() {}",
        }] },
    }));
    h.result("sess_1");
    finish(turn).unwrap();

    let session = events.reduce();
    let agent = session
        .blocks
        .iter()
        .find(|block| {
            block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("toolu_agent")
        })
        .unwrap();
    assert_eq!(
        agent
            .agent_run
            .as_ref()
            .and_then(|run| run.model.as_deref()),
        Some("claude-haiku-4-5")
    );
    let steps: Vec<(String, String, AgentStepKind, String, Option<String>)> = events
        .all()
        .into_iter()
        .filter_map(|event| match event {
            HarnessEvent::AgentStep {
                call_id,
                step_id,
                kind,
                text,
                status,
                ..
            } => Some((call_id, step_id, kind, text, status)),
            _ => None,
        })
        .collect();
    assert!(steps.iter().all(|step| step.0 == "toolu_agent"));
    let steps: Vec<_> = steps
        .into_iter()
        .map(|(_, step_id, kind, text, status)| (step_id, kind, text, status))
        .collect();
    assert_eq!(
        steps,
        [
            (
                "msg_sub_1:thinking".to_string(),
                AgentStepKind::Reasoning,
                "Start with the reducer.".to_string(),
                None
            ),
            (
                "msg_sub_1:text".to_string(),
                AgentStepKind::Message,
                "I will grep for tokens".to_string(),
                None
            ),
            (
                "toolu_sub_read".to_string(),
                AgentStepKind::Tool,
                "Read /repo/src/App.tsx".to_string(),
                Some("in_progress".to_string())
            ),
            (
                "toolu_sub_read".to_string(),
                AgentStepKind::Tool,
                String::new(),
                Some("completed".to_string())
            ),
        ]
    );
}

#[test]
fn keeps_a_failed_subagent_tool_result_on_its_tool_row() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_agent_call(&h, json!({ "description": "Run tests" }));
    h.emit(json!({
        "type": "assistant",
        "parent_tool_use_id": "toolu_agent",
        "message": { "content": [{ "type": "tool_use", "id": "toolu_sub_bash", "name": "Bash", "input": { "command": "npm test" } }] },
    }));
    h.emit(json!({
        "type": "user",
        "parent_tool_use_id": "toolu_agent",
        "message": { "content": [{
            "type": "tool_result",
            "tool_use_id": "toolu_sub_bash",
            "is_error": true,
            "content": [{ "type": "text", "text": "Tests failed: assertion error" }],
        }] },
    }));
    h.result("sess_1");
    finish(turn).unwrap();

    let session = events.reduce();
    let steps = &session
        .blocks
        .iter()
        .find(|block| {
            block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("toolu_agent")
        })
        .and_then(|block| block.agent_run.as_ref())
        .unwrap()
        .steps;
    assert_eq!(steps.len(), 1);
    let step = &steps[0];
    assert_eq!(step.id, "toolu_sub_bash");
    assert_eq!(step.kind, AgentStepKind::Tool);
    assert_eq!(step.text, "npm test");
    assert_eq!(step.tool_kind.as_deref(), Some("execute"));
    assert_eq!(step.status.as_deref(), Some("failed"));
    assert_eq!(
        step.detail.as_deref(),
        Some("Tests failed: assertion error")
    );
}

#[test]
fn does_not_mirror_a_subagent_result_onto_the_parent_tool_row() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    emit_agent_call(&h, json!({ "description": "Correctness review" }));
    h.emit(json!({
        "type": "user",
        "parent_tool_use_id": "toolu_agent",
        "message": { "content": [{
            "type": "tool_result", "tool_use_id": "toolu_sub_read", "content": "export function App() {}",
        }] },
    }));
    h.result("sess_1");
    finish(turn).unwrap();

    // The parent stays in flight: only the subagent's own row settles.
    assert!(!events.any(|event| matches!(
        event,
        HarnessEvent::ToolUpdated { call_id, status: Some(status), .. }
            if call_id == "toolu_agent" && status == "completed"
    )));
}

#[test]
fn ends_the_turn_once_a_subagent_has_reported_back_inline() {
    let h = Harness::new();
    let (_, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_inline_subagent();
    h.result("sess_1");
    finish(turn).unwrap();
}

#[test]
fn routes_an_unexpected_provider_exit_to_the_turn_that_is_actually_running() {
    let h = Harness::new();
    let (first, turn) = h.start_turn("s1", TurnOptions::default());
    h.result("sess_1");
    finish(turn).unwrap();

    let user_messages = h.user_count();
    let (second, turn) = h.send(
        "s1",
        TurnOptions {
            text: Some("try again"),
            ..Default::default()
        },
    );
    h.wait_for(|| h.user_count() > user_messages, "second user prompt");

    h.exit(Some(1));
    let error = finish(turn).unwrap_err();
    assert!(error.to_string().contains("Claude Code exited"));
    assert!(!first.any(|event| matches!(event, HarnessEvent::SessionEnded { .. })));
    assert!(second.any(|event| event == &HarnessEvent::SessionEnded { code: Some(1) }));
    assert!(second.any(|event| event
        == &HarnessEvent::SessionError {
            message: "Claude Code exited".into()
        }));
}

// describe("claude background tasks")

#[test]
fn keeps_the_turn_working_through_a_background_command_and_claudes_follow_up() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash("b1");
    assert!(!settles_within(&turn, Duration::from_millis(30)));
    assert!(!has_message_completed(&events));
    assert_eq!(
        events.background_updates(),
        [vec!["Wait 30 seconds then print done".to_string()]]
    );

    h.emit_bash_finished("b1");
    assert!(!settles_within(&turn, Duration::from_millis(30)));

    h.emit_follow_up_turn("It finished and printed done.");
    finish(turn).unwrap();
    assert_eq!(
        events.background_updates(),
        [vec!["Wait 30 seconds then print done".to_string()], vec![]]
    );
    // The command waited on sits under the message Claude left off with as a
    // row of its own, and the reply is a new message after it, once.
    let session = events.reduce();
    let background = session
        .blocks
        .iter()
        .find(|block| {
            block
                .tool
                .as_ref()
                .is_some_and(|tool| tool.background == Some(true))
        })
        .unwrap();
    let tool = background.tool.as_ref().unwrap();
    assert_eq!(tool.call_id.as_deref(), Some("background:b1"));
    assert_eq!(tool.status.as_deref(), Some("completed"));
    assert_eq!(
        tool.detail.as_deref(),
        Some("Background command \"sleep 30 && echo done\" completed (exit code 0)")
    );
    assert!(background.text.contains("sleep 30"));
    let order: Vec<String> = session
        .blocks
        .iter()
        .filter(|block| {
            block.role == BlockRole::Assistant
                || block
                    .tool
                    .as_ref()
                    .is_some_and(|tool| tool.background == Some(true))
        })
        .map(|block| {
            if block
                .tool
                .as_ref()
                .is_some_and(|tool| tool.background == Some(true))
            {
                "[background]".to_string()
            } else {
                block.text.clone()
            }
        })
        .collect();
    assert_eq!(
        order,
        ["waiting", "[background]", "It finished and printed done."]
    );
}

#[test]
fn shows_the_waited_on_command_as_a_live_row_under_claudes_last_message() {
    let h = Harness::new();
    let (events, _turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash("b1");

    let session = events.reduce();
    let count = session.blocks.len();
    assert_eq!(session.blocks[count - 2].text, "waiting");
    let last = session.blocks[count - 1].tool.as_ref().unwrap();
    assert_eq!(last.background, Some(true));
    assert_eq!(last.status.as_deref(), Some("in_progress"));
    assert_eq!(last.kind.as_deref(), Some("execute"));
}

#[test]
fn lets_the_turn_go_if_a_finished_task_never_wakes_claude() {
    let h = Harness::new();
    let (_, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash("b1");
    h.emit_bash_finished("b1");
    assert!(!settles_within(&turn, GRACE / 2));
    assert!(settles_within(&turn, GRACE * 3));
    finish(turn).unwrap();
}

#[test]
fn waits_for_claudes_follow_up_when_a_command_finishes_before_the_result() {
    let h = Harness::new();
    let (_, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash_launch("b1");
    h.emit_bash_finished("b1");
    h.result("sess_1");
    assert!(!settles_within(&turn, Duration::from_millis(30)));
    h.emit_follow_up_turn("It finished and printed done.");
    finish(turn).unwrap();
}

#[test]
fn ends_at_the_result_when_a_later_tool_result_already_carried_the_notice() {
    let h = Harness::new();
    let (_, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash_launch("b1");
    h.emit_bash_finished("b1");
    h.emit(json!({
        "type": "assistant",
        "session_id": "sess_1",
        "message": { "content": [{ "type": "tool_use", "id": "toolu_ls", "name": "Bash", "input": { "command": "ls" } }] },
    }));
    h.emit(json!({
        "type": "user",
        "session_id": "sess_1",
        "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_ls", "content": "" }] },
    }));
    h.result("sess_1");
    assert!(settles_within(&turn, GRACE / 2));
    finish(turn).unwrap();
}

#[test]
fn lets_the_turn_go_if_a_command_finished_before_the_result_never_wakes_claude() {
    let h = Harness::new();
    let (_, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash_launch("b1");
    h.emit_bash_finished("b1");
    h.result("sess_1");
    assert!(!settles_within(&turn, GRACE / 2));
    assert!(settles_within(&turn, GRACE * 3));
    finish(turn).unwrap();
}

#[test]
fn stops_background_commands_when_the_turn_is_stopped() {
    let h = Harness::new();
    let (events, turn) = h.start_turn("s1", TurnOptions::default());
    h.emit_background_bash("b7");

    smol::block_on(h.sessions.cancel_turn("s1")).unwrap();
    finish(turn).unwrap();
    let requests: Vec<Value> = h
        .parse()
        .into_iter()
        .filter_map(|message| message.get("request").cloned())
        .collect();
    assert!(requests.contains(&json!({ "subtype": "stop_task", "task_id": "b7" })));
    assert_eq!(requests.last(), Some(&json!({ "subtype": "interrupt" })));
    assert!(has_message_completed(&events));
}

// describe("claude plan permissions")

#[test]
fn answers_residual_plan_mode_permissions_without_prompting_the_user() {
    let h = Harness::new();
    let (events, turn) = h.start_turn(
        "s1",
        TurnOptions {
            runtime_mode: Some(RuntimeMode::Auto),
            intent: Some(TurnIntent::Plan),
            ..Default::default()
        },
    );
    h.emit(json!({
        "type": "control_request",
        "request_id": "read_1",
        "request": { "subtype": "can_use_tool", "tool_name": "Read", "input": { "file_path": "/repo/src/App.tsx" } },
    }));
    h.emit(json!({
        "type": "control_request",
        "request_id": "write_1",
        "request": { "subtype": "can_use_tool", "tool_name": "Write", "input": { "file_path": "/repo/src/new.ts" } },
    }));
    h.wait_for(
        || {
            h.parse()
                .iter()
                .filter(|m| m["type"] == "control_response")
                .count()
                >= 2
        },
        "plan permission responses",
    );
    assert_eq!(
        h.response_for("read_1").unwrap()["response"]["response"]["behavior"],
        "allow"
    );
    assert_eq!(
        h.response_for("write_1").unwrap()["response"]["response"]["behavior"],
        "deny"
    );
    assert!(!events.any(|event| matches!(event, HarnessEvent::ApprovalRequested { .. })));
    h.result("sess_1");
    finish(turn).unwrap();
}

#[test]
fn leaves_the_captured_plan_ready_to_build_after_a_subagent_explored_for_it() {
    let h = Harness::new();
    let (events, turn) = h.start_turn(
        "s1",
        TurnOptions {
            intent: Some(TurnIntent::Plan),
            ..Default::default()
        },
    );
    h.emit_inline_subagent();
    h.emit(json!({
        "type": "control_request",
        "request_id": "exit_1",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "ExitPlanMode",
            "tool_use_id": "toolu_exit",
            "input": { "plan": "# Plan\n\nRewrite the auth module." },
        },
    }));
    h.wait_for(
        || h.response_for("exit_1").is_some(),
        "exit plan mode response",
    );
    h.result("sess_1");

    // The turn must end for the session to stop being busy; until it does,
    // the plan's Build control stays disabled however the plan block reads.
    finish(turn).unwrap();
    let session = events.reduce();
    let plan = session
        .blocks
        .iter()
        .find(|block| block.role == BlockRole::Plan)
        .unwrap();
    assert!(plan.text.contains("Rewrite the auth module."));
    assert!(!plan.is_streaming());
    assert_eq!(
        plan.plan.as_ref().map(|plan| plan.status),
        Some(PlanStatus::Ready)
    );
}

// describe("claude manual compaction")

#[test]
fn runs_the_built_in_command_and_requires_a_compact_boundary() {
    let h = Harness::new();
    let (_, turn) = h.start_turn("s1", TurnOptions::default());
    h.result("sess_1");
    finish(turn).unwrap();
    h.clear_sent();

    let events = Events::default();
    let sessions = h.sessions.clone();
    let sink = events.sink();
    let compact = smol::spawn(async move {
        sessions
            .compact_context(
                HarnessSessionInput {
                    session_id: "s1".into(),
                    cwd: "/repo".into(),
                    model: "claude:claude-sonnet-5".into(),
                    model_settings: None,
                    provider_account_id: None,
                    runtime_mode: RuntimeMode::Supervised,
                    intent: None,
                    controls_agents: None,
                    app_access: None,
                },
                sink,
            )
            .await
    });
    h.wait_for(|| h.user_count() > 0, "compact command");
    let command = h.parse().into_iter().find(|m| m["type"] == "user").unwrap();
    assert_eq!(
        command["message"]["content"],
        json!([{ "type": "text", "text": "/compact" }])
    );

    h.emit(json!({
        "type": "assistant",
        "session_id": "sess_1",
        "message": { "content": [{ "type": "text", "text": "not transcript output" }] },
    }));
    h.emit(json!({ "type": "system", "subtype": "compact_boundary", "session_id": "sess_1" }));
    h.result("sess_1");
    finish(compact).unwrap();

    assert!(events.any(|event| event
        == &HarnessEvent::Status {
            text: "Compacted context".into()
        }));
    assert!(!events.any(|event| matches!(event, HarnessEvent::MessageDelta { .. })));
}

// Not in the TypeScript suite: the sink runs after the session unlocks.

#[test]
fn a_sink_can_answer_an_approval_from_inside_its_callback() {
    let h = Harness::new();
    let events = Events::default();
    let sink: EventSink = {
        let record = events.sink();
        let sessions = h.sessions.clone();
        Arc::new(move |event: HarnessEvent| {
            if let HarnessEvent::ApprovalRequested { request_id, .. } = &event {
                sessions.respond_approval("s1", *request_id, ApprovalDecision::Allow);
            }
            record(event);
        })
    };
    let turn = h.send_with("s1", TurnOptions::default(), sink);
    h.wait_for(
        || {
            h.parse()
                .iter()
                .any(|m| m["request"]["subtype"] == "initialize")
        },
        "initialize",
    );
    h.emit(json!({ "type": "system", "subtype": "init", "session_id": "sess_1" }));
    h.wait_for(|| h.user_count() > 0, "user prompt");
    h.emit(json!({
        "type": "control_request",
        "request_id": "bash_1",
        "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "ls" } },
    }));
    h.wait_for(|| h.response_for("bash_1").is_some(), "approval response");
    assert_eq!(
        h.response_for("bash_1").unwrap()["response"]["response"]["behavior"],
        "allow"
    );
    h.result("sess_1");
    finish(turn).unwrap();
    assert!(events.any(|event| matches!(
        event,
        HarnessEvent::ApprovalResolved {
            decision: monocode_core::block::ApprovalDecided::Allow,
            ..
        }
    )));
}
