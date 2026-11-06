//! Exact project identity and timestamp semantics of notificationPreferences.ts.
use super::{Preferences, SettingsView, widgets::secondary_button};
use crate::{
    input,
    notifications::{
        delivery::{Permission, ProjectRule},
        platform,
    },
    popover::{Align, Anchor, AnchorSlot, Menu, MenuAction, MenuEvent, menu_popover, popover},
    theme::*,
};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
use gpui::{AnyElement, Context, Entity, Role, Subscription, Task, Window, div, prelude::*, px};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

pub(crate) const CATEGORIES: [(&str, &str); 5] = [
    ("pullRequests", "Pull requests / Merge requests"),
    ("issues", "Issues and Linear tasks"),
    ("agentFinished", "Agent finished"),
    ("agentInput", "Agent approvals and questions"),
    ("reminders", "Reminders"),
];
fn path_key(path: &str) -> String {
    let windows =
        path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\") || path.starts_with("//");
    let normalized = if windows || cfg!(target_os = "windows") {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    let normalized = normalized.trim_end_matches('/');
    if normalized.is_empty() {
        return "/".into();
    }
    if windows {
        normalized.to_lowercase()
    } else {
        normalized.into()
    }
}
fn looks_like_project(path: &str) -> bool {
    let key = path_key(path);
    !path.is_empty()
        && key != "/"
        && key != "~"
        && !(key.len() == 2 && key.ends_with(':'))
        && !path.contains(".app/")
        && !path.contains(".app\\")
        && std::env::var("HOME")
            .ok()
            .is_none_or(|home| path_key(&home) != key)
}
#[derive(Clone)]
pub(super) struct Project {
    id: String,
    name: String,
    detail: String,
    kind: String,
}
impl Preferences {
    pub(crate) fn known_notification_project_id(&self, cwd: &str) -> Option<String> {
        if !looks_like_project(cwd) {
            return None;
        }
        let key = path_key(cwd);
        self.notification_projects
            .as_array()
            .into_iter()
            .flatten()
            .find(|project| {
                valid_project(project)
                    && project["paths"].as_array().is_some_and(|paths| {
                        paths
                            .iter()
                            .any(|p| p.as_str().is_some_and(|p| path_key(p) == key))
                    })
            })
            .and_then(|project| project["id"].as_str())
            .map(str::to_owned)
            .or_else(|| Some(format!("local:{key}")))
    }
    /// None means muted until manually resumed. Time is milliseconds since epoch.
    pub(crate) fn mute_notification_project(&mut self, cwd: &str, until: Option<u64>) {
        if let Some(id) = self.known_notification_project_id(cwd) {
            self.mute_projects(
                &[id],
                Some(until.map_or(Value::Null, |at| json!(at))),
                monocode_core::time::now_ms(),
            );
        }
    }
    pub(crate) fn resume_notification_project(&mut self, cwd: &str) {
        if let Some(id) = self.known_notification_project_id(cwd) {
            self.mute_projects(&[id], None, monocode_core::time::now_ms());
        }
    }
    pub(crate) fn allows_project_notification(
        &self,
        cwd: &str,
        category: &str,
        occurred_at: Option<u64>,
        now: u64,
    ) -> bool {
        self.known_notification_project_id(cwd)
            .is_some_and(|id| self.allows_notification_project(&id, category, occurred_at, now))
    }
    pub(crate) fn allows_notification_project(
        &self,
        id: &str,
        category: &str,
        occurred_at: Option<u64>,
        now: u64,
    ) -> bool {
        let rule = ProjectRule::from_preferences(&self.project_notifications, id, category);
        rule.allows(occurred_at.unwrap_or(now), now)
    }
    /// How many of the times notification rules switch at (mutes ending,
    /// resumes, categories enabled) are at or before `now`. A change means
    /// some rule now answers differently.
    pub(crate) fn notification_times_passed(&self, now: u64) -> usize {
        let passed = |value: &Value| value.as_f64().is_some_and(|at| at <= now as f64);
        self.project_notifications
            .as_object()
            .into_iter()
            .flat_map(|map| map.values())
            .map(|p| {
                let enabled = p["enabledAfter"]
                    .as_object()
                    .map_or(0, |map| map.values().filter(|at| passed(at)).count());
                usize::from(passed(&p["mutedUntil"]))
                    + usize::from(passed(&p["resumedAt"]))
                    + enabled
            })
            .sum()
    }
    fn project_muted(&self, id: &str, now: u64) -> bool {
        let p = &self.project_notifications[id];
        p.get("mutedUntil") == Some(&Value::Null)
            || p["mutedUntil"].as_u64().is_some_and(|at| at > now)
    }
    fn mute_projects(&mut self, ids: &[String], until: Option<Value>, now: u64) {
        if !self.project_notifications.is_object() {
            self.project_notifications = json!({});
        }
        for id in ids {
            let p = &mut self.project_notifications[id];
            if !p.is_object() {
                *p = json!({"disabled":[]});
            }
            if let Some(until) = &until {
                p["mutedUntil"] = until.clone();
            } else if p.get("mutedUntil").is_some() {
                p.as_object_mut().unwrap().remove("mutedUntil");
                p["resumedAt"] = json!(now);
            }
        }
    }
    fn set_category(&mut self, id: &str, category: &str, enabled: bool, now: u64) {
        if !self.project_notifications.is_object() {
            self.project_notifications = json!({});
        }
        let p = &mut self.project_notifications[id];
        if !p.is_object() {
            *p = json!({"disabled":[]});
        }
        let mut disabled: Vec<_> = p["disabled"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|id| CATEGORIES.iter().any(|(cat, _)| id == cat))
            .map(str::to_owned)
            .collect();
        let was_disabled = disabled.iter().any(|id| id == category);
        disabled.retain(|id| id != category);
        if !enabled {
            disabled.push(category.to_owned());
        }
        if enabled && was_disabled {
            if !p["enabledAfter"].is_object() {
                p["enabledAfter"] = json!({});
            }
            p["enabledAfter"][category] = json!(now);
        }
        p["disabled"] = json!(disabled);
    }
}
fn valid_project(p: &Value) -> bool {
    ["id", "name", "detail"]
        .iter()
        .all(|key| p[*key].is_string())
        && p["kind"]
            .as_str()
            .is_some_and(|kind| ["repository", "local", "linear", "jira"].contains(&kind))
        && p["paths"]
            .as_array()
            .is_some_and(|paths| paths.iter().all(Value::is_string))
}
struct MuteMenu {
    menu: Entity<Menu>,
    anchor: AnchorSlot,
    _events: Subscription,
}
struct CustomMute {
    ids: Vec<String>,
    input: Entity<input::Input>,
    date: NaiveDate,
    focused_date: NaiveDate,
    focus_pending: bool,
    focus: gpui::FocusHandle,
    anchor: AnchorSlot,
    month: NaiveDate,
    error: Option<String>,
}
pub(super) struct State {
    permission: Permission,
    active: bool,
    _deadline: Option<Task<()>>,
    receiver: Option<Receiver<Permission>>,
    _poll: Option<Task<()>>,
    selecting: bool,
    selected: HashSet<String>,
    expanded: Option<String>,
    menu: Option<MuteMenu>,
    custom: Option<CustomMute>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            permission: Permission::Prompt,
            active: false,
            _deadline: None,
            receiver: None,
            _poll: None,
            selecting: false,
            selected: HashSet::new(),
            expanded: None,
            menu: None,
            custom: None,
        }
    }
}
impl SettingsView {
    pub(super) fn notification_window_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = window.is_window_active();
        if active && !self.notification_state.active && self.prefs.notifications {
            self.probe_notification_permission(false, cx);
        }
        self.notification_state.active = active;
        if let Some(custom) = &mut self.notification_state.custom
            && custom.focus_pending
        {
            custom.focus_pending = false;
            window.focus(&custom.focus, cx);
        }
    }
    pub(super) fn dismiss_notification_transient(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.notification_state.custom.take().is_some()
            || self.notification_state.menu.take().is_some()
        {
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        false
    }
    pub(super) fn notification_deadline(&mut self, cx: &mut Context<Self>) {
        let now = monocode_core::time::now_ms();
        let deadline = self
            .prefs
            .project_notifications
            .as_object()
            .into_iter()
            .flat_map(|map| map.values())
            .filter_map(|p| p["mutedUntil"].as_u64())
            .filter(|at| *at > now)
            .min();
        self.notification_state._deadline = deadline.map(|at| {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(at.saturating_sub(now)))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.notification_deadline(cx);
                    cx.notify();
                });
            })
        });
    }
    pub(super) fn probe_notification_permission(&mut self, request: bool, cx: &mut Context<Self>) {
        self.notification_state.receiver = Some(if request {
            platform::request_permission()
        } else {
            platform::permission()
        });
        self.notification_state._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let done = this
                    .update(cx, |this, cx| {
                        let result = this
                            .notification_state
                            .receiver
                            .as_ref()
                            .map(Receiver::try_recv);
                        match result {
                            Some(Ok(permission)) => {
                                this.notification_state.permission = permission;
                                this.notification_state.receiver = None;
                                cx.notify();
                                true
                            }
                            Some(Err(TryRecvError::Empty)) => false,
                            _ => {
                                this.notification_state.receiver = None;
                                true
                            }
                        }
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        }));
    }
    pub(super) fn notification_permission_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !self.prefs.notifications {
            return Vec::new();
        }
        match self.notification_state.permission {
            Permission::Denied => vec![
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
                    .text_color(content(0.45))
                    .child("Permission needed")
                    .child(
                        secondary_button(
                            "notifications-system-settings",
                            "Open System Settings",
                            false,
                            false,
                        )
                        .child("Open System Settings")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let _ = platform::open_settings();
                            this.probe_notification_permission(false, cx);
                        })),
                    )
                    .into_any_element(),
            ],
            Permission::Unsupported => vec![
                div()
                    .text_size(px(12.))
                    .text_color(content(0.45))
                    .child("Not available on this platform")
                    .into_any_element(),
            ],
            _ => Vec::new(),
        }
    }
    fn notification_projects(&self) -> Vec<Project> {
        let mut projects = Vec::new();
        let mut ids = HashSet::new();
        let paths: Vec<_> = self
            .ctx
            .projects
            .iter()
            .map(|(path, _)| path.as_str())
            .chain(std::iter::once(self.ctx.cwd.as_str()))
            .filter(|path| looks_like_project(path))
            .collect();
        for p in self
            .prefs
            .notification_projects
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| valid_project(p))
        {
            if p["paths"].as_array().is_some_and(|entries| {
                entries.is_empty()
                    || entries.iter().any(|entry| {
                        entry.as_str().is_some_and(|entry| {
                            paths.iter().any(|path| path_key(entry) == path_key(path))
                        })
                    })
            }) {
                let id = p["id"].as_str().unwrap().to_owned();
                ids.insert(id.clone());
                projects.push(Project {
                    id,
                    name: p["name"].as_str().unwrap().into(),
                    detail: p["detail"].as_str().unwrap().into(),
                    kind: p["kind"].as_str().unwrap().into(),
                });
            }
        }
        for path in paths {
            let Some(id) = self.prefs.known_notification_project_id(path) else {
                continue;
            };
            if ids.insert(id.clone()) {
                projects.push(Project {
                    id,
                    name: self
                        .ctx
                        .projects
                        .iter()
                        .find(|(p, _)| path_key(p) == path_key(path))
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).into()),
                    detail: path.into(),
                    kind: "local".into(),
                });
            }
        }
        projects.sort_by_key(|p| p.name.to_lowercase());
        projects
    }
    fn open_notification_mute(
        &mut self,
        ids: Vec<String>,
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = Local::now();
        let mut items = vec![];
        for hours in [1, 4, 8] {
            let at = now + chrono::Duration::hours(hours);
            let day = if at.date_naive() == now.date_naive() {
                ""
            } else {
                "Tomorrow, "
            };
            items.push(
                MenuAction::new(
                    format!("mute:{hours}"),
                    format!(
                        "{hours} {} ({day}{})",
                        if hours == 1 { "hour" } else { "hours" },
                        at.format("%-H:%M")
                    ),
                )
                .into(),
            );
        }
        items.push(MenuAction::new("mute:indefinite", "Until resumed").into());
        items.push(MenuAction::new("mute:custom", "Choose date and time").into());
        let menu = cx.new(|cx| Menu::new("Mute notifications", items, window, cx).width(244.));
        let targets = ids.clone();
        let events = cx.subscribe(&menu, move |this, _, event: &MenuEvent, cx| {
            if let MenuEvent::Picked(id, _) = event {
                let now = monocode_core::time::now_ms();
                if id.as_ref() == "mute:custom" {
                    let initial = if targets.len() == 1 {
                        this.prefs.project_notifications[&targets[0]]["mutedUntil"]
                            .as_u64()
                            .filter(|at| *at > now)
                            .unwrap_or(now + 3_600_000)
                    } else {
                        now + 3_600_000
                    };
                    let rounded = initial.div_ceil(60_000) * 60_000;
                    let text = chrono::DateTime::from_timestamp_millis(rounded as i64)
                        .map(|at| at.with_timezone(&Local).format("%H:%M").to_string())
                        .unwrap_or_default();
                    let field = cx.new(|cx| {
                        let mut field = input::Input::field("Time", "HH:mm", content(0.4), cx);
                        field.set_text(text, cx);
                        field
                    });
                    let date = chrono::DateTime::from_timestamp_millis(rounded as i64)
                        .map(|at| at.with_timezone(&Local).date_naive())
                        .unwrap_or_else(|| Local::now().date_naive());
                    this.notification_state.custom = Some(CustomMute {
                        ids: targets.clone(),
                        input: field,
                        date,
                        focused_date: date,
                        focus_pending: true,
                        focus: cx.focus_handle(),
                        anchor: this
                            .notification_state
                            .menu
                            .as_ref()
                            .map(|menu| menu.anchor.clone())
                            .unwrap_or_default(),
                        month: date.with_day(1).unwrap(),
                        error: None,
                    });
                } else {
                    let until = if id.as_ref() == "mute:indefinite" {
                        Some(Value::Null)
                    } else {
                        id.strip_prefix("mute:")
                            .and_then(|h| h.parse::<u64>().ok())
                            .map(|h| json!(now + h * 3_600_000))
                    };
                    if let Some(until) = until {
                        this.update(cx, |p| p.mute_projects(&targets, Some(until), now));
                    }
                }
            }
            this.notification_state.menu = None;
            cx.notify();
        });
        self.notification_state.menu = Some(MuteMenu {
            menu,
            anchor,
            _events: events,
        });
        cx.notify();
    }
    fn mute_controls(&self, ids: Vec<String>, key: String, cx: &mut Context<Self>) -> AnyElement {
        let now = monocode_core::time::now_ms();
        let muted: Vec<_> = ids
            .iter()
            .filter(|id| self.prefs.project_muted(id, now))
            .collect();
        let status = if muted.is_empty() {
            String::new()
        } else if ids.len() > 1 {
            format!("{} of {} projects muted", muted.len(), ids.len())
        } else {
            match self.prefs.project_notifications[&ids[0]]["mutedUntil"].as_u64() {
                Some(at) => chrono::DateTime::from_timestamp_millis(at as i64)
                    .map(|at| {
                        format!(
                            "Muted until {}",
                            at.with_timezone(&Local).format("%b %-d, %Y, %-H:%M")
                        )
                    })
                    .unwrap_or_default(),
                None => "Muted until resumed".into(),
            }
        };
        let anchor = AnchorSlot::default();
        let trigger = anchor.clone();
        let resume_ids = ids.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .flex_wrap()
            .when(!status.is_empty(), |d| {
                d.child(
                    div()
                        .id("project-notification-accessible-1")
                        .role(Role::Status)
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(status),
                )
            })
            .when(!muted.is_empty(), |d| {
                d.child(
                    secondary_button(
                        format!("resume-{key}"),
                        "Resume notifications",
                        false,
                        false,
                    )
                    .child("Resume notifications")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update(cx, |p| {
                            p.mute_projects(&resume_ids, None, monocode_core::time::now_ms())
                        })
                    })),
                )
            })
            .child(
                secondary_button(
                    format!("mute-{key}"),
                    if muted.is_empty() {
                        "Mute notifications"
                    } else {
                        "Change mute duration"
                    },
                    false,
                    ids.is_empty(),
                )
                .relative()
                .child(anchor.probe())
                .child(icon("bell-off", 14.))
                .child(if muted.is_empty() { "Mute" } else { "Muted" })
                .child(icon("chevron-down", 12.))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_notification_mute(ids.clone(), trigger.clone(), window, cx)
                })),
            )
            .into_any_element()
    }
    pub(super) fn project_notification_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let projects = self.notification_projects();
        let all_ids: Vec<_> = projects.iter().map(|p| p.id.clone()).collect();
        self.notification_state
            .selected
            .retain(|id| all_ids.contains(id));
        let selecting = self.notification_state.selecting;
        let selected: Vec<_> = all_ids
            .iter()
            .filter(|id| self.notification_state.selected.contains(*id))
            .cloned()
            .collect();
        let all_selected = !all_ids.is_empty() && selected.len() == all_ids.len();
        let mut rows = Vec::new();
        if selecting {
            let select_ids = all_ids.clone();
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py(px(14.))
                    .border_b_1()
                    .border_color(content(0.05))
                    .child(
                        secondary_button(
                            "notification-select-all",
                            "Select all projects",
                            false,
                            false,
                        )
                        .child(if selected.is_empty() {
                            "Select all projects".into()
                        } else {
                            format!("{} selected", selected.len())
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if all_selected {
                                this.notification_state.selected.clear();
                            } else {
                                this.notification_state
                                    .selected
                                    .extend(select_ids.iter().cloned());
                            }
                            cx.notify();
                        })),
                    )
                    .when(!selected.is_empty(), |d| {
                        d.child(self.mute_controls(selected.clone(), "selected".into(), cx))
                    })
                    .into_any_element(),
            );
        }
        for (ix, project) in projects.iter().enumerate() {
            let id = project.id.clone();
            let expand_id = id.clone();
            let select_id = id.clone();
            let expanded = self.notification_state.expanded.as_ref() == Some(&id);
            let muted = self.prefs.project_muted(&id, monocode_core::time::now_ms());
            let categories: Vec<_> = CATEGORIES
                .iter()
                .filter(|(cat, _)| match project.kind.as_str() {
                    "linear" | "jira" => *cat == "issues",
                    "local" => !["pullRequests", "issues"].contains(cat),
                    _ => true,
                })
                .collect();
            let enabled_count = categories
                .iter()
                .filter(|(cat, _)| {
                    !self.prefs.project_notifications[&id]["disabled"]
                        .as_array()
                        .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(cat)))
                })
                .count();
            let status = if muted {
                "All notifications paused".into()
            } else if enabled_count == categories.len() {
                "All categories enabled".into()
            } else {
                format!("{enabled_count} of {} enabled", categories.len())
            };
            let mut category_rows = Vec::new();
            if muted {
                category_rows.push(div().id("project-notification-accessible-2").role(Role::Status).py_3().text_size(px(12.)).text_color(content(0.45)).child("Your category choices apply when notifications resume. You can edit them while muted.").into_any_element());
            }
            for (cat, label) in categories {
                let category = (*cat).to_owned();
                let project_id = id.clone();
                let enabled = !self.prefs.project_notifications[&id]["disabled"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(cat)));
                category_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .py_3()
                        .text_size(px(12.))
                        .text_color(content(0.7))
                        .child(*label)
                        .child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "notification-category-{id}-{cat}"
                                )))
                                .role(Role::Switch)
                                .aria_label(format!("{label} for {}", project.name))
                                .aria_toggled(if enabled {
                                    gpui::Toggled::True
                                } else {
                                    gpui::Toggled::False
                                })
                                .cursor_pointer()
                                .w(px(36.))
                                .h(px(20.))
                                .relative()
                                .rounded_full()
                                .bg(if enabled {
                                    accent_color()
                                } else {
                                    content(0.2)
                                })
                                .child(
                                    div()
                                        .absolute()
                                        .top(px(2.))
                                        .left(px(if enabled { 18. } else { 2. }))
                                        .size(px(16.))
                                        .rounded_full()
                                        .bg(gpui::rgb(0xffffff)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.update(cx, |p| {
                                        p.set_category(
                                            &project_id,
                                            &category,
                                            !enabled,
                                            monocode_core::time::now_ms(),
                                        )
                                    })
                                })),
                        )
                        .into_any_element(),
                );
            }
            rows.push(
                div()
                    .id(("notification-project", ix))
                    .border_b_1()
                    .border_color(content(0.05))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_3()
                            .px_4()
                            .py(px(14.))
                            .when(selecting, |d| {
                                d.child(
                                    secondary_button(
                                        format!("select-notification-{id}"),
                                        format!("Select {}", project.name),
                                        false,
                                        false,
                                    )
                                    .child(if self.notification_state.selected.contains(&id) {
                                        "✓"
                                    } else {
                                        "□"
                                    })
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if !this.notification_state.selected.remove(&select_id)
                                            {
                                                this.notification_state
                                                    .selected
                                                    .insert(select_id.clone());
                                            }
                                            cx.notify();
                                        },
                                    )),
                                )
                            })
                            .child(
                                div()
                                    .id(gpui::SharedString::from(format!(
                                        "expand-notification-{id}"
                                    )))
                                    .role(Role::Button)
                                    .aria_label(format!(
                                        "Notification categories for {} ({})",
                                        project.name, project.detail
                                    ))
                                    .cursor_pointer()
                                    .flex_1()
                                    .min_w(px(180.))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(icon("folder", 16.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(13.))
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .child(project.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .mt_1()
                                                    .text_size(px(12.))
                                                    .text_color(content(0.45))
                                                    .child(format!(
                                                        "{}{status}",
                                                        if project.kind == "local" {
                                                            "Local project · "
                                                        } else {
                                                            ""
                                                        }
                                                    )),
                                            ),
                                    )
                                    .child(icon(
                                        if expanded {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        14.,
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.notification_state.expanded = if expanded {
                                            None
                                        } else {
                                            Some(expand_id.clone())
                                        };
                                        cx.notify();
                                    })),
                            )
                            .child(self.mute_controls(vec![id], format!("project-{ix}"), cx)),
                    )
                    .when(expanded, |d| {
                        d.child(
                            div()
                                .border_t_1()
                                .border_color(content(0.05))
                                .px_4()
                                .children(category_rows),
                        )
                    })
                    .into_any_element(),
            );
        }
        if projects.is_empty() {
            rows.push(div().id("project-notification-accessible-3").role(Role::Status).px_4().py(px(14.)).text_size(px(12.)).text_color(content(0.45)).child("Open a project or connect an Inbox provider to configure its notifications.").into_any_element());
        }
        self.group(Some("project-notifications"),true,"Project notifications",Some("Choose sounds, banners and sidebar indicators by category. Mute pauses them without changing your choices. Unread items stay marked in Inbox.".into()),
            Some(secondary_button("notification-select-projects",if selecting {"Done"} else {"Select projects"},false,projects.is_empty()).child(if selecting {"Done"} else {"Select projects"})
                .on_click(cx.listener(|this,_,_,cx| {this.notification_state.selecting = !this.notification_state.selecting;this.notification_state.selected.clear();cx.notify();})).into_any_element()),rows)
    }
    pub(super) fn notification_overlay(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut overlays = Vec::new();
        if let Some(menu) = &self.notification_state.menu {
            overlays.push(
                menu_popover(&menu.menu, Anchor::Slot(menu.anchor.clone()), cx)
                    .gap(4.)
                    .into_any_element(),
            );
        }
        if let Some(custom) = &self.notification_state.custom {
            overlays.push(
                popover(
                    "notification-custom-mute",
                    Anchor::Slot(custom.anchor.clone()),
                )
                .align(Align::End)
                .width(280.)
                .gap(6.)
                .on_dismiss({
                    let weak = cx.weak_entity();
                    move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.notification_state.custom = None;
                            cx.notify();
                        });
                    }
                })
                .child(
                    div()
                        .id("project-notification-accessible-4")
                        .role(Role::Dialog)
                        .aria_label("Mute project notifications")
                        .w(px(280.))
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(content(0.15))
                        .bg(pane_fill())
                        .shadow_xl()
                        .child(
                            div()
                                .mb_3()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child("Mute all notifications until"),
                        )
                        .child(self.mute_calendar(custom, cx))
                        .child(
                            div()
                                .mt_3()
                                .text_size(px(10.))
                                .text_color(content(0.4))
                                .child("Time · Local time, 24-hour"),
                        )
                        .child(
                            div()
                                .h(px(34.))
                                .px_2()
                                .border_1()
                                .border_color(content(0.15))
                                .rounded_md()
                                .child(custom.input.clone()),
                        )
                        .when(custom.error.is_some(), |d| {
                            d.child(
                                div()
                                    .id("project-notification-accessible-5")
                                    .role(Role::Alert)
                                    .mt_3()
                                    .text_size(px(12.))
                                    .text_color(gpui::rgb(0xff6467))
                                    .child(custom.error.clone().unwrap_or_default()),
                            )
                        })
                        .child(
                            div()
                                .mt_3()
                                .pt(px(10.))
                                .border_t_1()
                                .border_color(content(0.1))
                                .flex()
                                .justify_between()
                                .child(
                                    secondary_button(
                                        "cancel-custom-notification-mute",
                                        "Cancel",
                                        false,
                                        false,
                                    )
                                    .child("Cancel")
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.notification_state.custom = None;
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    secondary_button(
                                        "save-custom-notification-mute",
                                        "Mute until then",
                                        false,
                                        false,
                                    )
                                    .child("Mute until then")
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            let Some(custom) = &this.notification_state.custom
                                            else {
                                                return;
                                            };
                                            let parsed = NaiveDateTime::parse_from_str(
                                                &format!(
                                                    "{} {}",
                                                    custom.date,
                                                    custom.input.read(cx).content.trim()
                                                ),
                                                "%Y-%m-%d %H:%M",
                                            )
                                            .ok()
                                            .and_then(|at| {
                                                Local.from_local_datetime(&at).earliest()
                                            })
                                            .map(|at| at.timestamp_millis().max(0) as u64);
                                            let now = monocode_core::time::now_ms();
                                            match parsed {
                                                Some(at) if at > now => {
                                                    let ids = custom.ids.clone();
                                                    this.update(cx, |p| {
                                                        p.mute_projects(&ids, Some(json!(at)), now)
                                                    });
                                                    this.notification_state.custom = None;
                                                }
                                                _ => {
                                                    this.notification_state
                                                        .custom
                                                        .as_mut()
                                                        .unwrap()
                                                        .error = Some(
                                                        if parsed.is_some() {
                                                            "Choose a date and time in the future."
                                                        } else {
                                                            "Choose a valid date and time."
                                                        }
                                                        .into(),
                                                    );
                                                }
                                            }
                                            cx.notify();
                                        },
                                    )),
                                ),
                        ),
                )
                .into_any_element(),
            );
        }
        overlays
    }
}
impl SettingsView {
    fn mute_calendar(&self, custom: &CustomMute, cx: &mut Context<Self>) -> AnyElement {
        let today = Local::now().date_naive();
        let month = custom.month;
        let previous_disabled = month <= today.with_day(1).unwrap();
        let shift = |date: NaiveDate, amount: i32| {
            let total = date.year() * 12 + date.month0() as i32 + amount;
            NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
                .unwrap_or(date)
        };
        let previous = shift(month, -1);
        let next = shift(month, 1);
        let count = (next - month).num_days() as usize;
        let offset = month.weekday().num_days_from_monday() as usize;
        let cells = (offset + count).div_ceil(7) * 7;
        let mut weeks = Vec::new();
        for week in 0..cells / 7 {
            let mut days = Vec::new();
            for column in 0..7 {
                let cell = week * 7 + column;
                let date = cell
                    .checked_sub(offset)
                    .filter(|index| *index < count)
                    .map(|index| month + chrono::Duration::days(index as i64));
                let Some(date) = date else {
                    days.push(div().w(px(36.)).h(px(32.)).into_any_element());
                    continue;
                };
                let disabled = date < today;
                let selected = date == custom.date;
                let focused = date == custom.focused_date;
                days.push(
                    div()
                        .w(px(36.))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id(("mute-calendar-day", cell))
                                .role(Role::Button)
                                .aria_label(date.to_string())
                                .aria_selected(selected)
                                .size(px(32.))
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.))
                                .text_color(content(if disabled { 0.2 } else { 0.7 }))
                                .when(focused, |d| d.border_1().border_color(accent_color()))
                                .when(selected, |d| {
                                    d.bg(content(0.1)).border_1().border_color(content(0.2))
                                })
                                .when(!disabled, |d| {
                                    d.cursor_pointer().hover(|s| s.bg(content(0.05)))
                                })
                                .child(date.day().to_string())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !disabled {
                                        if let Some(custom) = &mut this.notification_state.custom {
                                            custom.date = date;
                                            custom.focused_date = date;
                                            custom.error = None;
                                        }
                                        cx.notify();
                                    }
                                })),
                        )
                        .into_any_element(),
                );
            }
            weeks.push(div().flex().py(px(2.)).children(days));
        }
        div()
            .child(
                div()
                    .h(px(32.))
                    .mb_2()
                    .px_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(month.format("%B %Y").to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(2.))
                            .child(
                                secondary_button(
                                    "mute-previous-month",
                                    "Previous month",
                                    false,
                                    previous_disabled,
                                )
                                .child(icon("chevron-left", 14.))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if !previous_disabled {
                                            if let Some(custom) =
                                                &mut this.notification_state.custom
                                            {
                                                custom.month = previous;
                                            }
                                            cx.notify();
                                        }
                                    },
                                )),
                            )
                            .child(
                                secondary_button("mute-next-month", "Next month", false, false)
                                    .child(icon("chevron-right", 14.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(custom) = &mut this.notification_state.custom {
                                            custom.month = next;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id("notification-mute-calendar")
                    .role(Role::Grid)
                    .aria_label("Choose date")
                    .track_focus(&custom.focus)
                    .tab_index(0)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            if let Some(custom) = &this.notification_state.custom {
                                window.focus(&custom.focus, cx);
                            }
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                        let Some(custom) = &mut this.notification_state.custom else {
                            return;
                        };
                        let day = custom.focused_date;
                        let offset = day.weekday().num_days_from_monday() as i64;
                        let next = match event.keystroke.key.as_str() {
                            "enter" | "space" => {
                                custom.date = day;
                                custom.error = None;
                                Some(day)
                            }
                            "left" => Some(day - chrono::Duration::days(1)),
                            "right" => Some(day + chrono::Duration::days(1)),
                            "up" => Some(day - chrono::Duration::days(7)),
                            "down" => Some(day + chrono::Duration::days(7)),
                            "home" => Some(day - chrono::Duration::days(offset)),
                            "end" => Some(day + chrono::Duration::days(6 - offset)),
                            "pageup" => day.checked_sub_months(chrono::Months::new(1)),
                            "pagedown" => day.checked_add_months(chrono::Months::new(1)),
                            _ => None,
                        };
                        if let Some(next) = next {
                            let next = next.max(Local::now().date_naive());
                            custom.focused_date = next;
                            custom.month = next.with_day(1).unwrap();
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .flex()
                    .flex_col()
                    .child(
                        div().flex().children(
                            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
                                .into_iter()
                                .map(|day| {
                                    div()
                                        .w(px(36.))
                                        .text_center()
                                        .pb(px(6.))
                                        .text_size(px(10.))
                                        .text_color(content(0.4))
                                        .child(day)
                                }),
                        ),
                    )
                    .children(weeks),
            )
            .into_any_element()
    }
}
