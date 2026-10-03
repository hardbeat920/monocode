//! Port of src/features/inbox/ui/LinkedWorkItemUpdateNotice.test.ts. The
//! announce cue's dedupe lives with the host (settings sounds), so these
//! tests check what the notice hands it.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Entity, Task, TestAppContext, VisualTestContext};

use super::{draw, init};
use crate::data::*;
use crate::fixtures::{FakeServices, NOW};
use crate::pr::linked_notice::{LinkedWorkItemUpdateNotice, NoticeHandlers};

fn base_card() -> LinkedWorkItemUpdateCard {
    LinkedWorkItemUpdateCard {
        kind: WorkItemKind::Pr,
        repo: "acme/app".into(),
        number: 42,
        title: "Update sidebar activity".into(),
        url: "https://github.com/acme/app/pull/42".into(),
        state: "open".into(),
        since: 0,
        updated_at: 1,
        status: LinkedWorkItemUpdateStatus::Ready,
        counts: LinkedWorkItemActivityCounts {
            comments: 1,
            reviews: 0,
            commits: 0,
        },
        entries: vec![LinkedWorkItemActivityEntry {
            id: "comment-1".into(),
            kind: LinkedWorkItemActivityKind::Comment,
            author: "maya".into(),
            text: "Please cover the empty state".into(),
            created_at: "2026-09-13T11:00:00Z".into(),
            url: "https://github.com/acme/app/pull/42#issuecomment-1".into(),
        }],
        truncated: false,
    }
}

#[derive(Default)]
struct Log {
    calls: Vec<String>,
    announced: Vec<(String, i64, LinkedWorkItemUpdateStatus)>,
}

struct Harness<'a> {
    services: Rc<FakeServices>,
    log: Rc<RefCell<Log>>,
    notice: Entity<LinkedWorkItemUpdateNotice>,
    cx: &'a mut VisualTestContext,
}

fn mount(
    cx: &mut TestAppContext,
    card: Option<LinkedWorkItemUpdateCard>,
    archive: Option<bool>,
    delete: Option<bool>,
) -> Harness<'_> {
    cx.update(init);
    let services = FakeServices::new(NOW);
    let log: Rc<RefCell<Log>> = Rc::default();
    let entry = |name: &'static str, log: &Rc<RefCell<Log>>| {
        let log = log.clone();
        Rc::new(move |_: &mut gpui::Window, _: &mut gpui::App| {
            log.borrow_mut().calls.push(name.into())
        })
    };
    let handlers = NoticeHandlers {
        on_acknowledge: entry("acknowledge", &log),
        on_dismiss: entry("dismiss", &log),
        on_open_discussion: entry("open_discussion", &log),
        on_add_to_chat: {
            let log = log.clone();
            Rc::new(move |card, _, _| {
                log.borrow_mut()
                    .calls
                    .push(format!("add_to_chat {}", card.entries[0].text))
            })
        },
        on_announce: Some({
            let log = log.clone();
            Rc::new(move |session, card, _| {
                log.borrow_mut()
                    .announced
                    .push((session.to_string(), card.updated_at, card.status))
            })
        }),
        on_archive_session: archive.map(|result| {
            let log = log.clone();
            Rc::new(move |_: &mut gpui::App| {
                log.borrow_mut().calls.push("archive".into());
                Task::ready(Ok(result))
            }) as Rc<dyn Fn(&mut gpui::App) -> DataTask<bool>>
        }),
        on_delete_session: delete.map(|result| {
            let log = log.clone();
            Rc::new(move |_: &mut gpui::App| {
                log.borrow_mut().calls.push("delete".into());
                Task::ready(Ok(result))
            }) as Rc<dyn Fn(&mut gpui::App) -> DataTask<bool>>
        }),
    };
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let (notice, cx) = cx.add_window_view(move |_, cx| {
        let mut notice =
            LinkedWorkItemUpdateNotice::new(services_dyn, "session-1".into(), card, handlers, cx);
        notice.set_animate(false);
        notice
    });
    draw(cx);
    Harness {
        services,
        log,
        notice,
        cx,
    }
}

#[gpui::test]
fn stays_hidden_while_details_load_and_shows_when_ready(cx: &mut TestAppContext) {
    let loading = LinkedWorkItemUpdateCard {
        status: LinkedWorkItemUpdateStatus::Loading,
        ..base_card()
    };
    let h = mount(cx, Some(loading), None, None);
    assert!(!h.notice.read_with(h.cx, |notice, _| notice.shown()));
    h.notice.update(h.cx, |notice, cx| {
        notice.set_card("session-1".into(), Some(base_card()), cx)
    });
    draw(h.cx);
    assert!(h.notice.read_with(h.cx, |notice, _| notice.shown()));
    // An unchanged card is not announced again.
    h.notice.update(h.cx, |notice, cx| {
        notice.set_card("session-1".into(), Some(base_card()), cx)
    });
    let announced = &h.log.borrow().announced;
    assert_eq!(announced.len(), 2);
    assert_eq!(announced[1].2, LinkedWorkItemUpdateStatus::Ready);
}

#[gpui::test]
fn offers_discussion_and_agent_actions_for_a_new_comment(cx: &mut TestAppContext) {
    let h = mount(cx, Some(base_card()), None, None);
    h.cx.update(|window, cx| {
        h.notice
            .update(cx, |notice, cx| notice.open_activity(window, cx))
    });
    assert_eq!(h.log.borrow().calls, ["acknowledge", "open_discussion"]);
    h.cx.update(|window, cx| {
        h.notice
            .update(cx, |notice, cx| notice.add_to_chat(window, cx))
    });
    assert_eq!(
        h.log.borrow().calls[2..],
        ["acknowledge", "add_to_chat Please cover the empty state"]
    );
}

#[gpui::test]
fn opens_the_exact_commit_for_commit_activity(cx: &mut TestAppContext) {
    let card = LinkedWorkItemUpdateCard {
        counts: LinkedWorkItemActivityCounts {
            comments: 0,
            reviews: 0,
            commits: 1,
        },
        entries: vec![LinkedWorkItemActivityEntry {
            id: "abcdef123456".into(),
            kind: LinkedWorkItemActivityKind::Commit,
            author: "nik".into(),
            text: "Handle linked activity".into(),
            created_at: "2026-09-13T11:30:00Z".into(),
            url: "https://github.com/acme/app/commit/abcdef123456".into(),
        }],
        ..base_card()
    };
    let h = mount(cx, Some(card), None, None);
    h.cx.update(|window, cx| {
        h.notice
            .update(cx, |notice, cx| notice.open_activity(window, cx))
    });
    assert_eq!(h.log.borrow().calls, ["acknowledge"]);
    assert_eq!(
        h.services.state.borrow().opened_urls,
        ["https://github.com/acme/app/commit/abcdef123456"]
    );
}

#[gpui::test]
fn offers_archive_and_confirmed_delete_flows_for_a_merged_pull_request(cx: &mut TestAppContext) {
    let card = LinkedWorkItemUpdateCard {
        state: "merged".into(),
        ..base_card()
    };
    let h = mount(cx, Some(card), Some(true), Some(false));
    h.cx.update(|window, cx| {
        h.notice
            .update(cx, |notice, cx| notice.run_cleanup(false, window, cx))
    });
    draw(h.cx);
    assert_eq!(h.log.borrow().calls, ["delete"]);
    h.cx.update(|window, cx| {
        h.notice
            .update(cx, |notice, cx| notice.run_cleanup(true, window, cx))
    });
    draw(h.cx);
    assert_eq!(h.log.borrow().calls, ["delete", "archive", "acknowledge"]);
}

#[gpui::test]
fn dismissing_acknowledges_then_dismisses(cx: &mut TestAppContext) {
    let h = mount(cx, Some(base_card()), None, None);
    h.cx.update(|window, cx| h.notice.update(cx, |notice, cx| notice.dismiss(window, cx)));
    assert_eq!(h.log.borrow().calls, ["acknowledge", "dismiss"]);
}
