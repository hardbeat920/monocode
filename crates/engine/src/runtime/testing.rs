//! Test doubles for the runtime and for other engine packages' tests: an
//! in-memory backend that records every store command (the TypeScript
//! tests' `invoke` mock), gates that hold a command until released, and a
//! scriptable workspace hook.
//!
//! Enable with the `test-support` feature from another crate.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::{App, Task, TestAppContext};
use monocode_core::Session;
use monocode_store::checkpoint::{CheckpointApplyResult, CheckpointFileDiff, CheckpointStatus};
use monocode_store::session_store::{
    InFlightSession, SessionRecord, SessionSearchOptions, SessionSearchResult,
    SessionSummary as StoredSummary, SessionUpsert,
};
use parking_lot::Mutex;
use serde_json::Value;

use super::backend::{CheckpointBackend, SessionBackend, StoreFuture};
use super::engine::{Engine, EngineConfig};
use super::hooks::{EngineHooks, WorkspaceHooks};
use super::in_flight::ResumedWorkspace;

/// Releases one held store command.
pub struct Gate(Option<oneshot::Sender<()>>);

impl Gate {
    pub fn release(mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[derive(Default)]
struct FakeState {
    commands: Vec<String>,
    calls: Vec<(String, Value)>,
    records: HashMap<String, SessionRecord>,
    workspace_snapshot: Option<Value>,
    in_flight: Vec<InFlightSession>,
    failing: HashSet<String>,
    gates: HashMap<String, VecDeque<oneshot::Receiver<()>>>,
    shell_commands: HashMap<String, String>,
}

/// An in-memory `SessionBackend` and `CheckpointBackend`.
#[derive(Default)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

impl FakeBackend {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Command names in call order, like `invoke.mock.calls.map(c => c[0])`.
    pub fn commands(&self) -> Vec<String> {
        self.state.lock().commands.clone()
    }

    /// Every call with its arguments as JSON.
    pub fn calls(&self, command: &str) -> Vec<Value> {
        self.state
            .lock()
            .calls
            .iter()
            .filter(|(name, _)| name == command)
            .map(|(_, args)| args.clone())
            .collect()
    }

    pub fn clear_calls(&self) {
        let mut state = self.state.lock();
        state.commands.clear();
        state.calls.clear();
    }

    /// The next call of `command` waits until the returned gate is released.
    pub fn hold_next(&self, command: &str) -> Gate {
        let (sender, receiver) = oneshot::channel();
        self.state
            .lock()
            .gates
            .entry(command.to_string())
            .or_default()
            .push_back(receiver);
        Gate(Some(sender))
    }

    /// Make every call of `command` fail (`mockRejectedValue`).
    pub fn set_failing(&self, command: &str, failing: bool) {
        let mut state = self.state.lock();
        if failing {
            state.failing.insert(command.to_string());
        } else {
            state.failing.remove(command);
        }
    }

    /// A stored row `session_get` returns.
    pub fn insert_record(&self, record: SessionRecord) {
        self.state.lock().records.insert(record.id.clone(), record);
    }

    /// Store `session` as `session_upsert` would.
    pub fn insert_session(&self, session: &Session) {
        let payload = super::session_store::sanitize_session_for_persist(session);
        let record = record_from_upsert(&payload);
        self.insert_record(record);
    }

    pub fn record(&self, session_id: &str) -> Option<SessionRecord> {
        self.state.lock().records.get(session_id).cloned()
    }

    pub fn set_workspace_snapshot(&self, snapshot: Option<Value>) {
        self.state.lock().workspace_snapshot = snapshot;
    }

    pub fn workspace_snapshot(&self) -> Option<Value> {
        self.state.lock().workspace_snapshot.clone()
    }

    pub fn set_in_flight(&self, refs: Vec<(&str, &str)>) {
        self.state.lock().in_flight = refs
            .into_iter()
            .map(|(id, cwd)| InFlightSession {
                session_id: id.into(),
                cwd: cwd.into(),
            })
            .collect();
    }

    pub fn in_flight(&self) -> Vec<(String, String)> {
        self.state
            .lock()
            .in_flight
            .iter()
            .map(|entry| (entry.session_id.clone(), entry.cwd.clone()))
            .collect()
    }

    /// Bash commands `claude_shell_commands` returns, by tool-use id.
    pub fn set_shell_commands(&self, commands: HashMap<String, String>) {
        self.state.lock().shell_commands = commands;
    }

    fn call<T: Send + 'static>(
        &self,
        command: &str,
        args: Value,
        result: impl FnOnce(&mut FakeState) -> Result<T, String> + Send + 'static,
    ) -> StoreFuture<T> {
        let (gate, failing) = {
            let mut state = self.state.lock();
            state.commands.push(command.to_string());
            state.calls.push((command.to_string(), args));
            let gate = state.gates.get_mut(command).and_then(VecDeque::pop_front);
            (gate, state.failing.contains(command))
        };
        let state = self.state.clone();
        let command = command.to_string();
        async move {
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            if failing {
                return Err(format!("{command} failed"));
            }
            let mut state = state.lock();
            result(&mut state)
        }
        .boxed()
    }
}

fn summary_from_upsert(payload: &SessionUpsert) -> StoredSummary {
    StoredSummary {
        id: payload.id.clone(),
        orchestration_lead_id: None,
        orchestration: None,
        cwd: payload.cwd.clone(),
        harness: payload.harness.clone(),
        model: payload.model.clone(),
        runtime_mode: payload.runtime_mode.clone(),
        title: payload.title.clone(),
        provider_session_id: payload.provider_session_id.clone(),
        branch: payload.branch.clone(),
        worktree_cwd: payload.worktree_cwd.clone(),
        worktree_removed: payload.worktree_removed,
        repo: None,
        additions: 0,
        deletions: 0,
        created_at: 1,
        updated_at: 1,
        archived: false,
        pinned: false,
        draft: false,
        linked_work_item: payload.linked_work_item.clone(),
        automation_id: payload.automation_id.clone(),
    }
}

fn record_from_upsert(payload: &SessionUpsert) -> SessionRecord {
    SessionRecord {
        id: payload.id.clone(),
        orchestration_lead_id: None,
        cwd: payload.cwd.clone(),
        harness: payload.harness.clone(),
        model: payload.model.clone(),
        model_settings: payload.model_settings.clone(),
        runtime_mode: payload.runtime_mode.clone(),
        title: payload.title.clone(),
        provider_session_id: payload.provider_session_id.clone(),
        provider_account_id: payload.provider_account_id.clone(),
        blocks: payload.blocks.clone(),
        context_used: payload.context_used,
        context_window: payload.context_window,
        branch: payload.branch.clone(),
        worktree_cwd: payload.worktree_cwd.clone(),
        worktree_removed: payload.worktree_removed,
        linked_work_item: payload.linked_work_item.clone(),
        automation_id: payload.automation_id.clone(),
        created_at: 1,
        updated_at: 1,
    }
}

fn args<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

impl SessionBackend for FakeBackend {
    fn upsert(&self, session: SessionUpsert) -> StoreFuture<StoredSummary> {
        self.call(
            "session_upsert",
            serde_json::json!({ "session": args(&session) }),
            move |state| {
                let summary = summary_from_upsert(&session);
                state
                    .records
                    .insert(session.id.clone(), record_from_upsert(&session));
                Ok(summary)
            },
        )
    }

    fn get(&self, session_id: String) -> StoreFuture<Option<SessionRecord>> {
        self.call(
            "session_get",
            serde_json::json!({ "sessionId": session_id }),
            move |state| Ok(state.records.get(&session_id).cloned()),
        )
    }

    fn list_by_project(&self, cwd: String) -> StoreFuture<Vec<StoredSummary>> {
        self.call(
            "session_list_by_project",
            serde_json::json!({ "cwd": cwd }),
            move |state| {
                Ok(state
                    .records
                    .values()
                    .filter(|record| record.cwd == cwd)
                    .map(|record| StoredSummary {
                        id: record.id.clone(),
                        orchestration_lead_id: None,
                        orchestration: None,
                        cwd: record.cwd.clone(),
                        harness: record.harness.clone(),
                        model: record.model.clone(),
                        runtime_mode: record.runtime_mode.clone(),
                        title: record.title.clone(),
                        provider_session_id: record.provider_session_id.clone(),
                        branch: record.branch.clone(),
                        worktree_cwd: record.worktree_cwd.clone(),
                        worktree_removed: record.worktree_removed,
                        repo: None,
                        additions: 0,
                        deletions: 0,
                        created_at: record.created_at,
                        updated_at: record.updated_at,
                        archived: false,
                        pinned: false,
                        draft: false,
                        linked_work_item: record.linked_work_item.clone(),
                        automation_id: record.automation_id.clone(),
                    })
                    .collect())
            },
        )
    }

    fn rebase_project(&self, from_cwd: String, to_cwd: String) -> StoreFuture<()> {
        self.call(
            "session_rebase_project",
            serde_json::json!({ "fromCwd": from_cwd, "toCwd": to_cwd }),
            |_| Ok(()),
        )
    }

    fn list_linked(&self) -> StoreFuture<Vec<StoredSummary>> {
        self.call("session_list_linked", Value::Null, |_| Ok(Vec::new()))
    }

    fn search(&self, options: SessionSearchOptions) -> StoreFuture<SessionSearchResult> {
        self.call(
            "session_search",
            serde_json::json!({ "query": options.query, "cwd": options.cwd }),
            |_| {
                Ok(SessionSearchResult {
                    hits: Vec::new(),
                    truncated: false,
                })
            },
        )
    }

    fn cancel_search(&self, search_owner: String) -> StoreFuture<()> {
        self.call(
            "cancel_session_search",
            serde_json::json!({ "searchOwner": search_owner }),
            |_| Ok(()),
        )
    }

    fn delete(&self, session_id: String, image_paths: Vec<String>) -> StoreFuture<()> {
        self.call(
            "session_delete",
            serde_json::json!({ "sessionId": session_id, "imagePaths": image_paths }),
            move |state| {
                state.records.remove(&session_id);
                Ok(())
            },
        )
    }

    fn set_archived(&self, session_id: String, archived: bool) -> StoreFuture<()> {
        self.call(
            "session_set_archived",
            serde_json::json!({ "sessionId": session_id, "archived": archived }),
            |_| Ok(()),
        )
    }

    fn set_pinned(&self, session_id: String, pinned: bool) -> StoreFuture<()> {
        self.call(
            "session_set_pinned",
            serde_json::json!({ "sessionId": session_id, "pinned": pinned }),
            |_| Ok(()),
        )
    }

    fn set_linked_work_item(&self, session_id: String, item: Option<Value>) -> StoreFuture<()> {
        self.call(
            "session_set_linked_work_item",
            serde_json::json!({ "sessionId": session_id, "linkedWorkItem": item }),
            |_| Ok(()),
        )
    }

    fn set_in_flight(&self, sessions: Vec<InFlightSession>) -> StoreFuture<()> {
        self.call(
            "session_set_in_flight",
            serde_json::json!({ "sessions": args(&sessions) }),
            move |state| {
                state.in_flight = sessions;
                Ok(())
            },
        )
    }

    fn list_in_flight(&self) -> StoreFuture<Vec<InFlightSession>> {
        self.call("session_list_in_flight", Value::Null, |state| {
            Ok(state.in_flight.clone())
        })
    }

    fn take_in_flight(&self) -> StoreFuture<Vec<InFlightSession>> {
        self.call("session_take_in_flight", Value::Null, |state| {
            Ok(std::mem::take(&mut state.in_flight))
        })
    }

    fn set_workspace_snapshot(&self, snapshot: Value) -> StoreFuture<()> {
        self.call(
            "workspace_set_snapshot",
            serde_json::json!({ "snapshot": snapshot.clone() }),
            move |state| {
                state.workspace_snapshot = Some(snapshot);
                Ok(())
            },
        )
    }

    fn workspace_snapshot(&self) -> StoreFuture<Option<Value>> {
        self.call("workspace_get_snapshot", Value::Null, |state| {
            Ok(state.workspace_snapshot.clone())
        })
    }

    fn claude_shell_commands(
        &self,
        provider_session_id: String,
        _provider_account_id: Option<String>,
        tool_ids: Vec<String>,
    ) -> StoreFuture<HashMap<String, String>> {
        self.call(
            "claude_shell_commands",
            serde_json::json!({ "providerSessionId": provider_session_id, "toolIds": tool_ids.clone() }),
            move |state| {
                Ok(state
                    .shell_commands
                    .iter()
                    .filter(|(id, _)| tool_ids.contains(id))
                    .map(|(id, command)| (id.clone(), command.clone()))
                    .collect())
            },
        )
    }
}

fn empty_status() -> CheckpointStatus {
    CheckpointStatus { files: Vec::new() }
}

impl CheckpointBackend for FakeBackend {
    fn ensure(&self, session_id: String, cwd: String) -> StoreFuture<()> {
        self.call(
            "session_checkpoint_ensure",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd }),
            |_| Ok(()),
        )
    }

    fn prepare(&self, session_id: String, cwd: String, paths: Vec<String>) -> StoreFuture<()> {
        self.call(
            "session_checkpoint_prepare",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd, "paths": paths }),
            |_| Ok(()),
        )
    }

    fn capture(&self, session_id: String, cwd: String, paths: Vec<String>) -> StoreFuture<()> {
        self.call(
            "session_checkpoint_capture",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd, "paths": paths }),
            |_| Ok(()),
        )
    }

    fn status(&self, session_id: String, cwd: String) -> StoreFuture<CheckpointStatus> {
        self.call(
            "session_checkpoint_status",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd }),
            |_| Ok(empty_status()),
        )
    }

    fn apply(
        &self,
        session_id: String,
        from_cwd: String,
        to_cwd: String,
    ) -> StoreFuture<CheckpointApplyResult> {
        self.call(
            "session_checkpoint_apply",
            serde_json::json!({ "sessionId": session_id, "fromCwd": from_cwd, "toCwd": to_cwd }),
            |_| {
                Ok(CheckpointApplyResult {
                    files: Vec::new(),
                    already_applied: 0,
                })
            },
        )
    }

    fn cleanup_safe(&self, session_id: String, cwd: String) -> StoreFuture<bool> {
        self.call(
            "session_checkpoint_cleanup_safe",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd }),
            |_| Ok(true),
        )
    }

    fn forget(&self, session_id: String) -> StoreFuture<()> {
        self.call(
            "session_checkpoint_forget",
            serde_json::json!({ "sessionId": session_id }),
            |_| Ok(()),
        )
    }

    fn file_diff(
        &self,
        session_id: String,
        cwd: String,
        relative: String,
    ) -> StoreFuture<CheckpointFileDiff> {
        self.call(
            "session_checkpoint_file_diff",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd, "relative": relative.clone() }),
            move |_| {
                Ok(CheckpointFileDiff {
                    path: relative.clone(),
                    relative,
                    status: "modified".into(),
                    original: String::new(),
                    current: String::new(),
                    binary: false,
                    too_large: false,
                })
            },
        )
    }

    fn undo(
        &self,
        session_id: String,
        cwd: String,
        relative: Option<String>,
    ) -> StoreFuture<CheckpointStatus> {
        self.call(
            "session_checkpoint_undo",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd, "relative": relative }),
            |_| Ok(empty_status()),
        )
    }

    fn keep(
        &self,
        session_id: String,
        cwd: String,
        relative: Option<String>,
    ) -> StoreFuture<CheckpointStatus> {
        self.call(
            "session_checkpoint_keep",
            serde_json::json!({ "sessionId": session_id, "cwd": cwd, "relative": relative }),
            |_| Ok(empty_status()),
        )
    }
}

/// A workspace hook whose answers tests set directly.
#[derive(Default)]
pub struct TestWorkspace {
    pub tab_session_ids: RefCell<Vec<String>>,
    /// Sessions the user can see. `None` treats every session as visible.
    pub foreground: RefCell<Option<HashSet<String>>>,
    pub hidden: RefCell<bool>,
    /// What `collect_snapshot` returns; `None` skips snapshot writes.
    pub snapshot: RefCell<Option<Value>>,
    /// Answer to every confirmation dialog.
    pub confirm_answer: RefCell<bool>,
    pub confirms: RefCell<Vec<String>>,
    pub closed_windows: RefCell<usize>,
    pub hidden_windows: RefCell<usize>,
}

impl TestWorkspace {
    pub fn new() -> Rc<Self> {
        let workspace = Rc::new(Self::default());
        *workspace.confirm_answer.borrow_mut() = true;
        workspace
    }
}

impl WorkspaceHooks for TestWorkspace {
    fn window_hidden(&self, _cx: &App) -> bool {
        *self.hidden.borrow()
    }

    fn is_foreground(&self, session_id: &str, _cx: &App) -> bool {
        self.foreground
            .borrow()
            .as_ref()
            .is_none_or(|ids| ids.contains(session_id))
    }

    fn tab_session_ids(&self, _cx: &App) -> Vec<String> {
        self.tab_session_ids.borrow().clone()
    }

    fn collect_snapshot(&self, sessions: &[Session], _cx: &App) -> Option<Value> {
        let base = self.snapshot.borrow().clone()?;
        let mut snapshot = base;
        if let Some(object) = snapshot.as_object_mut() {
            let ids: Vec<Value> = sessions
                .iter()
                .map(|s| Value::String(s.id.clone()))
                .collect();
            object.insert("sessionIds".into(), Value::Array(ids));
        }
        Some(snapshot)
    }

    fn collect_resumed_snapshot(&self, workspace: &ResumedWorkspace) -> Option<Value> {
        Some(serde_json::json!({
            "projectCwd": workspace.project_cwd,
            "layout": workspace.layout,
            "sessionIds": workspace.sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
        }))
    }

    fn snapshot_session_ids(&self, snapshot: &Value) -> Vec<String> {
        snapshot["sessionIds"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn hydrate_snapshot(
        &self,
        snapshot: &Value,
        loaded: &HashMap<String, Session>,
        interrupted: &HashSet<String>,
    ) -> Option<ResumedWorkspace> {
        let sessions: Vec<Session> = self
            .snapshot_session_ids(snapshot)
            .iter()
            .filter_map(|id| loaded.get(id))
            .map(|session| {
                if interrupted.contains(&session.id) {
                    super::in_flight::mark_turn_interrupted(session)
                } else {
                    session.clone()
                }
            })
            .collect();
        if sessions.is_empty() {
            return None;
        }
        Some(ResumedWorkspace {
            project_cwd: snapshot["projectCwd"].as_str().unwrap_or("~").to_string(),
            layout: snapshot["layout"].clone(),
            sessions,
        })
    }

    fn layout_for_sessions(&self, session_ids: &[String]) -> Value {
        serde_json::json!({ "tabs": session_ids })
    }

    fn confirm(&self, message: &str, _ok_label: &str, _cx: &mut App) -> Task<bool> {
        self.confirms.borrow_mut().push(message.to_string());
        Task::ready(*self.confirm_answer.borrow())
    }

    fn hide_window(&self, _cx: &mut App) {
        *self.hidden_windows.borrow_mut() += 1;
    }

    fn close_window(&self, _cx: &mut App) {
        *self.closed_windows.borrow_mut() += 1;
    }
}

/// Install an `Engine` over a fresh `FakeBackend`.
pub fn init_test_engine(cx: &mut TestAppContext) -> Arc<FakeBackend> {
    init_test_engine_with(cx, EngineHooks::default())
}

/// Install an `Engine` over a fresh `FakeBackend` with these hooks.
pub fn init_test_engine_with(cx: &mut TestAppContext, hooks: EngineHooks) -> Arc<FakeBackend> {
    let backend = FakeBackend::new();
    let mut config = EngineConfig::with_backend(backend.clone());
    config.hooks = hooks;
    cx.update(|cx| Engine::init(config, cx));
    backend
}
