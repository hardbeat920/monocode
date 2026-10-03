//! Session reminders. Ports src/features/sessions/model/sessionReminders.ts
//! (presets and labels), src/features/notifications/hooks/
//! useSessionReminders.ts (the `Reminders` entity), the App.tsx callbacks
//! around it (`openReminderSession`, `ensureReminderSessionsSaved`), and the
//! src-tauri side: the 5 s poller that claims due reminders and shows their
//! banners, and the window choice for opening one.
//!
//! The TypeScript ran one hook per window against a shared backend. Here
//! one entity serves every window; each window attaches a `ReminderHost`
//! under its label and registers the sessions it shows.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::Shared;
use gpui::{AppContext, Context, EventEmitter, Task};
use monocode_settings::Kv;
use monocode_store::reminders::{NOTIFICATION_PREFIX, parse_notification};
use monocode_store::session_store::validate_id;

use super::backend::{
    ReminderPreferences, ReminderRule, ReminderTarget, RemindersBackend, SessionReminder,
};
use super::host::{ReminderApp, ReminderHost, ensure_open_session};
use super::local_time::{self, LocalFields};
use crate::attention::notification_preferences::{
    NotificationCategory, PROJECT_NOTIFICATIONS_KEY, get_project_notification_rule,
    load_notification_preferences, next_mute_deadline,
};
use crate::attention::notification_projects::{
    NOTIFICATION_PROJECTS_KEY, known_notification_project,
};
use crate::attention::notifications::{
    NOTIFICATIONS_KEY, NotificationText, load_notifications_enabled,
};
use crate::attention::sounds::{SOUNDS_KEY, load_sounds_enabled};
use crate::attention::{AttentionPlatform, Clock};
use crate::runtime::Engine;

/// The poller interval src-tauri used.
pub const REMINDER_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// The list refresh that catches renamed or deleted sessions, missed
/// events, and a machine waking after a reminder's time.
pub const REMINDER_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// `reminderTime`: the due time for a snooze preset, `None` for unknown
/// presets and for an evening that has passed.
pub fn reminder_time(preset: &str, now: i64) -> Option<i64> {
    match preset {
        "reminder:1h" => return Some(now + 60 * 60 * 1000),
        "reminder:3h" => return Some(now + 3 * 60 * 60 * 1000),
        _ => {}
    }
    let mut date = LocalFields::of(now);
    match preset {
        "reminder:evening" => {
            date.hours = 18;
        }
        "reminder:tomorrow" => {
            date.day += 1;
            date.hours = 9;
        }
        "reminder:next-week" => {
            let days = match (8 - local_time::weekday(now)) % 7 {
                0 => 7,
                days => days,
            };
            date.day += days;
            date.hours = 9;
        }
        _ => return None,
    }
    date.minutes = 0;
    date.seconds = 0;
    date.millis = 0;
    let at = date.to_ms();
    (at > now).then_some(at)
}

/// `formatReminderTime`: `Mon, Sep 21, 9:00 AM` in the en-US form.
pub fn format_reminder_time(due_at: i64) -> String {
    let date = LocalFields::of(due_at);
    format!(
        "{}, {} {}, {}",
        local_time::short_weekday(local_time::weekday(due_at)),
        local_time::short_month(date.month),
        date.day,
        local_time::clock_12h(date.hours, date.minutes)
    )
}

/// What changed, for views that need more than `cx.notify()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemindersEvent {
    /// The reminder list changed (`monocode:reminders-changed`).
    Changed,
    /// An action failed; the app showed `ReminderApp::show_error`.
    Failed(String),
}

type Configuration = Shared<Task<Result<(), String>>>;

/// Reminders for saved sessions, across every window.
pub struct Reminders {
    backend: Arc<dyn RemindersBackend>,
    kv: Kv,
    clock: Clock,
    platform: Option<Arc<dyn AttentionPlatform>>,
    app: Rc<dyn ReminderApp>,
    windows: BTreeMap<String, Rc<dyn ReminderHost>>,

    reminders: Vec<SessionReminder>,
    now: i64,
    error: Option<String>,
    revision: u64,
    configuration_revision: u64,
    configuration_queue: Option<Configuration>,

    _kv_subscriptions: Vec<monocode_settings::Subscription>,
    _kv_watch: Option<Task<()>>,
    mute_timer: Option<Task<()>>,
    poller: Option<Task<()>>,
    refresher: Option<Task<()>>,
}

impl EventEmitter<RemindersEvent> for Reminders {}

impl Reminders {
    pub fn new(
        backend: Arc<dyn RemindersBackend>,
        kv: Kv,
        clock: Clock,
        platform: Option<Arc<dyn AttentionPlatform>>,
        app: Rc<dyn ReminderApp>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (changes, changed) = async_channel::unbounded::<String>();
        let kv_subscriptions = [
            PROJECT_NOTIFICATIONS_KEY,
            NOTIFICATION_PROJECTS_KEY,
            NOTIFICATIONS_KEY,
            SOUNDS_KEY,
        ]
        .into_iter()
        .map(|key| {
            let changes = changes.clone();
            kv.subscribe_key(key, move |change| {
                let _ = changes.try_send(change.key.clone());
            })
        })
        .collect();
        let kv_watch = cx.spawn(async move |this, cx| {
            while let Ok(key) = changed.recv().await {
                if this
                    .update(cx, |this, cx| {
                        if key == PROJECT_NOTIFICATIONS_KEY {
                            this.schedule_mute_expiry(cx);
                        }
                        this.configure_current(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let now = clock();
        let mut this = Self {
            backend,
            kv,
            clock,
            platform,
            app,
            windows: BTreeMap::new(),
            reminders: Vec::new(),
            now,
            error: None,
            revision: 0,
            configuration_revision: 0,
            configuration_queue: None,
            _kv_subscriptions: kv_subscriptions,
            _kv_watch: Some(kv_watch),
            mute_timer: None,
            poller: None,
            refresher: None,
        };
        this.schedule_mute_expiry(cx);
        this
    }

    /// Replace the app calls (error dialog, saving, new windows).
    pub fn set_app(&mut self, app: Rc<dyn ReminderApp>) {
        self.app = app;
    }

    // Reading.

    /// Every reminder, soonest first.
    pub fn reminders(&self) -> &[SessionReminder] {
        &self.reminders
    }

    /// The reminders to show as due notices: past due, in a known project,
    /// and allowed by the project's reminder rule.
    pub fn due(&self) -> Vec<SessionReminder> {
        self.reminders
            .iter()
            .filter(|reminder| {
                let Some(project) = known_notification_project(&self.kv, &reminder.cwd) else {
                    return false;
                };
                if reminder.due_at > self.now {
                    return false;
                }
                let rule = get_project_notification_rule(
                    &self.kv,
                    &project.id,
                    NotificationCategory::Reminders,
                );
                rule.enabled && reminder.due_at > rule.after
            })
            .cloned()
            .collect()
    }

    /// The last load error, for the notices' retry row.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The reminder for a session, if any.
    pub fn reminder_for(&self, session_id: &str) -> Option<&SessionReminder> {
        self.reminders
            .iter()
            .find(|reminder| reminder.session_id == session_id)
    }

    // Windows.

    /// A window mounted: remember its host, load, and take an open request
    /// queued before it existed.
    pub fn attach_window(
        &mut self,
        owner: &str,
        host: Rc<dyn ReminderHost>,
        cx: &mut Context<Self>,
    ) {
        self.windows.insert(owner.to_string(), host);
        let refresh = self.refresh(cx);
        let owner = owner.to_string();
        cx.spawn(async move |this, cx| {
            refresh.await;
            this.update(cx, |this, cx| this.take_open(&owner, cx)).ok();
        })
        .detach();
    }

    /// The window closed.
    pub fn detach_window(&mut self, owner: &str) {
        self.windows.remove(owner);
        let _ = self.backend.register_window(owner, Vec::new());
    }

    /// Labels of the attached windows.
    pub fn window_labels(&self) -> Vec<String> {
        self.windows.keys().cloned().collect()
    }

    /// `reminder_register_window`: the sessions a window shows (Inbox Asks
    /// excluded), for choosing the window that opens a reminder.
    pub fn register_window(&mut self, owner: &str, session_ids: Vec<String>) {
        if let Err(error) = self.backend.register_window(owner, session_ids) {
            self.error = Some(error);
        }
    }

    /// The window gained focus or became visible.
    pub fn window_focused(&mut self, owner: &str, cx: &mut Context<Self>) {
        self.refresh(cx).detach();
        self.take_open(owner, cx);
    }

    /// Store change notices from elsewhere (`monocode:reminders-changed`).
    pub fn notify_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(RemindersEvent::Changed);
        self.refresh(cx).detach();
    }

    // Loading and native configuration.

    /// Load the list, then send the delivery preferences for it.
    pub fn refresh(&mut self, cx: &mut Context<Self>) -> Task<()> {
        self.revision += 1;
        let request = self.revision;
        let list = self.backend.list();
        cx.spawn(async move |this, cx| {
            let result = async {
                let items = list.await?;
                let configure = this
                    .update(cx, |this, cx| {
                        if request != this.revision {
                            return None;
                        }
                        this.reminders = items;
                        this.now = (this.clock)();
                        this.error = None;
                        cx.notify();
                        Some(this.configure(cx))
                    })
                    .map_err(|error| error.to_string())?;
                match configure {
                    Some(configure) => configure.await,
                    None => Ok(()),
                }
            }
            .await;
            if let Err(error) = result {
                this.update(cx, |this, cx| {
                    if request == this.revision {
                        this.error = Some(error);
                        cx.notify();
                    }
                })
                .ok();
            }
        })
    }

    /// The preferences native delivery uses: the notification and sound
    /// settings, and each reminder's project rule.
    pub fn delivery_preferences(&self) -> ReminderPreferences {
        ReminderPreferences {
            notifications_enabled: load_notifications_enabled(&self.kv),
            sound: load_sounds_enabled(&self.kv),
            project_rules: self
                .reminders
                .iter()
                .filter_map(|reminder| {
                    let project = known_notification_project(&self.kv, &reminder.cwd)?;
                    let rule = get_project_notification_rule(
                        &self.kv,
                        &project.id,
                        NotificationCategory::Reminders,
                    );
                    Some((
                        reminder.session_id.clone(),
                        ReminderRule {
                            enabled: rule.enabled,
                            after: rule.after,
                        },
                    ))
                })
                .collect(),
        }
    }

    /// `configure`: native preferences are global. Writes stay in order, and
    /// a queued write that a newer one replaced is skipped. The snapshot is
    /// taken when the write runs.
    pub fn configure(&mut self, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        self.configuration_revision += 1;
        let request = self.configuration_revision;
        let previous = self.configuration_queue.take();
        let backend = self.backend.clone();
        let pending: Configuration = cx
            .spawn(async move |this, cx| {
                if let Some(previous) = previous {
                    let _ = previous.await;
                }
                let preferences = this
                    .update(cx, |this, _| {
                        (request == this.configuration_revision)
                            .then(|| this.delivery_preferences())
                    })
                    .map_err(|error| error.to_string())?;
                match preferences {
                    Some(preferences) => backend.configure(preferences).await,
                    None => Ok(()),
                }
            })
            .shared();
        self.configuration_queue = Some(pending.clone());
        cx.spawn(async move |this, cx| {
            let result = pending.await;
            match result {
                Err(error)
                    if this
                        .read_with(cx, |this, _| this.configuration_revision == request)
                        .unwrap_or(false) =>
                {
                    Err(error)
                }
                _ => Ok(()),
            }
        })
    }

    /// A setting or project rule changed: redraw the due list and send the
    /// new rules.
    fn configure_current(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let configure = self.configure(cx);
        cx.spawn(async move |this, cx| {
            if let Err(error) = configure.await {
                this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The deadline half of `subscribeNotificationPreferences`: a timed mute
    /// that ends changes the rules.
    fn schedule_mute_expiry(&mut self, cx: &mut Context<Self>) {
        let now = (self.clock)();
        let Some(deadline) = next_mute_deadline(&load_notification_preferences(&self.kv), now)
        else {
            self.mute_timer = None;
            return;
        };
        let wait = Duration::from_millis((deadline - now).clamp(0, 2_147_483_647) as u64);
        let timer = cx.background_executor().timer(wait);
        self.mute_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| {
                this.schedule_mute_expiry(cx);
                this.configure_current(cx);
            })
            .ok();
        }));
    }

    // Actions.

    fn report(&mut self, error: String, cx: &mut Context<Self>) {
        self.app.show_error(&error, cx);
        cx.emit(RemindersEvent::Failed(error));
    }

    /// Save the conversations, then set their reminder.
    pub fn schedule(
        &mut self,
        session_ids: Vec<String>,
        due_at: i64,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let saved = self.app.ensure_saved(&session_ids, cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                saved.await?;
                backend.set(session_ids, due_at).await?;
                let refresh = this
                    .update(cx, |this, cx| {
                        cx.emit(RemindersEvent::Changed);
                        this.refresh(cx)
                    })
                    .map_err(|error| error.to_string())?;
                refresh.await;
                Ok(())
            }
            .await;
            if let Err(error) = &result {
                this.update(cx, |this, cx| this.report(error.clone(), cx))
                    .ok();
            }
            result
        })
    }

    /// Clear reminders; with `expected_due_at`, only ones still due then.
    pub fn cancel(
        &mut self,
        session_ids: Vec<String>,
        expected_due_at: Option<i64>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let clear = self.backend.clear(session_ids, expected_due_at);
        cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                clear.await?;
                let refresh = this
                    .update(cx, |this, cx| {
                        cx.emit(RemindersEvent::Changed);
                        this.refresh(cx)
                    })
                    .map_err(|error| error.to_string())?;
                refresh.await;
                Ok(())
            }
            .await;
            if let Err(error) = &result {
                this.update(cx, |this, cx| this.report(error.clone(), cx))
                    .ok();
            }
            result
        })
    }

    /// `dismissDue`: the session continued, so its due reminder is done. A
    /// reminder still in the future stays.
    pub fn dismiss_due(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let now = (self.clock)();
        let Some(due_at) = self
            .reminders
            .iter()
            .find(|reminder| reminder.session_id == session_id && reminder.due_at <= now)
            .map(|reminder| reminder.due_at)
        else {
            return Task::ready(Ok(()));
        };
        self.cancel(vec![session_id.to_string()], Some(due_at), cx)
    }

    /// `open`: route the reminder to the window that should show it.
    pub fn open(&mut self, target: ReminderTarget, cx: &mut Context<Self>) -> Result<(), String> {
        let result =
            validate_id(&target.session_id, "session").and_then(|_| self.queue_open(target, cx));
        if let Err(error) = &result {
            self.report(error.clone(), cx);
        }
        result
    }

    /// A banner was clicked: `reminder:<session id>:<due at>`.
    pub fn open_from_notification(&mut self, identifier: &str, cx: &mut Context<Self>) {
        let rest = identifier
            .strip_prefix(NOTIFICATION_PREFIX)
            .unwrap_or(identifier);
        let Some((session_id, due_at)) = parse_notification(rest) else {
            return;
        };
        let _ = self.queue_open(ReminderTarget { session_id, due_at }, cx);
    }

    /// `queue_open` from src-tauri: prefer the focused window that shows
    /// the session, then any window that shows it, then the focused window,
    /// then `main`, then the first. The request waits until that window
    /// takes it.
    fn queue_open(&mut self, target: ReminderTarget, cx: &mut Context<Self>) -> Result<(), String> {
        let owners = self.backend.window_sessions();
        let owns = |label: &str| {
            owners
                .get(label)
                .is_some_and(|ids| ids.contains(&target.session_id))
        };
        let focused = |host: &Rc<dyn ReminderHost>| host.is_focused(cx);
        let chosen = self
            .windows
            .iter()
            .find(|(label, host)| owns(label) && focused(host))
            .or_else(|| self.windows.iter().find(|(label, _)| owns(label)))
            .or_else(|| self.windows.iter().find(|(_, host)| focused(host)))
            .or_else(|| self.windows.get_key_value("main"))
            .or_else(|| self.windows.iter().next())
            .map(|(label, host)| (label.clone(), host.clone()));
        self.backend
            .queue_open(target, chosen.as_ref().map(|(label, _)| label.clone()))?;
        match chosen {
            Some((label, host)) => {
                host.bring_forward(cx);
                self.take_open(&label, cx);
            }
            None => self.app.open_new_window(cx),
        }
        Ok(())
    }

    /// `takeOpen`: open the request meant for this window, if any.
    pub fn take_open(&mut self, owner: &str, cx: &mut Context<Self>) {
        let windows = &self.windows;
        let request = self
            .backend
            .take_open(owner, &|label: &str| windows.contains_key(label));
        match request {
            Ok(Some(request)) => self.open_here(owner, request, cx).detach(),
            Ok(None) => {}
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    /// `openHere`: show the session in this window, then clear the
    /// reminder that asked for it. An old notification never clears a newer
    /// reminder for the same session.
    pub fn open_here(
        &mut self,
        owner: &str,
        target: ReminderTarget,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let Some(host) = self.windows.get(owner).cloned() else {
            return Task::ready(Err("The window is no longer open.".into()));
        };
        let opening = ensure_open_session(&target.session_id, cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                let session = opening
                    .await
                    .ok_or_else(|| "This conversation is no longer available.".to_string())?;
                let shown = cx.update(|cx| host.show_session(&session, cx));
                shown.await?;
                backend
                    .clear(vec![target.session_id.clone()], Some(target.due_at))
                    .await?;
                let refresh = this
                    .update(cx, |this, cx| {
                        cx.emit(RemindersEvent::Changed);
                        this.refresh(cx)
                    })
                    .map_err(|error| error.to_string())?;
                refresh.await;
                Ok(())
            }
            .await;
            if let Err(error) = &result {
                this.update(cx, |this, cx| this.report(error.clone(), cx))
                    .ok();
            }
            result
        })
    }

    // Background work.

    /// Start the 5 s poller that claims due reminders and shows their
    /// banners, and the 30 s list refresh.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.poller.is_none() {
            let backend = self.backend.clone();
            let executor = cx.background_executor().clone();
            self.poller = Some(cx.spawn(async move |this, cx| {
                loop {
                    executor.timer(REMINDER_POLL_INTERVAL).await;
                    let Ok(poll) = backend.poll_due().await else {
                        continue;
                    };
                    let alive = this.update(cx, |this, cx| {
                        if poll.changed {
                            this.notify_changed(cx);
                        }
                        this.show_banners(poll.notices, cx);
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            }));
        }
        if self.refresher.is_none() {
            let executor = cx.background_executor().clone();
            self.refresher = Some(cx.spawn(async move |this, cx| {
                loop {
                    executor.timer(REMINDER_REFRESH_INTERVAL).await;
                    if this
                        .update(cx, |this, cx| this.refresh(cx).detach())
                        .is_err()
                    {
                        return;
                    }
                }
            }));
        }
    }

    /// Stop the poller and the refresh.
    pub fn stop(&mut self) {
        self.poller = None;
        self.refresher = None;
    }

    fn show_banners(
        &mut self,
        notices: Vec<super::backend::ReminderNotice>,
        cx: &mut Context<Self>,
    ) {
        let Some(platform) = self.platform.clone() else {
            return;
        };
        if notices.is_empty() {
            return;
        }
        cx.background_spawn(async move {
            for notice in notices {
                let text = NotificationText {
                    title: notice.title,
                    subtitle: notice.subtitle,
                    body: notice.body,
                };
                if let Err(error) =
                    platform.show_notification(&notice.identifier, &text, notice.sound)
                {
                    log::warn!("Could not show a reminder banner: {error}");
                }
            }
        })
        .detach();
    }
}

/// `ensureReminderSessionsSaved`: save each open session; one that cannot
/// be saved yet has no message.
pub fn ensure_sessions_saved(session_ids: &[String], cx: &gpui::App) -> Task<Result<(), String>> {
    let sessions = Engine::sessions(cx);
    let writer = Engine::writer(cx);
    let open: Vec<_> = session_ids
        .iter()
        .filter_map(|id| sessions.read(cx).get(id).cloned())
        .collect();
    cx.spawn(async move |_| {
        for session in open {
            let saved = writer.upsert_session(&session).await?;
            if saved.is_none() {
                return Err(
                    "Send a message in this conversation before setting a reminder.".into(),
                );
            }
        }
        Ok(())
    })
}
