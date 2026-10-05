//! Port of src/features/sessions/model/sessionWorkItem.ts: the GitHub issue
//! or pull request a session is about, from its first message, an Inbox
//! card, or an automation event, and the sessions that match an Inbox row.

use std::sync::LazyLock;

use futures::FutureExt;
use futures::future::BoxFuture;
use monocode_core::Extra;
use monocode_core::js;
use monocode_core::session::LinkedWorkItem;
use monocode_harness::core::session_title::{GeneratedWorkItemHint, WorkItemKind as HintKind};
use regex::Regex;
use serde_json::json;

use super::client::InboxClient;
use super::github_tasks::{IdentityFields, inbox_identity_key};
use super::types::{GitPr, InboxItem, InboxProvider, WorkItemKind, work_kind, work_kind_str};

static GITHUB_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)https?://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/(pull|issues)/(\d+)(?-u:\b)",
    )
    .expect("valid regex")
});
static VALID_REPO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").expect("valid regex"));
static PR_HINT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:pr|pull\s+request)\s*#?\s*(\d+)(?-u:\b)").expect("valid regex")
});
static ISSUE_HINT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)issue\s*#?\s*(\d+)(?-u:\b)").expect("valid regex"));
static CURRENT_PR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:this|the|current)\s+(?:pr|pull\s+request)(?-u:\b)")
        .expect("valid regex")
});
static AUTOMATION_EVENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^github:(pr|issue):([^/:]+/[^/:]+):([1-9]\d*)$").expect("valid regex")
});

/// `validNumber`: a safe positive integer.
fn valid_number(value: i64) -> bool {
    value > 0 && value <= 9_007_199_254_740_991
}

/// `Number(digits)` for a matched digit run, `None` past the safe range.
fn parse_digits(digits: &str) -> Option<i64> {
    let value: f64 = digits.parse().ok()?;
    (value <= 9_007_199_254_740_991.0).then_some(value as i64)
}

/// `githubUrl`.
fn github_url(repo: &str, kind: WorkItemKind, number: i64) -> String {
    format!(
        "https://github.com/{repo}/{}/{number}",
        if kind == WorkItemKind::Pr {
            "pull"
        } else {
            "issues"
        }
    )
}

fn linked(kind: WorkItemKind, repo: &str, number: i64, url: String) -> LinkedWorkItem {
    LinkedWorkItem {
        kind,
        repo: repo.to_string(),
        number,
        url,
        extra: Extra::new(),
    }
}

/// `parseGithubWorkItemUrl`: the first GitHub issue or pull request URL in
/// the message.
pub fn parse_github_work_item_url(message: &str) -> Option<LinkedWorkItem> {
    let captures = GITHUB_URL_RE.captures(message)?;
    let number = parse_digits(&captures[4]).filter(|number| valid_number(*number))?;
    let repo = format!("{}/{}", &captures[1], &captures[2]);
    let kind = if captures[3].eq_ignore_ascii_case("pull") {
        WorkItemKind::Pr
    } else {
        WorkItemKind::Issue
    };
    Some(linked(kind, &repo, number, github_url(&repo, kind, number)))
}

/// A work item kind and number named in the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkItemHint {
    pub kind: WorkItemKind,
    pub number: i64,
}

impl From<GeneratedWorkItemHint> for WorkItemHint {
    fn from(hint: GeneratedWorkItemHint) -> Self {
        Self {
            kind: match hint.kind {
                HintKind::Issue => WorkItemKind::Issue,
                HintKind::Pr => WorkItemKind::Pr,
            },
            number: hint.number,
        }
    }
}

/// `explicitHint`.
fn explicit_hint(message: &str) -> Option<WorkItemHint> {
    for (kind, pattern) in [
        (WorkItemKind::Pr, &*PR_HINT_RE),
        (WorkItemKind::Issue, &*ISSUE_HINT_RE),
    ] {
        if let Some(number) = pattern
            .captures(message)
            .and_then(|captures| parse_digits(&captures[1]))
            .filter(|number| valid_number(*number))
        {
            return Some(WorkItemHint { kind, number });
        }
    }
    None
}

/// `validRepo`.
pub fn valid_repo(repo: &str) -> bool {
    VALID_REPO_RE.is_match(repo)
}

fn repo_from_github_url(url: &str) -> Option<String> {
    GITHUB_URL_RE
        .captures(url)
        .map(|captures| format!("{}/{}", &captures[1], &captures[2]))
}

fn references_current_pr(message: &str) -> bool {
    CURRENT_PR_RE.is_match(message)
}

impl InboxClient {
    /// `gitPrStatus`: the pull request for the checkout's current branch.
    pub fn git_pr_status(&self, cwd: &str) -> super::client::Pending<Option<GitPr>> {
        self.request("git_pr_status", json!({ "cwd": cwd }))
    }

    /// `resolveLinkedWorkItem`: explicit first-message context resolved to
    /// one stable GitHub identity. A URL wins, then an explicit or generated
    /// number against the checkout's repository, then "this PR".
    pub fn resolve_linked_work_item(
        &self,
        message: &str,
        cwd: &str,
        generated_hint: Option<GeneratedWorkItemHint>,
    ) -> BoxFuture<'static, Option<LinkedWorkItem>> {
        if let Some(from_url) = parse_github_work_item_url(message) {
            return futures::future::ready(Some(from_url)).boxed();
        }
        let hint = explicit_hint(message).or(generated_hint.map(WorkItemHint::from));
        let client = self.clone();
        let cwd = cwd.to_string();
        if let Some(hint) = hint.filter(|hint| valid_number(hint.number)) {
            return async move {
                let repo = client.github_repo(&cwd).await.ok()?;
                if !valid_repo(&repo) {
                    return None;
                }
                Some(linked(
                    hint.kind,
                    &repo,
                    hint.number,
                    github_url(&repo, hint.kind, hint.number),
                ))
            }
            .boxed();
        }
        if !references_current_pr(message) {
            return futures::future::ready(None).boxed();
        }
        async move {
            let pr = client.git_pr_status(&cwd).await.ok()??;
            if !valid_number(pr.number) {
                return None;
            }
            let repo = match repo_from_github_url(&pr.url) {
                Some(repo) => repo,
                None => client.github_repo(&cwd).await.ok()?,
            };
            if !valid_repo(&repo) {
                return None;
            }
            let url = if pr.url.is_empty() {
                github_url(&repo, WorkItemKind::Pr, pr.number)
            } else {
                pr.url.clone()
            };
            Some(linked(WorkItemKind::Pr, &repo, pr.number, url))
        }
        .boxed()
    }
}

/// `linkedWorkItemFromInboxItem`: GitHub issues and pull requests only.
pub fn linked_work_item_from_inbox_item(item: &InboxItem) -> Option<LinkedWorkItem> {
    if item.provider != InboxProvider::Github
        || !valid_number(item.number)
        || !valid_repo(&item.repo)
    {
        return None;
    }
    let kind = work_kind(item.kind)?;
    let url = if item.url.is_empty() {
        github_url(&item.repo, kind, item.number)
    } else {
        item.url.clone()
    };
    Some(linked(kind, &item.repo, item.number, url))
}

/// `linkedWorkItemFromAutomationEvent`: the GitHub identity persisted on an
/// event-triggered automation run.
pub fn linked_work_item_from_automation_event(
    trigger: &str,
    event_kind: Option<&str>,
    event_key: Option<&str>,
) -> Option<LinkedWorkItem> {
    if trigger != "event" || event_kind != Some("github") {
        return None;
    }
    let captures = AUTOMATION_EVENT_RE.captures(js::trim(event_key.unwrap_or("")))?;
    let number = parse_digits(&captures[3]).filter(|number| valid_number(*number))?;
    let kind = if captures[1].eq_ignore_ascii_case("pr") {
        WorkItemKind::Pr
    } else {
        WorkItemKind::Issue
    };
    let repo = captures[2].to_string();
    if !valid_repo(&repo) {
        return None;
    }
    Some(linked(kind, &repo, number, github_url(&repo, kind, number)))
}

/// `inboxItemMatchesLinkedWorkItem`.
pub fn inbox_item_matches_linked_work_item(item: &InboxItem, linked: &LinkedWorkItem) -> bool {
    item.provider == InboxProvider::Github
        && work_kind(item.kind) == Some(linked.kind)
        && item.number == linked.number
        && js::trim(&item.repo).to_lowercase() == js::trim(&linked.repo).to_lowercase()
}

/// `linkedWorkItemInboxKey`: the Inbox selection key, without building a
/// full Inbox item.
pub fn linked_work_item_inbox_key(linked: &LinkedWorkItem) -> String {
    format!(
        "github:{}",
        inbox_identity_key(IdentityFields {
            provider: None,
            kind: work_kind_str(linked.kind),
            number: linked.number,
            repo: &linked.repo,
            url: &linked.url,
            identifier: None,
            id: None,
        })
    )
}

/// `relatedSessionsForInboxItem`: sessions whose stored GitHub identity
/// matches an Inbox row.
pub fn related_sessions_for_inbox_item<'a, T>(
    item: &InboxItem,
    sessions: &'a [T],
    linked: impl Fn(&T) -> Option<&LinkedWorkItem>,
) -> Vec<&'a T> {
    if item.provider != InboxProvider::Github {
        return Vec::new();
    }
    sessions
        .iter()
        .filter(|session| {
            linked(session).is_some_and(|linked| inbox_item_matches_linked_work_item(item, linked))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use serde_json::json;

    use super::*;
    use crate::inbox::client::test_support::{client, settle, settle_future};
    use crate::inbox::github_tasks::inbox_item_key;
    use crate::inbox::github_tasks::test_items::{item, with};
    use crate::inbox::types::InboxKind;

    fn work(kind: WorkItemKind, repo: &str, number: i64, url: &str) -> LinkedWorkItem {
        linked(kind, repo, number, url.into())
    }

    #[test]
    fn parses_a_github_pull_request_url_without_repository_lookup() {
        assert_eq!(
            parse_github_work_item_url(
                "Please review https://github.com/openai/codex/pull/321?diff=split"
            ),
            Some(work(
                WorkItemKind::Pr,
                "openai/codex",
                321,
                "https://github.com/openai/codex/pull/321"
            ))
        );
    }

    #[test]
    fn creates_a_stable_link_from_a_github_inbox_item() {
        let issue = with(item(12, ""), |row| {
            row.repo = "openai/codex".into();
            row.url = "https://github.com/openai/codex/issues/12".into();
        });
        let link = linked_work_item_from_inbox_item(&issue).unwrap();
        assert_eq!(
            link,
            work(
                WorkItemKind::Issue,
                "openai/codex",
                12,
                "https://github.com/openai/codex/issues/12"
            )
        );
        assert!(inbox_item_matches_linked_work_item(&issue, &link));
        assert_eq!(linked_work_item_inbox_key(&link), inbox_item_key(&issue));
    }

    #[test]
    fn restores_a_linked_pr_from_a_persisted_automation_event() {
        assert_eq!(
            linked_work_item_from_automation_event(
                "event",
                Some("github"),
                Some("github:pr:openai/codex:321")
            ),
            Some(work(
                WorkItemKind::Pr,
                "openai/codex",
                321,
                "https://github.com/openai/codex/pull/321"
            ))
        );
    }

    #[test]
    fn does_not_link_non_github_or_malformed_automation_events() {
        assert_eq!(
            linked_work_item_from_automation_event(
                "event",
                Some("gitlab"),
                Some("gitlab:pr:openai/codex:321")
            ),
            None
        );
        assert_eq!(
            linked_work_item_from_automation_event(
                "event",
                Some("github"),
                Some("github:pr:missing-number")
            ),
            None
        );
    }

    #[gpui::test]
    fn resolves_an_explicit_pr_number_against_the_session_repository(cx: &mut TestAppContext) {
        let (client, backend) = client(cx, |_, _| Ok(json!("openai/codex")));
        let resolved = settle_future(
            cx,
            client.resolve_linked_work_item("Please fix PR #42", "/tmp/codex", None),
        );
        assert_eq!(
            resolved,
            Some(work(
                WorkItemKind::Pr,
                "openai/codex",
                42,
                "https://github.com/openai/codex/pull/42"
            ))
        );
        assert_eq!(
            backend.calls(),
            vec![(
                "git_github_repo".to_string(),
                json!({ "cwd": "/tmp/codex" })
            )]
        );
    }

    #[gpui::test]
    fn resolves_this_pr_through_the_branch_status(cx: &mut TestAppContext) {
        let (client, _) = client(cx, |command, _| match command {
            "git_pr_status" => Ok(json!({
                "number": 7,
                "title": "Fix",
                "url": "https://github.com/acme/web/pull/7",
                "state": "OPEN",
            })),
            _ => Err("no".into()),
        });
        let resolved = settle_future(
            cx,
            client.resolve_linked_work_item("Review this PR please", "/tmp/web", None),
        );
        assert_eq!(
            resolved,
            Some(work(
                WorkItemKind::Pr,
                "acme/web",
                7,
                "https://github.com/acme/web/pull/7"
            ))
        );
        let none = client
            .resolve_linked_work_item("Hello there", "/tmp/web", None)
            .now_or_never()
            .unwrap();
        assert_eq!(none, None);
        let hinted = settle_future(
            cx,
            client.resolve_linked_work_item(
                "Work on it",
                "/tmp/web",
                Some(GeneratedWorkItemHint {
                    kind: HintKind::Issue,
                    number: 3,
                }),
            ),
        );
        assert_eq!(hinted, None);
        assert!(
            settle(cx, client.git_pr_status("/tmp/web"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn does_not_associate_a_linear_inbox_item() {
        let linear = with(item(12, ""), |row| {
            row.provider = InboxProvider::Linear;
            row.kind = InboxKind::Linear;
            row.repo = String::new();
        });
        assert_eq!(linked_work_item_from_inbox_item(&linear), None);
    }

    #[test]
    fn finds_sessions_related_to_the_same_github_inbox_item() {
        let pr = with(item(42, ""), |row| {
            row.kind = InboxKind::Pr;
            row.repo = "Acme/App".into();
        });
        let matching = work(
            WorkItemKind::Pr,
            "acme/app",
            42,
            "https://github.com/acme/app/pull/42",
        );
        let mut other_number = matching.clone();
        other_number.number = 43;
        let mut other_kind = matching.clone();
        other_kind.kind = WorkItemKind::Issue;
        let sessions = vec![
            ("matching", Some(matching)),
            ("other-number", Some(other_number)),
            ("other-kind", Some(other_kind)),
            ("unlinked", None),
        ];
        let related: Vec<&str> =
            related_sessions_for_inbox_item(&pr, &sessions, |session| session.1.as_ref())
                .into_iter()
                .map(|session| session.0)
                .collect();
        assert_eq!(related, ["matching"]);
        let linear = with(pr, |row| {
            row.provider = InboxProvider::Linear;
            row.kind = InboxKind::Linear;
        });
        assert!(
            related_sessions_for_inbox_item(&linear, &sessions, |session| session.1.as_ref())
                .is_empty()
        );
    }
}
