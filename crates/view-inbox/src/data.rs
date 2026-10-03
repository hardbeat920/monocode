//! The inbox data the views read and the actions they ask for.
//!
//! The React views imported the inbox model (`githubTasks.ts`,
//! `inboxFilters.ts`, `useGithubPrChecks.ts`, `ciRepairTracking.ts`) and the
//! Tauri wrappers directly. Here they reach them through four traits:
//!
//! - [`InboxServices`]: one per app. Opens the per-page, per-item, and per-PR
//!   data, and carries the app-wide reads and actions (seen state, CI repair
//!   history, job details, sending an item to an agent, links, clipboard).
//! - [`InboxListData`]: the Inbox page list. The engine's `InboxList` entity.
//! - [`InboxDetailData`]: one item's description, thread, diff, comments,
//!   and pull request actions. The engine's `InboxItemDetail` entity.
//! - [`PrChecksData`]: one pull request's checks. The engine's `PrChecks`
//!   entity.
//!
//! The record types mirror `monocode_engine::inbox::types` field for field,
//! with the same serde shapes, so the app's adapter converts with a JSON
//! round trip or a field copy. This crate does not depend on the engine.

use std::rc::Rc;

use gpui::{AnyElement, AnyView, App, Hsla, Subscription, Task, Window};
use serde::{Deserialize, Serialize};

pub use monocode_core::inbox::{
    GithubLabel, InboxComposerCard, InboxKind, InboxProvider, LinkedWorkItemActivityCounts,
    LinkedWorkItemActivityEntry, LinkedWorkItemActivityKind, LinkedWorkItemUpdateCard,
    LinkedWorkItemUpdateStatus, WorkItemKind,
};
pub use monocode_core::session::LinkedWorkItem;

/// A data call that finishes later. Errors are the messages the UI shows.
pub type DataTask<T> = Task<Result<T, String>>;

/// A change listener. Implementations call it after their own update, so it
/// may read the data again.
pub type Listener = Box<dyn Fn(&mut App)>;

/// A callback a view runs on a click.
pub type Action = Rc<dyn Fn(&mut Window, &mut App)>;

/// A callback that receives a value.
pub type ValueAction<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

/// `InboxSource`: the provider tabs.
pub type InboxSource = InboxProvider;

/// `GithubAssignee`, also the person shape of every tracker.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAssignee {
    pub login: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
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
    /// A GitHub issue with the required fields, for tests and the gallery.
    pub fn github(kind: InboxKind, repo: &str, number: i64, title: &str) -> Self {
        let path = if kind == InboxKind::Pr {
            "pull"
        } else {
            "issues"
        };
        Self {
            kind,
            number,
            title: title.into(),
            url: format!("https://github.com/{repo}/{path}/{number}"),
            state: "open".into(),
            state_reason: None,
            created_at: None,
            updated_at: String::new(),
            labels: Vec::new(),
            assignees: Vec::new(),
            draft: false,
            repo: repo.into(),
            project_path: String::new(),
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

    /// `item.provider === "linear" || item.provider === "jira"`.
    pub fn is_tracker(&self) -> bool {
        matches!(self.provider, InboxProvider::Linear | InboxProvider::Jira)
    }
}

/// `GithubWorkItemDetails`, also the tracker details.
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

/// `GithubWorkItemThread`, also the tracker threads.
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
    /// Merge, squash, or rebase.
    pub fn is_merge(self) -> bool {
        matches!(self, Self::Merge | Self::Squash | Self::Rebase)
    }
}

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

/// `GithubPrChecks`: a head commit and its checks, always together.
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

/// `LinearTeam` and `JiraProject`: an id, a short key, and a name.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerGroup {
    pub id: String,
    pub key: String,
    pub name: String,
}

/// `LinearProjectOption`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearProjectOption {
    pub id: String,
    pub name: String,
}

/// `SessionTimeFilter`, shared with the session list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum InboxTimeFilter {
    #[default]
    #[serde(rename = "all")]
    All,
    #[serde(rename = "today")]
    Today,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
}

/// `InboxStatusFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InboxStatusFilter {
    pub open: bool,
    pub draft: bool,
    pub closed: bool,
    pub merged: bool,
}

/// `InboxFilters`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxFilters {
    pub assigned_to_me: bool,
    pub hidden_projects: Vec<String>,
    /// Linear project ids to hide.
    pub hidden_linear_projects: Vec<String>,
    pub hidden_kinds: Vec<InboxKind>,
    pub time: InboxTimeFilter,
    pub status: InboxStatusFilter,
}

/// `InboxSourceConnections`: `None` means the status check has not
/// resolved yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InboxSourceConnections {
    pub github: Option<bool>,
    pub linear: Option<bool>,
    pub jira: Option<bool>,
    pub gitlab: Option<bool>,
    pub azuredevops: Option<bool>,
}

/// One selected check in a CI repair target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiRepairCheck {
    pub name: String,
    pub workflow: String,
    pub url: Option<String>,
}

/// `TrackedCiRepair["phase"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CiRepairPhase {
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

/// `TrackedCiRepair`: a repair target plus its chat and phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedCiRepair {
    pub repo: String,
    pub number: i64,
    pub head_oid: String,
    pub checks: Vec<CiRepairCheck>,
    pub id: String,
    pub cwd: String,
    pub session_id: String,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    pub phase: CiRepairPhase,
}

/// The `details` of a `CiRepairEvidence`: the job details, or only a notice
/// when they could not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiEvidenceDetails {
    Full(GithubCheckDetails),
    Notice(String),
}

/// `CiRepairEvidence`: a failed check and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiRepairEvidence {
    pub check: GithubPrCheck,
    pub details: Option<CiEvidenceDetails>,
}

/// What `CheckRepairForm` hands to `buildCiRepairRequest`: the PR, the
/// commit, and the evidence for each selected check, in selection order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiRepairStart {
    pub repo: String,
    pub number: i64,
    pub head_oid: String,
    pub evidence: Vec<CiRepairEvidence>,
}

/// `InboxReplyTarget`: the comment a reply answers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InboxReplyTarget {
    /// Linear replies go under this comment id.
    pub id: String,
    pub author: String,
    /// GitHub replies go into this review thread.
    pub thread_id: String,
}

/// `InboxMediaKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxMediaKind {
    Image,
    Video,
}

/// Fetched inbox media and its sniffed type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxMedia {
    pub kind: InboxMediaKind,
    pub mime: String,
    pub bytes: std::sync::Arc<Vec<u8>>,
}

/// A session that names an inbox item (`relatedSessionsForInboxItem`), or a
/// project chat a CI repair can continue.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelatedSession {
    pub id: String,
    /// `sessionDisplayTitle(title, harness)`.
    pub title: String,
    pub archived: bool,
}

/// How a project shows beside its name: a logo image or a pixel mascot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectMark {
    pub logo_path: Option<String>,
    pub mascot_name: Option<String>,
    pub mascot_color: Option<Hsla>,
}

/// `InboxProjectOption`: a rail project for the filter menu and the project
/// picker.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InboxProjectOption {
    pub path: String,
    pub name: String,
    pub mark: ProjectMark,
}

/// One listed card with what the list needs beside the item.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedItem {
    /// `inboxItemKey(item)`.
    pub key: String,
    pub item: InboxItem,
    /// `isInboxEntryUnseen`.
    pub unseen: bool,
    /// `relatedSessionsForInboxItem(item, sessions)`.
    pub related_sessions: Vec<RelatedSession>,
    /// The mark of the card's project.
    pub project_mark: ProjectMark,
}

/// One loadable value: what is on screen, whether a load runs, and its
/// error.
#[derive(Debug, Clone, PartialEq)]
pub struct Loadable<T> {
    pub value: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T> Default for Loadable<T> {
    fn default() -> Self {
        Self {
            value: None,
            loading: false,
            error: None,
        }
    }
}

impl<T> Loadable<T> {
    pub fn ready(value: T) -> Self {
        Self {
            value: Some(value),
            loading: false,
            error: None,
        }
    }

    pub fn loading() -> Self {
        Self {
            value: None,
            loading: true,
            error: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            value: None,
            loading: false,
            error: Some(error.into()),
        }
    }
}

/// What the Inbox page shows besides the cards.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxListState {
    /// The current working folder, for the project picker's default.
    pub cwd: String,
    /// The rail's projects, sorted by name.
    pub projects: Vec<InboxProjectOption>,
    /// How many items were fetched, before filters.
    pub item_count: usize,
    /// The first load is running and nothing is cached.
    pub loading: bool,
    /// A cached list is on screen while a fresh one loads.
    pub revalidating: bool,
    pub source: InboxSource,
    pub visible_sources: Vec<InboxSource>,
    pub connectable_sources: Vec<InboxSource>,
    /// The open tab's provider error.
    pub source_error: Option<String>,
    pub read_status_error: Option<String>,
    /// The filters with projects that left the rail dropped.
    pub filters: InboxFilters,
    /// `hasActiveInboxFilters` for the open tab.
    pub filters_active: bool,
    /// Whether the open tab has an unread card.
    pub source_has_unseen: bool,
    pub linear_projects: Vec<LinearProjectOption>,
    pub linear_teams: Vec<TrackerGroup>,
    pub hidden_linear_team_ids: Vec<String>,
    pub jira_projects: Vec<TrackerGroup>,
    pub hidden_jira_project_ids: Vec<String>,
    /// The selection key to wait for while a linked target loads.
    pub target_selection_key: Option<String>,
}

impl Default for InboxListState {
    fn default() -> Self {
        Self {
            cwd: String::new(),
            projects: Vec::new(),
            item_count: 0,
            loading: false,
            revalidating: false,
            source: InboxProvider::Github,
            visible_sources: Vec::new(),
            connectable_sources: Vec::new(),
            source_error: None,
            read_status_error: None,
            filters: InboxFilters::default(),
            filters_active: false,
            source_has_unseen: false,
            linear_projects: Vec::new(),
            linear_teams: Vec::new(),
            hidden_linear_team_ids: Vec::new(),
            jira_projects: Vec::new(),
            hidden_jira_project_ids: Vec::new(),
            target_selection_key: None,
        }
    }
}

/// The Inbox page list. The engine's `InboxList` implements it.
pub trait InboxListData {
    /// Calls `listener` after every change.
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;
    fn state(&self, cx: &App) -> InboxListState;
    /// The open tab's cards after filters and `search`, with a linked target
    /// pinned first on the GitHub tab.
    fn visible_items(&self, search: &str, cx: &App) -> Vec<ListedItem>;
    fn set_source(&self, source: InboxSource, cx: &mut App);
    fn set_filters(&self, filters: InboxFilters, cx: &mut App);
    fn set_hidden_linear_team_ids(&self, ids: Vec<String>, cx: &mut App);
    fn set_hidden_jira_project_ids(&self, ids: Vec<String>, cx: &mut App);
    /// The Refresh button: fetch again past the freshness window.
    fn refresh(&self, cx: &mut App);
    /// "Mark all as read" for the open tab.
    fn mark_source_read(&self, cx: &mut App);
    /// A card was opened.
    fn mark_item_seen(&self, item: &ListedItem, cx: &mut App);
    /// A card changed in place (a PR action).
    fn update_item(&self, item: InboxItem, cx: &mut App);
}

/// What an item's detail pane shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InboxDetailState {
    pub details: Loadable<WorkItemDetails>,
    pub thread: Loadable<WorkItemThread>,
    pub diff: Loadable<PrDiff>,
    pub reply_to: Option<InboxReplyTarget>,
    pub posting: bool,
    pub post_error: Option<String>,
    pub action_busy: bool,
    pub action_error: Option<String>,
    /// "Merge queued or auto-merge enabled." after a merge that did not land.
    pub action_notice: Option<String>,
}

/// One item's detail data. The engine's `InboxItemDetail` implements it.
pub trait InboxDetailData {
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;
    fn state(&self, cx: &App) -> InboxDetailState;
    /// The same item with fresh fields, or another one.
    fn set_item(&self, item: InboxItem, cx: &mut App);
    /// The pane's `revision`: reload the description, thread, and open diff.
    fn set_revision(&self, revision: u64, cx: &mut App);
    /// The Code tab is showing: load the diff with full files or hunks.
    fn show_diff(&self, full_file: bool, cx: &mut App);
    fn set_reply_to(&self, reply_to: Option<InboxReplyTarget>, cx: &mut App);
    /// `postComment`.
    fn post_comment(&self, body: String, cx: &mut App) -> DataTask<()>;
    /// `githubPrAction`: resolves with the updated card for the list.
    fn run_pr_action(&self, action: GithubPrAction, cx: &mut App) -> DataTask<InboxItem>;
}

/// `useGithubPrChecks` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrChecksParams {
    pub cwd: String,
    pub repo: String,
    pub number: i64,
    pub enabled: bool,
    /// Open PRs poll; closed or merged ones load once and on demand.
    pub open: bool,
    /// The panel is showing.
    pub poll: bool,
    pub revision: u64,
}

/// `GithubPrChecksView` without its `refresh`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrChecksState {
    pub checks: Option<GithubPrChecks>,
    /// The first load: no results yet.
    pub loading: bool,
    /// Revalidating results already on screen.
    pub refreshing: bool,
    pub error: Option<String>,
    /// Earlier results stayed on screen after a failed refresh.
    pub stale: bool,
    /// Bumps on every settled load, so expanded rows reload their job
    /// details even when the answer did not change (React keyed that on the
    /// checks object's identity).
    pub generation: u64,
}

/// One pull request's checks. The engine's `PrChecks` implements it.
pub trait PrChecksData {
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;
    fn state(&self, cx: &App) -> PrChecksState;
    fn set_params(&self, params: PrChecksParams, cx: &mut App);
    fn refresh(&self, cx: &mut App);
}

/// App-wide inbox reads and actions.
pub trait InboxServices {
    /// `Date.now()`, so tests and the gallery can pin relative times.
    fn now_ms(&self) -> i64;

    /// Data for one item's detail pane.
    fn open_detail(&self, item: &InboxItem, cx: &mut App) -> Rc<dyn InboxDetailData>;

    /// Data for one pull request's checks.
    fn open_pr_checks(&self, params: PrChecksParams, cx: &mut App) -> Rc<dyn PrChecksData>;

    /// The cached GitHub card for a linked work item, if any
    /// (`peekGithubWorkItem`, with the project path and provider filled in).
    fn peek_github_work_item(
        &self,
        cwd: &str,
        target: &LinkedWorkItem,
        cx: &App,
    ) -> Option<InboxItem>;

    /// `githubWorkItem`, as a card for `cwd`.
    fn github_work_item(
        &self,
        cwd: &str,
        target: &LinkedWorkItem,
        cx: &mut App,
    ) -> DataTask<InboxItem>;

    /// `fetchGithubCheckDetails`.
    fn fetch_check_details(
        &self,
        cwd: &str,
        repo: &str,
        job_id: &str,
        cx: &mut App,
    ) -> DataTask<GithubCheckDetails>;

    /// `gitCommitFileDiff(...).current` for a text file at `sha`: `None` for
    /// binary, too large, or missing files.
    fn commit_file_text(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
        cx: &mut App,
    ) -> DataTask<Option<String>>;

    /// Every tracked CI repair, newest first (`getCiRepairs`).
    fn ci_repairs(&self, cx: &App) -> Vec<TrackedCiRepair>;

    /// Calls `listener` when the CI repair history changes.
    fn subscribe_ci_repairs(&self, listener: Listener, cx: &mut App) -> Subscription;

    /// `onStart`: send an issue to an agent. `body` is the tracker
    /// description.
    fn start_item(&self, item: InboxItem, body: Option<String>, cx: &mut App) -> DataTask<()>;

    /// `onRepairChecks`: build the request and start the repair, in a new
    /// project chat or in `session_id`.
    fn repair_checks(
        &self,
        item: &InboxItem,
        start: CiRepairStart,
        session_id: Option<String>,
        cx: &mut App,
    ) -> DataTask<()>;

    /// The project chats a CI repair can continue for this item's project.
    fn repair_sessions(&self, item: &InboxItem, cx: &App) -> Vec<RelatedSession>;

    /// `onAsk`: open or reuse the Ask conversation for an item. Resolves
    /// with its session id.
    fn ask(&self, item: &InboxItem, cx: &mut App) -> DataTask<String>;

    /// `onAskRestart`.
    fn ask_restart(&self, item: &InboxItem, cx: &mut App) -> DataTask<String>;

    /// The session pane to show in the Ask panel (the React portal).
    fn ask_pane(&self, session_id: &str, window: &mut Window, cx: &mut App) -> Option<AnyView>;

    /// The Ask panel closed or moved to another item.
    fn ask_unmounted(&self, cx: &mut App);

    fn open_url(&self, url: &str, cx: &mut App);

    /// `fetchInboxMedia` plus `sniffInboxMedia`: the bytes of a remote image
    /// or video in an issue body, read with the provider's credentials.
    fn fetch_media(&self, _url: &str, _cx: &mut App) -> DataTask<InboxMedia> {
        Task::ready(Err("unsupported".into()))
    }

    /// `copyText`. Returns whether the copy worked.
    fn copy_text(&self, text: &str, cx: &mut App) -> bool;

    /// `playCue("copy")`.
    fn play_copy_cue(&self, _cx: &mut App) {}

    /// A project's mark: its logo or its pixel mascot. The default is a
    /// small dot in the mascot color.
    fn project_mark(&self, mark: &ProjectMark, size: f32, cx: &App) -> AnyElement {
        crate::style::default_project_mark(mark, size, cx)
    }
}
