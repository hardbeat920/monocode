//! The `Automations` entity: the automations list and run history the view
//! shows, create, edit, enable, delete, and run now, and the launch and
//! scheduler code from src/app/App.tsx (`launchAutomation`,
//! `ensureAutomationRecovery`, the 30 s evaluation effect, and
//! `onInboxAppeared`).
//!
//! The TypeScript evaluated due automations on a 30 s `setInterval` and when
//! the window became visible. Here a task checks every 5 s and evaluates
//! every 30 s of wall-clock time, and at once when the wall clock jumped
//! ahead of the timer, which is what a machine waking from sleep looks
//! like. Overdue occurrences then go through the store's claim with their
//! grace period, as they did after a long poll gap.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::Shared;
use gpui::{Context, EventEmitter, Task, WeakEntity};
use monocode_core::Session;
use monocode_core::paths::path_key;
use monocode_core::session::{
    LinkedWorkItem, WorkspaceMode, format_session_title, session_work_cwd,
};
use monocode_layout::new_tab;
use monocode_settings::Kv;

use super::backend::AutomationsBackend;
use super::events::{
    InboxEventItem, InboxRetries, claim_inbox_automation_runs,
    linked_work_item_from_automation_event,
};
use super::host::{LaunchHost, NoWorkspace};
use super::model::{
    Automation, AutomationDraft, AutomationRun, AutomationRunStatus, AutomationRunTrigger,
    AutomationWorkspaceMode, automation_upsert, claim_due_automations, draft_from_automation,
};
use crate::attention::Clock;
use crate::history::session_folders::{
    SessionFolderTarget, load_session_folders, place_session_in_folder, save_session_folders,
};
use crate::runtime::Engine;
use crate::submit::SubmitOptions;
use crate::submit::acceptance::{ControlOutcome, ControlStatus, OnSettled, submit_with_settlement};
use crate::submit::paths::looks_like_project;

/// How often the scheduler wakes to look at the wall clock.
pub const SCHEDULER_TICK: Duration = Duration::from_secs(5);
/// How often it claims due automations (the TypeScript interval).
pub const EVALUATE_INTERVAL_MS: i64 = 30_000;
/// A wall-clock step this far past one tick means the machine slept.
const SLEEP_SLACK_MS: i64 = 10_000;

/// The message a rejected submission records on its run.
pub const REJECTION_MESSAGE: &str = "The selected agent session could not start this run.";

/// What changed, for views that need more than `cx.notify()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationsEvent {
    /// Automations or runs changed (`monocode:automations-changed`).
    Changed,
}

type Recovery = Shared<Task<Result<(), String>>>;

/// Automations and their runs, plus the scheduler.
pub struct Automations {
    backend: Arc<dyn AutomationsBackend>,
    kv: Kv,
    clock: Clock,
    host: Rc<dyn LaunchHost>,

    list: Vec<Automation>,
    loaded: bool,
    error: Option<String>,
    /// `rememberedAutomationId`: the selection outlives the view.
    selected_id: Option<String>,
    runs: Vec<AutomationRun>,
    saving: bool,
    running: Option<String>,
    refresh_revision: u64,
    runs_task: Option<Task<()>>,

    /// `automationSessionReservations`: sessions a launch is using.
    reservations: HashSet<String>,
    recovery: Option<Recovery>,
    /// `automationRecoveryCutoffRef`: runs created before this entity
    /// existed belong to an earlier app run.
    recovery_cutoff: i64,
    evaluating: bool,
    inbox_retries: Rc<RefCell<InboxRetries>>,
    scheduler: Option<Task<()>>,
}

impl EventEmitter<AutomationsEvent> for Automations {}

impl Automations {
    pub fn new(
        backend: Arc<dyn AutomationsBackend>,
        kv: Kv,
        clock: Clock,
        _cx: &mut Context<Self>,
    ) -> Self {
        let recovery_cutoff = clock();
        Self {
            backend,
            kv,
            clock,
            host: Rc::new(NoWorkspace),
            list: Vec::new(),
            loaded: false,
            error: None,
            selected_id: None,
            runs: Vec::new(),
            saving: false,
            running: None,
            refresh_revision: 0,
            runs_task: None,
            reservations: HashSet::new(),
            recovery: None,
            recovery_cutoff,
            evaluating: false,
            inbox_retries: Rc::default(),
            scheduler: None,
        }
    }

    /// The window launches open in (tabs, focus, submit).
    pub fn set_host(&mut self, host: Rc<dyn LaunchHost>) {
        self.host = host;
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    // Reading.

    /// Every automation, newest change first.
    pub fn automations(&self) -> &[Automation] {
        &self.list
    }

    /// The first list load has not finished.
    pub fn is_loading(&self) -> bool {
        !self.loaded
    }

    /// The last load or action error, for the view's banner.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn clear_error(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        cx.notify();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    pub fn selected(&self) -> Option<&Automation> {
        let id = self.selected_id.as_deref()?;
        self.list.iter().find(|automation| automation.id == id)
    }

    /// The selected automation's run history, newest first.
    pub fn runs(&self) -> &[AutomationRun] {
        &self.runs
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    /// The automation a "Run now" is starting.
    pub fn running(&self) -> Option<&str> {
        self.running.as_deref()
    }

    /// Sessions a launch is using.
    pub fn reservations(&self) -> &HashSet<String> {
        &self.reservations
    }

    // Loading.

    /// Load the list and keep the selection when it still exists, else pick
    /// the first automation.
    pub fn refresh(&mut self, cx: &mut Context<Self>) -> Task<()> {
        self.refresh_revision += 1;
        let revision = self.refresh_revision;
        let list = self.backend.list();
        cx.spawn(async move |this, cx| {
            let result = list.await;
            this.update(cx, |this, cx| {
                if revision != this.refresh_revision {
                    return;
                }
                match result {
                    Ok(next) => {
                        let keep = this
                            .selected_id
                            .as_ref()
                            .is_some_and(|id| next.iter().any(|entry| entry.id == *id));
                        if !keep {
                            this.selected_id = next.first().map(|entry| entry.id.clone());
                        }
                        this.list = next;
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                this.loaded = true;
                this.load_runs(cx);
                cx.notify();
            })
            .ok();
        })
    }

    /// Something else changed the automations (another window, the host).
    pub fn notify_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(AutomationsEvent::Changed);
        self.refresh(cx).detach();
    }

    /// Select an automation and load its runs.
    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected_id = id;
        self.load_runs(cx);
        cx.notify();
    }

    /// The run history effect: reload when the list or the selection
    /// changes, dropping a stale answer.
    fn load_runs(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            self.runs.clear();
            self.runs_task = None;
            return;
        };
        let runs = self.backend.runs(id.clone());
        self.runs_task = Some(cx.spawn(async move |this, cx| {
            let result = runs.await;
            this.update(cx, |this, cx| {
                if this.selected_id.as_deref() != Some(id.as_str()) {
                    return;
                }
                this.runs = result.unwrap_or_default();
                cx.notify();
            })
            .ok();
        }));
    }

    /// A local change: tell listeners and reload.
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(AutomationsEvent::Changed);
        self.refresh(cx).detach();
    }

    // Editing.

    /// `onSave`: save the draft, select it, and reload.
    pub fn save(
        &mut self,
        draft: AutomationDraft,
        cx: &mut Context<Self>,
    ) -> Task<Result<Automation, String>> {
        if self.saving {
            return Task::ready(Err("Already saving.".into()));
        }
        self.saving = true;
        cx.notify();
        let upsert = self.backend.upsert(automation_upsert(&draft, self.now()));
        cx.spawn(async move |this, cx| {
            let result = upsert.await;
            this.update(cx, |this, cx| {
                this.saving = false;
                match &result {
                    Ok(saved) => {
                        this.selected_id = Some(saved.id.clone());
                        this.changed(cx);
                    }
                    Err(error) => this.error = Some(error.clone()),
                }
                cx.notify();
            })
            .ok();
            result
        })
    }

    /// `setAutomationEnabled`.
    pub fn set_enabled(
        &mut self,
        automation: &Automation,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Automation, String>> {
        let draft = AutomationDraft {
            enabled,
            ..draft_from_automation(automation)
        };
        let upsert = self.backend.upsert(automation_upsert(&draft, self.now()));
        cx.spawn(async move |this, cx| {
            let result = upsert.await;
            this.update(cx, |this, cx| match &result {
                Ok(_) => this.changed(cx),
                Err(error) => {
                    this.error = Some(error.clone());
                    cx.notify();
                }
            })
            .ok();
            result
        })
    }

    /// `deleteAutomation`, with its run history. The view confirms first.
    pub fn delete(&mut self, id: &str, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let delete = self.backend.delete(id.to_string());
        cx.spawn(async move |this, cx| {
            let result = delete.await;
            this.update(cx, |this, cx| match &result {
                Ok(()) => {
                    this.selected_id = None;
                    this.changed(cx);
                }
                Err(error) => {
                    this.error = Some(error.clone());
                    cx.notify();
                }
            })
            .ok();
            result
        })
    }

    /// `onRun`: record a manual run and launch it in front.
    pub fn run_now(
        &mut self,
        automation_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        if self.running.is_some() {
            return Task::ready(Ok(()));
        }
        let Some(automation) = self
            .list
            .iter()
            .find(|automation| automation.id == automation_id)
            .cloned()
        else {
            return Task::ready(Err("Automation not found.".into()));
        };
        self.running = Some(automation_id.to_string());
        cx.notify();
        let run_now = self.backend.run_now(automation_id.to_string(), self.now());
        cx.spawn(async move |this, cx| {
            let result = async {
                let run = run_now.await?;
                this.update(cx, |this, cx| this.changed(cx))
                    .map_err(|error| error.to_string())?;
                let launch = this
                    .update(cx, |this, cx| {
                        let prompt = run
                            .prompt
                            .clone()
                            .unwrap_or_else(|| automation.prompt.clone());
                        this.launch(automation, run, true, prompt, None, cx)
                    })
                    .map_err(|error| error.to_string())?;
                launch.await
            }
            .await;
            this.update(cx, |this, cx| {
                if let Err(error) = &result {
                    this.error = Some(error.clone());
                }
                this.running = None;
                cx.notify();
            })
            .ok();
            result
        })
    }

    // Launching.

    /// `launchAutomation`: open (or reuse) the automation's session, mark
    /// the run running, and submit its prompt. The run settles to
    /// succeeded, failed, or cancelled when the turn ends; a launch that
    /// fails before the turn starts records the failure.
    pub fn launch(
        &mut self,
        automation: Automation,
        run: AutomationRun,
        reveal: bool,
        prompt: String,
        source_work_item: Option<LinkedWorkItem>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let event_run = run.trigger == AutomationRunTrigger::Event;
        let linked = source_work_item.or_else(|| linked_work_item_from_automation_event(&run));
        let host = self.host.clone();
        let sessions = Engine::sessions(cx);
        let reusable = if automation.reuse_session {
            automation.last_session_id.as_ref().and_then(|last| {
                sessions
                    .read(cx)
                    .all()
                    .iter()
                    .find(|entry| {
                        entry.id == *last
                            && entry.harness == automation.harness
                            && !entry.is_busy()
                            && entry.worktree_removed != Some(true)
                            && !self.reservations.contains(&entry.id)
                            && reusable_workspace(&automation, entry)
                    })
                    .cloned()
            })
        } else {
            None
        };

        let session = match reusable {
            None => {
                let mut session = host.new_session(
                    automation.harness,
                    &automation.cwd,
                    Some(&automation.model),
                    Some(automation.runtime_mode),
                    automation.model_settings.as_ref(),
                    cx,
                );
                session.title = if event_run {
                    automation.harness.label().to_string()
                } else {
                    format_session_title(automation.harness, &automation.name)
                };
                session.automation_id = Some(automation.id.clone());
                if let Some(linked) = &linked {
                    session.linked_work_item = Some(linked.clone());
                }
                match automation.workspace_mode {
                    AutomationWorkspaceMode::Worktree => {
                        session.workspace_mode = Some(WorkspaceMode::Worktree);
                        session.worktree_base = Some("HEAD".into());
                    }
                    AutomationWorkspaceMode::Existing => {
                        if let Some(worktree) = automation
                            .worktree_cwd
                            .as_ref()
                            .filter(|worktree| !worktree.is_empty())
                        {
                            session.worktree_cwd = Some(worktree.clone());
                        }
                    }
                    AutomationWorkspaceMode::Current => {}
                }
                let added = session.clone();
                sessions.update(cx, |sessions, cx| sessions.upsert(added, cx));
                let tab = new_tab(&session.id);
                let tab_id = tab.id.clone();
                host.append_tab(tab, &automation.cwd, cx);
                if reveal {
                    host.activate_tab(&tab_id, cx);
                }
                session
            }
            Some(existing) => {
                let mut stamped = existing;
                stamped.automation_id = Some(automation.id.clone());
                stamped.model = automation.model.clone();
                stamped.model_settings = automation.model_settings.clone().unwrap_or_default();
                stamped.runtime_mode = automation.runtime_mode;
                if let Some(linked) = &linked {
                    stamped.linked_work_item = Some(linked.clone());
                }
                let replaced = stamped.clone();
                sessions.update(cx, |sessions, cx| sessions.upsert(replaced, cx));
                if reveal {
                    host.focus_open_session(&stamped.id, cx);
                }
                stamped
            }
        };

        let session_id = session.id.clone();
        self.reservations.insert(session_id.clone());

        if let Some(folder_id) = automation
            .session_folder_id
            .as_ref()
            .filter(|folder| !folder.is_empty())
            && looks_like_project(&automation.cwd)
        {
            let folders = place_session_in_folder(
                &load_session_folders(&self.kv, &automation.cwd),
                &session_id,
                &SessionFolderTarget::Existing {
                    folder_id: folder_id.clone(),
                },
            );
            save_session_folders(&self.kv, &automation.cwd, &folders);
        }

        if reveal {
            host.show_sessions(&session.cwd, cx);
        }

        let backend = self.backend.clone();
        let clock = self.clock.clone();
        let running = backend.run_update(
            run.id.clone(),
            AutomationRunStatus::Running,
            Some(session_id.clone()),
            None,
            self.now(),
        );
        cx.spawn(async move |this, cx| {
            if let Err(reason) = running.await {
                let failed = backend.run_update(
                    run.id.clone(),
                    AutomationRunStatus::Failed,
                    None,
                    Some(reason.clone()),
                    clock(),
                );
                failed.await.ok();
                this.update(cx, |this, cx| {
                    this.reservations.remove(&session_id);
                    this.changed(cx);
                })
                .ok();
                return Err(reason);
            }
            this.update(cx, |this, cx| this.changed(cx)).ok();
            // From here the settlement callback owns the reservation,
            // including a rejected submission that never starts a turn.
            let on_settled = settle_run(this.clone(), backend, clock, run.id, session_id.clone());
            submit_with_settlement(
                cx,
                move |on_settled, cx| {
                    host.submit(
                        &session_id,
                        &prompt,
                        Vec::new(),
                        SubmitOptions {
                            refresh_title: event_run,
                            on_settled: Some(on_settled),
                            ..SubmitOptions::default()
                        },
                        cx,
                    )
                },
                on_settled,
                REJECTION_MESSAGE,
            )
            .await;
            Ok(())
        })
    }

    // Scheduling.

    /// Start the scheduler: recover interrupted runs, then claim and launch
    /// due runs every 30 s and after the machine wakes.
    pub fn start_scheduler(&mut self, cx: &mut Context<Self>) {
        if self.scheduler.is_some() {
            return;
        }
        let clock = self.clock.clone();
        let timer = cx.background_executor().clone();
        self.scheduler = Some(cx.spawn(async move |this, cx| {
            let mut last_tick = clock();
            let mut last_evaluation = last_tick;
            if this
                .update(cx, |this, cx| this.evaluate(cx).detach())
                .is_err()
            {
                return;
            }
            loop {
                timer.timer(SCHEDULER_TICK).await;
                let now = clock();
                let slept = now - last_tick > SCHEDULER_TICK.as_millis() as i64 + SLEEP_SLACK_MS;
                last_tick = now;
                if !slept && now - last_evaluation < EVALUATE_INTERVAL_MS {
                    continue;
                }
                last_evaluation = now;
                if this
                    .update(cx, |this, cx| this.evaluate(cx).detach())
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    /// Stop the scheduler (`disposed`).
    pub fn stop_scheduler(&mut self) {
        self.scheduler = None;
    }

    /// The window became visible: evaluate now.
    pub fn window_visible(&mut self, cx: &mut Context<Self>) {
        self.evaluate(cx).detach();
    }

    /// One scheduler pass: finish recovery, claim due runs, and launch them
    /// in the background. Failures wait for the next pass; launches record
    /// their own failures.
    pub fn evaluate(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.evaluating {
            return Task::ready(());
        }
        self.evaluating = true;
        let recovery = self.ensure_recovery(cx);
        let backend = self.backend.clone();
        let clock = self.clock.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                recovery.await?;
                let (due, any_due) = claim_due_automations(backend.as_ref(), clock()).await?;
                this.update(cx, |this, cx| {
                    if any_due {
                        this.changed(cx);
                    }
                    for item in due {
                        let prompt = item
                            .run
                            .prompt
                            .clone()
                            .unwrap_or_else(|| item.automation.prompt.clone());
                        this.launch(item.automation, item.run, false, prompt, None, cx)
                            .detach();
                    }
                })
                .map_err(|error| error.to_string())
            }
            .await;
            if let Err(error) = result {
                log::warn!("Automation scheduling will retry: {error}");
            }
            this.update(cx, |this, _| this.evaluating = false).ok();
        })
    }

    /// `ensureAutomationRecovery`: once per app run, cancel runs that were
    /// running when the app stopped and relaunch the pending ones. A failed
    /// recovery runs again on the next call.
    pub fn ensure_recovery(&mut self, cx: &mut Context<Self>) -> Recovery {
        if let Some(recovery) = &self.recovery {
            return recovery.clone();
        }
        let recover = self.backend.recover(self.recovery_cutoff, self.now());
        let recovery = cx
            .spawn(async move |this, cx| {
                let result = async {
                    let pending = recover.await?;
                    for item in pending {
                        let prompt = item
                            .run
                            .prompt
                            .clone()
                            .unwrap_or_else(|| item.automation.prompt.clone());
                        let launch = this
                            .update(cx, |this, cx| {
                                this.launch(item.automation, item.run, false, prompt, None, cx)
                            })
                            .map_err(|error| error.to_string())?;
                        launch.await.ok();
                    }
                    Ok(())
                }
                .await;
                if result.is_err() {
                    this.update(cx, |this, _| this.recovery = None).ok();
                }
                result
            })
            .shared();
        self.recovery = Some(recovery.clone());
        recovery
    }

    /// `onInboxAppeared`: launch the automations new Inbox items trigger.
    pub fn inbox_appeared(
        &mut self,
        items: Vec<InboxEventItem>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let recovery = self.ensure_recovery(cx);
        let backend = self.backend.clone();
        let kv = self.kv.clone();
        let retries = self.inbox_retries.clone();
        let clock = self.clock.clone();
        cx.spawn(async move |this, cx| {
            if recovery.await.is_err() {
                return;
            }
            let Ok(claimed) =
                claim_inbox_automation_runs(backend.as_ref(), &kv, &retries, &items, clock()).await
            else {
                return;
            };
            this.update(cx, |this, cx| {
                if !claimed.is_empty() {
                    this.changed(cx);
                }
                for item in claimed {
                    this.launch(
                        item.due.automation,
                        item.due.run,
                        false,
                        item.prompt,
                        item.linked_work_item,
                        cx,
                    )
                    .detach();
                }
            })
            .ok();
        })
    }
}

/// The reuse check's workspace rule: a current-folder automation reuses a
/// session on the project folder itself; an existing-worktree automation
/// reuses one working in that worktree; a new-worktree automation never
/// reuses.
fn reusable_workspace(automation: &Automation, entry: &Session) -> bool {
    match automation.workspace_mode {
        AutomationWorkspaceMode::Current => {
            entry.workspace_mode != Some(WorkspaceMode::Worktree)
                && entry.worktree_cwd.as_deref().is_none_or(str::is_empty)
                && path_key(&entry.cwd) == path_key(&automation.cwd)
        }
        AutomationWorkspaceMode::Existing => {
            path_key(session_work_cwd(entry))
                == path_key(automation.worktree_cwd.as_deref().unwrap_or(""))
        }
        AutomationWorkspaceMode::Worktree => false,
    }
}

/// The settlement callback: record how the turn ended, then release the
/// session's reservation.
fn settle_run(
    this: WeakEntity<Automations>,
    backend: Arc<dyn AutomationsBackend>,
    clock: Clock,
    run_id: String,
    session_id: String,
) -> OnSettled {
    Rc::new(move |outcome: ControlOutcome, cx| {
        let status = match outcome.status {
            ControlStatus::Completed => AutomationRunStatus::Succeeded,
            ControlStatus::Cancelled => AutomationRunStatus::Cancelled,
            ControlStatus::Failed => AutomationRunStatus::Failed,
        };
        let update = backend.run_update(
            run_id.clone(),
            status,
            Some(session_id.clone()),
            outcome.error.clone(),
            clock(),
        );
        let this = this.clone();
        let session_id = session_id.clone();
        cx.spawn(async move |cx| {
            let updated = update.await;
            this.update(cx, |this, cx| {
                this.reservations.remove(&session_id);
                if updated.is_ok() {
                    this.changed(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    })
}
