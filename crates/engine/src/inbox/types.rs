//! Port of the inbox data types from src/features/inbox/model/githubTasks.ts,
//! gitlab.ts, azureDevOps.ts, jira.ts, linear.ts, and githubPrChecks.ts.
//!
//! The shapes match what the Tauri commands returned to TypeScript, so the
//! live backend converts each command's result through its JSON form.

use serde::{Deserialize, Serialize};

pub use monocode_core::inbox::{GithubLabel, InboxKind, InboxProvider, WorkItemKind};

/// `InboxProvider` as its string id.
pub fn provider_str(provider: InboxProvider) -> &'static str {
    match provider {
        InboxProvider::Github => "github",
        InboxProvider::Linear => "linear",
        InboxProvider::Jira => "jira",
        InboxProvider::Gitlab => "gitlab",
        InboxProvider::AzureDevops => "azuredevops",
    }
}

/// `InboxKind` as its string id.
pub fn kind_str(kind: InboxKind) -> &'static str {
    match kind {
        InboxKind::Issue => "issue",
        InboxKind::Pr => "pr",
        InboxKind::Linear => "linear",
        InboxKind::Jira => "jira",
    }
}

/// `GithubTaskKind` as its string id.
pub fn work_kind_str(kind: WorkItemKind) -> &'static str {
    match kind {
        WorkItemKind::Issue => "issue",
        WorkItemKind::Pr => "pr",
    }
}

/// A GitHub or repository kind as an `InboxKind`.
pub fn inbox_kind(kind: WorkItemKind) -> InboxKind {
    match kind {
        WorkItemKind::Issue => InboxKind::Issue,
        WorkItemKind::Pr => InboxKind::Pr,
    }
}

/// `item.kind === "issue" || item.kind === "pr"`.
pub fn work_kind(kind: InboxKind) -> Option<WorkItemKind> {
    match kind {
        InboxKind::Issue => Some(WorkItemKind::Issue),
        InboxKind::Pr => Some(WorkItemKind::Pr),
        InboxKind::Linear | InboxKind::Jira => None,
    }
}

/// Every provider in the order the TypeScript listed `InboxProvider`.
pub const INBOX_PROVIDERS: [InboxProvider; 5] = [
    InboxProvider::Github,
    InboxProvider::Linear,
    InboxProvider::Jira,
    InboxProvider::Gitlab,
    InboxProvider::AzureDevops,
];

/// `GithubPrAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GithubPrAction {
    Merge,
    Squash,
    Rebase,
    Draft,
    Ready,
    Close,
    Reopen,
}

impl GithubPrAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Close => "close",
            Self::Reopen => "reopen",
        }
    }
}

/// `GithubAssignee`, also the person shape of every tracker.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAssignee {
    pub login: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// `GithubWorkItem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubWorkItem {
    pub kind: WorkItemKind,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    /// GitHub issue closure reason, such as `completed` or `not_planned`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<GithubLabel>,
    #[serde(default)]
    pub assignees: Vec<GithubAssignee>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub repo: String,
}

/// `GitlabWorkItem` and `AzureDevOpsWorkItem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryWorkItem {
    pub kind: WorkItemKind,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<GithubLabel>,
    #[serde(default)]
    pub assignees: Vec<GithubAssignee>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub attention_reason: String,
}

/// `LinearIssue` and `JiraIssue`. Jira issues have no Linear project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerIssue {
    pub id: String,
    pub identifier: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    /// Linear's workflow state type, or Jira's status category (`new`,
    /// `indeterminate`, or `done`).
    pub state_type: String,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<GithubLabel>,
    #[serde(default)]
    pub assignees: Vec<GithubAssignee>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub team_id: String,
    #[serde(default)]
    pub team_name: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub project_path: String,
}

/// `InboxItem`: one card from any provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub kind: InboxKind,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<GithubLabel>,
    #[serde(default)]
    pub assignees: Vec<GithubAssignee>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub project_path: String,
    pub provider: InboxProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    /// Linear project the issue belongs to. Empty when it sits outside every
    /// project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_type: Option<String>,
    /// GitLab To-Do action that caused this item to need attention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

impl InboxItem {
    /// `{ ...item, projectPath, provider }` for a GitHub work item.
    pub fn from_github(item: &GithubWorkItem, project_path: &str, provider: InboxProvider) -> Self {
        Self {
            kind: inbox_kind(item.kind),
            number: item.number,
            title: item.title.clone(),
            url: item.url.clone(),
            state: item.state.clone(),
            state_reason: item.state_reason.clone(),
            created_at: item.created_at.clone(),
            updated_at: item.updated_at.clone(),
            labels: item.labels.clone(),
            assignees: item.assignees.clone(),
            draft: item.draft,
            repo: item.repo.clone(),
            project_path: project_path.to_string(),
            provider,
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

    /// `{ ...cached, ...item }`: a fresh GitHub snapshot over a cached card.
    pub fn merge_github(&self, item: &GithubWorkItem) -> Self {
        let mut next = self.clone();
        next.kind = inbox_kind(item.kind);
        next.number = item.number;
        next.title = item.title.clone();
        next.url = item.url.clone();
        next.state = item.state.clone();
        if item.state_reason.is_some() {
            next.state_reason = item.state_reason.clone();
        }
        if item.created_at.is_some() {
            next.created_at = item.created_at.clone();
        }
        next.updated_at = item.updated_at.clone();
        next.labels = item.labels.clone();
        next.assignees = item.assignees.clone();
        next.draft = item.draft;
        next.repo = item.repo.clone();
        next
    }

    /// The `GithubWorkItem` fields of a GitHub card, `{ ...item, kind }`.
    pub fn to_github(&self) -> Option<GithubWorkItem> {
        Some(GithubWorkItem {
            kind: work_kind(self.kind)?,
            number: self.number,
            title: self.title.clone(),
            url: self.url.clone(),
            state: self.state.clone(),
            state_reason: self.state_reason.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            labels: self.labels.clone(),
            assignees: self.assignees.clone(),
            draft: self.draft,
            repo: self.repo.clone(),
        })
    }
}

/// `GithubWorkItemDetails`, also the GitLab, Azure DevOps, Jira, and Linear
/// details. Trackers leave the ref and review fields unset.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemDetails {
    pub body: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<String>,
}

/// `GithubWorkItemComment`, the comment shape every provider returns.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemComment {
    pub id: String,
    pub kind: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_avatar_url: Option<String>,
    pub body: String,
    pub created_at: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub replies: Vec<WorkItemComment>,
}

/// `GithubWorkItemCommit`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemCommit {
    pub oid: String,
    pub message_headline: String,
    pub author: String,
    pub committed_date: String,
    pub url: String,
}

/// `GithubWorkItemThread`, also the tracker threads, which carry no commits.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemThread {
    #[serde(default)]
    pub comments: Vec<WorkItemComment>,
    #[serde(default)]
    pub commits: Vec<WorkItemCommit>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub review_decision: String,
    #[serde(default)]
    pub base_ref_name: String,
    #[serde(default)]
    pub head_ref_name: String,
}

/// `GithubPrFile`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrFile {
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
}

/// `GithubPrDiff`, `GitlabMrDiff`, and `AzureDevOpsMrDiff`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrDiff {
    pub additions: i64,
    pub deletions: i64,
    #[serde(default)]
    pub files: Vec<PrFile>,
    pub patch: String,
    pub truncated: bool,
}

/// `"open" | "all"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InboxState {
    #[default]
    Open,
    All,
}

impl InboxState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::All => "all",
        }
    }
}

/// `GithubWorkItemQuery`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubWorkItemQuery {
    pub kind: WorkItemKind,
    pub assigned_to_me: bool,
    pub state: InboxState,
    pub search: String,
}

/// `InboxQuery`. `None` team or project lists fall back to the stored ones.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InboxQuery {
    pub assigned_to_me: bool,
    pub state: InboxState,
    pub search: String,
    pub linear_hidden_team_ids: Option<Vec<String>>,
    pub jira_hidden_project_ids: Option<Vec<String>>,
}

/// `InboxProviderErrors`: `Partial<Record<InboxProvider, string>>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InboxProviderErrors {
    entries: Vec<(InboxProvider, String)>,
}

impl InboxProviderErrors {
    pub fn new() -> Self {
        Self::default()
    }

    /// `errors[provider] = message`. The first assignment fixes the key order.
    pub fn set(&mut self, provider: InboxProvider, message: impl Into<String>) {
        let message = message.into();
        match self.entries.iter_mut().find(|(key, _)| *key == provider) {
            Some(entry) => entry.1 = message,
            None => self.entries.push((provider, message)),
        }
    }

    pub fn get(&self, provider: InboxProvider) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| *key == provider)
            .map(|(_, message)| message.as_str())
    }

    /// `Object.keys(errors)`.
    pub fn providers(&self) -> Vec<InboxProvider> {
        self.entries.iter().map(|(provider, _)| *provider).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (InboxProvider, &str)> {
        self.entries
            .iter()
            .map(|(provider, message)| (*provider, message.as_str()))
    }

    /// The same message for every provider.
    pub fn all(message: &str) -> Self {
        let mut errors = Self::new();
        for provider in INBOX_PROVIDERS {
            errors.set(provider, message);
        }
        errors
    }
}

/// `InboxListResult`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InboxListResult {
    pub items: Vec<InboxItem>,
    pub errors: InboxProviderErrors,
}

/// `GithubStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatus {
    pub connected: bool,
    pub installed: bool,
    pub authenticated: bool,
}

/// `GithubStarStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GithubStarStatus {
    Starred,
    NotStarred,
    Unavailable,
}

/// `GitlabStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitlabStatus {
    pub connected: bool,
    #[serde(default)]
    pub url: String,
}

/// `AzureDevOpsStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsStatus {
    pub connected: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub organization: String,
}

/// `JiraStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraStatus {
    pub connected: bool,
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub email: String,
}

/// `LinearStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearStatus {
    pub connected: bool,
}

/// `LinearTeam` and `JiraProject`: an id, a short key, and a name.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerGroup {
    pub id: String,
    pub key: String,
    pub name: String,
}

pub type LinearTeam = TrackerGroup;
pub type JiraProject = TrackerGroup;

/// `GithubPrCheckState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GithubPrCheckState {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
    Unknown,
}

/// `GithubPrCheck`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrCheck {
    pub name: String,
    pub workflow: String,
    pub state: GithubPrCheckState,
    pub url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// `GithubPrChecks`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrChecks {
    pub head_oid: String,
    pub checks: Vec<GithubPrCheck>,
}

/// One step of `GithubCheckDetails`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCheckStep {
    pub name: String,
    pub state: GithubPrCheckState,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// One annotation of `GithubCheckDetails`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCheckAnnotation {
    pub path: String,
    pub line: i64,
    pub message: String,
    pub level: String,
}

/// `GithubCheckDetails`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCheckDetails {
    pub steps: Vec<GithubCheckStep>,
    pub annotations: Vec<GithubCheckAnnotation>,
    pub notice: Option<String>,
}

/// `GitPr` from the session working copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPr {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
}
