//! Port of src/features/inbox/ui/InboxPrChecks.test.ts (the checks panel
//! and its tab).

use std::rc::Rc;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{draw, init};
use crate::data::*;
use crate::fixtures::{FakeChecks, FakeServices, NOW};
use crate::model::ChecksOverall;
use crate::pr::checks::{ChecksFilter, PrChecksTab, PrChecksView};
use crate::pr::repair_form::CheckRepair;

pub fn check(name: &str, state: GithubPrCheckState) -> GithubPrCheck {
    GithubPrCheck {
        name: name.into(),
        workflow: "Build".into(),
        state,
        url: None,
        started_at: None,
        completed_at: None,
    }
}

pub fn with_url(mut check: GithubPrCheck, url: &str) -> GithubPrCheck {
    check.url = Some(url.into());
    check
}

pub fn checks_state(head: &str, checks: Vec<GithubPrCheck>) -> PrChecksState {
    PrChecksState {
        checks: Some(GithubPrChecks {
            head_oid: head.into(),
            checks,
        }),
        ..Default::default()
    }
}

pub struct Harness<'a> {
    pub services: Rc<FakeServices>,
    pub data: FakeChecks,
    pub view: Entity<PrChecksView>,
    pub cx: &'a mut VisualTestContext,
}

pub fn mount<'a>(
    cx: &'a mut TestAppContext,
    services: Rc<FakeServices>,
    state: PrChecksState,
    cwd: &str,
    repo: &str,
    repair: Option<CheckRepair>,
) -> Harness<'a> {
    cx.update(init);
    let data = FakeChecks::new(state);
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let data_dyn: Rc<dyn PrChecksData> = Rc::new(data.clone());
    let cwd = cwd.to_string();
    let repo = repo.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut view = PrChecksView::new(services_dyn, data_dyn, cwd, repo, repair, cx);
        view.set_animate(false, cx);
        view
    });
    draw(cx);
    Harness {
        services,
        data,
        view,
        cx,
    }
}

impl Harness<'_> {
    pub fn set(&mut self, state: PrChecksState) {
        let data = self.data.clone();
        self.cx.update(|_, cx| data.set(state, cx));
        draw(self.cx);
    }

    pub fn row(&mut self, name: &str) -> crate::pr::checks::CheckRowInfo {
        self.view
            .read_with(self.cx, |view, _| view.row_info(name))
            .unwrap_or_else(|| panic!("no row {name}"))
    }

    pub fn toggle(&mut self, name: &str) {
        let name = name.to_string();
        self.view
            .update(self.cx, |view, cx| view.toggle_named(&name, cx));
        draw(self.cx);
    }

    pub fn calls(&self) -> Vec<String> {
        self.services.calls()
    }
}

const JOB_2: &str = "https://github.com/acme/web/actions/runs/1/job/2";
const JOB_123: &str = "https://github.com/acme/web/actions/runs/9/job/123";

#[gpui::test]
fn keeps_expanded_details_on_the_same_check_when_checks_share_a_url(cx: &mut TestAppContext) {
    let first = with_url(check("build", GithubPrCheckState::Pass), JOB_2);
    let second = GithubPrCheck {
        name: "lint".into(),
        ..first.clone()
    };
    let services = FakeServices::new(NOW);
    let mut h = mount(
        cx,
        services,
        checks_state("abc", vec![first.clone(), second.clone()]),
        "/tmp/web",
        "acme/web",
        None,
    );
    h.toggle("build");
    h.set(checks_state("abc", vec![second, first]));
    assert!(h.row("build").expanded);
    assert!(!h.row("lint").expanded);
}

#[gpui::test]
fn filters_attention_checks_without_hiding_cancelled_or_unknown_outcomes(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let h = mount(
        cx,
        services,
        checks_state(
            "abc",
            vec![
                check("Tests", GithubPrCheckState::Fail),
                check("Build", GithubPrCheckState::Pending),
                check("Deploy", GithubPrCheckState::Cancel),
                check("External scan", GithubPrCheckState::Unknown),
                check("Lint", GithubPrCheckState::Pass),
                check("Publish", GithubPrCheckState::Skipping),
            ],
        ),
        "",
        "",
        None,
    );
    let names = |h: &Harness<'_>| h.view.read_with(h.cx, |view, _| view.visible_names());
    assert_eq!(
        h.view.read_with(h.cx, |view, _| view.filter()),
        ChecksFilter::Attention
    );
    assert_eq!(names(&h), ["Tests", "Build", "Deploy", "External scan"]);
    h.view
        .update(h.cx, |view, cx| view.set_filter(ChecksFilter::All, cx));
    assert_eq!(
        names(&h),
        [
            "Tests",
            "Build",
            "Deploy",
            "External scan",
            "Lint",
            "Publish"
        ]
    );
    h.view.update(h.cx, |view, cx| {
        view.set_filter(ChecksFilter::Attention, cx)
    });
    assert_eq!(names(&h), ["Tests", "Build", "Deploy", "External scan"]);
}

#[gpui::test]
fn shows_annotation_source_from_the_checked_commit_and_links_to_that_same_revision(
    cx: &mut TestAppContext,
) {
    let head = "a".repeat(40);
    let services = FakeServices::new(NOW);
    {
        let mut state = services.state.borrow_mut();
        state.check_details.insert(
            "123".into(),
            Ok(GithubCheckDetails {
                steps: Vec::new(),
                annotations: vec![GithubCheckAnnotation {
                    path: "src/preview test.ts".into(),
                    line: 2,
                    level: "failure".into(),
                    message: "Assertion failed\nExpected: 200\nReceived: 500".into(),
                }],
                notice: None,
            }),
        );
        state.files.insert(
            "src/preview test.ts".into(),
            "const status = response.status;\nexpect(status).toBe(200);\nfinish();".into(),
        );
    }
    let mut h = mount(
        cx,
        services,
        checks_state(
            &head,
            vec![with_url(check("Tests", GithubPrCheckState::Fail), JOB_123)],
        ),
        "/tmp/web",
        "acme/web",
        None,
    );
    let row = h.row("Tests");
    assert!(row.expanded);
    assert_eq!(row.annotations, 1);
    assert!(h.calls().contains(&format!(
        "commit_file_text /tmp/web {head} src/preview test.ts"
    )));
}

#[gpui::test]
fn opens_the_only_failed_actions_job_and_shows_its_failed_step_and_error(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().check_details.insert(
        "123".into(),
        Ok(GithubCheckDetails {
            steps: vec![
                GithubCheckStep {
                    name: "Install".into(),
                    state: GithubPrCheckState::Pass,
                    started_at: None,
                    completed_at: None,
                },
                GithubCheckStep {
                    name: "Run tests".into(),
                    state: GithubPrCheckState::Fail,
                    started_at: Some("2030-01-01T10:00:00Z".into()),
                    completed_at: Some("2030-01-01T10:00:12Z".into()),
                },
            ],
            annotations: vec![GithubCheckAnnotation {
                path: "src/app.test.ts".into(),
                line: 42,
                message: "Expected 2, received 1".into(),
                level: "failure".into(),
            }],
            notice: None,
        }),
    );
    let mut h = mount(
        cx,
        services,
        checks_state(
            "abc",
            vec![with_url(
                check("Windows", GithubPrCheckState::Fail),
                JOB_123,
            )],
        ),
        "/tmp/web",
        "acme/web",
        None,
    );
    assert!(
        h.calls()
            .contains(&"fetch_check_details /tmp/web acme/web 123".to_string())
    );
    let row = h.row("Windows");
    assert!(row.expanded);
    assert_eq!(row.subtitle.as_deref(), Some("Expected 2, received 1"));
    assert_eq!(row.steps, ["Install", "Run tests"]);
    h.toggle("Windows");
    let row = h.row("Windows");
    assert!(!row.expanded);
    assert_eq!(row.subtitle.as_deref(), Some("Expected 2, received 1"));
}

#[gpui::test]
fn keeps_expanded_evidence_while_polling_a_pending_job_and_shows_new_steps(
    cx: &mut TestAppContext,
) {
    let head = "a".repeat(40);
    let details = GithubCheckDetails {
        steps: vec![GithubCheckStep {
            name: "Install".into(),
            state: GithubPrCheckState::Pass,
            started_at: None,
            completed_at: None,
        }],
        annotations: (1..=6)
            .map(|index| GithubCheckAnnotation {
                path: "src/app.ts".into(),
                line: 1,
                message: format!("Annotation {index}"),
                level: "failure".into(),
            })
            .collect(),
        notice: None,
    };
    let services = FakeServices::new(NOW);
    {
        let mut state = services.state.borrow_mut();
        state
            .check_details
            .insert("123".into(), Ok(details.clone()));
        state
            .files
            .insert("src/app.ts".into(), "source preview".into());
    }
    let job = with_url(check("Windows", GithubPrCheckState::Pending), JOB_123);
    let state = checks_state(&head, vec![job]);
    let mut h = mount(
        cx,
        services.clone(),
        state.clone(),
        "/tmp/web",
        "acme/web",
        None,
    );
    h.toggle("Windows");
    let name = "Windows".to_string();
    h.view
        .update(h.cx, |view, cx| view.show_all_annotations(&name, cx));
    draw(h.cx);
    assert_eq!(h.row("Windows").shown_annotations, 6);

    // A poll: same answer, new load. The details stay while the job reloads.
    services.state.borrow_mut().hold_check_details = true;
    h.set(PrChecksState {
        generation: 1,
        ..state
    });
    let row = h.row("Windows");
    assert!(row.has_details);
    assert!(row.loading);
    assert_eq!(row.shown_annotations, 6);
    let sender = services
        .state
        .borrow_mut()
        .held_check_details
        .pop()
        .expect("a held request");
    let mut next = details;
    next.steps.push(GithubCheckStep {
        name: "Run tests".into(),
        state: GithubPrCheckState::Pending,
        started_at: None,
        completed_at: None,
    });
    let _ = sender.send(Ok(next));
    draw(h.cx);
    let row = h.row("Windows");
    assert_eq!(row.steps, ["Install", "Run tests"]);
    assert_eq!(row.shown_annotations, 6);
    let reads = h
        .calls()
        .iter()
        .filter(|call| call.starts_with("commit_file_text"))
        .count();
    assert_eq!(reads, 1);
}

#[gpui::test]
fn clears_the_old_failed_step_when_a_collapsed_job_is_refreshed(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().check_details.insert(
        "123".into(),
        Ok(GithubCheckDetails {
            steps: vec![GithubCheckStep {
                name: "Old failure".into(),
                state: GithubPrCheckState::Fail,
                started_at: None,
                completed_at: None,
            }],
            annotations: Vec::new(),
            notice: None,
        }),
    );
    let job = with_url(check("Windows", GithubPrCheckState::Fail), JOB_123);
    let mut h = mount(
        cx,
        services,
        checks_state("abc", vec![job.clone()]),
        "/tmp/web",
        "acme/web",
        None,
    );
    h.toggle("Windows");
    assert_eq!(
        h.row("Windows").subtitle.as_deref(),
        Some("Failed at Old failure")
    );
    h.set(checks_state(
        "abc",
        vec![GithubPrCheck {
            state: GithubPrCheckState::Pending,
            ..job
        }],
    ));
    let row = h.row("Windows");
    assert_eq!(row.subtitle, None);
    assert!(!row.expanded);
}

#[gpui::test]
fn opens_a_job_that_fails_during_polling_and_lets_the_user_retry_unavailable_details(
    cx: &mut TestAppContext,
) {
    let services = FakeServices::new(NOW);
    let job = with_url(
        check("Linux", GithubPrCheckState::Pending),
        "https://github.com/acme/web/actions/runs/9/job/124",
    );
    let mut h = mount(
        cx,
        services.clone(),
        checks_state("abc", vec![job.clone()]),
        "/tmp/web",
        "acme/web",
        None,
    );
    assert!(
        h.calls()
            .iter()
            .all(|call| !call.starts_with("fetch_check_details"))
    );
    services
        .state
        .borrow_mut()
        .check_details
        .insert("124".into(), Err("Request timed out".into()));
    h.set(checks_state(
        "abc",
        vec![GithubPrCheck {
            state: GithubPrCheckState::Fail,
            ..job
        }],
    ));
    let row = h.row("Linux");
    assert!(row.expanded);
    assert_eq!(row.error.as_deref(), Some("Request timed out"));
    services.state.borrow_mut().check_details.insert(
        "124".into(),
        Ok(GithubCheckDetails {
            steps: vec![GithubCheckStep {
                name: "Build".into(),
                state: GithubPrCheckState::Fail,
                started_at: None,
                completed_at: None,
            }],
            annotations: Vec::new(),
            notice: None,
        }),
    );
    h.view
        .update(h.cx, |view, cx| view.retry_named("Linux", cx));
    draw(h.cx);
    let row = h.row("Linux");
    assert_eq!(row.subtitle.as_deref(), Some("Failed at Build"));
    assert_eq!(row.error, None);
    assert_eq!(row.annotations, 0);
}

#[gpui::test]
fn renders_grouped_rows_with_workflow_status_duration_and_an_opener_link(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let mut skip = check("skip job", GithubPrCheckState::Skipping);
    skip.workflow = "Lint".into();
    let mut test = with_url(
        check("test job", GithubPrCheckState::Pass),
        "https://github.com/acme/web/actions/runs/9",
    );
    test.started_at = Some("2030-01-01T10:00:00Z".into());
    test.completed_at = Some("2030-01-01T10:01:05Z".into());
    let mut failed = check("failed job", GithubPrCheckState::Fail);
    failed.workflow = String::new();
    let mut h = mount(
        cx,
        services,
        checks_state("abc123", vec![skip, test, failed]),
        "",
        "",
        None,
    );
    h.view
        .update(h.cx, |view, cx| view.set_filter(ChecksFilter::All, cx));
    draw(h.cx);
    assert_eq!(
        h.view.read_with(h.cx, |view, _| view.visible_names()),
        ["failed job", "test job", "skip job"]
    );
    let test = h.row("test job");
    assert_eq!(test.meta, "Build · Passed · 1m 05s");
    assert!(test.linked);
    assert_eq!(test.title, "test job · Passed, took 1m 05s, Build");
    let failed = h.row("failed job");
    assert!(!failed.linked);
    assert_eq!(failed.meta, "Failed");
    assert_eq!(h.row("skip job").meta, "Lint · Skipped");
}

#[gpui::test]
fn keeps_rows_without_a_valid_http_url_unlinked(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let mut h = mount(
        cx,
        services,
        checks_state(
            "abc123",
            vec![
                with_url(check("ftp job", GithubPrCheckState::Pass), "ftp://ci/run/1"),
                check("no url job", GithubPrCheckState::Pass),
            ],
        ),
        "",
        "",
        None,
    );
    assert!(!h.row("ftp job").linked);
    assert!(!h.row("no url job").linked);
}

#[gpui::test]
fn separates_the_initial_loading_state_from_the_no_checks_state(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let mut h = mount(
        cx,
        services,
        PrChecksState {
            loading: true,
            ..Default::default()
        },
        "",
        "",
        None,
    );
    assert!(h.view.read_with(h.cx, |view, _| view.state().loading));
    h.set(checks_state("abc", Vec::new()));
    assert!(!h.view.read_with(h.cx, |view, _| view.state().loading));
    assert!(
        h.view
            .read_with(h.cx, |view, _| view.visible_names().is_empty())
    );
}

#[gpui::test]
fn offers_a_retry_after_a_load_error_and_refreshes_on_demand(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let h = mount(
        cx,
        services,
        PrChecksState {
            error: Some("rate limited".into()),
            ..Default::default()
        },
        "",
        "",
        None,
    );
    h.view.update(h.cx, |view, cx| view.refresh(cx));
    assert_eq!(*h.data.refreshes.borrow(), 1);
}

#[gpui::test]
fn marks_kept_results_as_out_of_date_after_a_failed_refresh(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let mut state = checks_state("abc", vec![check("ci/build", GithubPrCheckState::Pass)]);
    state.error = Some("rate limited".into());
    state.stale = true;
    let mut h = mount(cx, services, state, "", "", None);
    assert!(h.view.read_with(h.cx, |view, _| view.state().stale));
    assert_eq!(h.row("ci/build").meta, "Build · Passed");
}

#[test]
fn names_the_per_state_counts_on_the_tab_for_color_blind_safe_reading() {
    let fail = ChecksOverall::Fail {
        failed: 2,
        description: "2 failed, 1 passed".into(),
    };
    assert_eq!(PrChecksTab::label(&fail), "Checks: 2 failed, 1 passed");
    let neutral = ChecksOverall::Neutral {
        description: "3 skipped".into(),
    };
    assert_eq!(PrChecksTab::label(&neutral), "Checks: 3 skipped");
    let loading = ChecksOverall::Loading {
        description: "Loading checks".into(),
    };
    assert_eq!(PrChecksTab::label(&loading), "Checks: Loading checks");
}
