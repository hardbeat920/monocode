//! Hosts the same notification stack as App.tsx, and routes native clicks.
use super::*;
use crate::notifications::{
    app_update::{self, AppUpdateEvent, InstalledUpdateNotice, WhatsNew, WhatsNewEvent},
    approvals::{self, ApprovalNotices, SessionInputs},
    delivery::{self, Permission, Target},
    errors,
    file_error::{FileActionError, FileActionErrorEvent},
    github_activity::{self, GithubActivity, GithubActivityEvent},
    harness_updates::{HarnessUpdates, HarnessUpdatesEvent},
    layout_notice::{LayoutNotice, LayoutNoticeEvent},
    linked_activity::{Cleanup, LinkedActivity, LinkedActivityEvent},
    platform, reminders,
    updater::{self, Event as UpdaterEvent, Updater},
};
use std::{sync::mpsc::Receiver, time::Duration};

pub(super) struct State {
    pub region: AnchorSlot,
    pub harness: Entity<HarnessUpdates>,
    pub approvals: Entity<ApprovalNotices>,
    pub reminders: Entity<reminders::View>,
    pub layout: Entity<LayoutNotice>,
    pub installed: Entity<InstalledUpdateNotice>,
    pub updater: Entity<Updater>,
    pub file_error: Entity<FileActionError>,
    pub reminder_error: Option<String>,
    pub github: Entity<GithubActivity>,
    linked: HashMap<i64, Entity<LinkedActivity>>,
    linked_cleanup: HashMap<i64, Cleanup>,
    linked_seen_ready: bool,
    linked_last_poll: Option<std::time::Instant>,
    linked_revisions: HashMap<String, i64>,
    linked_primed: bool,
    linked_started_at: u64,
    linked_seen_writes: Option<gpui::Task<()>>,
    pub(super) whats_new: Option<(Entity<WhatsNew>, Subscription, FocusHandle)>,
    permission: Permission,
    permission_probe: Option<Receiver<Permission>>,
    announcements: approvals::InputAnnouncements,
    responding: HashSet<i64>,
    sequence: u64,
    badge: usize,
    sounds_enabled: bool,
    sounds_enabled_at: u64,
    poll: Option<gpui::Task<()>>,
    /// How many notification mute and resume times had passed at the last poll.
    rule_times_passed: usize,
}

impl State {
    pub(super) fn new(workspace: &Workspace, store: Database, cx: &mut Context<Shell>) -> Self {
        Self {
            region: AnchorSlot::default(),
            harness: cx
                .new(|cx| HarnessUpdates::new(workspace.preferences.hidden_providers.clone(), cx)),
            approvals: cx.new(|_| ApprovalNotices::default()),
            reminders: cx.new(|_| reminders::View::default()),
            layout: cx.new(|_| {
                LayoutNotice::new(
                    !workspace.classic_layout,
                    workspace.new_layout_prompt_dismissed,
                )
            }),
            installed: cx.new(|_| InstalledUpdateNotice::new(None)),
            updater: updater::shared(store, cx),
            file_error: cx.new(|_| FileActionError::new()),
            reminder_error: None,
            github: cx.new(|_| GithubActivity::new(HashMap::new())),
            linked: HashMap::new(),
            linked_cleanup: HashMap::new(),
            linked_seen_ready: false,
            linked_last_poll: None,
            linked_revisions: HashMap::new(),
            linked_primed: false,
            linked_started_at: monocode_core::time::now_ms(),
            linked_seen_writes: None,
            whats_new: None,
            permission: Permission::Unsupported,
            permission_probe: None,
            announcements: Default::default(),
            responding: HashSet::new(),
            sequence: 0,
            badge: 0,
            sounds_enabled: workspace.preferences.sounds,
            sounds_enabled_at: 0,
            poll: None,
            rule_times_passed: 0,
        }
    }
}

impl Shell {
    pub(super) fn app_update_footer(&self, cx: &App) -> AnyElement {
        let state = &self.notification_state;
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .when(state.installed.read(cx).is_visible(), |d| {
                d.child(div().mx_2().mt_2().child(state.installed.clone()))
            })
            .when(state.updater.read(cx).actionable(), |d| {
                d.child(
                    div()
                        .mx_2()
                        .mt_2()
                        .child(updater::row(state.updater.clone(), cx)),
                )
            })
            .into_any_element()
    }
    pub(super) fn refresh_notification_permission(&mut self) {
        self.notification_state.permission_probe = Some(platform::permission());
    }

    /// Call from SessionChanged, so a turn beginning and ending between render
    /// frames still produces exactly one completion. Render sync shares the set.
    pub(super) fn notification_session_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        self.process_notification_inputs(cx);
        let Some(view) = self.sessions.get(&id) else {
            return;
        };
        let chat = view.read(cx);
        if chat.responding() {
            self.notification_state.responding.insert(id);
            return;
        }
        if !self.notification_state.responding.remove(&id)
            || chat.busy
            || chat.notification_was_cancelled()
            || chat.summary.archived
            || self
                .monos
                .as_ref()
                .is_some_and(|monos| monos.read(cx).is_mono_session(id))
        {
            return;
        }
        let summary = chat.summary.clone();
        let body = chat.notification_reply_body(cx);
        self.deliver_session_notice(
            &summary,
            "agentFinished",
            &body,
            Some(crate::sound::Cue::Finished),
            cx,
        );
    }

    pub(super) fn bind_notices(&mut self, cx: &mut Context<Self>) {
        self.bind_linked_activity(cx);
        self.observations.push(cx.subscribe(
            &self.notification_state.updater,
            |this, _, event: &UpdaterEvent, cx| {
                if let UpdaterEvent::Error(error) = event {
                    this.error = Some(error.clone());
                }
                this.invalidate_chrome(cx);
                if let Some(settings) = &this.settings {
                    settings.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            },
        ));
        self.observations.push(cx.subscribe(
            &self.notification_state.harness,
            |this, _, event, cx| {
                if let HarnessUpdatesEvent::Updated(kind) = event {
                    for view in this.notification_chats(cx) {
                        if Kind::from_id(&view.read(cx).summary.provider) == *kind {
                            view.update(cx, |view, cx| {
                                view.refresh_models_after_harness_update(cx)
                            });
                        }
                    }
                    this.sync_settings(cx);
                }
                cx.notify();
            },
        ));
        self.observations.push(cx.subscribe(
            &self.notification_state.approvals,
            |this, _, event, cx| match event {
                approvals::Action::OpenSession { session_id } => {
                    if let Ok(id) = session_id.parse() {
                        this.open_notice_target(Target::Session(id), cx);
                    }
                }
                approvals::Action::Decide {
                    session_id,
                    request_id,
                    decision,
                } => {
                    if let Ok(id) = session_id.parse()
                        && let Some(view) = this.notification_chat(id, cx)
                    {
                        view.update(cx, |view, cx| {
                            view.decide_notification_request(request_id, decision.clone(), cx);
                        });
                    }
                    cx.notify();
                }
            },
        ));
        self.observations.push(cx.subscribe(
            &self.notification_state.layout,
            |this, _, event, cx| {
                if matches!(event, LayoutNoticeEvent::Try) && this.workspace.classic_layout {
                    // Dismisses the notice and picks the default frame.
                    this.toggle_layout(cx);
                } else {
                    this.workspace.new_layout_prompt_dismissed = true;
                    this.persist(cx);
                    cx.notify();
                }
            },
        ));
        self.observations.push(cx.subscribe(
            &self.notification_state.reminders,
            |this, _, event, cx| match event {
                reminders::Action::Open { session_id, due_at } => this.open_notice_target(
                    Target::Reminder {
                        session_id: *session_id,
                        due_at: *due_at,
                    },
                    cx,
                ),
                reminders::Action::Dismiss { session_id, due_at } => {
                    this.clear_notification_reminder(*session_id, *due_at, cx)
                }
                reminders::Action::Snooze {
                    session_id,
                    expected_due_at,
                    due_at,
                } => this.snooze_notification_reminder(*session_id, *expected_due_at, *due_at, cx),
                reminders::Action::Retry => this.reload_notification_reminders(cx),
                reminders::Action::OpenSettings => {
                    cx.spawn(async move |this, cx| {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.open_settings(crate::settings::Section::General, window, cx)
                        });
                    })
                    .detach();
                }
            },
        ));
        self.observations.push(cx.subscribe(
            &self.notification_state.installed,
            |this, _, event, cx| match event {
                AppUpdateEvent::Open(version) => this.open_whats_new(version.clone(), cx),
                AppUpdateEvent::Dismiss => {
                    this.notification_state
                        .installed
                        .update(cx, |notice, cx| notice.set_version(None, cx));
                    this.invalidate_chrome(cx);
                    cx.notify();
                }
            },
        ));
        self.observations.push(cx.subscribe(
            &self.notification_state.file_error,
            |this, _, _: &FileActionErrorEvent, cx| {
                this.notification_state
                    .file_error
                    .update(cx, |notice, cx| notice.set_message(None, cx));
                cx.notify();
            },
        ));
        if let Some(monos) = self.monos.clone() {
            self.observations.push(cx.subscribe(
                &monos,
                |this, _, _: &crate::monos::NotificationChanged, cx| {
                    this.process_notification_inputs(cx);
                    cx.notify()
                },
            ));
            self.observations.push(cx.subscribe(
                &monos,
                |this, _, event: &crate::monos::NotificationFinished, cx| {
                    if !event.was_cancelled && !event.habit && !event.summary.archived {
                        this.deliver_session_notice(
                            &event.summary,
                            "agentFinished",
                            &event.body,
                            Some(crate::sound::Cue::Finished),
                            cx,
                        );
                    }
                },
            ));
        }
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            locked(&store).and_then(|store| app_update::consume_installed_update(&store))
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Some(version)) = work.await {
                let _ = this.update(cx, |this, cx| {
                    this.notification_state
                        .installed
                        .update(cx, |notice, cx| notice.set_version(Some(version), cx));
                    this.invalidate_chrome(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn start_native_notifications(&mut self, cx: &mut Context<Self>) {
        platform::set_dock_badge(0);
        if platform::initialize() {
            self.notification_state.permission = Permission::Prompt;
            self.notification_state.permission_probe = Some(platform::permission());
        }
        self.notification_state.badge = 0;
        self.process_notification_inputs(cx);
        self.notification_state.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |this, cx| {
                        let permission = this
                            .notification_state
                            .permission_probe
                            .as_ref()
                            .and_then(|rx| rx.try_recv().ok());
                        if let Some(permission) = permission {
                            this.notification_state.permission = permission;
                            this.notification_state.permission_probe = None;
                            cx.notify();
                        }
                        for target in platform::drain_clicks() {
                            this.open_notice_target(target, cx);
                        }
                        this.poll_reminders(cx);
                        this.poll_linked_activity(cx);
                        // Scheduled mutes expire without a preference change;
                        // redraw when one does, not on every tick.
                        let passed = this
                            .workspace
                            .preferences
                            .notification_times_passed(monocode_core::time::now_ms());
                        if passed != this.notification_state.rule_times_passed {
                            this.notification_state.rule_times_passed = passed;
                            if !this.notification_state.approvals.read(cx).is_empty()
                                || this
                                    .notification_chats(cx)
                                    .iter()
                                    .any(|v| v.read(cx).needs_approval())
                            {
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn notification_chats(&self, cx: &App) -> Vec<Entity<Conversation>> {
        let mut views: Vec<_> = self.sessions.values().cloned().collect();
        if let Some(monos) = &self.monos {
            views.extend(monos.read(cx).all_notification_chats(cx));
        }
        let mut seen = HashSet::new();
        views.retain(|view| seen.insert(view.read(cx).summary.id));
        views.sort_by_key(|view| view.read(cx).summary.id);
        views
    }

    fn notification_chat(&self, id: i64, cx: &App) -> Option<Entity<Conversation>> {
        self.sessions
            .get(&id)
            .cloned()
            .or_else(|| self.monos.as_ref()?.read(cx).notification_entity(id, cx))
    }

    /// Advance banners and the Dock badge while an occluded window skips render.
    fn process_notification_inputs(&mut self, cx: &mut Context<Self>) {
        let snapshots: Vec<_> = self
            .notification_chats(cx)
            .iter()
            .map(|view| {
                let chat = view.read(cx);
                SessionInputs {
                    session_id: chat.summary.id.to_string(),
                    title: chat.display_title().to_owned(),
                    provider: chat.summary.provider.clone(),
                    harness: Kind::from_id(&chat.summary.provider).label().to_owned(),
                    pending: chat.pending_notification_inputs(cx),
                    inbox_ask: false,
                    orchestration_worker: self.orchestration_worker(chat.summary.id),
                    conversation_focused: false,
                }
            })
            .collect();
        self.notification_state.approvals.update(cx, |notices, _| {
            notices.track_pending(&snapshots);
        });
        let badge = delivery::dock_badge_count(
            snapshots
                .iter()
                .filter_map(|s| Some((s.session_id.parse().ok()?, !s.pending.is_empty()))),
        );
        if badge != self.notification_state.badge {
            self.notification_state.badge = badge;
            platform::set_dock_badge(badge);
        }
        for (session, input) in self.notification_state.announcements.reconcile(&snapshots) {
            if let Ok(id) = session.session_id.parse()
                && let Some(view) = self.notification_chat(id, cx)
            {
                let summary = view.read(cx).summary.clone();
                let body = if input.kind == approvals::InputKind::Approval {
                    format!("Approve: {}", input.label)
                } else {
                    input.label
                };
                self.deliver_session_notice(
                    &summary,
                    "agentInput",
                    &body,
                    Some(crate::sound::Cue::Input),
                    cx,
                );
            }
        }
    }

    fn notice_session_visible(&self, id: i64, cx: &App) -> bool {
        if self.app_view_open() || self.settings.is_some() || self.app_search.is_some() {
            return false;
        }
        if self.mono_active(cx) {
            return self
                .monos
                .as_ref()
                .and_then(|m| m.read(cx).active_notification_session(cx))
                == Some(id);
        }
        self.workspace.active_tab().is_some_and(|tab| {
            self.workspace
                .layout(tab)
                .leaf_ids()
                .into_iter()
                .any(|pane| {
                    self.workspace.session_for_pane(pane) == id
                        && !self.workspace.review_owners.contains_key(&pane)
                })
        })
    }

    pub(super) fn sync_notices(&mut self, window: &Window, cx: &mut Context<Self>) {
        let now = monocode_core::time::now_ms();
        let prefs = self.workspace.preferences.clone();
        if self.notification_state.sounds_enabled != prefs.sounds {
            self.notification_state.sounds_enabled = prefs.sounds;
            if prefs.sounds {
                self.notification_state.sounds_enabled_at = now;
            }
        }
        let views = self.notification_chats(cx);
        let mut snapshots = Vec::new();
        let mut directories = HashMap::new();
        let mut completed = Vec::new();
        let mut live_ids = HashSet::new();
        for view in views {
            let chat = view.read(cx);
            let id = chat.summary.id;
            live_ids.insert(id);
            if chat.responding() {
                self.notification_state.responding.insert(id);
            } else if self.notification_state.responding.remove(&id)
                && !chat.busy
                && !chat.notification_was_cancelled()
                && !chat.summary.archived
                && self.sessions.contains_key(&id)
                && !self
                    .monos
                    .as_ref()
                    .is_some_and(|monos| monos.read(cx).is_mono_session(id))
            {
                completed.push((chat.summary.clone(), chat.notification_reply_body(cx)));
            }
            directories.insert(id.to_string(), chat.summary.cwd.clone());
            let mono_visible = self.mono_active(cx) && self.notice_session_visible(id, cx);
            snapshots.push(SessionInputs {
                session_id: id.to_string(),
                title: chat.display_title().to_owned(),
                provider: chat.summary.provider.clone(),
                harness: Kind::from_id(&chat.summary.provider).label().to_owned(),
                pending: chat.pending_notification_inputs(cx),
                inbox_ask: false,
                orchestration_worker: self.orchestration_worker(id),
                conversation_focused: self.window_active
                    && (self.workspace.active == Some(id) || mono_visible)
                    && self.notice_session_visible(id, cx)
                    && chat.composer_focused(window, cx),
            });
        }
        self.notification_state
            .responding
            .retain(|id| live_ids.contains(id));
        let badge = delivery::dock_badge_count(
            snapshots
                .iter()
                .filter_map(|s| Some((s.session_id.parse().ok()?, !s.pending.is_empty()))),
        );
        if self.notification_state.badge != badge {
            self.notification_state.badge = badge;
            platform::set_dock_badge(badge);
        }
        let announcements = self.notification_state.announcements.reconcile(&snapshots);
        for (session, input) in announcements {
            if let Ok(id) = session.session_id.parse()
                && let Some(view) = self.notification_chat(id, cx)
            {
                let summary = view.read(cx).summary.clone();
                self.deliver_session_notice(
                    &summary,
                    "agentInput",
                    &input.label,
                    Some(crate::sound::Cue::Input),
                    cx,
                );
            }
        }
        for (summary, body) in completed {
            self.deliver_session_notice(
                &summary,
                "agentFinished",
                &body,
                Some(crate::sound::Cue::Finished),
                cx,
            );
        }
        self.notification_state.approvals.update(cx, |notices, cx| {
            notices.sync(
                &snapshots,
                |session, at| {
                    let occurred = at
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        .min(u64::MAX as u128) as u64;
                    directories.get(&session.session_id).is_some_and(|cwd| {
                        prefs.allows_project_notification(cwd, "agentInput", Some(occurred), now)
                    })
                },
                cx,
            )
        });
        let due = self
            .reminders
            .iter()
            .filter(|r| r.due_at <= now)
            .filter_map(|r| {
                let summary = self.session_summaries.get(&r.session_id)?;
                prefs
                    .allows_project_notification(&summary.cwd, "reminders", Some(r.due_at), now)
                    .then(|| reminders::Snapshot {
                        session_id: r.session_id,
                        due_at: r.due_at,
                        title: summary.title.clone(),
                        project: project_name(&summary.cwd),
                        time_label: local_reminder_label(r.due_at),
                    })
            })
            .collect();
        let error = self.notification_state.reminder_error.clone();
        self.notification_state.reminders.update(cx, |notices, cx| {
            notices.sync(due, error, prefs.notifications, cx)
        });
        self.notification_state.layout.update(cx, |notice, cx| {
            notice.set_snapshot(
                !self.workspace.classic_layout,
                self.workspace.new_layout_prompt_dismissed,
                cx,
            )
        });
    }

    pub(super) fn deliver_session_notice(
        &mut self,
        summary: &Session,
        category: &str,
        body: &str,
        cue: Option<crate::sound::Cue>,
        cx: &mut Context<Self>,
    ) {
        self.dispatch_notice(
            summary,
            category,
            Target::Session(summary.id),
            body,
            cue,
            cx,
        );
    }

    pub(super) fn deliver_reminder_notice(&mut self, id: i64, due_at: u64, cx: &mut Context<Self>) {
        if let Some(summary) = self.session_summaries.get(&id).cloned() {
            self.dispatch_notice(
                &summary,
                "reminders",
                Target::Reminder {
                    session_id: id,
                    due_at,
                },
                &summary.title,
                None,
                cx,
            );
        }
    }

    fn dispatch_notice(
        &mut self,
        summary: &Session,
        category: &str,
        target: Target,
        body: &str,
        cue: Option<crate::sound::Cue>,
        cx: &mut Context<Self>,
    ) {
        let now = monocode_core::time::now_ms();
        let occurred = match target {
            Target::Session(_) => now,
            Target::Reminder { due_at, .. } => due_at,
        };
        let prefs = &self.workspace.preferences;
        if !prefs.allows_project_notification(&summary.cwd, category, Some(occurred), now) {
            return;
        }
        let notify = delivery::should_notify(
            prefs.notifications,
            self.notification_state.permission,
            self.window_active,
            category != "reminders" && self.notice_session_visible(summary.id, cx),
        );
        if !notify {
            if prefs.sounds
                && occurred >= self.notification_state.sounds_enabled_at
                && let Some(cue) = cue
            {
                crate::sound::play(cue);
            }
            return;
        }
        self.notification_state.sequence = self.notification_state.sequence.wrapping_add(1);
        let body = if body.trim().is_empty() && category == "agentFinished" {
            format!("{} finished", Kind::from_id(&summary.provider).label())
        } else {
            body.to_owned()
        };
        let receiver = platform::show(platform::Notification {
            tag: target.tag(self.notification_state.sequence),
            subtitle: if category == "reminders" {
                "Session reminder".into()
            } else {
                summary.title.clone()
            },
            body: delivery::clip_body(&body),
            sound: prefs.sounds,
        });
        let directory = summary.cwd.clone();
        let category = category.to_owned();
        let work = cx.background_executor().spawn(async move {
            receiver
                .recv_timeout(Duration::from_secs(5))
                .ok()
                .is_some_and(|result| result.is_ok())
        });
        cx.spawn(async move |this, cx| {
            let accepted = work.await;
            let _ = this.update(cx, |this, _| {
                let now = monocode_core::time::now_ms();
                let prefs = &this.workspace.preferences;
                if !accepted
                    && prefs.sounds
                    && occurred >= this.notification_state.sounds_enabled_at
                    && prefs.allows_project_notification(&directory, &category, Some(occurred), now)
                    && let Some(cue) = cue
                {
                    crate::sound::play(cue);
                }
            });
        })
        .detach();
    }

    fn open_notice_target(&mut self, target: Target, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let _ = this.update_in(cx, |this, window, cx| {
                let id = match target {
                    Target::Session(id) => id,
                    Target::Reminder { session_id, due_at } => {
                        this.clear_notification_reminder(session_id, due_at, cx);
                        session_id
                    }
                };
                this.close_settings(window, cx);
                this.leave_app_views(cx);
                this.leave_app_search(cx);
                if let Some(monos) = this.monos.clone()
                    && monos.update(cx, |m, cx| m.open_notification_session(id, window, cx))
                {
                    window.activate_window();
                    return;
                }
                // Habit runs share their live Conversation with the Mono
                // manager. Adopt that entity so controls answer the running
                // provider, rather than constructing a disconnected history.
                if !this.sessions.contains_key(&id)
                    && let Some(view) = this.notification_chat(id, cx)
                {
                    let summary = view.read(cx).summary.clone();
                    this.session_summaries.insert(id, summary.clone());
                    this.session_marks
                        .insert(id, cx.new(|_| SessionMark::new(summary.provider.clone())));
                    this.tab_marks
                        .insert(id, cx.new(|_| SessionMark::tab(summary.provider.clone())));
                    this.observations.push(cx.subscribe(
                        &view,
                        move |this, _, _: &SessionChanged, cx| {
                            this.refresh_session_mark(id, cx);
                            this.notification_session_changed(id, cx);
                            this.invalidate_chrome(cx);
                            cx.notify();
                        },
                    ));
                    this.sessions.insert(id, view);
                    if !this.order.contains(&id) {
                        this.order.push(id);
                    }
                }
                if this.session_summaries.contains_key(&id) {
                    // `open` normally dismisses today's due reminder. A stale
                    // OS target must preserve a newer occurrence instead.
                    let newer = match target {
                        Target::Reminder { due_at, .. } => this
                            .reminders
                            .iter()
                            .find(|r| r.session_id == id && r.due_at != due_at)
                            .cloned(),
                        _ => None,
                    };
                    if newer.is_some() {
                        this.reminders.retain(|r| r.session_id != id);
                    }
                    this.close_settings(window, cx);
                    this.open(id, window, cx);
                    if let Some(newer) = newer {
                        this.reminders.push(newer);
                    }
                    window.activate_window();
                }
            });
        })
        .detach();
    }

    pub(super) fn clear_notification_reminder(
        &mut self,
        id: i64,
        expected: u64,
        cx: &mut Context<Self>,
    ) {
        let removed = self
            .reminders
            .iter()
            .find(|r| r.session_id == id && r.due_at == expected)
            .cloned();
        self.reminders
            .retain(|r| !(r.session_id == id && r.due_at == expected));
        platform::dismiss(
            &Target::Reminder {
                session_id: id,
                due_at: expected,
            }
            .tag(0),
        );
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx.background_executor().spawn(async move {
                locked(&store).and_then(|s| s.clear_reminders(&[id], Some(expected)))
            });
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Err(error) => {
                        if let Some(removed) = removed
                            && !this.reminders.iter().any(|r| r.session_id == id)
                        {
                            this.reminders.push(removed);
                        }
                        this.notification_state.reminder_error = Some(error);
                    }
                    Ok(()) => this.notification_state.reminder_error = None,
                }
                this.invalidate_chrome(cx);
                cx.notify();
            });
        }));
        self.invalidate_chrome(cx);
        cx.notify();
    }

    fn snooze_notification_reminder(
        &mut self,
        id: i64,
        expected: u64,
        due_at: u64,
        cx: &mut Context<Self>,
    ) {
        if !self
            .reminders
            .iter()
            .any(|r| r.session_id == id && r.due_at == expected)
        {
            return;
        }
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx.background_executor().spawn(async move {
                locked(&store).and_then(|s| s.snooze_reminder_if(id, expected, due_at))
            });
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(_) => {
                    platform::dismiss(
                        &Target::Reminder {
                            session_id: id,
                            due_at: expected,
                        }
                        .tag(0),
                    );
                    this.notification_state.reminder_error = None;
                    this.reload_notification_reminders(cx);
                }
                Err(error) => {
                    this.notification_state.reminder_error = Some(error);
                    cx.notify();
                }
            });
        }));
    }

    fn reload_notification_reminders(&mut self, cx: &mut Context<Self>) {
        let expected_snapshot = self.reminders.clone();
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx
                .background_executor()
                .spawn(async move { locked(&store).and_then(|s| s.reminders()) });
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(reminders) => {
                        // Preserve a newer optimistic schedule/cancel while its
                        // ordered write waits behind this reload operation.
                        if this.reminders == expected_snapshot {
                            this.reminders = reminders;
                        }
                        this.notification_state.reminder_error = None;
                        this.poll_reminders(cx);
                    }
                    Err(error) => this.notification_state.reminder_error = Some(error),
                }
                this.invalidate_chrome(cx);
                cx.notify();
            });
        }));
    }

    pub(super) fn open_whats_new(&mut self, version: String, cx: &mut Context<Self>) {
        let view = cx.new(|_| WhatsNew::new(version.clone()));
        let weak = view.downgrade();
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            locked(&store).and_then(|store| store.setting("monocode.nativeReleaseNotes"))
        });
        cx.spawn(async move |_, cx| {
            if let Ok(Some(raw)) = work.await
                && let Ok(notes) = serde_json::from_str::<serde_json::Value>(&raw)
                && notes["version"].as_str() == Some(version.as_str())
                && let Some(body) = notes["body"].as_str()
            {
                let _ = weak.update(cx, |view, cx| view.set_release_notes(body, cx));
            }
        })
        .detach();
        let events = cx.subscribe(&view, |this, _, _: &WhatsNewEvent, cx| {
            this.notification_state.whats_new = None;
            cx.notify();
        });
        self.notification_state.whats_new = Some((view, events, cx.focus_handle()));
        cx.notify();
    }

    pub(super) fn notices_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.notification_state;
        let mono_error = self.monos.as_ref().and_then(|monos| {
            monos
                .read(cx)
                .error
                .clone()
                .map(|message| (monos.clone(), message))
        });
        let stack = div()
            .id("notification-stack")
            .absolute()
            .top(px(12.))
            .right(px(12.))
            .w(px(360.))
            .max_w(px((self.viewport_width - 24.).max(0.)))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .child(state.region.probe())
            .when_some(self.error.clone(), |stack, message| {
                stack.child(errors::card(
                    "app-error",
                    message,
                    icon_button("dismiss-error", "Dismiss error", "close")
                        .debug_selector(|| "dismiss-error".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.error = None;
                            cx.notify();
                        })),
                ))
            })
            .when_some(mono_error, |stack, (monos, message)| {
                stack.child(errors::card(
                    "mono-error",
                    message,
                    icon_button("mono-error-close", "Dismiss Mono error", "close")
                        .debug_selector(|| "mono-error-close".into())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            monos.update(cx, |monos, cx| monos.dismiss_error(cx));
                        })),
                ))
            })
            .when(state.file_error.read(cx).is_visible(), |stack| {
                stack.child(state.file_error.clone())
            })
            .when(!state.reminders.read(cx).is_empty(), |d| {
                d.child(
                    div()
                        .w(px(320.))
                        .max_w_full()
                        .child(state.reminders.clone()),
                )
            })
            .when(state.harness.read(cx).is_visible(), |d| {
                d.child(div().w(px(340.)).max_w_full().child(state.harness.clone()))
            })
            .when(state.layout.read(cx).is_visible(), |d| {
                d.child(div().w(px(340.)).max_w_full().child(state.layout.clone()))
            })
            .when(!state.approvals.read(cx).is_empty(), |d| {
                d.child(
                    div()
                        .w(px(360.))
                        .max_w_full()
                        .child(state.approvals.clone()),
                )
            });
        // Toasts remain actionable above dialogs, including Mono settings.
        gpui::deferred(stack).with_priority(110).into_any_element()
    }

    pub(super) fn notice_modals(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut elements = Vec::new();
        if let Some((view, _, focus)) = &self.notification_state.whats_new {
            let view = view.clone();
            let focus = focus.clone();
            if !focus.contains_focused(window, cx) {
                window.focus(&focus, cx);
            }
            elements.push(
                div()
                    .id("whats-new-overlay")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::modal::scrim("whats-new-scrim"))
                    .track_focus(&focus)
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            this.notification_state.whats_new = None;
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.notification_state.whats_new = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(view),
                    )
                    .into_any_element(),
            );
        }
        elements
    }
}

impl Shell {
    fn bind_linked_activity(&mut self, cx: &mut Context<Self>) {
        self.observations.push(cx.subscribe(
            &self.notification_state.github,
            |this, _, event, cx| {
                match event {
                    GithubActivityEvent::Acknowledged(seen) => {
                        let seen = seen.clone();
                        let store = this.store.clone();
                        let previous = this.notification_state.linked_seen_writes.take();
                        this.notification_state.linked_seen_writes =
                            Some(cx.spawn(async move |_, cx| {
                                if let Some(previous) = previous {
                                    previous.await;
                                }
                                cx.background_executor()
                                    .spawn(async move {
                                        let _ = locked(&store)
                                            .and_then(|s| github_activity::save_seen(&s, &seen));
                                    })
                                    .await;
                            }));
                    }
                    GithubActivityEvent::Changed => {
                        this.observe_linked_activity(cx);
                        this.sync_linked_activity(cx);
                    }
                }
                cx.notify();
            },
        ));
        let store = self.store.clone();
        let metadata = self.workspace.desktop_preferences_metadata.clone();
        let work = cx.background_executor().spawn(async move {
            locked(&store)
                .and_then(|s| github_activity::load_seen_with_metadata(&s, metadata.as_ref()))
        });
        cx.spawn(async move |this, cx| {
            let seen = work.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.notification_state
                    .github
                    .update(cx, |feed, _| feed.restore_seen(seen));
                this.notification_state.linked_seen_ready = true;
                this.poll_linked_activity(cx);
            });
        })
        .detach();
    }

    fn poll_linked_activity(&mut self, cx: &mut Context<Self>) {
        if self.notification_state.linked_seen_ready
            && self
                .notification_state
                .linked_last_poll
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
            && self.notification_state.github.read(cx).ready_to_poll()
        {
            self.notification_state.linked_last_poll = Some(std::time::Instant::now());
            let mut sessions = self.session_summaries.clone();
            for view in self.notification_chats(cx) {
                let summary = view.read(cx).summary.clone();
                sessions.insert(summary.id, summary);
            }
            self.notification_state.github.update(cx, |feed, cx| {
                feed.poll(sessions.into_values().collect(), cx)
            });
        }
        let pending = self.notification_state.linked_cleanup.clone();
        for (id, action) in pending {
            if self.archiving.contains(&id) || self.deleting.contains(&id) {
                continue;
            }
            let success = match action {
                Cleanup::Archive => self.session_summaries.get(&id).is_some_and(|s| s.archived),
                Cleanup::Delete => !self.session_summaries.contains_key(&id),
            };
            self.notification_state.linked_cleanup.remove(&id);
            if let Some(notice) = self.notification_state.linked.get(&id).cloned() {
                notice.update(cx, |notice, cx| notice.finish_cleanup(success, cx));
            }
            if success {
                self.notification_state
                    .github
                    .update(cx, |feed, cx| feed.acknowledge(id, cx));
            }
        }
    }

    fn linked_activity_allowed(&self, card: &github_activity::Card) -> bool {
        let project = format!(
            "repository:github.com/{}",
            card.item.repo.trim().to_lowercase()
        );
        let category = if card.item.kind == "pr" {
            "pullRequests"
        } else {
            "issues"
        };
        let occurred = u64::try_from(card.updated_at).ok();
        occurred.is_some()
            && self.workspace.preferences.allows_notification_project(
                &project,
                category,
                occurred,
                monocode_core::time::now_ms(),
            )
    }

    fn observe_linked_activity(&mut self, cx: &mut Context<Self>) {
        let cards: Vec<_> = self
            .notification_state
            .github
            .read(cx)
            .cards
            .values()
            .cloned()
            .collect();
        for card in cards {
            let key = format!(
                "{}:{}:{}:{}",
                card.session,
                card.item.repo.to_lowercase(),
                card.item.kind,
                card.item.number
            );
            let previous = self.notification_state.linked_revisions.get(&key).copied();
            self.notification_state
                .linked_revisions
                .insert(key, previous.unwrap_or(0).max(card.updated_at));
            // The first successful feed is a baseline, never a new-activity
            // announcement. Acknowledgment and local-turn cutoffs are applied
            // by the feed before a card enters this collection.
            let newly_changed = self.notification_state.linked_primed
                && previous.is_none_or(|at| card.updated_at > at);
            let occurred = u64::try_from(card.updated_at).unwrap_or(0);
            if newly_changed
                && !card.details_error
                && self.linked_activity_allowed(&card)
                && self.workspace.preferences.sounds
                && occurred >= self.notification_state.sounds_enabled_at
                && occurred >= self.notification_state.linked_started_at
            {
                // React uses a deduplicated chime, without an OS/banner here.
                crate::sound::play(crate::sound::Cue::LinkedActivity);
            }
        }
        self.notification_state.linked_primed = true;
    }

    fn sync_linked_activity(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<_> = self.notification_state.linked.keys().copied().collect();
        for id in ids {
            let card = self
                .notification_state
                .github
                .read(cx)
                .cards
                .get(&id)
                .filter(|card| self.linked_activity_allowed(card))
                .cloned();
            let can_cleanup = self.session_summaries.get(&id).is_some_and(|s| !s.archived);
            if let Some(notice) = self.notification_state.linked.get(&id) {
                notice.update(cx, |notice, cx| {
                    notice.set_snapshot(card, can_cleanup, can_cleanup, cx)
                });
            }
        }
    }

    /// The pane host positions the returned card at top/right 12px, clipped to
    /// the exact session surface, so split chats and Mono sessions stay scoped.
    pub(super) fn linked_notice(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<LinkedActivity>> {
        let card = self
            .notification_state
            .github
            .read(cx)
            .cards
            .get(&id)
            .filter(|card| self.linked_activity_allowed(card))
            .cloned()?;
        let can_cleanup = self.session_summaries.get(&id).is_some_and(|s| !s.archived);
        if let Some(notice) = self.notification_state.linked.get(&id) {
            notice.update(cx, |notice, cx| {
                notice.set_snapshot(Some(card), can_cleanup, can_cleanup, cx)
            });
            return Some(notice.clone());
        }
        let notice = cx.new(|cx| {
            let mut notice = LinkedActivity::new();
            notice.set_snapshot(Some(card), can_cleanup, can_cleanup, cx);
            notice
        });
        self.observations
            .push(cx.subscribe(&notice, |this, _, event, cx| {
                this.handle_linked_activity(event.clone(), cx)
            }));
        self.notification_state.linked.insert(id, notice.clone());
        Some(notice)
    }

    fn handle_linked_activity(&mut self, event: LinkedActivityEvent, cx: &mut Context<Self>) {
        match event {
            LinkedActivityEvent::Acknowledge(id) | LinkedActivityEvent::Dismiss(id) => {
                self.notification_state
                    .github
                    .update(cx, |feed, cx| feed.acknowledge(id, cx));
            }
            LinkedActivityEvent::OpenUrl(url) => cx.open_url(&url),
            LinkedActivityEvent::OpenDiscussion(id) => {
                // A native threaded discussion pane is not implemented yet.
                if let Some(item) = self
                    .session_summaries
                    .get(&id)
                    .and_then(|s| s.work_item.as_ref())
                {
                    cx.open_url(&item.url);
                }
            }
            LinkedActivityEvent::AddToChat(id, prompt) => {
                cx.spawn(async move |this, cx| {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.close_settings(window, cx);
                        let opened_mono = this.monos.clone().is_some_and(|monos| {
                            monos.update(cx, |m, cx| m.open_notification_session(id, window, cx))
                        });
                        if !opened_mono {
                            this.open(id, window, cx);
                        }
                        if let Some(view) = this.notification_chat(id, cx) {
                            let input = view.read(cx).input.clone();
                            if !input.read(cx).is_composing() {
                                input.update(cx, |input, cx| {
                                    let draft = input.content.to_string();
                                    let separator = if draft.is_empty() || draft.ends_with("\n\n") {
                                        ""
                                    } else if draft.ends_with('\n') {
                                        "\n"
                                    } else {
                                        "\n\n"
                                    };
                                    input.set_text(
                                        format!("{draft}{separator}{}\n\n", prompt.trim()),
                                        cx,
                                    );
                                });
                                window.focus(&input.focus_handle(cx), cx);
                            }
                        }
                        window.activate_window();
                        cx.notify();
                    });
                })
                .detach();
            }
            LinkedActivityEvent::Archive(id) => {
                self.toggle_archive(id, cx);
                if self.archiving.contains(&id) {
                    self.notification_state
                        .linked_cleanup
                        .insert(id, Cleanup::Archive);
                } else if let Some(notice) = self.notification_state.linked.get(&id) {
                    notice.update(cx, |notice, cx| notice.finish_cleanup(false, cx));
                }
            }
            LinkedActivityEvent::Delete(id) => {
                cx.spawn(async move |this, cx| {
                    let prompt = this.update_in(cx, |_, window, cx| {
                        window.prompt(
                            gpui::PromptLevel::Warning,
                            "Delete this conversation permanently?",
                            Some("Its messages and saved settings will be removed."),
                            &["Delete", "Cancel"],
                            cx,
                        )
                    });
                    let confirmed = match prompt {
                        Ok(prompt) => matches!(prompt.await, Ok(0)),
                        Err(_) => false,
                    };
                    let _ = this.update_in(cx, |this, window, cx| {
                        if confirmed {
                            this.delete_session(id, window, cx);
                        }
                        if this.deleting.contains(&id) {
                            this.notification_state
                                .linked_cleanup
                                .insert(id, Cleanup::Delete);
                        } else if let Some(notice) = this.notification_state.linked.get(&id) {
                            notice.update(cx, |notice, cx| notice.finish_cleanup(false, cx));
                        }
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    #[gpui::test]
    fn error_cards_share_top_right_stack_and_dismiss_independently(cx: &mut TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let body = visual.debug_bounds("main-pane-frame").unwrap();
        window
            .update(visual, |shell, _, cx| {
                shell.error = Some("Could not save workspace".into());
                shell
                    .notification_state
                    .file_error
                    .update(cx, |notice, cx| {
                        notice.set_message(Some("Could not rename file".into()), cx);
                    });
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        let app = visual.debug_bounds("app-error").unwrap();
        let file = visual.debug_bounds("file-action-error").unwrap();
        let viewport = window.update(visual, |_, w, _| w.viewport_size()).unwrap();
        assert_eq!(app.top(), px(12.));
        assert_eq!(app.right(), viewport.width - px(12.));
        assert_eq!(app.size.width, px(360.));
        assert_eq!(file.size.width, app.size.width);
        assert_eq!(file.right(), app.right());
        assert_eq!(file.top(), app.bottom() + px(8.));
        assert_eq!(visual.debug_bounds("main-pane-frame").unwrap(), body);

        let close = visual.debug_bounds("dismiss-error").unwrap();
        visual.simulate_click(close.center(), Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("app-error").is_none());
        assert_eq!(
            visual.debug_bounds("file-action-error").unwrap().top(),
            px(12.)
        );

        visual.simulate_resize(gpui::size(px(300.), px(800.)));
        visual.run_until_parked();
        visual.refresh().unwrap();
        let file = visual.debug_bounds("file-action-error").unwrap();
        assert_eq!(file.left(), px(12.));
        assert_eq!(file.right(), px(288.));
        let close = visual.debug_bounds("dismiss-file-action-error").unwrap();
        visual.simulate_click(close.center(), Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("file-action-error").is_none());
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.error.is_none());
                assert!(!shell.notification_state.file_error.read(cx).is_visible());
            })
            .unwrap();
    }
}
