//! Test doubles for the orchestrator: an in-memory control store and a host
//! whose sessions are a plain list, as orchestration.test.ts mocked them.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{App, AppContext as _, Entity, Task, TestAppContext};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{HarnessId, Session};

use super::host::{
    ChoiceModel, Done, HarnessChoice, OrchestrationHost, OrchestrationStorage, WorkerIntegration,
    WorkerPreparation,
};
use super::orchestrator::Orchestrator;
use super::state::{OrchestrationRun, OrchestrationTask, OrchestrationWorkspace, WorkspaceKind};
use crate::submit::ControlOutcome;

/// A one-shot behavior for the next store call.
pub enum Once<T> {
    Fail(String),
    Value(T),
    /// Resolve when the sender fires.
    Hold(oneshot::Receiver<Result<T, String>>),
}

type ScopesFn = Box<dyn Fn(&str, &[String]) -> Vec<String>>;
type ResolveFn = Box<dyn Fn(&str) -> String>;

/// The mocked `Storage`.
pub struct FakeStore {
    pub saved: RefCell<HashMap<String, OrchestrationRun>>,
    pub saves: RefCell<Vec<OrchestrationRun>>,
    pub save_failures: RefCell<VecDeque<String>>,
    pub enabled: RefCell<Vec<(String, String)>>,
    pub disabled: RefCell<Vec<String>>,
    pub scope_calls: Cell<usize>,
    pub scopes_once: RefCell<VecDeque<Once<Vec<String>>>>,
    pub scopes_impl: RefCell<ScopesFn>,
    pub resolve_calls: Cell<usize>,
    pub resolve_once: RefCell<VecDeque<Once<String>>>,
    pub resolve_impl: RefCell<ResolveFn>,
}

impl FakeStore {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            saved: RefCell::default(),
            saves: RefCell::default(),
            save_failures: RefCell::default(),
            enabled: RefCell::default(),
            disabled: RefCell::default(),
            scope_calls: Cell::new(0),
            scopes_once: RefCell::default(),
            scopes_impl: RefCell::new(Box::new(default_scopes)),
            resolve_calls: Cell::new(0),
            resolve_once: RefCell::default(),
            resolve_impl: RefCell::new(Box::new(|path| path.to_string())),
        })
    }
}

/// `files.map((file) => (file === "." ? cwd : `${cwd}/${file}`))`.
pub fn default_scopes(cwd: &str, files: &[String]) -> Vec<String> {
    files
        .iter()
        .map(|file| {
            if file == "." {
                cwd.to_string()
            } else {
                format!("{cwd}/{file}")
            }
        })
        .collect()
}

fn once_task<T: 'static>(once: Once<T>, cx: &App) -> Task<Result<T, String>> {
    match once {
        Once::Fail(error) => Task::ready(Err(error)),
        Once::Value(value) => Task::ready(Ok(value)),
        Once::Hold(receiver) => cx.foreground_executor().spawn(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err("released".to_string()))
        }),
    }
}

impl OrchestrationStorage for FakeStore {
    fn save(&self, run: &OrchestrationRun, _cx: &App) -> Task<Result<(), String>> {
        self.saves.borrow_mut().push(run.clone());
        if let Some(error) = self.save_failures.borrow_mut().pop_front() {
            return Task::ready(Err(error));
        }
        self.saved
            .borrow_mut()
            .insert(run.lead_id.clone(), run.clone());
        Task::ready(Ok(()))
    }

    fn load(&self, id: &str, _cx: &App) -> Task<Result<Option<OrchestrationRun>, String>> {
        Task::ready(Ok(self.saved.borrow().get(id).cloned()))
    }

    fn enable(&self, id: &str, cwd: &str, _cx: &App) -> Task<Result<String, String>> {
        self.enabled
            .borrow_mut()
            .push((id.to_string(), cwd.to_string()));
        Task::ready(Ok(
            "/Applications/MonoCode.app/Contents/MacOS/monocode".to_string()
        ))
    }

    fn disable(&self, id: &str, _cx: &App) -> Task<Result<(), String>> {
        self.disabled.borrow_mut().push(id.to_string());
        Task::ready(Ok(()))
    }

    fn scopes(&self, cwd: &str, files: &[String], cx: &App) -> Task<Result<Vec<String>, String>> {
        self.scope_calls.set(self.scope_calls.get() + 1);
        if let Some(once) = self.scopes_once.borrow_mut().pop_front() {
            return once_task(once, cx);
        }
        Task::ready(Ok((self.scopes_impl.borrow())(cwd, files)))
    }

    fn resolve_path(&self, path: &str, cx: &App) -> Task<Result<String, String>> {
        self.resolve_calls.set(self.resolve_calls.get() + 1);
        if let Some(once) = self.resolve_once.borrow_mut().pop_front() {
            return once_task(once, cx);
        }
        Task::ready(Ok((self.resolve_impl.borrow())(path)))
    }
}

/// The mocked `OrchestrationHost`.
pub struct FakeHost {
    pub sessions: Rc<RefCell<Vec<Session>>>,
    pub choices: RefCell<Vec<HarnessChoice>>,
    pub completions: RefCell<HashMap<String, Done>>,
    pub submits: RefCell<Vec<(String, String)>>,
    pub stops: RefCell<Vec<String>>,
    pub stop_holds: RefCell<VecDeque<oneshot::Receiver<()>>>,
    pub steers: RefCell<Vec<(String, String)>>,
    pub approvals: RefCell<Vec<(String, i64, ApprovalDecision)>>,
    pub answers: RefCell<Vec<(String, i64, UserQuestionReply)>>,
    pub created: RefCell<Vec<OrchestrationTask>>,
    pub integrated: RefCell<Vec<String>>,
    pub integrate_failures: RefCell<VecDeque<String>>,
    pub cleanups: RefCell<Vec<(String, bool)>>,
    pub cleanup_result: Cell<bool>,
    /// Workers share this checkout instead of `/worktrees/<task id>`.
    pub worker_checkout: RefCell<Option<String>>,
}

impl FakeHost {
    pub fn new(sessions: Vec<Session>) -> Rc<Self> {
        Rc::new(Self {
            sessions: Rc::new(RefCell::new(sessions)),
            choices: RefCell::new(vec![HarnessChoice {
                harness: HarnessId::Codex,
                models: vec![ChoiceModel {
                    id: "codex:test".into(),
                    name: "Test".into(),
                }],
            }]),
            completions: RefCell::default(),
            submits: RefCell::default(),
            stops: RefCell::default(),
            stop_holds: RefCell::default(),
            steers: RefCell::default(),
            approvals: RefCell::default(),
            answers: RefCell::default(),
            created: RefCell::default(),
            integrated: RefCell::default(),
            integrate_failures: RefCell::default(),
            cleanups: RefCell::default(),
            cleanup_result: Cell::new(true),
            worker_checkout: RefCell::new(None),
        })
    }

    pub fn with_session<R>(&self, id: &str, change: impl FnOnce(&mut Session) -> R) -> R {
        let mut sessions = self.sessions.borrow_mut();
        let session = sessions
            .iter_mut()
            .find(|session| session.id == id)
            .unwrap_or_else(|| panic!("no session {id}"));
        change(session)
    }

    pub fn session_busy(&self, id: &str) -> bool {
        self.with_session(id, |session| session.is_busy())
    }

    pub fn set_busy(&self, id: &str, busy: bool) {
        self.with_session(id, |session| session.busy = Some(busy));
    }

    /// The pending completion for this session (`completions.get(id)`).
    pub fn take_completion(&self, id: &str) -> Option<Done> {
        self.completions.borrow_mut().remove(id)
    }

    pub fn submit_count(&self) -> usize {
        self.submits.borrow().len()
    }
}

/// A blank session, `newSession(harness, cwd)` with a fixed id.
pub fn session(id: &str, harness: HarnessId, cwd: &str) -> Session {
    Session::blank(id, harness, format!("{harness}:test"), cwd)
}

impl OrchestrationHost for FakeHost {
    fn session(&self, id: &str, _cx: &App) -> Option<Session> {
        self.sessions
            .borrow()
            .iter()
            .find(|session| session.id == id)
            .cloned()
    }

    fn sessions(&self, _cx: &App) -> Vec<Session> {
        self.sessions.borrow().clone()
    }

    fn choices(&self, _cx: &App) -> Vec<HarnessChoice> {
        self.choices.borrow().clone()
    }

    fn create_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        _cx: &mut App,
    ) -> Task<Result<WorkerPreparation, String>> {
        self.created.borrow_mut().push(task.clone());
        let mut worker = session(&task.session_id, task.harness, &run.cwd);
        worker.busy = Some(false);
        self.sessions.borrow_mut().push(worker);
        let checkout = self
            .worker_checkout
            .borrow()
            .clone()
            .unwrap_or_else(|| format!("/worktrees/{}", task.id));
        Task::ready(Ok(WorkerPreparation {
            scratch_dir: Some(format!(
                "/private/var/folders/test/T/monocode-worker-{}",
                task.session_id
            )),
            workspace: OrchestrationWorkspace {
                id: format!("checkout:{checkout}"),
                project_cwd: run.cwd.clone(),
                checkout_cwd: checkout.clone(),
                kind: WorkspaceKind::Worktree,
                branch: Some(format!("mc/orch-{}", task.id)),
                extra: Default::default(),
            },
        }))
    }

    fn integrate_worker(
        &self,
        _run: &OrchestrationRun,
        task: &OrchestrationTask,
        _cx: &mut App,
    ) -> Task<Result<WorkerIntegration, String>> {
        self.integrated.borrow_mut().push(task.id.clone());
        if let Some(error) = self.integrate_failures.borrow_mut().pop_front() {
            return Task::ready(Err(error));
        }
        Task::ready(Ok(WorkerIntegration::default()))
    }

    fn cleanup_worker(
        &self,
        _run: &OrchestrationRun,
        task: &OrchestrationTask,
        only_if_unchanged: bool,
        _cx: &mut App,
    ) -> Task<Result<bool, String>> {
        self.cleanups
            .borrow_mut()
            .push((task.id.clone(), only_if_unchanged));
        Task::ready(Ok(self.cleanup_result.get()))
    }

    fn submit(&self, id: &str, text: &str, done: Done, _cx: &mut App) {
        self.submits
            .borrow_mut()
            .push((id.to_string(), text.to_string()));
        self.set_busy(id, true);
        let sessions = self.sessions.clone();
        let session_id = id.to_string();
        self.completions.borrow_mut().insert(
            id.to_string(),
            Box::new(move |outcome, cx| {
                if let Some(session) = sessions
                    .borrow_mut()
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.busy = Some(false);
                }
                done(outcome, cx);
            }),
        );
    }

    fn stop(&self, id: &str, cx: &mut App) -> Task<Result<(), String>> {
        self.stops.borrow_mut().push(id.to_string());
        if let Some(hold) = self.stop_holds.borrow_mut().pop_front() {
            return cx.foreground_executor().spawn(async move {
                let _ = hold.await;
                Ok(())
            });
        }
        if let Some(session) = self
            .sessions
            .borrow_mut()
            .iter_mut()
            .find(|session| session.id == id)
        {
            session.busy = Some(false);
        }
        Task::ready(Ok(()))
    }

    fn steer(&self, id: &str, text: &str, _cx: &mut App) -> Task<Result<(), String>> {
        self.steers
            .borrow_mut()
            .push((id.to_string(), text.to_string()));
        Task::ready(Ok(()))
    }

    fn respond_approval(
        &self,
        id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        _cx: &mut App,
    ) {
        self.approvals
            .borrow_mut()
            .push((id.to_string(), request_id, decision));
    }

    fn answer_question(&self, id: &str, request_id: i64, reply: UserQuestionReply, _cx: &mut App) {
        self.answers
            .borrow_mut()
            .push((id.to_string(), request_id, reply));
    }
}

/// An orchestrator over the fakes, bound to `host`.
pub fn orchestrator(
    store: Rc<FakeStore>,
    host: Rc<FakeHost>,
    cx: &mut TestAppContext,
) -> Entity<Orchestrator> {
    cx.update(|cx| {
        cx.new(|_| {
            let mut orchestrator = Orchestrator::new(store);
            orchestrator.bind(host);
            orchestrator
        })
    })
}

/// Run an async flow to completion. Panics when it is still waiting.
pub fn finish<T: 'static>(cx: &mut TestAppContext, task: Task<T>) -> T {
    cx.run_until_parked();
    futures::FutureExt::now_or_never(task).expect("the flow is still waiting")
}

/// Fire a completion the way a finished turn would.
pub fn complete(cx: &mut TestAppContext, done: Done, outcome: ControlOutcome) {
    cx.update(|cx| done(outcome, cx));
    cx.run_until_parked();
}
