//! Port of the pure parts of src/features/inbox/model/githubTasks.ts: query
//! text, avatars, labels, cache keys, identity keys, dedupe and sort, search,
//! status labels, and the start drafts and composer cards. The caches and
//! fetches live in `client`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use monocode_core::inbox::InboxComposerCard;
use monocode_core::js;

use super::text::{locale_compare, non_empty, non_empty_trimmed};
use super::time::date_parse_or_zero;
use super::types::{
    GithubWorkItem, GithubWorkItemQuery, InboxItem, InboxKind, InboxProvider, InboxQuery,
    InboxState, WorkItemKind, kind_str, work_kind_str,
};
use crate::runtime::util::project_path::normalize_project_path;

pub use monocode_core::inbox::compose_inbox_message;

/// `INBOX_CACHE_FRESH_MS`.
pub const INBOX_CACHE_FRESH_MS: i64 = 30_000;

/// `INBOX_ALL_LIMIT`: closed history competes for the same slots, so an
/// unfiltered fetch needs the wider page.
pub const INBOX_ALL_LIMIT: u32 = 100;

/// `query.state === "all" ? INBOX_ALL_LIMIT : undefined`.
pub fn limit_for_state(state: InboxState) -> Option<u32> {
    (state == InboxState::All).then_some(INBOX_ALL_LIMIT)
}

/// `inboxListCacheKey`.
pub fn inbox_list_cache_key(projects: &[String], query: &InboxQuery) -> String {
    let mut paths: Vec<String> = unique_inbox_projects(projects)
        .iter()
        .map(|path| normalize_project_path(path))
        .collect();
    paths.sort();
    let mut teams = query.linear_hidden_team_ids.clone().unwrap_or_default();
    teams.sort();
    let mut jira_projects = query.jira_hidden_project_ids.clone().unwrap_or_default();
    jira_projects.sort();
    format!(
        "{}:{}:{}:{}:{}",
        if query.assigned_to_me { 1 } else { 0 },
        query.state.as_str(),
        paths.join("|"),
        teams.join(","),
        jira_projects.join(",")
    )
}

/// `formatGithubQuery`.
pub fn format_github_query(query: &GithubWorkItemQuery) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if query.assigned_to_me {
        parts.push("assignee:@me");
    }
    parts.push(if query.kind == WorkItemKind::Pr {
        "is:pr"
    } else {
        "is:issue"
    });
    if query.state == InboxState::Open {
        parts.push("is:open");
    }
    let text = js::trim(&query.search);
    if !text.is_empty() {
        parts.push(text);
    }
    parts.join(" ")
}

/// `githubAvatarUrl`.
pub fn github_avatar_url(login: &str, size: Option<u32>) -> String {
    let name = js::trim(login);
    if name.is_empty() {
        return String::new();
    }
    format!(
        "https://avatars.githubusercontent.com/{}?s={}",
        js::encode_uri_component(name),
        size.unwrap_or(64)
    )
}

/// `inboxPersonAvatarUrl`.
pub fn inbox_person_avatar_url(
    provider: InboxProvider,
    login: &str,
    avatar_url: Option<&str>,
) -> String {
    let explicit = avatar_url.map(js::trim).unwrap_or("");
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    if provider == InboxProvider::Github {
        return github_avatar_url(login, None);
    }
    String::new()
}

/// `workItemLookupKey` and `detailsCacheKey`.
pub fn details_cache_key(repo: &str, kind: WorkItemKind, number: i64) -> String {
    format!(
        "{}:{}:{}",
        js::trim(repo).to_lowercase(),
        work_kind_str(kind),
        number
    )
}

/// `prDiffCacheKey`.
pub fn pr_diff_cache_key(repo: &str, number: i64, full_context: bool) -> String {
    format!(
        "{}:pr:{}{}",
        js::trim(repo).to_lowercase(),
        number,
        if full_context { ":full" } else { "" }
    )
}

/// `githubReviewDecisionLabel`.
pub fn github_review_decision_label(decision: &str) -> &'static str {
    match js::trim(decision).to_uppercase().as_str() {
        "APPROVED" => "Approved",
        "CHANGES_REQUESTED" => "Changes requested",
        "REVIEW_REQUIRED" => "Review required",
        _ => "",
    }
}

/// `githubReviewStateLabel`.
pub fn github_review_state_label(state: &str) -> &'static str {
    match js::trim(state).to_uppercase().as_str() {
        "APPROVED" => "Approved",
        "CHANGES_REQUESTED" => "Requested changes",
        "DISMISSED" => "Dismissed",
        "COMMENTED" => "Commented",
        _ => "",
    }
}

/// `gitlabAttentionLabel`.
pub fn gitlab_attention_label(reason: &str) -> String {
    let action = js::trim(reason).to_lowercase();
    let label = match action.as_str() {
        "assigned" => "Assigned to you",
        "mentioned" | "directly_addressed" => "Mentioned you",
        "review_requested" => "Review requested",
        "review_submitted" => "Review submitted",
        "approval_required" => "Approval required",
        "build_failed" => "Pipeline failed",
        "unmergeable" => "Cannot be merged",
        "merge_train_removed" => "Removed from merge train",
        "member_access_requested" => "Access requested",
        "marked" => "Added to your to-dos",
        _ => {
            return action
                .split('_')
                .filter(|word| !word.is_empty())
                .enumerate()
                .map(|(index, word)| {
                    if index == 0 {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().chain(chars).collect(),
                            None => String::new(),
                        }
                    } else {
                        word.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    };
    label.to_string()
}

/// `uniqueInboxProjects`: normalized paths, first occurrence wins.
pub fn unique_inbox_projects(projects: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for project in projects {
        let path = normalize_project_path(project);
        if path.is_empty() || !seen.insert(path.clone()) {
            continue;
        }
        unique.push(path);
    }
    unique
}

/// A local checkout and the repository it resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRepo {
    pub path: String,
    pub repo: String,
}

impl ProjectRepo {
    pub fn new(path: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            repo: repo.into(),
        }
    }
}

/// `groupProjectsByRepo`: each repository once, with its first checkout.
pub fn group_projects_by_repo(resolved: &[ProjectRepo]) -> Vec<ProjectRepo> {
    let mut seen = HashSet::new();
    let mut grouped = Vec::new();
    for project in resolved {
        let repo = js::trim(&project.repo).to_lowercase();
        let key = if repo.is_empty() {
            format!("path:{}", normalize_project_path(&project.path))
        } else {
            repo
        };
        if !seen.insert(key) {
            continue;
        }
        grouped.push(ProjectRepo {
            path: project.path.clone(),
            repo: js::trim(&project.repo).to_string(),
        });
    }
    grouped
}

/// The `{ items, error? }` result of `collectInboxResults`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollectedInboxItems {
    pub items: Vec<InboxItem>,
    pub error: Option<String>,
}

/// `collectInboxResults`: the batches that succeeded, or the first error
/// when every one failed.
pub fn collect_inbox_results(
    settled: Vec<Result<Vec<InboxItem>, String>>,
    preferred_paths: &[String],
) -> CollectedInboxItems {
    let mut batches = Vec::new();
    let mut errors = Vec::new();
    for result in settled {
        match result {
            Ok(batch) => batches.push(batch),
            Err(error) => errors.push(error),
        }
    }
    if batches.is_empty() && !errors.is_empty() {
        return CollectedInboxItems {
            items: Vec::new(),
            error: Some(errors.swap_remove(0)),
        };
    }
    CollectedInboxItems {
        items: dedupe_inbox_items(batches.into_iter().flatten().collect(), preferred_paths),
        error: None,
    }
}

/// The fields `inboxIdentityKey` reads.
#[derive(Debug, Clone, Copy)]
pub struct IdentityFields<'a> {
    pub provider: Option<InboxProvider>,
    pub kind: &'a str,
    pub number: i64,
    pub repo: &'a str,
    pub url: &'a str,
    pub identifier: Option<&'a str>,
    pub id: Option<&'a str>,
}

impl InboxItem {
    pub fn identity_fields(&self) -> IdentityFields<'_> {
        IdentityFields {
            provider: Some(self.provider),
            kind: kind_str(self.kind),
            number: self.number,
            repo: &self.repo,
            url: &self.url,
            identifier: self.identifier.as_deref(),
            id: self.id.as_deref(),
        }
    }
}

/// `inboxIdentityKey`.
pub fn inbox_identity_key(item: IdentityFields<'_>) -> String {
    if item.provider == Some(InboxProvider::Linear) {
        if let Some(identity) =
            non_empty_trimmed(item.identifier).or_else(|| non_empty_trimmed(item.id))
        {
            return identity.to_lowercase();
        }
        return format!("linear:{}", item.number);
    }
    if item.provider == Some(InboxProvider::Jira) {
        // The numeric id survives an issue moving projects; its key does not.
        if let Some(identity) =
            non_empty_trimmed(item.id).or_else(|| non_empty_trimmed(item.identifier))
        {
            return identity.to_lowercase();
        }
        return format!("jira:{}", item.number);
    }
    let repo = js::trim(item.repo).to_lowercase();
    if !repo.is_empty() {
        return format!("{repo}:{}:{}", item.kind, item.number);
    }
    let url = js::trim(item.url).to_lowercase();
    if !url.is_empty() {
        return url;
    }
    format!("{}:{}", item.kind, item.number)
}

/// `inboxItemKey`.
pub fn inbox_item_key(item: &InboxItem) -> String {
    format!(
        "{}:{}",
        super::types::provider_str(item.provider),
        inbox_identity_key(item.identity_fields())
    )
}

/// `githubWorkItemKey`.
pub fn github_work_item_key(item: &GithubWorkItem) -> String {
    format!("{}:{}:{}", item.repo, work_kind_str(item.kind), item.number)
}

/// `dedupeInboxItems`: one card per provider identity, preferring the
/// earliest preferred checkout, then sorted.
pub fn dedupe_inbox_items(items: Vec<InboxItem>, preferred_paths: &[String]) -> Vec<InboxItem> {
    let mut rank: HashMap<String, usize> = HashMap::new();
    for (index, path) in preferred_paths.iter().enumerate() {
        // `new Map(entries)` keeps the last index for a repeated path.
        rank.insert(normalize_project_path(path), index);
    }
    let mut order: Vec<String> = Vec::new();
    let mut best: HashMap<String, InboxItem> = HashMap::new();
    for item in items {
        // Keyed with the provider prefix so identical repo/kind/number
        // triples from different providers never collapse into one card.
        let key = inbox_item_key(&item);
        match best.get(&key) {
            None => {
                order.push(key.clone());
                best.insert(key, item);
            }
            Some(current) => {
                if prefer_inbox_item(&item, current, &rank) {
                    best.insert(key, item);
                }
            }
        }
    }
    sort_inbox_items(
        order
            .into_iter()
            .filter_map(|key| best.remove(&key))
            .collect(),
    )
}

fn prefer_inbox_item(next: &InboxItem, current: &InboxItem, rank: &HashMap<String, usize>) -> bool {
    let next_rank = rank.get(&normalize_project_path(&next.project_path));
    let current_rank = rank.get(&normalize_project_path(&current.project_path));
    if next_rank != current_rank {
        return match (next_rank, current_rank) {
            (Some(next), Some(current)) => next < current,
            (Some(_), None) => true,
            _ => false,
        };
    }
    locale_compare(&next.project_path, &current.project_path) == Ordering::Less
}

/// `sortInboxItems`: newest update first, then project, kind, and number.
pub fn sort_inbox_items(mut items: Vec<InboxItem>) -> Vec<InboxItem> {
    items.sort_by(|a, b| {
        let updated = date_parse_or_zero(&b.updated_at) - date_parse_or_zero(&a.updated_at);
        if updated != 0 {
            return updated.cmp(&0);
        }
        if a.project_path != b.project_path {
            return locale_compare(&a.project_path, &b.project_path);
        }
        if a.kind != b.kind {
            return locale_compare(kind_str(a.kind), kind_str(b.kind));
        }
        b.number.cmp(&a.number)
    });
    items
}

/// `inboxItemStatus`.
pub fn inbox_item_status(
    kind: InboxKind,
    state: &str,
    draft: bool,
    state_type: Option<&str>,
) -> &'static str {
    if kind == InboxKind::Linear {
        let state_type = state_type.map(|value| js::trim(value).to_lowercase());
        if matches!(state_type.as_deref(), Some("completed" | "canceled")) {
            return "Closed";
        }
        return "Open";
    }
    if kind == InboxKind::Jira {
        return if state_type
            .map(|value| js::trim(value).to_lowercase())
            .as_deref()
            == Some("done")
        {
            "Closed"
        } else {
            "Open"
        };
    }
    if draft {
        return "Draft";
    }
    if state == "merged" {
        return "Merged";
    }
    if state == "closed" {
        return "Closed";
    }
    "Open"
}

impl InboxItem {
    /// `inboxItemStatus(item)`.
    pub fn status(&self) -> &'static str {
        inbox_item_status(
            self.kind,
            &self.state,
            self.draft,
            self.state_type.as_deref(),
        )
    }
}

/// `matchesInboxQuery`.
pub fn matches_inbox_query(item: &InboxItem, query: &str) -> bool {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return true;
    }
    let kind = match item.kind {
        InboxKind::Pr => {
            if item.provider == InboxProvider::Gitlab {
                "merge request mr"
            } else {
                "pull request pr"
            }
        }
        InboxKind::Linear => "linear issue",
        InboxKind::Jira => "jira issue",
        InboxKind::Issue => "issue",
    };
    // `[...].join(" ")` prints `undefined` holes as empty strings.
    let mut parts: Vec<String> = vec![
        item.title.clone(),
        item.repo.clone(),
        item.project_path.clone(),
        item.identifier.clone().unwrap_or_default(),
        item.team_name.clone().unwrap_or_default(),
        item.project_name.clone().unwrap_or_default(),
        item.attention_reason.clone().unwrap_or_default(),
        kind.to_string(),
        format!("#{}", item.number),
        item.number.to_string(),
    ];
    parts.extend(item.labels.iter().map(|label| label.name.clone()));
    parts.extend(item.assignees.iter().map(|person| person.login.clone()));
    parts.join(" ").to_lowercase().contains(&needle)
}

/// `filterInboxItems`.
pub fn filter_inbox_items(items: &[InboxItem], query: &str) -> Vec<InboxItem> {
    items
        .iter()
        .filter(|item| matches_inbox_query(item, query))
        .cloned()
        .collect()
}

/// `inboxItemRef`.
pub fn inbox_item_ref(
    provider: Option<InboxProvider>,
    number: i64,
    identifier: Option<&str>,
) -> String {
    if matches!(provider, Some(InboxProvider::Linear | InboxProvider::Jira))
        && let Some(identifier) = non_empty_trimmed(identifier)
    {
        return identifier.to_string();
    }
    format!("#{number}")
}

impl InboxItem {
    /// `inboxItemRef(item)`.
    pub fn item_ref(&self) -> String {
        inbox_item_ref(Some(self.provider), self.number, self.identifier.as_deref())
    }

    /// `item.provider === "linear" || item.provider === "jira"`.
    pub fn is_tracker(&self) -> bool {
        matches!(self.provider, InboxProvider::Linear | InboxProvider::Jira)
    }
}

/// `inboxStartDraft`.
pub fn inbox_start_draft(item: &InboxItem, body: Option<&str>) -> String {
    if item.is_tracker() {
        let provider = if item.provider == InboxProvider::Jira {
            "Jira"
        } else {
            "Linear"
        };
        let id = non_empty_trimmed(item.identifier.as_deref())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{provider} #{}", item.number));
        let title = js::trim(&item.title);
        let title = if title.is_empty() { id.as_str() } else { title };
        let mut lines = vec![
            format!("Work on this {provider} issue:"),
            String::new(),
            format!("{id} {title}"),
        ];
        let url = js::trim(&item.url);
        if !url.is_empty() {
            lines.push(url.to_string());
        }
        if let Some(description) = non_empty_trimmed(body) {
            lines.push(String::new());
            lines.push(description.to_string());
        }
        return format!("{}\n", lines.join("\n"));
    }
    let kind = if item.kind == InboxKind::Pr {
        "pull request"
    } else {
        "issue"
    };
    let provider = match item.provider {
        InboxProvider::Gitlab => "GitLab",
        InboxProvider::AzureDevops => "ADO",
        _ => "GitHub",
    };
    let provider_kind = if item.provider == InboxProvider::Gitlab && item.kind == InboxKind::Pr {
        "merge request"
    } else {
        kind
    };
    let title = js::trim(&item.title);
    let title = if title.is_empty() {
        format!("{provider} {provider_kind} #{}", item.number)
    } else {
        title.to_string()
    };
    let mut lines = vec![
        format!("Work on this {provider} {provider_kind}:"),
        String::new(),
        format!("#{} {title}", item.number),
    ];
    let url = js::trim(&item.url);
    if !url.is_empty() {
        lines.push(url.to_string());
    }
    format!("{}\n", lines.join("\n"))
}

/// `inboxComposerCard`: the compact chip shown above the composer when
/// starting from Inbox.
pub fn inbox_composer_card(item: &InboxItem, body: Option<&str>) -> InboxComposerCard {
    let tracker = item.is_tracker();
    let reference = item.item_ref();
    let title = js::trim(&item.title);
    InboxComposerCard {
        provider: item.provider,
        kind: item.kind,
        identifier: reference.clone(),
        title: if title.is_empty() {
            reference
        } else {
            title.to_string()
        },
        url: js::trim(&item.url).to_string(),
        source: if tracker {
            non_empty(item.team_name.as_deref())
                .map(str::to_string)
                .unwrap_or_else(|| item.repo.clone())
        } else {
            item.repo.clone()
        },
        labels: item.labels.iter().take(2).cloned().collect(),
        prompt: js::trim_end(&inbox_start_draft(item, body)).to_string(),
    }
}

#[cfg(test)]
pub(crate) mod test_items {
    use super::*;

    /// The `item(overrides)` helper of the TypeScript tests.
    pub fn item(number: i64, updated_at: &str) -> InboxItem {
        InboxItem {
            kind: InboxKind::Issue,
            number,
            title: "Item".into(),
            url: "https://github.com/acme/web/issues/1".into(),
            state: "open".into(),
            state_reason: None,
            created_at: None,
            updated_at: updated_at.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            draft: false,
            repo: "acme/web".into(),
            project_path: "/tmp/web".into(),
            provider: InboxProvider::Github,
            id: None,
            identifier: None,
            team_id: None,
            team_name: None,
            project_id: None,
            project_name: None,
            state_type: None,
            attention_reason: None,
        }
    }

    pub fn with(mut base: InboxItem, edit: impl FnOnce(&mut InboxItem)) -> InboxItem {
        edit(&mut base);
        base
    }
}

#[cfg(test)]
mod tests {
    use super::test_items::{item, with};
    use super::*;
    use crate::inbox::types::GithubLabel;

    fn numbers(items: &[InboxItem]) -> Vec<i64> {
        items.iter().map(|row| row.number).collect()
    }

    #[test]
    fn format_github_query_builds_the_assigned_open_issues_query() {
        assert_eq!(
            format_github_query(&GithubWorkItemQuery {
                kind: WorkItemKind::Issue,
                assigned_to_me: true,
                state: InboxState::Open,
                search: String::new(),
            }),
            "assignee:@me is:issue is:open"
        );
    }

    #[test]
    fn format_github_query_appends_free_text_after_the_qualifiers() {
        assert_eq!(
            format_github_query(&GithubWorkItemQuery {
                kind: WorkItemKind::Pr,
                assigned_to_me: false,
                state: InboxState::Open,
                search: "  checkout  ".into(),
            }),
            "is:pr is:open checkout"
        );
    }

    #[test]
    fn github_avatar_url_builds_encodes_and_blanks() {
        assert_eq!(
            github_avatar_url("maya", None),
            "https://avatars.githubusercontent.com/maya?s=64"
        );
        assert_eq!(
            github_avatar_url("dependabot[bot]", None),
            "https://avatars.githubusercontent.com/dependabot%5Bbot%5D?s=64"
        );
        assert_eq!(github_avatar_url("  ", None), "");
    }

    #[test]
    fn inbox_person_avatar_url_prefers_explicit_and_only_invents_github() {
        assert_eq!(
            inbox_person_avatar_url(
                InboxProvider::Linear,
                "Ada",
                Some("https://uploads.linear.app/ada.png")
            ),
            "https://uploads.linear.app/ada.png"
        );
        assert_eq!(
            inbox_person_avatar_url(InboxProvider::Github, "maya", None),
            "https://avatars.githubusercontent.com/maya?s=64"
        );
        assert_eq!(
            inbox_person_avatar_url(InboxProvider::Linear, "Ada", None),
            ""
        );
        assert_eq!(
            inbox_person_avatar_url(
                InboxProvider::Gitlab,
                "maya",
                Some("https://gitlab.example.com/uploads/maya.png")
            ),
            "https://gitlab.example.com/uploads/maya.png"
        );
    }

    #[test]
    fn review_labels() {
        assert_eq!(github_review_decision_label("APPROVED"), "Approved");
        assert_eq!(
            github_review_decision_label("changes_requested"),
            "Changes requested"
        );
        assert_eq!(
            github_review_decision_label("REVIEW_REQUIRED"),
            "Review required"
        );
        assert_eq!(github_review_decision_label(""), "");
        assert_eq!(github_review_state_label("APPROVED"), "Approved");
        assert_eq!(github_review_state_label("COMMENTED"), "Commented");
        assert_eq!(github_review_state_label("PENDING"), "");
    }

    #[test]
    fn gitlab_attention_labels() {
        assert_eq!(gitlab_attention_label("assigned"), "Assigned to you");
        assert_eq!(gitlab_attention_label("mentioned"), "Mentioned you");
        assert_eq!(
            gitlab_attention_label("review_requested"),
            "Review requested"
        );
        assert_eq!(gitlab_attention_label("marked"), "Added to your to-dos");
        assert_eq!(gitlab_attention_label("unknown_action"), "Unknown action");
        assert_eq!(gitlab_attention_label(""), "");
    }

    #[test]
    fn sort_orders_by_newest_update_mixing_issues_and_pull_requests() {
        let sorted = sort_inbox_items(vec![
            with(item(1, "2026-08-27T08:00:00Z"), |row| {
                row.title = "older issue".into()
            }),
            with(item(2, "2026-08-27T10:00:00Z"), |row| {
                row.kind = InboxKind::Pr;
                row.title = "newer pr".into();
            }),
        ]);
        assert_eq!(numbers(&sorted), [2, 1]);
    }

    #[test]
    fn list_cache_key_includes_hidden_linear_team_ids() {
        let projects = vec!["/tmp/web".to_string()];
        let base = InboxQuery::default();
        let teams = InboxQuery {
            linear_hidden_team_ids: Some(vec!["t2".into()]),
            ..InboxQuery::default()
        };
        assert_ne!(
            inbox_list_cache_key(&projects, &base),
            inbox_list_cache_key(&projects, &teams)
        );
    }

    #[test]
    fn collect_keeps_items_from_projects_that_succeeded() {
        let kept = item(4, "2026-08-27T11:00:00Z");
        assert_eq!(
            collect_inbox_results(vec![Ok(vec![kept.clone()]), Err("gh missing".into())], &[]),
            CollectedInboxItems {
                items: vec![kept],
                error: None
            }
        );
    }

    #[test]
    fn collect_reports_an_error_when_every_project_fetch_failed() {
        assert_eq!(
            collect_inbox_results(
                vec![
                    Err("not a github repo".into()),
                    Err("command not found".into())
                ],
                &[]
            ),
            CollectedInboxItems {
                items: vec![],
                error: Some("not a github repo".into())
            }
        );
    }

    #[test]
    fn dedupe_keeps_one_card_per_github_issue_across_local_checkouts() {
        let rows = vec![
            with(item(10, "2026-08-27T10:00:00Z"), |row| {
                row.project_path = "/tmp/agent-terminal".into();
                row.repo = "hardbeat920/monocode".into();
            }),
            with(item(10, "2026-08-27T10:00:00Z"), |row| {
                row.project_path = "/tmp/monocode".into();
                row.repo = "HardBeat920/monocode".into();
            }),
        ];
        let deduped = dedupe_inbox_items(
            rows,
            &["/tmp/monocode".into(), "/tmp/agent-terminal".into()],
        );
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].project_path, "/tmp/monocode");
        assert_eq!(
            inbox_item_key(&deduped[0]),
            "github:hardbeat920/monocode:issue:10"
        );
    }

    #[test]
    fn dedupe_keeps_same_number_items_from_a_fork_and_its_parent_separate() {
        let rows = vec![
            with(item(10, "2026-08-27T10:00:00Z"), |row| {
                row.repo = "contributor/web".into()
            }),
            with(item(10, "2026-08-27T10:00:00Z"), |row| {
                row.repo = "acme/web".into()
            }),
        ];
        assert_eq!(dedupe_inbox_items(rows, &[]).len(), 2);
    }

    #[test]
    fn dedupe_keeps_identical_triples_from_different_providers() {
        let rows: Vec<InboxItem> = [
            InboxProvider::Github,
            InboxProvider::Gitlab,
            InboxProvider::AzureDevops,
        ]
        .into_iter()
        .map(|provider| {
            with(item(9, "2026-08-27T10:00:00Z"), |row| {
                row.provider = provider;
            })
        })
        .collect();
        let deduped = dedupe_inbox_items(rows, &[]);
        assert_eq!(deduped.len(), 3);
        let mut providers: Vec<&str> = deduped
            .iter()
            .map(|entry| super::super::types::provider_str(entry.provider))
            .collect();
        providers.sort();
        assert_eq!(providers, ["azuredevops", "github", "gitlab"]);
    }

    #[test]
    fn group_fetches_each_github_remote_once() {
        let grouped = group_projects_by_repo(&[
            ProjectRepo::new("/tmp/monocode", "hardbeat920/monocode"),
            ProjectRepo::new("/tmp/agent-terminal", "HardBeat920/monocode"),
            ProjectRepo::new("/tmp/docs", "acme/docs"),
        ]);
        let paths: Vec<&str> = grouped
            .iter()
            .map(|project| project.path.as_str())
            .collect();
        assert_eq!(paths, ["/tmp/monocode", "/tmp/docs"]);
    }

    #[test]
    fn group_fetches_a_shared_upstream_once_while_preserving_its_preferred_checkout() {
        assert_eq!(
            group_projects_by_repo(&[
                ProjectRepo::new("/tmp/fork-a", "maya/web"),
                ProjectRepo::new("/tmp/fork-a", "acme/web"),
                ProjectRepo::new("/tmp/fork-b", "lin/web"),
                ProjectRepo::new("/tmp/fork-b", "ACME/web"),
            ]),
            vec![
                ProjectRepo::new("/tmp/fork-a", "maya/web"),
                ProjectRepo::new("/tmp/fork-a", "acme/web"),
                ProjectRepo::new("/tmp/fork-b", "lin/web"),
            ]
        );
    }

    #[test]
    fn repository_cache_keys_separate_same_number_items() {
        assert_ne!(
            details_cache_key("maya/web", WorkItemKind::Issue, 10),
            details_cache_key("acme/web", WorkItemKind::Issue, 10)
        );
        assert_ne!(
            pr_diff_cache_key("maya/web", 10, false),
            pr_diff_cache_key("acme/web", 10, false)
        );
    }

    #[test]
    fn unique_inbox_projects_drops_duplicate_paths() {
        assert_eq!(
            unique_inbox_projects(&["/tmp/web/".into(), "/tmp/web".into(), "/tmp/docs".into()]),
            ["/tmp/web", "/tmp/docs"]
        );
    }

    fn search_rows() -> Vec<InboxItem> {
        vec![
            with(item(12, "2026-08-27T10:00:00Z"), |row| {
                row.kind = InboxKind::Pr;
                row.title = "Fix checkout".into();
                row.repo = "acme/web".into();
                row.labels = vec![GithubLabel {
                    name: "bug".into(),
                    color: "d73a4a".into(),
                }];
            }),
            with(item(4, "2026-08-27T09:00:00Z"), |row| {
                row.title = "Add dark mode".into();
                row.repo = "acme/docs".into();
                row.project_path = "/tmp/docs".into();
            }),
        ]
    }

    #[test]
    fn filter_keeps_every_item_when_the_query_is_empty() {
        assert_eq!(filter_inbox_items(&search_rows(), "  ").len(), 2);
    }

    #[test]
    fn filter_matches_title_number_kind_repo_and_labels() {
        let rows = search_rows();
        assert_eq!(numbers(&filter_inbox_items(&rows, "checkout")), [12]);
        assert_eq!(numbers(&filter_inbox_items(&rows, "#4")), [4]);
        assert_eq!(numbers(&filter_inbox_items(&rows, "pull")), [12]);
        assert_eq!(numbers(&filter_inbox_items(&rows, "docs")), [4]);
        assert_eq!(numbers(&filter_inbox_items(&rows, "bug")), [12]);
    }

    fn linear_item() -> InboxItem {
        with(item(9, "2026-08-27T10:00:00Z"), |row| {
            row.kind = InboxKind::Linear;
            row.provider = InboxProvider::Linear;
            row.identifier = Some("ENG-9".into());
            row.title = "Fix auth".into();
            row.url = "https://linear.app/acme/issue/ENG-9".into();
        })
    }

    #[test]
    fn start_draft_seeds_kind_title_and_url() {
        let issue = with(item(10, "2026-08-27T10:00:00Z"), |row| {
            row.title = "Normalize streamed plan".into();
            row.url = "https://github.com/acme/web/issues/10".into();
        });
        assert_eq!(
            inbox_start_draft(&issue, None),
            "Work on this GitHub issue:\n\n#10 Normalize streamed plan\nhttps://github.com/acme/web/issues/10\n"
        );
        let pr = with(item(12, "2026-08-27T10:00:00Z"), |row| {
            row.kind = InboxKind::Pr;
            row.title = "Fix checkout".into();
            row.url = "https://github.com/acme/web/pull/12".into();
        });
        assert!(inbox_start_draft(&pr, None).contains("Work on this GitHub pull request:"));
    }

    #[test]
    fn start_draft_seeds_linear_issues_with_the_identifier() {
        assert_eq!(
            inbox_start_draft(&linear_item(), None),
            "Work on this Linear issue:\n\nENG-9 Fix auth\nhttps://linear.app/acme/issue/ENG-9\n"
        );
        assert!(
            inbox_start_draft(
                &linear_item(),
                Some("Steps to reproduce the login failure.")
            )
            .contains("Steps to reproduce the login failure.")
        );
    }

    #[test]
    fn start_draft_uses_gitlab_merge_request_wording() {
        let mr = with(item(12, "2026-08-27T10:00:00Z"), |row| {
            row.kind = InboxKind::Pr;
            row.provider = InboxProvider::Gitlab;
            row.title = "Fix checkout".into();
            row.url = "https://gitlab.example.com/acme/web/-/merge_requests/12".into();
        });
        assert_eq!(
            inbox_start_draft(&mr, None),
            "Work on this GitLab merge request:\n\n#12 Fix checkout\nhttps://gitlab.example.com/acme/web/-/merge_requests/12\n"
        );
    }

    #[test]
    fn item_keys() {
        assert_eq!(inbox_item_key(&linear_item()), "linear:eng-9");
        let gitlab = with(item(9, "2026-08-27T10:00:00Z"), |row| {
            row.provider = InboxProvider::Gitlab;
            row.url = "https://gitlab.example.com/acme/web/-/issues/9".into();
        });
        assert_eq!(inbox_item_key(&gitlab), "gitlab:acme/web:issue:9");
        let github = with(gitlab.clone(), |row| row.provider = InboxProvider::Github);
        assert_ne!(inbox_item_key(&gitlab), inbox_item_key(&github));
    }

    #[test]
    fn composer_card_for_linear_github_and_gitlab() {
        let linear = with(linear_item(), |row| {
            row.team_name = Some("Engineering".into())
        });
        let card = inbox_composer_card(&linear, Some("Reset the session cookie."));
        assert_eq!(card.provider, InboxProvider::Linear);
        assert_eq!(card.kind, InboxKind::Linear);
        assert_eq!(card.identifier, "ENG-9");
        assert_eq!(card.title, "Fix auth");
        assert_eq!(card.source, "Engineering");
        assert_eq!(card.url, "https://linear.app/acme/issue/ENG-9");
        assert!(card.prompt.contains("Work on this Linear issue:"));
        assert!(card.prompt.contains("Reset the session cookie."));

        let issue = with(item(10, "2026-08-27T10:00:00Z"), |row| {
            row.title = "Normalize streamed plan".into();
            row.url = "https://github.com/acme/web/issues/10".into();
        });
        let card = inbox_composer_card(&issue, None);
        assert_eq!(
            (card.provider, card.kind),
            (InboxProvider::Github, InboxKind::Issue)
        );
        assert_eq!(card.identifier, "#10");
        assert_eq!(card.title, "Normalize streamed plan");
        assert_eq!(card.source, "acme/web");

        let gitlab = with(issue.clone(), |row| {
            row.provider = InboxProvider::Gitlab;
            row.url = "https://gitlab.example.com/acme/web/-/issues/10".into();
        });
        let card = inbox_composer_card(&gitlab, None);
        assert_eq!(card.provider, InboxProvider::Gitlab);
        assert_eq!(card.identifier, "#10");
        assert_eq!(card.source, "acme/web");
    }

    #[test]
    fn compose_message_sends_the_prompt_and_appends_a_note() {
        let issue = with(item(10, "2026-08-27T10:00:00Z"), |row| {
            row.title = "Normalize streamed plan".into();
            row.url = "https://github.com/acme/web/issues/10".into();
        });
        let card = inbox_composer_card(&issue, None);
        assert_eq!(
            compose_inbox_message(Some(&card), "  "),
            "Work on this GitHub issue:\n\n#10 Normalize streamed plan\nhttps://github.com/acme/web/issues/10"
        );
        assert!(
            compose_inbox_message(Some(&card), "Start with the parser.")
                .contains("\n\nStart with the parser.")
        );
        assert_eq!(compose_inbox_message(None, " hello "), "hello");
    }
}
