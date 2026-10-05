//! Port of src/features/inbox/hooks/useGithubPrChecks.test.ts.

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Entity, TestAppContext};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::backend::fake::FakeBackend;
use super::client::InboxClient;
use super::pr_checks::{PrChecks, PrChecksParams};
use crate::runtime::testing::{TestWorkspace, init_test_engine_with};
use crate::runtime::{EngineHooks, WorkspaceHooks};

fn checks(head_oid: &str, states: &[&str]) -> Value {
    json!({
        "headOid": head_oid,
        "checks": states.iter().enumerate().map(|(index, state)| json!({
            "name": format!("{state}-{index}"),
            "workflow": "CI",
            "state": state,
            "url": null,
            "startedAt": null,
            "completedAt": null,
        })).collect::<Vec<_>>(),
    })
}

/// `mockResolvedValue` with a queue of `mockResolvedValueOnce` answers.
#[derive(Default)]
struct Answers {
    once: VecDeque<Result<Value, String>>,
    always: Option<Result<Value, String>>,
}

struct Setup {
    backend: Arc<FakeBackend>,
    answers: Arc<Mutex<Answers>>,
    workspace: Rc<TestWorkspace>,
    client: InboxClient,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let workspace = TestWorkspace::new();
    let hooks = EngineHooks {
        workspace: workspace.clone() as Rc<dyn WorkspaceHooks>,
        ..EngineHooks::default()
    };
    init_test_engine_with(cx, hooks);
    let answers = Arc::new(Mutex::new(Answers::default()));
    let script = answers.clone();
    let backend = FakeBackend::new(move |_, _| {
        let mut answers = script.lock();
        answers
            .once
            .pop_front()
            .or_else(|| answers.always.clone())
            .unwrap_or_else(|| Ok(checks("default", &[])))
    });
    let client = InboxClient::new(
        backend.clone(),
        monocode_settings::Kv::in_memory(),
        cx.executor(),
    );
    Setup {
        backend,
        answers,
        workspace,
        client,
    }
}

impl Setup {
    fn always(&self, value: Value) {
        self.answers.lock().always = Some(Ok(value));
    }

    fn once(&self, answer: Result<Value, String>) {
        self.answers.lock().once.push_back(answer);
    }

    fn calls(&self) -> usize {
        self.backend.count("git_github_pr_checks")
    }

    fn render(&self, cx: &mut TestAppContext, params: PrChecksParams) -> Entity<PrChecks> {
        let client = self.client.clone();
        cx.new(|cx| PrChecks::new(client, params, cx))
    }

    fn set_hidden(&self, hidden: bool) {
        *self.workspace.hidden.borrow_mut() = hidden;
    }
}

fn base() -> PrChecksParams {
    PrChecksParams {
        cwd: "/tmp/web".into(),
        repo: "acme/web".into(),
        number: 7,
        enabled: true,
        open: true,
        poll: true,
        revision: 0,
    }
}

fn rerender(view: &Entity<PrChecks>, cx: &mut TestAppContext, params: PrChecksParams) {
    view.update(cx, |view, cx| view.set_params(params, cx));
    cx.run_until_parked();
}

fn advance(cx: &mut TestAppContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}

fn head(view: &Entity<PrChecks>, cx: &mut TestAppContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        view.checks().map(|checks| checks.head_oid.clone())
    })
}

fn refresh(view: &Entity<PrChecks>, cx: &mut TestAppContext) {
    view.update(cx, |view, cx| view.refresh(cx));
}

#[gpui::test]
fn loads_once_on_mount_for_open_prs_whatever_the_revision_is(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &["pass"]));
    let view = s.render(cx, base());
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    assert_eq!(
        s.backend.calls_to("git_github_pr_checks"),
        [json!({ "cwd": "/tmp/web", "repo": "acme/web", "number": 7 })]
    );
    view.read_with(cx, |view, _| {
        assert!(!view.loading());
        assert_eq!(view.checks().unwrap().head_oid, "a");
        assert!(!view.stale());
        assert_eq!(view.error(), None);
    });
}

#[gpui::test]
fn keeps_initial_loading_distinct_from_an_answered_no_checks_state(cx: &mut TestAppContext) {
    let s = setup(cx);
    let pending = s.backend.hold_next("git_github_pr_checks");
    let view = s.render(cx, base());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.loading());
        assert!(view.checks().is_none());
    });
    pending.resolve(json!({ "headOid": "a", "checks": [] }));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.loading());
        assert!(!view.refreshing());
        assert_eq!(view.checks().unwrap().head_oid, "a");
        assert!(view.checks().unwrap().checks.is_empty());
    });
}

#[gpui::test]
fn polls_every_30_seconds_for_open_prs(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &[]));
    let _view = s.render(cx, base());
    cx.run_until_parked();
    advance(cx, 29_999);
    assert_eq!(s.calls(), 1);
    advance(cx, 1);
    assert_eq!(s.calls(), 2);
}

#[gpui::test]
fn pauses_polling_for_a_hidden_pr_panel_and_refreshes_when_shown(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &[]));
    let view = s.render(
        cx,
        PrChecksParams {
            poll: false,
            ..base()
        },
    );
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    advance(cx, 90_000);
    assert_eq!(s.calls(), 1);
    rerender(&view, cx, base());
    assert_eq!(s.calls(), 2);
    rerender(
        &view,
        cx,
        PrChecksParams {
            poll: false,
            ..base()
        },
    );
    advance(cx, 60_000);
    assert_eq!(s.calls(), 2);
}

#[gpui::test]
fn skips_hidden_ticks_and_refreshes_when_the_window_becomes_visible_again(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &[]));
    let view = s.render(cx, base());
    cx.run_until_parked();
    s.set_hidden(true);
    advance(cx, 90_000);
    assert_eq!(s.calls(), 1);
    s.set_hidden(false);
    view.update(cx, |view, cx| view.window_became_visible(cx));
    cx.run_until_parked();
    assert_eq!(s.calls(), 2);
}

#[gpui::test]
fn loads_closed_prs_only_on_mount_and_on_manual_refresh(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &[]));
    let view = s.render(
        cx,
        PrChecksParams {
            open: false,
            ..base()
        },
    );
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    advance(cx, 90_000);
    view.update(cx, |view, cx| view.window_became_visible(cx));
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    refresh(&view, cx);
    cx.run_until_parked();
    assert_eq!(s.calls(), 2);
}

#[gpui::test]
fn keeps_previous_results_after_a_failed_refresh_but_marks_them_stale(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.once(Ok(checks("a", &["pass"])));
    let view = s.render(cx, base());
    cx.run_until_parked();
    s.once(Err("network down".into()));
    advance(cx, 30_000);
    view.read_with(cx, |view, _| {
        assert!(view.stale());
        assert_eq!(view.error(), Some("network down"));
        assert_eq!(view.checks().unwrap().head_oid, "a");
    });
    s.once(Ok(checks("b", &["fail"])));
    advance(cx, 30_000);
    view.read_with(cx, |view, _| {
        assert!(!view.stale());
        assert_eq!(view.error(), None);
        assert_eq!(view.checks().unwrap().head_oid, "b");
    });
}

#[gpui::test]
fn drops_a_late_answer_after_the_pr_changed_and_replaces_with_the_new_head(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    let first = s.backend.hold_next("git_github_pr_checks");
    let second = s.backend.hold_next("git_github_pr_checks");
    let view = s.render(cx, base());
    cx.run_until_parked();
    rerender(
        &view,
        cx,
        PrChecksParams {
            number: 8,
            ..base()
        },
    );
    assert_eq!(
        s.backend.calls_to("git_github_pr_checks")[1],
        json!({ "cwd": "/tmp/web", "repo": "acme/web", "number": 8 })
    );
    first.resolve(checks("old", &["fail"]));
    cx.run_until_parked();
    assert_eq!(head(&view, cx), None);
    second.resolve(checks("new", &["pass"]));
    cx.run_until_parked();
    assert_eq!(head(&view, cx).as_deref(), Some("new"));
    view.read_with(cx, |view, _| {
        let states: Vec<_> = view
            .checks()
            .unwrap()
            .checks
            .iter()
            .map(|check| check.state)
            .collect();
        assert_eq!(states, [super::types::GithubPrCheckState::Pass]);
    });
}

#[gpui::test]
fn coalesces_a_revision_refresh_that_lands_mid_flight_into_one_follow_up(cx: &mut TestAppContext) {
    let s = setup(cx);
    let first = s.backend.hold_next("git_github_pr_checks");
    let view = s.render(cx, base());
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    s.always(checks("rev2", &[]));
    rerender(
        &view,
        cx,
        PrChecksParams {
            revision: 1,
            ..base()
        },
    );
    assert_eq!(s.calls(), 1);
    first.resolve(checks("rev1", &[]));
    cx.run_until_parked();
    assert_eq!(s.calls(), 2);
    view.read_with(cx, |view, _| {
        assert_eq!(view.checks().unwrap().head_oid, "rev2");
        assert!(!view.loading());
        assert!(!view.refreshing());
    });
}

#[gpui::test]
fn coalesces_repeated_manual_refreshes_during_a_request_into_one_retry(cx: &mut TestAppContext) {
    let s = setup(cx);
    let first = s.backend.hold_next("git_github_pr_checks");
    let view = s.render(cx, base());
    cx.run_until_parked();
    refresh(&view, cx);
    refresh(&view, cx);
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    s.always(checks("b", &[]));
    first.reject("stale");
    cx.run_until_parked();
    assert_eq!(s.calls(), 2);
    view.read_with(cx, |view, _| {
        assert_eq!(view.checks().unwrap().head_oid, "b");
        assert!(!view.stale());
    });
}

#[gpui::test]
fn does_not_launch_a_queued_poll_while_the_window_is_hidden(cx: &mut TestAppContext) {
    let s = setup(cx);
    let pending = s.backend.hold_next("git_github_pr_checks");
    let view = s.render(cx, base());
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    // The tick lands mid-flight and queues an automatic follow-up.
    advance(cx, 30_000);
    assert_eq!(s.calls(), 1);
    s.set_hidden(true);
    pending.resolve(checks("a", &[]));
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    // Becoming visible picks the polling back up.
    s.set_hidden(false);
    s.always(checks("a", &[]));
    view.update(cx, |view, cx| view.window_became_visible(cx));
    cx.run_until_parked();
    assert_eq!(s.calls(), 2);
    assert_eq!(head(&view, cx).as_deref(), Some("a"));
}

#[gpui::test]
fn does_not_launch_a_queued_poll_after_the_pr_closes_but_manual_refresh_still_works(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    let pending = s.backend.hold_next("git_github_pr_checks");
    let view = s.render(cx, base());
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    advance(cx, 30_000);
    rerender(
        &view,
        cx,
        PrChecksParams {
            open: false,
            ..base()
        },
    );
    pending.resolve(checks("a", &[]));
    cx.run_until_parked();
    assert_eq!(s.calls(), 1);
    s.always(checks("b", &[]));
    refresh(&view, cx);
    cx.run_until_parked();
    assert_eq!(s.calls(), 2);
    assert_eq!(head(&view, cx).as_deref(), Some("b"));
}

#[gpui::test]
fn never_lets_requests_overlap_and_drops_a_queued_follow_up_on_unmount(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &[]));
    let view = s.render(cx, base());
    refresh(&view, cx);
    cx.run_until_parked();
    // The refresh coalesced behind the mount load, then ran on its own.
    assert_eq!(s.calls(), 2);

    let pending = s.backend.hold_next("git_github_pr_checks");
    advance(cx, 30_000);
    assert_eq!(s.calls(), 3);
    refresh(&view, cx);
    cx.run_until_parked();
    drop(view);
    cx.run_until_parked();
    pending.resolve(checks("late", &[]));
    cx.run_until_parked();
    assert_eq!(s.calls(), 3);
}

#[gpui::test]
fn disabling_clears_results_and_stops_polling(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.always(checks("a", &["pass"]));
    let view = s.render(cx, base());
    cx.run_until_parked();
    rerender(
        &view,
        cx,
        PrChecksParams {
            enabled: false,
            ..base()
        },
    );
    view.read_with(cx, |view, _| {
        assert!(view.checks().is_none());
        assert!(!view.loading());
    });
    advance(cx, 90_000);
    assert_eq!(s.calls(), 1);
}
