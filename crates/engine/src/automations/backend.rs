//! The automation and reminder commands the TypeScript reached through
//! `invoke`, as traits. `StoreAutomationsBackend` and
//! `StoreRemindersBackend` run them over `monocode-store` on the background
//! executor; tests use fakes or an in-memory store.
//!
//! The store's types keep their fields private, so values cross the trait
//! as this package's serde types and convert through JSON, the same shape
//! the TypeScript sent and received.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::FutureExt;
use gpui::BackgroundExecutor;
use monocode_store::StoreEvents;
use monocode_store::automations as store_automations;
use monocode_store::reminders::{self as store_reminders, OpenReminder, ReminderService};
use monocode_store::session_store::SessionStore;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::events::AutomationEventClaim;
use super::model::{
    Automation, AutomationRun, AutomationRunStatus, AutomationUpsert, DueAutomationRun,
};
use crate::runtime::StoreFuture;
use crate::runtime::backend::NoStoreEvents;

/// The automation commands of src-tauri, one method per `invoke` name.
pub trait AutomationsBackend: Send + Sync + 'static {
    /// `automations_list`: newest change first.
    fn list(&self) -> StoreFuture<Vec<Automation>>;
    /// `automations_upsert`.
    fn upsert(&self, automation: AutomationUpsert) -> StoreFuture<Automation>;
    /// `automations_delete`, with its run history.
    fn delete(&self, id: String) -> StoreFuture<()>;
    /// `automation_runs_list`: newest first, without launch prompts.
    fn runs(&self, automation_id: String) -> StoreFuture<Vec<AutomationRun>>;
    /// `automation_runs_recover`: cancel runs that were running when the app
    /// stopped and return the pending ones created before `started_before`.
    fn recover(&self, started_before: i64, now: i64) -> StoreFuture<Vec<DueAutomationRun>>;
    /// `automation_run_now`: a manual pending run.
    fn run_now(&self, automation_id: String, now: i64) -> StoreFuture<AutomationRun>;
    /// `automations_claim_due`: atomically move the schedule from
    /// `expected_next_run_at` to `next_run_at` and record the run. `None`
    /// when another claim got there first.
    fn claim_due(
        &self,
        automation_id: String,
        expected_next_run_at: i64,
        next_run_at: i64,
        now: i64,
    ) -> StoreFuture<Option<DueAutomationRun>>;
    /// `automations_claim_event`: record an Inbox event once per automation.
    fn claim_event(
        &self,
        automation_id: String,
        claim: AutomationEventClaim,
        now: i64,
    ) -> StoreFuture<Option<DueAutomationRun>>;
    /// `automation_run_update`.
    fn run_update(
        &self,
        run_id: String,
        status: AutomationRunStatus,
        session_id: Option<String>,
        error: Option<String>,
        now: i64,
    ) -> StoreFuture<AutomationRun>;
}

/// JSON round trip between this package's types and the store's.
fn convert<T: Serialize, U: DeserializeOwned>(value: T) -> Result<U, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| error.to_string())
}

/// Convert a list item by item, skipping entries this build cannot read
/// (an unknown harness, for example) instead of failing the whole list.
fn convert_list<T: Serialize, U: DeserializeOwned>(values: Vec<T>) -> Result<Vec<U>, String> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        match convert(value) {
            Ok(converted) => out.push(converted),
            Err(error) => log::warn!("Skipping a stored entry this build cannot read: {error}"),
        }
    }
    Ok(out)
}

/// `AutomationsBackend` over `monocode.db`.
#[derive(Clone)]
pub struct StoreAutomationsBackend {
    store: Arc<SessionStore>,
    events: Arc<dyn StoreEvents>,
    /// `None` runs each call inline, for tests.
    executor: Option<BackgroundExecutor>,
}

impl StoreAutomationsBackend {
    pub fn new(
        store: Arc<SessionStore>,
        events: Arc<dyn StoreEvents>,
        executor: BackgroundExecutor,
    ) -> Self {
        Self {
            store,
            events,
            executor: Some(executor),
        }
    }

    /// A backend with nobody listening for change notices.
    pub fn quiet(store: Arc<SessionStore>, executor: BackgroundExecutor) -> Self {
        Self::new(store, Arc::new(NoStoreEvents), executor)
    }

    /// A backend that runs each call on the caller's thread.
    pub fn inline(store: Arc<SessionStore>) -> Self {
        Self {
            store,
            events: Arc::new(NoStoreEvents),
            executor: None,
        }
    }

    fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(&SessionStore, &dyn StoreEvents) -> Result<T, String> + Send + 'static,
    ) -> StoreFuture<T> {
        let store = self.store.clone();
        let events = self.events.clone();
        match &self.executor {
            Some(executor) => executor
                .spawn(async move { op(&store, events.as_ref()) })
                .boxed(),
            None => futures::future::ready(op(&store, events.as_ref())).boxed(),
        }
    }
}

impl AutomationsBackend for StoreAutomationsBackend {
    fn list(&self) -> StoreFuture<Vec<Automation>> {
        self.run(|store, _| convert_list(store_automations::automations_list(store)?))
    }

    fn upsert(&self, automation: AutomationUpsert) -> StoreFuture<Automation> {
        self.run(move |store, events| {
            let input = convert(automation)?;
            convert(store_automations::automations_upsert(events, store, input)?)
        })
    }

    fn delete(&self, id: String) -> StoreFuture<()> {
        self.run(move |store, events| store_automations::automations_delete(events, store, id))
    }

    fn runs(&self, automation_id: String) -> StoreFuture<Vec<AutomationRun>> {
        self.run(move |store, _| {
            convert_list(store_automations::automation_runs_list(
                store,
                automation_id,
            )?)
        })
    }

    fn recover(&self, started_before: i64, now: i64) -> StoreFuture<Vec<DueAutomationRun>> {
        self.run(move |store, events| {
            convert_list(store_automations::automation_runs_recover(
                events,
                store,
                started_before,
                now,
            )?)
        })
    }

    fn run_now(&self, automation_id: String, now: i64) -> StoreFuture<AutomationRun> {
        self.run(move |store, events| {
            convert(store_automations::automation_run_now(
                events,
                store,
                automation_id,
                now,
            )?)
        })
    }

    fn claim_due(
        &self,
        automation_id: String,
        expected_next_run_at: i64,
        next_run_at: i64,
        now: i64,
    ) -> StoreFuture<Option<DueAutomationRun>> {
        self.run(move |store, events| {
            store_automations::automations_claim_due(
                events,
                store,
                automation_id,
                expected_next_run_at,
                next_run_at,
                now,
            )?
            .map(convert)
            .transpose()
        })
    }

    fn claim_event(
        &self,
        automation_id: String,
        claim: AutomationEventClaim,
        now: i64,
    ) -> StoreFuture<Option<DueAutomationRun>> {
        self.run(move |store, events| {
            let claim = convert(claim)?;
            store_automations::automations_claim_event(events, store, automation_id, claim, now)?
                .map(convert)
                .transpose()
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
        self.run(move |store, events| {
            convert(store_automations::automation_run_update(
                events,
                store,
                run_id,
                status.as_str().to_string(),
                session_id,
                error,
                now,
            )?)
        })
    }
}

/// `SessionReminder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReminder {
    pub session_id: String,
    pub due_at: i64,
    pub fired_at: Option<i64>,
    pub title: String,
    pub harness: monocode_core::HarnessId,
    pub cwd: String,
}

/// `ReminderTarget`: a reminder by session and due time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderTarget {
    pub session_id: String,
    pub due_at: i64,
}

impl From<&SessionReminder> for ReminderTarget {
    fn from(reminder: &SessionReminder) -> Self {
        Self {
            session_id: reminder.session_id.clone(),
            due_at: reminder.due_at,
        }
    }
}

/// One project's rule for native reminder delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderRule {
    pub enabled: bool,
    pub after: i64,
}

/// The preferences `reminder_configure` takes: the notification and sound
/// settings, and the project rule of each reminder's session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderPreferences {
    pub notifications_enabled: bool,
    pub sound: bool,
    pub project_rules: BTreeMap<String, ReminderRule>,
}

/// A banner the poller wants shown. `identifier` is
/// `reminder:<session id>:<due at>`; a click comes back to
/// `Reminders::open_from_notification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderNotice {
    pub identifier: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub sound: bool,
}

/// One poller pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReminderPoll {
    /// The pass claimed due reminders (`monocode:reminders-changed`).
    pub changed: bool,
    pub notices: Vec<ReminderNotice>,
}

/// The reminder commands of src-tauri. The window bookkeeping methods are
/// synchronous, as the Tauri commands were.
pub trait RemindersBackend: Send + Sync + 'static {
    /// `reminder_list`.
    fn list(&self) -> StoreFuture<Vec<SessionReminder>>;
    /// `reminder_set`.
    fn set(&self, session_ids: Vec<String>, due_at: i64) -> StoreFuture<()>;
    /// `reminder_clear`: only reminders still due at `expected_due_at`, when
    /// given.
    fn clear(&self, session_ids: Vec<String>, expected_due_at: Option<i64>) -> StoreFuture<()>;
    /// `reminder_configure`.
    fn configure(&self, preferences: ReminderPreferences) -> StoreFuture<()>;
    /// One pass of the 5 s poller (`poll_due`).
    fn poll_due(&self) -> StoreFuture<ReminderPoll>;
    /// `reminder_register_window`: the sessions `owner` shows.
    fn register_window(&self, owner: &str, session_ids: Vec<String>) -> Result<(), String>;
    /// The windows that registered a session, by owner.
    fn window_sessions(&self) -> HashMap<String, Vec<String>>;
    /// Keep a request for `owner` (`None`: any window) until it takes it.
    fn queue_open(&self, target: ReminderTarget, owner: Option<String>) -> Result<(), String>;
    /// `reminder_take_open`.
    fn take_open(
        &self,
        owner: &str,
        owner_exists: &dyn Fn(&str) -> bool,
    ) -> Result<Option<ReminderTarget>, String>;
}

/// `RemindersBackend` over `monocode.db` and a `ReminderService`.
#[derive(Clone)]
pub struct StoreRemindersBackend {
    store: Arc<SessionStore>,
    events: Arc<dyn StoreEvents>,
    service: Arc<ReminderService>,
    /// `None` runs each call inline, for tests.
    executor: Option<BackgroundExecutor>,
}

impl StoreRemindersBackend {
    pub fn new(
        store: Arc<SessionStore>,
        events: Arc<dyn StoreEvents>,
        executor: BackgroundExecutor,
    ) -> Self {
        Self {
            store,
            events,
            service: Arc::new(ReminderService::default()),
            executor: Some(executor),
        }
    }

    /// A backend with nobody listening for change notices.
    pub fn quiet(store: Arc<SessionStore>, executor: BackgroundExecutor) -> Self {
        Self::new(store, Arc::new(NoStoreEvents), executor)
    }

    /// A backend that runs each call on the caller's thread.
    pub fn inline(store: Arc<SessionStore>) -> Self {
        Self {
            store,
            events: Arc::new(NoStoreEvents),
            service: Arc::new(ReminderService::default()),
            executor: None,
        }
    }

    fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(&SessionStore, &dyn StoreEvents, &ReminderService) -> Result<T, String>
        + Send
        + 'static,
    ) -> StoreFuture<T> {
        let store = self.store.clone();
        let events = self.events.clone();
        let service = self.service.clone();
        match &self.executor {
            Some(executor) => executor
                .spawn(async move { op(&store, events.as_ref(), &service) })
                .boxed(),
            None => futures::future::ready(op(&store, events.as_ref(), &service)).boxed(),
        }
    }
}

/// Forwards change notices and remembers that reminders changed.
struct Tracking<'a> {
    inner: &'a dyn StoreEvents,
    reminders: AtomicBool,
}

impl StoreEvents for Tracking<'_> {
    fn reminders_changed(&self) {
        self.reminders.store(true, Ordering::SeqCst);
        self.inner.reminders_changed();
    }

    fn automations_changed(&self) {
        self.inner.automations_changed();
    }
}

impl RemindersBackend for StoreRemindersBackend {
    fn list(&self) -> StoreFuture<Vec<SessionReminder>> {
        self.run(|store, _, _| convert_list(store_reminders::reminder_list(store)?))
    }

    fn set(&self, session_ids: Vec<String>, due_at: i64) -> StoreFuture<()> {
        self.run(move |store, events, _| {
            store_reminders::reminder_set(store, events, session_ids, due_at)
        })
    }

    fn clear(&self, session_ids: Vec<String>, expected_due_at: Option<i64>) -> StoreFuture<()> {
        self.run(move |store, events, _| {
            store_reminders::reminder_clear(store, events, session_ids, expected_due_at)
        })
    }

    fn configure(&self, preferences: ReminderPreferences) -> StoreFuture<()> {
        self.run(move |_, _, service| {
            store_reminders::reminder_configure(service, convert(preferences)?)
        })
    }

    fn poll_due(&self) -> StoreFuture<ReminderPoll> {
        self.run(|store, events, service| {
            let tracking = Tracking {
                inner: events,
                reminders: AtomicBool::new(false),
            };
            let mut notices = Vec::new();
            store_reminders::poll_due(service, store, &tracking, |notice| {
                notices.push(ReminderNotice {
                    identifier: notice.identifier,
                    title: notice.title,
                    subtitle: notice.subtitle,
                    body: notice.body,
                    sound: notice.sound,
                });
            });
            Ok(ReminderPoll {
                changed: tracking.reminders.load(Ordering::SeqCst),
                notices,
            })
        })
    }

    fn register_window(&self, owner: &str, session_ids: Vec<String>) -> Result<(), String> {
        store_reminders::reminder_register_window(&self.service, owner, session_ids)
    }

    fn window_sessions(&self) -> HashMap<String, Vec<String>> {
        self.service
            .window_sessions
            .lock()
            .map(|owners| owners.clone())
            .unwrap_or_default()
    }

    fn queue_open(&self, target: ReminderTarget, owner: Option<String>) -> Result<(), String> {
        let mut pending = self
            .service
            .pending_open
            .lock()
            .map_err(|error| error.to_string())?;
        *pending = Some(OpenReminder {
            session_id: target.session_id,
            due_at: target.due_at,
            window_label: owner,
        });
        Ok(())
    }

    fn take_open(
        &self,
        owner: &str,
        owner_exists: &dyn Fn(&str) -> bool,
    ) -> Result<Option<ReminderTarget>, String> {
        Ok(
            store_reminders::reminder_take_open(&self.service, owner, owner_exists)?.map(
                |request| ReminderTarget {
                    session_id: request.session_id,
                    due_at: request.due_at,
                },
            ),
        )
    }
}
