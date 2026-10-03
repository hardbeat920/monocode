//! Port of the `Orchestrator` class in
//! src/features/orchestration/model/orchestration.ts: runs, tasks, dispatch,
//! scopes, review, resume, and the idempotent control receipts.
//!
//! The entity holds the state the class kept in its fields. The flows that
//! were `async` methods are free functions over a `WeakEntity`, so they lease
//! the entity only to read or change state, never while they call the host.
//! The host calls into Submit, which reads this entity.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::future::{LocalBoxFuture, Shared};
use futures::{FutureExt, StreamExt};
use gpui::{App, AsyncApp, Context, Entity, Task, WeakEntity};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::orchestration::{
    OrchestrationChoice, OrchestrationProposal, OrchestrationProposalStatus,
};
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{Extra, HARNESSES, HarnessEvent, HarnessId, Session};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::host::{OrchestrationHost, OrchestrationStorage, PendingInput};
use super::plan::{validate_settings, validate_tasks};
use super::state::{
    DispatchStage, DispatchState, OrchestrationDispatch, OrchestrationRun, OrchestrationTask,
    RequestReceipt, RunStatus, TaskStatus, WorkspacePolicy, normalize_orchestration_run,
    orchestration_checkout_cwd, orchestration_workspace, workspace_identity,
};
use super::support::{
    check_fields, lead_prompt, listed12, orchestration_path_key, question_answers, recovery_turn,
    same_checkout, scope_contains, scopes_overlap, slice_tail, strings, text, worker_turn_prompt,
};
use crate::attention::approval_toast::pending_approval_for_session;
use crate::attention::notifications::InputKind;
use crate::submit::{ControlOutcome, ControlStatus};

/// Resolves when a queued operation has finished, however it finished.
type Tail = Shared<LocalBoxFuture<'static, ()>>;
type SharedResult = Shared<LocalBoxFuture<'static, Result<Value, String>>>;

fn ready_tail() -> Tail {
    futures::future::ready(()).boxed_local().shared()
}

/// `Date.now()`.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

const RELEASED: &str = "The orchestrator is no longer available";

/// A change `pause` applies to the run in the same commit.
pub type RunPatch = Box<dyn FnOnce(&mut OrchestrationRun)>;

/// A run being started from an approved proposal.
#[derive(Debug, Clone)]
pub struct ApprovedStart {
    pub proposal_id: String,
    pub allowed_models: Vec<OrchestrationChoice>,
    pub tasks: Vec<OrchestrationTask>,
}

/// The app's orchestrator. Views observe it; every change notifies.
pub struct Orchestrator {
    runs: Vec<Rc<OrchestrationRun>>,
    loaded: HashSet<String>,
    deleted: HashSet<String>,
    persisted: HashMap<String, Rc<OrchestrationRun>>,
    saves: Tail,
    actions: Tail,
    pumping: bool,
    starting: HashSet<String>,
    pump_again: bool,
    waking: HashSet<String>,
    blocked: HashMap<String, String>,
    announced: HashMap<String, HashSet<String>>,
    /// Async write checks are isolated per dispatch, including retries.
    write_checks: HashMap<String, HashMap<u64, Tail>>,
    next_check: u64,
    inflight: HashMap<String, (String, SharedResult)>,
    listeners: HashMap<u64, mpsc::UnboundedSender<()>>,
    next_listener: u64,
    host: Option<Rc<dyn OrchestrationHost>>,
    store: Rc<dyn OrchestrationStorage>,
}

impl Orchestrator {
    pub fn new(store: Rc<dyn OrchestrationStorage>) -> Self {
        Self {
            runs: Vec::new(),
            loaded: HashSet::new(),
            deleted: HashSet::new(),
            persisted: HashMap::new(),
            saves: ready_tail(),
            actions: ready_tail(),
            pumping: false,
            starting: HashSet::new(),
            pump_again: false,
            waking: HashSet::new(),
            blocked: HashMap::new(),
            announced: HashMap::new(),
            write_checks: HashMap::new(),
            next_check: 0,
            inflight: HashMap::new(),
            listeners: HashMap::new(),
            next_listener: 0,
            host: None,
            store,
        }
    }

    /// `bind`.
    pub fn bind(&mut self, host: Rc<dyn OrchestrationHost>) {
        self.host = Some(host);
    }

    pub fn host(&self) -> Option<Rc<dyn OrchestrationHost>> {
        self.host.clone()
    }

    pub fn store(&self) -> Rc<dyn OrchestrationStorage> {
        self.store.clone()
    }

    /// `snapshot`: every loaded run, oldest change first.
    pub fn snapshot(&self) -> Vec<Rc<OrchestrationRun>> {
        self.runs.clone()
    }

    /// `run`: the run this session leads.
    pub fn run(&self, id: &str) -> Option<Rc<OrchestrationRun>> {
        self.runs.iter().find(|run| run.lead_id == id).cloned()
    }

    /// `forSession`: the run this session leads or works in.
    pub fn for_session(&self, id: &str) -> Option<Rc<OrchestrationRun>> {
        self.runs
            .iter()
            .find(|run| run.lead_id == id || run.tasks.iter().any(|task| task.session_id == id))
            .cloned()
    }

    /// Lead ids of runs that are active or paused.
    pub fn running_lead_ids(&self) -> HashSet<String> {
        self.runs
            .iter()
            .filter(|run| matches!(run.status, RunStatus::Active | RunStatus::Paused))
            .map(|run| run.lead_id.clone())
            .collect()
    }

    fn session(&self, id: &str, cx: &App) -> Option<Session> {
        self.host.as_ref()?.session(id, cx)
    }

    /// `resumeBlocker`: another busy session in the checkout a run would use.
    pub fn resume_blocker(
        &self,
        lead_id: &str,
        checkout_cwd: Option<&str>,
        cx: &App,
    ) -> Option<Session> {
        let host = self.host.as_ref()?;
        let lead = host.session(lead_id, cx)?;
        let cwd = match checkout_cwd {
            Some(cwd) => cwd.to_string(),
            None => match self.run(lead_id) {
                Some(run) => orchestration_checkout_cwd(&run),
                None => lead
                    .worktree_cwd
                    .clone()
                    .unwrap_or_else(|| lead.cwd.clone()),
            },
        };
        host.sessions(cx).into_iter().find(|session| {
            session.id != lead_id
                && session.is_busy()
                && same_checkout(
                    session.worktree_cwd.as_deref().unwrap_or(&session.cwd),
                    &cwd,
                )
        })
    }

    /// `resumeLeadBusy`.
    pub fn resume_lead_busy(&self, lead_id: &str, cx: &App) -> bool {
        self.session(lead_id, cx).is_some_and(|lead| lead.is_busy())
    }

    fn emit(&mut self, cx: &mut Context<Self>) {
        self.listeners
            .retain(|_, listener| listener.unbounded_send(()).is_ok());
        cx.notify();
    }

    fn subscribe(&mut self) -> (u64, mpsc::UnboundedReceiver<()>) {
        let (sender, receiver) = mpsc::unbounded();
        let id = self.next_listener;
        self.next_listener += 1;
        self.listeners.insert(id, sender);
        (id, receiver)
    }

    /// Put `run` in place of the run with its lead, moving it last.
    fn replace_run(&mut self, run: Rc<OrchestrationRun>) {
        self.runs.retain(|entry| entry.lead_id != run.lead_id);
        self.runs.push(run);
    }

    /// Replace the run with this lead where it stands (`runs.map`).
    fn map_run(&mut self, lead_id: &str, next: impl FnOnce(&OrchestrationRun) -> OrchestrationRun) {
        if let Some(index) = self.runs.iter().position(|run| run.lead_id == lead_id) {
            let replaced = next(&self.runs[index]);
            self.runs[index] = Rc::new(replaced);
        }
    }

    /// `submissionError`: why this session cannot take an ordinary turn now.
    pub fn submission_error(&self, id: &str, managed: bool, cx: &App) -> Option<String> {
        if managed {
            return None;
        }
        let session = self.session(id, cx)?;
        let own = self.for_session(id);
        if let Some(own) = &own
            && own.lead_id != id
            && (own.status == RunStatus::Active
                || own.tasks.iter().any(OrchestrationTask::is_active))
        {
            return Some("This worker is managed by the orchestrator. Send instructions through its lead or stop the run first.".into());
        }
        let work_cwd = session.worktree_cwd.as_deref().unwrap_or(&session.cwd);
        let other = self.runs.iter().any(|run| {
            (run.status == RunStatus::Active || run.tasks.iter().any(OrchestrationTask::is_active))
                && run.lead_id != id
                && same_checkout(&orchestration_checkout_cwd(run), work_cwd)
        });
        if other {
            return Some("This checkout has an active orchestrator. Stop that run before starting independent work.".into());
        }
        if own
            .as_ref()
            .is_some_and(|own| own.status == RunStatus::Paused)
        {
            return Some(
                "Resume or stop orchestration before sending the lead another turn.".into(),
            );
        }
        if let Some(own) = &own
            && own.status == RunStatus::Active
            && !same_checkout(&orchestration_checkout_cwd(own), work_cwd)
        {
            return Some(
                "Return the lead to its original project or stop orchestration first.".into(),
            );
        }
        None
    }

    /// `prompt`: add the run's control envelope to a lead's outgoing prompt.
    pub fn prompt(&self, id: &str, prompt: &str) -> String {
        match self.run(id) {
            Some(run) if run.status == RunStatus::Active => lead_prompt(prompt, &run.cli),
            _ => prompt.to_string(),
        }
    }

    /// `inactiveReason`.
    pub fn inactive_reason(run: &OrchestrationRun) -> String {
        if run.status == RunStatus::Paused {
            format!(
                "This run is paused. {} list, get and wait remain available for inspection. Do not retry mutations or keep polling: explain the pause and ask the user to click Resume in MonoCode. Resume will continue interrupted tasks from their retained worker checkouts; policy-blocked tasks remain stopped for an explicit retry or cancellation.",
                run.error.as_deref().unwrap_or("Work was interrupted.")
            )
        } else {
            format!(
                "This run is {}. Inspect results with list or get; do not keep retrying commands for this run.",
                run.status
            )
        }
    }

    /// `pendingInput`: what a worker is blocked on. Workers have no
    /// user-facing prompt: the lead answers for them, and escalates to the
    /// user in its own conversation when it does not want to decide alone.
    pub fn pending_input(&self, task: &OrchestrationTask, cx: &App) -> Option<PendingInput> {
        let worker = self.session(&task.session_id, cx)?;
        let pending = pending_approval_for_session(&worker)?;
        let detail = match pending.kind {
            InputKind::Approval => pending.block.as_ref().map(|block| {
                match block.tool.as_ref().and_then(|tool| tool.detail.as_deref()) {
                    Some(detail) => monocode_core::js::trim(detail).to_string(),
                    None => block.text.clone(),
                }
            }),
            InputKind::Question => None,
        };
        Some(PendingInput {
            kind: pending.kind,
            request_id: pending.request_id,
            label: pending.label,
            detail,
            questions: worker
                .pending_question
                .as_ref()
                .map(|question| question.questions.clone()),
        })
    }

    /// `waitingFor`.
    pub fn waiting_for(run: &OrchestrationRun, task: &OrchestrationTask) -> Option<String> {
        if task.status != TaskStatus::Queued {
            return None;
        }
        if let Some(dependency) = run
            .tasks
            .iter()
            .find(|entry| task.depends_on.contains(&entry.id) && !entry.accepted)
        {
            return Some(format!("Waiting for review: {}", dependency.title));
        }
        if let Some(owner) = run
            .tasks
            .iter()
            .find(|entry| entry.is_active() && scopes_overlap(&entry.scopes, &task.scopes))
        {
            return Some(format!("Waiting for files: {}", owner.title));
        }
        Some("Waiting for a worker slot".into())
    }

    /// `view`: the run as `list` and `wait` report it.
    pub fn view(&self, run: &OrchestrationRun, cx: &App) -> Value {
        let mut value = serde_json::to_value(run).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.remove("cli");
            object.remove("requests");
            if run.status != RunStatus::Active {
                object.insert("recovery".into(), Value::String(Self::inactive_reason(run)));
            }
            let tasks: Vec<Value> = run
                .tasks
                .iter()
                .map(|task| {
                    let mut entry = serde_json::to_value(task).unwrap_or(Value::Null);
                    if let Some(entry) = entry.as_object_mut() {
                        entry.remove("prompt");
                        entry.remove("scopes");
                        if let Some(waiting) = Self::waiting_for(run, task) {
                            entry.insert("waitingFor".into(), Value::String(waiting));
                        }
                        if let Some(pending) = self.pending_input(task, cx) {
                            entry.insert(
                                "needsInput".into(),
                                serde_json::to_value(pending).unwrap_or(Value::Null),
                            );
                        }
                    }
                    entry
                })
                .collect();
            object.insert("tasks".into(), Value::Array(tasks));
        }
        value
    }

    /// `blockedKeys`: `${taskId}:${requestId}` for every worker currently
    /// blocked on the lead.
    fn blocked_keys(&self, run: &OrchestrationRun, cx: &App) -> Vec<String> {
        run.tasks
            .iter()
            .filter_map(|task| {
                self.pending_input(task, cx)
                    .map(|pending| format!("{}:{}", task.id, pending.request_id))
            })
            .collect()
    }

    /// `observe`: check every write a running worker reports against its
    /// scope.
    pub fn observe(&mut self, id: &str, event: &HarnessEvent, cx: &mut Context<Self>) {
        let (preview, status, event_paths) = match event {
            HarnessEvent::ToolStarted {
                preview,
                status,
                paths,
                ..
            }
            | HarnessEvent::ToolUpdated {
                preview,
                status,
                paths,
                ..
            } => (preview, status, paths),
            _ => return,
        };
        let Some(run) = self.for_session(id) else {
            return;
        };
        let Some(task) = run
            .tasks
            .iter()
            .find(|entry| entry.session_id == id && entry.status == TaskStatus::Running)
            .cloned()
        else {
            return;
        };
        let Some(preview) = preview
            .as_ref()
            .filter(|preview| preview.kind == monocode_core::block::ToolPreviewKind::Write)
        else {
            return;
        };
        if run.status != RunStatus::Active
            || matches!(
                status.as_deref(),
                Some("failed") | Some("error") | Some("cancelled")
            )
        {
            return;
        }
        let paths: Vec<String> = match event_paths {
            Some(paths) => paths.clone(),
            None => preview
                .path
                .clone()
                .filter(|path| !path.is_empty())
                .into_iter()
                .collect(),
        };
        let Some(dispatch_id) = task.active_dispatch_id.clone() else {
            return;
        };
        let lead_id = run.lead_id.clone();
        let check_id = self.next_check;
        self.next_check += 1;
        let (done, finished) = oneshot::channel::<()>();
        let tracked = self.write_checks.contains_key(&dispatch_id);
        if let Some(checks) = self.write_checks.get_mut(&dispatch_id) {
            checks.insert(check_id, finished.map(|_| ()).boxed_local().shared());
        }
        let checkout = task
            .workspace
            .as_ref()
            .map(|workspace| workspace.checkout_cwd.clone())
            .unwrap_or_else(|| orchestration_checkout_cwd(&run));
        cx.spawn(async move |this, cx| {
            if let Err(error) =
                check_writes(&this, &lead_id, &task, &dispatch_id, &paths, &checkout, cx).await
            {
                log::error!("[orchestration] write check failed: {error}");
            }
            let _ = done.send(());
            if tracked {
                this.update(cx, |this, _| {
                    if let Some(checks) = this.write_checks.get_mut(&dispatch_id) {
                        checks.remove(&check_id);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// `sync`: tell `wait` about blocked workers and hand finished results
    /// and questions to an idle lead.
    pub fn sync(&mut self, cx: &mut Context<Self>) {
        let runs = self.runs.clone();
        for run in &runs {
            if run.status != RunStatus::Active {
                continue;
            }
            // A blocked worker changes no run state, so `wait` needs telling.
            let keys = self.blocked_keys(run, cx).join(",");
            if self.blocked.get(&run.lead_id) != Some(&keys) {
                self.blocked.insert(run.lead_id.clone(), keys);
                self.emit(cx);
            }
        }
        for run in &self.runs.clone() {
            if run.status != RunStatus::Active || self.waking.contains(&run.lead_id) {
                continue;
            }
            let Some(lead) = self.session(&run.lead_id, cx) else {
                continue;
            };
            if lead.is_busy()
                || lead
                    .queued_messages
                    .as_ref()
                    .is_some_and(|queue| !queue.is_empty())
            {
                continue;
            }
            let announced = self
                .announced
                .get(&run.lead_id)
                .cloned()
                .unwrap_or_default();
            let results = run.tasks.iter().any(|task| !task.delivered);
            let blocked = self
                .blocked_keys(run, cx)
                .into_iter()
                .any(|key| !announced.contains(&key));
            if !results && !blocked {
                continue;
            }
            self.waking.insert(run.lead_id.clone());
            let lead_id = run.lead_id.clone();
            // Let session state settle before checking idle; never interrupt user input.
            cx.spawn(async move |this, cx| {
                if let Err(error) = continue_lead(&this, &lead_id, cx).await {
                    log::error!("[orchestration] continuation failed: {error}");
                }
                this.update(cx, |this, _| {
                    this.waking.remove(&lead_id);
                })
                .ok();
            })
            .detach();
        }
    }
}

// Small accessors for the async flows.

fn read<R>(
    this: &WeakEntity<Orchestrator>,
    cx: &mut AsyncApp,
    read: impl FnOnce(&Orchestrator, &App) -> R,
) -> Result<R, String> {
    this.read_with(cx, read).map_err(|_| RELEASED.to_string())
}

fn update<R>(
    this: &WeakEntity<Orchestrator>,
    cx: &mut AsyncApp,
    update: impl FnOnce(&mut Orchestrator, &mut Context<Orchestrator>) -> R,
) -> Result<R, String> {
    this.update(cx, update).map_err(|_| RELEASED.to_string())
}

fn current_run(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    cx: &mut AsyncApp,
) -> Result<Option<Rc<OrchestrationRun>>, String> {
    read(this, cx, |this, _| this.run(lead_id))
}

fn require_run(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    cx: &mut AsyncApp,
) -> Result<Rc<OrchestrationRun>, String> {
    current_run(this, lead_id, cx)?
        .ok_or_else(|| "No orchestration run was found for this lead".to_string())
}

fn host(
    this: &WeakEntity<Orchestrator>,
    cx: &mut AsyncApp,
) -> Result<Rc<dyn OrchestrationHost>, String> {
    read(this, cx, |this, _| this.host.clone())?
        .ok_or_else(|| "The orchestrator is not bound to the app".to_string())
}

fn store(
    this: &WeakEntity<Orchestrator>,
    cx: &mut AsyncApp,
) -> Result<Rc<dyn OrchestrationStorage>, String> {
    read(this, cx, |this, _| this.store.clone())
}

/// `host?.stop(id)`: nothing when no host is bound.
async fn stop_session(
    this: &WeakEntity<Orchestrator>,
    id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let Some(host) = read(this, cx, |this, _| this.host.clone())? else {
        return Ok(());
    };
    cx.update(|cx| host.stop(id, cx)).await
}

/// Start `pump` on its own task (`void this.pump()`).
pub(crate) fn spawn_pump(this: &WeakEntity<Orchestrator>, cx: &AsyncApp) {
    let this = this.clone();
    cx.spawn(async move |cx| pump(&this, cx).await).detach();
}

fn spawn_sync(this: &WeakEntity<Orchestrator>, cx: &mut AsyncApp) {
    update(this, cx, |this, cx| this.sync(cx)).ok();
}

/// `commit`: normalize, publish, and save a run. A failed save pauses the
/// run and stops its processes.
pub(crate) async fn commit(
    this: &WeakEntity<Orchestrator>,
    run: OrchestrationRun,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let run = Rc::new(normalize_orchestration_run(&run));
    let lead_id = run.lead_id.clone();
    let previous = update(this, cx, |this, cx| {
        this.replace_run(run.clone());
        this.emit(cx);
        this.saves.clone()
    })?;
    let store = store(this, cx)?;
    let saved: Shared<Task<Result<(), String>>> = {
        let this = this.clone();
        let run = run.clone();
        cx.spawn(async move |cx| {
            previous.await;
            let save = cx.update(|cx| store.save(&run, cx));
            save.await?;
            this.update(cx, |this, _| {
                this.persisted.insert(run.lead_id.clone(), run.clone());
            })
            .ok();
            Ok(())
        })
        .shared()
    };
    update(this, cx, |this, _| {
        this.saves = saved.clone().map(|_| ()).boxed_local().shared();
    })?;
    let Err(error) = saved.await else {
        return Ok(());
    };
    let running: Vec<String> = update(this, cx, |this, cx| {
        let Some(current) = this.run(&lead_id) else {
            return Vec::new();
        };
        let running = current
            .tasks
            .iter()
            .filter(|task| task.is_active())
            .map(|task| task.session_id.clone())
            .collect();
        this.map_run(&lead_id, |entry| {
            let mut next = entry.clone();
            next.status = RunStatus::Paused;
            next.error = Some(format!("Could not save run: {error}"));
            for task in &mut next.tasks {
                if task.is_active() {
                    task.status = TaskStatus::Cancelling;
                }
            }
            next
        });
        this.emit(cx);
        running
    })?;
    // Keep the checkout reserved until processes have stopped, even if the
    // database is unavailable. Resume sends a recovery turn into the same
    // retained checkout instead of pretending the interrupted turn finished.
    let ids: Vec<String> = std::iter::once(lead_id.clone()).chain(running).collect();
    let stops = ids.into_iter().map(|id| {
        let this = this.clone();
        let lead_id = lead_id.clone();
        let mut cx = cx.clone();
        async move {
            if stop_session(&this, &id, &mut cx).await.is_err() {
                return;
            }
            update(&this, &mut cx, |this, cx| {
                let Some(latest) = this.run(&lead_id) else {
                    return;
                };
                let stopped_dispatch = latest
                    .tasks
                    .iter()
                    .find(|task| task.session_id == id && task.is_active())
                    .and_then(|task| task.active_dispatch_id.clone());
                let mut next = (*latest).clone();
                for task in &mut next.tasks {
                    if task.session_id == id && task.is_active() {
                        task.status = TaskStatus::Interrupted;
                        task.accepted = false;
                        task.delivered = true;
                        if let Some(dispatch) = task.active_dispatch_id.take() {
                            task.last_dispatch_id = Some(dispatch);
                        }
                        task.error = Some("Stopped because run history could not be saved. Resume will continue from the retained worker checkout.".into());
                        task.recovery_prompt =
                            Some(recovery_turn("run history could not be saved"));
                    }
                }
                next.map_dispatch(stopped_dispatch.as_deref(), |dispatch| {
                    dispatch.state = DispatchState::Interrupted;
                    dispatch.stage = DispatchStage::Settled;
                    dispatch.updated_at = now_ms();
                    dispatch.error = Some("Stopped because run history could not be saved.".into());
                });
                this.map_run(&lead_id, |_| next);
                this.emit(cx);
            })
            .ok();
        }
    });
    futures::future::join_all(stops).await;
    Err(error)
}

async fn patch_task(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    id: &str,
    patch: impl FnOnce(&mut OrchestrationTask),
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let Some(run) = current_run(this, lead_id, cx)? else {
        return Ok(());
    };
    let mut next = (*run).clone();
    next.map_task(id, patch);
    commit(this, next, cx).await
}

async fn patch_dispatch(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    dispatch_id: &str,
    patch: impl FnOnce(&mut OrchestrationDispatch),
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let Some(run) = current_run(this, lead_id, cx)? else {
        return Ok(());
    };
    let mut next = (*run).clone();
    next.map_dispatch(Some(dispatch_id), |dispatch| {
        patch(dispatch);
        dispatch.updated_at = now_ms();
    });
    commit(this, next, cx).await
}

/// `cleanupUnchangedWorker`: remove an isolated checkout that holds no
/// unreviewed work. `Ok(false)` keeps it.
async fn cleanup_unchanged_worker(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task_id: &str,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let Some(run) = current_run(this, lead_id, cx)? else {
        return Ok(true);
    };
    let Some(task) = run.task(task_id).cloned() else {
        return Ok(true);
    };
    if task.workspace_policy == Some(WorkspacePolicy::Shared) || task.workspace.is_none() {
        return Ok(true);
    }
    let attempt: Result<bool, String> = async {
        let Some(host) = read(this, cx, |this, _| this.host.clone())? else {
            return Ok(false);
        };
        let cleaned = cx
            .update(|cx| host.cleanup_worker(&run, &task, true, cx))
            .await?;
        if !cleaned {
            return Ok(false);
        }
        let Some(current) = current_run(this, lead_id, cx)? else {
            return Ok(false);
        };
        let mut next = (*current).clone();
        next.map_task(task_id, |entry| entry.workspace = None);
        next.map_dispatch(task.last_dispatch_id.as_deref(), |dispatch| {
            dispatch.stage = DispatchStage::Cleaned;
            dispatch.cleanup_error = None;
            dispatch.updated_at = now_ms();
        });
        commit(this, next, cx).await?;
        Ok(true)
    }
    .await;
    match attempt {
        Ok(cleaned) => Ok(cleaned),
        Err(error) => {
            if let Some(dispatch_id) = &task.last_dispatch_id {
                patch_dispatch(
                    this,
                    lead_id,
                    dispatch_id,
                    |dispatch| dispatch.cleanup_error = Some(error),
                    cx,
                )
                .await?;
            }
            Ok(false)
        }
    }
}

/// `hydrate`: load a lead's saved run once. A run that was active when the
/// app stopped comes back paused with its workers interrupted.
pub async fn hydrate(
    this: &WeakEntity<Orchestrator>,
    id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let skip = update(this, cx, |this, _| {
        if this.loaded.contains(id) || this.run(id).is_some() {
            return true;
        }
        this.loaded.insert(id.to_string());
        false
    })?;
    if skip {
        return Ok(());
    }
    let result: Result<(), String> = async {
        let store = store(this, cx)?;
        let loaded = cx.update(|cx| store.load(id, cx)).await?;
        let Some(loaded) = loaded else {
            return Ok(());
        };
        if read(this, cx, |this, _| this.run(id).is_some() || this.deleted.contains(id))? {
            return Ok(());
        }
        let run = normalize_orchestration_run(&loaded);
        let was_active = run.status == RunStatus::Active;
        if was_active || run.tasks.iter().any(OrchestrationTask::is_active) {
            let ids: Vec<String> = std::iter::once(id.to_string())
                .chain(
                    run.tasks
                        .iter()
                        .filter(|task| task.is_active())
                        .map(|task| task.session_id.clone()),
                )
                .collect();
            let stops = ids.iter().map(|session_id| {
                let this = this.clone();
                let mut cx = cx.clone();
                let session_id = session_id.clone();
                async move { stop_session(&this, &session_id, &mut cx).await }
            });
            for result in futures::future::join_all(stops).await {
                result?;
            }
            cx.update(|cx| store.disable(id, cx)).await?;
        }
        let interrupted: HashSet<String> = run
            .tasks
            .iter()
            .filter(|task| task.is_active())
            .filter_map(|task| task.active_dispatch_id.clone())
            .collect();
        let mut next = run.clone();
        if was_active {
            next.status = RunStatus::Paused;
            next.error = Some("Run interrupted while MonoCode was not running. Worker checkouts were retained; Resume will continue them.".into());
            next.last_pause_reason =
                Some("MonoCode stopped while the orchestration run was active.".into());
        }
        for task in &mut next.tasks {
            if task.is_active() {
                task.status = TaskStatus::Interrupted;
                if let Some(dispatch) = task.active_dispatch_id.take() {
                    task.last_dispatch_id = Some(dispatch);
                }
                task.accepted = false;
                task.error = Some("Interrupted while MonoCode was not running. Resume will continue from the retained worker checkout.".into());
                task.recovery_prompt = Some(recovery_turn(
                    "MonoCode stopped while the worker was running",
                ));
                task.delivered = true;
            } else {
                task.delivered = task.accepted || task.status == TaskStatus::Queued;
            }
        }
        let now = now_ms();
        for dispatch in next.dispatches.get_or_insert_with(Vec::new) {
            if interrupted.contains(&dispatch.id) {
                dispatch.state = DispatchState::Interrupted;
                dispatch.stage = DispatchStage::Settled;
                dispatch.updated_at = now;
                dispatch.error = Some("Interrupted while MonoCode was not running.".into());
            }
        }
        commit(this, next, cx).await
    }
    .await;
    if result.is_err() {
        update(this, cx, |this, _| {
            this.loaded.remove(id);
        })?;
    }
    result
}

/// `start`: begin or resume a run. Starting twice at once is refused.
pub async fn start(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    allowed_harnesses: &[HarnessId],
    max_workers: i64,
    approved: Option<ApprovedStart>,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let already = update(this, cx, |this, _| {
        !this.starting.insert(lead_id.to_string())
    })?;
    if already {
        return Err("The run is already starting".into());
    }
    let result = start_run(this, lead_id, allowed_harnesses, max_workers, approved, cx).await;
    update(this, cx, |this, _| {
        this.starting.remove(lead_id);
    })
    .ok();
    result
}

/// The approved assignments as the lead's first turn lists them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovedAssignment<'a> {
    task_id: &'a str,
    title: &'a str,
    prompt: &'a str,
    harness: HarnessId,
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_settings: Option<&'a monocode_core::ModelSettings>,
    files: &'a [String],
    depends_on: &'a [String],
}

/// `startApproved`: start exactly the assignments the user confirmed.
pub async fn start_approved(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    proposal_id: &str,
    proposal: &OrchestrationProposal,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    if proposal.status != OrchestrationProposalStatus::Ready {
        return Err("Review a completed proposal before starting".into());
    }
    let settings = validate_settings(&proposal.settings)?;
    let proposal_checkout = proposal.checkout_cwd.as_deref().unwrap_or(&proposal.cwd);
    let planned = validate_tasks(&proposal.tasks, &settings, Some(proposal_checkout))?;
    let host = host(this, cx)?;
    let lead = cx.update(|cx| host.session(lead_id, cx));
    let Some(lead) = lead.filter(|lead| {
        same_checkout(&lead.cwd, &proposal.cwd)
            && same_checkout(
                lead.worktree_cwd.as_deref().unwrap_or(&lead.cwd),
                proposal_checkout,
            )
    }) else {
        return Err("Return to the proposal's checkout before starting".into());
    };
    let available = cx.update(|cx| host.choices(cx));
    let allowed_models: Vec<OrchestrationChoice> = settings
        .choices
        .iter()
        .filter(|choice| {
            available.iter().any(|entry| {
                entry.harness == choice.harness
                    && entry.models.iter().any(|model| model.id == choice.model)
            })
        })
        .cloned()
        .collect();
    if planned.iter().any(|task| {
        !allowed_models
            .iter()
            .any(|choice| choice.harness == task.harness && choice.model == task.model)
    }) {
        return Err(
            "An assigned model is no longer available. Change that assignment before starting."
                .into(),
        );
    }
    let ids: HashMap<String, String> = planned
        .iter()
        .map(|task| (task.id.clone(), new_id()))
        .collect();
    let store = store(this, cx)?;
    let lead_checkout = lead
        .worktree_cwd
        .clone()
        .unwrap_or_else(|| lead.cwd.clone());
    let scopes = cx.update(|cx| {
        planned
            .iter()
            .map(|task| store.scopes(&lead_checkout, &task.files, cx))
            .collect::<Vec<_>>()
    });
    let scopes = futures::future::join_all(scopes).await;
    let mut tasks = Vec::new();
    for (task, scopes) in planned.iter().zip(scopes) {
        tasks.push(OrchestrationTask {
            id: ids[&task.id].clone(),
            assignment_id: Some(task.id.clone()),
            session_id: new_id(),
            title: task.title.clone(),
            harness: task.harness,
            model: task.model.clone(),
            model_settings: task.model_settings.clone(),
            prompt: task.prompt.clone(),
            files: task.files.clone(),
            scopes: scopes?,
            write_scopes: None,
            scratch_dir: None,
            depends_on: task
                .depends_on
                .iter()
                .filter_map(|id| ids.get(id).cloned())
                .collect(),
            status: TaskStatus::Queued,
            accepted: false,
            result: String::new(),
            error: None,
            recovery_prompt: None,
            delivered: true,
            workspace_policy: Some(WorkspacePolicy::IsolatedChild),
            workspace: None,
            active_dispatch_id: None,
            last_dispatch_id: None,
            accepted_dispatch_id: None,
            extra: Extra::new(),
        });
    }
    let current_lead = cx.update(|cx| host.session(lead_id, cx));
    if !current_lead.is_some_and(|lead| {
        same_checkout(
            lead.worktree_cwd.as_deref().unwrap_or(&lead.cwd),
            proposal_checkout,
        )
    }) {
        return Err("Return to the proposal's checkout before starting".into());
    }
    let mut harnesses: Vec<HarnessId> = Vec::new();
    for choice in &allowed_models {
        if !harnesses.contains(&choice.harness) {
            harnesses.push(choice.harness);
        }
    }
    let assignments: Vec<ApprovedAssignment> = tasks
        .iter()
        .map(|task| ApprovedAssignment {
            task_id: &task.id,
            title: &task.title,
            prompt: &task.prompt,
            harness: task.harness,
            model: &task.model,
            model_settings: task.model_settings.as_ref(),
            files: &task.files,
            depends_on: &task.depends_on,
        })
        .collect();
    let text = format!(
        "The user confirmed the orchestration card, including any edits. The app has already queued the exact assignments below; do not delegate duplicates. Supervise them through the control CLI, review their changes, request corrections when needed, and finish the original request.\n\nOriginal request:\n{}\n\nApproved assignments:\n{}",
        proposal.request,
        serde_json::to_string(&assignments).unwrap_or_default()
    );
    start(
        this,
        lead_id,
        &harnesses,
        settings.max_workers,
        Some(ApprovedStart {
            proposal_id: proposal_id.to_string(),
            allowed_models,
            tasks: tasks.clone(),
        }),
        cx,
    )
    .await?;
    let weak = this.clone();
    let lead = lead_id.to_string();
    cx.update(|cx| {
        host.submit(
            &lead.clone(),
            &text,
            Box::new(move |outcome, cx| {
                if outcome.status != ControlStatus::Completed {
                    let error = outcome.error.unwrap_or_else(|| {
                        "The lead was interrupted. Its agents were stopped; review and resume the run."
                            .into()
                    });
                    spawn_pause(&weak, &lead, error, None, cx);
                }
            }),
            cx,
        );
    });
    Ok(())
}

async fn start_run(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    allowed_harnesses: &[HarnessId],
    max_workers: i64,
    approved: Option<ApprovedStart>,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let host = host(this, cx)?;
    let Some(lead) = cx.update(|cx| host.session(lead_id, cx)) else {
        return Err("The orchestration lead is unavailable".into());
    };
    let previous = current_run(this, lead_id, cx)?;
    let paused = previous
        .as_ref()
        .is_some_and(|run| run.status == RunStatus::Paused);
    if lead.is_busy() {
        return Err(if paused {
            "Wait for the lead's interrupted turn to finish before resuming orchestration".into()
        } else {
            "Wait for the lead's current turn to finish".into()
        });
    }
    let workspace = workspace_identity(
        &lead.cwd,
        lead.worktree_cwd.as_deref().unwrap_or(&lead.cwd),
        lead.branch.as_deref(),
    );
    if !(1..=4).contains(&max_workers) {
        return Err("Choose 1 to 4 workers".into());
    }
    let available: Vec<HarnessId> = cx
        .update(|cx| host.choices(cx))
        .into_iter()
        .map(|choice| choice.harness)
        .collect();
    if allowed_harnesses.is_empty() || allowed_harnesses.iter().any(|id| !available.contains(id)) {
        return Err("Choose installed worker harnesses".into());
    }
    if let Some(previous) = &previous {
        if previous.status == RunStatus::Active || (approved.is_some() && paused) {
            return Err("Stop the current run before starting another proposal".into());
        }
        if previous.tasks.iter().any(OrchestrationTask::is_active) {
            return Err(if paused {
                "Wait for interrupted agents to stop before resuming orchestration".into()
            } else {
                "Stop active workers before changing the run".into()
            });
        }
        if paused
            && !same_checkout(
                &orchestration_checkout_cwd(previous),
                &workspace.checkout_cwd,
            )
        {
            return Err(
                "Return the lead to its original checkout before resuming orchestration".into(),
            );
        }
    }
    let blocker = read(this, cx, |this, cx| {
        this.resume_blocker(lead_id, Some(&workspace.checkout_cwd), cx)
    })?;
    if let Some(blocker) = blocker {
        let title = monocode_core::js::trim(&blocker.title);
        let label = if title.is_empty() {
            blocker.id.as_str()
        } else {
            title
        };
        return Err(format!(
            "\"{label}\" is still running in this checkout. Stop it before {}.",
            if paused {
                "resuming orchestration"
            } else {
                "starting orchestration"
            }
        ));
    }
    let store = store(this, cx)?;
    let canonical_root = cx
        .update(|cx| store.scopes(&workspace.checkout_cwd, &[".".to_string()], cx))
        .await?
        .into_iter()
        .next();
    let cli = cx
        .update(|cx| store.enable(lead_id, &workspace.checkout_cwd, cx))
        .await?;
    let resumed_tasks: Vec<OrchestrationTask> = match previous.as_ref().filter(|_| paused) {
        Some(previous) => previous
            .tasks
            .iter()
            .map(|task| {
                let mut next = task.clone();
                if task.status == TaskStatus::Interrupted {
                    next.status = TaskStatus::Queued;
                    next.error = None;
                    next.delivered = true;
                    next.accepted = false;
                    next.active_dispatch_id = None;
                    next.recovery_prompt =
                        Some(task.recovery_prompt.clone().unwrap_or_else(|| {
                            recovery_turn(
                                previous
                                    .error
                                    .as_deref()
                                    .or(previous.last_pause_reason.as_deref())
                                    .unwrap_or("the run was paused"),
                            )
                        }));
                }
                next
            })
            .collect(),
        None => Vec::new(),
    };
    let paused_previous = previous.as_ref().filter(|_| paused);
    let mut harnesses: Vec<HarnessId> = Vec::new();
    for id in allowed_harnesses {
        if !harnesses.contains(id) {
            harnesses.push(*id);
        }
    }
    let attempt: Result<(), String> = async {
        // Refresh the child environment before its next turn.
        cx.update(|cx| host.stop(lead_id, cx)).await?;
        let (allowed_models, proposal_id, tasks) = match &approved {
            Some(approved) => (
                Some(approved.allowed_models.clone()),
                Some(approved.proposal_id.clone()),
                approved.tasks.clone(),
            ),
            None => (
                paused_previous.and_then(|run| run.allowed_models.clone()),
                paused_previous.and_then(|run| run.proposal_id.clone()),
                resumed_tasks,
            ),
        };
        commit(
            this,
            OrchestrationRun {
                version: 2,
                lead_id: lead_id.to_string(),
                cwd: lead.cwd.clone(),
                workspace: Some(workspace.clone()),
                canonical_root,
                status: RunStatus::Active,
                allowed_harnesses: harnesses,
                allowed_models,
                proposal_id,
                max_workers,
                cli,
                tasks,
                dispatches: Some(
                    paused_previous
                        .map(|run| run.dispatch_list().to_vec())
                        .unwrap_or_default(),
                ),
                error: None,
                continuations: 0,
                last_pause_reason: paused_previous
                    .and_then(|run| run.error.clone().or_else(|| run.last_pause_reason.clone())),
                requests: paused_previous
                    .map(|run| run.requests.clone())
                    .unwrap_or_default(),
                extra: Extra::new(),
            },
            cx,
        )
        .await
    }
    .await;
    if let Err(error) = attempt {
        cx.update(|cx| store.disable(lead_id, cx)).await?;
        return Err(error);
    }
    spawn_pump(this, cx);
    spawn_sync(this, cx);
    Ok(())
}

/// `handle`: run one control command for a lead. Each `requestId` applies at
/// most once; a retry with the same input gets the same answer.
pub async fn handle(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    request_id: &str,
    action: &str,
    input: &Map<String, Value>,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    check_fields(action, input)?;
    let signature =
        serde_json::to_string(&json!({ "action": action, "input": input })).unwrap_or_default();
    let key = format!("{lead_id}:{request_id}");
    let receipt = read(this, cx, |this, _| {
        this.persisted
            .get(lead_id)
            .and_then(|run| run.requests.get(request_id).cloned())
    })?;
    if let Some(previous) = receipt {
        if previous.signature != signature {
            return Err("Request ID was already used with different input".into());
        }
        return Ok(previous.result);
    }
    let pending = read(this, cx, |this, _| this.inflight.get(&key).cloned())?;
    if let Some((pending_signature, pending)) = pending {
        if pending_signature != signature {
            return Err("Request ID was already used with different input".into());
        }
        return pending.await;
    }
    if action == "wait" {
        return wait(this, lead_id, input, cx).await;
    }
    let previous = read(this, cx, |this, _| this.actions.clone())?;
    let result: SharedResult = {
        let this = this.clone();
        let lead_id = lead_id.to_string();
        let request_id = request_id.to_string();
        let action = action.to_string();
        let input = input.clone();
        let signature = signature.clone();
        let task = cx.spawn(async move |cx| {
            previous.await;
            let run = require_run(&this, &lead_id, cx)?;
            let inspect = action == "list" || action == "get";
            if run.status != RunStatus::Active && !inspect {
                return Err(Orchestrator::inactive_reason(&run));
            }
            if !inspect && run.requests.len() >= 512 {
                return Err("Run command limit reached. Stop the run and start another after reviewing the files.".into());
            }
            execute(&this, run, &action, &input, &request_id, &signature, cx).await
        });
        task.boxed_local().shared()
    };
    update(this, cx, |this, _| {
        this.actions = result.clone().map(|_| ()).boxed_local().shared();
        this.inflight
            .insert(key.clone(), (signature.clone(), result.clone()));
    })?;
    let outcome = result.await;
    update(this, cx, |this, _| {
        this.inflight.remove(&key);
    })
    .ok();
    spawn_pump(this, cx);
    outcome
}

fn input_task(
    run: &OrchestrationRun,
    input: &Map<String, Value>,
) -> Result<OrchestrationTask, String> {
    let id = text(input.get("taskId"), "taskId", 128)?;
    run.task(&id).cloned().ok_or_else(|| {
        "Task does not belong to this run. Use a taskId returned by delegate or list.".into()
    })
}

/// Persist the mutation and its retry receipt together, before dispatch.
async fn record(
    this: &WeakEntity<Orchestrator>,
    next: OrchestrationRun,
    request_id: &str,
    signature: &str,
    result: Value,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let mut next = next;
    next.requests.insert(
        request_id.to_string(),
        RequestReceipt {
            signature: signature.to_string(),
            result: result.clone(),
        },
    );
    commit(this, next, cx).await?;
    Ok(result)
}

fn dependent_started(run: &OrchestrationRun, id: &str) -> bool {
    run.tasks.iter().any(|entry| {
        entry.depends_on.iter().any(|dependency| dependency == id)
            && entry.status != TaskStatus::Queued
            && entry.status != TaskStatus::Cancelled
    })
}

async fn execute(
    this: &WeakEntity<Orchestrator>,
    run: Rc<OrchestrationRun>,
    action: &str,
    input: &Map<String, Value>,
    request_id: &str,
    signature: &str,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let lead_id = run.lead_id.clone();
    let lead_id = lead_id.as_str();
    let host = host(this, cx)?;
    match action {
        "list" => {
            let choices = cx.update(|cx| host.choices(cx));
            let view = read(this, cx, |this, cx| this.view(&run, cx))?;
            let harnesses: Vec<Value> = choices
                .into_iter()
                .filter(|choice| run.allowed_harnesses.contains(&choice.harness))
                .map(|choice| {
                    let models: Vec<_> = choice
                        .models
                        .iter()
                        .filter(|model| {
                            run.allowed_models.as_ref().is_none_or(|allowed| {
                                allowed.iter().any(|entry| {
                                    entry.harness == choice.harness && entry.model == model.id
                                })
                            })
                        })
                        .cloned()
                        .collect();
                    json!({ "harness": choice.harness, "models": models })
                })
                .collect();
            Ok(json!({ "run": view, "harnesses": harnesses }))
        }
        "get" => {
            // The fields list reports, plus this task's own prompt.
            let current = require_run(this, lead_id, cx)?;
            let target = input_task(&current, input)?;
            let pending = read(this, cx, |this, cx| this.pending_input(&target, cx))?;
            let mut value = serde_json::to_value(&target).unwrap_or(Value::Null);
            if let Some(object) = value.as_object_mut() {
                object.insert("runStatus".into(), json!(run.status));
                if run.status != RunStatus::Active {
                    object.insert(
                        "recovery".into(),
                        Value::String(Orchestrator::inactive_reason(&run)),
                    );
                }
                object.remove("scopes");
                if let Some(waiting) = Orchestrator::waiting_for(&current, &target) {
                    object.insert("waitingFor".into(), Value::String(waiting));
                }
                if let Some(pending) = pending {
                    object.insert(
                        "needsInput".into(),
                        serde_json::to_value(pending).unwrap_or(Value::Null),
                    );
                }
            }
            Ok(value)
        }
        "delegate" => {
            if run.tasks.len() >= 40 {
                return Err("This run has reached its 40-task limit".into());
            }
            let harness_name = text(input.get("harness"), "harness", 30_000)?;
            let harness = HarnessId::parse(&harness_name)
                .filter(|id| HARNESSES.contains(id) && run.allowed_harnesses.contains(id));
            let Some(harness) = harness else {
                let allowed: Vec<String> = run
                    .allowed_harnesses
                    .iter()
                    .map(|id| id.to_string())
                    .collect();
                return Err(format!(
                    "Harness \"{harness_name}\" is not allowed in this run. Allowed: {}.",
                    listed12(&allowed)
                ));
            };
            let choices = cx.update(|cx| host.choices(cx));
            let Some(choice) = choices.iter().find(|entry| entry.harness == harness) else {
                return Err("Worker harness is unavailable".into());
            };
            let permitted: Vec<String> = choice
                .models
                .iter()
                .filter(|model| {
                    run.allowed_models.as_ref().is_none_or(|allowed| {
                        allowed
                            .iter()
                            .any(|entry| entry.harness == harness && entry.model == model.id)
                    })
                })
                .map(|model| model.id.clone())
                .collect();
            let model = match input.get("model") {
                None | Some(Value::Null) => permitted.first().cloned(),
                Some(value) => Some(text(Some(value), "model", 256)?),
            };
            let Some(model) = model.filter(|model| permitted.contains(model)) else {
                let names = listed12(&permitted);
                return Err(format!(
                    "Choose a model ID returned by list for {harness}: {}.",
                    if names.is_empty() {
                        "none available"
                    } else {
                        &names
                    }
                ));
            };
            let title = text(input.get("title"), "title", 160)?;
            let prompt = text(input.get("prompt"), "prompt", 30_000)?;
            let files = strings(input.get("files"), "files", 64)?;
            if files.is_empty() {
                return Err("Declare at least one file/directory scope in files, or '.' for exclusive checkout access".into());
            }
            let depends_on = match input.get("dependsOn") {
                None | Some(Value::Null) => Vec::new(),
                Some(value) => strings(Some(value), "dependsOn", 40)?,
            };
            let missing: Vec<String> = depends_on
                .iter()
                .filter(|id| {
                    !run.tasks
                        .iter()
                        .any(|item| &item.id == *id && item.status != TaskStatus::Cancelled)
                })
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Err(format!(
                    "dependsOn must hold taskIds from this run; unknown or cancelled: {}.",
                    listed12(&missing)
                ));
            }
            let store = store(this, cx)?;
            let checkout = orchestration_checkout_cwd(&run);
            let scopes = cx.update(|cx| store.scopes(&checkout, &files, cx)).await?;
            let created = OrchestrationTask {
                id: new_id(),
                assignment_id: None,
                session_id: new_id(),
                title,
                harness,
                model,
                model_settings: None,
                prompt,
                files,
                scopes,
                write_scopes: None,
                scratch_dir: None,
                depends_on,
                status: TaskStatus::Queued,
                accepted: false,
                result: String::new(),
                error: None,
                recovery_prompt: None,
                delivered: true,
                workspace_policy: Some(WorkspacePolicy::IsolatedChild),
                workspace: None,
                active_dispatch_id: None,
                last_dispatch_id: None,
                accepted_dispatch_id: None,
                extra: Extra::new(),
            };
            let result = json!({
                "taskId": created.id,
                "sessionId": created.session_id,
                "status": "queued",
            });
            let mut next = (*require_run(this, lead_id, cx)?).clone();
            next.tasks.push(created);
            record(this, next, request_id, signature, result, cx).await
        }
        "message" | "retry" => {
            let current = require_run(this, lead_id, cx)?;
            let target = input_task(&current, input)?;
            if target.is_active() || target.status == TaskStatus::Queued {
                return Err(format!(
                    "Wait for this worker or cancel it before {}; {} is {}.",
                    if action == "message" {
                        "sending a new turn"
                    } else {
                        "retrying"
                    },
                    target.title,
                    target.status
                ));
            }
            if dependent_started(&run, &target.id) {
                return Err("A dependent task has already started; create a separate correction task after it finishes".into());
            }
            if action == "message" {
                let prompt = text(input.get("text"), "text", 30_000)?;
                let mut next = (*require_run(this, lead_id, cx)?).clone();
                next.map_task(&target.id, |entry| {
                    entry.prompt = prompt;
                    reset_for_turn(entry);
                });
                return record(
                    this,
                    next,
                    request_id,
                    signature,
                    json!({ "taskId": target.id, "status": "queued" }),
                    cx,
                )
                .await;
            }
            let files = strings(input.get("files"), "files", 64)?;
            if files.is_empty() {
                return Err("Declare at least one corrected project-relative file/directory scope, or '.' for the whole checkout".into());
            }
            let store = store(this, cx)?;
            let checkout = orchestration_checkout_cwd(require_run(this, lead_id, cx)?.as_ref());
            let scopes = cx.update(|cx| store.scopes(&checkout, &files, cx)).await?;
            let prompt = text(input.get("text"), "text", 30_000)?;
            let mut next = (*require_run(this, lead_id, cx)?).clone();
            let recorded_files = files.clone();
            next.map_task(&target.id, |entry| {
                entry.prompt = prompt;
                entry.files = files;
                entry.scopes = scopes;
                entry.write_scopes = None;
                reset_for_turn(entry);
            });
            record(
                this,
                next,
                request_id,
                signature,
                json!({ "taskId": target.id, "status": "queued", "files": recorded_files }),
                cx,
            )
            .await
        }
        "steer" => {
            let target = input_task(&require_run(this, lead_id, cx)?.as_ref().clone(), input)?;
            if target.status != TaskStatus::Running {
                return Err(format!(
                    "Only a running agent can be steered; {} is {}. {}",
                    target.title,
                    target.status,
                    if target.status == TaskStatus::Queued {
                        "It has not started yet, so edit it with message instead."
                    } else {
                        "Send it a fresh turn with message."
                    }
                ));
            }
            let guidance = text(input.get("text"), "text", 30_000)?;
            cx.update(|cx| host.steer(&target.session_id, &guidance, cx))
                .await?;
            let next = (*require_run(this, lead_id, cx)?).clone();
            record(
                this,
                next,
                request_id,
                signature,
                json!({ "taskId": target.id, "steered": true }),
                cx,
            )
            .await
        }
        "cancel" => {
            let target = input_task(&require_run(this, lead_id, cx)?.as_ref().clone(), input)?;
            cancel_task(this, lead_id, &target.id, cx).await?;
            let next = (*require_run(this, lead_id, cx)?).clone();
            record(
                this,
                next,
                request_id,
                signature,
                json!({ "cancelled": true }),
                cx,
            )
            .await
        }
        "respond" => {
            let target = input_task(&require_run(this, lead_id, cx)?.as_ref().clone(), input)?;
            let pending = read(this, cx, |this, cx| this.pending_input(&target, cx))?
                .filter(|pending| pending.kind == InputKind::Approval);
            let Some(pending) = pending else {
                return Err(format!(
                    "{} is not waiting on an approval. Read needsInput from list or wait before responding.",
                    target.title
                ));
            };
            if !same_request(input.get("requestId"), pending.request_id) {
                return Err(format!(
                    "Stale requestId. {} is waiting on {}.",
                    target.title, pending.request_id
                ));
            }
            let decision = match text(input.get("decision"), "decision", 16)?.as_str() {
                "allow" => ApprovalDecision::Allow,
                "deny" => ApprovalDecision::Deny,
                _ => return Err("decision must be \"allow\" or \"deny\"".into()),
            };
            cx.update(|cx| {
                host.respond_approval(&target.session_id, pending.request_id, decision, cx)
            });
            let next = (*require_run(this, lead_id, cx)?).clone();
            record(
                this,
                next,
                request_id,
                signature,
                json!({ "taskId": target.id, "decision": decision }),
                cx,
            )
            .await
        }
        "answer" => {
            let target = input_task(&require_run(this, lead_id, cx)?.as_ref().clone(), input)?;
            let pending = read(this, cx, |this, cx| this.pending_input(&target, cx))?
                .filter(|pending| pending.kind == InputKind::Question);
            let Some(pending) = pending else {
                return Err(format!(
                    "{} is not waiting on a question. Read needsInput from list or wait before answering.",
                    target.title
                ));
            };
            if !same_request(input.get("requestId"), pending.request_id) {
                return Err(format!(
                    "Stale requestId. {} is waiting on {}.",
                    target.title, pending.request_id
                ));
            }
            let reply = if input.get("skip") == Some(&Value::Bool(true)) {
                UserQuestionReply::Skipped
            } else {
                UserQuestionReply::Answered {
                    answers: question_answers(
                        input.get("answers"),
                        pending.questions.as_deref().unwrap_or(&[]),
                    )?,
                    custom: None,
                }
            };
            let answered = matches!(reply, UserQuestionReply::Answered { .. });
            cx.update(|cx| host.answer_question(&target.session_id, pending.request_id, reply, cx));
            let next = (*require_run(this, lead_id, cx)?).clone();
            record(
                this,
                next,
                request_id,
                signature,
                json!({ "taskId": target.id, "answered": answered }),
                cx,
            )
            .await
        }
        "review" => review(this, &run, input, request_id, signature, cx).await,
        "finish" => finish(this, lead_id, request_id, signature, cx).await,
        _ => Err("Unknown action. Run control --help.".into()),
    }
}

/// `input.requestId !== pending.requestId`, as strict equality on a JSON
/// number.
fn same_request(value: Option<&Value>, request_id: i64) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|value| value == request_id as f64)
}

/// The fields `message` and `retry` reset for a fresh turn.
fn reset_for_turn(entry: &mut OrchestrationTask) {
    entry.status = TaskStatus::Queued;
    entry.accepted = false;
    entry.result = String::new();
    entry.error = None;
    entry.recovery_prompt = None;
    entry.delivered = true;
    entry.active_dispatch_id = None;
    entry.accepted_dispatch_id = None;
}

async fn review(
    this: &WeakEntity<Orchestrator>,
    run: &OrchestrationRun,
    input: &Map<String, Value>,
    request_id: &str,
    signature: &str,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let lead_id = run.lead_id.as_str();
    let host = host(this, cx)?;
    let mut target = input_task(&require_run(this, lead_id, cx)?.as_ref().clone(), input)?;
    if target.status != TaskStatus::Completed {
        return Err(format!(
            "Only a completed result can be accepted; {} is {}. {}",
            target.title,
            target.status,
            if matches!(
                target.status,
                TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Interrupted
            ) {
                "Send it another turn with message, retry it with corrected scope when necessary, or drop it with cancel."
            } else {
                "Wait for it to finish, or cancel it."
            }
        ));
    }
    let Some(dispatch_id) = target.last_dispatch_id.clone() else {
        return Err("This task has no completed dispatch to review".into());
    };
    let isolated = target.workspace_policy != Some(WorkspacePolicy::Shared);
    if !target.accepted {
        if isolated {
            if target.workspace.is_none() {
                return Err("This worker's isolated checkout is unavailable. Its changes were not accepted.".into());
            }
            patch_dispatch(
                this,
                lead_id,
                &dispatch_id,
                |dispatch| {
                    dispatch.stage = DispatchStage::IntegrationStarted;
                    dispatch.cleanup_error = None;
                },
                cx,
            )
            .await?;
            let current = require_run(this, lead_id, cx)?;
            cx.update(|cx| host.integrate_worker(&current, &target, cx))
                .await?;
        }
        let mut next = (*require_run(this, lead_id, cx)?).clone();
        next.map_task(&target.id, |entry| {
            entry.accepted = true;
            entry.accepted_dispatch_id = Some(dispatch_id.clone());
        });
        next.map_dispatch(Some(&dispatch_id), |dispatch| {
            if isolated {
                dispatch.stage = DispatchStage::Integrated;
            }
            dispatch.updated_at = now_ms();
        });
        commit(this, next, cx).await?;
        if let Some(updated) = require_run(this, lead_id, cx)?.task(&target.id) {
            target = updated.clone();
        }
    }
    let mut cleaned = !isolated;
    let mut cleanup_error: Option<String> = None;
    if isolated {
        let current = require_run(this, lead_id, cx)?;
        match cx
            .update(|cx| host.cleanup_worker(&current, &target, false, cx))
            .await
        {
            Ok(result) => cleaned = result,
            Err(error) => cleanup_error = Some(error),
        }
        let mut next = (*require_run(this, lead_id, cx)?).clone();
        if cleaned {
            next.map_task(&target.id, |entry| entry.workspace = None);
        }
        let error = cleanup_error.clone();
        next.map_dispatch(Some(&dispatch_id), |dispatch| {
            if cleaned {
                dispatch.stage = DispatchStage::Cleaned;
            }
            dispatch.cleanup_error = error;
            dispatch.updated_at = now_ms();
        });
        commit(this, next, cx).await?;
    }
    let mut result = json!({ "accepted": true, "integrated": isolated, "cleaned": cleaned });
    if let Some(error) = cleanup_error {
        result["cleanupError"] = Value::String(error);
    }
    let next = (*require_run(this, lead_id, cx)?).clone();
    record(this, next, request_id, signature, result, cx).await
}

async fn finish(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    request_id: &str,
    signature: &str,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let host = host(this, cx)?;
    let outstanding: Vec<String> = require_run(this, lead_id, cx)?
        .tasks
        .iter()
        .filter(|entry| entry.status != TaskStatus::Cancelled && !entry.accepted)
        .map(|entry| format!("{} ({})", entry.title, entry.status))
        .collect();
    if !outstanding.is_empty() {
        return Err(format!(
            "Review all remaining tasks before finishing. Outstanding: {}. Accept a completed task with review, correct a failed or blocked task with message/retry, or drop it with cancel.",
            listed12(&outstanding)
        ));
    }
    let mut cleanup_pending: Vec<String> = Vec::new();
    let retained: Vec<OrchestrationTask> = require_run(this, lead_id, cx)?
        .tasks
        .iter()
        .filter(|entry| {
            entry.workspace_policy != Some(WorkspacePolicy::Shared) && entry.workspace.is_some()
        })
        .cloned()
        .collect();
    for retained in retained {
        let checkout = retained
            .workspace
            .as_ref()
            .map(|workspace| workspace.checkout_cwd.clone())
            .unwrap_or_default();
        if !retained.accepted {
            if !cleanup_unchanged_worker(this, lead_id, &retained.id, cx).await? {
                cleanup_pending.push(checkout);
            }
            continue;
        }
        let dispatch_id = retained.accepted_dispatch_id.clone();
        let stage = require_run(this, lead_id, cx)?
            .dispatch_list()
            .iter()
            .find(|entry| Some(&entry.id) == dispatch_id.as_ref())
            .map(|entry| entry.stage);
        if stage == Some(DispatchStage::Cleaned) {
            continue;
        }
        let attempt: Result<bool, String> = async {
            let current = require_run(this, lead_id, cx)?;
            let cleaned = cx
                .update(|cx| host.cleanup_worker(&current, &retained, false, cx))
                .await?;
            if cleaned {
                let mut next = (*require_run(this, lead_id, cx)?).clone();
                next.map_task(&retained.id, |entry| entry.workspace = None);
                next.map_dispatch(dispatch_id.as_deref(), |entry| {
                    entry.stage = DispatchStage::Cleaned;
                    entry.cleanup_error = None;
                    entry.updated_at = now_ms();
                });
                commit(this, next, cx).await?;
            }
            Ok(cleaned)
        }
        .await;
        match attempt {
            Ok(true) => {}
            Ok(false) => cleanup_pending.push(checkout),
            Err(error) => {
                cleanup_pending.push(checkout);
                if let Some(dispatch_id) = &dispatch_id {
                    patch_dispatch(
                        this,
                        lead_id,
                        dispatch_id,
                        |dispatch| dispatch.cleanup_error = Some(error),
                        cx,
                    )
                    .await?;
                }
            }
        }
    }
    let mut next = (*require_run(this, lead_id, cx)?).clone();
    next.status = RunStatus::Finished;
    let mut result = json!({ "finished": true });
    if !cleanup_pending.is_empty() {
        result["cleanupPending"] = json!(cleanup_pending);
    }
    let result = record(this, next, request_id, signature, result, cx).await?;
    let store = store(this, cx)?;
    cx.update(|cx| store.disable(lead_id, cx)).await?;
    Ok(result)
}

/// `wait`: long-poll until a run changes or a worker blocks on the lead.
async fn wait(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    input: &Map<String, Value>,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let run = require_run(this, lead_id, cx)?;
    let seconds = match input.get("timeoutSeconds") {
        None | Some(Value::Null) => Some(20.0),
        Some(value) => value.as_f64(),
    }
    .filter(|seconds| seconds.is_finite() && (0.0..=25.0).contains(seconds))
    .ok_or_else(|| "timeoutSeconds must be 0 to 25".to_string())?;
    // A worker blocking on the lead changes no run state, so watch for that
    // separately; otherwise the lead sleeps while an agent waits on it.
    let blocked = read(this, cx, |this, _| this.blocked.get(lead_id).cloned())?;
    // Input that arrived before `wait` is already actionable. Only long-poll
    // while every running worker can still make progress without the lead.
    let idle_blocked = read(this, cx, |this, cx| this.blocked_keys(&run, cx).is_empty())?;
    if run.status == RunStatus::Active
        && idle_blocked
        && run
            .tasks
            .iter()
            .any(|task| task.is_active() || task.status == TaskStatus::Queued)
    {
        let (listener, mut changes) = update(this, cx, |this, _| this.subscribe())?;
        let mut timer = cx
            .background_executor()
            .timer(Duration::from_secs_f64(seconds))
            .fuse();
        loop {
            futures::select_biased! {
                change = changes.next() => {
                    if change.is_none() {
                        break;
                    }
                    let finished = read(this, cx, |this, _| {
                        !this.run(lead_id).is_some_and(|current| Rc::ptr_eq(&current, &run))
                            || this.blocked.get(lead_id) != blocked.as_ref()
                    })?;
                    if finished {
                        break;
                    }
                }
                _ = timer => break,
            }
        }
        update(this, cx, |this, _| {
            this.listeners.remove(&listener);
        })
        .ok();
    }
    let current = require_run(this, lead_id, cx)?;
    read(this, cx, |this, cx| this.view(&current, cx))
}

/// `pump`: dispatch every queued task that has a worker slot, accepted
/// dependencies, and no overlapping scope.
async fn pump(this: &WeakEntity<Orchestrator>, cx: &mut AsyncApp) {
    let start = update(this, cx, |this, _| {
        if this.pumping {
            this.pump_again = true;
            return None;
        }
        this.host.as_ref()?;
        this.pumping = true;
        Some(this.runs.clone())
    });
    let Ok(Some(initial_runs)) = start else {
        return;
    };
    if let Err(error) = pump_runs(this, &initial_runs, cx).await {
        log::error!("[orchestration] dispatch failed: {error}");
    }
    let again = update(this, cx, |this, _| {
        this.pumping = false;
        std::mem::take(&mut this.pump_again)
    })
    .unwrap_or(false);
    if again {
        spawn_pump(this, cx);
    }
}

async fn pump_runs(
    this: &WeakEntity<Orchestrator>,
    initial_runs: &[Rc<OrchestrationRun>],
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let host = host(this, cx)?;
    for initial in initial_runs {
        let Some(initial_run) = current_run(this, &initial.lead_id, cx)? else {
            continue;
        };
        if initial_run.status != RunStatus::Active {
            continue;
        }
        for initial_task in &initial_run.tasks {
            let Some(run) = current_run(this, &initial.lead_id, cx)? else {
                break;
            };
            if run.status != RunStatus::Active
                || run.tasks.iter().filter(|task| task.is_active()).count() as i64
                    >= run.max_workers
            {
                break;
            }
            let Some(task) = run.task(&initial_task.id).cloned() else {
                continue;
            };
            if task.status != TaskStatus::Queued
                || task
                    .depends_on
                    .iter()
                    .any(|id| !run.task(id).is_some_and(|entry| entry.accepted))
            {
                continue;
            }
            if run
                .tasks
                .iter()
                .any(|entry| entry.is_active() && scopes_overlap(&entry.scopes, &task.scopes))
            {
                continue;
            }
            if cx.update(|cx| host.session(&run.lead_id, cx)).is_none() {
                stop_run(this, &run.lead_id, cx).await?;
                break;
            }
            let dispatch_id = new_id();
            let now = now_ms();
            let dispatch = OrchestrationDispatch {
                id: dispatch_id.clone(),
                task_id: task.id.clone(),
                session_id: task.session_id.clone(),
                workspace: orchestration_workspace(&run),
                state: DispatchState::Starting,
                stage: DispatchStage::Accepted,
                started_at: now,
                updated_at: now,
                result: None,
                error: None,
                cleanup_error: None,
                extra: Extra::new(),
            };
            // Persist authority before any external worker/resource operation.
            let mut next = (*run).clone();
            next.map_task(&task.id, |entry| {
                entry.status = TaskStatus::Running;
                entry.active_dispatch_id = Some(dispatch_id.clone());
                entry.accepted_dispatch_id = None;
            });
            next.dispatches.get_or_insert_with(Vec::new).push(dispatch);
            commit(this, next, cx).await?;
            update(this, cx, |this, _| {
                this.write_checks
                    .insert(dispatch_id.clone(), HashMap::new());
            })?;
            let attempt = dispatch_task(this, &host, &run.lead_id, &task, &dispatch_id, cx).await;
            if let Err(error) = attempt {
                settle(
                    this,
                    &run.lead_id,
                    &task.id,
                    ControlOutcome::failed(error),
                    &dispatch_id,
                    cx,
                )
                .await?;
            }
        }
    }
    Ok(())
}

fn still_dispatched(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task_id: &str,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let run = current_run(this, lead_id, cx)?;
    Ok(run.is_some_and(|run| {
        run.status == RunStatus::Active
            && run
                .task(task_id)
                .is_some_and(|task| task.status == TaskStatus::Running)
    }))
}

/// The body of `pump`'s `try`: prepare the worker and send its turn.
async fn dispatch_task(
    this: &WeakEntity<Orchestrator>,
    host: &Rc<dyn OrchestrationHost>,
    lead_id: &str,
    task: &OrchestrationTask,
    dispatch_id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let available = cx.update(|cx| host.choices(cx)).iter().any(|choice| {
        choice.harness == task.harness && choice.models.iter().any(|model| model.id == task.model)
    });
    if !available {
        return Err(
            "The assigned harness/model is no longer available. Review this task before retrying."
                .into(),
        );
    }
    let active_run = require_run(this, lead_id, cx)?;
    let active_task = active_run
        .task(&task.id)
        .cloned()
        .ok_or_else(|| "The task is no longer part of this run".to_string())?;
    let prepared = cx
        .update(|cx| host.create_worker(&active_run, &active_task, cx))
        .await?;
    if !still_dispatched(this, lead_id, &task.id, cx)? {
        return Ok(());
    }
    let store = store(this, cx)?;
    let write_scopes = cx
        .update(|cx| store.scopes(&prepared.workspace.checkout_cwd, &task.files, cx))
        .await?;
    let mut next = (*require_run(this, lead_id, cx)?).clone();
    next.map_task(&task.id, |entry| {
        entry.workspace = Some(prepared.workspace.clone());
        entry.scratch_dir = prepared.scratch_dir.clone();
        entry.write_scopes = Some(write_scopes);
    });
    next.map_dispatch(Some(dispatch_id), |entry| {
        entry.workspace = prepared.workspace.clone();
        entry.stage = DispatchStage::SessionPrepared;
        entry.updated_at = now_ms();
    });
    commit(this, next, cx).await?;
    if !still_dispatched(this, lead_id, &task.id, cx)? {
        return Ok(());
    }
    let prompt = worker_turn_prompt(
        task.recovery_prompt.as_deref().unwrap_or(&task.prompt),
        &task.files,
        prepared.scratch_dir.as_deref(),
    );
    let weak = this.clone();
    let (lead, task_id, dispatch) = (
        lead_id.to_string(),
        task.id.clone(),
        dispatch_id.to_string(),
    );
    cx.update(|cx| {
        host.submit(
            &task.session_id,
            &prompt,
            Box::new(move |outcome, cx| {
                let this = weak.clone();
                cx.spawn(async move |cx| {
                    if let Err(error) = settle(&this, &lead, &task_id, outcome, &dispatch, cx).await
                    {
                        log::error!("[orchestration] settle failed: {error}");
                    }
                })
                .detach();
            }),
            cx,
        )
    });
    let still_active = current_run(this, lead_id, cx)?.is_some_and(|run| {
        run.task(&task.id)
            .is_some_and(|entry| entry.active_dispatch_id.as_deref() == Some(dispatch_id))
    });
    if still_active {
        patch_dispatch(
            this,
            lead_id,
            dispatch_id,
            |dispatch| {
                dispatch.state = DispatchState::Running;
                dispatch.stage = DispatchStage::TurnSubmitted;
            },
            cx,
        )
        .await?;
    }
    Ok(())
}

fn task_status(outcome: ControlStatus) -> (TaskStatus, DispatchState) {
    match outcome {
        ControlStatus::Completed => (TaskStatus::Completed, DispatchState::Completed),
        ControlStatus::Failed => (TaskStatus::Failed, DispatchState::Failed),
        ControlStatus::Cancelled => (TaskStatus::Cancelled, DispatchState::Cancelled),
    }
}

/// `settle`: record how a dispatched turn ended. Only the task's active
/// dispatch may settle it.
async fn settle(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task_id: &str,
    outcome: ControlOutcome,
    dispatch_id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let task = current_run(this, lead_id, cx)?.and_then(|run| run.task(task_id).cloned());
    let Some(task) = task.filter(|task| task.active_dispatch_id.as_deref() == Some(dispatch_id))
    else {
        return Ok(());
    };
    if task.status == TaskStatus::Cancelling {
        if !outcome.text.is_empty() {
            let result = slice_tail(&outcome.text, 20_000).to_string();
            patch_task(this, lead_id, task_id, |entry| entry.result = result, cx).await?;
        }
        return Ok(());
    }
    if task.status != TaskStatus::Running {
        return Ok(());
    }
    // A fast final response must not make an unchecked write reviewable.
    let checks: Vec<Tail> = read(this, cx, |this, _| {
        this.write_checks
            .get(dispatch_id)
            .map(|checks| checks.values().cloned().collect())
            .unwrap_or_default()
    })?;
    futures::future::join_all(checks).await;
    let task = current_run(this, lead_id, cx)?.and_then(|run| run.task(task_id).cloned());
    if !task.is_some_and(|task| {
        task.status == TaskStatus::Running
            && task.active_dispatch_id.as_deref() == Some(dispatch_id)
    }) {
        return Ok(());
    }
    update(this, cx, |this, _| {
        this.write_checks.remove(dispatch_id);
    })?;
    let (status, state) = task_status(outcome.status);
    let result = slice_tail(&outcome.text, 20_000).to_string();
    let mut next = (*require_run(this, lead_id, cx)?).clone();
    next.map_task(task_id, |entry| {
        entry.status = status;
        entry.result = result.clone();
        entry.error = outcome.error.clone();
        entry.recovery_prompt = None;
        entry.delivered = false;
        entry.accepted = false;
        entry.active_dispatch_id = None;
        entry.last_dispatch_id = Some(dispatch_id.to_string());
    });
    next.map_dispatch(Some(dispatch_id), |dispatch| {
        dispatch.state = state;
        dispatch.stage = DispatchStage::Settled;
        dispatch.updated_at = now_ms();
        dispatch.result = Some(result.clone());
        dispatch.error = outcome.error.clone();
    });
    commit(this, next, cx).await?;
    if outcome.status == ControlStatus::Failed {
        cleanup_unchanged_worker(this, lead_id, task_id, cx).await?;
    }
    spawn_pump(this, cx);
    spawn_sync(this, cx);
    Ok(())
}

/// `cancelTask`: stop the worker if it runs, then cancel the task.
pub async fn cancel_task(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task_id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let task = current_run(this, lead_id, cx)?.and_then(|run| run.task(task_id).cloned());
    let Some(task) = task.filter(|task| task.status != TaskStatus::Cancelled) else {
        return Ok(());
    };
    let dispatch_id = task.active_dispatch_id.clone();
    if task.is_active() {
        patch_task(
            this,
            lead_id,
            task_id,
            |entry| entry.status = TaskStatus::Cancelling,
            cx,
        )
        .await?;
        let host = host(this, cx)?;
        cx.update(|cx| host.stop(&task.session_id, cx)).await?;
    }
    let Some(run) = current_run(this, lead_id, cx)? else {
        return Ok(());
    };
    let mut next = (*run).clone();
    next.map_task(task_id, |entry| {
        entry.status = TaskStatus::Cancelled;
        entry.accepted = false;
        entry.delivered = false;
        entry.active_dispatch_id = None;
        entry.recovery_prompt = None;
        if let Some(dispatch_id) = &dispatch_id {
            entry.last_dispatch_id = Some(dispatch_id.clone());
        }
    });
    next.map_dispatch(dispatch_id.as_deref(), |dispatch| {
        dispatch.state = DispatchState::Cancelled;
        dispatch.stage = DispatchStage::Settled;
        dispatch.updated_at = now_ms();
    });
    commit(this, next, cx).await?;
    if let Some(dispatch_id) = &dispatch_id {
        update(this, cx, |this, _| {
            this.write_checks.remove(dispatch_id);
        })?;
    }
    cleanup_unchanged_worker(this, lead_id, task_id, cx).await?;
    spawn_pump(this, cx);
    Ok(())
}

/// Stop a running worker and mark it `status`, the shared body of
/// `interruptTask` and `blockTask`.
async fn halt_task(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task_id: &str,
    reason: &str,
    blocked: bool,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let task = current_run(this, lead_id, cx)?.and_then(|run| run.task(task_id).cloned());
    let Some(task) = task.filter(|task| task.status == TaskStatus::Running) else {
        return Ok(false);
    };
    let dispatch_id = task.active_dispatch_id.clone();
    let error = reason.to_string();
    patch_task(
        this,
        lead_id,
        task_id,
        |entry| {
            entry.status = TaskStatus::Cancelling;
            entry.error = Some(error);
        },
        cx,
    )
    .await?;
    let host = host(this, cx)?;
    cx.update(|cx| host.stop(&task.session_id, cx)).await?;
    let Some(run) = current_run(this, lead_id, cx)? else {
        return Ok(false);
    };
    let mut next = (*run).clone();
    next.map_task(task_id, |entry| {
        entry.status = if blocked {
            TaskStatus::Blocked
        } else {
            TaskStatus::Interrupted
        };
        entry.accepted = false;
        entry.delivered = !blocked;
        entry.active_dispatch_id = None;
        entry.error = Some(reason.to_string());
        entry.recovery_prompt = (!blocked).then(|| recovery_turn(reason));
        if let Some(dispatch_id) = &dispatch_id {
            entry.last_dispatch_id = Some(dispatch_id.clone());
        }
    });
    next.map_dispatch(dispatch_id.as_deref(), |dispatch| {
        dispatch.state = if blocked {
            DispatchState::Blocked
        } else {
            DispatchState::Interrupted
        };
        dispatch.stage = DispatchStage::Settled;
        dispatch.updated_at = now_ms();
        dispatch.error = Some(reason.to_string());
    });
    commit(this, next, cx).await?;
    if let Some(dispatch_id) = &dispatch_id {
        update(this, cx, |this, _| {
            this.write_checks.remove(dispatch_id);
        })?;
    }
    Ok(true)
}

/// `blockTask`: stop only this worker after a write outside its scope.
async fn block_task(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task_id: &str,
    reason: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    if halt_task(this, lead_id, task_id, reason, true, cx).await? {
        spawn_pump(this, cx);
        spawn_sync(this, cx);
    }
    Ok(())
}

/// `pause`: a paused run has no supervisor, so its agents stop with it.
/// Leaving them editing with nobody reviewing is how a run quietly diverges
/// from what the user approved. Their worktrees are retained. Queued work is
/// left alone: `pump` will not dispatch while paused, so it resumes intact.
pub async fn pause(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    error: &str,
    patch: Option<RunPatch>,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let Some(run) = current_run(this, lead_id, cx)?.filter(|run| run.status == RunStatus::Active)
    else {
        return Ok(());
    };
    let mut next = (*run).clone();
    if let Some(patch) = patch {
        patch(&mut next);
    }
    next.status = RunStatus::Paused;
    next.error = Some(error.to_string());
    next.last_pause_reason = Some(error.to_string());
    commit(this, next, cx).await?;
    // Serialize state transitions: parallel commits here can resurrect an
    // already-stopped worker from an older snapshot.
    let running: Vec<String> = require_run(this, lead_id, cx)?
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Running)
        .map(|task| task.id.clone())
        .collect();
    for task_id in running {
        halt_task(this, lead_id, &task_id, error, false, cx).await?;
    }
    Ok(())
}

fn spawn_pause(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    error: String,
    patch: Option<RunPatch>,
    cx: &mut App,
) {
    let this = this.clone();
    let lead_id = lead_id.to_string();
    cx.spawn(async move |cx| {
        if let Err(error) = pause(&this, &lead_id, &error, patch, cx).await {
            log::error!("[orchestration] pause failed: {error}");
        }
    })
    .detach();
}

/// `stopRun`: stop the lead and every worker, cancel queued work, and
/// release the control grant.
pub async fn stop_run(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let Some(run) = current_run(this, lead_id, cx)? else {
        return Ok(());
    };
    let mut save_error: Option<String> = None;
    let mut stopping = (*run).clone();
    stopping.status = RunStatus::Stopped;
    if let Err(error) = commit(this, stopping, cx).await {
        save_error = Some(error);
    }
    let ids: Vec<String> = std::iter::once(lead_id.to_string())
        .chain(
            run.tasks
                .iter()
                .filter(|task| task.is_active())
                .map(|task| task.session_id.clone()),
        )
        .collect();
    let stops = ids.iter().map(|id| {
        let this = this.clone();
        let mut cx = cx.clone();
        let id = id.clone();
        async move { stop_session(&this, &id, &mut cx).await }
    });
    for result in futures::future::join_all(stops).await {
        result?;
    }
    let latest = require_run(this, lead_id, cx)?;
    let mut stopped = (*latest).clone();
    stopped.status = RunStatus::Stopped;
    let active_dispatches: HashSet<String> = latest
        .tasks
        .iter()
        .filter(|task| task.is_active())
        .filter_map(|task| task.active_dispatch_id.clone())
        .collect();
    for task in &mut stopped.tasks {
        if task.is_active() || task.status == TaskStatus::Queued {
            task.status = TaskStatus::Cancelled;
            task.accepted = false;
            task.delivered = true;
            if let Some(dispatch) = task.active_dispatch_id.take() {
                task.last_dispatch_id = Some(dispatch);
            }
        }
    }
    let now = now_ms();
    for dispatch in stopped.dispatches.get_or_insert_with(Vec::new) {
        if active_dispatches.contains(&dispatch.id) {
            dispatch.state = DispatchState::Cancelled;
            dispatch.stage = DispatchStage::Settled;
            dispatch.updated_at = now;
        }
    }
    update(this, cx, |this, _| {
        for task in &latest.tasks {
            if let Some(dispatch) = &task.active_dispatch_id {
                this.write_checks.remove(dispatch);
            }
        }
    })?;
    if let Err(error) = commit(this, stopped.clone(), cx).await {
        save_error = Some(error);
    }
    if save_error.is_none() {
        let unaccepted: Vec<String> = stopped
            .tasks
            .iter()
            .filter(|task| !task.accepted)
            .map(|task| task.id.clone())
            .collect();
        for task_id in unaccepted {
            cleanup_unchanged_worker(this, lead_id, &task_id, cx).await?;
        }
    }
    update(this, cx, |this, cx| {
        let after_cleanup = this
            .run(lead_id)
            .map(|run| (*run).clone())
            .unwrap_or(stopped);
        let mut next = after_cleanup;
        if let Some(error) = &save_error {
            next.error = Some(format!("Run stopped; could not save history: {error}"));
        }
        this.map_run(lead_id, |_| next);
        this.emit(cx);
    })?;
    let store = store(this, cx)?;
    cx.update(|cx| store.disable(lead_id, cx)).await
}

/// `stopForSession`: `None` when the session has no run to stop.
pub fn stop_for_session(
    entity: &Entity<Orchestrator>,
    id: &str,
    cx: &mut App,
) -> Option<Task<Result<(), String>>> {
    let run = entity.read(cx).for_session(id)?;
    if run.status != RunStatus::Active
        && run.status != RunStatus::Paused
        && !run.tasks.iter().any(OrchestrationTask::is_active)
    {
        return None;
    }
    let this = entity.downgrade();
    let id = id.to_string();
    if run.lead_id == id {
        return Some(cx.spawn(async move |cx| stop_run(&this, &id, cx).await));
    }
    let task_id = run
        .tasks
        .iter()
        .find(|task| task.session_id == id)
        .map(|task| task.id.clone())?;
    let lead_id = run.lead_id.clone();
    Some(cx.spawn(async move |cx| cancel_task(&this, &lead_id, &task_id, cx).await))
}

/// `deleteSession`: drain control writes before the database removes a lead
/// or one of its workers.
pub async fn delete_session<F>(
    this: &WeakEntity<Orchestrator>,
    id: &str,
    remove: impl FnOnce() -> F + 'static,
    cx: &mut AsyncApp,
) -> Result<(), String>
where
    F: std::future::Future<Output = Result<(), String>> + 'static,
{
    let previous = read(this, cx, |this, _| this.actions.clone())?;
    let task: Shared<Task<Result<(), String>>> = {
        let this = this.clone();
        let id = id.to_string();
        cx.spawn(async move |cx| {
            previous.await;
            let run = read(&this, cx, |this, _| this.for_session(&id))?;
            if let Some(run) = &run
                && (run.status == RunStatus::Active
                    || run.status == RunStatus::Paused
                    || run.tasks.iter().any(OrchestrationTask::is_active))
            {
                stop_run(&this, &run.lead_id, cx).await?;
            }
            let saves = read(&this, cx, |this, _| this.saves.clone())?;
            saves.await;
            remove().await?;
            update(&this, cx, |this, _| {
                this.deleted.insert(id.clone());
            })?;
            let Some(run) = run else {
                return Ok(());
            };
            update(&this, cx, |this, cx| {
                this.runs.retain(|entry| entry.lead_id != run.lead_id);
                this.persisted.remove(&run.lead_id);
                this.blocked.remove(&run.lead_id);
                this.announced.remove(&run.lead_id);
                this.emit(cx);
            })?;
            if run.lead_id != id {
                // Read the transaction's pruned graph; never save the pre-delete snapshot.
                let store = store(&this, cx)?;
                match cx.update(|cx| store.load(&run.lead_id, cx)).await {
                    Ok(Some(updated)) => {
                        let normalized = Rc::new(normalize_orchestration_run(&updated));
                        update(&this, cx, |this, cx| {
                            this.runs.push(normalized.clone());
                            this.persisted
                                .insert(run.lead_id.clone(), normalized.clone());
                            this.emit(cx);
                        })?;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        log::error!(
                            "[orchestration] could not reload orchestration after deletion: {error}"
                        );
                        update(&this, cx, |this, _| {
                            this.loaded.remove(&run.lead_id);
                        })?;
                    }
                }
            }
            Ok(())
        })
        .shared()
    };
    update(this, cx, |this, _| {
        this.actions = task.clone().map(|_| ()).boxed_local().shared();
    })?;
    task.await
}

/// The waking half of `sync`: deliver results and blocked questions to an
/// idle lead in one continuation turn.
async fn continue_lead(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let host = host(this, cx)?;
    let Some(current) = current_run(this, lead_id, cx)? else {
        return Ok(());
    };
    let session = cx.update(|cx| host.session(lead_id, cx));
    if current.status != RunStatus::Active
        || !session.is_some_and(|session| {
            !session.is_busy()
                && !session
                    .queued_messages
                    .as_ref()
                    .is_some_and(|queue| !queue.is_empty())
        })
    {
        return Ok(());
    }
    let results: Vec<OrchestrationTask> = current
        .tasks
        .iter()
        .filter(|task| !task.delivered && !task.is_active() && task.status != TaskStatus::Queued)
        .cloned()
        .collect();
    let seen = read(this, cx, |this, _| {
        this.announced.get(lead_id).cloned().unwrap_or_default()
    })?;
    let waiting: Vec<(OrchestrationTask, PendingInput, String)> = read(this, cx, |this, cx| {
        current
            .tasks
            .iter()
            .filter_map(|task| {
                let pending = this.pending_input(task, cx)?;
                let key = format!("{}:{}", task.id, pending.request_id);
                (!seen.contains(&key)).then(|| (task.clone(), pending, key))
            })
            .collect()
    })?;
    if results.is_empty() && waiting.is_empty() {
        return Ok(());
    }
    if current.continuations >= 20 {
        return pause(
            this,
            lead_id,
            "Automatic continuation limit reached. Its agents were stopped; review and resume the run.",
            None,
            cx,
        )
        .await;
    }
    let result_ids: HashSet<String> = results.iter().map(|task| task.id.clone()).collect();
    let mut next = (*current).clone();
    next.continuations += 1;
    next.last_pause_reason = None;
    for task in &mut next.tasks {
        if result_ids.contains(&task.id) {
            task.delivered = true;
        }
    }
    commit(this, next, cx).await?;
    update(this, cx, |this, _| {
        let mut keys = seen.clone();
        keys.extend(waiting.iter().map(|(_, _, key)| key.clone()));
        this.announced.insert(lead_id.to_string(), keys);
    })?;
    let summary = results
        .iter()
        .map(|task| {
            format!(
                "{} — {}: {}\n{}\n{}",
                task.id,
                task.title,
                task.status,
                task.error.as_deref().unwrap_or(""),
                slice_tail(&task.result, 4000)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let asks = waiting
        .iter()
        .map(|(task, pending, _)| {
            format!(
                "{} — {} needs {} (requestId {}): {}\n{}{}",
                task.id,
                task.title,
                if pending.kind == InputKind::Approval {
                    "an approval"
                } else {
                    "an answer"
                },
                pending.request_id,
                pending.label,
                pending
                    .detail
                    .as_deref()
                    .map(|detail| monocode_core::js::slice_prefix(detail, 2000))
                    .unwrap_or(""),
                pending
                    .questions
                    .as_ref()
                    .map(|questions| format!(
                        "\n{}",
                        serde_json::to_string(questions).unwrap_or_default()
                    ))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let body = [
        current
            .last_pause_reason
            .as_ref()
            .map(|reason| format!("Previous interruption: {reason}\nInterrupted workers were continued from their retained checkouts. Inspect their results normally before review. Policy-blocked tasks were not restarted; correct or explicitly re-scope those tasks before retrying them. Dependent validation remains queued until its prerequisites pass review."))
            .unwrap_or_default(),
        if results.is_empty() {
            String::new()
        } else {
            format!("Worker results are ready. Review the work, request corrections through the CLI when needed, and finish the original task.\n\n{summary}")
        },
        if waiting.is_empty() {
            String::new()
        } else {
            format!("These agents are blocked waiting on you. Decide each one with respond or answer; they stay stopped until you do. Judge it against the task you assigned, and ask the user in this conversation only when the call is genuinely theirs to make.\n\n{asks}")
        },
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n");
    let weak = this.clone();
    let lead = lead_id.to_string();
    cx.update(|cx| {
        host.submit(
            &lead.clone(),
            &body,
            Box::new(move |outcome, cx| {
                if outcome.status != ControlStatus::Completed {
                    let error = outcome.error.unwrap_or_else(|| {
                        "Lead continuation was interrupted. Its agents were stopped; review and resume."
                            .into()
                    });
                    spawn_pause(
                        &weak,
                        &lead,
                        error,
                        Some(Box::new(move |run: &mut OrchestrationRun| {
                            for task in &mut run.tasks {
                                if result_ids.contains(&task.id) {
                                    task.delivered = false;
                                }
                            }
                        })),
                        cx,
                    );
                } else if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |this, cx| this.sync(cx));
                }
            }),
            cx,
        )
    });
    Ok(())
}

/// The body of `observe`'s check: resolve each reported path and stop the
/// worker on the first write outside its scope.
async fn check_writes(
    this: &WeakEntity<Orchestrator>,
    lead_id: &str,
    task: &OrchestrationTask,
    dispatch_id: &str,
    paths: &[String],
    checkout: &str,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let still_running = |this: &WeakEntity<Orchestrator>, cx: &mut AsyncApp| {
        current_run(this, lead_id, cx).map(|run| {
            run.is_some_and(|run| {
                run.status == RunStatus::Active
                    && run.tasks.iter().any(|entry| {
                        entry.id == task.id
                            && entry.status == TaskStatus::Running
                            && entry.active_dispatch_id.as_deref() == Some(dispatch_id)
                    })
            })
        })
    };
    let store = store(this, cx)?;
    for path in paths {
        let absolute = if is_absolute_report(path) {
            path.clone()
        } else {
            format!("{checkout}/{path}")
        };
        let resolved = match cx.update(|cx| store.resolve_path(&absolute, cx)).await {
            Ok(resolved) => resolved,
            Err(error) => {
                if still_running(this, cx)? {
                    block_task(
                        this,
                        lead_id,
                        &task.id,
                        &format!(
                            "Could not verify {}'s reported write to {path}: {error}. Only this worker was stopped and its checkout was retained. Inspect the path, then retry it with an explicit project-relative scope or cancel it.",
                            task.title
                        ),
                        cx,
                    )
                    .await?;
                }
                return Ok(());
            }
        };
        if !still_running(this, cx)? {
            return Ok(());
        }
        let normalized = orchestration_path_key(&resolved);
        let scopes = task.write_scopes.as_ref().unwrap_or(&task.scopes);
        let inside = scopes
            .iter()
            .chain(task.scratch_dir.iter())
            .any(|scope| scope_contains(&orchestration_path_key(scope), &normalized));
        if inside {
            continue;
        }
        block_task(
            this,
            lead_id,
            &task.id,
            &format!(
                "{} reported a write outside its assignment: {path}. Only this worker was stopped and its checkout was retained; other independent work continues. Inspect the reported path. Retry with corrected project-relative files only if the write is genuinely required, otherwise send a correction within the existing scope or cancel the task.",
                task.title
            ),
            cx,
        )
        .await?;
        return Ok(());
    }
    Ok(())
}

/// `/^(?:[\\/]|[a-z]:[\\/])/i`.
fn is_absolute_report(path: &str) -> bool {
    let bytes = path.as_bytes();
    matches!(bytes.first(), Some(b'/') | Some(b'\\'))
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}
