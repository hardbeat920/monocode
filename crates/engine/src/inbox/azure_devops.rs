//! Port of src/features/inbox/model/azureDevOps.ts: Azure DevOps status and
//! settings, the repository lookup, work item and pull request lists,
//! To-Dos, details, threads, comments, and pull request diffs, with their
//! caches. Unlike GitLab, a request that a cache clear or a forced refresh
//! superseded does not write its result back.
//!
//! `notifyAzureDevOpsChange` becomes `InboxSignal::AzureDevOpsChange`.

use std::collections::HashMap;

use monocode_core::js;
use serde_json::json;

use super::backend::args_with_limit;
use super::client::{Flights, InboxClient, InboxSignal, Pending, ready};
use super::github_tasks::details_cache_key;
use super::inbox_self_activity::InboxSelfActivityTarget;
use super::types::{
    AzureDevOpsStatus, InboxProvider, InboxState, PrDiff, RepositoryWorkItem, WorkItemDetails,
    WorkItemKind, WorkItemThread, inbox_kind, work_kind_str,
};
use crate::runtime::util::project_path::normalize_project_path;

/// The module-level maps of azureDevOps.ts.
#[derive(Default)]
pub(crate) struct AzureDevOpsCache {
    repo_by_path: HashMap<String, String>,
    details_by_key: HashMap<String, WorkItemDetails>,
    details_inflight: Flights<WorkItemDetails>,
    thread_by_key: HashMap<String, WorkItemThread>,
    thread_inflight: Flights<WorkItemThread>,
    diff_by_key: HashMap<String, PrDiff>,
    diff_inflight: Flights<PrDiff>,
}

impl AzureDevOpsCache {
    /// `clearAzureDevOpsCache`.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl InboxClient {
    /// `clearAzureDevOpsCache`.
    pub fn clear_azure_dev_ops_cache(&self) {
        self.state().azure_devops.clear();
    }

    /// `azureDevOpsConnected`.
    pub fn azure_dev_ops_connected(&self) -> Pending<AzureDevOpsStatus> {
        self.request("azure_devops_status", json!({}))
    }

    /// `saveAzureDevOpsConfig`, or `disconnectAzureDevOps` with an empty token.
    pub fn save_azure_dev_ops_config(&self, url: &str, token: &str) -> Pending<AzureDevOpsStatus> {
        let call = self.call::<AzureDevOpsStatus>(
            "azure_devops_set_config",
            json!({ "url": js::trim(url), "token": js::trim(token) }),
        );
        let client = self.clone();
        self.spawn_pending(async move {
            let status = call.await?;
            client.clear_azure_dev_ops_cache();
            client.emit(InboxSignal::AzureDevOpsChange);
            Ok(status)
        })
    }

    /// `disconnectAzureDevOps`.
    pub fn disconnect_azure_dev_ops(&self, url: &str) -> Pending<AzureDevOpsStatus> {
        self.save_azure_dev_ops_config(url, "")
    }

    /// `azureDevOpsRepo`.
    pub fn azure_dev_ops_repo(&self, cwd: &str) -> Pending<String> {
        let key = normalize_project_path(cwd);
        if let Some(cached) = self.state().azure_devops.repo_by_path.get(&key) {
            return ready(Ok(cached.clone()));
        }
        let call = self.call::<String>("azure_devops_repo", json!({ "cwd": cwd }));
        let client = self.clone();
        self.spawn_pending(async move {
            let repo = call.await?;
            client
                .state()
                .azure_devops
                .repo_by_path
                .insert(key, repo.clone());
            Ok(repo)
        })
    }

    /// `listAzureDevOpsWorkItems`.
    pub fn list_azure_dev_ops_work_items(
        &self,
        cwd: &str,
        kind: WorkItemKind,
        assigned_to_me: bool,
        state: InboxState,
        limit: Option<u32>,
    ) -> Pending<Vec<RepositoryWorkItem>> {
        self.request(
            "azure_devops_list_work_items",
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

    /// `listAzureDevOpsTodos`.
    pub fn list_azure_dev_ops_todos(
        &self,
        kind: WorkItemKind,
        limit: Option<u32>,
    ) -> Pending<Vec<RepositoryWorkItem>> {
        self.request(
            "azure_devops_list_todos",
            args_with_limit(json!({ "kind": work_kind_str(kind) }), limit),
        )
    }

    /// `peekAzureDevOpsWorkItemDetails`.
    pub fn peek_azure_dev_ops_work_item_details(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<WorkItemDetails> {
        self.state()
            .azure_devops
            .details_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `azureDevOpsWorkItemDetails`.
    pub fn azure_dev_ops_work_item_details(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Pending<WorkItemDetails> {
        let key = details_cache_key(repo, kind, number);
        let mut state = self.state();
        if let Some(pending) = state.azure_devops.details_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<WorkItemDetails>(
            "azure_devops_work_item_details",
            json!({ "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            // A cache clear (an organization change) supersedes this request:
            // skip the write so a stale answer cannot repopulate the cache.
            if let Ok(details) = &result
                && state
                    .azure_devops
                    .details_inflight
                    .is_current(&cache_key, id)
            {
                state
                    .azure_devops
                    .details_by_key
                    .insert(cache_key.clone(), details.clone());
            }
            state.azure_devops.details_inflight.finish(&cache_key, id);
            result
        });
        state
            .azure_devops
            .details_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `peekAzureDevOpsWorkItemThread`.
    pub fn peek_azure_dev_ops_work_item_thread(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<WorkItemThread> {
        self.state()
            .azure_devops
            .thread_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `azureDevOpsWorkItemThread`.
    pub fn azure_dev_ops_work_item_thread(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        force: bool,
    ) -> Pending<WorkItemThread> {
        let key = details_cache_key(repo, kind, number);
        let mut state = self.state();
        if force {
            state.azure_devops.thread_by_key.remove(&key);
            state.azure_devops.thread_inflight.remove(&key);
        }
        if let Some(pending) = state.azure_devops.thread_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<WorkItemThread>(
            "azure_devops_work_item_thread",
            json!({ "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            // A forced refresh or a cache clear supersedes this request.
            if let Ok(thread) = &result
                && state
                    .azure_devops
                    .thread_inflight
                    .is_current(&cache_key, id)
            {
                state
                    .azure_devops
                    .thread_by_key
                    .insert(cache_key.clone(), thread.clone());
            }
            state.azure_devops.thread_inflight.finish(&cache_key, id);
            result
        });
        state
            .azure_devops
            .thread_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `azureDevOpsWorkItemComment`.
    pub fn azure_dev_ops_work_item_comment(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        body: &str,
    ) -> Pending<String> {
        let call = self.call::<String>(
            "azure_devops_work_item_comment",
            json!({ "repo": repo, "kind": work_kind_str(kind), "number": number, "body": js::trim(body) }),
        );
        let key = details_cache_key(repo, kind, number);
        let client = self.clone();
        let repo = repo.to_string();
        self.spawn_pending(async move {
            let url = call.await?;
            {
                let mut state = client.state();
                state.azure_devops.thread_by_key.remove(&key);
                state.azure_devops.thread_inflight.remove(&key);
            }
            client.record_inbox_self_activity(InboxSelfActivityTarget::work_item(
                InboxProvider::AzureDevops,
                inbox_kind(kind),
                &repo,
                number,
            ));
            Ok(url)
        })
    }

    /// `peekAzureDevOpsMrDiff`.
    pub fn peek_azure_dev_ops_mr_diff(&self, repo: &str, number: i64) -> Option<PrDiff> {
        self.state()
            .azure_devops
            .diff_by_key
            .get(&details_cache_key(repo, WorkItemKind::Pr, number))
            .cloned()
    }

    /// `azureDevOpsMrDiff`.
    pub fn azure_dev_ops_mr_diff(&self, repo: &str, number: i64) -> Pending<PrDiff> {
        let key = details_cache_key(repo, WorkItemKind::Pr, number);
        let mut state = self.state();
        if let Some(pending) = state.azure_devops.diff_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<PrDiff>(
            "azure_devops_mr_diff",
            json!({ "repo": repo, "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(diff) = &result
                && state.azure_devops.diff_inflight.is_current(&cache_key, id)
            {
                state
                    .azure_devops
                    .diff_by_key
                    .insert(cache_key.clone(), diff.clone());
            }
            state.azure_devops.diff_inflight.finish(&cache_key, id);
            result
        });
        state
            .azure_devops
            .diff_inflight
            .insert(key, id, pending.clone());
        pending
    }
}
