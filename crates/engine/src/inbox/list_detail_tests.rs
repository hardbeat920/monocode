//! Tests for `InboxList` (the InboxView data effects) and `InboxItemDetail`
//! (the detail pane loads, comments, and pull request actions).

use std::sync::Arc;

use gpui::{AppContext, Entity, TestAppContext};
use monocode_core::Extra;
use monocode_core::session::LinkedWorkItem;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::backend::fake::FakeBackend;
use super::client::InboxClient;
use super::detail::{InboxItemDetail, InboxReplyTarget};
use super::github_tasks::test_items::{item, with};
use super::inbox_filters::{
    INBOX_CONNECTIONS_KEY, INBOX_SOURCE_KEY, InboxFilters, InboxStatusFilter,
};
use super::linear::LINEAR_HIDDEN_TEAMS_KEY;
use super::list::InboxList;
use super::types::{GithubPrAction, InboxItem, InboxKind, InboxProvider, WorkItemKind};

fn work_item(repo: &str, kind: &str, number: i64, state: &str) -> Value {
    json!({
        "kind": kind,
        "number": number,
        "title": format!("{kind} {number}"),
        "url": format!("https://github.com/{repo}/{}/{number}", if kind == "pr" { "pull" } else { "issues" }),
        "state": state,
        "updatedAt": "2026-09-13T12:00:00Z",
        "labels": [],
        "assignees": [],
        "draft": false,
        "repo": repo,
    })
}

fn client_with(
    cx: &mut TestAppContext,
    handler: impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static,
) -> (InboxClient, Arc<FakeBackend>) {
    let backend = FakeBackend::new(handler);
    let client = InboxClient::new(
        backend.clone(),
        monocode_settings::Kv::in_memory(),
        cx.executor(),
    );
    (client, backend)
}

/// GitHub connected with one open issue and one closed PR; Linear connected
/// with one team; everything else disconnected.
fn handler(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "git_github_status" => {
            Ok(json!({ "connected": true, "installed": true, "authenticated": true }))
        }
        "linear_status" => Ok(json!({ "connected": true })),
        "jira_status" | "gitlab_status" | "azure_devops_status" => {
            Ok(json!({ "connected": false }))
        }
        "git_github_repositories" => Ok(json!(["acme/web"])),
        "git_github_work_items" => Ok(if args["kind"] == "issue" {
            json!([work_item("acme/web", "issue", 1, "open")])
        } else {
            json!([work_item("acme/web", "pr", 2, "closed")])
        }),
        "git_github_work_item" => Ok(work_item("acme/web", "pr", 99, "open")),
        "linear_list_teams" => Ok(json!([{ "id": "t1", "key": "ENG", "name": "Engineering" }])),
        "linear_list_issues" => Ok(json!([])),
        _ => Err(format!("Unexpected command: {command}")),
    }
}

fn mount(
    cx: &mut TestAppContext,
    client: &InboxClient,
    target: Option<LinkedWorkItem>,
) -> Entity<InboxList> {
    let client = client.clone();
    cx.new(|cx| InboxList::new(client, &[], "/tmp/web", target, cx))
}

fn numbers(items: &[InboxItem]) -> Vec<i64> {
    items.iter().map(|item| item.number).collect()
}

#[gpui::test]
fn the_list_loads_reads_connections_and_falls_back_from_a_disconnected_tab(
    cx: &mut TestAppContext,
) {
    let (client, _) = client_with(cx, handler);
    client.kv().set_item(INBOX_SOURCE_KEY, "jira");
    let list = mount(cx, &client, None);
    list.read_with(cx, |list, _| {
        assert!(list.loading());
        assert_eq!(list.source(), InboxProvider::Jira);
    });
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        assert!(!list.loading());
        assert_eq!(numbers(list.items()), [1, 2]);
        assert_eq!(list.connections().jira, Some(false));
        assert_eq!(list.source(), InboxProvider::Github);
        assert_eq!(
            list.visible_sources(),
            [InboxProvider::Github, InboxProvider::Linear]
        );
        assert_eq!(
            list.connectable_sources(),
            [
                InboxProvider::Jira,
                InboxProvider::Gitlab,
                InboxProvider::AzureDevops
            ]
        );
        assert_eq!(numbers(&list.visible_items("")), [1, 2]);
        assert_eq!(numbers(&list.visible_items("pr")), [2]);
    });
    assert_eq!(
        client.kv().get_item(INBOX_SOURCE_KEY).as_deref(),
        Some("github")
    );
    assert!(
        client
            .kv()
            .get_item(INBOX_CONNECTIONS_KEY)
            .unwrap()
            .contains(r#""linear":true"#)
    );
}

#[gpui::test]
fn the_list_shows_a_fresh_cache_without_fetching_and_refresh_forces_one(cx: &mut TestAppContext) {
    let (client, backend) = client_with(cx, handler);
    let first = mount(cx, &client, None);
    cx.run_until_parked();
    drop(first);
    let fetched = backend.count("git_github_work_items");
    let list = mount(cx, &client, None);
    list.read_with(cx, |list, _| {
        assert!(!list.loading());
        assert_eq!(numbers(list.items()), [1, 2]);
    });
    cx.run_until_parked();
    assert_eq!(backend.count("git_github_work_items"), fetched);
    list.update(cx, |list, cx| list.refresh(cx));
    list.read_with(cx, |list, _| assert!(list.revalidating()));
    cx.run_until_parked();
    assert_eq!(backend.count("git_github_work_items"), fetched + 2);
    list.read_with(cx, |list, _| assert!(!list.revalidating()));
}

#[gpui::test]
fn the_list_applies_filters_and_refetches_when_the_fetch_state_changes(cx: &mut TestAppContext) {
    let (client, backend) = client_with(cx, handler);
    let list = mount(cx, &client, None);
    cx.run_until_parked();
    let fetched = backend.count("git_github_work_items");
    list.update(cx, |list, cx| {
        list.set_filters(
            InboxFilters {
                status: InboxStatusFilter {
                    open: true,
                    ..Default::default()
                },
                ..InboxFilters::default()
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(backend.count("git_github_work_items"), fetched + 2);
    assert_eq!(
        backend.calls_to("git_github_work_items").last().unwrap()["state"],
        json!("open")
    );
    list.read_with(cx, |list, _| {
        assert!(list.filters_active());
        assert_eq!(numbers(&list.visible_items("")), [1]);
    });
}

#[gpui::test]
fn the_list_pins_a_linked_target_it_had_to_look_up(cx: &mut TestAppContext) {
    let (client, backend) = client_with(cx, handler);
    let target = LinkedWorkItem {
        kind: WorkItemKind::Pr,
        repo: "acme/web".into(),
        number: 99,
        url: "https://github.com/acme/web/pull/99".into(),
        extra: Extra::new(),
    };
    client.kv().set_item(INBOX_SOURCE_KEY, "linear");
    let list = mount(cx, &client, Some(target));
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        assert_eq!(list.source(), InboxProvider::Github);
        assert_eq!(
            list.target_selection_key().as_deref(),
            Some("github:acme/web:pr:99")
        );
        assert_eq!(numbers(&list.visible_items("")), [99, 1, 2]);
    });
    // The temporary switch to GitHub is not saved.
    assert_eq!(
        client.kv().get_item(INBOX_SOURCE_KEY).as_deref(),
        Some("linear")
    );
    assert_eq!(
        backend.calls_to("git_github_work_item"),
        [json!({ "cwd": "/tmp/web", "repo": "acme/web", "kind": "pr", "number": 99 })]
    );
}

#[gpui::test]
fn the_list_marks_the_open_tab_read(cx: &mut TestAppContext) {
    let (client, _) = client_with(cx, handler);
    client.seed_inbox_seen_if_needed(&[super::inbox_seen::InboxSeenEntry::new(
        "github:other:issue:1",
        "2026-01-01T00:00:00Z",
    )]);
    let list = mount(cx, &client, None);
    cx.run_until_parked();
    assert!(list.read_with(cx, |list, _| list.source_has_unseen()));
    list.update(cx, |list, cx| list.mark_source_read(cx));
    list.read_with(cx, |list, _| {
        assert!(!list.source_has_unseen());
        assert_eq!(list.read_status_error(), None);
    });
}

#[gpui::test]
fn the_linear_tab_loads_its_team_roster_and_saves_hidden_teams(cx: &mut TestAppContext) {
    let (client, backend) = client_with(cx, handler);
    let list = mount(cx, &client, None);
    cx.run_until_parked();
    list.update(cx, |list, cx| list.set_source(InboxProvider::Linear, cx));
    cx.run_until_parked();
    list.read_with(cx, |list, _| assert_eq!(list.linear_teams().len(), 1));
    list.update(cx, |list, cx| {
        list.set_linear_hidden_team_ids(vec!["t1".into()], cx)
    });
    cx.run_until_parked();
    assert_eq!(
        client.kv().get_item(LINEAR_HIDDEN_TEAMS_KEY).as_deref(),
        Some(r#"["t1"]"#)
    );
    list.read_with(cx, |list, _| {
        assert_eq!(list.linear_hidden_team_ids(), ["t1"]);
        assert!(list.filters_active());
    });
    // Every team is hidden, so Linear issues are not fetched at all.
    assert_eq!(backend.count("linear_list_issues"), 1);
}

#[gpui::test]
fn a_failed_first_load_reports_the_error_on_every_tab(cx: &mut TestAppContext) {
    let (client, _) = client_with(cx, |command, args| match command {
        "linear_status" => Err("linear exploded".into()),
        _ => handler(command, args),
    });
    let list = mount(cx, &client, None);
    cx.run_until_parked();
    list.read_with(cx, |list, _| {
        assert!(list.items().is_empty());
        assert_eq!(list.source_error(), Some("linear exploded"));
        assert_eq!(
            list.provider_errors().get(InboxProvider::Jira),
            Some("linear exploded")
        );
    });
}

// The detail pane.

fn pr_item() -> InboxItem {
    with(item(7, "2026-09-13T12:00:00Z"), |row| {
        row.kind = InboxKind::Pr;
        row.repo = "acme/web".into();
        row.project_path = "/tmp/web".into();
    })
}

fn detail_handler(
    answers: Arc<Mutex<Vec<String>>>,
) -> impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static {
    move |command, args| {
        answers.lock().push(command.to_string());
        match command {
            "git_github_work_item_details" => {
                Ok(json!({ "body": "Body", "author": "maya", "reviewDecision": "" }))
            }
            "git_github_work_item_thread" => {
                Ok(json!({ "comments": [], "truncated": false, "reviewDecision": "APPROVED" }))
            }
            "git_github_pr_diff" => Ok(json!({
                "additions": 1,
                "deletions": 0,
                "files": [],
                "patch": if args["fullContext"] == true { "full" } else { "hunks" },
                "truncated": false,
            })),
            "git_github_work_item_comment" => Ok(json!("https://github.com/acme/web/pull/7#c")),
            "git_github_pr_action" => Ok(work_item("acme/web", "pr", 7, "open")),
            "linear_issue_thread" => Err("linear down".into()),
            "linear_issue_details" => Ok(json!({ "body": "Linear body", "author": "Ada" })),
            _ => Err(format!("Unexpected command: {command}")),
        }
    }
}

fn open_detail(
    cx: &mut TestAppContext,
    client: &InboxClient,
    item: InboxItem,
) -> Entity<InboxItemDetail> {
    let client = client.clone();
    cx.new(|cx| InboxItemDetail::new(client, item, cx))
}

#[gpui::test]
fn the_detail_pane_loads_details_thread_and_diff(cx: &mut TestAppContext) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, backend) = client_with(cx, detail_handler(log));
    let detail = open_detail(cx, &client, pr_item());
    detail.read_with(cx, |detail, _| {
        assert!(detail.details().loading);
        assert!(detail.thread().loading);
    });
    cx.run_until_parked();
    detail.read_with(cx, |detail, _| {
        assert_eq!(detail.details().value.as_ref().unwrap().body, "Body");
        assert!(!detail.thread().loading);
        assert_eq!(detail.review_decision(), "APPROVED");
    });
    detail.update(cx, |detail, cx| detail.show_diff(true, cx));
    cx.run_until_parked();
    detail.read_with(cx, |detail, _| {
        assert_eq!(detail.diff().value.as_ref().unwrap().patch, "full")
    });
    assert_eq!(
        backend.calls_to("git_github_pr_diff")[0]["fullContext"],
        json!(true)
    );

    // A second pane shows the cached answers at once.
    let again = open_detail(cx, &client, pr_item());
    again.read_with(cx, |detail, _| {
        assert!(!detail.details().loading);
        assert_eq!(detail.details().value.as_ref().unwrap().body, "Body");
    });
}

#[gpui::test]
fn the_side_panel_reuses_a_hover_prefetch_while_the_inbox_refetches(cx: &mut TestAppContext) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, backend) = client_with(cx, detail_handler(log));
    client.prefetch_github_work_item("/tmp/web", "acme/web", WorkItemKind::Pr, 7);
    cx.run_until_parked();
    assert_eq!(backend.count("git_github_work_item_details"), 1);
    assert_eq!(backend.count("git_github_pr_diff"), 1);

    let fresh = Some(super::github_tasks::GITHUB_WORK_ITEM_FRESH_MS);
    let panel_client = client.clone();
    let panel = cx.new(|cx| InboxItemDetail::with_max_age(panel_client, pr_item(), fresh, cx));
    panel.update(cx, |detail, cx| detail.show_diff(false, cx));
    cx.run_until_parked();
    panel.read_with(cx, |detail, _| {
        assert_eq!(detail.details().value.as_ref().unwrap().body, "Body");
        assert_eq!(detail.diff().value.as_ref().unwrap().patch, "hunks");
        assert!(!detail.thread().loading);
    });
    assert_eq!(backend.count("git_github_work_item_details"), 1);
    assert_eq!(backend.count("git_github_work_item_thread"), 1);
    assert_eq!(backend.count("git_github_pr_diff"), 1);

    // The Inbox page shows the cache at once but still fetches.
    let inbox = open_detail(cx, &client, pr_item());
    cx.run_until_parked();
    inbox.read_with(cx, |detail, _| assert!(detail.details().value.is_some()));
    assert_eq!(backend.count("git_github_work_item_details"), 2);
    assert_eq!(backend.count("git_github_work_item_thread"), 2);
}

#[gpui::test]
fn the_detail_pane_posts_a_reply_and_reloads_the_thread(cx: &mut TestAppContext) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, backend) = client_with(cx, detail_handler(log));
    let detail = open_detail(cx, &client, pr_item());
    cx.run_until_parked();
    detail.update(cx, |detail, cx| {
        detail.set_reply_to(
            Some(InboxReplyTarget {
                id: "c1".into(),
                thread_id: "t1".into(),
            }),
            cx,
        )
    });
    let post = detail.update(cx, |detail, cx| detail.post_comment(" Thanks ", cx));
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(post), Some(Ok(())));
    assert_eq!(
        backend.calls_to("git_github_work_item_comment"),
        [json!({
            "cwd": "/tmp/web",
            "repo": "acme/web",
            "kind": "pr",
            "number": 7,
            "body": "Thanks",
            "inReplyTo": "t1",
        })]
    );
    assert_eq!(backend.count("git_github_work_item_thread"), 2);
    detail.read_with(cx, |detail, _| {
        assert!(detail.reply_to().is_none());
        assert!(!detail.posting());
        assert_eq!(detail.post_error(), None);
    });
}

#[gpui::test]
fn the_detail_pane_runs_a_merge_and_notes_a_queued_one(cx: &mut TestAppContext) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, _) = client_with(cx, detail_handler(log));
    let detail = open_detail(cx, &client, pr_item());
    cx.run_until_parked();
    let run = detail.update(cx, |detail, cx| {
        detail.run_pr_action(GithubPrAction::Squash, cx)
    });
    cx.run_until_parked();
    let updated = futures::FutureExt::now_or_never(run).unwrap().unwrap();
    assert_eq!(updated.provider, InboxProvider::Github);
    assert_eq!(updated.project_path, "/tmp/web");
    detail.read_with(cx, |detail, _| {
        assert_eq!(
            detail.action_notice(),
            Some("Merge queued or auto-merge enabled.")
        );
        assert!(!detail.action_busy());
    });
}

#[gpui::test]
fn the_detail_pane_reports_tracker_errors(cx: &mut TestAppContext) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, _) = client_with(cx, detail_handler(log));
    let jira = with(item(42, "2026-09-13T12:00:00Z"), |row| {
        row.provider = InboxProvider::Jira;
        row.kind = InboxKind::Jira;
        row.identifier = None;
    });
    let detail = open_detail(cx, &client, jira);
    cx.run_until_parked();
    detail.read_with(cx, |detail, _| {
        assert_eq!(
            detail.details().error.as_deref(),
            Some("Missing Jira issue")
        );
    });
    let linear = with(item(9, "2026-09-13T12:00:00Z"), |row| {
        row.provider = InboxProvider::Linear;
        row.kind = InboxKind::Linear;
        row.id = Some("lin-9".into());
    });
    detail.update(cx, |detail, cx| detail.set_item(linear, cx));
    cx.run_until_parked();
    detail.read_with(cx, |detail, _| {
        assert_eq!(detail.details().value.as_ref().unwrap().body, "Linear body");
        assert_eq!(detail.thread().error.as_deref(), Some("linear down"));
    });
}
