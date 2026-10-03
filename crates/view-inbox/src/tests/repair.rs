//! Port of src/features/inbox/ui/InboxPrChecksRepair.test.ts: repair
//! progress in the checks panel and the "Fix with AI" form.

use std::rc::Rc;

use gpui::TestAppContext;

use super::checks::{Harness, check, checks_state, mount, with_url};
use super::draw;
use crate::data::*;
use crate::fixtures::{FakeServices, NOW};
use crate::pr::checks::ChecksFilter;
use crate::pr::repair_form::CheckRepair;

fn repair(number: i64, sessions: Vec<RelatedSession>, services: &Rc<FakeServices>) -> CheckRepair {
    let start_services = services.clone();
    let opened = services.clone();
    CheckRepair {
        number,
        sessions,
        on_start: Rc::new(move |start, session_id, cx| {
            let item = InboxItem::github(InboxKind::Pr, "acme/web", number, "PR");
            start_services.repair_checks(&item, start, session_id, cx)
        }),
        on_open_session: Some(Rc::new(move |id, _, _| {
            opened
                .state
                .borrow_mut()
                .calls
                .push(format!("open_session {id}"));
        })),
    }
}

fn tracked(
    cwd: &str,
    head: &str,
    checks: &[GithubPrCheck],
    session: &str,
    phase: CiRepairPhase,
    started_at: i64,
) -> TrackedCiRepair {
    TrackedCiRepair {
        repo: "acme/web".into(),
        number: 42,
        head_oid: head.into(),
        checks: checks
            .iter()
            .map(|check| CiRepairCheck {
                name: check.name.clone(),
                workflow: check.workflow.clone(),
                url: check.url.clone(),
            })
            .collect(),
        id: format!("{session}-{head}"),
        cwd: cwd.into(),
        session_id: session.into(),
        started_at,
        sequence: None,
        phase,
    }
}

fn ci(name: &str, state: GithubPrCheckState) -> GithubPrCheck {
    GithubPrCheck {
        workflow: "CI".into(),
        ..check(name, state)
    }
}

impl Harness<'_> {
    fn set_repairs(&mut self, repairs: Vec<TrackedCiRepair>) {
        let services = self.services.clone();
        self.cx.update(|_, cx| services.set_repairs(repairs, cx));
        draw(self.cx);
    }

    fn cards(&mut self) -> Vec<(String, crate::pr::repair_progress::RepairCardText)> {
        self.view.read_with(self.cx, |view, _| view.repair_cards())
    }

    fn form_open(&mut self) -> bool {
        self.view
            .read_with(self.cx, |view, _| view.form().is_some())
    }
}

#[gpui::test]
fn only_marks_the_selected_job_as_repairing_when_check_names_repeat(cx: &mut TestAppContext) {
    let first = with_url(
        ci("tests", GithubPrCheckState::Fail),
        "https://ci.example/jobs/1",
    );
    let second = with_url(
        ci("tests", GithubPrCheckState::Fail),
        "https://ci.example/jobs/2",
    );
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().repairs = vec![tracked(
        "/duplicate-jobs",
        "abc",
        std::slice::from_ref(&first),
        "first-chat",
        CiRepairPhase::Running,
        NOW,
    )];
    let repair = repair(42, Vec::new(), &services);
    let mut h = mount(
        cx,
        services.clone(),
        checks_state("abc", vec![first.clone(), second.clone()]),
        "/duplicate-jobs",
        "acme/web",
        Some(repair),
    );
    assert_eq!(h.row("tests").repair.as_deref(), Some("Repairing"));
    let mut both = services.state.borrow().repairs.clone();
    both.insert(
        0,
        tracked(
            "/duplicate-jobs",
            "abc",
            std::slice::from_ref(&second),
            "second-chat",
            CiRepairPhase::Running,
            NOW + 1,
        ),
    );
    h.set_repairs(both);
    assert_eq!(h.cards().len(), 2);
}

#[gpui::test]
fn reveals_and_expands_the_repaired_check(cx: &mut TestAppContext) {
    let linux = with_url(
        ci("Unit tests / Linux", GithubPrCheckState::Fail),
        "https://github.com/acme/web/actions/runs/1/job/1",
    );
    let windows = with_url(
        ci("Unit tests / Windows", GithubPrCheckState::Fail),
        "https://github.com/acme/web/actions/runs/1/job/2",
    );
    let services = FakeServices::new(NOW);
    let repair = repair(42, Vec::new(), &services);
    let mut h = mount(
        cx,
        services.clone(),
        checks_state("old", vec![linux.clone(), windows.clone()]),
        "/single-followup",
        "acme/web",
        Some(repair),
    );
    assert!(h.row("Unit tests / Linux").expanded);
    h.set_repairs(vec![tracked(
        "/single-followup",
        "old",
        std::slice::from_ref(&linux),
        "linux-chat",
        CiRepairPhase::Completed,
        NOW,
    )]);
    let mut passed = linux.clone();
    passed.state = GithubPrCheckState::Pass;
    passed.url = Some("https://github.com/acme/web/actions/runs/2/job/3".into());
    passed.started_at = Some("2026-09-30T15:00:01Z".into());
    h.set(checks_state("new", vec![passed, windows]));
    let cards = h.cards();
    assert_eq!(cards[0].1.subtitle, "Unit tests / Linux");
    assert_eq!(cards[0].1.clauses[0].1, "CI passed");
    assert!(cards[0].1.can_show_check);
    assert!(!h.row("Unit tests / Windows").expanded);
    // The passing row sits in the hidden group until "Show check".
    assert!(
        !h.view
            .read_with(h.cx, |view, _| view.visible_names())
            .contains(&"Unit tests / Linux".to_string())
    );
    h.view.update(h.cx, |view, cx| {
        view.show_check("Unit tests / Linux", "CI", cx)
    });
    draw(h.cx);
    assert_eq!(
        h.view.read_with(h.cx, |view, _| view.filter()),
        ChecksFilter::All
    );
    assert!(h.row("Unit tests / Linux").expanded);
    h.toggle("Unit tests / Linux");
    assert!(!h.row("Unit tests / Linux").expanded);
    h.view.update(h.cx, |view, cx| {
        view.show_check("Unit tests / Linux", "CI", cx)
    });
    draw(h.cx);
    assert!(h.row("Unit tests / Linux").expanded);
}

#[gpui::test]
fn collapses_a_batch_into_one_conversation_card_and_keeps_results_in_the_check_rows(
    cx: &mut TestAppContext,
) {
    let checks: Vec<GithubPrCheck> = (1..=10)
        .map(|index| ci(&format!("Test {index}"), GithubPrCheckState::Fail))
        .collect();
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().repairs = vec![tracked(
        "/batch",
        "old",
        &checks,
        "batch-chat",
        CiRepairPhase::Running,
        NOW,
    )];
    let repair = repair(42, Vec::new(), &services);
    let mut h = mount(
        cx,
        services.clone(),
        checks_state("old", checks.clone()),
        "/batch",
        "acme/web",
        Some(repair),
    );
    let cards = h.cards();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].1.label, "10 checks");
    assert_eq!(cards[0].1.subtitle, "CI repair for 10 checks");
    let finished = vec![tracked(
        "/batch",
        "old",
        &checks,
        "batch-chat",
        CiRepairPhase::Completed,
        NOW,
    )];
    h.set_repairs(finished);
    let next: Vec<GithubPrCheck> = checks
        .iter()
        .enumerate()
        .map(|(index, check)| GithubPrCheck {
            state: if index < 8 {
                GithubPrCheckState::Pass
            } else {
                GithubPrCheckState::Fail
            },
            started_at: Some("2026-09-30T15:00:01Z".into()),
            ..check.clone()
        })
        .collect();
    h.set(checks_state("new", next));
    let cards = h.cards();
    let clauses: Vec<String> = cards[0]
        .1
        .clauses
        .iter()
        .map(|(_, text)| text.clone())
        .collect();
    assert!(clauses.contains(&"8 passed".to_string()));
    assert!(clauses.contains(&"2 still failing".to_string()));
    assert_eq!(h.row("Test 9").repair.as_deref(), Some("Still failing"));
    h.view
        .update(h.cx, |view, cx| view.set_filter(ChecksFilter::All, cx));
    assert_eq!(h.row("Test 1").repair.as_deref(), Some("CI passed"));
}

#[gpui::test]
fn shows_live_repair_progress_and_then_waits_for_ci(cx: &mut TestAppContext) {
    let failing = ci("tests", GithubPrCheckState::Fail);
    let services = FakeServices::new(NOW);
    let sessions = vec![RelatedSession {
        id: "repair-chat".into(),
        title: "Fix tests".into(),
        archived: false,
    }];
    let repair = repair(42, sessions, &services);
    let mut h = mount(
        cx,
        services.clone(),
        checks_state("abc", vec![failing.clone()]),
        "/progress",
        "acme/web",
        Some(repair),
    );
    h.set_repairs(vec![tracked(
        "/progress",
        "abc",
        std::slice::from_ref(&failing),
        "repair-chat",
        CiRepairPhase::Running,
        NOW,
    )]);
    assert_eq!(h.cards()[0].1.clauses[0].1, "Repair in progress");
    h.set_repairs(vec![tracked(
        "/progress",
        "abc",
        std::slice::from_ref(&failing),
        "repair-chat",
        CiRepairPhase::Completed,
        NOW,
    )]);
    assert_eq!(h.cards()[0].1.clauses[0].1, "Awaiting new GitHub checks");
}

#[gpui::test]
fn starts_a_repair_with_all_failed_checks_and_the_selected_project_chat(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let sessions = vec![
        RelatedSession {
            id: "chat1".into(),
            title: "Repair tests".into(),
            archived: false,
        },
        RelatedSession {
            id: "chat2".into(),
            title: "Unrelated work".into(),
            archived: false,
        },
    ];
    let repair = repair(42, sessions, &services);
    let h = mount(
        cx,
        services.clone(),
        checks_state(
            "abc123",
            vec![
                ci("lint", GithubPrCheckState::Fail),
                ci("tests", GithubPrCheckState::Fail),
                ci("build", GithubPrCheckState::Pass),
            ],
        ),
        "/web",
        "acme/web",
        Some(repair),
    );
    h.cx.update(|window, cx| {
        h.view.update(cx, |view, cx| view.fix_all(window, cx));
    });
    draw(h.cx);
    let form = h
        .view
        .read_with(h.cx, |view, _| view.form().cloned())
        .expect("form open");
    h.cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.set_query("Repair", window, cx);
            form.step(true, cx);
            form.pick_active(cx);
            form.start(cx);
        })
    });
    draw(h.cx);
    let starts = services.state.borrow().repair_starts.clone();
    assert_eq!(starts.len(), 1);
    let (start, session) = &starts[0];
    assert_eq!(session.as_deref(), Some("chat1"));
    assert_eq!(start.repo, "acme/web");
    assert_eq!(start.number, 42);
    assert_eq!(start.head_oid, "abc123");
    let names: Vec<&str> = start
        .evidence
        .iter()
        .map(|evidence| evidence.check.name.as_str())
        .collect();
    assert_eq!(names, ["lint", "tests"]);
}

#[gpui::test]
fn does_not_start_a_repair_after_leaving_checks_while_details_load(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().hold_check_details = true;
    let repair = repair(42, Vec::new(), &services);
    let lint = with_url(
        ci("lint", GithubPrCheckState::Fail),
        "https://github.com/acme/web/actions/runs/1/job/2",
    );
    let mut h = mount(
        cx,
        services.clone(),
        checks_state("abc", vec![lint]),
        "",
        "acme/web",
        Some(repair),
    );
    h.cx.update(|window, cx| h.view.update(cx, |view, cx| view.fix_all(window, cx)));
    draw(h.cx);
    let form = h
        .view
        .read_with(h.cx, |view, _| view.form().cloned())
        .unwrap();
    form.update(h.cx, |form, cx| form.start(cx));
    draw(h.cx);
    // Leaving the checks closes the form.
    h.view
        .update(h.cx, |view, cx| view.set_filter(ChecksFilter::All, cx));
    drop(form);
    draw(h.cx);
    assert!(!h.form_open());
    let held = std::mem::take(&mut services.state.borrow_mut().held_check_details);
    for sender in held {
        let _ = sender.send(Ok(GithubCheckDetails::default()));
    }
    draw(h.cx);
    assert!(services.state.borrow().repair_starts.is_empty());
}

#[gpui::test]
fn prepares_several_ci_jobs_concurrently_before_starting_a_repair(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().hold_check_details = true;
    let repair = repair(42, Vec::new(), &services);
    let checks: Vec<GithubPrCheck> = (0..4)
        .map(|index| {
            with_url(
                ci(&format!("tests-{index}"), GithubPrCheckState::Fail),
                &format!(
                    "https://github.com/acme/web/actions/runs/1/job/{}",
                    index + 1
                ),
            )
        })
        .collect();
    let h = mount(
        cx,
        services.clone(),
        checks_state("abc", checks),
        "",
        "acme/web",
        Some(repair),
    );
    h.cx.update(|window, cx| h.view.update(cx, |view, cx| view.fix_all(window, cx)));
    draw(h.cx);
    let form = h
        .view
        .read_with(h.cx, |view, _| view.form().cloned())
        .unwrap();
    form.update(h.cx, |form, cx| form.start(cx));
    draw(h.cx);
    assert_eq!(services.state.borrow().held_check_details.len(), 3);
    let first = services.state.borrow_mut().held_check_details.remove(0);
    let _ = first.send(Ok(GithubCheckDetails::default()));
    draw(h.cx);
    assert_eq!(services.state.borrow().held_check_details.len(), 3);
    assert_eq!(
        services
            .calls()
            .iter()
            .filter(|call| call.starts_with("fetch_check_details"))
            .count(),
        4
    );
    let rest = std::mem::take(&mut services.state.borrow_mut().held_check_details);
    for sender in rest {
        let _ = sender.send(Ok(GithubCheckDetails::default()));
    }
    draw(h.cx);
    assert_eq!(services.state.borrow().repair_starts.len(), 1);
}

#[gpui::test]
fn closes_the_repair_selection_when_results_change(cx: &mut TestAppContext) {
    let failed = ci("lint", GithubPrCheckState::Fail);
    let services = FakeServices::new(NOW);
    let repair = repair(42, Vec::new(), &services);
    let mut h = mount(
        cx,
        services,
        checks_state("abc", vec![failed.clone()]),
        "/web",
        "acme/web",
        Some(repair),
    );
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.fix_named("lint", window, cx))
    });
    draw(h.cx);
    assert!(h.form_open());
    h.set(checks_state(
        "abc",
        vec![GithubPrCheck {
            state: GithubPrCheckState::Pass,
            ..failed
        }],
    ));
    assert!(!h.form_open());
}

#[gpui::test]
fn closes_the_repair_selection_when_the_pr_changes(cx: &mut TestAppContext) {
    let failed = ci("lint", GithubPrCheckState::Fail);
    let services = FakeServices::new(NOW);
    let first = repair(42, Vec::new(), &services);
    let next = repair(43, Vec::new(), &services);
    let mut h = mount(
        cx,
        services,
        checks_state("abc", vec![failed]),
        "/web",
        "acme/web",
        Some(first),
    );
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.fix_named("lint", window, cx))
    });
    draw(h.cx);
    assert!(h.form_open());
    h.view
        .update(h.cx, |view, cx| view.set_repair(Some(next), cx));
    draw(h.cx);
    assert!(!h.form_open());
}

fn keep_pending_repair(cx: &mut TestAppContext, failed_refresh: bool) {
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().hold_check_details = true;
    let repair = repair(42, Vec::new(), &services);
    let lint = with_url(
        ci("lint", GithubPrCheckState::Fail),
        "https://github.com/acme/web/actions/runs/1/job/2",
    );
    let state = checks_state("abc", vec![lint]);
    let mut h = mount(
        cx,
        services.clone(),
        state.clone(),
        "",
        "acme/web",
        Some(repair),
    );
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.fix_named("lint", window, cx))
    });
    draw(h.cx);
    let form = h
        .view
        .read_with(h.cx, |view, _| view.form().cloned())
        .unwrap();
    form.update(h.cx, |form, cx| form.start(cx));
    draw(h.cx);
    h.set(PrChecksState {
        refreshing: !failed_refresh,
        stale: failed_refresh,
        error: failed_refresh.then(|| "network down".to_string()),
        ..state
    });
    assert!(h.form_open());
    let held = std::mem::take(&mut services.state.borrow_mut().held_check_details);
    for sender in held {
        let _ = sender.send(Ok(GithubCheckDetails::default()));
    }
    draw(h.cx);
    assert_eq!(services.state.borrow().repair_starts.len(), 1);
}

#[gpui::test]
fn keeps_a_pending_repair_through_a_refreshing_checks_refresh(cx: &mut TestAppContext) {
    keep_pending_repair(cx, false);
}

#[gpui::test]
fn keeps_a_pending_repair_through_a_failed_checks_refresh(cx: &mut TestAppContext) {
    keep_pending_repair(cx, true);
}

#[gpui::test]
fn keeps_the_selected_failed_job_when_another_check_changes(cx: &mut TestAppContext) {
    let failed = ci("lint", GithubPrCheckState::Fail);
    let other = ci("build", GithubPrCheckState::Pending);
    let services = FakeServices::new(NOW);
    let repair = repair(42, Vec::new(), &services);
    let mut h = mount(
        cx,
        services,
        checks_state("abc", vec![failed.clone(), other.clone()]),
        "/web",
        "acme/web",
        Some(repair),
    );
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.fix_named("lint", window, cx))
    });
    draw(h.cx);
    h.set(checks_state(
        "abc",
        vec![
            failed,
            GithubPrCheck {
                state: GithubPrCheckState::Pass,
                ..other
            },
        ],
    ));
    assert!(h.form_open());
}
