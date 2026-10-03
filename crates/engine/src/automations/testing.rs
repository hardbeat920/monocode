//! Test doubles: a store backend that can fail chosen commands, fake
//! windows and reminder commands, and builders for automations and Inbox
//! items.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::{App, Task};
use monocode_core::block::{Block, BlockRole, TurnIntent};
use monocode_core::inbox::{InboxKind, InboxProvider};
use monocode_core::{Attachment, Extra, HarnessId, Session};
use monocode_layout::{WorkspaceTab, leaf_ids, split_pane};
use monocode_store::session_store::SessionStore;
use parking_lot::Mutex;
use serde_json::Value;

use super::backend::{
    AutomationsBackend, ReminderPoll, ReminderPreferences, ReminderTarget, RemindersBackend,
    SessionReminder, StoreAutomationsBackend,
};
use super::events::{AutomationEventClaim, InboxEventItem};
use super::host::{LaunchHost, ReminderApp, ReminderHost, SessionPlacement};
use super::local_time::local_ms;
use super::model::{
    Automation, AutomationDraft, AutomationRun, AutomationRunStatus, AutomationTrigger,
    AutomationUpsert, DueAutomationRun, new_automation_draft,
};
use crate::runtime::{Engine, StoreFuture};
use crate::submit::{OnSettled, SubmissionAcceptance, SubmitOptions};

/// `new Date("2026-09-19T10:20:00").getTime()`: a local time.
pub fn at(year: i32, month: i64, day: i64, hours: i64, minutes: i64) -> i64 {
    local_ms(year, month - 1, day, hours, minutes)
}

/// An automation built from a draft, like the TypeScript tests' spread.
pub fn automation_from(draft: AutomationDraft, id: &str) -> Automation {
    Automation {
        id: id.to_string(),
        name: draft.name,
        prompt: draft.prompt,
        harness: draft.harness,
        model: draft.model,
        model_settings: Some(draft.model_settings),
        cwd: draft.cwd,
        workspace_mode: draft.workspace_mode,
        worktree_cwd: Some(draft.worktree_cwd),
        session_folder_id: Some(draft.session_folder_id),
        reuse_session: draft.reuse_session,
        runtime_mode: draft.runtime_mode,
        trigger_kind: draft.trigger_kind,
        trigger_event: draft.trigger_event,
        schedule_kind: draft.schedule_kind,
        minute: draft.minute,
        time: draft.time,
        day_of_week: draft.day_of_week,
        triggers: Some(draft.triggers),
        missed_run_grace_minutes: draft.missed_run_grace_minutes,
        enabled: draft.enabled,
        next_run_at: at(2026, 9, 21, 9, 0),
        last_run_at: None,
        last_run_status: None,
        last_run_error: None,
        last_session_id: None,
        created_at: at(2026, 9, 19, 9, 0),
        updated_at: at(2026, 9, 19, 10, 0),
        extra: Extra::new(),
    }
}

/// The `automation(overrides)` helper of automationEvents.test.ts.
pub fn review_automation(triggers: Vec<AutomationTrigger>) -> Automation {
    let mut draft = new_automation_draft("/tmp/web", HarnessId::Codex, "model");
    draft.name = "Review pull requests".into();
    draft.prompt = "Review the newly opened pull request.".into();
    draft.triggers = triggers;
    automation_from(draft, "automation-id")
}

/// The `item(overrides)` helper of automationEvents.test.ts.
pub fn inbox_item() -> InboxEventItem {
    InboxEventItem {
        kind: InboxKind::Pr,
        number: 12,
        title: "Fix checkout".into(),
        url: "https://github.com/acme/web/pull/12".into(),
        state: "open".into(),
        created_at: Some("2026-09-19T15:00:00Z".into()),
        updated_at: "2026-09-19T15:00:00Z".into(),
        draft: false,
        repo: "acme/web".into(),
        project_path: "/tmp/web".into(),
        provider: InboxProvider::Github,
        id: None,
        identifier: None,
        extra: Extra::new(),
    }
}

/// An in-memory `monocode.db`.
pub fn memory_store() -> Arc<SessionStore> {
    Arc::new(SessionStore::open_in_memory().expect("in-memory store"))
}

/// Seed a saved session row, as the store tests do.
pub fn seed_session(store: &SessionStore, id: &str, cwd: &str, title: &str) {
    store
        .lock_conn()
        .expect("store lock")
        .execute(
            "INSERT INTO sessions (id, cwd, harness, model, runtime_mode, title,
             created_at, updated_at, has_user_message) VALUES (?1, ?2, 'codex', '',
             'supervised', ?3, 1, 1, 1)",
            [id, cwd, title],
        )
        .expect("seed session");
}

/// The store backend, run inline, with failures per command.
pub struct FlakyAutomations {
    pub inner: StoreAutomationsBackend,
    failures: Mutex<HashMap<&'static str, VecDeque<String>>>,
    calls: Mutex<Vec<&'static str>>,
}

impl FlakyAutomations {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: StoreAutomationsBackend::inline(memory_store()),
            failures: Mutex::default(),
            calls: Mutex::default(),
        })
    }

    /// Fail the next call to `command` with `message`.
    pub fn fail_next(&self, command: &'static str, message: &str) {
        self.failures
            .lock()
            .entry(command)
            .or_default()
            .push_back(message.to_string());
    }

    pub fn calls(&self, command: &str) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|call| **call == command)
            .count()
    }

    fn call<T: Send + 'static>(
        &self,
        command: &'static str,
        run: impl FnOnce() -> StoreFuture<T>,
    ) -> StoreFuture<T> {
        self.calls.lock().push(command);
        let failure = self
            .failures
            .lock()
            .get_mut(command)
            .and_then(VecDeque::pop_front);
        match failure {
            Some(message) => futures::future::ready(Err(message)).boxed(),
            None => run(),
        }
    }

    /// Save an automation through the store, scheduled after the real now.
    pub fn save(&self, upsert: AutomationUpsert) -> Automation {
        futures::executor::block_on(self.inner.upsert(upsert)).expect("upsert")
    }
}

impl AutomationsBackend for FlakyAutomations {
    fn list(&self) -> StoreFuture<Vec<Automation>> {
        self.call("automations_list", || self.inner.list())
    }

    fn upsert(&self, automation: AutomationUpsert) -> StoreFuture<Automation> {
        self.call("automations_upsert", || self.inner.upsert(automation))
    }

    fn delete(&self, id: String) -> StoreFuture<()> {
        self.call("automations_delete", || self.inner.delete(id))
    }

    fn runs(&self, automation_id: String) -> StoreFuture<Vec<AutomationRun>> {
        self.call("automation_runs_list", || self.inner.runs(automation_id))
    }

    fn recover(&self, started_before: i64, now: i64) -> StoreFuture<Vec<DueAutomationRun>> {
        self.call("automation_runs_recover", || {
            self.inner.recover(started_before, now)
        })
    }

    fn run_now(&self, automation_id: String, now: i64) -> StoreFuture<AutomationRun> {
        self.call("automation_run_now", || {
            self.inner.run_now(automation_id, now)
        })
    }

    fn claim_due(
        &self,
        automation_id: String,
        expected_next_run_at: i64,
        next_run_at: i64,
        now: i64,
    ) -> StoreFuture<Option<DueAutomationRun>> {
        self.call("automations_claim_due", || {
            self.inner
                .claim_due(automation_id, expected_next_run_at, next_run_at, now)
        })
    }

    fn claim_event(
        &self,
        automation_id: String,
        claim: AutomationEventClaim,
        now: i64,
    ) -> StoreFuture<Option<DueAutomationRun>> {
        self.call("automations_claim_event", || {
            self.inner.claim_event(automation_id, claim, now)
        })
    }

    fn run_update(
        &self,
        run_id: String,
        status: AutomationRunStatus,
        session_id: Option<String>,
        error: Option<String>,
        now: i64,
    ) -> StoreFuture<AutomationRun> {
        self.call("automation_run_update", || {
            self.inner
                .run_update(run_id, status, session_id, error, now)
        })
    }
}

// Hosts.

/// One `submit` the fake host received.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitCall {
    pub session_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub intent: Option<TurnIntent>,
    pub refresh_title: bool,
}

type SubmitHandler = Box<dyn Fn(&str, &str, &mut App) -> SubmissionAcceptance>;

/// A workspace window that records every call. `submit` adds the user turn
/// to the session (the TypeScript tests' `commit`) unless a handler is set.
#[derive(Default)]
pub struct FakeHost {
    pub tabs: RefCell<Vec<WorkspaceTab>>,
    pub log: RefCell<Vec<String>>,
    pub submits: RefCell<Vec<SubmitCall>>,
    pub settles: RefCell<Vec<OnSettled>>,
    pub drafts: RefCell<Vec<(String, String, String)>>,
    pub refuse_drafts: Cell<bool>,
    pub handler: RefCell<Option<SubmitHandler>>,
    pub focused: Cell<bool>,
    pub forward: Cell<usize>,
    pub project_cwd: RefCell<String>,
}

impl FakeHost {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn note(&self, entry: String) {
        self.log.borrow_mut().push(entry);
    }

    /// Answer `submit` with `handler` instead of committing the turn.
    pub fn on_submit(
        &self,
        handler: impl Fn(&str, &str, &mut App) -> SubmissionAcceptance + 'static,
    ) {
        *self.handler.borrow_mut() = Some(Box::new(handler));
    }

    /// Settle the `index`th managed submission.
    pub fn settle(&self, index: usize, outcome: crate::submit::ControlOutcome, cx: &mut App) {
        let settle = self.settles.borrow()[index].clone();
        settle(outcome, cx);
    }
}

/// `commit`: add the user turn to the open session.
pub fn commit_turn(session_id: &str, text: &str, cx: &mut App) -> bool {
    let sessions = Engine::sessions(cx);
    let text = text.to_string();
    sessions.update(cx, |sessions, cx| {
        sessions.update(session_id, cx, |session| {
            session
                .blocks
                .push(Block::new("user-turn", BlockRole::User, text));
        })
    })
}

impl LaunchHost for FakeHost {
    fn append_tab(&self, tab: WorkspaceTab, cwd: &str, _cx: &mut App) {
        self.note(format!("append:{}:{cwd}", tab.id));
        self.tabs.borrow_mut().push(tab);
    }

    fn activate_tab(&self, tab_id: &str, _cx: &mut App) {
        self.note(format!("activate:{tab_id}"));
    }

    fn focus_open_session(&self, session_id: &str, _cx: &mut App) {
        self.note(format!("focus:{session_id}"));
    }

    fn show_sessions(&self, cwd: &str, _cx: &mut App) {
        self.note(format!("show:{cwd}"));
    }

    fn place_session(
        &self,
        session_id: &str,
        placement: &SessionPlacement,
        cwd: &str,
        reveal: bool,
        _cx: &mut App,
    ) -> Result<String, String> {
        let mut tabs = self.tabs.borrow_mut();
        let Some(tab) = tabs
            .iter_mut()
            .find(|tab| leaf_ids(&tab.layout).contains(&placement.beside_session_id))
        else {
            return Err("Target pane unavailable".into());
        };
        tab.layout = split_pane(
            &tab.layout,
            &placement.beside_session_id,
            placement.direction,
            session_id,
        );
        if reveal {
            tab.focused_id = session_id.to_string();
        }
        let id = tab.id.clone();
        drop(tabs);
        self.note(format!("place:{session_id}:{cwd}"));
        Ok(id)
    }

    fn set_project_cwd(&self, cwd: &str, _cx: &mut App) {
        *self.project_cwd.borrow_mut() = cwd.to_string();
        self.note(format!("project:{cwd}"));
    }

    fn remember_project(&self, cwd: &str, _cx: &mut App) {
        self.note(format!("remember:{cwd}"));
    }

    fn reveal_tab(&self, tab_id: &str, cwd: &str, _cx: &mut App) {
        self.note(format!("reveal:{tab_id}:{cwd}"));
    }

    fn submit(
        &self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        options: SubmitOptions,
        cx: &mut App,
    ) -> SubmissionAcceptance {
        self.submits.borrow_mut().push(SubmitCall {
            session_id: session_id.to_string(),
            text: text.to_string(),
            attachments,
            intent: options.intent,
            refresh_title: options.refresh_title,
        });
        if let Some(settle) = options.on_settled {
            self.settles.borrow_mut().push(settle);
        }
        if let Some(handler) = self.handler.borrow().as_ref() {
            return handler(session_id, text, cx);
        }
        SubmissionAcceptance::Ready(commit_turn(session_id, text, cx))
    }

    fn save_draft(
        &self,
        session_id: &str,
        text: &str,
        _attachments: Vec<Attachment>,
        request_id: &str,
        cx: &mut App,
    ) -> bool {
        self.drafts.borrow_mut().push((
            session_id.to_string(),
            text.to_string(),
            request_id.to_string(),
        ));
        if self.refuse_drafts.get() {
            return false;
        }
        let sessions = Engine::sessions(cx);
        let (text, request_id) = (text.to_string(), request_id.to_string());
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                let mut block = Block::new("draft-turn", BlockRole::User, text);
                block.draft = Some(true);
                block.app_request_id = Some(request_id);
                session.blocks.push(block);
            })
        })
    }

    fn prepare_attachments(&self, files: Vec<Attachment>, _cx: &App) -> Task<Vec<Attachment>> {
        Task::ready(files)
    }

    fn is_focused(&self, _cx: &App) -> bool {
        self.focused.get()
    }

    fn bring_forward(&self, _cx: &mut App) {
        self.forward.set(self.forward.get() + 1);
    }
}

/// The runtime sees the fake window's tabs, so it keeps their sessions
/// open instead of detaching them as idle.
impl crate::runtime::WorkspaceHooks for FakeHost {
    fn tab_session_ids(&self, _cx: &App) -> Vec<String> {
        self.tabs
            .borrow()
            .iter()
            .flat_map(|tab| leaf_ids(&tab.layout))
            .collect()
    }
}

/// Install an `Engine` whose workspace hooks read these windows' tabs.
pub fn init_engine_with_hosts(cx: &mut gpui::TestAppContext, hosts: &[Rc<FakeHost>]) {
    let hosts: Vec<Rc<FakeHost>> = hosts.to_vec();
    let workspace = Rc::new(Windows(hosts));
    crate::runtime::testing::init_test_engine_with(
        cx,
        crate::runtime::EngineHooks {
            workspace,
            ..crate::runtime::EngineHooks::default()
        },
    );
}

struct Windows(Vec<Rc<FakeHost>>);

impl crate::runtime::WorkspaceHooks for Windows {
    fn tab_session_ids(&self, cx: &App) -> Vec<String> {
        self.0
            .iter()
            .flat_map(|host| crate::runtime::WorkspaceHooks::tab_session_ids(host.as_ref(), cx))
            .collect()
    }
}

/// A saved session in `Sessions`.
pub fn open_session(id: &str, cwd: &str, cx: &mut App) -> Session {
    let mut session = Session::blank(id, HarnessId::Codex, "codex:model", cwd);
    session.title = "Continue this work".into();
    let added = session.clone();
    let sessions = Engine::sessions(cx);
    sessions.update(cx, |sessions, cx| sessions.upsert(added, cx));
    session
}

// Reminders.

/// The `invoke` mock of useSessionReminders.test.ts.
#[derive(Default)]
pub struct FakeReminders {
    pub stored: Mutex<Vec<SessionReminder>>,
    pub pending: Mutex<Option<(ReminderTarget, Option<String>)>>,
    pub owners: Mutex<HashMap<String, Vec<String>>>,
    pub configurations: Mutex<Vec<ReminderPreferences>>,
    pub calls: Mutex<Vec<(String, Value)>>,
    configure_holds: Mutex<VecDeque<oneshot::Receiver<()>>>,
}

impl FakeReminders {
    pub fn new(stored: Vec<SessionReminder>) -> Arc<Self> {
        let fake = Self::default();
        *fake.stored.lock() = stored;
        Arc::new(fake)
    }

    pub fn hold_configure(&self) -> oneshot::Sender<()> {
        let (sender, receiver) = oneshot::channel();
        self.configure_holds.lock().push_back(receiver);
        sender
    }

    pub fn calls(&self, command: &str) -> Vec<Value> {
        self.calls
            .lock()
            .iter()
            .filter(|(name, _)| name == command)
            .map(|(_, args)| args.clone())
            .collect()
    }

    pub fn last_configuration(&self) -> Option<ReminderPreferences> {
        self.configurations.lock().last().cloned()
    }

    fn record(&self, command: &str, args: Value) {
        self.calls.lock().push((command.to_string(), args));
    }
}

impl RemindersBackend for FakeReminders {
    fn list(&self) -> StoreFuture<Vec<SessionReminder>> {
        self.record("reminder_list", Value::Null);
        futures::future::ready(Ok(self.stored.lock().clone())).boxed()
    }

    fn set(&self, session_ids: Vec<String>, due_at: i64) -> StoreFuture<()> {
        self.record(
            "reminder_set",
            serde_json::json!({ "sessionIds": session_ids, "dueAt": due_at }),
        );
        let template = self.stored.lock().first().cloned();
        if let Some(template) = template {
            *self.stored.lock() = session_ids
                .into_iter()
                .map(|session_id| SessionReminder {
                    session_id,
                    due_at,
                    fired_at: None,
                    ..template.clone()
                })
                .collect();
        }
        futures::future::ready(Ok(())).boxed()
    }

    fn clear(&self, session_ids: Vec<String>, expected_due_at: Option<i64>) -> StoreFuture<()> {
        self.record(
            "reminder_clear",
            serde_json::json!({ "sessionIds": session_ids, "expectedDueAt": expected_due_at }),
        );
        self.stored.lock().retain(|item| {
            !session_ids.contains(&item.session_id)
                || expected_due_at.is_some_and(|due| item.due_at != due)
        });
        futures::future::ready(Ok(())).boxed()
    }

    fn configure(&self, preferences: ReminderPreferences) -> StoreFuture<()> {
        self.record(
            "reminder_configure",
            serde_json::to_value(&preferences).unwrap_or(Value::Null),
        );
        self.configurations.lock().push(preferences);
        match self.configure_holds.lock().pop_front() {
            Some(hold) => async move {
                let _ = hold.await;
                Ok(())
            }
            .boxed(),
            None => futures::future::ready(Ok(())).boxed(),
        }
    }

    fn poll_due(&self) -> StoreFuture<ReminderPoll> {
        futures::future::ready(Ok(ReminderPoll::default())).boxed()
    }

    fn register_window(&self, owner: &str, session_ids: Vec<String>) -> Result<(), String> {
        self.owners.lock().insert(owner.to_string(), session_ids);
        Ok(())
    }

    fn window_sessions(&self) -> HashMap<String, Vec<String>> {
        self.owners.lock().clone()
    }

    fn queue_open(&self, target: ReminderTarget, owner: Option<String>) -> Result<(), String> {
        self.record(
            "reminder_open",
            serde_json::json!({ "sessionId": target.session_id, "dueAt": target.due_at }),
        );
        *self.pending.lock() = Some((target, owner));
        Ok(())
    }

    fn take_open(
        &self,
        owner: &str,
        owner_exists: &dyn Fn(&str) -> bool,
    ) -> Result<Option<ReminderTarget>, String> {
        let mut pending = self.pending.lock();
        let take = pending.as_ref().is_some_and(|(_, chosen)| {
            chosen
                .as_deref()
                .is_none_or(|label| label == owner || !owner_exists(label))
        });
        Ok(if take {
            pending.take().map(|(target, _)| target)
        } else {
            None
        })
    }
}

/// The app side of reminders: `ensureSaved` and `message`.
#[derive(Default)]
pub struct FakeReminderApp {
    pub save_failures: RefCell<VecDeque<String>>,
    pub saved: RefCell<Vec<Vec<String>>>,
    pub errors: RefCell<Vec<String>>,
    pub new_windows: Cell<usize>,
}

impl ReminderApp for FakeReminderApp {
    fn ensure_saved(&self, session_ids: &[String], _cx: &mut App) -> Task<Result<(), String>> {
        self.saved.borrow_mut().push(session_ids.to_vec());
        match self.save_failures.borrow_mut().pop_front() {
            Some(error) => Task::ready(Err(error)),
            None => Task::ready(Ok(())),
        }
    }

    fn show_error(&self, message: &str, _cx: &mut App) {
        self.errors.borrow_mut().push(message.to_string());
    }

    fn open_new_window(&self, _cx: &mut App) {
        self.new_windows.set(self.new_windows.get() + 1);
    }
}

/// One window for reminders: `onOpenSession`.
#[derive(Default)]
pub struct FakeReminderWindow {
    pub opened: RefCell<Vec<String>>,
    pub fail: RefCell<Option<String>>,
    pub focused: Cell<bool>,
    pub forward: Cell<usize>,
}

impl ReminderHost for FakeReminderWindow {
    fn show_session(&self, session: &Session, _cx: &mut App) -> Task<Result<(), String>> {
        if let Some(error) = self.fail.borrow().clone() {
            return Task::ready(Err(error));
        }
        self.opened.borrow_mut().push(session.id.clone());
        Task::ready(Ok(()))
    }

    fn is_focused(&self, _cx: &App) -> bool {
        self.focused.get()
    }

    fn bring_forward(&self, _cx: &mut App) {
        self.forward.set(self.forward.get() + 1);
    }
}
