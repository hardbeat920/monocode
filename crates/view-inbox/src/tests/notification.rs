//! Port of src/features/inbox/ui/InboxNotificationMenu.test.ts and the Inbox
//! menu cases of ProjectNotificationMenu.test.ts. The project rail and the
//! preference store are the attention package's; a fake stands in.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, Entity, Subscription, TestAppContext, VisualTestContext, point, px};

use super::{draw, init};
use crate::data::Listener;
use crate::fixtures::Listeners;
use crate::list::notification_menu::{
    CloseNotificationMenu, InboxNotificationData, InboxNotificationMenu, NotificationMenuState,
};

#[derive(Default)]
struct FakeState {
    menu: NotificationMenuState,
    fail_writes: bool,
    calls: Vec<String>,
}

#[derive(Clone, Default)]
struct FakeNotifications {
    state: Rc<RefCell<FakeState>>,
    listeners: Listeners,
}

const NOW: i64 = 1_894_653_000_000;

impl InboxNotificationData for FakeNotifications {
    fn subscribe(&self, listener: Listener, _: &mut App) -> Subscription {
        self.listeners.subscribe(listener)
    }

    fn state(&self, _: &App) -> NotificationMenuState {
        self.state.borrow().menu.clone()
    }

    fn now_ms(&self) -> i64 {
        NOW
    }

    fn mark_all_read(&self, _: &mut App) -> bool {
        let mut state = self.state.borrow_mut();
        state.calls.push("mark_all_read".into());
        if state.fail_writes {
            return false;
        }
        state.menu.has_unread = false;
        true
    }

    fn mute_all(&self, until: Option<i64>, _: &mut App) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.calls.push(format!("mute_all {until:?}"));
        if state.fail_writes {
            return Err("Storage full".into());
        }
        state.menu.muted_count = state.menu.project_count;
        Ok(())
    }

    fn resume_muted(&self, _: &mut App) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.calls.push("resume_muted".into());
        state.menu.muted_count = 0;
        Ok(())
    }

    fn open_settings(&self, _: &mut App) {
        self.state.borrow_mut().calls.push("open_settings".into());
    }
}

struct Harness<'a> {
    data: FakeNotifications,
    menu: Entity<InboxNotificationMenu>,
    closed: Rc<RefCell<usize>>,
    cx: &'a mut VisualTestContext,
}

fn mount(cx: &mut TestAppContext, state: NotificationMenuState) -> Harness<'_> {
    cx.update(init);
    let data = FakeNotifications::default();
    data.state.borrow_mut().menu = state;
    let data_dyn: Rc<dyn InboxNotificationData> = Rc::new(data.clone());
    let (menu, cx) = cx.add_window_view(move |_, cx| {
        let mut menu = InboxNotificationMenu::new(data_dyn, point(px(24.), px(24.)), cx);
        menu.set_animate(false);
        menu
    });
    let closed = Rc::new(RefCell::new(0));
    let sink = closed.clone();
    cx.update(|_, cx| {
        cx.subscribe(&menu, move |_, _: &CloseNotificationMenu, _| {
            *sink.borrow_mut() += 1
        })
        .detach();
    });
    draw(cx);
    Harness {
        data,
        menu,
        closed,
        cx,
    }
}

impl Harness<'_> {
    fn pick(&mut self, id: &str) {
        let id = id.to_string();
        self.cx
            .update(|window, cx| self.menu.update(cx, |menu, cx| menu.pick(&id, window, cx)));
        draw(self.cx);
    }

    fn calls(&self) -> Vec<String> {
        self.data.state.borrow().calls.clone()
    }

    fn error(&mut self) -> Option<String> {
        self.menu
            .read_with(self.cx, |menu, _| menu.error().map(str::to_string))
    }
}

fn two_projects() -> NotificationMenuState {
    NotificationMenuState {
        project_count: 2,
        muted_count: 0,
        has_unread: true,
        can_open_settings: true,
    }
}

#[gpui::test]
fn keeps_unread_items_and_the_menu_open_when_marking_read_fails_then_allows_retry(
    cx: &mut TestAppContext,
) {
    let mut h = mount(cx, two_projects());
    h.data.state.borrow_mut().fail_writes = true;
    h.pick("read-all");
    assert_eq!(
        h.error().as_deref(),
        Some("Could not save read status. Please try again.")
    );
    assert_eq!(*h.closed.borrow(), 0);
    h.data.state.borrow_mut().fail_writes = false;
    h.pick("read-all");
    assert_eq!(*h.closed.borrow(), 1);
    assert_eq!(h.calls(), ["mark_all_read", "mark_all_read"]);
}

#[gpui::test]
fn disables_mark_all_as_read_when_everything_is_read(cx: &mut TestAppContext) {
    let mut h = mount(
        cx,
        NotificationMenuState {
            has_unread: false,
            ..two_projects()
        },
    );
    h.pick("read-all");
    assert!(h.calls().is_empty());
    assert_eq!(*h.closed.borrow(), 0);
}

#[gpui::test]
fn mutes_all_projects_for_one_hour_directly_from_inbox(cx: &mut TestAppContext) {
    let mut h = mount(cx, two_projects());
    h.pick("mute");
    assert!(h.menu.read_with(h.cx, |menu, _| menu.mute_open()));
    h.pick("mute:1");
    assert_eq!(h.calls(), [format!("mute_all Some({})", NOW + 3_600_000)]);
    assert_eq!(*h.closed.borrow(), 1);
}

#[gpui::test]
fn mutes_until_resumed(cx: &mut TestAppContext) {
    let mut h = mount(cx, two_projects());
    h.pick("mute:indefinite");
    assert_eq!(h.calls(), ["mute_all None"]);
}

#[gpui::test]
fn resumes_muted_projects_and_disables_resume_when_none_are_muted(cx: &mut TestAppContext) {
    let mut h = mount(
        cx,
        NotificationMenuState {
            muted_count: 1,
            ..two_projects()
        },
    );
    h.pick("resume");
    assert_eq!(h.calls(), ["resume_muted"]);
    let mut h2_state = two_projects();
    h2_state.muted_count = 0;
    h.data.state.borrow_mut().menu = h2_state;
    h.pick("resume");
    assert_eq!(h.calls(), ["resume_muted"]);
}

#[gpui::test]
fn opens_custom_timing_from_the_duration_submenu(cx: &mut TestAppContext) {
    let mut h = mount(cx, two_projects());
    h.pick("mute");
    h.pick("mute:custom");
    assert!(h.menu.read_with(h.cx, |menu, _| menu.custom_open()));
    assert!(h.calls().is_empty());
}

#[gpui::test]
fn keeps_the_menu_open_and_reports_failed_persistence_so_the_action_can_be_retried(
    cx: &mut TestAppContext,
) {
    let mut h = mount(cx, two_projects());
    h.data.state.borrow_mut().fail_writes = true;
    h.pick("mute:4");
    assert_eq!(
        h.error().as_deref(),
        Some("Could not save notification preferences. Please try again.")
    );
    assert_eq!(*h.closed.borrow(), 0);
    h.data.state.borrow_mut().fail_writes = false;
    h.pick("mute:4");
    assert_eq!(*h.closed.borrow(), 1);
}

#[gpui::test]
fn opens_notification_settings_for_all_projects_from_the_inbox_menu(cx: &mut TestAppContext) {
    let mut h = mount(cx, two_projects());
    h.pick("settings");
    assert_eq!(h.calls(), ["open_settings"]);
    assert_eq!(*h.closed.borrow(), 1);
}
