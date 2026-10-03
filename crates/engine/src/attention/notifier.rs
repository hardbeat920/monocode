//! The `Notifier` entity: OS banners, the Dock badge, sounds, and the
//! sessions that finished while the user looked elsewhere.
//!
//! Ports the stateful half of src/features/notifications/model/notifications.ts
//! (permission cache, window focus, `notifySession`,
//! `announceSessionFinished`), dockBadge.ts, the `useInputNotifications`
//! hook, the change subscriptions of notificationPreferences.ts and
//! notificationProjects.ts, the cue state of sounds.ts, and the App.tsx
//! effects at lines 1647-1704: the boot permission probe, the unseen
//! finished set, the live agents list, and the Dock badge sync.
//!
//! Rule: code running inside `Notifier::update` never updates `Sessions`.
//! A `Sessions` flush calls back into the notifier for the Dock badge, so
//! the reverse would lease the notifier twice.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription, Task};
use monocode_core::inbox::LinkedWorkItemUpdateCard;
use monocode_core::session::{Session, session_needs_input};
use monocode_settings::Kv;

use super::live_agents::{LiveAgent, live_agents_from_sessions};
use super::notification_preferences::{
    NotificationCategory, NotificationSubject, PROJECT_NOTIFICATIONS_KEY,
    allows_project_notification, load_notification_preferences, next_mute_deadline,
};
use super::notification_projects::{NOTIFICATION_PROJECTS_KEY, known_notification_project};
use super::notifications::{
    NOTIFICATIONS_KEY, NotificationEvent, NotificationPermission, load_notifications_enabled,
    notification_text, pending_input_notifications, save_notifications_enabled, should_notify,
};
use super::platform::AttentionPlatform;
use super::sounds::{
    SOUNDS_KEY, SOUNDS_VOLUME, SoundCue, SoundCues, cue_allowed, load_sounds_enabled,
    save_sounds_enabled,
};
use super::{AttentionFocus, Clock};
use crate::runtime::engine::Engine;
use crate::runtime::session_done::next_unseen_finished_sessions;
use crate::runtime::sessions::{Sessions, SessionsEvent};

/// What changed on the `Notifier`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifierEvent {
    /// The user clicked a banner for this open session
    /// (`NOTIFICATION_CLICK_EVENT`). The app brings its window forward;
    /// the notifier already asked the router to open the session.
    Clicked(String),
    /// Project notification preferences changed, or a timed mute expired.
    PreferencesChanged,
    /// The notification project catalog changed.
    ProjectsChanged,
    /// The notifications or sounds setting changed.
    SettingsChanged,
    /// The OS reported a permission.
    PermissionChanged(NotificationPermission),
    /// The unseen finished set changed.
    UnseenChanged,
}

/// OS notifications and the attention state around them.
pub struct Notifier {
    kv: Kv,
    platform: Arc<dyn AttentionPlatform>,
    clock: Clock,
    /// `cachedNotificationPermission`: the last permission the OS reported.
    permission: NotificationPermission,
    /// Tracked from the window focus event, not from the document.
    window_focused: bool,
    focus: AttentionFocus,
    /// `notifiedInputIdsRef`: input requests already announced.
    notified_input: HashSet<String>,
    busy_for_done: HashSet<String>,
    focused_for_done: Option<String>,
    unseen_finished: HashSet<String>,
    /// `lastCount` in dockBadge.ts.
    last_badge_count: Option<usize>,
    cues: SoundCues,
    mute_timer: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _kv_subscriptions: Vec<monocode_settings::Subscription>,
    _kv_watch: Option<Task<()>>,
}

impl EventEmitter<NotifierEvent> for Notifier {}

impl Notifier {
    pub fn new(
        kv: Kv,
        platform: Arc<dyn AttentionPlatform>,
        clock: Clock,
        sessions: &Entity<Sessions>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.subscribe(sessions, |this, _, event: &SessionsEvent, cx| {
                if *event == SessionsEvent::BusyChanged {
                    this.update_unseen(cx);
                }
            }),
            cx.observe(sessions, |this, _, cx| this.run_input_notifications(cx)),
        ];
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
                    .update(cx, |this, cx| this.kv_changed(&key, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let busy_for_done = sessions.read(cx).busy_session_ids().clone();
        Self {
            kv,
            platform,
            clock,
            permission: NotificationPermission::Prompt,
            window_focused: true,
            focus: AttentionFocus::default(),
            notified_input: HashSet::new(),
            busy_for_done,
            focused_for_done: None,
            unseen_finished: HashSet::new(),
            last_badge_count: None,
            cues: SoundCues::default(),
            mute_timer: None,
            _subscriptions: subscriptions,
            _kv_subscriptions: kv_subscriptions,
            _kv_watch: Some(kv_watch),
        }
    }

    /// Boot work: cache the OS decision so a turn ending later can skip a
    /// denied banner, arm the mute expiry timer, and announce requests
    /// already open.
    pub fn boot(&mut self, cx: &mut Context<Self>) {
        if load_notifications_enabled(&self.kv) {
            self.probe_permission(cx).detach();
        }
        self.schedule_mute_expiry(cx);
        self.run_input_notifications(cx);
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    // Reading.

    /// `cachedNotificationPermission`.
    pub fn permission(&self) -> NotificationPermission {
        self.permission
    }

    pub fn window_focused(&self) -> bool {
        self.window_focused
    }

    pub fn focus(&self) -> &AttentionFocus {
        &self.focus
    }

    /// Sessions that finished while unfocused and are still unseen.
    pub fn unseen_finished_ids(&self) -> &HashSet<String> {
        &self.unseen_finished
    }

    /// `loadNotificationsEnabled`.
    pub fn notifications_enabled(&self) -> bool {
        load_notifications_enabled(&self.kv)
    }

    /// `loadSoundsEnabled`.
    pub fn sounds_enabled(&self) -> bool {
        load_sounds_enabled(&self.kv)
    }

    /// The agents panel rows: working sessions and unseen finished ones,
    /// empty while the live agents setting is off.
    pub fn live_agents(&self, cx: &App) -> Vec<LiveAgent> {
        if !monocode_settings::settings_store::load_live_agents_enabled(&self.kv) {
            return Vec::new();
        }
        let sessions = Engine::sessions(cx);
        live_agents_from_sessions(sessions.read(cx).all(), &self.unseen_finished)
    }

    // Settings.

    /// The notifications switch: saving `true` also asks the OS.
    pub fn set_notifications_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        save_notifications_enabled(&self.kv, enabled);
        if enabled {
            self.request_permission(cx).detach();
        }
    }

    /// The sounds switch.
    pub fn set_sounds_enabled(&mut self, enabled: bool, _cx: &mut Context<Self>) {
        save_sounds_enabled(&self.kv, enabled, self.now());
    }

    // Permission.

    fn permission_task(
        &mut self,
        ask: fn(&dyn AttentionPlatform) -> NotificationPermission,
        cx: &mut Context<Self>,
    ) -> Task<NotificationPermission> {
        let platform = self.platform.clone();
        let check = cx.background_spawn(async move { ask(platform.as_ref()) });
        cx.spawn(async move |this, cx| {
            let permission = check.await;
            this.update(cx, |this, cx| {
                this.permission = permission;
                cx.emit(NotifierEvent::PermissionChanged(permission));
                cx.notify();
            })
            .ok();
            permission
        })
    }

    /// `probeNotificationPermission`.
    pub fn probe_permission(&mut self, cx: &mut Context<Self>) -> Task<NotificationPermission> {
        self.permission_task(|platform| platform.notification_permission(), cx)
    }

    /// `requestNotificationPermission`: shows the OS prompt when undecided.
    pub fn request_permission(&mut self, cx: &mut Context<Self>) -> Task<NotificationPermission> {
        self.permission_task(|platform| platform.request_notification_permission(), cx)
    }

    /// `openNotificationSettings`.
    pub fn open_notification_settings(&self, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let platform = self.platform.clone();
        cx.background_spawn(async move { platform.open_notification_settings() })
    }

    // Focus.

    /// `setWindowFocused`. The window focus handler also flushes harness
    /// events and syncs the badge; `Attention::set_window_focused` does both.
    pub fn set_window_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.window_focused = focused;
        if focused {
            // `subscribeNotificationPreferences` re-read on window focus.
            self.schedule_mute_expiry(cx);
        }
        cx.notify();
    }

    pub fn set_focus(&mut self, focus: AttentionFocus, cx: &mut Context<Self>) {
        if self.focus == focus {
            return;
        }
        self.focus = focus;
        self.update_unseen(cx);
        self.run_input_notifications(cx);
    }

    /// The unseen finished set, recomputed when the busy sessions or the
    /// visible session change.
    fn update_unseen(&mut self, cx: &mut Context<Self>) {
        let busy = Engine::sessions(cx).read(cx).busy_session_ids().clone();
        let focused = self.focus.visible_session_id().map(str::to_string);
        if busy == self.busy_for_done && focused == self.focused_for_done {
            return;
        }
        let next = next_unseen_finished_sessions(
            &self.busy_for_done,
            &busy,
            &self.unseen_finished,
            focused.as_deref(),
        );
        self.busy_for_done = busy;
        self.focused_for_done = focused;
        if next != self.unseen_finished {
            self.unseen_finished = next;
            cx.emit(NotifierEvent::UnseenChanged);
            cx.notify();
        }
    }

    // Dock badge.

    /// `syncDockBadge`: push the count of sessions waiting on the user.
    pub fn sync_dock_badge(&mut self, sessions: &[Session]) {
        let count = sessions
            .iter()
            .filter(|session| session_needs_input(session))
            .count();
        if self.last_badge_count == Some(count) {
            return;
        }
        self.last_badge_count = Some(count);
        self.platform.set_dock_badge(count as u32);
    }

    /// `syncDockBadge(sessionsRef.current)`.
    pub fn sync_dock_badge_now(&mut self, cx: &mut Context<Self>) {
        let sessions = Engine::sessions(cx);
        let sessions = sessions.read(cx).all().to_vec();
        self.sync_dock_badge(&sessions);
    }

    // Banners.

    /// `useInputNotifications`: one banner per new approval or question.
    /// Each update coalesces banners per session but leaves its other
    /// requests eligible for the next update.
    fn run_input_notifications(&mut self, cx: &mut Context<Self>) {
        let sessions = Engine::sessions(cx);
        let visible = self.focus.visible_session_id().map(str::to_string);
        let mut banners = Vec::new();
        {
            let all = sessions.read(cx).all();
            let pending = pending_input_notifications(all);
            self.notified_input
                .retain(|key| pending.iter().any(|entry| entry.key == *key));
            let mut notified_sessions = HashSet::new();
            for entry in pending {
                if self.notified_input.contains(&entry.key)
                    || notified_sessions.contains(&entry.session_id)
                {
                    continue;
                }
                notified_sessions.insert(entry.session_id.clone());
                self.notified_input.insert(entry.key.clone());
                if let Some(session) = all.iter().find(|session| session.id == entry.session_id) {
                    banners.push((
                        session.clone(),
                        NotificationEvent::Input {
                            kind: entry.kind,
                            request_id: entry.request_id,
                        },
                    ));
                }
            }
        }
        for (session, event) in banners {
            let visible = visible.as_deref() == Some(session.id.as_str());
            self.notify_session(&session, event, visible, cx).detach();
        }
    }

    /// The notification subject for a session event, or `None` when the
    /// session has no project identity or is an Inbox Ask.
    fn subject_for(
        &self,
        session: &Session,
        event: NotificationEvent,
    ) -> Option<NotificationSubject> {
        if session.inbox_ask.is_some() {
            return None;
        }
        let occurred_at = self.now();
        let project = known_notification_project(&self.kv, &session.cwd)?;
        Some(NotificationSubject {
            project_id: project.id,
            category: if event == NotificationEvent::Finished {
                NotificationCategory::AgentFinished
            } else {
                NotificationCategory::AgentInput
            },
            occurred_at: Some(occurred_at),
        })
    }

    /// `notifySession`: sends the banner when policy allows. Resolves `true`
    /// once the OS accepted it, so callers can skip the in-app cue.
    pub fn notify_session(
        &mut self,
        session: &Session,
        event: NotificationEvent,
        session_visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        match self.subject_for(session, event) {
            Some(subject) => {
                self.notify_project_session(session, event, session_visible, &subject, cx)
            }
            None => Task::ready(false),
        }
    }

    /// `announceSessionFinished`: one policy decision covers the OS banner
    /// and its in-app sound fallback.
    pub fn announce_session_finished(
        &mut self,
        session: &Session,
        session_visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(subject) = self.subject_for(session, NotificationEvent::Finished) else {
            return Task::ready(());
        };
        let sent = self.notify_project_session(
            session,
            NotificationEvent::Finished,
            session_visible,
            &subject,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if !sent.await {
                this.update(cx, |this, _| {
                    this.play_cue(SoundCue::TurnFinished, Some(&subject))
                })
                .ok();
            }
        })
    }

    /// The end of a turn, announced on the next tick so the banner quotes the
    /// reply's final text. Reads the session and the visible session then.
    pub fn announce_finished_later(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let session_id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            let announce = this.update(cx, |this, cx| {
                let session = Engine::sessions(cx).read(cx).get(&session_id).cloned()?;
                let visible = this.focus.visible_session_id() == Some(session_id.as_str());
                Some(this.announce_session_finished(&session, visible, cx))
            });
            if let Ok(Some(announce)) = announce {
                announce.await;
            }
        })
        .detach();
    }

    fn notify_project_session(
        &mut self,
        session: &Session,
        event: NotificationEvent,
        session_visible: bool,
        subject: &NotificationSubject,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if !allows_project_notification(&self.kv, subject, self.now()) {
            return Task::ready(false);
        }
        let decision = should_notify(
            load_notifications_enabled(&self.kv),
            self.permission,
            self.window_focused,
            session_visible,
        );
        if !decision {
            return Task::ready(false);
        }
        let text = notification_text(session, event);
        let sound = load_sounds_enabled(&self.kv);
        let platform = self.platform.clone();
        let session_id = session.id.clone();
        cx.background_spawn(async move {
            platform
                .show_notification(&session_id, &text, sound)
                .is_ok()
        })
    }

    /// A banner was clicked (`NOTIFICATION_CLICK_EVENT`). Every window hears
    /// it; only the one holding the session acts.
    pub fn notification_clicked(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if !Engine::sessions(cx).read(cx).contains(session_id) {
            return;
        }
        cx.emit(NotifierEvent::Clicked(session_id.to_string()));
        let session_id = session_id.to_string();
        cx.defer(move |cx| super::approvals::Approvals::open_approval_session(&session_id, cx));
    }

    // Sounds.

    /// `playCue`. Project cues need their subject.
    pub fn play_cue(&mut self, cue: SoundCue, subject: Option<&NotificationSubject>) -> bool {
        if !cue_allowed(&self.kv, cue, subject, self.now()) {
            return false;
        }
        self.platform.play_sound(cue.sound(), SOUNDS_VOLUME);
        true
    }

    /// `announceLinkedActivity`.
    pub fn announce_linked_activity(
        &mut self,
        session_id: &str,
        card: Option<&LinkedWorkItemUpdateCard>,
    ) {
        if let Some(subject) = self.cues.linked_activity_subject(session_id, card) {
            self.play_cue(SoundCue::LinkedActivity, Some(&subject));
        }
    }

    /// `announceUpdateAvailable`.
    pub fn announce_update_available(&mut self, version: Option<&str>) {
        if self.cues.update_available(version) {
            self.play_cue(SoundCue::UpdateAvailable, None);
        }
    }

    /// `resetSoundCues`.
    pub fn reset_sound_cues(&mut self) {
        self.cues.reset();
    }

    // Store changes.

    fn kv_changed(&mut self, key: &str, cx: &mut Context<Self>) {
        match key {
            PROJECT_NOTIFICATIONS_KEY => {
                self.schedule_mute_expiry(cx);
                cx.emit(NotifierEvent::PreferencesChanged);
            }
            NOTIFICATION_PROJECTS_KEY => cx.emit(NotifierEvent::ProjectsChanged),
            _ => cx.emit(NotifierEvent::SettingsChanged),
        }
        cx.notify();
    }

    /// The schedule in `subscribeNotificationPreferences`: tell listeners
    /// when the next timed mute expires.
    fn schedule_mute_expiry(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
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
                cx.emit(NotifierEvent::PreferencesChanged);
                cx.notify();
            })
            .ok();
        }));
    }
}
