//! GeneralPage and UpdateRow.
use crate::notifications::updater::{self, Action};
use crate::settings::{
    SettingsView,
    widgets::{primary_button, secondary_button},
};
use crate::theme::*;
use gpui::{AnyElement, Context, FontWeight, div, prelude::*, px};

impl SettingsView {
    pub(super) fn general_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let alerts = vec![
            self.row(
                Some("sounds"),
                true,
                "Sounds",
                Some("Play sounds for activity, completed turns, and updates.".into()),
                vec![
                    self.toggle("sounds", "Sounds", p.sounds, false, cx, |this, on, cx| {
                        this.update(cx, |p| p.sounds = on)
                    }),
                ],
            ),
            self.row(
                Some("notifications"),
                false,
                "Notifications",
                Some("Alert when reminders are due or agents finish or need input.".into()),
                {
                    let mut controls = self.notification_permission_controls(cx);
                    controls.push(self.toggle(
                        "notifications",
                        "Notifications",
                        p.notifications,
                        false,
                        cx,
                        |this, on, cx| {
                            this.update(cx, |p| p.notifications = on);
                            if on {
                                this.probe_notification_permission(true, cx);
                            }
                        },
                    ));
                    controls
                },
            ),
        ];
        let mut workspace = vec![
            self.row(
                Some("file-tabs"),
                true,
                "File tabs",
                Some("Choose where files open.".into()),
                vec![self.segmented(
                    "file-tabs",
                    "File tabs",
                    &p.file_tabs,
                    &[("pane", "Beside chat"), ("workspace", "Top bar")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.file_tabs = v.into()),
                )],
            ),
            self.row(
                Some("file-word-wrap"),
                false,
                "Word wrap",
                Some("Wrap long lines to fit the editor.".into()),
                vec![self.toggle(
                    "file-word-wrap",
                    "Word wrap",
                    p.file_word_wrap,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.file_word_wrap = on),
                )],
            ),
            self.row(
                Some("tab-animations"),
                false,
                "Tab animations",
                Some("Animate tabs as they open and close.".into()),
                vec![self.toggle(
                    "tab-animations",
                    "Tab animations",
                    p.tab_animations,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.tab_animations = on),
                )],
            ),
            self.row(
                Some("notes"),
                false,
                "Notes",
                Some("Show Notes in project navigation. Mention saved notes with @note.".into()),
                vec![
                    self.toggle("notes", "Notes", p.notes, false, cx, |this, on, cx| {
                        this.update(cx, |p| p.notes = on)
                    }),
                ],
            ),
        ];
        if cfg!(target_os = "macos") {
            workspace.push(self.row(
                Some("quick-composer"),
                false,
                "Quick composer",
                Some(format!(
                    "Press {} from any app. Return starts a session in the background; ⌘Return opens it. Change the shortcut in Keybindings.",
                    super::keybindings::quick_composer_label(&p)
                ).into()),
                {let mut controls=Vec::new();if let Some(error)=&self.ctx.quick_error{controls.push(super::super::widgets::note(error.clone()));}controls.push(self.toggle(
                    "quick-composer",
                    "Quick composer",
                    p.quick_composer,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.quick_composer = on),
                ));controls},
            ));
        }
        workspace.push(self.row(
            Some("working-agents"),
            false,
            "Working agents",
            Some("Show active chats across projects. Finished chats stay until opened.".into()),
            vec![self.toggle(
                "working-agents",
                "Working agents",
                p.working_agents,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.working_agents = on),
            )],
        ));
        workspace.push(self.row(
            Some("action-opens-terminal"),
            false,
            "Show terminal for actions",
            Some(
                "Open the terminal when an action runs. Off, actions run in the background.".into(),
            ),
            vec![self.toggle(
                "action-opens-terminal",
                "Show terminal for actions",
                p.action_opens_terminal,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.action_opens_terminal = on),
            )],
        ));
        workspace.push(self.row(
            Some("terminal-in-panel"),
            false,
            "Terminal in side panel",
            Some("In the Focused layout, open the terminal as a tab in the right panel instead of below the chat.".into()),
            vec![self.toggle(
                "terminal-in-panel",
                "Terminal in side panel",
                p.terminal_in_panel,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.terminal_in_panel = on),
            )],
        ));
        if cfg!(target_os = "windows") {
            workspace.push(self.row(
                Some("close-to-tray"),
                false,
                "Close to tray",
                Some("Closing a window hides it to the system tray instead of quitting, so running agents keep going. Reopen from the tray icon, and quit for real from its menu. Turn this off to have close end the window.".into()),
                vec![self.toggle(
                    "close-to-tray",
                    "Close to tray",
                    p.close_to_tray,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.close_to_tray = on),
                )],
            ));
        }
        let keep_awake = self.keep_awake_rows(&p, cx);
        let version = crate::notifications::updater::version();
        let about = vec![
            self.row(
                Some("update"),
                true,
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child("Version")
                    .child(
                        div()
                            .font_family(crate::fonts::mono())
                            .text_size(px(12.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(content(0.45))
                            .child(version),
                    ),
                Some(
                    cx.try_global::<updater::Shared>()
                        .and_then(|shared| updater::status(&shared.0, cx))
                        .unwrap_or_else(|| {
                            "View release notes and check for app and agent updates.".into()
                        })
                        .into(),
                ),
                vec![
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            secondary_button("whats-new", "What's new", false, false)
                                .child("What's new")
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(crate::settings::SettingsEvent::WhatsNew)
                                })),
                        )
                        .child(
                            secondary_button(
                                "check-harness-updates",
                                "Check harness updates",
                                false,
                                false,
                            )
                            .child("Check harness updates")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(crate::settings::SettingsEvent::CheckHarnessUpdates)
                            })),
                        )
                        .child(self.app_update_button(cx))
                        .into_any_element(),
                ],
            ),
        ];
        let mut groups = vec![
            self.group(None, true, "Alerts", None, None, alerts),
            self.group(None, false, "Workspace", None, None, workspace),
        ];
        if !keep_awake.is_empty() {
            groups.push(self.group(None, false, "Keep awake", None, None, keep_awake));
        }
        groups.extend(self.hotspot_group(cx));
        groups.push(self.group(None, false, "About", None, None, about));
        groups
    }
}
impl SettingsView {
    /// The footer pill's defaults (macOS only).
    fn keep_awake_rows(
        &mut self,
        p: &crate::settings::Preferences,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !crate::keep_awake::KeepAwake::supported() {
            return Vec::new();
        }
        let k = p.keep_awake.clone();
        let minutes = |options: &[u32], current: u32| {
            let mut values: Vec<u32> = options.to_vec();
            if !values.contains(&current) {
                values.push(current);
                values.sort_unstable();
            }
            values
                .into_iter()
                .map(|m| {
                    let label = if m == 0 {
                        "None".to_string()
                    } else {
                        monocode_core::keep_awake::length(std::time::Duration::from_secs(
                            u64::from(m) * 60,
                        ))
                    };
                    (m.to_string(), label.into(), None)
                })
                .collect::<Vec<_>>()
        };
        vec![
            self.row(
                Some("keep-awake-auto"),
                true,
                "Keep Mac awake while agents work",
                Some("Same as the switch in the footer's coffee pill. Off, the Mac sleeps as usual.".into()),
                vec![self.toggle("keep-awake-auto", "Keep Mac awake while agents work", k.enabled, false, cx, |this, on, cx| {
                    this.update(cx, |p| p.keep_awake.enabled = on)
                })],
            ),
            self.row(
                Some("keep-awake-grace"),
                false,
                "Wait after agents finish",
                Some("Room for a follow-up before the Mac can sleep.".into()),
                vec![self.select(
                    "keep-awake-grace",
                    "Wait after agents finish",
                    &k.grace_minutes.to_string(),
                    minutes(&[0, 5, 15, 30, 60], k.grace_minutes),
                    Some(120.),
                    cx,
                    |this, v, cx| {
                        if let Ok(v) = v.parse() {
                            this.update(cx, |p| p.keep_awake.grace_minutes = v)
                        }
                    },
                )],
            ),
            self.row(
                Some("keep-awake-length"),
                false,
                "Default length",
                Some("Used for \"For a time\".".into()),
                vec![self.select(
                    "keep-awake-length",
                    "Default length",
                    &k.timer_minutes.to_string(),
                    minutes(&[30, 60, 120, 240, 480], k.timer_minutes),
                    Some(120.),
                    cx,
                    |this, v, cx| {
                        if let Ok(v) = v.parse() {
                            this.update(cx, |p| p.keep_awake.timer_minutes = v)
                        }
                    },
                )],
            ),
            self.row(
                Some("keep-awake-display"),
                false,
                "Keep the display on",
                Some("Off, the screen still dims and sleeps on its usual schedule.".into()),
                vec![self.toggle("keep-awake-display", "Keep the display on", k.display, false, cx, |this, on, cx| {
                    this.update(cx, |p| p.keep_awake.display = on)
                })],
            ),
            self.row(
                Some("keep-awake-lid"),
                false,
                "Even with the lid closed",
                Some("Turns off lid-close sleep while it holds and back on when done. Asks for your admin password once.".into()),
                vec![self.toggle("keep-awake-lid", "Even with the lid closed", k.lid, false, cx, |_, on, cx| {
                    cx.emit(crate::settings::SettingsEvent::KeepAwakeLid(on))
                })],
            ),
            self.row(
                Some("keep-awake-lid-screen"),
                false,
                "When the lid closes",
                Some("The built-in screen can stay lit under a closed lid.".into()),
                vec![self.segmented(
                    "keep-awake-lid-screen",
                    "When the lid closes",
                    &k.lid_screen,
                    &[("off", "Screen off"), ("dim", "Dim"), ("leave", "Leave on")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.keep_awake.lid_screen = v.into()),
                )],
            ),
            self.row(
                Some("keep-awake-lid-battery"),
                false,
                "Lid closed on battery too",
                Some("Off, a closed lid only stays awake while plugged in.".into()),
                vec![self.toggle("keep-awake-lid-battery", "Lid closed on battery too", k.lid_on_battery, false, cx, |this, on, cx| {
                    this.update(cx, |p| p.keep_awake.lid_on_battery = on)
                })],
            ),
            self.row(
                Some("keep-awake-lock"),
                false,
                "Lock when the lid closes",
                Some("Staying awake skips the lock that sleep would do.".into()),
                vec![self.toggle("keep-awake-lock", "Lock when the lid closes", k.lock, false, cx, |this, on, cx| {
                    this.update(cx, |p| p.keep_awake.lock = on)
                })],
            ),
            self.row(
                Some("keep-awake-lid-sound"),
                false,
                "Sound when the lid closes",
                Some("Mute turns it back on when you open the lid. Pause doesn't resume.".into()),
                vec![self.segmented(
                    "keep-awake-lid-sound",
                    "Sound when the lid closes",
                    &k.lid_sound,
                    &[("leave", "Leave"), ("mute", "Mute"), ("pause", "Pause media")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.keep_awake.lid_sound = v.into()),
                )],
            ),
            self.row(
                Some("keep-awake-low-power"),
                false,
                "Low Power Mode on battery",
                Some("While a closed lid stays awake on battery; your usual mode comes back after.".into()),
                vec![self.toggle("keep-awake-low-power", "Low Power Mode on battery", k.low_power, false, cx, |_, on, cx| {
                    cx.emit(crate::settings::SettingsEvent::KeepAwakeLowPower(on))
                })],
            ),
            self.row(
                Some("keep-awake-heat"),
                false,
                "Let it sleep if it gets hot",
                Some("With the lid closed, as in a bag. It picks up again once you wake the Mac.".into()),
                vec![self.toggle("keep-awake-heat", "Let it sleep if it gets hot", k.heat_guard, false, cx, |this, on, cx| {
                    this.update(cx, |p| p.keep_awake.heat_guard = on)
                })],
            ),
            self.row(
                Some("keep-awake-battery"),
                false,
                "Stop below battery",
                Some("On battery, lets the Mac sleep when charge drops to this.".into()),
                vec![self.select(
                    "keep-awake-battery",
                    "Stop below battery",
                    &k.stop_below.to_string(),
                    [(0, "Never"), (10, "10%"), (20, "20%"), (30, "30%")]
                        .into_iter()
                        .map(|(v, l)| (v.to_string(), l.into(), None))
                        .collect(),
                    Some(120.),
                    cx,
                    |this, v, cx| {
                        if let Ok(v) = v.parse() {
                            this.update(cx, |p| p.keep_awake.stop_below = v)
                        }
                    },
                )],
            ),
        ]
    }
    /// Check for updates, or once one is found, install it in place.
    /// Check for updates, or once one is found, install it in place.
    fn app_update_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let updater = cx
            .try_global::<updater::Shared>()
            .map(|shared| shared.0.clone());
        match updater.as_ref().map(|u| updater::action(u, cx)) {
            Some(Action::Install(version)) => {
                let label = format!("Update to {version}");
                let mut ink = shell_fill();
                ink.a = 1.;
                primary_button("check-updates", label.clone(), false)
                    .child(icon("arrow-down-circle", 14.).text_color(ink))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        if let Some(updater) = &updater {
                            updater::install(updater, cx)
                        }
                    })
                    .into_any_element()
            }
            Some(Action::Busy(label)) => {
                secondary_button("check-updates", label.clone(), false, true)
                    .child(icon("loader", 14.).text_color(content(0.7)))
                    .child(label)
                    .into_any_element()
            }
            Some(Action::Check) | None => {
                secondary_button("check-updates", "Check for updates", false, false)
                    .child(icon("refresh-cw", 14.).text_color(content(0.7)))
                    .child("Check for updates")
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(crate::settings::SettingsEvent::CheckAppUpdates)
                    }))
                    .into_any_element()
            }
        }
    }
}
