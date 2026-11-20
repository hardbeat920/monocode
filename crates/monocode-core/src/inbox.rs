//! Native Inbox services. Provider adapters preserve the Tauri API's requests,
//! validation, response parsers and credential file formats.
mod avatar;
mod media;
pub use media::png as media_png;
pub mod azure_devops;
pub use avatar::png as avatar_png;
mod github;
pub mod gitlab;
pub mod jira;
pub mod linear;
pub mod sonar;
mod sonar_native;

use crate::{desktop_preferences::DesktopPreferences, session_groups::WorkItem, store::Store};
use chrono::{Datelike, Local, TimeZone};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

pub const PROVIDERS: [&str; 6] = ["github", "linear", "jira", "gitlab", "azuredevops", "sonar"];
pub fn provider_label(provider: &str) -> &'static str {
    match provider {
        "sonar" => "Sonar",
        "linear" => "Linear",
        "jira" => "Jira",
        "gitlab" => "GitLab",
        "azuredevops" => "ADO",
        _ => "GitHub",
    }
}
pub fn is_tracker(provider: &str) -> bool {
    matches!(provider, "linear" | "jira" | "sonar")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Label {
    pub name: String,
    pub color: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Person {
    pub login: String,
    pub avatar_url: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Item {
    pub provider: String,
    pub kind: String,
    pub id: String,
    pub identifier: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub state_type: String,
    pub state_reason: String,
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<Label>,
    pub assignees: Vec<Person>,
    pub draft: bool,
    pub repo: String,
    pub team_id: String,
    pub team_name: String,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub attention_reason: String,
    pub sonar_project: String,
    pub sonar_branch: String,
}
impl Item {
    pub fn key(&self) -> String {
        let identity = if self.provider == "sonar" {
            serde_json::to_string(&(&self.sonar_project, &self.sonar_branch, &self.id))
                .unwrap_or_default()
        } else if self.provider == "linear" {
            let id = if self.identifier.trim().is_empty() {
                self.id.trim()
            } else {
                self.identifier.trim()
            };
            if id.is_empty() {
                format!("linear:{}", self.number)
            } else {
                id.to_lowercase()
            }
        } else if self.provider == "jira" {
            let id = if self.id.trim().is_empty() {
                self.identifier.trim()
            } else {
                self.id.trim()
            };
            if id.is_empty() {
                format!("jira:{}", self.number)
            } else {
                id.to_lowercase()
            }
        } else if !self.repo.trim().is_empty() {
            format!(
                "{}:{}:{}",
                self.repo.trim().to_lowercase(),
                self.kind,
                self.number
            )
        } else if !self.url.trim().is_empty() {
            self.url.trim().to_lowercase()
        } else {
            format!("{}:{}", self.kind, self.number)
        };
        format!("{}:{identity}", self.provider)
    }
    pub fn reference(&self) -> String {
        if self.identifier.is_empty() {
            format!("#{}", self.number)
        } else {
            self.identifier.clone()
        }
    }
    pub fn updated_ms(&self) -> i64 {
        timestamp_ms(&self.updated_at)
    }
    pub fn status(&self) -> &'static str {
        if self.state.eq_ignore_ascii_case("merged") {
            "Merged"
        } else if (self.provider == "sonar"
            && matches!(
                self.state.to_lowercase().as_str(),
                "accepted" | "falsepositive" | "false_positive" | "wontfix" | "fixed"
            ))
            || matches!(
                self.state.to_lowercase().as_str(),
                "closed" | "done" | "resolved" | "removed" | "completed"
            )
            || matches!(self.state_type.as_str(), "completed" | "canceled" | "done")
        {
            "Closed"
        } else if self.draft {
            "Draft"
        } else {
            "Open"
        }
    }
    pub fn work_item(&self) -> WorkItem {
        WorkItem {
            kind: self.kind.clone(),
            repo: self.repo.clone(),
            number: self.number.max(0) as u64,
            url: self.url.clone(),
        }
    }
    pub fn prompt(&self, body: &str, ask: bool) -> String {
        format!(
            "{}\n\n{} {}: {}\n{}\n\n{}",
            if ask {
                "Help me understand and discuss this work item."
            } else {
                "Work on this item. Read its description and discussion, inspect the project, and implement the required changes."
            },
            provider_label(&self.provider),
            self.reference(),
            self.title,
            self.url,
            body
        )
    }
}
pub fn timestamp_ms(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|at| at.timestamp_millis())
        .unwrap_or(0)
}
pub fn relative_time(value: &str) -> String {
    let at = timestamp_ms(value);
    if at == 0 {
        return String::new();
    }
    let seconds = ((crate::time::now_ms() as i64 - at).max(0)) / 1000;
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        86400..604800 => format!("{}d", seconds / 86400),
        _ => chrono::DateTime::from_timestamp_millis(at)
            .map(|d| d.format("%b %-d").to_string())
            .unwrap_or_default(),
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Details {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
    pub review_decision: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub kind: String,
    pub author: String,
    pub author_avatar_url: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
    pub state: String,
    pub path: String,
    pub line: Option<i64>,
    pub resolved: bool,
    pub thread_id: String,
    pub replies: Vec<Comment>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Commit {
    pub oid: String,
    pub message_headline: String,
    pub author: String,
    pub committed_date: String,
    pub url: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thread {
    pub comments: Vec<Comment>,
    pub commits: Vec<Commit>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Diff {
    pub additions: i64,
    pub deletions: i64,
    pub files: Vec<DiffFile>,
    pub patch: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub workflow: String,
    pub state: String,
    pub url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Checks {
    pub head_oid: String,
    pub checks: Vec<Check>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Status {
    pub connected: bool,
    pub installed: bool,
    pub authenticated: bool,
    pub url: String,
    pub site: String,
    pub email: String,
    pub organization: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OptionRow {
    pub id: String,
    pub name: String,
    pub key: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StatusFilter {
    pub open: bool,
    pub draft: bool,
    pub closed: bool,
    pub merged: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Filters {
    pub assigned_to_me: bool,
    pub hidden_projects: Vec<String>,
    pub hidden_linear_projects: Vec<String>,
    pub hidden_sonar_branches: Vec<String>,
    pub hidden_kinds: Vec<String>,
    pub time: String,
    pub status: StatusFilter,
}
impl Default for Filters {
    fn default() -> Self {
        Self {
            assigned_to_me: false,
            hidden_projects: vec![],
            hidden_linear_projects: vec![],
            hidden_sonar_branches: vec![],
            hidden_kinds: vec![],
            time: "all".into(),
            status: StatusFilter::default(),
        }
    }
}
impl Filters {
    pub fn fetch_state(&self) -> &str {
        if self.status.closed || self.status.merged || !(self.status.open || self.status.draft) {
            "all"
        } else {
            "open"
        }
    }
    pub fn matches(&self, item: &Item, source: &str, search: &str) -> bool {
        if item.provider != source {
            return false;
        }
        let tracker = is_tracker(source);
        if source == "sonar"
            && self
                .hidden_sonar_branches
                .contains(&sonar::branch_key(&item.sonar_project, &item.sonar_branch))
        {
            return false;
        }
        if !tracker
            && !((source == "gitlab" || source == "azuredevops") && self.assigned_to_me)
            && self.hidden_projects.contains(&item.project_path)
        {
            return false;
        }
        if !tracker && self.hidden_kinds.contains(&item.kind) {
            return false;
        }
        let linear_project = if item.project_id.is_empty() {
            "~none"
        } else {
            &item.project_id
        };
        if source == "linear"
            && self
                .hidden_linear_projects
                .iter()
                .any(|id| id == linear_project)
        {
            return false;
        }
        let s = &self.status;
        if (s.open || s.closed || (!tracker && (s.draft || s.merged)))
            && !match item.status() {
                "Open" => s.open,
                "Draft" => s.draft && !tracker,
                "Closed" => s.closed,
                "Merged" => s.merged && !tracker,
                _ => true,
            }
        {
            return false;
        }
        let now = Local::now();
        let start = match self.time.as_str() {
            "today" => Local
                .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                .single()
                .map(|d| d.timestamp_millis())
                .unwrap_or(0),
            "7d" => now.timestamp_millis() - 7 * 86400000,
            "30d" => now.timestamp_millis() - 30 * 86400000,
            _ => 0,
        };
        if start > 0 && item.updated_ms() < start {
            return false;
        }
        let text = format!(
            "{} {} {} {} {} {}",
            item.title,
            item.reference(),
            item.number,
            item.repo,
            item.team_name,
            item.labels
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase();
        search
            .split_whitespace()
            .all(|word| text.contains(&word.to_lowercase()))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub seeded: bool,
    pub items: BTreeMap<String, i64>,
}
impl Seen {
    pub fn unseen(&self, item: &Item) -> bool {
        self.seeded
            && self
                .items
                .get(&item.key())
                .is_none_or(|at| item.updated_ms() > *at)
    }
    pub fn mark(&mut self, item: &Item) {
        let old = self.items.entry(item.key()).or_default();
        *old = (*old).max(item.updated_ms());
    }
    pub fn seed(&mut self, items: &[Item]) {
        if !self.seeded && !items.is_empty() {
            for item in items {
                self.mark(item);
            }
            self.seeded = true;
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewState {
    pub source: String,
    pub filters: Filters,
    pub seen: Seen,
    pub hidden_linear_teams: Vec<String>,
    pub hidden_jira_projects: Vec<String>,
    pub list_width: f32,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            source: "github".into(),
            filters: Filters::default(),
            seen: Seen::default(),
            hidden_linear_teams: vec![],
            hidden_jira_projects: vec![],
            list_width: 280.,
        }
    }
}
impl ViewState {
    pub fn fetch_scope(&self, projects: &[PathBuf]) -> String {
        let mut paths = projects
            .iter()
            .map(|p| crate::files::path_key(&p.to_string_lossy()))
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        let mut teams = self.hidden_linear_teams.clone();
        teams.sort();
        let mut jira = self.hidden_jira_projects.clone();
        jira.sort();
        format!(
            "{}:{}:{}:{}:{}",
            self.filters.assigned_to_me,
            self.filters.fetch_state(),
            paths.join("|"),
            teams.join(","),
            jira.join(",")
        )
    }
    pub fn load(store: &Store) -> Self {
        if let Some(state) = store
            .setting("native-inbox-state")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            return state;
        }
        let desktop = store
            .data_dir()
            .map(DesktopPreferences::load)
            .unwrap_or_default();
        let mut state = Self::default();
        if let Some(source) = desktop
            .get("monocode.inboxSource")
            .filter(|s| PROVIDERS.contains(s))
        {
            state.source = source.into();
        }
        if let Some(filters) = desktop
            .json("monocode.inboxFilters")
            .and_then(|v| serde_json::from_value(v).ok())
        {
            state.filters = filters;
        }
        if let Some(seen) = desktop.json("monocode.inboxSeen") {
            state.seen = serde_json::from_value(seen.clone()).unwrap_or_else(|_| Seen {
                seeded: true,
                items: serde_json::from_value(seen).unwrap_or_default(),
            });
        }
        state.hidden_linear_teams = desktop
            .json("monocode.linearHiddenTeams")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        state.hidden_jira_projects = desktop
            .json("monocode.jiraHiddenProjects")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        state
    }
    pub fn save(&self, store: &Store) -> Result<(), String> {
        store.save_setting(
            "native-inbox-state",
            &serde_json::to_string(self).map_err(|e| e.to_string())?,
        )
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub errors: BTreeMap<String, String>,
    pub connections: BTreeMap<String, Status>,
    pub linear_teams: Vec<OptionRow>,
    pub jira_projects: Vec<OptionRow>,
    pub sonar_projects: Vec<sonar::SonarProject>,
    pub sonar_branches: Vec<sonar::SonarBranch>,
    pub sonar_selected_project: String,
}

/// The Tauri InboxNotificationTracker: provider baselines and revisions are
/// separate from the user's read state. Startup and query changes never
/// trigger automations for an existing backlog.
#[derive(Default)]
pub struct NotificationTracker {
    revisions: HashMap<String, i64>,
    primed: HashSet<String>,
    scope: Option<String>,
}
impl NotificationTracker {
    pub fn observe(&mut self, snapshot: &Snapshot, scope: String) -> Vec<Item> {
        if self.scope.as_ref() != Some(&scope) {
            self.primed.clear();
        }
        self.scope = Some(scope);
        let mut appeared = vec![];
        for item in &snapshot.items {
            if snapshot.errors.contains_key(&item.provider) {
                continue;
            }
            let at = item.updated_ms();
            if at == 0 {
                continue;
            }
            let project = match item.provider.as_str() {
                "linear" if !item.project_id.is_empty() => {
                    format!("linear:project:{}", item.project_id)
                }
                "linear" => format!("linear:team:{}:unassigned", item.team_id),
                "jira" => format!(
                    "jira:{}:project:{}",
                    item.url
                        .split('/')
                        .nth(2)
                        .unwrap_or("unknown")
                        .to_lowercase(),
                    if item.team_id.is_empty() {
                        &item.repo
                    } else {
                        &item.team_id
                    }
                ),
                _ if !item.project_path.is_empty() => crate::files::path_key(&item.project_path),
                _ => format!("{}:{}", item.provider, item.repo.to_lowercase()),
            };
            let id = if item.id.is_empty() {
                item.number.to_string()
            } else {
                item.id.clone()
            };
            let key = format!("{project}:{}:{id}", item.kind);
            let previous = self.revisions.get(&key).copied();
            if self.primed.contains(&item.provider) && previous.is_none() {
                appeared.push(item.clone());
            }
            self.revisions.insert(key, previous.unwrap_or(0).max(at));
        }
        for provider in PROVIDERS {
            if !snapshot.errors.contains_key(provider) {
                self.primed.insert(provider.into());
            }
        }
        appeared
    }
}

#[derive(Clone)]
pub struct Service {
    pub data_dir: PathBuf,
    pub cwd: PathBuf,
}
/// Pairs each repository with one checkout to fetch it through. Several
/// checkouts of the same repository share its rows, so the active checkout
/// goes first and keeps them when the inbox is scoped to it.
fn repo_checkouts<'a>(
    projects: &'a [PathBuf],
    cwd: &Path,
    mut repos: impl FnMut(&Path) -> Option<Vec<String>>,
) -> Vec<(&'a PathBuf, String)> {
    let mut ordered = projects.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|project| project.as_path() != cwd);
    let mut visited = HashSet::new();
    let mut checkouts = vec![];
    for project in ordered {
        for repo in repos(project).unwrap_or_default() {
            if visited.insert(repo.to_lowercase()) {
                checkouts.push((project, repo));
            }
        }
    }
    checkouts
}
fn convert<T: Serialize, U: DeserializeOwned>(value: T) -> Result<U, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
impl Service {
    pub fn sonar_content(&self, item: &Item) -> Result<sonar::SonarIssueContent, String> {
        sonar::sonar_issue_content(
            self.data_dir.clone(),
            item.sonar_project.clone(),
            item.sonar_branch.clone(),
            item.id.clone(),
        )
        .map(|mut content| {
            // Issue search does not always echo its branch query in the issue object.
            content.issue["branch"] = serde_json::Value::String(item.sonar_branch.clone());
            content.issue["project"] = serde_json::Value::String(item.sonar_project.clone());
            content
        })
    }
    pub fn status(&self, provider: &str) -> Result<Status, String> {
        match provider {
            "sonar" => convert(sonar::sonar_status(self.data_dir.clone())?),
            "linear" => convert(linear::linear_status(self.data_dir.clone())?),
            "jira" => convert(jira::jira_status(self.data_dir.clone())?),
            "gitlab" => convert(gitlab::gitlab_status(self.data_dir.clone())?),
            "azuredevops" => convert(azure_devops::azure_devops_status(self.data_dir.clone())?),
            _ => {
                let installed = crate::scm::resolve_gui_binary("gh").is_some();
                let authenticated = installed
                    && run_gh(
                        &self.cwd,
                        &["auth", "status", "--active", "--hostname", "github.com"],
                        true,
                    )
                    .is_ok();
                Ok(Status {
                    installed,
                    authenticated,
                    connected: authenticated,
                    ..Default::default()
                })
            }
        }
    }
    pub fn connect(
        &self,
        provider: &str,
        url: String,
        email: String,
        token: String,
    ) -> Result<Status, String> {
        match provider {
            "sonar" => convert(if token.is_empty() {
                sonar::sonar_disconnect(self.data_dir.clone())?
            } else {
                sonar::sonar_connect(self.data_dir.clone(), url, token)?
            }),
            "linear" => convert(linear::linear_set_token(self.data_dir.clone(), token)?),
            "jira" => convert(jira::jira_set_config(
                self.data_dir.clone(),
                url,
                email,
                token,
            )?),
            "gitlab" => convert(gitlab::gitlab_set_config(
                self.data_dir.clone(),
                url,
                token,
            )?),
            "azuredevops" => convert(azure_devops::azure_devops_set_config(
                self.data_dir.clone(),
                url,
                token,
            )?),
            _ => self.status("github"),
        }
    }
    pub fn list(&self, projects: &[PathBuf], state: &ViewState) -> Snapshot {
        self.list_with_project_names(projects, state, &HashMap::new())
    }
    pub fn list_with_project_names(
        &self,
        projects: &[PathBuf],
        state: &ViewState,
        appearances: &HashMap<String, crate::project_appearance::ProjectAppearance>,
    ) -> Snapshot {
        let mut snapshot = Snapshot::default();
        let limit = if state.filters.fetch_state() == "all" {
            100
        } else {
            40
        };
        for provider in PROVIDERS {
            let status = match self.status(provider) {
                Ok(status) => status,
                Err(error) => {
                    snapshot.errors.insert(provider.into(), error);
                    continue;
                }
            };
            let connected = status.connected;
            snapshot.connections.insert(provider.into(), status);
            if !connected {
                continue;
            }
            let result: Result<Vec<Item>, String> = (|| {
                if provider == "sonar" {
                    let status = sonar::sonar_status(self.data_dir.clone())?;
                    snapshot.sonar_projects = status.projects;
                    snapshot.sonar_selected_project = status.selected_project;
                    let rows = sonar::sonar_list_issues(
                        self.data_dir.clone(),
                        state.filters.assigned_to_me,
                        state.filters.fetch_state().into(),
                    )?;
                    snapshot.sonar_branches = rows.branches;
                    if !rows.errors.is_empty() {
                        snapshot
                            .errors
                            .insert("sonar".into(), rows.errors.join("\n"));
                    }
                    let mut checkouts = HashMap::new();
                    return Ok(rows
                        .issues
                        .into_iter()
                        .map(|issue| {
                            let checkout = checkouts
                                .entry(issue.repository_name.clone())
                                .or_insert_with(|| {
                                    sonar::project_with_names(
                                        projects,
                                        &self.cwd,
                                        &issue.repository_name,
                                        appearances,
                                    )
                                })
                                .clone();
                            sonar::item(issue, &[], &checkout)
                        })
                        .collect());
                }
                if provider == "linear" {
                    let teams = linear::linear_list_teams(self.data_dir.clone())?;
                    let visible = teams
                        .iter()
                        .filter(|t| !state.hidden_linear_teams.contains(&t.id))
                        .map(|t| t.id.clone())
                        .collect::<Vec<_>>();
                    snapshot.linear_teams = convert(teams)?;
                    if !state.hidden_linear_teams.is_empty() && visible.is_empty() {
                        return Ok(vec![]);
                    }
                    return convert(linear::linear_list_issues(
                        self.data_dir.clone(),
                        state.filters.assigned_to_me,
                        state.filters.fetch_state().into(),
                        if state.hidden_linear_teams.is_empty() {
                            vec![]
                        } else {
                            visible
                        },
                        Some(limit),
                    )?);
                }
                if provider == "jira" {
                    let projects = jira::jira_list_projects(self.data_dir.clone())?;
                    let visible = projects
                        .iter()
                        .filter(|p| !state.hidden_jira_projects.contains(&p.id))
                        .map(|p| p.id.clone())
                        .collect::<Vec<_>>();
                    snapshot.jira_projects = convert(projects)?;
                    if !state.hidden_jira_projects.is_empty() && visible.is_empty() {
                        return Ok(vec![]);
                    }
                    return convert(jira::jira_list_issues(
                        self.data_dir.clone(),
                        state.filters.assigned_to_me,
                        state.filters.fetch_state().into(),
                        if state.hidden_jira_projects.is_empty() {
                            vec![]
                        } else {
                            visible
                        },
                        Some(limit),
                    )?);
                }
                let mut items = Vec::new();
                if state.filters.assigned_to_me && provider != "github" {
                    for kind in ["issue", "pr"] {
                        let rows: Vec<Item> = if provider == "gitlab" {
                            convert(gitlab::gitlab_list_todos(
                                self.data_dir.clone(),
                                kind.into(),
                                Some(limit),
                            )?)?
                        } else {
                            convert(azure_devops::azure_devops_list_todos(
                                self.data_dir.clone(),
                                kind.into(),
                                Some(limit),
                            )?)?
                        };
                        items.extend(rows.into_iter().map(|mut item| {
                            item.provider = provider.into();
                            item
                        }));
                    }
                    return Ok(items);
                }
                let checkouts = repo_checkouts(projects, &self.cwd, |project| {
                    match provider {
                        "github" => github::git_github_repositories_for(project),
                        "gitlab" => gitlab::gitlab_repo(
                            self.data_dir.clone(),
                            project.to_string_lossy().into_owned(),
                        )
                        .map(|r| vec![r]),
                        _ => azure_devops::azure_devops_repo(
                            self.data_dir.clone(),
                            project.to_string_lossy().into_owned(),
                        )
                        .map(|r| vec![r]),
                    }
                    .ok()
                });
                for (project, repo) in checkouts {
                    for kind in ["issue", "pr"] {
                        let rows: Vec<Item> = match provider {
                            "github" => convert(github::git_github_work_items_for(
                                project,
                                &repo,
                                kind,
                                state.filters.assigned_to_me,
                                state.filters.fetch_state(),
                                "",
                                limit,
                            )?)?,
                            "gitlab" => convert(gitlab::gitlab_list_work_items(
                                self.data_dir.clone(),
                                project.to_string_lossy().into_owned(),
                                kind.into(),
                                false,
                                state.filters.fetch_state().into(),
                                Some(limit),
                            )?)?,
                            _ => convert(azure_devops::azure_devops_list_work_items(
                                self.data_dir.clone(),
                                project.to_string_lossy().into_owned(),
                                kind.into(),
                                false,
                                state.filters.fetch_state().into(),
                                Some(limit),
                            )?)?,
                        };
                        items.extend(rows.into_iter().map(|mut item| {
                            item.provider = provider.into();
                            item.project_path = project.to_string_lossy().into_owned();
                            item
                        }));
                    }
                }
                Ok(items)
            })();
            match result {
                Ok(items) => snapshot.items.extend(items),
                Err(error) => {
                    snapshot.errors.insert(provider.into(), error);
                }
            }
        }
        snapshot.items.sort_by(|a, b| {
            b.updated_ms()
                .cmp(&a.updated_ms())
                .then_with(|| a.title.cmp(&b.title))
        });
        let mut seen = HashSet::new();
        snapshot.items.retain(|i| seen.insert(i.key()));
        snapshot
    }
    pub fn details(&self, item: &Item) -> Result<Details, String> {
        match item.provider.as_str() {
            "sonar" => self.sonar_content(item).map(|content| Details {
                body: sonar::body(&content),
                head_ref_name: item.sonar_branch.clone(),
                ..Default::default()
            }),
            "linear" => convert(linear::linear_issue_details(
                self.data_dir.clone(),
                item.id.clone(),
            )?),
            "jira" => convert(jira::jira_issue_details(
                self.data_dir.clone(),
                item.identifier.clone(),
            )?),
            "gitlab" => convert(gitlab::gitlab_work_item_details(
                self.data_dir.clone(),
                item.repo.clone(),
                item.kind.clone(),
                item.number,
            )?),
            "azuredevops" => convert(azure_devops::azure_devops_work_item_details(
                self.data_dir.clone(),
                item.repo.clone(),
                item.kind.clone(),
                item.number,
            )?),
            _ => convert(github::git_github_work_item_details_for(
                self.project(item),
                &item.repo,
                &item.kind,
                item.number,
            )?),
        }
    }
    fn project<'a>(&'a self, item: &'a Item) -> &'a Path {
        if item.project_path.is_empty() {
            &self.cwd
        } else {
            Path::new(&item.project_path)
        }
    }
    pub fn thread(&self, item: &Item) -> Result<Thread, String> {
        match item.provider.as_str() {
            "sonar" => Ok(Thread::default()),
            "linear" => convert(linear::linear_issue_thread(
                self.data_dir.clone(),
                item.id.clone(),
            )?),
            "jira" => convert(jira::jira_issue_thread(
                self.data_dir.clone(),
                item.identifier.clone(),
            )?),
            "gitlab" => convert(gitlab::gitlab_work_item_thread(
                self.data_dir.clone(),
                item.repo.clone(),
                item.kind.clone(),
                item.number,
            )?),
            "azuredevops" => convert(azure_devops::azure_devops_work_item_thread(
                self.data_dir.clone(),
                item.repo.clone(),
                item.kind.clone(),
                item.number,
            )?),
            _ => convert(github::git_github_work_item_thread_for(
                self.project(item),
                &item.repo,
                &item.kind,
                item.number,
            )?),
        }
    }
    pub fn comment(&self, item: &Item, body: String, reply_to: String) -> Result<String, String> {
        match item.provider.as_str() {
            "sonar" => Err("Add an optional comment when changing the Sonar status".into()),
            "linear" => {
                linear::linear_issue_comment(self.data_dir.clone(), item.id.clone(), body, reply_to)
            }
            "jira" => {
                jira::jira_issue_comment(self.data_dir.clone(), item.identifier.clone(), body)
            }
            "gitlab" => gitlab::gitlab_work_item_comment(
                self.data_dir.clone(),
                item.repo.clone(),
                item.kind.clone(),
                item.number,
                body,
            ),
            "azuredevops" => azure_devops::azure_devops_work_item_comment(
                self.data_dir.clone(),
                item.repo.clone(),
                item.kind.clone(),
                item.number,
                body,
            ),
            _ => github::git_github_work_item_comment_for(
                self.project(item),
                &item.repo,
                &item.kind,
                item.number,
                &body,
                &reply_to,
            ),
        }
    }
    pub fn diff(&self, item: &Item) -> Result<Diff, String> {
        self.diff_context(item, false)
    }
    pub fn diff_context(&self, item: &Item, full_context: bool) -> Result<Diff, String> {
        match item.provider.as_str() {
            "sonar" => self
                .sonar_content(item)
                .map(|content| sonar::diff(&content)),
            "gitlab" => convert(gitlab::gitlab_mr_diff(
                self.data_dir.clone(),
                item.repo.clone(),
                item.number,
            )?),
            "azuredevops" => convert(azure_devops::azure_devops_mr_diff(
                self.data_dir.clone(),
                item.repo.clone(),
                item.number,
            )?),
            "github" => convert(github::git_github_pr_diff_for(
                self.project(item),
                &item.repo,
                item.number,
                full_context,
            )?),
            _ => Err("This issue has no pull request diff".into()),
        }
    }
    pub fn checks(&self, item: &Item) -> Result<Checks, String> {
        convert(github::git_github_pr_checks_for(
            self.project(item),
            &item.repo,
            item.number,
        )?)
    }
    pub fn action(&self, item: &Item, action: &str) -> Result<Item, String> {
        let mut updated: Item = convert(github::git_github_pr_action_for(
            self.project(item),
            &item.repo,
            item.number,
            action,
        )?)?;
        updated.provider = item.provider.clone();
        updated.project_path = item.project_path.clone();
        Ok(updated)
    }
}
pub(super) fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    run_command("git", root, args, true)
}
/// githubWorkItem: one GitHub issue or pull request a message links to,
/// looked up with `gh` from `cwd` (any folder, when the session's is gone).
pub fn github_work_item(cwd: &Path, repo: &str, kind: &str, number: i64) -> Result<Item, String> {
    let mut item: Item = convert(github::git_github_work_item_for(
        &gh_root(cwd),
        repo,
        kind,
        number,
    )?)?;
    item.provider = "github".into();
    Ok(item)
}
/// githubWorkItemDetails for [`github_work_item`]'s item.
pub fn github_work_item_details(
    cwd: &Path,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<Details, String> {
    convert(github::git_github_work_item_details_for(
        &gh_root(cwd),
        repo,
        kind,
        number,
    )?)
}
fn gh_root(cwd: &Path) -> PathBuf {
    let cwd = crate::scm::expand_home(cwd);
    if cwd.is_dir() {
        cwd
    } else {
        crate::scm::shell::dirs_home()
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    }
}
pub(super) fn run_gh(root: &Path, args: &[&str], allow_empty: bool) -> Result<String, String> {
    run_command("gh", root, args, allow_empty)
}
fn run_command(
    program: &str,
    root: &Path,
    args: &[&str],
    allow_empty: bool,
) -> Result<String, String> {
    let binary = crate::scm::resolve_gui_binary(program).ok_or_else(|| {
        format!(
            "{} is not installed. Install {} and check again.",
            if program == "gh" {
                "GitHub CLI (`gh`)"
            } else {
                program
            },
            program
        )
    })?;
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("GIT_PAGER", "cat");
    crate::scm::shell::apply_gui_env(&mut command);
    crate::process::configure_child(&mut command);
    let output = command.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    if stdout.is_empty() && !allow_empty {
        Err(format!("{program} returned no output"))
    } else {
        Ok(stdout)
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    fn item(number: i64, provider: &str) -> Item {
        Item {
            provider: provider.into(),
            kind: "issue".into(),
            repo: "acme/app".into(),
            number,
            id: number.to_string(),
            updated_at: "2026-10-21T10:00:00Z".into(),
            ..Item::default()
        }
    }
    fn snapshot(items: Vec<Item>) -> Snapshot {
        Snapshot {
            items,
            ..Snapshot::default()
        }
    }

    #[test]
    fn first_baseline_and_existing_revisions_never_appear() {
        let mut tracker = NotificationTracker::default();
        assert!(tracker
            .observe(&snapshot(vec![item(1, "github")]), "scope".into())
            .is_empty());
        let mut updated = item(1, "github");
        updated.updated_at = "2026-10-21T11:00:00Z".into();
        let appeared = tracker.observe(&snapshot(vec![updated, item(2, "github")]), "scope".into());
        assert_eq!(
            appeared.iter().map(|i| i.number).collect::<Vec<_>>(),
            vec![2]
        );
        assert!(tracker
            .observe(&snapshot(vec![item(2, "github")]), "scope".into())
            .is_empty());
    }

    #[test]
    fn failed_provider_has_its_own_successful_baseline() {
        let mut tracker = NotificationTracker::default();
        let mut failed = snapshot(vec![item(1, "linear")]);
        failed.errors.insert("linear".into(), "offline".into());
        assert!(tracker.observe(&failed, "scope".into()).is_empty());
        assert!(tracker
            .observe(&snapshot(vec![item(2, "linear")]), "scope".into())
            .is_empty());
        let appeared = tracker.observe(
            &snapshot(vec![item(2, "linear"), item(3, "linear")]),
            "scope".into(),
        );
        assert_eq!(appeared.len(), 1);
        assert_eq!(appeared[0].number, 3);
    }

    #[test]
    fn changing_query_baselines_new_scope_without_forgetting_known_items() {
        let mut tracker = NotificationTracker::default();
        assert!(tracker
            .observe(&snapshot(vec![item(1, "github")]), "one".into())
            .is_empty());
        assert!(tracker
            .observe(&snapshot(vec![item(2, "github")]), "two".into())
            .is_empty());
        assert!(tracker
            .observe(
                &snapshot(vec![item(1, "github"), item(2, "github")]),
                "two".into()
            )
            .is_empty());
        assert_eq!(
            tracker
                .observe(&snapshot(vec![item(3, "github")]), "two".into())
                .len(),
            1
        );
    }

    #[test]
    fn seen_keys_match_desktop_identity_rules() {
        let mut github = item(12, "github");
        github.url = "https://GitHub.com/Acme/App/pull/12/".into();
        assert_eq!(github.key(), "github:acme/app:issue:12");
        let mut linear = item(7, "linear");
        linear.id = "stable-id".into();
        linear.identifier = "MON-7".into();
        assert_eq!(linear.key(), "linear:mon-7");
    }

    #[test]
    fn two_checkouts_of_one_repository_fetch_it_through_the_active_one() {
        let first = PathBuf::from("/work/app");
        let second = PathBuf::from("/work/app-review");
        let other = PathBuf::from("/work/site");
        let projects = [first.clone(), second.clone(), other.clone()];
        let repos = |project: &Path| {
            Some(vec![if project == other {
                "acme/site".to_string()
            } else {
                "Acme/App".to_string()
            }])
        };
        for (cwd, owner) in [(&first, &first), (&second, &second), (&other, &first)] {
            let checkouts = repo_checkouts(&projects, cwd, repos);
            assert_eq!(checkouts.len(), 2, "each repository is fetched once");
            assert!(
                checkouts
                    .iter()
                    .any(|(project, repo)| *project == owner && repo == "Acme/App"),
                "from {cwd:?}: {checkouts:?}"
            );
        }
    }
}
