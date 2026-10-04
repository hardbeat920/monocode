//! Ports of NotificationMuteControl.test.ts and
//! ProjectNotificationSettings.test.ts.

use std::time::Duration;

use chrono::{Local, TimeZone as _};
use gpui::{AppContext as _, TestAppContext};

use super::*;
use crate::accounts::host::NotificationsHost;
use crate::accounts::mute_control::NotificationMuteControl;
use crate::accounts::notification_model::{
    Mute, NotificationCategory as C, NotificationProject, NotificationProjectKind, PreferencePatch,
    Preferences, ProjectNotificationPreference,
};
use crate::accounts::project_notifications::{
    ProjectNotificationProps, ProjectNotificationSettings,
};

/// A fake `NotificationsHost` with the engine's patch semantics and a
/// write that can be made to fail.
#[derive(Default)]
pub(super) struct FakeNotifications {
    pub now: Cell<i64>,
    pub prefs: RefCell<Preferences>,
    pub fail: Cell<bool>,
    pub catalog: RefCell<Vec<NotificationProject>>,
    observers: RefCell<Vec<Rc<OnChange>>>,
}

impl FakeNotifications {
    fn new(now: i64) -> Rc<Self> {
        let host = Self::default();
        host.now.set(now);
        Rc::new(host)
    }

    fn remember(&self, projects: Vec<NotificationProject>) {
        self.catalog.borrow_mut().extend(projects);
    }

    fn set(&self, id: &str, preference: ProjectNotificationPreference) {
        self.prefs.borrow_mut().insert(id.into(), preference);
    }

    fn get(&self, id: &str) -> Option<ProjectNotificationPreference> {
        self.prefs.borrow().get(id).cloned()
    }
}

impl NotificationsHost for FakeNotifications {
    fn now(&self) -> i64 {
        self.now.get()
    }

    fn observe(&self, on_change: OnChange, _: &mut App) -> Option<Subscription> {
        self.observers.borrow_mut().push(Rc::new(on_change));
        None
    }

    fn preferences(&self, _: &App) -> Preferences {
        self.prefs.borrow().clone()
    }

    fn update_preferences(
        &self,
        project_ids: &[String],
        patch: &PreferencePatch,
        cx: &mut App,
    ) -> Result<(), String> {
        if self.fail.get() {
            return Err("Storage full".into());
        }
        {
            let mut prefs = self.prefs.borrow_mut();
            for id in project_ids {
                let mut next = prefs.get(id).cloned().unwrap_or_default();
                if let Some(disabled) = &patch.disabled {
                    next.disabled = disabled.clone();
                }
                if let Some(muted_until) = patch.muted_until {
                    next.muted_until = muted_until;
                }
                prefs.insert(id.clone(), next);
            }
        }
        let observers: Vec<Rc<OnChange>> = self.observers.borrow().clone();
        cx.defer(move |cx| {
            for observer in observers {
                observer(cx);
            }
        });
        Ok(())
    }

    fn notification_projects(&self, paths: &[String], _: &App) -> Vec<NotificationProject> {
        let catalog = self.catalog.borrow();
        let requested: Vec<&String> = paths
            .iter()
            .filter(|path| !path.is_empty() && path.as_str() != "/")
            .collect();
        let mut projects: Vec<NotificationProject> = catalog
            .iter()
            .filter(|project| {
                project.paths.is_empty()
                    || project.paths.iter().any(|path| requested.contains(&path))
            })
            .cloned()
            .collect();
        for path in requested {
            if projects.iter().any(|project| project.paths.contains(path)) {
                continue;
            }
            let name = path.rsplit('/').next().unwrap_or(path).to_string();
            projects.push(NotificationProject {
                id: format!("local:{path}"),
                name,
                detail: path.clone(),
                kind: NotificationProjectKind::Local,
                paths: vec![path.clone()],
            });
        }
        projects
    }
}

fn repository(id: &str, name: &str) -> NotificationProject {
    NotificationProject {
        id: id.into(),
        name: name.into(),
        detail: "github.com".into(),
        kind: NotificationProjectKind::Repository,
        paths: Vec::new(),
    }
}

const PRIVATE: &str = "repository:github.com/me/private";
const WORK: &str = "repository:github.com/work/app";

fn local(y: i32, m: u32, d: u32, h: u32) -> i64 {
    Local
        .with_ymd_and_hms(y, m, d, h, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis()
}

fn utc(y: i32, m: u32, d: u32, h: u32) -> i64 {
    chrono::Utc
        .with_ymd_and_hms(y, m, d, h, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis()
}

fn mount_control(
    cx: &mut TestAppContext,
    host: Rc<FakeNotifications>,
    ids: &[&str],
) -> (
    Entity<NotificationMuteControl>,
    &'static mut VisualTestContext,
) {
    let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    mount(cx, 700., 800., move |_, cx| {
        cx.new(|cx| NotificationMuteControl::new(host, ids, "mute", cx))
    })
}

fn set_time(cx: &mut VisualTestContext, control: &Entity<NotificationMuteControl>, value: &str) {
    let input = control.read_with(cx, |control, cx| {
        control
            .picker()
            .unwrap()
            .read(cx)
            .picker()
            .read(cx)
            .time_input()
            .clone()
    });
    cx.update(|window, cx| input.update(cx, |input, cx| input.set_value("", window, cx)));
    type_into(cx, &input, value);
}

#[gpui::test]
fn mutes_selected_projects_for_eight_hours_without_replacing_category_choices(
    cx: &mut TestAppContext,
) {
    let host = FakeNotifications::new(1_800_000_000_000);
    host.set(
        "private",
        ProjectNotificationPreference {
            disabled: vec![C::Issues],
            muted_until: None,
        },
    );
    let (_, cx) = mount_control(cx, host.clone(), &["private", "personal"]);
    click(cx, "mute/button:Mute notifications");
    click(cx, "mute/menuitem:mute:8");
    assert_eq!(
        host.get("private"),
        Some(ProjectNotificationPreference {
            disabled: vec![C::Issues],
            muted_until: Some(Mute::Until(1_800_028_800_000)),
        })
    );
    assert_eq!(
        host.get("personal"),
        Some(ProjectNotificationPreference {
            disabled: vec![],
            muted_until: Some(Mute::Until(1_800_028_800_000)),
        })
    );
    assert!(exists(cx, "mute/status:2 of 2 projects muted"));
}

#[gpui::test]
fn resumes_notifications_while_preserving_the_projects_chosen_categories(cx: &mut TestAppContext) {
    let host = FakeNotifications::new(1_800_000_000_000);
    host.set(
        "private",
        ProjectNotificationPreference {
            disabled: vec![C::Issues],
            muted_until: Some(Mute::UntilResumed),
        },
    );
    let (_, cx) = mount_control(cx, host.clone(), &["private"]);
    assert!(exists(cx, "text:Muted until resumed"));
    click(cx, "mute/button:Resume notifications");
    let private = host.get("private").unwrap();
    assert_eq!(private.disabled, [C::Issues]);
    assert_eq!(private.muted_until, None);
    assert!(!exists(cx, "text:Muted until resumed"));
}

#[gpui::test]
fn accepts_a_custom_future_time_and_rejects_an_expired_one_without_saving(cx: &mut TestAppContext) {
    let host = FakeNotifications::new(local(2030, 1, 15, 12));
    let (control, cx) = mount_control(cx, host.clone(), &["private"]);
    click(cx, "mute/button:Mute notifications");
    click(cx, "mute/menuitem:mute:custom");
    assert!(!exists(cx, "mute/menuitem:mute:1"));
    assert!(exists(cx, "mute/dialog:Mute project notifications"));
    assert!(exists(cx, "grid"));
    assert!(exists(cx, "button:Mute until then"));

    click(cx, "day:2030-01-15");
    assert!(exists(cx, "input:HH:mm"));
    set_time(cx, &control, "08:00");
    click(cx, "button:Mute until then");
    assert!(exists(cx, "text:Choose a date and time in the future."));
    assert_eq!(host.get("private"), None);

    click(cx, "day:2030-01-16");
    set_time(cx, &control, "17:00");
    host.fail.set(true);
    click(cx, "button:Mute until then");
    assert!(exists(
        cx,
        "text:Could not save notification preferences. Please try again."
    ));
    let value = control.read_with(cx, |control, cx| {
        control.picker().unwrap().read(cx).value().to_string()
    });
    assert_eq!(value, "2030-01-16T17:00");

    host.fail.set(false);
    click(cx, "button:Mute until then");
    assert_eq!(
        host.get("private").unwrap().muted_until,
        Some(Mute::Until(local(2030, 1, 16, 17)))
    );
    assert!(!exists(cx, "alert"));
    let trigger = control.read_with(cx, |control, _| control.trigger_focus().clone());
    assert!(cx.update(|window, _| trigger.is_focused(window)));
}

fn mount_settings(
    cx: &mut TestAppContext,
    host: Rc<FakeNotifications>,
    props: ProjectNotificationProps,
) -> (
    Entity<ProjectNotificationSettings>,
    &'static mut VisualTestContext,
) {
    mount(cx, 900., 1400., move |_, cx| {
        cx.new(|cx| ProjectNotificationSettings::new(host, props, cx))
    })
}

fn two_repositories(now: i64) -> Rc<FakeNotifications> {
    let host = FakeNotifications::new(now);
    host.remember(vec![
        repository(PRIVATE, "me/private"),
        repository(WORK, "work/app"),
    ]);
    host
}

fn switch_on(cx: &mut VisualTestContext, label: &str) -> bool {
    if exists(cx, &format!("switch-state:{label}=on"))
        || exists(cx, &format!("switch-state:{label}=on:described"))
    {
        return true;
    }
    assert!(
        exists(cx, &format!("switch-state:{label}=off"))
            || exists(cx, &format!("switch-state:{label}=off:described")),
        "missing switch {label}"
    );
    false
}

#[gpui::test]
fn saves_only_the_selected_projects_categories_while_preserving_its_mute_deadline(
    cx: &mut TestAppContext,
) {
    let host = two_repositories(1_800_000_000_000);
    host.set(
        PRIVATE,
        ProjectNotificationPreference {
            disabled: vec![],
            muted_until: Some(Mute::UntilResumed),
        },
    );
    let (_, cx) = mount_settings(cx, host.clone(), ProjectNotificationProps::default());
    click(cx, "button:Notification categories for me/private");
    for category in [
        "Issues and Linear tasks",
        "Agent finished",
        "Agent approvals and questions",
    ] {
        click(cx, &format!("switch:{category} for me/private"));
    }
    assert!(switch_on(
        cx,
        "Pull requests / Merge requests for me/private"
    ));
    assert!(!switch_on(cx, "Issues and Linear tasks for me/private"));
    assert_eq!(
        host.get(PRIVATE),
        Some(ProjectNotificationPreference {
            disabled: vec![C::Issues, C::AgentFinished, C::AgentInput],
            muted_until: Some(Mute::UntilResumed),
        })
    );
    assert_eq!(host.get(WORK), None);
}

#[gpui::test]
fn offers_only_issue_notifications_for_a_linear_project(cx: &mut TestAppContext) {
    let host = two_repositories(1_800_000_000_000);
    host.remember(vec![NotificationProject {
        id: "linear:project:roadmap".into(),
        name: "Roadmap".into(),
        detail: "Linear".into(),
        kind: NotificationProjectKind::Linear,
        paths: Vec::new(),
    }]);
    let (settings, cx) = mount_settings(cx, host.clone(), ProjectNotificationProps::default());
    click(cx, "button:Notification categories for Roadmap");
    assert!(exists(cx, "switch:Issues and Linear tasks for Roadmap"));
    for category in [
        "Pull requests / Merge requests",
        "Agent finished",
        "Agent approvals and questions",
        "Reminders",
    ] {
        assert!(!exists(cx, &format!("switch:{category} for Roadmap")));
    }
    assert_eq!(
        settings.read_with(cx, |settings, _| settings.expanded().map(str::to_string)),
        Some("linear:project:roadmap".into())
    );
    click(cx, "switch:Issues and Linear tasks for Roadmap");
    assert_eq!(
        host.get("linear:project:roadmap").unwrap().disabled,
        [C::Issues]
    );
}

#[gpui::test]
fn keeps_local_projects_while_offering_only_local_notification_categories(cx: &mut TestAppContext) {
    let host = two_repositories(1_800_000_000_000);
    let props = ProjectNotificationProps {
        cwd: "/fun".into(),
        ..Default::default()
    };
    let (_, cx) = mount_settings(cx, host, props);
    assert!(exists(cx, "text:Local project · All categories enabled"));
    click(cx, "button:Notification categories for fun");
    assert!(switch_on(cx, "Agent finished for fun"));
    assert!(switch_on(cx, "Agent approvals and questions for fun"));
    assert!(switch_on(cx, "Reminders for fun"));
    assert!(!exists(cx, "switch:Pull requests / Merge requests for fun"));
    assert!(!exists(cx, "switch:Issues and Linear tasks for fun"));
}

#[gpui::test]
fn discovers_recent_projects_and_focuses_the_project_requested_by_a_quick_action(
    cx: &mut TestAppContext,
) {
    let host = FakeNotifications::new(1_800_000_000_000);
    let props = ProjectNotificationProps {
        cwd: "/newwork".into(),
        recents: vec!["/newprivate".into()],
        notification_project_path: Some("/newprivate".into()),
        ..Default::default()
    };
    let (settings, cx) = mount_settings(cx, host, props);
    assert!(exists(cx, "fieldset:newwork"));
    assert!(switch_on(cx, "Agent finished for newprivate"));
    let card = settings.read_with(cx, |settings, _| {
        settings.card_focus("local:/newprivate").cloned().unwrap()
    });
    assert!(cx.update(|window, _| card.is_focused(window)));
    assert_eq!(
        settings.read_with(cx, |settings, _| settings.expanded().map(str::to_string)),
        Some("local:/newprivate".into())
    );
    assert!(exists(cx, "panel:newprivate"));
    assert!(!exists(cx, "panel:newwork"));
}

#[gpui::test]
fn keeps_projects_collapsed_until_opened_and_preserves_choices_when_switching_projects(
    cx: &mut TestAppContext,
) {
    let host = two_repositories(1_800_000_000_000);
    let (_, cx) = mount_settings(cx, host, ProjectNotificationProps::default());
    assert!(!exists(cx, "panel:me/private"));
    assert!(!exists(cx, "panel:work/app"));

    click(cx, "button:Notification categories for me/private");
    assert!(exists(cx, "panel:me/private"));
    click(cx, "switch:Issues and Linear tasks for me/private");
    assert!(exists(cx, "text:4 of 5 enabled"));
    click(cx, "button:Notification categories for work/app");
    assert!(!exists(cx, "panel:me/private"));
    assert!(exists(cx, "panel:work/app"));
    click(cx, "button:Notification categories for me/private");
    assert!(!switch_on(cx, "Issues and Linear tasks for me/private"));
    click(cx, "button:Notification categories for me/private");
    assert!(!exists(cx, "panel:me/private"));
    assert!(!exists(cx, "panel:work/app"));
}

#[gpui::test]
fn matches_intl_notification_project_order(cx: &mut TestAppContext) {
    let host = FakeNotifications::new(1_800_000_000_000);
    host.remember(
        [
            "filez",
            "file.a",
            "fileé",
            "filee\u{301}",
            "file-a",
            "filee",
            "file_a",
        ]
        .into_iter()
        .map(|name| repository(name, name))
        .collect(),
    );
    let (settings, cx) = mount_settings(cx, host, ProjectNotificationProps::default());
    let projects = settings.read_with(cx, |settings, cx| settings.projects(cx));
    assert_eq!(
        projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>(),
        [
            "file_a",
            "file-a",
            "file.a",
            "filee",
            "fileé",
            "filee\u{301}",
            "filez"
        ]
    );
    assert!(projects.iter().all(|project| project.id == project.name));
}

#[gpui::test]
fn mutes_several_selected_projects_without_changing_another_projects_notifications(
    cx: &mut TestAppContext,
) {
    let now = utc(2026, 9, 14, 8);
    let host = two_repositories(now);
    host.remember(vec![repository(
        "repository:github.com/me/other",
        "me/other",
    )]);
    let (_, cx) = mount_settings(cx, host.clone(), ProjectNotificationProps::default());
    assert!(!exists(cx, "checkbox:Select me/private"));
    click(cx, "button:select-projects");
    click(cx, "checkbox:Select me/private");
    click(cx, "checkbox:Select me/other");
    assert!(exists(cx, "group:Mute selected projects"));
    assert!(exists(cx, "text:2 selected"));
    click(cx, "bulk/button:Mute notifications");
    click(cx, "bulk/menuitem:mute:8");
    let deadline = Some(Mute::Until(utc(2026, 9, 14, 16)));
    assert_eq!(host.get(PRIVATE).unwrap().muted_until, deadline);
    assert_eq!(
        host.get("repository:github.com/me/other")
            .unwrap()
            .muted_until,
        deadline
    );
    assert_eq!(host.get(WORK), None);
}

fn explains_the_project_wide_pause(cx: &mut TestAppContext, manual: bool) {
    let now = utc(2030, 1, 15, 12);
    let host = two_repositories(now);
    host.set(
        PRIVATE,
        ProjectNotificationPreference {
            disabled: vec![C::Issues],
            muted_until: Some(if manual {
                Mute::UntilResumed
            } else {
                Mute::Until(now + 60_000)
            }),
        },
    );
    let (_, cx) = mount_settings(cx, host.clone(), ProjectNotificationProps::default());
    click(cx, "button:Notification categories for me/private");
    assert!(exists(cx, "text:All notifications paused"));
    assert!(exists(cx, "hint:me/private"));
    let pr = "Pull requests / Merge requests for me/private";
    assert!(exists(cx, &format!("switch-state:{pr}=on:described")));
    click(cx, "switch:Reminders for me/private");
    assert!(exists(cx, "text:All notifications paused"));
    assert!(exists(
        cx,
        &format!("project:{PRIVATE}/button:Resume notifications")
    ));
    if manual {
        click(
            cx,
            &format!("project:{PRIVATE}/button:Resume notifications"),
        );
    } else {
        host.now.set(now + 60_000);
        cx.executor().advance_clock(Duration::from_millis(60_000));
        draw(cx);
    }
    assert!(exists(cx, "text:3 of 5 enabled"));
    assert!(!exists(cx, "text:All notifications paused"));
    assert!(exists(cx, &format!("switch-state:{pr}=on")));
    assert!(!switch_on(cx, "Issues and Linear tasks for me/private"));
    assert_eq!(
        host.get(PRIVATE).unwrap().disabled,
        [C::Issues, C::Reminders]
    );
}

#[gpui::test]
fn explains_the_project_wide_pause_and_preserves_editable_category_choices_after_manual_resume(
    cx: &mut TestAppContext,
) {
    explains_the_project_wide_pause(cx, true);
}

#[gpui::test]
fn explains_the_project_wide_pause_and_preserves_editable_category_choices_after_expiry_resume(
    cx: &mut TestAppContext,
) {
    explains_the_project_wide_pause(cx, false);
}

#[gpui::test]
fn dismisses_the_mute_menu_and_custom_date_picker_without_changing_preferences(
    cx: &mut TestAppContext,
) {
    let host = two_repositories(1_800_000_000_000);
    let (settings, cx) = mount_settings(cx, host.clone(), ProjectNotificationProps::default());
    let control = settings.read_with(cx, |settings, _| {
        settings.control(PRIVATE).cloned().unwrap()
    });
    let scope = format!("project:{PRIVATE}");
    click(cx, &format!("{scope}/button:Mute notifications"));
    assert!(control.read_with(cx, |control, _| control.open().is_some()));
    keys(cx, "escape");
    assert!(!exists(cx, &format!("{scope}/menu:Mute notifications")));
    let trigger = control.read_with(cx, |control, _| control.trigger_focus().clone());
    assert!(cx.update(|window, _| trigger.is_focused(window)));

    click(cx, &format!("{scope}/button:Mute notifications"));
    click(cx, &format!("{scope}/menuitem:mute:custom"));
    assert!(exists(
        cx,
        &format!("{scope}/dialog:Mute project notifications")
    ));
    click(cx, "button:Cancel");
    assert!(!exists(
        cx,
        &format!("{scope}/dialog:Mute project notifications")
    ));
    assert!(control.read_with(cx, |control, _| control.open().is_none()));
    assert!(cx.update(|window, _| trigger.is_focused(window)));
    assert_eq!(host.get(PRIVATE), None);
}

#[gpui::test]
fn shows_an_empty_state_without_projects(cx: &mut TestAppContext) {
    let host = FakeNotifications::new(1_800_000_000_000);
    let (_, cx) = mount_settings(cx, host, ProjectNotificationProps::default());
    assert!(exists(
        cx,
        "text:Open a project or connect an Inbox provider to configure its notifications."
    ));
    assert!(!exists(cx, "button:select-projects"));
}
