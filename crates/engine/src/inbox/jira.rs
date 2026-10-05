//! Port of src/features/inbox/model/jira.ts: Jira status and settings,
//! projects, issues, details, threads, comments, and the hidden-project
//! setting, with caches keyed by issue key (ENG-42). A cache generation
//! keeps a request that finishes after an account change from restoring the
//! old account's data.
//!
//! localStorage becomes `Kv` with the same key and JSON.
//! `notifyJiraChange` becomes `InboxSignal::JiraChange`.

use std::collections::{HashMap, HashSet};

use monocode_core::js;
use monocode_settings::Kv;
use serde_json::{Value, json};

use super::backend::args_with_limit;
use super::client::{Flights, InboxClient, InboxSignal, Pending};
use super::github_tasks::limit_for_state;
use super::inbox_self_activity::InboxSelfActivityTarget;
use super::types::{
    InboxItem, InboxKind, InboxProvider, InboxQuery, InboxState, JiraProject, JiraStatus,
    TrackerIssue, WorkItemDetails, WorkItemThread,
};

/// `PROJECT_IDS_KEY`.
pub const JIRA_HIDDEN_PROJECTS_KEY: &str = "monocode.jiraHiddenProjects";

/// The module-level maps of jira.ts.
#[derive(Default)]
pub(crate) struct JiraCache {
    details_by_key: HashMap<String, WorkItemDetails>,
    thread_by_key: HashMap<String, WorkItemThread>,
    thread_inflight: Flights<WorkItemThread>,
    generation: u64,
}

impl JiraCache {
    /// `clearJiraCache`.
    pub(crate) fn clear(&mut self) {
        self.generation += 1;
        self.details_by_key.clear();
        self.thread_by_key.clear();
        self.thread_inflight.clear();
    }
}

/// `jiraProjectIdsForFetch`: `None` means do not filter by project, an
/// empty list means every known project is hidden.
pub fn jira_project_ids_for_fetch(
    projects: &[JiraProject],
    hidden_ids: &[String],
) -> Option<Vec<String>> {
    tracker_ids_for_fetch(projects, hidden_ids)
}

/// The shared body of `jiraProjectIdsForFetch` and `linearTeamIdsForFetch`.
pub(crate) fn tracker_ids_for_fetch(
    groups: &[super::types::TrackerGroup],
    hidden_ids: &[String],
) -> Option<Vec<String>> {
    if hidden_ids.is_empty() {
        return None;
    }
    let hidden: HashSet<&str> = hidden_ids.iter().map(String::as_str).collect();
    let visible: Vec<String> = groups
        .iter()
        .filter(|group| !hidden.contains(group.id.as_str()))
        .map(|group| group.id.clone())
        .collect();
    if visible.len() == groups.len() {
        return None;
    }
    Some(visible)
}

/// Read a stored list of non-empty ids, as `loadHiddenJiraProjectIds` and
/// `loadHiddenLinearTeamIds` do.
pub(crate) fn load_id_list(kv: &Kv, key: &str) -> Vec<String> {
    let Some(raw) = kv.get_item(key).filter(|raw| !raw.is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Array(values)) => values
            .into_iter()
            .filter_map(|value| match value {
                Value::String(id) if !id.is_empty() => Some(id),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `loadHiddenJiraProjectIds`.
pub fn load_hidden_jira_project_ids(kv: &Kv) -> Vec<String> {
    load_id_list(kv, JIRA_HIDDEN_PROJECTS_KEY)
}

/// `jiraIssueToInboxItem`.
pub fn jira_issue_to_inbox_item(issue: &TrackerIssue) -> InboxItem {
    InboxItem {
        kind: InboxKind::Jira,
        number: issue.number,
        title: issue.title.clone(),
        url: issue.url.clone(),
        state: issue.state.clone(),
        state_reason: None,
        created_at: None,
        updated_at: issue.updated_at.clone(),
        labels: issue.labels.clone(),
        assignees: issue.assignees.clone(),
        draft: false,
        repo: issue.repo.clone(),
        project_path: issue.project_path.clone(),
        provider: InboxProvider::Jira,
        id: Some(issue.id.clone()),
        identifier: Some(issue.identifier.clone()),
        team_id: Some(issue.team_id.clone()),
        team_name: Some(issue.team_name.clone()),
        project_id: None,
        project_name: None,
        state_type: Some(issue.state_type.clone()),
        attention_reason: None,
    }
}

impl InboxClient {
    /// `clearJiraCache`.
    pub fn clear_jira_cache(&self) {
        self.state().jira.clear();
    }

    /// `jiraConnected`.
    pub fn jira_connected(&self) -> Pending<JiraStatus> {
        self.request("jira_status", json!({}))
    }

    /// `saveJiraConfig`.
    pub fn save_jira_config(&self, site: &str, email: &str, token: &str) -> Pending<JiraStatus> {
        let call = self.call::<JiraStatus>(
            "jira_set_config",
            json!({ "site": js::trim(site), "email": js::trim(email), "token": js::trim(token) }),
        );
        let client = self.clone();
        self.spawn_pending(async move {
            let status = call.await?;
            client.clear_jira_cache();
            Ok(status)
        })
    }

    /// `disconnectJira`.
    pub fn disconnect_jira(&self) -> Pending<JiraStatus> {
        self.save_jira_config("", "", "")
    }

    /// `listJiraProjects`.
    pub fn list_jira_projects(&self) -> Pending<Vec<JiraProject>> {
        self.request("jira_list_projects", json!({}))
    }

    /// `listJiraIssues`.
    pub fn list_jira_issues(
        &self,
        assigned_to_me: bool,
        state: InboxState,
        project_ids: Vec<String>,
        limit: Option<u32>,
    ) -> Pending<Vec<TrackerIssue>> {
        self.request(
            "jira_list_issues",
            args_with_limit(
                json!({ "assignedToMe": assigned_to_me, "state": state.as_str(), "projectIds": project_ids }),
                limit,
            ),
        )
    }

    /// `peekJiraIssueDetails`.
    pub fn peek_jira_issue_details(&self, key: &str) -> Option<WorkItemDetails> {
        self.state().jira.details_by_key.get(key).cloned()
    }

    /// `jiraIssueDetails`.
    pub fn jira_issue_details(&self, key: &str) -> Pending<WorkItemDetails> {
        let generation = self.state().jira.generation;
        let call = self.call::<WorkItemDetails>("jira_issue_details", json!({ "key": key }));
        let client = self.clone();
        let key = key.to_string();
        self.spawn_pending(async move {
            let details = call.await?;
            let mut state = client.state();
            if generation == state.jira.generation {
                state.jira.details_by_key.insert(key, details.clone());
            }
            Ok(details)
        })
    }

    /// `peekJiraIssueThread`.
    pub fn peek_jira_issue_thread(&self, key: &str) -> Option<WorkItemThread> {
        self.state().jira.thread_by_key.get(key).cloned()
    }

    /// `jiraIssueThread`.
    pub fn jira_issue_thread(&self, key: &str, force: bool) -> Pending<WorkItemThread> {
        let mut state = self.state();
        if force {
            state.jira.thread_by_key.remove(key);
            state.jira.thread_inflight.remove(key);
        }
        if let Some(pending) = state.jira.thread_inflight.get(key) {
            return pending;
        }
        let generation = state.jira.generation;
        let id = state.next_id();
        let call = self.call::<WorkItemThread>("jira_issue_thread", json!({ "key": key }));
        let client = self.clone();
        let cache_key = key.to_string();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(thread) = &result
                && generation == state.jira.generation
                && state.jira.thread_inflight.is_current(&cache_key, id)
            {
                state
                    .jira
                    .thread_by_key
                    .insert(cache_key.clone(), thread.clone());
            }
            state.jira.thread_inflight.finish(&cache_key, id);
            result
        });
        state
            .jira
            .thread_inflight
            .insert(key.to_string(), id, pending.clone());
        pending
    }

    /// `jiraIssueComment`: `id` is the numeric issue id, `key` the issue key.
    pub fn jira_issue_comment(&self, id: &str, key: &str, body: &str) -> Pending<String> {
        let call = self.call::<String>(
            "jira_issue_comment",
            json!({ "key": key, "body": js::trim(body) }),
        );
        let client = self.clone();
        let id = id.to_string();
        let key = key.to_string();
        self.spawn_pending(async move {
            let url = call.await?;
            {
                let mut state = client.state();
                state.jira.thread_by_key.remove(&key);
                state.jira.thread_inflight.remove(&key);
            }
            client.record_inbox_self_activity(InboxSelfActivityTarget::tracker(
                InboxProvider::Jira,
                InboxKind::Jira,
                &id,
            ));
            Ok(url)
        })
    }

    /// `saveHiddenJiraProjectIds`.
    pub fn save_hidden_jira_project_ids(&self, ids: &[String]) {
        self.kv().set_item(
            JIRA_HIDDEN_PROJECTS_KEY,
            &serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()),
        );
        self.emit(InboxSignal::JiraChange);
    }

    /// `notifyJiraChange`.
    pub fn notify_jira_change(&self) {
        self.emit(InboxSignal::JiraChange);
    }

    /// `fetchJiraInboxItems`.
    pub(crate) async fn fetch_jira_inbox_items(
        &self,
        query: &InboxQuery,
    ) -> Result<Vec<InboxItem>, String> {
        let hidden_ids = query
            .jira_hidden_project_ids
            .clone()
            .unwrap_or_else(|| load_hidden_jira_project_ids(self.kv()));
        let mut project_ids = None;
        if !hidden_ids.is_empty() {
            project_ids =
                jira_project_ids_for_fetch(&self.list_jira_projects().await?, &hidden_ids);
            if project_ids.as_ref().is_some_and(Vec::is_empty) {
                return Ok(Vec::new());
            }
        }
        let issues = self
            .list_jira_issues(
                query.assigned_to_me,
                query.state,
                project_ids.unwrap_or_default(),
                limit_for_state(query.state),
            )
            .await?;
        let hidden: HashSet<&str> = hidden_ids.iter().map(String::as_str).collect();
        Ok(issues
            .iter()
            .filter(|issue| hidden.is_empty() || !hidden.contains(issue.team_id.as_str()))
            .map(jira_issue_to_inbox_item)
            .collect())
    }
}
