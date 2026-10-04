//! Ports of githubTasks.providers.test.ts, githubTasks.repositories.test.ts,
//! jira.test.ts, and the cache cases of sessionWorkItem.test.ts.

use std::sync::Arc;

use gpui::TestAppContext;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::client::test_support::{client, settle, unexpected};
use super::jira::jira_project_ids_for_fetch;
use super::types::{
    GithubPrAction, GithubWorkItem, InboxItem, InboxProvider, InboxProviderErrors, InboxQuery,
    InboxState, RepositoryWorkItem, TrackerGroup, TrackerIssue, WorkItemKind,
};

fn status_off(command: &str) -> Option<Result<Value, String>> {
    matches!(
        command,
        "linear_status" | "jira_status" | "gitlab_status" | "azure_devops_status"
    )
    .then(|| Ok(json!({ "connected": false })))
}

fn query(assigned_to_me: bool, state: InboxState) -> InboxQuery {
    InboxQuery {
        assigned_to_me,
        state,
        ..InboxQuery::default()
    }
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|path| path.to_string()).collect()
}

// githubTasks.providers.test.ts

fn provider_work_item(repo: &str, kind: &str) -> Value {
    json!({
        "kind": kind,
        "number": 10,
        "title": format!("{kind} item"),
        "url": format!("https://example.com/{repo}/{kind}/10"),
        "state": "open",
        "updatedAt": "2026-09-16T08:00:00Z",
        "labels": [{ "name": "bug", "color": "ff0000" }],
        "assignees": [{ "login": "maya", "avatarUrl": "https://example.com/maya.png" }],
        "draft": kind == "pr",
        "repo": repo,
        "attentionReason": "assigned",
    })
}

fn provider_item(
    provider: InboxProvider,
    repo: &str,
    kind: &str,
    item_repo: &str,
    project_path: &str,
) -> InboxItem {
    let work: RepositoryWorkItem = serde_json::from_value(provider_work_item(repo, kind)).unwrap();
    let mut item = super::client::repository_inbox_item(provider, &work, project_path, item_repo);
    item.repo = item_repo.to_string();
    item
}

type ListFn = Arc<dyn Fn(&str) -> Result<Value, String> + Send + Sync>;

struct ProviderCase {
    provider: InboxProvider,
    prefix: &'static str,
}

const PROVIDER_CASES: [ProviderCase; 2] = [
    ProviderCase {
        provider: InboxProvider::Gitlab,
        prefix: "gitlab",
    },
    ProviderCase {
        provider: InboxProvider::AzureDevops,
        prefix: "azure_devops",
    },
];

fn provider_projects() -> Vec<String> {
    paths(&[
        "/tmp/first/",
        "/tmp/first",
        "/tmp/second",
        "/tmp/missing",
        "/tmp/blank",
    ])
}

/// The `beforeEach` handler: GitHub resolves but lists nothing, the provider
/// under test is connected, and its repository lookups follow the paths.
fn provider_handler(
    prefix: &'static str,
    connected: Arc<Mutex<bool>>,
    list: Arc<Mutex<ListFn>>,
) -> impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static {
    move |command, args| {
        if command == "git_github_repositories" {
            return Ok(json!(["github/repo"]));
        }
        if command == "git_github_work_items" {
            return Ok(json!([]));
        }
        if command == format!("{prefix}_status") {
            return Ok(json!({ "connected": *connected.lock() }));
        }
        if let Some(off) = status_off(command) {
            return off;
        }
        if command == format!("{prefix}_repo") {
            return match args["cwd"].as_str().unwrap_or_default() {
                "/tmp/first" => Ok(json!(" acme/web ")),
                "/tmp/second" => Ok(json!("ACME/WEB")),
                "/tmp/blank" => Ok(json!("  ")),
                _ => Err("not a provider repository".into()),
            };
        }
        if command == format!("{prefix}_list_work_items")
            || command == format!("{prefix}_list_todos")
        {
            let list = list.lock().clone();
            return list(args["kind"].as_str().unwrap_or_default());
        }
        unexpected(command)
    }
}

type ProviderSetup = (
    super::client::InboxClient,
    Arc<super::backend::fake::FakeBackend>,
    Arc<Mutex<bool>>,
    Arc<Mutex<ListFn>>,
);

fn provider_setup(cx: &TestAppContext, case: &ProviderCase) -> ProviderSetup {
    let connected = Arc::new(Mutex::new(true));
    let list: Arc<Mutex<ListFn>> = Arc::new(Mutex::new(Arc::new(|kind: &str| {
        Ok(json!([provider_work_item("", kind)]))
    })));
    let (client, backend) = client(
        cx,
        provider_handler(case.prefix, connected.clone(), list.clone()),
    );
    (client, backend, connected, list)
}

#[gpui::test]
fn repository_providers_fetch_items_once_per_repository_using_the_first_checkout(
    cx: &mut TestAppContext,
) {
    for case in &PROVIDER_CASES {
        for state in [InboxState::Open, InboxState::All] {
            let (client, backend, _, _) = provider_setup(cx, case);
            let result = settle(
                cx,
                client.list_inbox_items(&provider_projects(), &query(false, state), false),
            )
            .unwrap();
            assert_eq!(
                result.items,
                vec![
                    provider_item(case.provider, "", "issue", "acme/web", "/tmp/first"),
                    provider_item(case.provider, "", "pr", "acme/web", "/tmp/first"),
                ]
            );
            assert!(result.errors.is_empty());
            let list_command = format!("{}_list_work_items", case.prefix);
            assert_eq!(backend.count(&list_command), 2);
            for kind in ["issue", "pr"] {
                let mut expected = json!({
                    "cwd": "/tmp/first",
                    "kind": kind,
                    "assignedToMe": false,
                    "state": state.as_str(),
                });
                if state == InboxState::All {
                    expected["limit"] = json!(100);
                }
                assert!(backend.calls_to(&list_command).contains(&expected));
            }
            assert_eq!(backend.count(&format!("{}_list_todos", case.prefix)), 0);
            assert_eq!(backend.count(&format!("{}_repo", case.prefix)), 4);
        }
    }
}

#[gpui::test]
fn repository_providers_fetch_assigned_items_globally_and_match_local_checkouts_ignoring_case(
    cx: &mut TestAppContext,
) {
    for case in &PROVIDER_CASES {
        for state in [InboxState::Open, InboxState::All] {
            let (client, backend, _, list) = provider_setup(cx, case);
            *list.lock() = Arc::new(|kind: &str| {
                let repo = if kind == "issue" {
                    "ACME/WEB"
                } else {
                    "other/remote"
                };
                Ok(json!([provider_work_item(repo, kind)]))
            });
            let result = settle(
                cx,
                client.list_inbox_items(&provider_projects(), &query(true, state), false),
            )
            .unwrap();
            assert_eq!(
                result.items,
                vec![
                    provider_item(case.provider, "other/remote", "pr", "other/remote", ""),
                    provider_item(case.provider, "ACME/WEB", "issue", "ACME/WEB", "/tmp/first"),
                ]
            );
            assert!(result.errors.is_empty());
            let todos = format!("{}_list_todos", case.prefix);
            assert_eq!(backend.count(&todos), 2);
            for kind in ["issue", "pr"] {
                let mut expected = json!({ "kind": kind });
                if state == InboxState::All {
                    expected["limit"] = json!(100);
                }
                assert!(backend.calls_to(&todos).contains(&expected));
            }
            assert_eq!(
                backend.count(&format!("{}_list_work_items", case.prefix)),
                0
            );
        }
    }
}

#[gpui::test]
fn repository_providers_preserve_the_repository_supplied_by_a_work_item(cx: &mut TestAppContext) {
    for case in &PROVIDER_CASES {
        let (client, _, _, list) = provider_setup(cx, case);
        *list.lock() =
            Arc::new(|kind: &str| Ok(json!([provider_work_item("canonical/repo", kind)])));
        let result = settle(
            cx,
            client.list_inbox_items(&provider_projects(), &query(false, InboxState::Open), false),
        )
        .unwrap();
        let repos: Vec<&str> = result.items.iter().map(|item| item.repo.as_str()).collect();
        assert_eq!(repos, ["canonical/repo", "canonical/repo"]);
    }
}

#[gpui::test]
fn repository_providers_keep_a_successful_batch_when_the_other_kind_fails(cx: &mut TestAppContext) {
    for case in &PROVIDER_CASES {
        for assigned in [false, true] {
            let (client, _, _, list) = provider_setup(cx, case);
            *list.lock() = Arc::new(|kind: &str| {
                if kind == "issue" {
                    return Err("issues unavailable".into());
                }
                Ok(json!([provider_work_item("acme/web", kind)]))
            });
            let result = settle(
                cx,
                client.list_inbox_items(
                    &provider_projects(),
                    &query(assigned, InboxState::Open),
                    false,
                ),
            )
            .unwrap();
            assert_eq!(
                result.items,
                vec![provider_item(
                    case.provider,
                    "acme/web",
                    "pr",
                    "acme/web",
                    "/tmp/first"
                )]
            );
            assert!(result.errors.is_empty());
        }
    }
}

#[gpui::test]
fn repository_providers_report_the_first_error_when_every_fetch_fails(cx: &mut TestAppContext) {
    for case in &PROVIDER_CASES {
        for assigned in [false, true] {
            let (client, _, _, list) = provider_setup(cx, case);
            *list.lock() = Arc::new(|kind: &str| Err(format!("{kind} unavailable")));
            let result = settle(
                cx,
                client.list_inbox_items(
                    &provider_projects(),
                    &query(assigned, InboxState::Open),
                    false,
                ),
            )
            .unwrap();
            assert!(result.items.is_empty());
            let mut errors = InboxProviderErrors::new();
            errors.set(case.provider, "issue unavailable");
            assert_eq!(result.errors, errors);
        }
    }
}

#[gpui::test]
fn repository_providers_return_an_empty_inbox_when_no_local_repositories_resolve(
    cx: &mut TestAppContext,
) {
    for case in &PROVIDER_CASES {
        let (client, backend, _, _) = provider_setup(cx, case);
        let result = settle(
            cx,
            client.list_inbox_items(
                &paths(&["/tmp/missing"]),
                &query(false, InboxState::Open),
                false,
            ),
        )
        .unwrap();
        assert!(result.items.is_empty() && result.errors.is_empty());
        assert_eq!(
            backend.count(&format!("{}_list_work_items", case.prefix)),
            0
        );
        assert_eq!(backend.count(&format!("{}_list_todos", case.prefix)), 0);
    }
}

#[gpui::test]
fn repository_providers_fetch_assigned_items_even_without_local_projects(cx: &mut TestAppContext) {
    for case in &PROVIDER_CASES {
        let (client, backend, _, list) = provider_setup(cx, case);
        *list.lock() = Arc::new(|kind: &str| Ok(json!([provider_work_item("other/remote", kind)])));
        let result = settle(
            cx,
            client.list_inbox_items(&[], &query(true, InboxState::Open), false),
        )
        .unwrap();
        assert_eq!(
            result.items,
            vec![
                provider_item(case.provider, "other/remote", "issue", "other/remote", ""),
                provider_item(case.provider, "other/remote", "pr", "other/remote", ""),
            ]
        );
        assert!(result.errors.is_empty());
        assert_eq!(backend.count(&format!("{}_list_todos", case.prefix)), 2);
    }
}

#[gpui::test]
fn repository_providers_skip_disconnected_providers(cx: &mut TestAppContext) {
    for case in &PROVIDER_CASES {
        let (client, backend, connected, _) = provider_setup(cx, case);
        *connected.lock() = false;
        let result = settle(
            cx,
            client.list_inbox_items(&provider_projects(), &query(false, InboxState::Open), false),
        )
        .unwrap();
        assert!(result.items.is_empty() && result.errors.is_empty());
        assert_eq!(
            backend.count(&format!("{}_list_work_items", case.prefix)),
            0
        );
        assert_eq!(backend.count(&format!("{}_repo", case.prefix)), 0);
    }
}

// githubTasks.repositories.test.ts

fn github_item(repo: &str, kind: &str) -> Value {
    json!({
        "kind": kind,
        "number": 10,
        "title": format!("{repo} item"),
        "url": format!("https://github.com/{repo}/{}/10", if kind == "pr" { "pull" } else { "issues" }),
        "state": "open",
        "updatedAt": "2026-09-16T08:00:00Z",
        "labels": [],
        "assignees": [],
        "draft": false,
        "repo": repo,
    })
}

#[gpui::test]
fn github_caches_the_local_repository_and_parent_metadata(cx: &mut TestAppContext) {
    let (client, backend) = client(cx, |_, _| Ok(json!(["maya/web", "acme/web"])));
    assert_eq!(
        settle(cx, client.github_repositories("/tmp/web")).unwrap(),
        ["maya/web", "acme/web"]
    );
    assert_eq!(
        settle(cx, client.github_repositories("/tmp/web/")).unwrap(),
        ["maya/web", "acme/web"]
    );
    assert_eq!(
        backend.calls(),
        vec![(
            "git_github_repositories".to_string(),
            json!({ "cwd": "/tmp/web" })
        )]
    );
    assert_eq!(
        settle(cx, client.github_repo("/tmp/web")).unwrap(),
        "maya/web"
    );
    assert_eq!(backend.calls().len(), 1);
}

#[gpui::test]
fn github_fetches_a_shared_parent_once_and_keeps_the_preferred_local_checkout(
    cx: &mut TestAppContext,
) {
    let (client, backend) = client(cx, |command, args| {
        if command == "git_github_repositories" {
            return Ok(if args["cwd"] == "/tmp/fork-a" {
                json!(["maya/web", "acme/web"])
            } else {
                json!(["lin/web", "ACME/web"])
            });
        }
        if command == "git_github_work_items" {
            let repo = args["repo"].as_str().unwrap_or_default();
            let kind = args["kind"].as_str().unwrap_or_default();
            return Ok(if repo.to_lowercase() == "acme/web" && kind == "issue" {
                json!([github_item("acme/web", kind)])
            } else {
                json!([])
            });
        }
        status_off(command).unwrap_or_else(|| unexpected(command))
    });
    let result = settle(
        cx,
        client.list_inbox_items(
            &paths(&["/tmp/fork-a", "/tmp/fork-b"]),
            &query(false, InboxState::Open),
            false,
        ),
    )
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].repo, "acme/web");
    assert_eq!(result.items[0].project_path, "/tmp/fork-a");
    let list_calls = backend.calls_to("git_github_work_items");
    assert_eq!(list_calls.len(), 6);
    assert_eq!(
        list_calls
            .iter()
            .filter(|args| args["repo"].as_str().unwrap_or_default().to_lowercase() == "acme/web")
            .count(),
        2
    );
}

#[gpui::test]
fn github_reports_an_error_when_repository_discovery_fails(cx: &mut TestAppContext) {
    let (client, _) = client(cx, |command, _| {
        if command == "git_github_repositories" {
            return Err("not a GitHub repository".into());
        }
        status_off(command).unwrap_or_else(|| unexpected(command))
    });
    let result = settle(
        cx,
        client.list_inbox_items(
            &paths(&["/tmp/local"]),
            &query(false, InboxState::Open),
            false,
        ),
    )
    .unwrap();
    assert!(result.items.is_empty());
    let mut errors = InboxProviderErrors::new();
    errors.set(InboxProvider::Github, "not a GitHub repository");
    assert_eq!(result.errors, errors);
}

#[gpui::test]
fn github_passes_the_repository_through_and_isolates_same_number_caches(cx: &mut TestAppContext) {
    let (client, backend) = client(cx, |command, args| match command {
        "git_github_work_item_details" => Ok(json!({ "body": args["repo"], "author": "octocat" })),
        "git_github_work_item_thread" => Ok(json!({
            "comments": [],
            "commits": [],
            "truncated": false,
            "reviewDecision": "",
            "baseRefName": "main",
            "headRefName": "feature",
        })),
        "git_github_pr_diff" => Ok(
            json!({ "additions": 1, "deletions": 0, "files": [], "patch": "diff", "truncated": false }),
        ),
        "git_github_work_item_comment" => Ok(json!(
            "https://github.com/acme/web/issues/10#issuecomment-1"
        )),
        _ => unexpected(command),
    });
    settle(
        cx,
        client.github_work_item_details("/tmp/web", "maya/web", WorkItemKind::Issue, 10, None),
    )
    .unwrap();
    settle(
        cx,
        client.github_work_item_details("/tmp/web", "acme/web", WorkItemKind::Issue, 10, None),
    )
    .unwrap();
    assert_eq!(
        client
            .peek_github_work_item_details("maya/web", WorkItemKind::Issue, 10)
            .unwrap()
            .body,
        "maya/web"
    );
    assert_eq!(
        client
            .peek_github_work_item_details("acme/web", WorkItemKind::Issue, 10)
            .unwrap()
            .body,
        "acme/web"
    );
    settle(
        cx,
        client.github_work_item_thread("/tmp/web", "acme/web", WorkItemKind::Pr, 10, false, None),
    )
    .unwrap();
    settle(
        cx,
        client.github_pr_diff("/tmp/web", "acme/web", 10, false, None),
    )
    .unwrap();
    settle(
        cx,
        client.github_work_item_comment(
            "/tmp/web",
            "acme/web",
            WorkItemKind::Issue,
            10,
            "Looks good",
            None,
        ),
    )
    .unwrap();
    assert_eq!(
        backend.calls_to("git_github_work_item_thread"),
        [json!({ "cwd": "/tmp/web", "repo": "acme/web", "kind": "pr", "number": 10 })]
    );
    assert_eq!(
        backend.calls_to("git_github_pr_diff"),
        [json!({ "cwd": "/tmp/web", "repo": "acme/web", "number": 10, "fullContext": false })]
    );
    assert_eq!(
        backend.calls_to("git_github_work_item_comment"),
        [json!({
            "cwd": "/tmp/web",
            "repo": "acme/web",
            "kind": "issue",
            "number": 10,
            "body": "Looks good",
            "inReplyTo": "",
        })]
    );
    assert!(
        client
            .peek_github_work_item_thread("acme/web", WorkItemKind::Pr, 10)
            .is_some()
    );
    assert!(client.peek_github_pr_diff("acme/web", 10, false).is_some());
}

#[gpui::test]
fn github_dedupes_a_thread_request_until_forced(cx: &mut TestAppContext) {
    let (client, backend) = client(cx, |_, _| Ok(json!({ "comments": [], "truncated": false })));
    let first =
        client.github_work_item_thread("/tmp/web", "acme/web", WorkItemKind::Pr, 10, false, None);
    let second =
        client.github_work_item_thread("/tmp/web", "acme/web", WorkItemKind::Pr, 10, false, None);
    settle(cx, first).unwrap();
    settle(cx, second).unwrap();
    assert_eq!(backend.calls().len(), 1);
    settle(
        cx,
        client.github_work_item_thread("/tmp/web", "acme/web", WorkItemKind::Pr, 10, true, None),
    )
    .unwrap();
    assert_eq!(backend.calls().len(), 2);
}

// githubWorkItemFreshness.test.ts

fn freshness_client(
    cx: &TestAppContext,
) -> (
    super::client::InboxClient,
    Arc<super::backend::fake::FakeBackend>,
    Arc<Mutex<i64>>,
) {
    let now = Arc::new(Mutex::new(1_000_000i64));
    let clock = now.clone();
    let backend = super::backend::fake::FakeBackend::new(|command, _| match command {
        "git_github_pr_diff" => Ok(
            json!({ "additions": 0, "deletions": 0, "files": [], "patch": "", "truncated": false }),
        ),
        "git_github_work_item_thread" => {
            Ok(json!({ "comments": [], "commits": [], "truncated": false }))
        }
        "git_github_work_item" => Ok(json!({
            "kind": "pr",
            "number": 1,
            "title": "Retry",
            "url": "https://github.com/o/r/pull/1",
            "state": "open",
            "updatedAt": "2026-10-03T09:00:00Z",
        })),
        _ => Ok(json!({ "body": "", "author": "" })),
    });
    let client = super::client::InboxClient::with_clock(
        backend.clone(),
        monocode_settings::Kv::in_memory(),
        cx.executor(),
        Arc::new(move || *clock.lock()),
    );
    (client, backend, now)
}

#[gpui::test]
fn github_shares_an_in_flight_details_request(cx: &mut TestAppContext) {
    let (client, backend, _) = freshness_client(cx);
    let first = client.github_work_item_details("/repo", "o/r", WorkItemKind::Pr, 1, None);
    let second = client.github_work_item_details("/repo", "o/r", WorkItemKind::Pr, 1, None);
    settle(cx, first).unwrap();
    settle(cx, second).unwrap();
    assert_eq!(backend.calls().len(), 1);
}

#[gpui::test]
fn github_reuses_recent_data_only_when_the_caller_allows_it(cx: &mut TestAppContext) {
    let (client, backend, now) = freshness_client(cx);
    let fresh = Some(30_000);
    settle(
        cx,
        client.github_work_item_details("/repo", "o/r", WorkItemKind::Pr, 1, None),
    )
    .unwrap();
    settle(
        cx,
        client.github_work_item_thread("/repo", "o/r", WorkItemKind::Pr, 1, false, None),
    )
    .unwrap();
    settle(cx, client.github_pr_diff("/repo", "o/r", 1, false, None)).unwrap();
    assert_eq!(backend.calls().len(), 3);

    settle(
        cx,
        client.github_work_item_details("/repo", "o/r", WorkItemKind::Pr, 1, fresh),
    )
    .unwrap();
    settle(
        cx,
        client.github_work_item_thread("/repo", "o/r", WorkItemKind::Pr, 1, false, fresh),
    )
    .unwrap();
    settle(cx, client.github_pr_diff("/repo", "o/r", 1, false, fresh)).unwrap();
    assert_eq!(backend.calls().len(), 3);

    settle(
        cx,
        client.github_work_item_details("/repo", "o/r", WorkItemKind::Pr, 1, None),
    )
    .unwrap();
    assert_eq!(backend.calls().len(), 4);

    // Past the window, even a caller that allows reuse fetches again.
    *now.lock() += 30_000;
    settle(
        cx,
        client.github_work_item_thread("/repo", "o/r", WorkItemKind::Pr, 1, false, fresh),
    )
    .unwrap();
    assert_eq!(backend.calls().len(), 5);

    // Clearing the cache forgets when anything arrived.
    client.clear_inbox_cache();
    settle(cx, client.github_pr_diff("/repo", "o/r", 1, false, fresh)).unwrap();
    assert_eq!(backend.calls().len(), 6);
}

#[gpui::test]
fn github_prefetch_warms_only_what_is_not_cached(cx: &mut TestAppContext) {
    let (client, backend, _) = freshness_client(cx);
    settle(
        cx,
        client.github_work_item_thread("/repo", "o/r", WorkItemKind::Pr, 1, false, None),
    )
    .unwrap();
    client.prefetch_github_work_item("/repo", "o/r", WorkItemKind::Pr, 1);
    cx.run_until_parked();
    assert_eq!(backend.count("git_github_work_item"), 1);
    assert_eq!(backend.count("git_github_work_item_details"), 1);
    assert_eq!(backend.count("git_github_work_item_thread"), 1);
    assert_eq!(backend.count("git_github_pr_diff"), 1);
    assert!(client.peek_github_pr_diff("o/r", 1, false).is_some());

    // Issues have no diff to warm, and a second hover finds everything cached.
    client.prefetch_github_work_item("/repo", "o/r", WorkItemKind::Pr, 1);
    client.prefetch_github_work_item("/repo", "o/r", WorkItemKind::Issue, 2);
    cx.run_until_parked();
    assert_eq!(backend.count("git_github_pr_diff"), 1);
    assert_eq!(backend.count("git_github_work_item_details"), 2);
    assert_eq!(backend.count("git_github_work_item_thread"), 2);
}

#[gpui::test]
fn the_list_cache_stays_fresh_for_thirty_seconds_and_dedupes_fetches(cx: &mut TestAppContext) {
    let now = Arc::new(Mutex::new(1_000_000i64));
    let clock = now.clone();
    let backend = super::backend::fake::FakeBackend::new(|command, _| {
        if command == "git_github_repositories" {
            return Ok(json!(["acme/web"]));
        }
        if command == "git_github_work_items" {
            return Ok(json!([]));
        }
        status_off(command).unwrap_or_else(|| unexpected(command))
    });
    let client = super::client::InboxClient::with_clock(
        backend.clone(),
        monocode_settings::Kv::in_memory(),
        cx.executor(),
        Arc::new(move || *clock.lock()),
    );
    let projects = paths(&["/tmp/web"]);
    let q = query(false, InboxState::Open);
    let first = client.list_inbox_items(&projects, &q, false);
    let second = client.list_inbox_items(&projects, &q, false);
    settle(cx, first).unwrap();
    settle(cx, second).unwrap();
    assert_eq!(backend.count("git_github_work_items"), 2);
    assert!(client.inbox_list_is_fresh(&projects, &q, client.now()));
    settle(cx, client.list_inbox_items(&projects, &q, false)).unwrap();
    assert_eq!(backend.count("git_github_work_items"), 2);
    *now.lock() += 30_000;
    assert!(!client.inbox_list_is_fresh(&projects, &q, client.now()));
    settle(cx, client.list_inbox_items(&projects, &q, false)).unwrap();
    assert_eq!(backend.count("git_github_work_items"), 4);
    assert!(client.peek_inbox_list(&projects, &q).is_some());
    client.clear_inbox_cache();
    assert!(client.peek_inbox_list(&projects, &q).is_none());
}

// jira.test.ts

fn jira_issue() -> Value {
    json!({
        "provider": "jira",
        "kind": "jira",
        "id": "10042",
        "identifier": "ENG-42",
        "number": 42,
        "title": "Fix auth",
        "url": "https://acme.atlassian.net/browse/ENG-42",
        "state": "In Progress",
        "stateType": "indeterminate",
        "updatedAt": "2026-09-23T10:00:00Z",
        "labels": [],
        "assignees": [],
        "draft": false,
        "repo": "ENG",
        "teamId": "10000",
        "teamName": "Engineering",
        "projectPath": "",
    })
}

pub(crate) fn jira_inbox_item() -> InboxItem {
    let issue: TrackerIssue = serde_json::from_value(jira_issue()).unwrap();
    super::jira::jira_issue_to_inbox_item(&issue)
}

fn jira_projects() -> Value {
    json!([
        { "id": "10000", "key": "ENG", "name": "Engineering" },
        { "id": "10001", "key": "OPS", "name": "Operations" },
    ])
}

pub(crate) fn jira_handler(command: &str, _args: &Value) -> Result<Value, String> {
    match command {
        "jira_status" => Ok(json!({ "connected": true })),
        _ if command.ends_with("_status") => Ok(json!({ "connected": false })),
        "jira_list_projects" => Ok(jira_projects()),
        "jira_list_issues" => Ok(json!([jira_issue()])),
        "jira_issue_details" => Ok(json!({ "body": "Reproduction steps", "author": "Ada" })),
        "jira_issue_thread" => Ok(json!({ "comments": [], "truncated": false })),
        "jira_issue_comment" => Ok(json!(
            "https://acme.atlassian.net/browse/ENG-42?focusedCommentId=7"
        )),
        "jira_set_config" => Ok(json!({ "connected": false, "site": "", "email": "" })),
        _ => unexpected(command),
    }
}

#[gpui::test]
fn jira_loads_account_wide_issues_without_a_local_repository(cx: &mut TestAppContext) {
    let (client, backend) = client(cx, jira_handler);
    let result = settle(
        cx,
        client.list_inbox_items(&[], &query(true, InboxState::Open), false),
    )
    .unwrap();
    assert_eq!(result.items, vec![jira_inbox_item()]);
    assert!(result.errors.is_empty());
    assert_eq!(
        backend.calls_to("jira_list_issues"),
        [json!({ "assignedToMe": true, "state": "open", "projectIds": [] })]
    );
    let item = jira_inbox_item();
    assert_eq!(item.status(), "Open");
    let mut done = item.clone();
    done.state_type = Some("done".into());
    assert_eq!(done.status(), "Closed");
}

#[gpui::test]
fn jira_filters_projects_before_fetching_and_skips_when_every_project_is_hidden(
    cx: &mut TestAppContext,
) {
    let (client, backend) = client(cx, jira_handler);
    let hidden = |ids: &[&str]| InboxQuery {
        jira_hidden_project_ids: Some(paths(ids)),
        ..query(true, InboxState::Open)
    };
    settle(cx, client.list_inbox_items(&[], &hidden(&["10001"]), false)).unwrap();
    assert_eq!(
        backend.calls_to("jira_list_issues")[0]["projectIds"],
        json!(["10000"])
    );
    backend.clear_calls();
    let result = settle(
        cx,
        client.list_inbox_items(&[], &hidden(&["10000", "10001"]), false),
    )
    .unwrap();
    assert!(result.items.is_empty());
    assert_eq!(backend.count("jira_list_issues"), 0);
    let projects: Vec<TrackerGroup> = serde_json::from_value(jira_projects()).unwrap();
    assert_eq!(
        jira_project_ids_for_fetch(&projects, &["deleted".into()]),
        None
    );
}

#[gpui::test]
fn jira_failure_keeps_github_items(cx: &mut TestAppContext) {
    let (client, _) = client(cx, |command, args| match command {
        "jira_status" => Err("Jira settings are invalid".into()),
        "git_github_repositories" => Ok(json!(["acme/web"])),
        "git_github_work_items" => Ok(if args["kind"] == "issue" {
            let mut item = jira_issue();
            item["kind"] = json!("issue");
            item["repo"] = json!("acme/web");
            json!([item])
        } else {
            json!([])
        }),
        _ => jira_handler(command, args),
    });
    let result = settle(
        cx,
        client.list_inbox_items(&paths(&["/repo"]), &query(true, InboxState::Open), false),
    )
    .unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].provider, InboxProvider::Github);
    let mut errors = InboxProviderErrors::new();
    errors.set(InboxProvider::Jira, "Jira settings are invalid");
    assert_eq!(result.errors, errors);
}

#[gpui::test]
fn jira_comment_invalidates_the_thread_and_suppresses_the_authors_notification(
    cx: &mut TestAppContext,
) {
    let (client, backend) = client(cx, jira_handler);
    settle(cx, client.jira_issue_thread("ENG-42", false)).unwrap();
    assert!(client.peek_jira_issue_thread("ENG-42").is_some());
    settle(
        cx,
        client.jira_issue_comment("10042", "ENG-42", "  Fixed\n\nPlease check  "),
    )
    .unwrap();
    assert_eq!(
        backend.calls_to("jira_issue_comment"),
        [json!({ "key": "ENG-42", "body": "Fixed\n\nPlease check" })]
    );
    assert!(client.peek_jira_issue_thread("ENG-42").is_none());
    let issue = jira_inbox_item();
    assert!(client.consume_inbox_self_activity(&issue));
    assert!(!client.consume_inbox_self_activity(&issue));
}

#[gpui::test]
fn jira_clears_credentials_and_cached_descriptions_on_disconnect_and_reconnect(
    cx: &mut TestAppContext,
) {
    let (client, backend) = client(cx, jira_handler);
    settle(cx, client.jira_issue_details("ENG-42")).unwrap();
    settle(cx, client.disconnect_jira()).unwrap();
    assert!(
        backend
            .calls_to("jira_set_config")
            .contains(&json!({ "site": "", "email": "", "token": "" }))
    );
    assert!(client.peek_jira_issue_details("ENG-42").is_none());
    settle(cx, client.jira_issue_details("ENG-42")).unwrap();
    settle(
        cx,
        client.save_jira_config(" acme ", " ada@example.com ", " token "),
    )
    .unwrap();
    assert!(
        backend
            .calls_to("jira_set_config")
            .contains(&json!({ "site": "acme", "email": "ada@example.com", "token": "token" }))
    );
    assert!(client.peek_jira_issue_details("ENG-42").is_none());
}

#[gpui::test]
fn jira_does_not_restore_a_previous_accounts_cache_when_a_request_finishes_late(
    cx: &mut TestAppContext,
) {
    let (client, backend) = client(cx, jira_handler);
    let held = backend.hold_next("jira_issue_details");
    let pending = client.jira_issue_details("ENG-42");
    cx.run_until_parked();
    client.clear_jira_cache();
    held.resolve(json!({ "body": "Old account", "author": "Ada" }));
    settle(cx, pending).unwrap();
    assert!(client.peek_jira_issue_details("ENG-42").is_none());
}

// sessionWorkItem.test.ts: the work item cache.

fn pr_42(state: &str, updated_at: &str) -> Value {
    json!({
        "kind": "pr",
        "repo": "openai/codex",
        "number": 42,
        "title": "Faster linked navigation",
        "url": "https://github.com/openai/codex/pull/42",
        "state": state,
        "updatedAt": updated_at,
        "labels": [],
        "assignees": [],
        "draft": false,
    })
}

#[gpui::test]
fn fetches_an_exact_cache_miss_once_and_reuses_that_result(cx: &mut TestAppContext) {
    let answer = Arc::new(Mutex::new(pr_42("open", "2026-09-09T12:00:00Z")));
    let current = answer.clone();
    let (client, backend) = client(cx, move |_, _| Ok(current.lock().clone()));
    let result: GithubWorkItem =
        serde_json::from_value(pr_42("open", "2026-09-09T12:00:00Z")).unwrap();
    assert_eq!(
        settle(
            cx,
            client.github_work_item("/tmp/codex", "openai/codex", WorkItemKind::Pr, 42, false)
        )
        .unwrap(),
        result
    );
    assert_eq!(
        settle(
            cx,
            client.github_work_item("/tmp/codex", "openai/codex", WorkItemKind::Pr, 42, false)
        )
        .unwrap(),
        result
    );
    *answer.lock() = pr_42("open", "2026-09-09T12:01:00Z");
    let refreshed: GithubWorkItem =
        serde_json::from_value(pr_42("open", "2026-09-09T12:01:00Z")).unwrap();
    assert_eq!(
        settle(
            cx,
            client.github_work_item("/tmp/codex", "openai/codex", WorkItemKind::Pr, 42, true)
        )
        .unwrap(),
        refreshed
    );
    assert_eq!(
        settle(
            cx,
            client.github_work_item("/tmp/codex", "openai/codex", WorkItemKind::Pr, 42, false)
        )
        .unwrap(),
        refreshed
    );
    assert_eq!(backend.calls().len(), 2);
    assert_eq!(
        backend.calls()[0],
        (
            "git_github_work_item".to_string(),
            json!({ "cwd": "/tmp/codex", "repo": "openai/codex", "kind": "pr", "number": 42 })
        )
    );
}

#[gpui::test]
fn runs_a_pull_request_action_and_caches_the_refreshed_result(cx: &mut TestAppContext) {
    let (client, backend) = client(cx, |_, _| Ok(pr_42("merged", "2026-09-09T12:05:00Z")));
    let merged: GithubWorkItem =
        serde_json::from_value(pr_42("merged", "2026-09-09T12:05:00Z")).unwrap();
    assert_eq!(
        settle(
            cx,
            client.github_pr_action("/tmp/codex", "openai/codex", 42, GithubPrAction::Squash)
        )
        .unwrap(),
        merged
    );
    assert_eq!(
        settle(
            cx,
            client.github_work_item("/tmp/codex", "openai/codex", WorkItemKind::Pr, 42, false)
        )
        .unwrap(),
        merged
    );
    assert_eq!(
        backend.calls(),
        vec![(
            "git_github_pr_action".to_string(),
            json!({ "cwd": "/tmp/codex", "repo": "openai/codex", "number": 42, "action": "squash" })
        )]
    );
}

#[gpui::test]
fn signals_reach_subscribers_until_they_unsubscribe(cx: &mut TestAppContext) {
    let (client, _) = client(cx, |command, _| unexpected(command));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let subscription = client.subscribe(move |signal| sink.lock().push(signal));
    client.record_inbox_self_activity(super::inbox_self_activity::InboxSelfActivityTarget::new(
        InboxProvider::Github,
    ));
    client.save_hidden_jira_project_ids(&["p".into()]);
    drop(subscription);
    client.notify_linear_change();
    assert_eq!(
        *seen.lock(),
        [
            super::client::InboxSignal::SelfActivity,
            super::client::InboxSignal::JiraChange
        ]
    );
}
