//! Port of src/features/inbox/ui/InboxView.test.ts (the item detail), the
//! "Checks tab user behavior" cases of InboxPrChecks.test.ts, and
//! LinkedWorkItemPanel.test.ts.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{draw, init};
use crate::data::*;
use crate::fixtures::{FakeChecks, FakeDetail, FakeServices, NOW};
use crate::model::{ChecksOverall, inbox_shows_full_file_diff};
use crate::pr::checks::PrChecksTab;
use crate::pr::detail::{DetailMode, DetailProps, DetailTab, InboxDetailEvent, InboxDetailView};
use crate::pr::linked_panel::{LinkedPanelEvent, LinkedPanelProps, LinkedWorkItemPanel};
use crate::style::{StatusTone, inbox_status_mark};

fn item() -> InboxItem {
    let mut item = InboxItem::github(InboxKind::Issue, "acme/web", 157, "A long inbox issue");
    item.updated_at = "2026-09-11T08:00:00Z".into();
    item.project_path = "/tmp/web".into();
    item
}

fn pr() -> InboxItem {
    let mut pr = item();
    pr.kind = InboxKind::Pr;
    pr.url = "https://github.com/acme/web/pull/157".into();
    pr
}

fn props(mode: DetailMode) -> DetailProps {
    DetailProps {
        cwd: "/tmp/web".into(),
        mode,
        visible: true,
        can_discuss: mode == DetailMode::Inbox,
        can_start: mode == DetailMode::Inbox,
        ..Default::default()
    }
}

fn related() -> Vec<RelatedSession> {
    vec![RelatedSession {
        id: "session-1".into(),
        title: "Review MonoCode Pull Request".into(),
        archived: false,
    }]
}

struct Harness<'a> {
    view: Entity<InboxDetailView>,
    events: Rc<RefCell<Vec<InboxDetailEvent>>>,
    cx: &'a mut VisualTestContext,
}

fn mount(
    cx: &mut TestAppContext,
    services: Rc<FakeServices>,
    item: InboxItem,
    props: DetailProps,
) -> Harness<'_> {
    cx.update(init);
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| {
        InboxDetailView::new(services_dyn, item, props, window, cx)
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &InboxDetailEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    Harness { view, events, cx }
}

impl Harness<'_> {
    fn header(&mut self) -> crate::pr::detail::DetailHeader {
        self.view.read_with(self.cx, |view, _| view.header())
    }

    fn overall(&mut self) -> Option<ChecksOverall> {
        self.view
            .read_with(self.cx, |view, _| view.checks_overall().cloned())
    }
}

#[test]
fn uses_githubs_purple_check_for_issues_closed_as_completed() {
    let mut completed = item();
    completed.state = "closed".into();
    completed.state_reason = Some("completed".into());
    let mark = inbox_status_mark(&completed);
    assert_eq!(mark.icon, monocode_ui::IconName::CheckCircle);
    assert_eq!(mark.tone, StatusTone::Merged);

    completed.state_reason = Some("not_planned".into());
    let mark = inbox_status_mark(&completed);
    assert_eq!(mark.icon, monocode_ui::IconName::CircleX);
    assert_eq!(mark.tone, StatusTone::Closed);
}

#[gpui::test]
fn shows_when_a_pr_was_created_alongside_its_last_update(cx: &mut TestAppContext) {
    let mut created = pr();
    created.created_at = Some("2026-09-01T08:00:00Z".into());
    let mut h = mount(
        cx,
        FakeServices::new(NOW),
        created,
        props(DetailMode::Inbox),
    );
    let header = h.header();
    assert_eq!(
        header.created.as_ref().map(|(iso, _)| iso.as_str()),
        Some("2026-09-01T08:00:00Z")
    );
    assert!(header.updated.is_some());
    let header = crate::pr::detail::DetailHeader::new(&pr(), &InboxDetailState::default(), NOW);
    assert!(header.created.is_none());
}

#[gpui::test]
fn keeps_issue_identity_and_actions_in_the_pinned_header(cx: &mut TestAppContext) {
    let mut local = item();
    local.project_path = "/tmp/local-project".into();
    let mut h = mount(cx, FakeServices::new(NOW), local, props(DetailMode::Inbox));
    let header = h.header();
    assert_eq!(header.kind_label, "Issue");
    assert_eq!(header.reference, "#157");
    assert_eq!(header.external_label, "Open on GitHub");
    assert!(header.unassigned);
    assert_eq!(header.source, "acme/web");
    assert!(!header.is_pr);
}

#[gpui::test]
fn adds_a_checks_tab_to_github_pull_requests_while_summary_stays_initial(cx: &mut TestAppContext) {
    let mut h = mount(cx, FakeServices::new(NOW), pr(), props(DetailMode::Inbox));
    assert!(h.header().is_pr);
    let overall = h.overall().expect("a checks tab");
    assert_eq!(PrChecksTab::label(&overall), "Checks: Loading checks");
    assert_eq!(
        h.view.read_with(h.cx, |view, _| view.tab()),
        DetailTab::Summary
    );
}

#[gpui::test]
fn keeps_the_checks_tab_off_issues(cx: &mut TestAppContext) {
    let mut h = mount(cx, FakeServices::new(NOW), item(), props(DetailMode::Inbox));
    assert!(h.overall().is_none());
}

#[gpui::test]
fn keeps_the_checks_tab_off_gitlab_merge_requests(cx: &mut TestAppContext) {
    let mut gitlab = pr();
    gitlab.provider = InboxProvider::Gitlab;
    gitlab.repo = "acme/platform".into();
    gitlab.url = "https://gitlab.example.com/acme/platform/-/merge_requests/12".into();
    let mut h = mount(cx, FakeServices::new(NOW), gitlab, props(DetailMode::Inbox));
    assert!(h.overall().is_none());
    let header = h.header();
    // GitLab merge requests get no GitHub lifecycle actions.
    assert!(!header.github_pr);
    assert!(header.is_pr);
    assert_eq!(header.kind_label, "Merge request");
    assert_eq!(header.external_label, "Review on GitLab");
}

#[gpui::test]
fn shows_the_checks_tab_for_linked_pull_requests_in_panel_mode(cx: &mut TestAppContext) {
    let mut h = mount(cx, FakeServices::new(NOW), pr(), props(DetailMode::Panel));
    let overall = h.overall().expect("a checks tab");
    assert_eq!(PrChecksTab::label(&overall), "Checks: Loading checks");
}

#[gpui::test]
fn pins_the_linked_item_identity_above_the_panel_scroller(cx: &mut TestAppContext) {
    let mut panel_props = props(DetailMode::Panel);
    panel_props.related_sessions = related();
    let mut h = mount(cx, FakeServices::new(NOW), pr(), panel_props);
    let header = h.header();
    assert_eq!(header.external_label, "Review on GitHub");
    assert_eq!(header.kind_label, "Pull request");
    assert!(!h.view.read_with(h.cx, |view, _| view.related_visible()));
}

#[test]
fn offers_full_file_diffs_only_for_github_pull_requests() {
    assert!(inbox_shows_full_file_diff(&pr()));
    let mut gitlab = pr();
    gitlab.provider = InboxProvider::Gitlab;
    assert!(!inbox_shows_full_file_diff(&gitlab));
    assert!(!inbox_shows_full_file_diff(&item()));
}

#[gpui::test]
fn keeps_the_linear_project_picker_beside_the_pinned_send_action(cx: &mut TestAppContext) {
    let mut linear = item();
    linear.provider = InboxProvider::Linear;
    linear.kind = InboxKind::Linear;
    linear.id = Some("linear-157".into());
    linear.identifier = Some("ENG-157".into());
    linear.team_name = Some("Engineering".into());
    let mut h = mount(cx, FakeServices::new(NOW), linear, props(DetailMode::Inbox));
    let header = h.header();
    assert!(header.choose_start_project);
    assert!(header.tracker);
    assert_eq!(header.reference, "ENG-157");
    assert_eq!(header.source, "Engineering");
    assert_eq!(header.external_label, "Open in Linear");
}

#[gpui::test]
fn shows_why_a_remote_gitlab_item_needs_attention_and_asks_for_a_workspace(
    cx: &mut TestAppContext,
) {
    let mut gitlab = item();
    gitlab.provider = InboxProvider::Gitlab;
    gitlab.repo = "acme/platform".into();
    gitlab.project_path = String::new();
    gitlab.url = "https://gitlab.example.com/acme/platform/-/issues/157".into();
    gitlab.attention_reason = Some("mentioned".into());
    let mut h = mount(cx, FakeServices::new(NOW), gitlab, props(DetailMode::Inbox));
    let header = h.header();
    assert_eq!(header.attention, "Mentioned you");
    assert!(header.choose_start_project);
    assert_eq!(header.external_label, "Open on GitLab");
}

#[gpui::test]
fn keeps_related_threads_in_the_pinned_header(cx: &mut TestAppContext) {
    let mut inbox_props = props(DetailMode::Inbox);
    inbox_props.related_sessions = related();
    let h = mount(cx, FakeServices::new(NOW), item(), inbox_props);
    assert!(h.view.read_with(h.cx, |view, _| view.related_visible()));
}

#[gpui::test]
fn offers_a_copy_action_for_the_head_branch_name(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().details.insert(
        157,
        FakeDetail::new(
            pr(),
            InboxDetailState {
                details: Loadable::ready(WorkItemDetails {
                    body: String::new(),
                    author: "octocat".into(),
                    base_ref_name: Some("main".into()),
                    head_ref_name: Some("feature/inbox-branch-copy".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
    );
    let mut h = mount(cx, services.clone(), pr(), props(DetailMode::Inbox));
    let header = h.header();
    assert_eq!(header.base_ref, "main");
    assert_eq!(header.head_ref, "feature/inbox-branch-copy");
    assert!(h.view.update(h.cx, |view, cx| view.copy_head_branch(cx)));
    assert_eq!(
        services.state.borrow().copied,
        ["feature/inbox-branch-copy"]
    );
    assert!(h.view.read_with(h.cx, |view, _| view.copied()));
}

#[gpui::test]
fn has_nothing_to_copy_when_the_pr_carries_no_branch_info(cx: &mut TestAppContext) {
    let mut other = pr();
    other.number = 999;
    let h = mount(cx, FakeServices::new(NOW), other, props(DetailMode::Inbox));
    assert!(!h.view.update(h.cx, |view, cx| view.copy_head_branch(cx)));
}

#[gpui::test]
fn sends_an_issue_to_an_agent_with_the_chosen_project(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let mut linear = item();
    linear.provider = InboxProvider::Linear;
    linear.kind = InboxKind::Linear;
    linear.id = Some("lin".into());
    services.state.borrow_mut().details.insert(
        157,
        FakeDetail::new(
            linear.clone(),
            InboxDetailState {
                details: Loadable::ready(WorkItemDetails {
                    body: "Tracker body".into(),
                    author: "pat".into(),
                    ..Default::default()
                }),
                thread: Loadable::ready(WorkItemThread::default()),
                ..Default::default()
            },
        ),
    );
    let mut detail_props = props(DetailMode::Inbox);
    detail_props.projects = crate::fixtures::sample_projects();
    detail_props.cwd = "/Users/dev/relay".into();
    let h = mount(cx, services.clone(), linear, detail_props);
    assert_eq!(
        h.view
            .read_with(h.cx, |view, _| view.start_project().to_string()),
        "/Users/dev/relay"
    );
    h.view.update(h.cx, |view, cx| view.start(cx));
    draw(h.cx);
    assert!(
        services
            .calls()
            .contains(&"start_item #157 /Users/dev/relay Tracker body".to_string())
    );
}

#[gpui::test]
fn runs_a_pull_request_action_and_hands_the_new_card_to_the_list(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let mut h = mount(cx, services.clone(), pr(), props(DetailMode::Inbox));
    let mut merged = pr();
    merged.state = "merged".into();
    let detail = services.detail(157).unwrap();
    *detail.action_result.borrow_mut() = Some(Ok(merged.clone()));
    h.view.update(h.cx, |view, cx| {
        view.ask_to_run(GithubPrAction::Squash, cx);
        view.run_action(cx);
    });
    draw(h.cx);
    assert!(
        detail
            .calls
            .borrow()
            .contains(&"run_pr_action Squash".to_string())
    );
    assert!(
        h.events
            .borrow()
            .contains(&InboxDetailEvent::ItemChanged(Box::new(merged)))
    );
    assert_eq!(h.header().status, "Merged");
}

#[gpui::test]
fn posts_a_comment_and_clears_the_reply(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let h = mount(cx, services.clone(), item(), props(DetailMode::Inbox));
    let detail = services.detail(157).unwrap();
    h.cx.update(|window, cx| {
        h.view.update(cx, |view, cx| {
            view.reply_to(
                Some(InboxReplyTarget {
                    id: "c1".into(),
                    author: "sam".into(),
                    thread_id: "t1".into(),
                }),
                window,
                cx,
            )
        })
    });
    draw(h.cx);
    assert!(detail.state.borrow().reply_to.is_some());
    // Typing goes through the field; post the draft the field holds.
    h.cx.update(|window, cx| {
        h.view.update(cx, |view, cx| {
            view.set_draft("Thanks, fixed", window, cx);
            view.submit_comment(window, cx);
        })
    });
    draw(h.cx);
    assert!(
        detail
            .calls
            .borrow()
            .contains(&"post_comment Thanks, fixed".to_string())
    );
    assert!(detail.state.borrow().reply_to.is_none());
    assert_eq!(
        h.view.read_with(h.cx, |view, _| view.draft().to_string()),
        ""
    );
}

// "Checks tab user behavior".

#[gpui::test]
fn loads_checks_on_open_in_the_inbox_even_while_summary_is_active(cx: &mut TestAppContext) {
    let services = FakeServices::new(NOW);
    let checks = FakeChecks::new(PrChecksState {
        checks: Some(GithubPrChecks {
            head_oid: "abc123".into(),
            checks: vec![GithubPrCheck {
                name: "build".into(),
                workflow: "CI".into(),
                state: GithubPrCheckState::Pass,
                url: Some("https://github.com/acme/web/actions/runs/9".into()),
                started_at: Some("2026-09-11T08:00:00Z".into()),
                completed_at: Some("2026-09-11T08:01:00Z".into()),
            }],
        }),
        ..Default::default()
    });
    services
        .state
        .borrow_mut()
        .checks
        .insert(157, checks.clone());
    let mut h = mount(cx, services.clone(), pr(), props(DetailMode::Inbox));
    assert!(
        services
            .calls()
            .contains(&"open_pr_checks #157".to_string())
    );
    {
        let params = checks.params.borrow();
        assert_eq!(params.cwd, "/tmp/web");
        assert_eq!(params.repo, "acme/web");
        assert_eq!(params.number, 157);
        assert!(params.enabled && params.open && params.poll);
    }
    assert_eq!(
        h.view.read_with(h.cx, |view, _| view.tab()),
        DetailTab::Summary
    );
    let overall = h.overall().unwrap();
    assert_eq!(PrChecksTab::label(&overall), "Checks: 1 passed");
    h.cx.update(|window, cx| {
        h.view
            .update(cx, |view, cx| view.set_tab(DetailTab::Checks, window, cx))
    });
    draw(h.cx);
    let row = h
        .view
        .read_with(h.cx, |view, cx| {
            view.checks_view()
                .and_then(|checks| checks.read(cx).row_info("build"))
        })
        .expect("the build row");
    assert_eq!(row.meta, "CI · Passed · 1m 00s");
    assert_eq!(row.title, "build · Passed, took 1m 00s, CI");
    assert!(row.linked);

    // A revision change revalidates the same PR.
    let mut next = props(DetailMode::Inbox);
    next.revision = 1;
    h.view.update(h.cx, |view, cx| view.set_props(next, cx));
    draw(h.cx);
    assert_eq!(checks.params.borrow().revision, 1);
}

#[gpui::test]
fn loads_checks_in_the_linked_side_panel_where_revision_stays_0(cx: &mut TestAppContext) {
    cx.update(init);
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().work_items.insert(157, pr());
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let target = LinkedWorkItem {
        kind: WorkItemKind::Pr,
        repo: "acme/web".into(),
        number: 157,
        url: "https://github.com/acme/web/pull/157".into(),
        extra: Default::default(),
    };
    let (panel, cx) = cx.add_window_view(move |window, cx| {
        LinkedWorkItemPanel::new(
            services_dyn,
            target,
            LinkedPanelProps {
                cwd: "/tmp/web".into(),
                visible: true,
                ..Default::default()
            },
            window,
            cx,
        )
    });
    draw(cx);
    assert!(
        services
            .calls()
            .contains(&"open_pr_checks #157".to_string())
    );
    let checks = services.checks(157).unwrap();
    assert_eq!(checks.params.borrow().revision, 0);
    let overall = panel.read_with(cx, |panel, cx| {
        panel
            .detail()
            .and_then(|detail| detail.read(cx).checks_overall().cloned())
    });
    assert!(overall.is_some());
}

// LinkedWorkItemPanel.test.ts.

#[gpui::test]
fn renders_a_linked_item_as_a_standalone_closable_side_panel(cx: &mut TestAppContext) {
    cx.update(init);
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().work_items.insert(157, item());
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let target = LinkedWorkItem {
        kind: WorkItemKind::Issue,
        repo: "acme/web".into(),
        number: 157,
        url: "https://github.com/acme/web/issues/157".into(),
        extra: Default::default(),
    };
    assert_eq!(crate::pr::linked_panel::linked_kind_label(&target), "Issue");
    let (panel, cx) = cx.add_window_view(move |window, cx| {
        LinkedWorkItemPanel::new(
            services_dyn,
            target,
            LinkedPanelProps {
                cwd: "/tmp/web".into(),
                visible: true,
                ..Default::default()
            },
            window,
            cx,
        )
    });
    let closed = Rc::new(RefCell::new(false));
    let sink = closed.clone();
    cx.update(|_, cx| {
        cx.subscribe(&panel, move |_, event: &LinkedPanelEvent, _| {
            if *event == LinkedPanelEvent::Close {
                *sink.borrow_mut() = true;
            }
        })
        .detach();
    });
    draw(cx);
    let mode = panel.read_with(cx, |panel, cx| {
        panel
            .detail()
            .map(|detail| detail.read(cx).header().external_label)
    });
    assert_eq!(mode, Some("Open on GitHub"));
    panel.update(cx, |panel, cx| panel.close(cx));
    cx.run_until_parked();
    assert!(*closed.borrow());
}

#[gpui::test]
fn keeps_fetched_data_mounted_while_hidden_and_reuses_it_when_shown_again(cx: &mut TestAppContext) {
    cx.update(init);
    let services = FakeServices::new(NOW);
    services.state.borrow_mut().work_items.insert(157, item());
    let services_dyn: Rc<dyn InboxServices> = services.clone();
    let target = LinkedWorkItem {
        kind: WorkItemKind::Issue,
        repo: "acme/web".into(),
        number: 157,
        url: "https://github.com/acme/web/issues/157".into(),
        extra: Default::default(),
    };
    let (panel, cx) = cx.add_window_view(move |window, cx| {
        LinkedWorkItemPanel::new(
            services_dyn,
            target,
            LinkedPanelProps {
                cwd: "/tmp/web".into(),
                visible: true,
                ..Default::default()
            },
            window,
            cx,
        )
    });
    draw(cx);
    let first = panel
        .read_with(cx, |panel, _| panel.detail().cloned())
        .unwrap();
    let lookups = |services: &FakeServices| {
        services
            .calls()
            .iter()
            .filter(|call| call.starts_with("github_work_item") || call.starts_with("open_detail"))
            .count()
    };
    let before = lookups(&services);
    panel.update(cx, |panel, cx| panel.set_visible(false, cx));
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.visible()));
    panel.update(cx, |panel, cx| panel.set_visible(true, cx));
    draw(cx);
    let second = panel
        .read_with(cx, |panel, _| panel.detail().cloned())
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(lookups(&services), before);
}
