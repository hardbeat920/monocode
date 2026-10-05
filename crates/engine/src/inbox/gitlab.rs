//! Port of src/features/inbox/model/gitlab.ts: GitLab status and settings,
//! the project lookup, issue and merge request lists, To-Dos, details,
//! threads, comments, and merge request diffs, with their caches.
//!
//! `notifyGitlabChange` becomes `InboxSignal::GitlabChange`.

use std::collections::HashMap;

use monocode_core::js;
use serde_json::json;

use super::backend::args_with_limit;
use super::client::{Flights, InboxClient, InboxSignal, Pending, ready};
use super::github_tasks::details_cache_key;
use super::inbox_self_activity::InboxSelfActivityTarget;
use super::types::{
    GitlabStatus, InboxProvider, InboxState, PrDiff, RepositoryWorkItem, WorkItemDetails,
    WorkItemKind, WorkItemThread, inbox_kind, work_kind_str,
};
use crate::runtime::util::project_path::normalize_project_path;

/// The module-level maps of gitlab.ts.
#[derive(Default)]
pub(crate) struct GitlabCache {
    repo_by_path: HashMap<String, String>,
    details_by_key: HashMap<String, WorkItemDetails>,
    thread_by_key: HashMap<String, WorkItemThread>,
    thread_inflight: Flights<WorkItemThread>,
    diff_by_key: HashMap<String, PrDiff>,
    diff_inflight: Flights<PrDiff>,
}

impl GitlabCache {
    /// `clearGitlabCache`.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl InboxClient {
    /// `clearGitlabCache`.
    pub fn clear_gitlab_cache(&self) {
        self.state().gitlab.clear();
    }

    /// `gitlabConnected`.
    pub fn gitlab_connected(&self) -> Pending<GitlabStatus> {
        self.request("gitlab_status", json!({}))
    }

    /// `saveGitlabConfig`, or `disconnectGitlab` with an empty token.
    pub fn save_gitlab_config(&self, url: &str, token: &str) -> Pending<GitlabStatus> {
        let call = self.call::<GitlabStatus>(
            "gitlab_set_config",
            json!({ "url": js::trim(url), "token": js::trim(token) }),
        );
        let client = self.clone();
        self.spawn_pending(async move {
            let status = call.await?;
            client.clear_gitlab_cache();
            client.emit(InboxSignal::GitlabChange);
            Ok(status)
        })
    }

    /// `disconnectGitlab`.
    pub fn disconnect_gitlab(&self, url: &str) -> Pending<GitlabStatus> {
        self.save_gitlab_config(url, "")
    }

    /// `gitlabRepo`: the checkout's GitLab project, cached by path.
    pub fn gitlab_repo(&self, cwd: &str) -> Pending<String> {
        let key = normalize_project_path(cwd);
        if let Some(cached) = self.state().gitlab.repo_by_path.get(&key) {
            return ready(Ok(cached.clone()));
        }
        let call = self.call::<String>("gitlab_repo", json!({ "cwd": cwd }));
        let client = self.clone();
        self.spawn_pending(async move {
            let repo = call.await?;
            client.state().gitlab.repo_by_path.insert(key, repo.clone());
            Ok(repo)
        })
    }

    /// `listGitlabWorkItems`.
    pub fn list_gitlab_work_items(
        &self,
        cwd: &str,
        kind: WorkItemKind,
        assigned_to_me: bool,
        state: InboxState,
        limit: Option<u32>,
    ) -> Pending<Vec<RepositoryWorkItem>> {
        self.request(
            "gitlab_list_work_items",
            args_with_limit(
                json!({
                    "cwd": cwd,
                    "kind": work_kind_str(kind),
                    "assignedToMe": assigned_to_me,
                    "state": state.as_str(),
                }),
                limit,
            ),
        )
    }

    /// `listGitlabTodos`.
    pub fn list_gitlab_todos(
        &self,
        kind: WorkItemKind,
        limit: Option<u32>,
    ) -> Pending<Vec<RepositoryWorkItem>> {
        self.request(
            "gitlab_list_todos",
            args_with_limit(json!({ "kind": work_kind_str(kind) }), limit),
        )
    }

    /// `peekGitlabWorkItemDetails`.
    pub fn peek_gitlab_work_item_details(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<WorkItemDetails> {
        self.state()
            .gitlab
            .details_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `gitlabWorkItemDetails`.
    pub fn gitlab_work_item_details(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Pending<WorkItemDetails> {
        let call = self.call::<WorkItemDetails>(
            "gitlab_work_item_details",
            json!({ "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let key = details_cache_key(repo, kind, number);
        let client = self.clone();
        self.spawn_pending(async move {
            let details = call.await?;
            client
                .state()
                .gitlab
                .details_by_key
                .insert(key, details.clone());
            Ok(details)
        })
    }

    /// `peekGitlabWorkItemThread`.
    pub fn peek_gitlab_work_item_thread(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<WorkItemThread> {
        self.state()
            .gitlab
            .thread_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `gitlabWorkItemThread`.
    pub fn gitlab_work_item_thread(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        force: bool,
    ) -> Pending<WorkItemThread> {
        let key = details_cache_key(repo, kind, number);
        let mut state = self.state();
        if force {
            state.gitlab.thread_by_key.remove(&key);
            state.gitlab.thread_inflight.remove(&key);
        }
        if let Some(pending) = state.gitlab.thread_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<WorkItemThread>(
            "gitlab_work_item_thread",
            json!({ "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(thread) = &result {
                state
                    .gitlab
                    .thread_by_key
                    .insert(cache_key.clone(), thread.clone());
            }
            state.gitlab.thread_inflight.finish(&cache_key, id);
            result
        });
        state
            .gitlab
            .thread_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `gitlabWorkItemComment`.
    pub fn gitlab_work_item_comment(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        body: &str,
    ) -> Pending<String> {
        let call = self.call::<String>(
            "gitlab_work_item_comment",
            json!({ "repo": repo, "kind": work_kind_str(kind), "number": number, "body": js::trim(body) }),
        );
        let key = details_cache_key(repo, kind, number);
        let client = self.clone();
        let repo = repo.to_string();
        self.spawn_pending(async move {
            let url = call.await?;
            {
                let mut state = client.state();
                state.gitlab.thread_by_key.remove(&key);
                state.gitlab.thread_inflight.remove(&key);
            }
            client.record_inbox_self_activity(InboxSelfActivityTarget::work_item(
                InboxProvider::Gitlab,
                inbox_kind(kind),
                &repo,
                number,
            ));
            Ok(url)
        })
    }

    /// `peekGitlabMrDiff`.
    pub fn peek_gitlab_mr_diff(&self, repo: &str, number: i64) -> Option<PrDiff> {
        self.state()
            .gitlab
            .diff_by_key
            .get(&details_cache_key(repo, WorkItemKind::Pr, number))
            .cloned()
    }

    /// `gitlabMrDiff`.
    pub fn gitlab_mr_diff(&self, repo: &str, number: i64) -> Pending<PrDiff> {
        let key = details_cache_key(repo, WorkItemKind::Pr, number);
        let mut state = self.state();
        if let Some(pending) = state.gitlab.diff_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<PrDiff>("gitlab_mr_diff", json!({ "repo": repo, "number": number }));
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(diff) = &result {
                state
                    .gitlab
                    .diff_by_key
                    .insert(cache_key.clone(), diff.clone());
            }
            state.gitlab.diff_inflight.finish(&cache_key, id);
            result
        });
        state.gitlab.diff_inflight.insert(key, id, pending.clone());
        pending
    }
}
