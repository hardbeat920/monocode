//! Port of src/features/settings/ui/BrowserSettings.tsx: the switch, which
//! browser agents use, the connection, and the threads holding tabs.
use crate::input::{self, Input};
use crate::{
    settings::{
        SettingsView,
        widgets::{note, red, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, Entity, FontWeight, SharedString, Task, div, prelude::*, px, rgb};
use monocode_core::browser::{
    protocol::{
        AccessMode, AuditEntry, BrowserChoice, BrowserKind, BrowserMode, BrowserSettings,
        BrowserState, ConnectionState, Limits, Outcome, TabId, TabOverview, TabState,
    },
    targets::FreeTab,
    view::{self, Action, Tone, tab_host},
};

/// Tailwind's amber-400 and emerald-400.
const AMBER: u32 = 0xfbbf24;
const EMERALD: u32 = 0x34d399;
const NO_FREE_TABS: &str =
    "No tab is free to give. Open a web page in the browser, then click Refresh.";

/// What the Browser page remembers between frames: the free tabs "Give a
/// tab to a thread" loaded, and the thread they go to.
#[derive(Default)]
pub(in crate::settings) struct BrowserPageState {
    free: Option<Vec<FreeTab>>,
    loading: bool,
    give_to: String,
    error: Option<String>,
    _load: Option<Task<()>>,
    /// Activity: the last actions, the thread and outcome shown, and
    /// whether Clear waits for its confirmation.
    audit: Option<Vec<AuditEntry>>,
    audit_session: String,
    audit_outcome: String,
    confirm_clear: bool,
    _audit: Option<Task<()>>,
    /// Advanced → Limits: the fields, and the save a pause in typing makes.
    limits: Vec<(&'static str, Entity<Input>)>,
    _limit_save: Option<Task<()>>,
}

/// The limits Advanced edits: key, label and largest value.
const LIMIT_FIELDS: [(&str, &str, u32); 3] = [
    ("tabs_per_session", "Tabs per thread", 64),
    ("tabs_total", "Tabs in total", 128),
    ("concurrent_actions", "Actions at once", 16),
];

fn limit(limits: &Limits, key: &str) -> u32 {
    match key {
        "tabs_per_session" => limits.tabs_per_session,
        "tabs_total" => limits.tabs_total,
        _ => limits.concurrent_actions,
    }
}

fn set_limit(limits: &mut Limits, key: &str, value: u32) {
    match key {
        "tabs_per_session" => limits.tabs_per_session = value,
        "tabs_total" => limits.tabs_total = value,
        _ => limits.concurrent_actions = value,
    }
}

/// A typed limit within 1 and `max`, or `None` when it is not a number.
fn clamp_limit(raw: &str, max: u32) -> Option<u32> {
    raw.trim()
        .parse::<i64>()
        .ok()
        .map(|v| v.clamp(1, i64::from(max)) as u32)
}

fn outcome_label(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Done => "done",
        Outcome::Blocked => "blocked",
        Outcome::Failed => "failed",
        Outcome::Unknown => "unknown",
    }
}

fn clock_time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Where the user turns remote debugging on for the found or chosen browser.
fn inspect_pages(state: &BrowserState) -> Vec<&'static str> {
    const CHROME: &str = "chrome://inspect/#remote-debugging";
    const EDGE: &str = "edge://inspect";
    const BRAVE: &str = "brave://inspect/#remote-debugging";
    let page = |kind: BrowserKind| match kind {
        BrowserKind::Edge => EDGE,
        BrowserKind::Brave => BRAVE,
        _ => CHROME,
    };
    if let Some(browser) = &state.connection.browser {
        return vec![page(browser.kind)];
    }
    match &state.settings.browser {
        BrowserChoice::Channel { channel } => vec![page(view::channel_kind(channel))],
        _ => vec![CHROME, EDGE],
    }
}

fn tone_color(tone: Tone) -> gpui::Hsla {
    match tone {
        Tone::Neutral => content(0.3),
        Tone::Pending | Tone::Warning => rgb(AMBER).into(),
        Tone::Good => rgb(EMERALD).into(),
        Tone::Error => red(1.),
    }
}

fn mono_chip(text: &'static str) -> AnyElement {
    div()
        .rounded(px(4.))
        .bg(content(0.08))
        .px(px(6.))
        .py(px(2.))
        .font_family(crate::fonts::mono())
        .text_size(px(11.))
        .text_color(content(1.))
        .child(text)
        .into_any_element()
}

/// A numbered list of steps under a group's rows, then notes (`true`:
/// muted).
fn steps(
    intro: Option<&'static str>,
    items: Vec<AnyElement>,
    notes: Vec<(&'static str, bool)>,
) -> AnyElement {
    div()
        .border_t_1()
        .border_color(content(0.05))
        .px_4()
        .py(px(14.))
        .text_size(px(12.))
        .line_height(px(19.5))
        .text_color(content(0.65))
        .when_some(intro, |d, intro| {
            d.child(div().mb_2().text_color(content(0.85)).child(intro))
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(items.into_iter().enumerate().map(|(ix, item)| {
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_shrink_0().child(format!("{}.", ix + 1)))
                        .child(div().min_w_0().flex_1().child(item))
                })),
        )
        .children(notes.into_iter().map(|(text, muted)| {
            div()
                .mt_2()
                .when(muted, |d| d.text_color(content(0.45)))
                .child(text)
        }))
        .into_any_element()
}

/// "Turn on remote debugging once in chrome://inspect or edge://inspect."
fn inspect_step(state: &BrowserState, before: &'static str, after: &'static str) -> AnyElement {
    let mut line = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(before);
    for (ix, page) in inspect_pages(state).into_iter().enumerate() {
        if ix > 0 {
            line = line.child("or");
        }
        line = line.child(mono_chip(page));
    }
    line.child(after).into_any_element()
}

fn text(value: &'static str) -> AnyElement {
    div().child(value).into_any_element()
}

impl SettingsView {
    pub(super) fn browser_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(state) = crate::browser::state(cx) else {
            return vec![self.group(
                Some("browser"),
                true,
                "Browser access",
                None,
                None,
                vec![note(
                    "Browser access could not start in this window. Restart MonoCode to try again.",
                )],
            )];
        };
        let error = crate::browser::get(cx)
            .and_then(|browser| browser.read(cx).error.clone())
            .or_else(|| self.browser.error.clone());
        let mut page = Vec::new();
        if let Some(error) = error {
            page.push(
                div()
                    .mb_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(red(0.3))
                    .bg(red(0.05))
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(red(1.))
                    .child(error)
                    .into_any_element(),
            );
        }
        page.push(self.browser_access_group(&state, cx));
        page.push(self.browser_connection_group(&state, cx));
        page.push(self.browser_sessions_group(&state, cx));
        page.push(self.browser_audit_group(&state, cx));
        page.push(self.browser_advanced_group(&state, cx));
        page
    }

    /// Save a change to the browser's settings.
    fn save_browser(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut BrowserSettings)) {
        let Some(mut settings) = crate::browser::state(cx).map(|state| state.settings) else {
            return;
        };
        change(&mut settings);
        crate::browser::command(cx, move |host| {
            host.set_settings(settings);
            Ok(())
        });
    }

    fn browser_access_group(&mut self, state: &BrowserState, cx: &mut Context<Self>) -> AnyElement {
        let settings = &state.settings;
        let managed = settings.mode.launches();
        let private = settings.mode == BrowserMode::Private;
        let mut rows = vec![
            self.row(
                Some("browser-toggle"),
                true,
                "Let agents use my browser",
                Some("MonoCode remembers this switch, and nothing starts until an agent needs the browser. Turning it off stops every browser action, takes access away from every thread and leaves the agents' tabs open.".into()),
                vec![self.toggle(
                    "browser-toggle",
                    "Let agents use my browser",
                    state.toggle_on,
                    false,
                    cx,
                    |_, on, cx| {
                        crate::browser::command(cx, move |host| {
                            host.set_toggle(on);
                            Ok(())
                        })
                    },
                )],
            ),
            self.row(
                Some("browser-mode"),
                false,
                "Browser",
                Some(if private {
                    "Like the MonoCode browser, with a fresh profile every time it starts. Nothing is kept when it closes: no sign-ins, cookies or history."
                } else if managed {
                    "MonoCode starts an installed browser with its own profile. Nothing to set up, and your own browser does not need to run."
                } else {
                    "MonoCode connects to the Chrome, Edge, Brave, Vivaldi, Opera or Arc you run, signed in as you."
                }
                .into()),
                vec![self.select(
                    "browser-mode",
                    "Browser",
                    match settings.mode {
                        BrowserMode::Managed => "managed",
                        BrowserMode::Private => "private",
                        BrowserMode::Attach => "attach",
                    },
                    vec![
                        ("managed".into(), "MonoCode browser".into(), None),
                        ("private".into(), "Private MonoCode browser".into(), None),
                        ("attach".into(), "Your own browser".into(), None),
                    ],
                    None,
                    cx,
                    |this, mode, cx| {
                        let mode = match mode.as_str() {
                            "private" => BrowserMode::Private,
                            "attach" => BrowserMode::Attach,
                            _ => BrowserMode::Managed,
                        };
                        this.save_browser(cx, |settings| settings.mode = mode);
                    },
                )],
            ),
        ];
        if managed {
            rows.push(
                div()
                    .border_t_1()
                    .border_color(content(0.05))
                    .px_4()
                    .py(px(10.))
                    .text_size(px(12.))
                    .line_height(px(19.5))
                    .text_color(content(0.45))
                    .child("Any thread's agent can take a tab you open in the MonoCode browser, or you can hand one to a thread with \"Give a tab to a thread\" below.")
                    .into_any_element(),
            );
        } else {
            rows.push(steps(
                Some("Your own browser needs these steps before agents can use it:"),
                vec![
                    inspect_step(state, "Turn on remote debugging once in", "."),
                    text("Keep the browser running. MonoCode never starts it."),
                    text("Choose Allow when the browser asks. It asks again on every new connection, for example after a browser or MonoCode restart."),
                    text("While MonoCode is connected, the browser shows a bar saying it is controlled by automated software."),
                ],
                vec![
                    ("Agents never see your own tabs. To let a thread use one, hand it over with \"Give a tab to a thread\" below once MonoCode is connected.", false),
                    ("The MonoCode browser needs none of this.", true),
                ],
            ));
        }
        rows.push(self.row(
            Some("browser-access-mode"),
            false,
            "Thread access",
            Some(if settings.access == AccessMode::Always {
                "Every thread can use the browser without asking. Orchestration runs never can. /browser still turns access off or on for one thread."
            } else {
                "The first time an agent needs the browser, its thread asks you. Allow it once and the thread keeps access. /browser turns it on or off ahead of time."
            }
            .into()),
            vec![self.select(
                "browser-access-mode",
                "Thread access",
                if settings.access == AccessMode::Always {
                    "always"
                } else {
                    "ask"
                },
                vec![
                    ("ask".into(), "Ask in the thread".into(), None),
                    ("always".into(), "Always on".into(), None),
                ],
                None,
                cx,
                |this, access, cx| {
                    let access = if access == "always" {
                        AccessMode::Always
                    } else {
                        AccessMode::Ask
                    };
                    this.save_browser(cx, |settings| settings.access = access);
                },
            )],
        ));
        rows.push(self.row(
            Some("browser-tabs-in-monocode"),
            false,
            "Show agent tabs",
            Some(if settings.tabs_in_monocode {
                if managed {
                    "A thread's new tab opens in a pane beside the thread, where you can watch it, click and type. No browser window comes forward for agent tabs; the MonoCode browser runs without one (Open browser still shows it, to sign in to sites)."
                } else {
                    "A thread's new tab opens in a pane beside the thread, where you can watch it, click and type. No browser window comes forward for agent tabs."
                }
            } else {
                "Tabs stay in the browser. \"Show in MonoCode\" in the thread's browser menu still opens one beside the thread."
            }
            .into()),
            vec![self.select(
                "browser-tabs-in-monocode",
                "Show agent tabs",
                if settings.tabs_in_monocode {
                    "monocode"
                } else {
                    "browser"
                },
                vec![
                    ("browser".into(), "In the browser window".into(), None),
                    ("monocode".into(), "In MonoCode, beside the thread".into(), None),
                ],
                None,
                cx,
                |this, place, cx| {
                    let inside = place == "monocode";
                    this.save_browser(cx, |settings| settings.tabs_in_monocode = inside);
                },
            )],
        ));
        if managed {
            let mut options: Vec<(String, SharedString, Option<AnyElement>)> =
                vec![("auto".into(), "Automatic".into(), None)];
            options.extend(
                view::managed_channels()
                    .map(|(channel, label)| (channel.to_owned(), label.into(), None)),
            );
            rows.push(self.row(
                Some("browser-choice"),
                false,
                "Which browser to start",
                Some("Automatic starts Chrome, or else the first of Edge, Brave, Vivaldi, Opera and Arc that is installed.".into()),
                vec![self.select(
                    "browser-choice",
                    "Which browser to start",
                    view::managed_choice(&settings.browser),
                    options,
                    None,
                    cx,
                    |this, channel, cx| {
                        let choice = if channel == "auto" {
                            BrowserChoice::Auto
                        } else {
                            BrowserChoice::Channel { channel }
                        };
                        this.save_browser(cx, |settings| settings.browser = choice);
                    },
                )],
            ));
            if !settings.tabs_in_monocode {
                rows.push(self.row(
                    Some("browser-show-window"),
                    false,
                    "Show browser window",
                    Some("Off: the MonoCode browser runs without a window. Agents work the same way, and Open browser still shows one.".into()),
                    vec![self.toggle(
                        "browser-show-window",
                        "Show browser window",
                        settings.show_window,
                        false,
                        cx,
                        |this, show, cx| this.save_browser(cx, |settings| settings.show_window = show),
                    )],
                ));
            }
            if !private {
                rows.push(self.row(
                    Some("browser-open"),
                    false,
                    "Sign in to sites",
                    Some("Opens the MonoCode browser so you can sign in to the sites agents need. It keeps your sign-ins for next time.".into()),
                    vec![secondary_button("browser-open", "Open browser", false, false)
                        .child("Open browser")
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::browser::command(cx, |host| host.open_window())
                        }))
                        .into_any_element()],
                ));
            }
        } else {
            let custom = matches!(settings.browser, BrowserChoice::UserDataDir { .. });
            let value = match &settings.browser {
                BrowserChoice::Auto => "auto".to_owned(),
                BrowserChoice::Channel { channel } => channel.clone(),
                BrowserChoice::UserDataDir { path } => path.clone(),
            };
            let mut options: Vec<(String, SharedString, Option<AnyElement>)> =
                vec![("auto".into(), "Automatic".into(), None)];
            options.extend(
                view::CHANNELS
                    .iter()
                    .map(|(channel, label)| ((*channel).to_owned(), (*label).into(), None)),
            );
            if let BrowserChoice::UserDataDir { path } = &settings.browser {
                options.push((path.clone(), format!("Profile {path}").into(), None));
            }
            options.push(("custom".into(), "Profile folder…".into(), None));
            rows.push(self.row(
                Some("browser-choice"),
                false,
                "Which browser to connect to",
                Some(if custom {
                    "MonoCode connects to the browser that uses this profile folder."
                } else {
                    "Automatic tries Chrome, then Edge, their Beta, Dev and Canary builds, then Brave, Vivaldi, Opera and Arc. Pick a profile folder for Snap, Flatpak or portable installs."
                }
                .into()),
                vec![self.select(
                    "browser-choice",
                    "Which browser to connect to",
                    &value,
                    options,
                    None,
                    cx,
                    move |this, choice, cx| match choice.as_str() {
                        "auto" => this.save_browser(cx, |s| s.browser = BrowserChoice::Auto),
                        "custom" => this.pick_browser_profile(cx),
                        channel if view::CHANNELS.iter().any(|(c, _)| *c == channel) => {
                            let channel = channel.to_owned();
                            this.save_browser(cx, |s| {
                                s.browser = BrowserChoice::Channel { channel }
                            })
                        }
                        _ => {}
                    },
                )],
            ));
            rows.push(self.row(
                Some("browser-separate-window"),
                false,
                "Agent tabs in a separate window",
                Some("Off: agent tabs open in the background of your current window, with ◆ MonoCode in front of their title. On: every agent tab opens in a window of its own.".into()),
                vec![self.toggle(
                    "browser-separate-window",
                    "Agent tabs in a separate window",
                    settings.separate_window,
                    false,
                    cx,
                    |this, on, cx| this.save_browser(cx, |settings| settings.separate_window = on),
                )],
            ));
        }
        self.group(
            Some("browser"),
            true,
            "Browser access",
            Some(if managed {
                "Agents open their own tabs in the MonoCode browser: your installed Chrome, Edge, Brave, Vivaldi, Opera or Arc with a profile of its own. Your own browser and its tabs stay out of their reach."
            } else {
                "Agents open their own tabs in your own browser, next to yours and signed in as you. They never see or touch your own tabs."
            }
            .into()),
            None,
            rows,
        )
    }

    /// "Profile folder…": the user data folder of a Snap, Flatpak or portable
    /// browser.
    fn pick_browser_profile(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose the browser's user data folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            let _ = this.update(cx, |this, cx| {
                this.save_browser(cx, |s| s.browser = BrowserChoice::UserDataDir { path })
            });
        })
        .detach();
    }

    fn browser_connection_group(
        &mut self,
        state: &BrowserState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let connection = view::connection_view(state);
        let action = connection.action.filter(|_| state.toggle_on).map(|action| {
            let label = match action {
                Action::Disconnect => "Disconnect",
                Action::Retry => "Try again",
                Action::Connect => "Connect",
            };
            secondary_button("browser-connect", label, false, false)
                .when(action != Action::Disconnect, |d| {
                    d.child(icon("refresh-cw", 14.))
                })
                .child(label)
                .on_click(cx.listener(move |_, _, _, cx| {
                    crate::browser::command(cx, move |host| {
                        if action == Action::Disconnect {
                            host.disconnect();
                        } else {
                            host.connect();
                        }
                        Ok(())
                    })
                }))
                .into_any_element()
        });
        let label = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .size_2()
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(tone_color(connection.tone)),
            )
            .child(connection.message.clone());
        let mut rows = vec![self.row(
            None,
            true,
            label,
            view::connection_detail(state).map(SharedString::from),
            action.into_iter().collect(),
        )];
        if state.toggle_on && connection.guide {
            rows.push(steps(
                None,
                vec![
                    inspect_step(state, "Open", "in the browser and turn on remote debugging."),
                    text("Click Connect here. The browser asks whether to allow the connection; choose Allow."),
                    text("While MonoCode is connected the browser shows that it is being controlled by automated software. The browser draws that bar and MonoCode can't hide it; it goes away when you disconnect."),
                ],
                vec![(
                    "Agents can open, read and use their own tabs, with your sign-ins. They can't see your tabs, cookies or downloads, and every agent tab carries ◆ MonoCode in its title.",
                    true,
                )],
            ));
        }
        self.group(
            Some("browser-connection"),
            false,
            "Connection",
            Some(if state.settings.mode.launches() {
                "MonoCode starts its browser when an agent needs it and connects to it directly."
            } else {
                "MonoCode never starts your browser. It finds the one you already run and asks it for permission on every new connection."
            }
            .into()),
            None,
            rows,
        )
    }

    /// What Settings calls a thread the browser module knows by its key.
    fn browser_session_name(&self, key: &str) -> String {
        self.ctx
            .browser_names
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "A closed thread".into())
    }

    fn browser_sessions_group(
        &mut self,
        state: &BrowserState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut keys: Vec<&str> = state
            .sessions
            .iter()
            .filter(|session| {
                session.granted
                    || state
                        .tabs
                        .iter()
                        .any(|t| t.session_id == session.session_id)
            })
            .map(|session| session.session_id.as_str())
            .collect();
        for tab in &state.tabs {
            if !keys.contains(&tab.session_id.as_str()) {
                keys.push(&tab.session_id);
            }
        }
        let live_tabs = state
            .tabs
            .iter()
            .filter(|tab| tab.tab.state != TabState::Revoked)
            .count();
        let close_all = (live_tabs > 0).then(|| {
            let label = if live_tabs == 1 {
                "Close the agent tab".to_owned()
            } else {
                format!("Close all {live_tabs} agent tabs")
            };
            secondary_button("browser-close-all", label.clone(), true, false)
                .child(label)
                .on_click(cx.listener(|_, _, _, cx| {
                    crate::browser::command(cx, |host| {
                        host.close_all_tabs();
                        Ok(())
                    })
                }))
                .into_any_element()
        });
        let mut rows = Vec::new();
        if state.toggle_on && state.connection.state == ConnectionState::Connected {
            rows.push(self.browser_give_row(state, cx));
        }
        if keys.is_empty() {
            rows.push(note(if state.settings.access == AccessMode::Always {
                "No thread has used the browser yet."
            } else {
                "No thread has browser access. A thread asks you when its agent needs the browser, or type /browser in it."
            }));
        }
        for (ix, key) in keys.into_iter().enumerate() {
            let access = state.sessions.iter().find(|s| s.session_id == key);
            let tabs: Vec<&TabOverview> =
                state.tabs.iter().filter(|t| t.session_id == key).collect();
            let granted = access.is_some_and(|a| a.granted);
            let mut summary = if granted {
                "Has access".to_owned()
            } else {
                "No access".to_owned()
            };
            summary.push_str(if tabs.len() == 1 { " · 1 tab" } else { " · " });
            if tabs.len() != 1 {
                summary.push_str(&format!("{} tabs", tabs.len()));
            }
            if access.is_some_and(|a| a.turn_active) {
                summary.push_str(" · working");
            }
            let revoke = granted.then(|| {
                let key = key.to_owned();
                secondary_button(
                    SharedString::from(format!("browser-revoke-{key}")),
                    "Revoke",
                    false,
                    false,
                )
                .child("Revoke")
                .on_click(cx.listener(move |_, _, _, cx| {
                    let key = key.clone();
                    crate::browser::command(cx, move |host| {
                        host.revoke(&key);
                        Ok(())
                    })
                }))
            });
            let name = self.browser_session_name(key);
            let first = ix == 0 && rows.is_empty();
            rows.push(
                div()
                    .px_4()
                    .py(px(14.))
                    .when(!first, |d| d.border_t_1().border_color(content(0.05)))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_4()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(content(1.))
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .mt(px(2.))
                                            .text_size(px(12.))
                                            .text_color(content(0.45))
                                            .child(summary),
                                    ),
                            )
                            .children(revoke),
                    )
                    .when(!tabs.is_empty(), |d| {
                        d.child(
                            div().mt_2().flex().flex_col().gap_1().children(
                                tabs.into_iter().map(|tab| self.browser_tab_row(tab, cx)),
                            ),
                        )
                    })
                    .into_any_element(),
            );
        }
        self.group(
            Some("browser-sessions"),
            false,
            "Threads and tabs",
            Some("Threads with browser access and the tabs their agents opened. Revoking keeps the tabs open; taking a tab back leaves it to you.".into()),
            close_all,
            rows,
        )
    }

    fn browser_tab_row(&self, tab: &TabOverview, cx: &mut Context<Self>) -> AnyElement {
        let id = tab.tab.tab.0.clone();
        let title = if tab.tab.title.is_empty() {
            tab_host(&tab.tab.url)
        } else {
            tab.tab.title.clone()
        };
        let state = match tab.tab.state {
            TabState::Attaching => Some("opening"),
            TabState::Closed => Some("closed"),
            TabState::Crashed => Some("crashed"),
            TabState::Revoked => Some("taken back"),
            TabState::Open => tab.tab.busy.then_some("working"),
        };
        let open = matches!(tab.tab.state, TabState::Open | TabState::Attaching);
        let release_id = id.clone();
        let close_id = id.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .bg(content(0.03))
            .px_2()
            .py(px(6.))
            .child(icon("globe", 14.))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(content(0.85))
                            .child(title),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child(match state {
                                Some(state) => format!("{} · {state}", tab_host(&tab.tab.url)),
                                None => tab_host(&tab.tab.url),
                            }),
                    ),
            )
            .when(open, |d| {
                d.child(
                    secondary_button(
                        SharedString::from(format!("browser-release-{id}")),
                        "Take back",
                        false,
                        false,
                    )
                    .child("Take back")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let tab = TabId(release_id.clone());
                        crate::browser::command(cx, move |host| host.release_tab(&tab))
                    })),
                )
            })
            .child(
                secondary_button(
                    SharedString::from(format!("browser-close-{id}")),
                    format!("Close {title}", title = tab_host(&tab.tab.url)),
                    false,
                    false,
                )
                .child(icon("x", 14.))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let tab = TabId(close_id.clone());
                    crate::browser::command(cx, move |host| host.close_tab_by_user(&tab))
                })),
            )
            .into_any_element()
    }

    /// "Give a tab to a thread": the browser's free pages, loaded on request,
    /// and the thread they go to.
    fn browser_give_row(&mut self, state: &BrowserState, cx: &mut Context<Self>) -> AnyElement {
        let managed = state.settings.mode.launches();
        let loading = self.browser.loading;
        let load_label = if self.browser.free.is_some() {
            "Refresh"
        } else {
            "Show my tabs"
        };
        let load = secondary_button("browser-free-tabs", load_label, false, loading)
            .when(self.browser.free.is_some(), |d| {
                d.child(icon("refresh-cw", 14.))
            })
            .child(load_label)
            .when(!loading, |d| {
                d.on_click(cx.listener(|this, _, _, cx| this.load_free_tabs(cx)))
            })
            .into_any_element();
        let mut controls = Vec::new();
        if self.browser.free.is_some() {
            let mut threads: Vec<(String, SharedString, Option<AnyElement>)> =
                vec![(String::new(), "Choose a thread…".into(), None)];
            threads.extend(
                self.ctx
                    .browser_names
                    .iter()
                    .map(|(key, name)| (key.clone(), name.clone().into(), None)),
            );
            controls.push(self.select(
                "browser-give-to",
                "Give tab to",
                &self.browser.give_to.clone(),
                threads,
                Some(200.),
                cx,
                |this, key, cx| {
                    this.browser.give_to = key;
                    cx.notify();
                },
            ));
        }
        controls.push(load);
        let row = self.row(
            Some("browser-give"),
            true,
            "Give a tab to a thread",
            Some(if managed {
                "Pick a tab you opened in the MonoCode browser and the thread that should have it. The thread gets browser access with it."
            } else {
                "Pick one of your own tabs and the thread that should have it. The thread gets browser access, and its agent sees only that tab of yours."
            }
            .into()),
            controls,
        );
        let Some(free) = self.browser.free.clone() else {
            return row;
        };
        let target = (!self.browser.give_to.is_empty()).then(|| self.browser.give_to.clone());
        div()
            .child(row)
            .child(
                div()
                    .px_4()
                    .pb(px(14.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(free.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child(NO_FREE_TABS),
                        )
                    })
                    .children(free.into_iter().map(|tab| {
                        let target = target.clone();
                        let host = tab_host(&tab.url);
                        let title = if tab.title.is_empty() {
                            host.clone()
                        } else {
                            tab.title.clone()
                        };
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_md()
                            .bg(content(0.03))
                            .px_2()
                            .py(px(6.))
                            .child(icon("globe", 14.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.))
                                            .text_color(content(0.85))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(11.))
                                            .text_color(content(0.45))
                                            .child(host),
                                    ),
                            )
                            .child(
                                secondary_button(
                                    SharedString::from(format!("browser-give-{}", tab.id)),
                                    "Give",
                                    false,
                                    target.is_none(),
                                )
                                .child("Give")
                                .when_some(target, |d, session| {
                                    let free = tab.id.clone();
                                    d.on_click(cx.listener(move |this, _, _, cx| {
                                        this.give_free_tab(free.clone(), session.clone(), cx)
                                    }))
                                }),
                            )
                    })),
            )
            .into_any_element()
    }

    fn load_free_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(task) = crate::browser::query(cx, |host| host.user_tabs()) else {
            return;
        };
        self.browser.loading = true;
        cx.notify();
        self.browser._load = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.browser.loading = false;
                match result {
                    Ok(tabs) => {
                        this.browser.free = Some(tabs);
                        this.browser.error = None;
                    }
                    Err(error) => this.browser.error = Some(error),
                }
                cx.notify();
            });
        }));
    }

    fn give_free_tab(&mut self, free: String, session: String, cx: &mut Context<Self>) {
        let given = free.clone();
        let Some(task) = crate::browser::query(cx, move |host| {
            host.give_tab(&free, &session, crate::browser::WINDOW)
        }) else {
            return;
        };
        self.browser._load = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        if let Some(free) = &mut this.browser.free {
                            free.retain(|tab| tab.id != given);
                        }
                        this.browser.error = None;
                    }
                    Err(error) => this.browser.error = Some(error),
                }
                cx.notify();
            });
        }));
    }
}

impl SettingsView {
    fn load_browser_audit(&mut self, cx: &mut Context<Self>) {
        let session =
            (!self.browser.audit_session.is_empty()).then(|| self.browser.audit_session.clone());
        let Some(task) = crate::browser::query(cx, move |host| {
            Ok(host.audit_list(
                session.as_deref(),
                monocode_core::browser::audit::DEFAULT_LIST_LIMIT,
            ))
        }) else {
            return;
        };
        self.browser._audit = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(entries) => this.browser.audit = Some(entries),
                    Err(error) => this.browser.error = Some(error),
                }
                cx.notify();
            });
        }));
    }

    fn browser_audit_group(&mut self, state: &BrowserState, cx: &mut Context<Self>) -> AnyElement {
        if self.browser.audit.is_none() && self.browser._audit.is_none() {
            self.load_browser_audit(cx);
        }
        let entries = self.browser.audit.clone().unwrap_or_default();
        let outcome = self.browser.audit_outcome.clone();
        let shown: Vec<&AuditEntry> = entries
            .iter()
            .filter(|entry| outcome.is_empty() || outcome_label(entry.outcome) == outcome)
            .collect();
        let mut sessions: Vec<String> = state
            .sessions
            .iter()
            .map(|s| s.session_id.clone())
            .collect();
        for entry in &entries {
            if !sessions.contains(&entry.session_id) {
                sessions.push(entry.session_id.clone());
            }
        }
        let mut thread_options: Vec<(String, SharedString, Option<AnyElement>)> =
            vec![(String::new(), "All threads".into(), None)];
        thread_options.extend(
            sessions
                .iter()
                .map(|key| (key.clone(), self.browser_session_name(key).into(), None)),
        );
        let mut outcome_options: Vec<(String, SharedString, Option<AnyElement>)> =
            vec![(String::new(), "All outcomes".into(), None)];
        outcome_options.extend(
            ["done", "blocked", "failed", "unknown"]
                .into_iter()
                .map(|o| (o.to_owned(), o.into(), None)),
        );
        let actions = if self.browser.confirm_clear {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    secondary_button(
                        "browser-audit-clear-yes",
                        "Delete the activity log",
                        true,
                        false,
                    )
                    .child("Delete the activity log")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.browser.confirm_clear = false;
                        let Some(task) = crate::browser::query(cx, |host| {
                            host.audit_clear();
                            Ok(())
                        }) else {
                            return;
                        };
                        this.browser._audit = Some(cx.spawn(async move |this, cx| {
                            let _ = task.await;
                            let _ = this.update(cx, |this, cx| {
                                this.browser.audit = None;
                                this.browser._audit = None;
                                cx.notify();
                            });
                        }));
                    })),
                )
                .child(
                    secondary_button("browser-audit-clear-no", "Cancel", false, false)
                        .child("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.browser.confirm_clear = false;
                            cx.notify();
                        })),
                )
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    secondary_button("browser-audit-refresh", "Refresh", false, false)
                        .child(icon("refresh-cw", 14.))
                        .child("Refresh")
                        .on_click(cx.listener(|this, _, _, cx| this.load_browser_audit(cx))),
                )
                .child(
                    secondary_button("browser-audit-clear", "Clear", true, entries.is_empty())
                        .child("Clear")
                        .when(!entries.is_empty(), |d| {
                            d.on_click(cx.listener(|this, _, _, cx| {
                                this.browser.confirm_clear = true;
                                cx.notify();
                            }))
                        }),
                )
        };
        let filters = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .py(px(10.))
            .child(self.select(
                "browser-audit-thread",
                "Thread",
                &self.browser.audit_session.clone(),
                thread_options,
                Some(200.),
                cx,
                |this, key, cx| {
                    this.browser.audit_session = key;
                    this.load_browser_audit(cx);
                },
            ))
            .child(self.select(
                "browser-audit-outcome",
                "Outcome",
                &outcome,
                outcome_options,
                Some(140.),
                cx,
                |this, outcome, cx| {
                    this.browser.audit_outcome = outcome;
                    cx.notify();
                },
            ))
            .into_any_element();
        let all_threads = self.browser.audit_session.is_empty();
        let list = if shown.is_empty() {
            note(if self.browser.audit.is_none() {
                "Loading…"
            } else {
                "No browser actions yet."
            })
        } else {
            div()
                .id("browser-audit-list")
                .max_h(px(320.))
                .overflow_y_scroll()
                .border_t_1()
                .border_color(content(0.05))
                .children(shown.into_iter().map(|entry| {
                    let place = match (&entry.url, all_threads) {
                        (Some(url), true) => {
                            format!(
                                "{} · {}",
                                tab_host(url),
                                self.browser_session_name(&entry.session_id)
                            )
                        }
                        (Some(url), false) => tab_host(url),
                        (None, true) => self.browser_session_name(&entry.session_id),
                        (None, false) => String::new(),
                    };
                    let result = match entry.reason {
                        Some(reason) => {
                            format!("{}: {}", outcome_label(entry.outcome), reason.as_str())
                        }
                        None => outcome_label(entry.outcome).to_owned(),
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py(px(6.))
                        .text_size(px(12.))
                        .border_b_1()
                        .border_color(content(0.05))
                        .child(
                            div()
                                .w(px(48.))
                                .flex_shrink_0()
                                .text_color(content(0.45))
                                .child(clock_time(entry.ts_ms)),
                        )
                        .child(
                            div()
                                .w(px(96.))
                                .flex_shrink_0()
                                .truncate()
                                .font_family(crate::fonts::mono())
                                .text_size(px(11.))
                                .text_color(content(1.))
                                .child(entry.action.clone()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(content(0.55))
                                .child(place),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(match entry.outcome {
                                    Outcome::Done => content(0.55),
                                    Outcome::Blocked => rgb(AMBER).into(),
                                    _ => red(1.),
                                })
                                .child(result),
                        )
                        .child(
                            div()
                                .w(px(56.))
                                .flex_shrink_0()
                                .text_right()
                                .text_color(content(0.4))
                                .child(format!("{} ms", entry.duration_ms)),
                        )
                }))
                .into_any_element()
        };
        self.group(
            Some("browser-audit"),
            false,
            "Activity",
            Some("The last 50 browser actions, kept for 30 days. Typed text and page content are never stored, and addresses are kept without their query.".into()),
            Some(actions.into_any_element()),
            vec![filters, list],
        )
    }

    fn browser_limit_fields(&mut self, limits: &Limits, cx: &mut Context<Self>) {
        if !self.browser.limits.is_empty() {
            return;
        }
        for (key, label, max) in LIMIT_FIELDS {
            let value = limit(limits, key);
            let input = cx.new(|cx| {
                let mut input = Input::field(label, "", content(0.35), cx);
                input.set_font_size(12.);
                input.set_text(value.to_string(), cx);
                input
            });
            self._subscriptions.push(cx.subscribe(
                &input,
                move |this, input, _: &input::Changed, cx| {
                    let raw = input.read(cx).content.to_string();
                    let Some(value) = clamp_limit(&raw, max) else {
                        return;
                    };
                    // Saved once typing pauses, not on every key.
                    this.browser._limit_save = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(600))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.save_browser(cx, |settings| {
                                set_limit(&mut settings.limits, key, value)
                            });
                        });
                    }));
                },
            ));
            self.browser.limits.push((key, input));
        }
    }

    fn pick_upload_roots(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Folders agents may upload from".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.save_browser(cx, |settings| {
                    for path in paths {
                        let path = path.to_string_lossy().into_owned();
                        if !settings.upload_roots.contains(&path) {
                            settings.upload_roots.push(path);
                        }
                    }
                })
            });
        })
        .detach();
    }

    fn browser_advanced_group(
        &mut self,
        state: &BrowserState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &state.settings;
        self.browser_limit_fields(&settings.limits, cx);
        let restore =
            secondary_button("browser-restore-defaults", "Restore defaults", false, false)
                .child(icon("rotate-ccw", 14.))
                .child("Restore defaults")
                .on_click(cx.listener(|this, _, _, cx| {
                    // The main switch is not a default to restore; it stays.
                    this.save_browser(cx, |settings| {
                        *settings = BrowserSettings {
                            enabled: settings.enabled,
                            ..BrowserSettings::default()
                        };
                    });
                    let defaults = Limits::default();
                    for (key, input) in this.browser.limits.clone() {
                        let value = limit(&defaults, key);
                        input.update(cx, |input, cx| input.set_text(value.to_string(), cx));
                    }
                }))
                .into_any_element();
        let mut rows = vec![self.row(
            Some("browser-upload-roots"),
            true,
            "Upload folders",
            Some("Agents can upload files from the thread's project folder and from these folders.".into()),
            vec![secondary_button("browser-add-roots", "Add folder…", false, false)
                .child("Add folder…")
                .on_click(cx.listener(|this, _, _, cx| this.pick_upload_roots(cx)))
                .into_any_element()],
        )];
        if !settings.upload_roots.is_empty() {
            rows.push(
                div()
                    .px_4()
                    .pb_3()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(settings.upload_roots.iter().map(|root| {
                        let remove = root.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(11.))
                                    .text_color(content(0.65))
                                    .child(root.clone()),
                            )
                            .child(
                                secondary_button(
                                    SharedString::from(format!("browser-remove-root-{root}")),
                                    format!("Remove {root}"),
                                    false,
                                    false,
                                )
                                .child(icon("x", 12.))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let remove = remove.clone();
                                        this.save_browser(cx, move |settings| {
                                            settings.upload_roots.retain(|entry| *entry != remove)
                                        });
                                    },
                                )),
                            )
                    }))
                    .into_any_element(),
            );
        }
        rows.push(self.row(
            Some("browser-dotfiles"),
            false,
            "Allow hidden files in uploads",
            Some("Files and folders whose name starts with a dot. SSH, cloud and Docker credentials stay blocked either way.".into()),
            vec![self.toggle(
                "browser-dotfiles",
                "Allow hidden files in uploads",
                settings.allow_dotfiles,
                false,
                cx,
                |this, allow, cx| this.save_browser(cx, |settings| settings.allow_dotfiles = allow),
            )],
        ));
        rows.push(self.row(
            Some("browser-mcp"),
            false,
            "Browser tools in providers",
            Some("Each agent gets the browser tools when it starts: as an MCP server for Claude, Codex and the ACP providers, and as a command line for the others. Nothing is written to their settings.".into()),
            Vec::new(),
        ));
        let limit_fields: Vec<AnyElement> = self
            .browser
            .limits
            .iter()
            .zip(LIMIT_FIELDS)
            .map(|((_, input), (_, label, _))| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.55))
                    .child(label)
                    .child(
                        div()
                            .w(px(56.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(content(0.05))
                            .px(px(6.))
                            .child(input.clone()),
                    )
                    .into_any_element()
            })
            .collect();
        rows.push(self.row(
            Some("browser-limits"),
            false,
            "Limits",
            Some("Over a tab limit the agent is asked to close a tab it no longer needs.".into()),
            limit_fields,
        ));
        self.group(
            Some("browser-advanced"),
            false,
            "Advanced",
            None,
            Some(restore),
            rows,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_stay_between_one_and_their_largest_value() {
        assert_eq!(clamp_limit(" 12 ", 64), Some(12));
        assert_eq!(clamp_limit("0", 64), Some(1));
        assert_eq!(clamp_limit("500", 16), Some(16));
        assert_eq!(clamp_limit("-3", 16), Some(1));
        assert_eq!(clamp_limit("ten", 16), None);
        let mut limits = Limits::default();
        set_limit(&mut limits, "tabs_total", 30);
        assert_eq!(limit(&limits, "tabs_total"), 30);
    }
}
