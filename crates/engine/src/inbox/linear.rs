//! Port of src/features/inbox/model/linear.ts: Linear status and token,
//! teams, issues, details, threads, comments, and the hidden-team setting,
//! with caches keyed by issue id.
//!
//! localStorage becomes `Kv` with the same key and JSON.
//! `notifyLinearChange` becomes `InboxSignal::LinearChange`.

use std::collections::{HashMap, HashSet};

use monocode_core::js;
use monocode_settings::Kv;
use serde_json::json;

use super::backend::args_with_limit;
use super::client::{Flights, InboxClient, InboxSignal, Pending};
use super::github_tasks::limit_for_state;
use super::inbox_self_activity::InboxSelfActivityTarget;
use super::jira::{load_id_list, tracker_ids_for_fetch};
use super::types::{
    InboxItem, InboxKind, InboxProvider, InboxQuery, InboxState, LinearStatus, LinearTeam,
    TrackerIssue, WorkItemDetails, WorkItemThread,
};

/// `TEAM_IDS_KEY`.
pub const LINEAR_HIDDEN_TEAMS_KEY: &str = "monocode.linearHiddenTeams";

/// The module-level maps of linear.ts.
#[derive(Default)]
pub(crate) struct LinearCache {
    details_by_id: HashMap<String, WorkItemDetails>,
    thread_by_id: HashMap<String, WorkItemThread>,
    thread_inflight: Flights<WorkItemThread>,
}

/// `linearTeamIdsForFetch`: `None` means do not filter by team, an empty
/// list means every known team is hidden.
pub fn linear_team_ids_for_fetch(
    teams: &[LinearTeam],
    hidden_ids: &[String],
) -> Option<Vec<String>> {
    tracker_ids_for_fetch(teams, hidden_ids)
}

/// `loadHiddenLinearTeamIds`.
pub fn load_hidden_linear_team_ids(kv: &Kv) -> Vec<String> {
    load_id_list(kv, LINEAR_HIDDEN_TEAMS_KEY)
}

/// `linearIssueToInboxItem`.
pub fn linear_issue_to_inbox_item(issue: &TrackerIssue) -> InboxItem {
    InboxItem {
        kind: InboxKind::Linear,
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
        provider: InboxProvider::Linear,
        id: Some(issue.id.clone()),
        identifier: Some(issue.identifier.clone()),
        team_id: Some(issue.team_id.clone()),
        team_name: Some(issue.team_name.clone()),
        project_id: Some(issue.project_id.clone()),
        project_name: Some(issue.project_name.clone()),
        state_type: Some(issue.state_type.clone()),
        attention_reason: None,
    }
}

impl InboxClient {
    /// `linearConnected`.
    pub fn linear_connected(&self) -> Pending<LinearStatus> {
        self.request("linear_status", json!({}))
    }

    /// `saveLinearToken`.
    pub fn save_linear_token(&self, token: &str) -> Pending<LinearStatus> {
        self.request("linear_set_token", json!({ "token": js::trim(token) }))
    }

    /// `disconnectLinear`.
    pub fn disconnect_linear(&self) -> Pending<LinearStatus> {
        self.request("linear_set_token", json!({ "token": "" }))
    }

    /// `listLinearTeams`.
    pub fn list_linear_teams(&self) -> Pending<Vec<LinearTeam>> {
        self.request("linear_list_teams", json!({}))
    }

    /// `listLinearIssues`.
    pub fn list_linear_issues(
        &self,
        assigned_to_me: bool,
        state: InboxState,
        team_ids: Vec<String>,
        limit: Option<u32>,
    ) -> Pending<Vec<TrackerIssue>> {
        self.request(
            "linear_list_issues",
            args_with_limit(
                json!({ "assignedToMe": assigned_to_me, "state": state.as_str(), "teamIds": team_ids }),
                limit,
            ),
        )
    }

    /// `peekLinearIssueDetails`.
    pub fn peek_linear_issue_details(&self, id: &str) -> Option<WorkItemDetails> {
        self.state().linear.details_by_id.get(id).cloned()
    }

    /// `linearIssueDetails`.
    pub fn linear_issue_details(&self, id: &str) -> Pending<WorkItemDetails> {
        let call = self.call::<WorkItemDetails>("linear_issue_details", json!({ "id": id }));
        let client = self.clone();
        let id = id.to_string();
        self.spawn_pending(async move {
            let details = call.await?;
            client
                .state()
                .linear
                .details_by_id
                .insert(id, details.clone());
            Ok(details)
        })
    }

    /// `peekLinearIssueThread`.
    pub fn peek_linear_issue_thread(&self, id: &str) -> Option<WorkItemThread> {
        self.state().linear.thread_by_id.get(id).cloned()
    }

    /// `linearIssueThread`.
    pub fn linear_issue_thread(&self, id: &str, force: bool) -> Pending<WorkItemThread> {
        let mut state = self.state();
        if force {
            state.linear.thread_by_id.remove(id);
            state.linear.thread_inflight.remove(id);
        }
        if let Some(pending) = state.linear.thread_inflight.get(id) {
            return pending;
        }
        let request = state.next_id();
        let call = self.call::<WorkItemThread>("linear_issue_thread", json!({ "id": id }));
        let client = self.clone();
        let cache_key = id.to_string();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(thread) = &result {
                state
                    .linear
                    .thread_by_id
                    .insert(cache_key.clone(), thread.clone());
            }
            state.linear.thread_inflight.finish(&cache_key, request);
            result
        });
        state
            .linear
            .thread_inflight
            .insert(id.to_string(), request, pending.clone());
        pending
    }

    /// `linearIssueComment`, or a reply under `parent_id`.
    pub fn linear_issue_comment(
        &self,
        id: &str,
        body: &str,
        parent_id: Option<&str>,
    ) -> Pending<String> {
        let call = self.call::<String>(
            "linear_issue_comment",
            json!({ "id": id, "body": js::trim(body), "parentId": parent_id.map(js::trim).unwrap_or("") }),
        );
        let client = self.clone();
        let id = id.to_string();
        self.spawn_pending(async move {
            let url = call.await?;
            {
                let mut state = client.state();
                state.linear.thread_by_id.remove(&id);
                state.linear.thread_inflight.remove(&id);
            }
            client.record_inbox_self_activity(InboxSelfActivityTarget::tracker(
                InboxProvider::Linear,
                InboxKind::Linear,
                &id,
            ));
            Ok(url)
        })
    }

    /// `saveHiddenLinearTeamIds`.
    pub fn save_hidden_linear_team_ids(&self, ids: &[String]) {
        self.kv().set_item(
            LINEAR_HIDDEN_TEAMS_KEY,
            &serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()),
        );
        self.emit(InboxSignal::LinearChange);
    }

    /// `notifyLinearChange`.
    pub fn notify_linear_change(&self) {
        self.emit(InboxSignal::LinearChange);
    }

    /// `fetchLinearInboxItems`.
    pub(crate) async fn fetch_linear_inbox_items(
        &self,
        query: &InboxQuery,
    ) -> Result<Vec<InboxItem>, String> {
        let hidden_ids = query
            .linear_hidden_team_ids
            .clone()
            .unwrap_or_else(|| load_hidden_linear_team_ids(self.kv()));
        let mut team_ids = None;
        if !hidden_ids.is_empty() {
            team_ids = linear_team_ids_for_fetch(&self.list_linear_teams().await?, &hidden_ids);
            if team_ids.as_ref().is_some_and(Vec::is_empty) {
                return Ok(Vec::new());
            }
        }
        let issues = self
            .list_linear_issues(
                query.assigned_to_me,
                query.state,
                team_ids.unwrap_or_default(),
                limit_for_state(query.state),
            )
            .await?;
        let hidden: HashSet<&str> = hidden_ids.iter().map(String::as_str).collect();
        Ok(issues
            .iter()
            .filter(|issue| hidden.is_empty() || !hidden.contains(issue.team_id.as_str()))
            .map(linear_issue_to_inbox_item)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::types::TrackerGroup;

    fn teams() -> Vec<LinearTeam> {
        vec![
            TrackerGroup {
                id: "t1".into(),
                key: "ENG".into(),
                name: "Engineering".into(),
            },
            TrackerGroup {
                id: "t2".into(),
                key: "DES".into(),
                name: "Design".into(),
            },
        ]
    }

    #[test]
    fn sends_no_team_filter_when_nothing_is_hidden() {
        assert_eq!(linear_team_ids_for_fetch(&teams(), &[]), None);
    }

    #[test]
    fn keeps_visible_team_ids() {
        assert_eq!(
            linear_team_ids_for_fetch(&teams(), &["t2".into()]),
            Some(vec!["t1".to_string()])
        );
    }

    #[test]
    fn returns_empty_when_every_known_team_is_hidden() {
        assert_eq!(
            linear_team_ids_for_fetch(&teams(), &["t1".into(), "t2".into()]),
            Some(vec![])
        );
    }

    #[test]
    fn sends_no_team_filter_when_hidden_ids_match_no_team() {
        assert_eq!(linear_team_ids_for_fetch(&teams(), &["gone".into()]), None);
    }

    #[test]
    fn loads_only_non_empty_string_ids() {
        let kv = Kv::in_memory();
        assert!(load_hidden_linear_team_ids(&kv).is_empty());
        kv.set_item(LINEAR_HIDDEN_TEAMS_KEY, r#"["t1", "", 3, "t2"]"#);
        assert_eq!(load_hidden_linear_team_ids(&kv), ["t1", "t2"]);
        kv.set_item(LINEAR_HIDDEN_TEAMS_KEY, "not json");
        assert!(load_hidden_linear_team_ids(&kv).is_empty());
    }
}
