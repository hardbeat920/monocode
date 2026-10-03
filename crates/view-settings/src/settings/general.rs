//! Port of `GeneralPage`, `UpdateRow`, and `NotificationsBlocked` in
//! SettingsView.tsx.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
};
use monocode_core::settings::FileTabMode;
use monocode_core::shortcut::quick_composer_shortcut_label;
use monocode_settings::settings_store as ss;
use monocode_ui::{IconName, Theme, UiStyled as _, u};

use super::chrome::{group, row};
use super::controls::{Leading, secondary_button, segmented, toggle};
use super::host::{NotificationPermission, UpdatePhase, UpdaterSnapshot};
use super::section::SectionContext;
use super::store;

type WhatsNewHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct GeneralSection {
    ctx: SectionContext,
    sounds_enabled: bool,
    notifications_enabled: bool,
    notification_permission: NotificationPermission,
    notes_enabled: bool,
    live_agents_enabled: bool,
    file_tab_mode: FileTabMode,
    tab_animations_enabled: bool,
    close_to_tray: bool,
    quick_composer_enabled: bool,
    quick_composer_error: Option<String>,
    snapshot: UpdaterSnapshot,
    on_open_whats_new: Option<WhatsNewHandler>,
    jobs: Vec<Task<()>>,
    _activation: Option<Subscription>,
}

impl GeneralSection {
    pub fn new(
        ctx: SectionContext,
        on_open_whats_new: Option<WhatsNewHandler>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kv = &ctx.kv;
        let platform = ctx.platform;
        let notification_permission = ctx.hosts.general.cached_notification_permission(cx);
        // The user may flip the switch in System Settings and come back:
        // re-read the OS state whenever the window regains focus.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.notifications_enabled {
                this.refresh_permission(cx);
            }
        });
        let mut this = Self {
            sounds_enabled: store::load_sounds_enabled(kv),
            notifications_enabled: store::load_notifications_enabled(kv),
            notification_permission,
            notes_enabled: ss::load_notes_enabled(kv),
            live_agents_enabled: ss::load_live_agents_enabled(kv),
            file_tab_mode: ss::load_file_tab_mode(kv),
            tab_animations_enabled: ss::load_tab_animations_enabled(kv),
            close_to_tray: ss::load_close_to_tray(kv, platform),
            quick_composer_enabled: ss::load_quick_composer_enabled(kv),
            quick_composer_error: None,
            snapshot: UpdaterSnapshot::default(),
            on_open_whats_new,
            jobs: Vec::new(),
            _activation: Some(activation),
            ctx,
        };
        if this.notifications_enabled {
            this.refresh_permission(cx);
        }
        let version = this.ctx.hosts.general.app_version(cx);
        this.jobs.push(cx.spawn(async move |this, cx| {
            let current_version = version.await;
            this.update(cx, |this, cx| {
                this.snapshot.current_version = current_version;
                cx.notify();
            })
            .ok();
        }));
        this
    }

    pub fn snapshot(&self) -> &UpdaterSnapshot {
        &self.snapshot
    }

    pub fn quick_composer_error(&self) -> Option<&str> {
        self.quick_composer_error.as_deref()
    }

    fn refresh_permission(&mut self, cx: &mut Context<Self>) {
        let probe = self.ctx.hosts.general.probe_notification_permission(cx);
        self.jobs.push(cx.spawn(async move |this, cx| {
            let permission = probe.await;
            this.update(cx, |this, cx| {
                this.notification_permission = permission;
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn on_sounds_enabled(&mut self, next: bool, cx: &mut Context<Self>) {
        store::save_sounds_enabled(&self.ctx.kv, next);
        self.sounds_enabled = next;
        cx.notify();
    }

    pub fn on_notifications_enabled(&mut self, next: bool, cx: &mut Context<Self>) {
        store::save_notifications_enabled(&self.ctx.kv, next);
        self.notifications_enabled = next;
        cx.notify();
        if !next {
            return;
        }
        let request = self.ctx.hosts.general.request_notification_permission(cx);
        self.jobs.push(cx.spawn(async move |this, cx| {
            let permission = request.await;
            this.update(cx, |this, cx| {
                this.notification_permission = permission;
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn on_notes_enabled(&mut self, next: bool, cx: &mut Context<Self>) {
        ss::save_notes_enabled(&self.ctx.kv, next);
        self.notes_enabled = next;
        cx.notify();
    }

    /// `onQuickComposerEnabled`: save the switch, then register or drop the
    /// hotkey. A failure leaves the switch where the user put it, so the next
    /// launch tries again, and says why it is dead.
    pub fn on_quick_composer_enabled(&mut self, next: bool, cx: &mut Context<Self>) {
        ss::save_quick_composer_enabled(&self.ctx.kv, next);
        self.quick_composer_enabled = next;
        self.quick_composer_error = None;
        let registered = self
            .ctx
            .hosts
            .keybindings
            .set_quick_composer_shortcut(next, None, cx);
        self.jobs.push(cx.spawn(async move |this, cx| {
            if let Err(error) = registered.await {
                this.update(cx, |this, cx| {
                    this.quick_composer_error = Some(error);
                    cx.notify();
                })
                .ok();
            }
        }));
        cx.notify();
    }

    pub fn on_live_agents_enabled(&mut self, next: bool, cx: &mut Context<Self>) {
        ss::save_live_agents_enabled(&self.ctx.kv, next);
        self.live_agents_enabled = next;
        cx.notify();
    }

    pub fn on_file_tab_mode(&mut self, next: FileTabMode, cx: &mut Context<Self>) {
        ss::save_file_tab_mode(&self.ctx.kv, next);
        self.file_tab_mode = next;
        cx.notify();
    }

    pub fn on_tab_animations_enabled(&mut self, next: bool, cx: &mut Context<Self>) {
        ss::save_tab_animations_enabled(&self.ctx.kv, next);
        self.tab_animations_enabled = next;
        cx.notify();
    }

    pub fn on_close_to_tray(&mut self, next: bool, cx: &mut Context<Self>) {
        ss::save_close_to_tray(&self.ctx.kv, next);
        self.close_to_tray = next;
        cx.notify();
    }

    /// The update button: download a found update, else check for one.
    pub fn on_update_click(&mut self, cx: &mut Context<Self>) {
        let busy = matches!(
            self.snapshot.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading
        );
        if busy {
            return;
        }
        let this = cx.entity().downgrade();
        let report: super::host::UpdateReporter = Rc::new(move |snapshot, cx| {
            this.update(cx, |this, cx| {
                this.snapshot = snapshot;
                cx.notify();
            })
            .ok();
        });
        if self.snapshot.phase == UpdatePhase::Available {
            self.ctx.hosts.general.install_pending_update(report, cx);
        } else {
            self.ctx.hosts.general.run_update_flow(true, report, cx);
        }
    }

    /// `UpdateRow`'s status line.
    pub fn update_status(&self) -> String {
        let snapshot = &self.snapshot;
        match snapshot.phase {
            UpdatePhase::Available => format!(
                "Version {} is available.",
                snapshot.available_version.as_deref().unwrap_or("")
            ),
            UpdatePhase::Downloading => match snapshot.progress {
                Some(progress) => format!("Downloading {progress}%"),
                None => "Downloading…".into(),
            },
            UpdatePhase::Checking => "Checking for updates…".into(),
            UpdatePhase::Current => "You're on the latest version.".into(),
            UpdatePhase::Error => snapshot
                .error
                .clone()
                .unwrap_or_else(|| "Update check failed.".into()),
            UpdatePhase::Idle => "MonoCode updates itself from the release feed.".into(),
        }
    }

    fn notifications_blocked(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let platform = self.ctx.platform;
        let mut el = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.45))
            .child("Permission needed");
        if platform.is_mac() || platform.is_windows() {
            let hover_fill = theme.content(0.10);
            let hover_ink = theme.colors.content;
            el = el.child(
                div()
                    .id("open-notification-settings")
                    .px(u(8.))
                    .py(u(4.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .text_color(theme.content(0.70))
                    .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                    .debug_selector(|| "button:open-notification-settings".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ctx.hosts.general.open_notification_settings(cx)
                    }))
                    .child("Open System Settings"),
            );
        }
        el.into_any_element()
    }

    fn quiet_text(text: impl Into<SharedString>, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        div()
            .text_px(theme.text.label)
            .text_color(theme.content(0.45))
            .child(text.into())
            .into_any_element()
    }

    fn update_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let reveal = self.ctx.reveal(cx);
        let busy = matches!(
            self.snapshot.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading
        );
        let has_update = self.snapshot.phase == UpdatePhase::Available;
        let version = self.snapshot.current_version.clone();
        let label = div()
            .flex()
            .items_baseline()
            .gap(u(8.))
            .child("Version")
            .child(
                div()
                    .font_family(theme.fonts.mono.clone())
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child(version.clone()),
            );
        let whats_new = {
            let open = self.on_open_whats_new.clone();
            let version = version.clone();
            secondary_button("whats-new", "What's new")
                .disabled(version == "…")
                .on_click(move |_, window, cx| {
                    if let Some(open) = open.clone() {
                        open(version.clone(), window, cx);
                    }
                })
        };
        let leading = if busy {
            Leading::Spinner
        } else if has_update {
            Leading::AccentIcon(IconName::ArrowDownCircle)
        } else {
            Leading::Icon(IconName::RefreshCw)
        };
        let check = secondary_button(
            "check-for-updates",
            if has_update {
                "Download"
            } else {
                "Check for updates"
            },
        )
        .leading(leading)
        .disabled(busy)
        .on_click(cx.listener(|this, _, _, cx| this.on_update_click(cx)));
        row(&reveal, label)
            .id("update")
            .description(self.update_status())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(whats_new)
                    .child(check),
            )
            .into_any_element()
    }
}

impl Render for GeneralSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = self.ctx.reveal(cx);
        let platform = self.ctx.platform;

        let mut notifications = row(&reveal, "Notifications")
            .id("notifications")
            .description("Notify when a reminder is due, or when an agent finishes or needs input in another session or while MonoCode is in the background. Click the notification to open that session.");
        if self.notifications_enabled
            && self.notification_permission == NotificationPermission::Denied
        {
            notifications = notifications.child(self.notifications_blocked(cx));
        }
        if self.notifications_enabled
            && self.notification_permission == NotificationPermission::Unsupported
        {
            notifications =
                notifications.child(Self::quiet_text("Not available on this platform", cx));
        }
        // Only a lone switch stays beside the label on a narrow page.
        let lone = !(self.notifications_enabled
            && matches!(
                self.notification_permission,
                NotificationPermission::Denied | NotificationPermission::Unsupported
            ));
        if lone {
            notifications = notifications.switch_only();
        }
        let notifications = notifications.child(
            toggle("Notifications", self.notifications_enabled).on_change(
                cx.listener(|this, next: &bool, _, cx| this.on_notifications_enabled(*next, cx)),
            ),
        );

        let alerts = group(&reveal, "Alerts")
            .first(true)
            .description("How MonoCode reaches you while you are looking somewhere else.")
            .child(
                row(&reveal, "Sounds")
                    .id("sounds")
                    .description("Short cues for project activity, finished turns, and available updates. Choose project notification categories in Inbox settings. Switches and Copy on a finished turn also play.")
                    .switch_only()
                    .child(toggle("Sounds", self.sounds_enabled).on_change(cx.listener(
                        |this, next: &bool, _, cx| this.on_sounds_enabled(*next, cx),
                    ))),
            )
            .child(notifications);

        let mut workspace = group(&reveal, "Workspace")
            .description("How project navigation and workspace tabs behave.")
            .child(
                row(&reveal, "File tabs")
                    .id("file-tabs")
                    .description("Open files beside the active chat, or give each file a normal tab in the top bar. Top-bar files can still be combined into split panes.")
                    .child(
                        segmented(
                            "File tabs",
                            self.file_tab_mode.as_str(),
                            [("pane", "Beside chat"), ("workspace", "Top bar")],
                        )
                        .on_change(cx.listener(|this, value: &str, _, cx| {
                            this.on_file_tab_mode(FileTabMode::parse(Some(value)), cx)
                        })),
                    ),
            )
            .child(
                row(&reveal, "Tab animations")
                    .id("tab-animations")
                    .description("Animate tabs as they open and close. Turn this off for instant tab changes.")
                    .switch_only()
                    .child(
                        toggle("Tab animations", self.tab_animations_enabled).on_change(
                            cx.listener(|this, next: &bool, _, cx| {
                                this.on_tab_animations_enabled(*next, cx)
                            }),
                        ),
                    ),
            )
            .child(
                row(&reveal, "Notes")
                    .id("notes")
                    .description("A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat.")
                    .switch_only()
                    .child(toggle("Notes", self.notes_enabled).on_change(cx.listener(
                        |this, next: &bool, _, cx| this.on_notes_enabled(*next, cx),
                    ))),
            );
        if platform.is_mac() {
            let label = quick_composer_shortcut_label(
                &ss::load_quick_composer_shortcut(&self.ctx.kv),
                platform,
            );
            let mut quick = row(&reveal, "Quick composer")
                .id("quick-composer")
                .description(format!("Press {label} in any app to float a prompt over it and start a session without switching to MonoCode. Change the shortcut in Keybindings. Return starts it in the background; ⌘Return starts it and brings the session forward."));
            match self.quick_composer_error.clone() {
                Some(error) => quick = quick.child(Self::quiet_text(error, cx)),
                None => quick = quick.switch_only(),
            }
            workspace = workspace.child(quick.child(
                toggle("Quick composer", self.quick_composer_enabled).on_change(cx.listener(
                    |this, next: &bool, _, cx| this.on_quick_composer_enabled(*next, cx),
                )),
            ));
        }
        workspace = workspace.child(
            row(&reveal, "Working agents")
                .id("working-agents")
                .description("When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session.")
                .switch_only()
                .child(
                    toggle("Working agents", self.live_agents_enabled).on_change(cx.listener(
                        |this, next: &bool, _, cx| this.on_live_agents_enabled(*next, cx),
                    )),
                ),
        );
        if platform.is_windows() {
            workspace = workspace.child(
                row(&reveal, "Close to tray")
                    .id("close-to-tray")
                    .description("Closing a window hides it to the system tray instead of quitting, so running agents keep going. Reopen from the tray icon, and quit for real from its menu. Turn this off to have close end the window.")
                    .switch_only()
                    .child(toggle("Close to tray", self.close_to_tray).on_change(cx.listener(
                        |this, next: &bool, _, cx| this.on_close_to_tray(*next, cx),
                    ))),
            );
        }

        let about = group(&reveal, "About").child(self.update_row(cx));
        div()
            .flex()
            .flex_col()
            .child(alerts)
            .child(workspace)
            .child(about)
    }
}
