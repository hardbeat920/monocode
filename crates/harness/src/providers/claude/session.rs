//! Port of src/integrations/harness/providers/claude/claude.ts: live Claude
//! Code sessions over stream-json.
//!
//! The TypeScript kept `liveByThread`, `resumeByThread`, `tasksByThread`, and
//! `cancelledThreads` in module globals. Here they live in a [`ClaudeSessions`]
//! value. Each live child is a [`LiveCell`]: its state sits behind a mutex
//! that the stdout reader, the turn future, approval continuations, and the
//! resume timer all take. Promise resolvers became oneshot senders.
//!
//! The sessions map and a live session are never locked together; a task map
//! is only locked inside its live session. Events queue in the session's
//! outbox while it is locked and reach the turn's [`EventSink`] in order once
//! the lock is released, so a sink may call back into this adapter.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, LazyLock, Weak};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use monocode_core::attachment::Attachment;
use monocode_core::block::{
    AgentStepKind, ApprovalDecided, TaskListItem, TaskListMeta, TurnIntent,
};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{
    ApprovalDecision, HarnessEvent, HarnessSessionInput, QuestionDecision, SendTurnInput,
    SteerTurnInput,
};
use monocode_core::user_question::{
    UserQuestionReply, question_prompt_title, questions_from_unknown,
};
use parking_lot::{Mutex, MutexGuard, ReentrantMutex};
use regex::Regex;
use serde_json::{Value, json};

use crate::core::provider_accounts::same_provider_account_id;
use crate::core::registry::EventSink;
use crate::core::task::{SharedSpawner, sleep, timeout};

use super::io::{SharedChildIo, claude_account};
use super::protocol::*;
use super::shared::{OrderedMap, is_agent_tool_name, join_stream_text, snapshot_remainder};

/// Task-list block key for TaskCreate and TaskUpdate items.
pub const CLAUDE_TASKS_KEY: &str = "claude-tasks";

/// `INIT_TIMEOUT_MS`.
pub const INIT_TIMEOUT: Duration = Duration::from_millis(8_000);

/// `RESUME_GRACE_MS`: how long a finished background task may take to wake
/// Claude before the turn is let go anyway. The follow-up turn normally starts
/// within a second or two.
pub const RESUME_GRACE: Duration = Duration::from_millis(15_000);

/// Settings the TypeScript read from app modules: `loadClaudeHooks` from
/// settings and `nativeModelId` from the live model catalog. Timings are
/// here so tests can shorten them.
#[derive(Clone)]
pub struct ClaudeSessionOptions {
    /// `loadClaudeHooks()`: let the user's `~/.claude` hooks run.
    pub claude_hooks: Arc<dyn Fn() -> bool + Send + Sync>,
    /// `nativeModelId(model)` over the app's model catalog.
    pub native_model_id: Arc<dyn Fn(&str) -> String + Send + Sync>,
    pub init_timeout: Duration,
    pub resume_grace: Duration,
}

impl Default for ClaudeSessionOptions {
    fn default() -> Self {
        Self {
            claude_hooks: Arc::new(|| monocode_core::settings::CLAUDE_HOOKS_DEFAULT),
            native_model_id: Arc::new(|model| {
                monocode_core::models::ModelCatalog::new().native_model_id_for(model)
            }),
            init_timeout: INIT_TIMEOUT,
            resume_grace: RESUME_GRACE,
        }
    }
}

/// `ApprovalOutcome`. A PermissionRequest hook can decide before the user
/// touches the prompt; Claude then cancels the control request out from under
/// us. That is not a rejection, so it gets its own outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalOutcome {
    Allow,
    Deny,
    Cancelled,
}

impl From<ApprovalDecision> for ApprovalOutcome {
    fn from(decision: ApprovalDecision) -> Self {
        match decision {
            ApprovalDecision::Allow => ApprovalOutcome::Allow,
            ApprovalDecision::Deny => ApprovalOutcome::Deny,
        }
    }
}

enum QuestionOutcome {
    Reply(UserQuestionReply),
    Cancelled,
}

struct PendingApproval {
    request_id: String,
    resolve: oneshot::Sender<ApprovalOutcome>,
}

struct PendingQuestion {
    request_id: String,
    event: HarnessEvent,
    resolve: oneshot::Sender<QuestionOutcome>,
}

#[derive(Debug, Clone)]
struct InFlightTool {
    id: String,
    name: String,
    input: Record,
    partial_json: String,
    title: String,
}

#[derive(Debug, Clone)]
struct LiveAgentTask {
    tool_use_id: Option<String>,
    description: String,
    backgrounded: bool,
}

#[derive(Debug, Clone)]
struct BackgroundTask {
    description: String,
    tool_use_id: Option<String>,
}

type SharedTasks = Arc<Mutex<ClaudeTaskMap>>;

/// A turn's `turnDone` and `turnFailed`, with a token so a late failure can
/// tell whether the turn it belonged to is still the one running.
struct TurnWaiter {
    token: u64,
    resolve: oneshot::Sender<Result<(), String>>,
}

/// `Live`: one running Claude Code child.
struct Live {
    thread_id: String,
    cwd: String,
    claude_session_id: String,
    provider_account_id: Option<String>,
    runtime_mode: RuntimeMode,
    planning: bool,
    settings_key: String,
    on_event: EventSink,
    // Ui ids only grow, so key order is insertion order.
    approvals: BTreeMap<i64, PendingApproval>,
    questions: BTreeMap<i64, PendingQuestion>,
    visible_question_id: Option<i64>,
    next_approval_ui_id: i64,
    next_control_id: i64,
    /// Content block index to tool id. The tool itself is in `tools_by_id`.
    tools_by_index: HashMap<i64, String>,
    tools_by_id: OrderedMap<InFlightTool>,
    agent_tasks: OrderedMap<LiveAgentTask>,
    /// Every task Claude still runs for this session, by id: subagents, shells
    /// it backgrounded, monitors. Each one ends in a notification that wakes
    /// Claude for another turn, so the MonoCode turn stays open until they are
    /// done.
    background_tasks: OrderedMap<BackgroundTask>,
    /// Rows shown for tasks still running when Claude yielded, by task id.
    background_rows: HashMap<String, String>,
    /// A task finished after Claude yielded; its follow-up turn is on the way.
    /// Holds the token of the running grace timer.
    awaiting_resume: Option<u64>,
    /// A task finished before Claude yielded and no tool result has carried
    /// the notice yet, so Claude will take another turn to read it.
    resume_expected: bool,
    /// Last background list sent to the UI, to skip repeats.
    background_key: String,
    /// Finished-subagent notes held until Claude picks the thread back up.
    task_notes: Vec<String>,
    /// TaskCreate and TaskUpdate items, keyed by Claude's task id.
    claude_tasks: SharedTasks,
    turn_result_seen: bool,
    /// Latest `rate_limit_event` refused requests; reported when the turn ends.
    usage_limit: Option<ClaudeUsageLimit>,
    cancelled: bool,
    mute_updates: bool,
    turn: Option<TurnWaiter>,
    turn_end_pending: bool,
    active_turn: bool,
    init_done: Option<oneshot::Sender<()>>,
    initialized: bool,
    emitted_assistant: String,
    emitted_reasoning: String,
    pending_assistant_boundary: bool,
    manual_compaction: bool,
    compaction_confirmed: bool,
    next_token: u64,
    /// What the live session needs to write, spawn, and time.
    io: SharedChildIo,
    spawner: SharedSpawner,
    resume_grace: Duration,
    cell: Weak<LiveCell>,
    outbox: Arc<Outbox>,
}

/// Events waiting for their sink. A live session queues events while its
/// lock is held and delivers them, in order, once the lock is released, so a
/// sink may call back into the adapter. Delivery holds a reentrant lock: a
/// sink that emits again delivers the new events nested, as the synchronous
/// TypeScript callbacks did, and another thread waits its turn.
#[derive(Default)]
struct Outbox {
    queue: Mutex<VecDeque<(EventSink, HarnessEvent)>>,
    delivering: ReentrantMutex<()>,
}

impl Outbox {
    fn push(&self, sink: &EventSink, event: HarnessEvent) {
        self.queue.lock().push_back((sink.clone(), event));
    }

    fn flush(&self) {
        let _delivering = self.delivering.lock();
        loop {
            let next = self.queue.lock().pop_front();
            let Some((sink, event)) = next else {
                return;
            };
            sink(event);
        }
    }
}

/// A live session and the lock that runs its turns one at a time
/// (`live.turns` in TypeScript).
struct LiveCell {
    state: Mutex<Live>,
    outbox: Arc<Outbox>,
    turns: futures::lock::Mutex<()>,
}

impl LiveCell {
    /// Lock the session. Events it emits reach their sink when the guard drops.
    fn lock(&self) -> LiveGuard<'_> {
        LiveGuard {
            guard: Some(self.state.lock()),
            outbox: &self.outbox,
        }
    }
}

struct LiveGuard<'a> {
    guard: Option<MutexGuard<'a, Live>>,
    outbox: &'a Outbox,
}

impl std::ops::Deref for LiveGuard<'_> {
    type Target = Live;

    fn deref(&self) -> &Live {
        self.guard.as_ref().expect("live guard in use")
    }
}

impl std::ops::DerefMut for LiveGuard<'_> {
    fn deref_mut(&mut self) -> &mut Live {
        self.guard.as_mut().expect("live guard in use")
    }
}

impl Drop for LiveGuard<'_> {
    fn drop(&mut self) {
        self.guard.take();
        self.outbox.flush();
    }
}

type LiveRef = Arc<LiveCell>;

#[derive(Debug, Clone)]
struct Resume {
    session_id: String,
    cwd: String,
    provider_account_id: Option<String>,
}

#[derive(Clone)]
struct RetainedTasks {
    provider_session_id: String,
    tasks: SharedTasks,
}

#[derive(Default)]
struct Globals {
    live_by_thread: HashMap<String, LiveRef>,
    resume_by_thread: HashMap<String, Resume>,
    /// Claude task lists outlive a live child: a restart that resumes the
    /// conversation keeps its task ids, so later TaskUpdate calls must find
    /// earlier tasks. Task ids belong to one Claude conversation, so each map
    /// records which one.
    tasks_by_thread: HashMap<String, RetainedTasks>,
    cancelled_threads: HashSet<String>,
}

struct Inner {
    io: SharedChildIo,
    spawner: SharedSpawner,
    options: ClaudeSessionOptions,
    globals: Mutex<Globals>,
}

/// Every live Claude Code session in the app.
#[derive(Clone)]
pub struct ClaudeSessions {
    inner: Arc<Inner>,
}

fn agent_model(name: &str, input: &Record) -> Option<String> {
    if !is_agent_tool_name(name) {
        return None;
    }
    string_field(Some(input), "model").map(str::to_string)
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

fn tool_started(call_id: &str, title: &str, kind: &str, status: &str) -> HarnessEvent {
    HarnessEvent::ToolStarted {
        agent_model: None,
        call_id: call_id.into(),
        title: title.into(),
        kind: Some(kind.into()),
        status: Some(status.into()),
        background: None,
        preview: None,
        paths: None,
    }
}

fn tool_updated(call_id: &str) -> HarnessEvent {
    HarnessEvent::ToolUpdated {
        agent_model: None,
        call_id: call_id.into(),
        title: None,
        kind: None,
        status: None,
        detail: None,
        preview: None,
        paths: None,
    }
}

/// `tool_updated` with the common fields set.
fn tool_update(
    call_id: &str,
    title: Option<&str>,
    kind: &str,
    status: Option<&str>,
    detail: Option<String>,
) -> HarnessEvent {
    let mut event = tool_updated(call_id);
    if let HarnessEvent::ToolUpdated {
        title: t,
        kind: k,
        status: s,
        detail: d,
        ..
    } = &mut event
    {
        *t = title.map(str::to_string);
        *k = Some(kind.to_string());
        *s = status.map(str::to_string);
        *d = detail;
    }
    event
}

fn agent_step(call_id: &str, step_id: &str, kind: AgentStepKind, text: &str) -> HarnessEvent {
    HarnessEvent::AgentStep {
        call_id: call_id.into(),
        step_id: step_id.into(),
        kind,
        text: text.into(),
        tool_kind: None,
        status: None,
        detail: None,
        preview: None,
        agent_name: None,
        agent_type: None,
    }
}

static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[_-]+").unwrap());

impl Live {
    /// Queue an event for the turn's sink. It is delivered when this
    /// session's lock is released.
    fn emit(&self, event: HarnessEvent) {
        self.outbox.push(&self.on_event, event);
    }

    fn token(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }

    /// `nextControlId`.
    fn next_control_id(&mut self) -> String {
        self.next_control_id += 1;
        format!("monocode_{}", self.next_control_id)
    }

    fn turn_token(&self) -> Option<u64> {
        self.turn.as_ref().map(|turn| turn.token)
    }

    fn write_json(&self, payload: &Value) -> BoxFuture<'static, Result<()>> {
        write_json(&self.io, &self.thread_id, payload)
    }

    fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        self.spawner.spawn(future.boxed());
    }

    fn deny_all_pending(&mut self) {
        for (_, pending) in std::mem::take(&mut self.approvals) {
            let _ = pending.resolve.send(ApprovalOutcome::Deny);
        }
        for (_, pending) in std::mem::take(&mut self.questions) {
            let _ = pending
                .resolve
                .send(QuestionOutcome::Reply(UserQuestionReply::Skipped));
        }
    }

    // `handleStreamEvent`.
    fn handle_stream_event(&mut self, rec: &Record) {
        let subagent = is_subagent_message(rec);
        if let Some(delta) = stream_delta_from_event(rec) {
            if subagent {
                return;
            }
            match delta.kind {
                ClaudeDeltaKind::Assistant => {
                    self.close_pending_assistant_message();
                    self.emitted_assistant = join_stream_text(&self.emitted_assistant, &delta.text);
                    self.emit(HarnessEvent::MessageDelta { text: delta.text });
                }
                ClaudeDeltaKind::Reasoning => {
                    self.emitted_reasoning = join_stream_text(&self.emitted_reasoning, &delta.text);
                    self.emit(HarnessEvent::ReasoningDelta { text: delta.text });
                }
            }
            return;
        }

        if let Some(started) = tool_start_from_event(rec) {
            if subagent {
                self.note_subagent_tool(rec, &started.id, &started.name, &started.input);
                return;
            }
            let tool = InFlightTool {
                title: tool_title(&started.name, &started.input),
                id: started.id,
                name: started.name,
                input: started.input,
                partial_json: String::new(),
            };
            if started.index >= 0 {
                self.tools_by_index.insert(started.index, tool.id.clone());
            }
            self.tools_by_id.set(tool.id.clone(), tool.clone());
            self.emit(HarnessEvent::ToolStarted {
                agent_model: agent_model(&tool.name, &tool.input),
                call_id: tool.id.clone(),
                title: tool.title.clone(),
                kind: Some(tool_kind_from_name(&tool.name)),
                status: Some(
                    if is_agent_tool_name(&tool.name) {
                        "in_progress"
                    } else {
                        "pending"
                    }
                    .into(),
                ),
                background: None,
                preview: preview_from_tool(&tool.name, &tool.input, None),
                paths: None,
            });
            self.emit_task_list_if_needed(&tool.name, &tool.input);
            return;
        }

        if let Some(json_delta) = input_json_delta_from_event(rec) {
            if subagent {
                return;
            }
            let Some(tool_id) = self.tools_by_index.get(&json_delta.index).cloned() else {
                return;
            };
            let Some(tool) = self.tools_by_id.get_mut(&tool_id) else {
                return;
            };
            tool.partial_json.push_str(&json_delta.partial);
            let Some(parsed) = try_parse_json_record(&tool.partial_json) else {
                return;
            };
            tool.input = parsed.clone();
            tool.title = tool_title(&tool.name, &parsed);
            let tool = tool.clone();
            self.emit(HarnessEvent::ToolUpdated {
                agent_model: agent_model(&tool.name, &tool.input),
                call_id: tool.id.clone(),
                title: Some(tool.title.clone()),
                kind: Some(tool_kind_from_name(&tool.name)),
                status: Some("pending".into()),
                detail: Some(summarize_tool_request(&tool.name, &parsed)),
                preview: preview_from_tool(&tool.name, &parsed, None),
                paths: None,
            });
            self.emit_task_list_if_needed(&tool.name, &parsed);
        }
    }

    // `handleAssistant`.
    fn handle_assistant(&mut self, rec: &Record) {
        if is_subagent_message(rec) {
            self.note_subagent_narration(rec);
            for tool_use in assistant_tool_uses(rec) {
                self.note_subagent_tool(rec, &tool_use.id, &tool_use.name, &tool_use.input);
            }
            return;
        }

        if let Some(used) = context_used_from_assistant(rec) {
            self.emit(HarnessEvent::Context {
                used: Some(used),
                window: None,
            });
        }

        let snapshot = assistant_text_blocks(rec).join("");
        if !snapshot.is_empty() {
            self.close_pending_assistant_message();
        }
        let extra = snapshot_remainder(&self.emitted_assistant, &snapshot).to_string();
        if !extra.is_empty() {
            self.emitted_assistant = join_stream_text(&self.emitted_assistant, &extra);
            self.emit(HarnessEvent::MessageDelta { text: extra });
        }

        for tool_use in assistant_tool_uses(rec) {
            if let Some(streamed) = self.tools_by_id.get_mut(&tool_use.id) {
                // content_block_start often has an empty input. The input JSON
                // delta may never form a parseable object before the complete
                // assistant snapshot. Reconcile that snapshot instead of
                // leaving the tool labelled "Shell".
                // TODO(port): TypeScript compared JSON.stringify output, which
                // also saw a key order change; this compares values.
                if streamed.input != tool_use.input {
                    streamed.input = tool_use.input.clone();
                    streamed.title = tool_title(&tool_use.name, &tool_use.input);
                    let streamed = streamed.clone();
                    self.emit(HarnessEvent::ToolUpdated {
                        agent_model: agent_model(&streamed.name, &tool_use.input),
                        call_id: streamed.id.clone(),
                        title: Some(streamed.title.clone()),
                        kind: Some(tool_kind_from_name(&streamed.name)),
                        status: Some(
                            if is_agent_tool_name(&streamed.name) {
                                "in_progress"
                            } else {
                                "pending"
                            }
                            .into(),
                        ),
                        detail: None,
                        preview: preview_from_tool(&streamed.name, &tool_use.input, None),
                        paths: None,
                    });
                    self.emit_task_list_if_needed(&streamed.name, &tool_use.input);
                }
                if tool_use.name == "ExitPlanMode"
                    && let Some(plan) =
                        extract_exit_plan_mode_plan(&Value::Object(tool_use.input.clone()))
                {
                    self.emit_plan(plan);
                }
                continue;
            }
            let tool = InFlightTool {
                id: tool_use.id.clone(),
                name: tool_use.name.clone(),
                input: tool_use.input.clone(),
                partial_json: String::new(),
                title: tool_title(&tool_use.name, &tool_use.input),
            };
            self.tools_by_id.set(tool_use.id.clone(), tool.clone());
            self.emit(HarnessEvent::ToolStarted {
                agent_model: agent_model(&tool.name, &tool.input),
                call_id: tool.id.clone(),
                title: tool.title.clone(),
                kind: Some(tool_kind_from_name(&tool.name)),
                status: Some(
                    if is_agent_tool_name(&tool.name) {
                        "in_progress"
                    } else {
                        "pending"
                    }
                    .into(),
                ),
                background: None,
                preview: preview_from_tool(&tool.name, &tool.input, None),
                paths: None,
            });
            if tool_use.name == "ExitPlanMode"
                && let Some(plan) =
                    extract_exit_plan_mode_plan(&Value::Object(tool_use.input.clone()))
            {
                self.emit_plan(plan);
            }
            self.emit_task_list_if_needed(&tool.name, &tool.input);
        }

        // Each assistant record is one Claude message. Wait until the next
        // message begins to close its UI block, so a backgrounded turn stays
        // visibly live.
        self.pending_assistant_boundary =
            !snapshot.is_empty() || !self.emitted_assistant.is_empty();
        self.emitted_assistant.clear();
        self.emitted_reasoning.clear();
    }

    fn emit_plan(&self, text: String) {
        self.emit(HarnessEvent::Plan {
            text,
            key: None,
            append: None,
            streaming: None,
        });
    }

    // `closePendingAssistantMessage`.
    fn close_pending_assistant_message(&mut self) {
        if !self.pending_assistant_boundary {
            return;
        }
        self.pending_assistant_boundary = false;
        self.emit(HarnessEvent::MessageCompleted);
    }

    // `handleUser`.
    fn handle_user(&mut self, rec: &Record) {
        if is_subagent_message(rec) {
            self.note_subagent_results(rec);
            return;
        }
        let results = tool_results_from_user_message(rec);
        // Claude Code hands finished-task notices over with the next tool
        // result, so Claude has read them and no extra turn is coming for them.
        if !results.is_empty() {
            self.resume_expected = false;
        }
        for result in results {
            let Some(tool) = self.tools_by_id.get(&result.tool_use_id).cloned() else {
                continue;
            };
            let agent = is_agent_tool_name(&tool.name);
            if agent && self.is_backgrounded_agent_tool(&tool.id) {
                continue;
            }
            let mut update = tool_update(
                &tool.id,
                Some(&tool.title),
                &tool_kind_from_name(&tool.name),
                Some(if result.is_error {
                    "failed"
                } else {
                    "completed"
                }),
                non_empty(&result.text),
            );
            if let HarnessEvent::ToolUpdated { preview, .. } = &mut update {
                *preview = preview_from_tool(&tool.name, &tool.input, Some(&result.text));
            }
            self.emit(update);
            let changed = !result.is_error && {
                let mut tasks = self.claude_tasks.lock();
                apply_claude_task_tool(&mut tasks, &tool.name, &tool.input, &result.text)
            };
            if changed {
                let items: Vec<TaskListItem> = self.claude_tasks.lock().values().cloned().collect();
                self.emit(HarnessEvent::TasksUpdated {
                    key: Some(CLAUDE_TASKS_KEY.into()),
                    explanation: None,
                    merge: None,
                    // The map is the source of truth, so a TaskUpdate subject is a rename.
                    authoritative: Some(true),
                    provider_session_id: Some(self.claude_session_id.clone()),
                    items,
                });
            }
            // What a subagent hands back is the last thing it said, so it
            // closes out that agent's own trail rather than sitting on the
            // parent row as detail.
            if agent && !monocode_core::js::trim(&result.text).is_empty() && !result.is_error {
                self.emit(agent_step(
                    &tool.id,
                    &format!("{}:report", tool.id),
                    AgentStepKind::Message,
                    &result.text,
                ));
            }
            if agent {
                self.settle_inline_agent_task(&tool.id);
            }
        }
    }

    /// `settleInlineAgentTask`: a subagent that was never backgrounded
    /// reports back on the parent's own tool result, and Claude sends no task
    /// record for one that ended inline. Without this its task would keep the
    /// turn open for good: the reply reads as finished while the composer and
    /// the plan's Build button stay disabled until a restart.
    fn settle_inline_agent_task(&mut self, tool_use_id: &str) {
        let settled: Vec<String> = self
            .agent_tasks
            .iter()
            .filter(|(_, task)| {
                task.tool_use_id.as_deref() == Some(tool_use_id) && !task.backgrounded
            })
            .map(|(task_id, _)| task_id.clone())
            .collect();
        if settled.is_empty() {
            return;
        }
        for task_id in &settled {
            self.agent_tasks.delete(task_id);
            self.background_tasks.delete(task_id);
        }
        self.maybe_finish_turn();
        self.sync_background_wait();
    }

    // `handleResult`.
    fn handle_result(&mut self, rec: &Record) {
        if is_subagent_message(rec) {
            return;
        }
        // A /compact result reports the summarizer call's usage, not the
        // rebuilt conversation level. The next real turn will provide the
        // fresh reading.
        if !self.manual_compaction
            && let Some(context) = context_from_result(rec)
        {
            self.emit(HarnessEvent::Context {
                used: context.used,
                window: context.window,
            });
        }
        if let Some(metrics) = turn_metrics_from_result(rec) {
            self.emit(HarnessEvent::TurnMetrics(metrics));
        }

        let result = turn_status_from_result(rec);
        if result.status == ClaudeTurnStatus::Failed
            && let Some(error) = &result.error
            && !self.cancelled
        {
            self.emit(HarnessEvent::SessionError {
                message: error.clone(),
            });
        }
        // A refused window can still fall back to another model, so only a
        // turn that ended in error was stopped by it.
        let turn_errored = rec.get("is_error") == Some(&Value::Bool(true))
            || result.status == ClaudeTurnStatus::Failed;
        let usage_limit = self
            .usage_limit
            .take()
            .or_else(|| is_usage_limit_result(rec).then(ClaudeUsageLimit::default));
        if let Some(limit) = usage_limit
            && turn_errored
            && !self.cancelled
        {
            self.emit(HarnessEvent::UsageLimited {
                resets_at: limit.resets_at,
            });
        }
        self.turn_result_seen = true;
        if self.resume_expected {
            self.resume_expected = false;
            if self.active_turn {
                self.await_resume();
            }
        }
        self.maybe_finish_turn();
        self.show_background_rows();
        self.sync_background_wait();
    }

    // `applyKnownToolInput`.
    fn apply_known_tool_input(&mut self, tool_name: &str, input: &Record, call_id: Option<&str>) {
        let Some(call_id) = call_id else {
            return;
        };
        if input.is_empty() {
            return;
        }
        let title = tool_title(tool_name, input);
        if let Some(existing) = self.tools_by_id.get_mut(call_id) {
            existing.input = input.clone();
            existing.title = title.clone();
        }
        let mut update = tool_update(
            call_id,
            Some(&title),
            &tool_kind_from_name(tool_name),
            Some("pending"),
            None,
        );
        if let HarnessEvent::ToolUpdated { preview, .. } = &mut update {
            *preview = preview_from_tool(tool_name, input, None);
        }
        self.emit(update);
    }

    // `waitApproval`.
    fn wait_approval(
        &mut self,
        ui_id: i64,
        request_id: &str,
    ) -> oneshot::Receiver<ApprovalOutcome> {
        let (resolve, outcome) = oneshot::channel();
        self.approvals.insert(
            ui_id,
            PendingApproval {
                request_id: request_id.into(),
                resolve,
            },
        );
        outcome
    }

    // `waitQuestion`.
    fn wait_question(
        &mut self,
        ui_id: i64,
        request_id: &str,
        event: HarnessEvent,
    ) -> oneshot::Receiver<QuestionOutcome> {
        let (resolve, outcome) = oneshot::channel();
        self.questions.insert(
            ui_id,
            PendingQuestion {
                request_id: request_id.into(),
                event,
                resolve,
            },
        );
        outcome
    }

    // `showNextQuestion`.
    fn show_next_question(&mut self) {
        if self.mute_updates || self.cancelled {
            return;
        }
        if self
            .visible_question_id
            .is_some_and(|id| self.questions.contains_key(&id))
        {
            return;
        }
        let next = self
            .questions
            .iter()
            .next()
            .map(|(id, pending)| (*id, pending.event.clone()));
        self.visible_question_id = next.as_ref().map(|(id, _)| *id);
        if let Some((_, event)) = next {
            self.emit(event);
        }
    }

    // `emitTaskListIfNeeded`.
    fn emit_task_list_if_needed(&self, tool_name: &str, input: &Record) {
        if !is_todo_tool(tool_name) {
            return;
        }
        if let Some(items) = task_list_from_todos(input) {
            self.emit(HarnessEvent::TasksUpdated {
                key: None,
                explanation: None,
                merge: None,
                authoritative: None,
                provider_session_id: None,
                items,
            });
        }
    }

    // `handleAgentLifecycle`.
    fn handle_agent_lifecycle(&mut self, rec: &Record) -> bool {
        if let Some(started) = parse_task_started(rec) {
            if started.ambient {
                return true;
            }
            self.background_tasks.set(
                started.task_id.clone(),
                BackgroundTask {
                    description: started.description.clone(),
                    tool_use_id: started.tool_use_id.clone(),
                },
            );
            self.sync_background_wait();
            if !is_agent_task_type(Some(&started.task_type)) {
                return true;
            }
            self.agent_tasks.set(
                started.task_id.clone(),
                LiveAgentTask {
                    tool_use_id: started.tool_use_id.clone(),
                    description: started.description.clone(),
                    backgrounded: started.backgrounded,
                },
            );
            self.upsert_agent_tool(
                started.tool_use_id.as_deref(),
                &started.description,
                "in_progress",
                None,
            );
            return true;
        }

        if let Some(progress) = parse_task_progress(rec) {
            let task = self.agent_tasks.get(&progress.task_id).cloned();
            let title = if !progress.description.is_empty() {
                progress.description.clone()
            } else {
                task.as_ref()
                    .map(|task| task.description.clone())
                    .filter(|description| !description.is_empty())
                    .unwrap_or_else(|| "Subagent".into())
            };
            let detail = progress
                .summary
                .clone()
                .filter(|summary| !summary.is_empty())
                .or_else(|| {
                    progress
                        .last_tool_name
                        .clone()
                        .filter(|name| !name.is_empty())
                })
                .or_else(|| {
                    progress.subagent_type.as_ref().map(|subagent_type| {
                        format!("{} subagent", SEPARATORS.replace_all(subagent_type, " "))
                    })
                });
            let call_id = progress
                .tool_use_id
                .clone()
                .or_else(|| task.and_then(|task| task.tool_use_id));
            self.upsert_agent_tool(call_id.as_deref(), &title, "in_progress", detail);
            return true;
        }

        if let Some(updated) = parse_task_updated(rec) {
            if let Some(task) = self.agent_tasks.get_mut(&updated.task_id) {
                if let Some(backgrounded) = updated.backgrounded {
                    task.backgrounded = backgrounded;
                }
                if let Some(description) = &updated.description {
                    task.description = description.clone();
                }
            }
            if let Some(background) = self.background_tasks.get_mut(&updated.task_id)
                && let Some(description) = &updated.description
            {
                background.description = description.clone();
            }
            if is_terminal_agent_task_status(updated.status.as_deref()) {
                self.settle_background_row(
                    &updated.task_id,
                    updated.status.as_deref().unwrap_or("completed"),
                    updated.error.clone(),
                );
                self.finish_background_task(&updated.task_id);
                self.complete_agent_task(
                    &updated.task_id,
                    if updated.status.as_deref() == Some("completed") {
                        "completed"
                    } else {
                        "failed"
                    },
                    updated.error.clone(),
                );
            }
            return true;
        }

        if let Some(notice) = parse_task_notification(rec) {
            if !notice.ambient {
                self.note_task_notification(&notice);
                self.finish_background_task(&notice.task_id);
                self.complete_agent_task(
                    &notice.task_id,
                    if notice.status == "completed" {
                        "completed"
                    } else {
                        "failed"
                    },
                    non_empty(&notice.summary),
                );
            }
            return true;
        }

        let Some(all_tasks) = parse_background_tasks(rec) else {
            return false;
        };
        let next: HashSet<&str> = all_tasks.iter().map(|task| task.task_id.as_str()).collect();
        let gone: Vec<String> = self
            .background_tasks
            .keys()
            .filter(|id| !next.contains(id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            self.settle_background_row(&id, "completed", None);
            self.finish_background_task(&id);
        }
        for row in &all_tasks {
            if !self.background_tasks.contains(&row.task_id) {
                self.background_tasks.set(
                    row.task_id.clone(),
                    BackgroundTask {
                        description: row.description.clone(),
                        tool_use_id: None,
                    },
                );
            }
        }
        let ended_agents: Vec<String> = self
            .agent_tasks
            .keys()
            .filter(|id| !next.contains(id.as_str()))
            .cloned()
            .collect();
        for id in ended_agents {
            self.complete_agent_task(&id, "completed", None);
        }
        for row in all_tasks
            .iter()
            .filter(|task| is_agent_task_type(Some(&task.task_type)))
        {
            if self.agent_tasks.contains(&row.task_id) {
                continue;
            }
            // The list carries no tool_use_id and often lands before
            // task_started, so find the Agent call that spawned it rather than
            // opening a second row.
            let tool_use_id = self.unclaimed_agent_call(&row.description);
            self.agent_tasks.set(
                row.task_id.clone(),
                LiveAgentTask {
                    tool_use_id: tool_use_id.clone(),
                    description: row.description.clone(),
                    backgrounded: true,
                },
            );
            self.upsert_agent_tool(
                tool_use_id.as_deref(),
                &row.description,
                "in_progress",
                None,
            );
        }
        self.maybe_finish_turn();
        self.sync_background_wait();
        true
    }

    // `handleToolProgress`.
    fn handle_tool_progress(&mut self, rec: &Record) {
        let Some(progress) = parse_tool_progress(rec) else {
            return;
        };
        let tool = self
            .tools_by_id
            .get(&progress.tool_use_id)
            .cloned()
            .or_else(|| {
                progress
                    .parent_tool_use_id
                    .as_ref()
                    .and_then(|parent| self.tools_by_id.get(parent).cloned())
            });
        let Some(tool) = tool.filter(|tool| is_agent_tool_name(&tool.name)) else {
            return;
        };
        self.emit(tool_update(
            &tool.id,
            Some(&tool.title),
            "agent",
            Some("in_progress"),
            None,
        ));
        // Progress names the call in flight. That is a step in the run, not
        // the result of it, so it goes to the panel rather than onto the
        // Agent row.
        if let Some(tool_name) = &progress.tool_name {
            let mut step = agent_step(
                &tool.id,
                &progress.tool_use_id,
                AgentStepKind::Tool,
                tool_name,
            );
            if let HarnessEvent::AgentStep {
                status, agent_type, ..
            } = &mut step
            {
                *status = Some("in_progress".into());
                *agent_type = progress.subagent_type.clone();
            }
            self.emit(step);
        }
    }

    /// `subagentParent`: the Agent call a subagent message belongs to, or
    /// nothing when the message came from somewhere the parent transcript has
    /// no row for.
    fn subagent_parent(&self, rec: &Record) -> Option<InFlightTool> {
        let parent_id = string_field(Some(rec), "parent_tool_use_id")?;
        let parent = self.tools_by_id.get(parent_id)?;
        is_agent_tool_name(&parent.name).then(|| parent.clone())
    }

    /// `noteSubagentTool`: a call a subagent made, mirrored onto the Agent row
    /// that spawned it. The parent keeps its own "still running" status; the
    /// step is what the panel under that row reads back.
    fn note_subagent_tool(&self, rec: &Record, id: &str, name: &str, input: &Record) {
        let Some(parent) = self.subagent_parent(rec) else {
            return;
        };
        let title = tool_title(name, input);
        // No detail: the Agent row's detail is the report the run hands back,
        // and writing the call of the moment there would leave whatever the
        // subagent happened to do last standing in as its result.
        self.emit(tool_update(
            &parent.id,
            Some(&parent.title),
            "agent",
            Some("in_progress"),
            None,
        ));
        if id.is_empty() {
            return;
        }
        let mut step = agent_step(&parent.id, id, AgentStepKind::Tool, &title);
        if let HarnessEvent::AgentStep {
            tool_kind,
            status,
            preview,
            ..
        } = &mut step
        {
            *tool_kind = Some(tool_kind_from_name(name));
            *status = Some("in_progress".into());
            *preview = preview_from_tool(name, input, None);
        }
        self.emit(step);
    }

    /// `noteSubagentNarration`: what a subagent said and thought on its way
    /// through the work. Its prose never joins the parent transcript, since
    /// that would read as the main agent talking, but it is the most legible
    /// thing in the panel for its own row.
    fn note_subagent_narration(&self, rec: &Record) {
        let Some(parent) = self.subagent_parent(rec) else {
            return;
        };
        if let Some(model) = string_field(record_field(Some(rec), "message"), "model") {
            let mut update = tool_updated(&parent.id);
            if let HarnessEvent::ToolUpdated {
                kind, agent_model, ..
            } = &mut update
            {
                *kind = Some("agent".into());
                *agent_model = Some(model.to_string());
            }
            self.emit(update);
        }
        let message_id =
            assistant_message_id(rec).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let thinking = assistant_thinking_blocks(rec).join("");
        let thinking = monocode_core::js::trim(&thinking);
        if !thinking.is_empty() {
            self.emit(agent_step(
                &parent.id,
                &format!("{message_id}:thinking"),
                AgentStepKind::Reasoning,
                thinking,
            ));
        }
        let text = assistant_text_blocks(rec).join("");
        let text = monocode_core::js::trim(&text);
        if !text.is_empty() {
            self.emit(agent_step(
                &parent.id,
                &format!("{message_id}:text"),
                AgentStepKind::Message,
                text,
            ));
        }
    }

    /// `noteSubagentResults`: settles the subagent's own tool rows once their
    /// results come back.
    fn note_subagent_results(&self, rec: &Record) {
        let Some(parent) = self.subagent_parent(rec) else {
            return;
        };
        for result in tool_results_from_user_message(rec) {
            let mut step = agent_step(&parent.id, &result.tool_use_id, AgentStepKind::Tool, "");
            if let HarnessEvent::AgentStep { status, detail, .. } = &mut step {
                *status = Some(
                    if result.is_error {
                        "failed"
                    } else {
                        "completed"
                    }
                    .into(),
                );
                if result.is_error && !result.text.is_empty() {
                    *detail = Some(result.text.clone());
                }
            }
            self.emit(step);
        }
    }

    // `isBackgroundedAgentTool`.
    fn is_backgrounded_agent_tool(&self, tool_use_id: &str) -> bool {
        self.agent_tasks
            .values()
            .any(|task| task.tool_use_id.as_deref() == Some(tool_use_id) && task.backgrounded)
    }

    /// `unclaimedAgentCall`: the latest Agent call with this description that
    /// no task has claimed yet.
    fn unclaimed_agent_call(&self, description: &str) -> Option<String> {
        let claimed: HashSet<Option<&str>> = self
            .agent_tasks
            .values()
            .map(|task| task.tool_use_id.as_deref())
            .collect();
        let mut found = None;
        for tool in self.tools_by_id.values() {
            if !is_agent_tool_name(&tool.name) || claimed.contains(&Some(tool.id.as_str())) {
                continue;
            }
            if string_field(Some(&tool.input), "description") == Some(description) {
                found = Some(tool.id.clone());
            }
        }
        found
    }

    // `upsertAgentTool`.
    fn upsert_agent_tool(
        &mut self,
        call_id: Option<&str>,
        title: &str,
        status: &str,
        detail: Option<String>,
    ) {
        let id = call_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("agent:{title}"));
        let Some(existing) = self.tools_by_id.get_mut(&id) else {
            self.tools_by_id.set(
                id.clone(),
                InFlightTool {
                    id: id.clone(),
                    name: "Agent".into(),
                    input: Record::new(),
                    partial_json: String::new(),
                    title: title.into(),
                },
            );
            self.emit(tool_started(&id, title, "agent", status));
            if !matches!(status, "in_progress" | "pending" | "running") {
                self.emit(tool_update(&id, Some(title), "agent", Some(status), detail));
            }
            return;
        };
        if !title.is_empty() {
            existing.title = title.into();
        }
        let title = existing.title.clone();
        self.emit(tool_update(
            &id,
            Some(&title),
            "agent",
            Some(status),
            detail,
        ));
    }

    // `completeAgentTask`.
    fn complete_agent_task(&mut self, task_id: &str, status: &str, detail: Option<String>) {
        let task = self.agent_tasks.get(task_id).cloned();
        self.agent_tasks.delete(task_id);
        if let Some(task) = task {
            let detail = detail.or_else(|| (status == "failed").then(|| "Subagent failed.".into()));
            self.upsert_agent_tool(
                task.tool_use_id.as_deref(),
                &task.description,
                status,
                detail,
            );
        }
        self.maybe_finish_turn();
    }

    /// `finishBackgroundTask`: a task is done. If Claude had already yielded,
    /// the notification about it starts a follow-up turn, so hold the MonoCode
    /// turn open for that too rather than settling in the gap between the two.
    fn finish_background_task(&mut self, task_id: &str) {
        if !self.background_tasks.delete(task_id) {
            return;
        }
        if self.active_turn && !self.turn_result_seen {
            // Claude is still working. The notice reaches it with its next
            // tool result, or starts another turn once this one yields.
            self.resume_expected = true;
        } else if self.active_turn {
            self.await_resume();
        }
        self.maybe_finish_turn();
        self.sync_background_wait();
    }

    // `awaitResume`.
    fn await_resume(&mut self) {
        if self.awaiting_resume.is_some() {
            return;
        }
        let token = self.token();
        self.awaiting_resume = Some(token);
        let cell = self.cell.clone();
        let grace = self.resume_grace;
        self.spawn(async move {
            sleep(grace).await;
            let Some(cell) = cell.upgrade() else {
                return;
            };
            let mut live = cell.lock();
            if live.awaiting_resume != Some(token) {
                return;
            }
            live.awaiting_resume = None;
            live.maybe_finish_turn();
            live.sync_background_wait();
        });
    }

    /// `noteClaudeTurnStarted`: Claude began another turn inside this
    /// MonoCode turn, woken by a finished task or by a follow-up written in
    /// while it waited. Its own result, not the earlier one, decides when the
    /// MonoCode turn ends.
    fn note_claude_turn_started(&mut self) {
        if !self.active_turn || !self.turn_result_seen {
            return;
        }
        self.turn_result_seen = false;
        // A new message, not more of the last one: its snapshot must not be
        // compared against what the earlier turn streamed.
        self.emitted_assistant.clear();
        self.emitted_reasoning.clear();
        self.pending_assistant_boundary = false;
        // Close the message Claude left off with, so the reply starts its own
        // and the fold puts the earlier one away, the same as prose between
        // tool calls. A background command's row already sits between the
        // two; a subagent's report is noted in the trail to do the same.
        self.emit(HarnessEvent::MessageCompleted);
        self.emit(HarnessEvent::ReasoningCompleted);
        for text in std::mem::take(&mut self.task_notes) {
            self.emit(HarnessEvent::Status { text });
        }
        self.clear_awaiting_resume();
        self.sync_background_wait();
    }

    /// `showBackgroundRows`: Claude yielded with commands still running. Each
    /// gets a live row under the message it left off with, like any call in
    /// flight, until it finishes. Subagents already have a row of their own.
    fn show_background_rows(&mut self) {
        if !self.active_turn || self.cancelled {
            return;
        }
        let tasks: Vec<(String, BackgroundTask)> = self
            .background_tasks
            .iter()
            .map(|(id, task)| (id.clone(), task.clone()))
            .collect();
        for (task_id, task) in tasks {
            if self.background_rows.contains_key(&task_id) || self.agent_tasks.contains(&task_id) {
                continue;
            }
            let source = task
                .tool_use_id
                .as_ref()
                .and_then(|id| self.tools_by_id.get(id))
                .cloned();
            if source
                .as_ref()
                .is_some_and(|source| is_agent_tool_name(&source.name))
            {
                continue;
            }
            let call_id = format!("background:{task_id}");
            self.background_rows.insert(task_id, call_id.clone());
            self.emit(HarnessEvent::ToolStarted {
                agent_model: None,
                title: match &source {
                    Some(source) => tool_title(&source.name, &source.input),
                    None => task.description.clone(),
                },
                kind: Some(match &source {
                    Some(source) => tool_kind_from_name(&source.name),
                    None => "execute".into(),
                }),
                call_id,
                status: Some("in_progress".into()),
                background: Some(true),
                preview: source
                    .as_ref()
                    .and_then(|source| preview_from_tool(&source.name, &source.input, None)),
                paths: None,
            });
        }
    }

    // `settleBackgroundRow`.
    fn settle_background_row(&self, task_id: &str, status: &str, detail: Option<String>) {
        let Some(call_id) = self.background_rows.get(task_id) else {
            return;
        };
        if self.mute_updates {
            return;
        }
        let mut update = tool_updated(call_id);
        if let HarnessEvent::ToolUpdated {
            status: s,
            detail: d,
            ..
        } = &mut update
        {
            *s = Some(
                if status == "completed" {
                    "completed"
                } else {
                    "failed"
                }
                .into(),
            );
            *d = detail.filter(|detail| !detail.is_empty());
        }
        self.emit(update);
    }

    /// `noteTaskNotification`: what Claude was told when a task finished. A
    /// command's row takes the summary; a subagent's is kept for the trail
    /// until Claude picks back up.
    fn note_task_notification(&mut self, notice: &ClaudeAgentTaskNotification) {
        if !self.active_turn {
            return;
        }
        if self.background_rows.contains_key(&notice.task_id) {
            self.settle_background_row(&notice.task_id, &notice.status, non_empty(&notice.summary));
            return;
        }
        let tool_is_agent = notice
            .tool_use_id
            .as_ref()
            .and_then(|id| self.tools_by_id.get(id))
            .is_some_and(|tool| is_agent_tool_name(&tool.name));
        let agent = tool_is_agent || self.agent_tasks.contains(&notice.task_id);
        if !agent || !self.turn_result_seen {
            return;
        }
        self.task_notes
            .push(non_empty(&notice.summary).unwrap_or_else(|| "Subagent finished.".into()));
    }

    // `clearAwaitingResume`.
    fn clear_awaiting_resume(&mut self) {
        self.awaiting_resume = None;
    }

    /// `syncBackgroundWait`: tells the UI what the turn is waiting on once
    /// Claude has yielded with work still running, and clears it when Claude
    /// picks the thread back up.
    fn sync_background_wait(&mut self) {
        let waiting: Vec<String> = if self.active_turn && self.turn_result_seen && !self.cancelled {
            self.background_tasks
                .values()
                .map(|task| task.description.clone())
                .collect()
        } else {
            Vec::new()
        };
        let key = waiting.join("\n");
        if key == self.background_key {
            return;
        }
        self.background_key = key;
        if self.mute_updates {
            return;
        }
        self.emit(HarnessEvent::BackgroundUpdated { tasks: waiting });
    }

    // `maybeFinishTurn`.
    fn maybe_finish_turn(&mut self) {
        if !self.turn_result_seen {
            return;
        }
        if !self.agent_tasks.is_empty() || !self.background_tasks.is_empty() {
            return;
        }
        if self.awaiting_resume.is_some() {
            return;
        }
        if !self.active_turn && self.turn.is_none() {
            return;
        }
        self.finish_active_turn(&[
            HarnessEvent::MessageCompleted,
            HarnessEvent::ReasoningCompleted,
        ]);
    }

    // `finishActiveTurn`.
    fn finish_active_turn(&mut self, extra_events: &[HarnessEvent]) {
        self.clear_awaiting_resume();
        self.resume_expected = false;
        self.turn_end_pending = false;
        self.active_turn = false;
        for event in extra_events {
            self.emit(event.clone());
        }
        match self.turn.take() {
            Some(turn) => {
                let _ = turn.resolve.send(Ok(()));
            }
            // `turnDone` and `turnFailed` are set and cleared together, so
            // no resolver here means no failure handler either.
            None => self.turn_end_pending = true,
        }
    }

    // `settlePendingTurn`.
    fn settle_pending_turn(&mut self) {
        if !self.turn_end_pending || self.turn.is_none() {
            return;
        }
        self.finish_active_turn(&[]);
    }

    // `markInitialized`.
    fn mark_initialized(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if let Some(done) = self.init_done.take() {
            let _ = done.send(());
        }
    }

    /// The synchronous start of `handleControlRequest`, up to its first
    /// `await`. It returns what is left to run: a write, or a wait for the
    /// user followed by a write.
    fn begin_control_request(
        &mut self,
        control: ClaudeControlRequest,
    ) -> BoxFuture<'static, Result<()>> {
        if control.subtype != "can_use_tool" && control.subtype != "permission" {
            return self.write_json(&build_control_response(&control.request_id, json!({})));
        }

        let tool_name = control.tool_name.clone().unwrap_or_else(|| "tool".into());
        let input = control.input.clone();

        if self.cancelled || self.mute_updates {
            let write = self.write_json(&build_control_response(
                &control.request_id,
                to_claude_permission_result(ApprovalDecision::Deny, &input),
            ));
            return async move {
                let _ = write.await;
                Ok(())
            }
            .boxed();
        }

        if tool_name == "AskUserQuestion" {
            let questions = questions_from_unknown(&Value::Object(input.clone()));
            let ui_id = self.next_approval_ui_id;
            self.next_approval_ui_id += 1;
            let title = question_prompt_title(&questions);
            let title = if title.is_empty() {
                extract_ask_user_question_title(&input)
            } else {
                title
            };
            let pending = self.wait_question(
                ui_id,
                &control.request_id,
                HarnessEvent::QuestionAsked {
                    request_id: ui_id,
                    title: Some(title),
                    questions,
                    call_id: control.tool_use_id.clone(),
                    auto_resolve_at: None,
                },
            );
            self.show_next_question();
            let cell = self.cell.clone();
            let io = self.io.clone();
            let thread_id = self.thread_id.clone();
            return async move {
                // A dropped sender means the session went away; nothing answers.
                let outcome = pending.await.unwrap_or(QuestionOutcome::Cancelled);
                let decision = match &outcome {
                    QuestionOutcome::Cancelled => QuestionDecision::Cancelled,
                    QuestionOutcome::Reply(UserQuestionReply::Answered { .. }) => {
                        QuestionDecision::Answered
                    }
                    QuestionOutcome::Reply(UserQuestionReply::Skipped) => QuestionDecision::Skipped,
                };
                if let Some(cell) = cell.upgrade() {
                    let mut live = cell.lock();
                    live.emit(HarnessEvent::QuestionResolved {
                        request_id: ui_id,
                        decision,
                    });
                    live.show_next_question();
                }
                let QuestionOutcome::Reply(reply) = outcome else {
                    return Ok(());
                };
                let response = match &reply {
                    UserQuestionReply::Answered { .. } => json!({
                        "behavior": "allow",
                        "updatedInput": ask_user_question_allow_input(&input, Some(&reply)),
                    }),
                    UserQuestionReply::Skipped => json!({
                        "behavior": "deny",
                        "message": "User cancelled tool execution.",
                    }),
                };
                write_json(
                    &io,
                    &thread_id,
                    &build_control_response(&control.request_id, response),
                )
                .await
            }
            .boxed();
        }

        if tool_name == "ExitPlanMode" {
            if let Some(plan) = extract_exit_plan_mode_plan(&Value::Object(input.clone())) {
                self.emit_plan(plan);
            }
            return self.write_json(&build_control_response(
                &control.request_id,
                json!({
                    "behavior": "deny",
                    "message": "The client captured your proposed plan. Stop here and wait for the user's feedback or implementation request in a later turn.",
                }),
            ));
        }

        self.apply_known_tool_input(&tool_name, &input, control.tool_use_id.as_deref());

        if self.planning {
            let kind = tool_kind_from_name(&tool_name);
            let decision = if kind == "read" || kind == "search" {
                ApprovalDecision::Allow
            } else {
                ApprovalDecision::Deny
            };
            return self.write_json(&build_control_response(
                &control.request_id,
                to_claude_permission_result(decision, &input),
            ));
        }

        if self.runtime_mode == RuntimeMode::FullAccess {
            return self.write_json(&build_control_response(
                &control.request_id,
                to_claude_permission_result(ApprovalDecision::Allow, &input),
            ));
        }

        let ui_id = self.next_approval_ui_id;
        self.next_approval_ui_id += 1;
        let pending = self.wait_approval(ui_id, &control.request_id);
        self.emit(HarnessEvent::ApprovalRequested {
            request_id: ui_id,
            title: tool_title(&tool_name, &input),
            kind: Some(tool_kind_from_name(&tool_name)),
            call_id: control.tool_use_id.clone(),
            preview: preview_from_tool(&tool_name, &input, None),
        });
        let cell = self.cell.clone();
        let io = self.io.clone();
        let thread_id = self.thread_id.clone();
        async move {
            let decision = pending.await.unwrap_or(ApprovalOutcome::Cancelled);
            if let Some(cell) = cell.upgrade() {
                cell.lock().emit(HarnessEvent::ApprovalResolved {
                    request_id: ui_id,
                    decision: match decision {
                        ApprovalOutcome::Allow => ApprovalDecided::Allow,
                        ApprovalOutcome::Deny => ApprovalDecided::Deny,
                        ApprovalOutcome::Cancelled => ApprovalDecided::Cancelled,
                    },
                });
            }
            let decision = match decision {
                ApprovalOutcome::Cancelled => return Ok(()),
                ApprovalOutcome::Allow => ApprovalDecision::Allow,
                ApprovalOutcome::Deny => ApprovalDecision::Deny,
            };
            write_json(
                &io,
                &thread_id,
                &build_control_response(
                    &control.request_id,
                    to_claude_permission_result(decision, &input),
                ),
            )
            .await
        }
        .boxed()
    }

    /// `handleControlRequest` with the TypeScript's `.catch`: a failure that
    /// belongs to the running turn fails it, any other one is reported.
    fn handle_control_request(&mut self, control: ClaudeControlRequest) {
        let turn = self.turn_token();
        let work = self.begin_control_request(control);
        let cell = self.cell.clone();
        self.spawn(async move {
            let Err(error) = work.await else {
                return;
            };
            let Some(cell) = cell.upgrade() else {
                return;
            };
            let mut live = cell.lock();
            if live.mute_updates || live.turn_token() != turn {
                return;
            }
            let message = format!("{error:#}");
            match live.turn.take() {
                Some(waiter) => {
                    let _ = waiter.resolve.send(Err(message));
                }
                None => live.emit(HarnessEvent::SessionError { message }),
            }
        });
    }
}

fn write_json(
    io: &SharedChildIo,
    child_id: &str,
    payload: &Value,
) -> BoxFuture<'static, Result<()>> {
    let line = serde_json::to_string(payload).unwrap_or_default();
    io.write_child(child_id, line)
}

impl ClaudeSessions {
    pub fn new(io: SharedChildIo, spawner: SharedSpawner, options: ClaudeSessionOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                io,
                spawner,
                options,
                globals: Mutex::new(Globals::default()),
            }),
        }
    }

    /// `settingsKeyFor`.
    fn settings_key_for(&self, input: &HarnessSessionInput) -> String {
        let settings = input.model_settings.as_ref();
        let get = |key: &str| {
            settings
                .and_then(|settings| settings.get(key))
                .map(String::as_str)
        };
        let model = (self.inner.options.native_model_id)(&input.model);
        format!(
            "{}:{}",
            input.provider_account_id.as_deref().unwrap_or("default"),
            claude_settings_key(&ClaudeSettingsKeyInput {
                model: &model,
                effort: get("effort"),
                fast: get("fast"),
                thinking: get("thinking"),
                context: get("context"),
                runtime_mode: input.runtime_mode,
                hooks: Some((self.inner.options.claude_hooks)()),
            })
        )
    }

    /// `launchOptions`.
    fn launch_options(
        &self,
        input: &HarnessSessionInput,
        resume: Option<String>,
        session_id: &str,
    ) -> ClaudeSpawnOptions {
        let native = (self.inner.options.native_model_id)(&input.model);
        let get = |key: &str| {
            input
                .model_settings
                .as_ref()
                .and_then(|settings| settings.get(key))
                .map(String::as_str)
        };
        let effort_raw = get("effort");
        let mut settings = ClaudeCliSettings::default();
        if get("thinking") == Some("true") {
            settings.always_thinking_enabled = Some(true);
        }
        if get("fast") == Some("true") {
            settings.fast_mode = Some(true);
        }
        if is_claude_ultracode_effort(effort_raw) {
            settings.ultracode = Some(true);
        }
        if !(self.inner.options.claude_hooks)() {
            settings.disable_all_hooks = Some(true);
        }
        ClaudeSpawnOptions {
            model: Some(resolve_claude_api_model_id(&native, get("context"))),
            effort: normalize_claude_cli_effort(effort_raw, Some(&native)),
            permission_mode: Some(if input.intent == Some(TurnIntent::Plan) {
                ClaudePermissionMode::Plan
            } else {
                runtime_mode_to_permission(input.runtime_mode)
            }),
            session_id: if resume.is_some() {
                None
            } else {
                Some(session_id.to_string())
            },
            resume,
            settings: (!settings.is_empty()).then_some(settings),
            ..Default::default()
        }
    }

    /// `sendClaudeTurn`.
    pub async fn send_turn(&self, input: SendTurnInput, on_event: EventSink) -> Result<()> {
        let thread_id = input.session.session_id.clone();
        let cell = match self.ensure_live(&input.session, on_event.clone()).await {
            Ok(cell) => cell,
            Err(error) => {
                self.inner
                    .globals
                    .lock()
                    .cancelled_threads
                    .remove(&thread_id);
                return Err(error);
            }
        };
        if self
            .inner
            .globals
            .lock()
            .cancelled_threads
            .remove(&thread_id)
        {
            return Ok(());
        }
        {
            let mut live = cell.lock();
            live.on_event = on_event;
            live.runtime_mode = input.session.runtime_mode;
        }
        let _turns = cell.turns.lock().await;
        {
            let mut live = cell.lock();
            live.cancelled = false;
            live.mute_updates = false;
        }
        let effort = input
            .session
            .model_settings
            .as_ref()
            .and_then(|settings| settings.get("effort"))
            .cloned();
        let attachments = input.attachments.clone().unwrap_or_default();
        match run_turn(&cell, &input.text, &attachments, effort.as_deref()).await {
            Err(_) if cell.lock().cancelled => Ok(()),
            result => result,
        }
    }

    /// `compactClaudeContext`.
    pub async fn compact_context(
        &self,
        input: HarnessSessionInput,
        on_event: EventSink,
    ) -> Result<()> {
        let settings_key = self.settings_key_for(&input);
        let existing = self
            .inner
            .globals
            .lock()
            .live_by_thread
            .get(&input.session_id)
            .cloned();
        let reuse = existing.filter(|cell| {
            let live = cell.lock();
            live.cwd == input.cwd && live.settings_key == settings_key
        });
        let cell = match reuse {
            Some(cell) => cell,
            None => self.ensure_live(&input, on_event.clone()).await?,
        };
        if self
            .inner
            .globals
            .lock()
            .cancelled_threads
            .remove(&input.session_id)
        {
            return Ok(());
        }
        {
            let mut live = cell.lock();
            live.on_event = on_event;
            live.runtime_mode = input.runtime_mode;
        }
        let _turns = cell.turns.lock().await;
        {
            let mut live = cell.lock();
            live.cancelled = false;
            live.mute_updates = false;
            live.manual_compaction = true;
            live.compaction_confirmed = false;
        }
        let result = match run_turn(&cell, "/compact", &[], None).await {
            Ok(()) if !cell.lock().compaction_confirmed => {
                Err(anyhow!("Claude Code did not confirm context compaction"))
            }
            result => result,
        };
        let mut live = cell.lock();
        live.manual_compaction = false;
        match result {
            Err(_) if live.cancelled => Ok(()),
            result => result,
        }
    }

    /// `steerClaudeTurn`.
    pub async fn steer_turn(&self, input: SteerTurnInput) -> Result<()> {
        let live = self
            .inner
            .globals
            .lock()
            .live_by_thread
            .get(&input.session_id)
            .cloned();
        if !live.is_some_and(|cell| cell.lock().active_turn) {
            bail!("No active turn to steer");
        }
        let effort = input
            .model_settings
            .as_ref()
            .and_then(|settings| settings.get("effort"))
            .map(String::as_str);
        let message = build_claude_user_message(
            &input.text,
            input.attachments.as_deref().unwrap_or(&[]),
            effort,
        )
        .map_err(|error| anyhow!(error))?;
        if user_message_content(&message).is_empty() {
            return Ok(());
        }
        write_json(&self.inner.io, &input.session_id, &message).await
    }

    fn live(&self, session_id: &str) -> Option<LiveRef> {
        self.inner
            .globals
            .lock()
            .live_by_thread
            .get(session_id)
            .cloned()
    }

    /// `respondClaudeApproval`.
    pub fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        let Some(cell) = self.live(session_id) else {
            return;
        };
        if let Some(pending) = cell.lock().approvals.remove(&request_id) {
            let _ = pending.resolve.send(decision.into());
        }
    }

    /// `respondClaudeQuestion`.
    pub fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        let Some(cell) = self.live(session_id) else {
            return;
        };
        if let Some(pending) = cell.lock().questions.remove(&request_id) {
            let _ = pending.resolve.send(QuestionOutcome::Reply(reply));
        }
    }

    /// `cancelClaudeTurn`.
    pub async fn cancel_turn(&self, session_id: &str) -> Result<()> {
        let cell = {
            let mut globals = self.inner.globals.lock();
            match globals.live_by_thread.get(session_id).cloned() {
                Some(cell) => cell,
                None => {
                    globals.cancelled_threads.insert(session_id.to_string());
                    return Ok(());
                }
            }
        };
        let task_ids: Vec<String> = {
            let mut live = cell.lock();
            live.cancelled = true;
            live.mute_updates = true;
            live.deny_all_pending();
            live.background_tasks.keys().cloned().collect()
        };
        // Stop means the whole run, including what Claude left going in the
        // background. Otherwise it finishes later and wakes Claude up again.
        for task_id in task_ids {
            let write = {
                let mut live = cell.lock();
                let id = live.next_control_id();
                live.write_json(&build_control_request(
                    &id,
                    json!({ "subtype": "stop_task", "task_id": task_id }),
                ))
            };
            let _ = write.await;
        }
        let write = {
            let mut live = cell.lock();
            let id = live.next_control_id();
            live.write_json(&build_control_request(
                &id,
                json!({ "subtype": "interrupt" }),
            ))
        };
        let _ = write.await;
        cell.lock().finish_active_turn(&[
            HarnessEvent::MessageCompleted,
            HarnessEvent::ReasoningCompleted,
        ]);
        Ok(())
    }

    /// `stopClaudeSession`.
    pub async fn stop_session(&self, session_id: &str) -> Result<()> {
        let cell = {
            let mut globals = self.inner.globals.lock();
            globals.cancelled_threads.remove(session_id);
            globals.live_by_thread.remove(session_id)
        };
        if let Some(cell) = cell {
            let mut live = cell.lock();
            live.mute_updates = true;
            live.deny_all_pending();
            live.active_turn = false;
            if let Some(turn) = live.turn.take() {
                let _ = turn.resolve.send(Ok(()));
            }
            if let Some(done) = live.init_done.take() {
                let _ = done.send(());
            }
        }
        self.inner.io.unwatch_child(session_id);
        let _ = self.inner.io.kill_child(session_id).await;
        Ok(())
    }

    /// `forgetClaudeSession`.
    pub async fn forget_session(&self, session_id: &str) -> Result<()> {
        {
            let mut globals = self.inner.globals.lock();
            globals.resume_by_thread.remove(session_id);
            globals.tasks_by_thread.remove(session_id);
        }
        self.stop_session(session_id).await
    }

    /// `bindClaudeSession`.
    pub fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        provider_account_id: Option<&str>,
    ) {
        let session_id = monocode_core::js::trim(provider_session_id);
        if thread_id.is_empty() || session_id.is_empty() || monocode_core::js::trim(cwd).is_empty()
        {
            return;
        }
        let mut globals = self.inner.globals.lock();
        globals.resume_by_thread.insert(
            thread_id.to_string(),
            Resume {
                session_id: session_id.to_string(),
                cwd: cwd.to_string(),
                provider_account_id: provider_account_id.map(str::to_string),
            },
        );
        // Task ids from another conversation mean nothing in this one.
        if globals
            .tasks_by_thread
            .get(thread_id)
            .is_none_or(|retained| retained.provider_session_id != session_id)
        {
            globals.tasks_by_thread.remove(thread_id);
        }
    }

    /// `restoreClaudeTaskLists`: seed the task map from a restored session's
    /// persisted panel. After an app restart only the transcript survives, and
    /// a resumed conversation still refers to its earlier task ids.
    pub fn restore_task_lists(&self, thread_id: &str, lists: &[TaskListMeta]) {
        let mut globals = self.inner.globals.lock();
        if thread_id.is_empty() || globals.tasks_by_thread.contains_key(thread_id) {
            return;
        }
        // Only a list produced by the conversation bound to this thread applies.
        let Some(provider_session_id) = globals
            .resume_by_thread
            .get(thread_id)
            .map(|resume| resume.session_id.clone())
        else {
            return;
        };
        let mut items: Vec<TaskListItem> = Vec::new();
        for entry in lists {
            if entry.key.as_deref() != Some(CLAUDE_TASKS_KEY) {
                continue;
            }
            if entry.provider_session_id.as_deref() != Some(provider_session_id.as_str()) {
                continue;
            }
            items = entry
                .items
                .iter()
                .filter(|item| item.id.as_deref().is_some_and(|id| !id.is_empty()))
                .cloned()
                .collect();
        }
        if items.is_empty() {
            return;
        }
        let tasks: ClaudeTaskMap = items
            .into_iter()
            .map(|item| (item.id.clone().unwrap_or_default(), item))
            .collect();
        globals.tasks_by_thread.insert(
            thread_id.to_string(),
            RetainedTasks {
                provider_session_id,
                tasks: Arc::new(Mutex::new(tasks)),
            },
        );
    }

    /// `ensureLive`.
    async fn ensure_live(
        &self,
        input: &HarnessSessionInput,
        on_event: EventSink,
    ) -> Result<LiveRef> {
        let thread_id = input.session_id.clone();
        let settings_key = self.settings_key_for(input);
        let planning = input.intent == Some(TurnIntent::Plan);
        let existing = self.live(&thread_id);
        if let Some(existing) = &existing {
            let mut live = existing.lock();
            if live.cwd == input.cwd
                && live.settings_key == settings_key
                && live.planning == planning
            {
                live.on_event = on_event;
                live.runtime_mode = input.runtime_mode;
                drop(live);
                return Ok(existing.clone());
            }
        }
        if let Some(existing) = existing {
            // Model and launch-setting changes require a fresh Claude process,
            // but they must resume the same provider conversation. Only a cwd
            // change invalidates the stored session because Claude sessions
            // are cwd-bound.
            if existing.lock().cwd != input.cwd {
                self.inner
                    .globals
                    .lock()
                    .resume_by_thread
                    .remove(&thread_id);
            }
            self.stop_session(&thread_id).await?;
        }

        let resume = {
            let mut globals = self.inner.globals.lock();
            let resume = globals.resume_by_thread.get(&thread_id).cloned();
            let usable = resume.filter(|resume| {
                resume.cwd == input.cwd
                    && same_provider_account_id(
                        resume.provider_account_id.as_deref(),
                        input.provider_account_id.as_deref(),
                    )
            });
            if usable.is_none() {
                globals.resume_by_thread.remove(&thread_id);
            }
            usable
        };
        let path = self.inner.io.resolve_claude_binary().await?;
        let claude_session_id = match &resume {
            Some(resume) => resume.session_id.clone(),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let claude_tasks = {
            let mut globals = self.inner.globals.lock();
            let tasks = globals
                .tasks_by_thread
                .get(&thread_id)
                .filter(|retained| retained.provider_session_id == claude_session_id)
                .map(|retained| retained.tasks.clone())
                .unwrap_or_default();
            globals.tasks_by_thread.insert(
                thread_id.clone(),
                RetainedTasks {
                    provider_session_id: claude_session_id.clone(),
                    tasks: tasks.clone(),
                },
            );
            tasks
        };
        let launch = self.launch_options(
            input,
            resume.as_ref().map(|resume| resume.session_id.clone()),
            &claude_session_id,
        );

        let outbox = Arc::new(Outbox::default());
        let cell: LiveRef = Arc::new_cyclic(|cell| LiveCell {
            state: Mutex::new(Live {
                thread_id: thread_id.clone(),
                cwd: input.cwd.clone(),
                claude_session_id: claude_session_id.clone(),
                provider_account_id: input.provider_account_id.clone(),
                runtime_mode: input.runtime_mode,
                planning,
                settings_key,
                on_event,
                approvals: BTreeMap::new(),
                questions: BTreeMap::new(),
                visible_question_id: None,
                next_approval_ui_id: 1,
                next_control_id: 1,
                tools_by_index: HashMap::new(),
                tools_by_id: OrderedMap::new(),
                agent_tasks: OrderedMap::new(),
                background_tasks: OrderedMap::new(),
                background_rows: HashMap::new(),
                awaiting_resume: None,
                resume_expected: false,
                background_key: String::new(),
                task_notes: Vec::new(),
                claude_tasks,
                turn_result_seen: false,
                usage_limit: None,
                cancelled: false,
                mute_updates: false,
                turn: None,
                turn_end_pending: false,
                active_turn: false,
                init_done: None,
                initialized: false,
                emitted_assistant: String::new(),
                emitted_reasoning: String::new(),
                pending_assistant_boundary: false,
                manual_compaction: false,
                compaction_confirmed: false,
                next_token: 0,
                io: self.inner.io.clone(),
                spawner: self.inner.spawner.clone(),
                resume_grace: self.inner.options.resume_grace,
                cell: cell.clone(),
                outbox: outbox.clone(),
            }),
            outbox,
            turns: futures::lock::Mutex::new(()),
        });

        let on_line = {
            let sessions = Arc::downgrade(&self.inner);
            let cell = cell.clone();
            let thread_id = thread_id.clone();
            Arc::new(move |line: String| {
                if let Some(inner) = sessions.upgrade() {
                    handle_line(&inner, &thread_id, &cell, &line);
                }
            })
        };
        let on_exit = {
            let sessions = Arc::downgrade(&self.inner);
            let cell = cell.clone();
            let thread_id = thread_id.clone();
            Arc::new(move |code: Option<i64>| {
                if let Some(inner) = sessions.upgrade() {
                    inner.globals.lock().live_by_thread.remove(&thread_id);
                }
                let mut live = cell.lock();
                if !live.mute_updates {
                    live.emit(HarnessEvent::SessionEnded { code });
                }
                if let Some(turn) = live.turn.take() {
                    let _ = turn.resolve.send(Err("Claude Code exited".into()));
                }
                if let Some(done) = live.init_done.take() {
                    let _ = done.send(());
                }
            })
        };
        self.inner.io.watch_child(&thread_id, on_line, on_exit);

        self.inner
            .io
            .spawn_child(
                &thread_id,
                &path,
                build_claude_spawn_args(&launch),
                &input.cwd,
                Some(claude_account(input.provider_account_id.as_deref())),
            )
            .await?;

        {
            let mut globals = self.inner.globals.lock();
            globals
                .live_by_thread
                .insert(thread_id.clone(), cell.clone());
            globals.resume_by_thread.insert(
                thread_id.clone(),
                Resume {
                    session_id: claude_session_id,
                    cwd: input.cwd.clone(),
                    provider_account_id: input.provider_account_id.clone(),
                },
            );
        }

        let started = async {
            let write = {
                let mut live = cell.lock();
                let id = live.next_control_id();
                live.write_json(&build_control_request(
                    &id,
                    json!({ "subtype": "initialize" }),
                ))
            };
            write.await?;
            wait_for_init(&cell, self.inner.options.init_timeout).await;
            let live = cell.lock();
            live.emit(HarnessEvent::SessionProviderBound {
                provider_session_id: live.claude_session_id.clone(),
            });
            live.emit(HarnessEvent::SessionStarted);
            Ok::<(), anyhow::Error>(())
        };
        match started.await {
            Ok(()) => Ok(cell),
            Err(error) => {
                self.stop_session(&thread_id).await?;
                Err(error)
            }
        }
    }

    /// `__claudeTestReset`.
    pub fn reset_for_tests(&self) {
        *self.inner.globals.lock() = Globals::default();
    }
}

/// `waitForInit`: resolves when the child reports it is ready, or after the
/// timeout either way.
async fn wait_for_init(cell: &LiveRef, wait: Duration) {
    let ready = {
        let mut live = cell.lock();
        if live.initialized {
            return;
        }
        let (done, ready) = oneshot::channel();
        live.init_done = Some(done);
        ready
    };
    if timeout(wait, ready).await.is_none() {
        cell.lock().init_done = None;
    }
}

/// `runTurn`.
async fn run_turn(
    cell: &LiveRef,
    text: &str,
    attachments: &[Attachment],
    effort: Option<&str>,
) -> Result<()> {
    let message =
        build_claude_user_message(text, attachments, effort).map_err(|error| anyhow!(error))?;
    if user_message_content(&message).is_empty() {
        return Ok(());
    }

    let (done, write) = {
        let mut live = cell.lock();
        live.emitted_assistant.clear();
        live.emitted_reasoning.clear();
        live.pending_assistant_boundary = false;
        live.tools_by_index.clear();
        live.tools_by_id.clear();
        live.agent_tasks.clear();
        live.background_tasks.clear();
        live.background_rows.clear();
        live.clear_awaiting_resume();
        live.resume_expected = false;
        live.background_key.clear();
        live.task_notes.clear();
        live.turn_result_seen = false;

        let (resolve, done) = oneshot::channel();
        let token = live.token();
        live.turn = Some(TurnWaiter { token, resolve });
        live.active_turn = true;
        live.settle_pending_turn();
        (done, live.write_json(&message))
    };

    let outcome = match write.await {
        Ok(()) => {
            cell.lock().settle_pending_turn();
            // A dropped resolver means the session went away without a verdict.
            done.await.unwrap_or(Ok(())).map_err(|error| anyhow!(error))
        }
        Err(error) => Err(error),
    };
    let mut live = cell.lock();
    live.turn = None;
    match outcome {
        Err(_) if live.cancelled => Ok(()),
        Err(error) => {
            live.emit(HarnessEvent::SessionError {
                message: format!("{error:#}"),
            });
            Err(error)
        }
        Ok(()) => Ok(()),
    }
}

/// A provider conversation change seen on a line, applied to the sessions
/// map once the live session is unlocked.
struct Rebind {
    provider_session_id: String,
    tasks: SharedTasks,
    resume: Resume,
}

/// `handleLine`.
fn handle_line(inner: &Inner, thread_id: &str, cell: &LiveRef, line: &str) {
    let Some(rec) = parse_json_line(line) else {
        return;
    };
    let mut rebind = None;
    handle_record(&mut cell.lock(), &rec, &mut rebind);
    if let Some(rebind) = rebind {
        let mut globals = inner.globals.lock();
        globals.tasks_by_thread.insert(
            thread_id.to_string(),
            RetainedTasks {
                provider_session_id: rebind.provider_session_id,
                tasks: rebind.tasks,
            },
        );
        globals
            .resume_by_thread
            .insert(thread_id.to_string(), rebind.resume);
    }
}

fn handle_record(live: &mut Live, rec: &Record, rebind: &mut Option<Rebind>) {
    let kind = string_field(Some(rec), "type");
    if kind == Some("keep_alive") {
        return;
    }

    if let Some(cancel_id) = parse_control_cancel_id(rec) {
        let approvals: Vec<i64> = live
            .approvals
            .iter()
            .filter(|(_, pending)| pending.request_id == cancel_id)
            .map(|(id, _)| *id)
            .collect();
        for ui_id in approvals {
            if let Some(pending) = live.approvals.remove(&ui_id) {
                let _ = pending.resolve.send(ApprovalOutcome::Cancelled);
            }
        }
        let questions: Vec<i64> = live
            .questions
            .iter()
            .filter(|(_, pending)| pending.request_id == cancel_id)
            .map(|(id, _)| *id)
            .collect();
        for ui_id in questions {
            if let Some(pending) = live.questions.remove(&ui_id) {
                let _ = pending.resolve.send(QuestionOutcome::Cancelled);
            }
        }
        return;
    }

    if let Some(control) = parse_control_request(rec) {
        live.handle_control_request(control);
        return;
    }

    if live.mute_updates {
        return;
    }

    if let Some(session_id) = session_id_from_message(rec)
        && session_id != live.claude_session_id
    {
        live.claude_session_id = session_id.clone();
        // A different conversation starts with its own task ids.
        live.claude_tasks = Arc::new(Mutex::new(ClaudeTaskMap::new()));
        *rebind = Some(Rebind {
            provider_session_id: session_id.clone(),
            tasks: live.claude_tasks.clone(),
            resume: Resume {
                session_id: session_id.clone(),
                cwd: live.cwd.clone(),
                provider_account_id: live.provider_account_id.clone(),
            },
        });
        live.emit(HarnessEvent::SessionProviderBound {
            provider_session_id: session_id,
        });
    }

    let subtype = string_field(Some(rec), "subtype").unwrap_or("");
    if kind == Some("system") && (subtype == "init" || subtype == "initialized") {
        live.mark_initialized();
        if subtype == "init" {
            live.note_claude_turn_started();
        }
    }

    if kind == Some("control_response") {
        live.mark_initialized();
        return;
    }

    if live.manual_compaction && kind != Some("system") && kind != Some("result") {
        return;
    }

    if live.handle_agent_lifecycle(rec) {
        return;
    }
    match kind {
        Some("tool_progress") => live.handle_tool_progress(rec),
        Some("stream_event") => {
            if !is_subagent_message(rec) {
                live.note_claude_turn_started();
            }
            live.handle_stream_event(rec);
        }
        Some("assistant") => {
            if !is_subagent_message(rec) {
                live.note_claude_turn_started();
            }
            live.handle_assistant(rec);
        }
        Some("user") => live.handle_user(rec),
        Some("result") => live.handle_result(rec),
        Some("rate_limit_event") => live.usage_limit = usage_limit_from_rate_limit_event(rec),
        Some("system") => {
            if let Some(text) = status_text_from_system(rec) {
                if subtype.starts_with("compact") {
                    live.compaction_confirmed = true;
                }
                live.emit(HarnessEvent::Status { text });
            }
        }
        _ => {}
    }
}
