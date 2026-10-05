//! Port of the caches and fetches in src/features/inbox/model/githubTasks.ts.
//!
//! The TypeScript kept module-level maps of results and in-flight promises.
//! `InboxClient` holds the same maps behind one lock, shared by every clone.
//! Each request runs on the background executor and settles whether or not
//! anyone awaits it, as a promise does, and callers get a `Pending`: a
//! cloneable future of the result. The GitLab, Azure DevOps, Jira, and Linear
//! caches extend this type from their own modules.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Weak};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared, join_all};
use gpui::BackgroundExecutor;
use monocode_core::js;
use monocode_settings::Kv;
use parking_lot::{Mutex, MutexGuard};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::azure_devops::AzureDevOpsCache;
use super::backend::{InboxBackend, args_with_limit};
use super::github_tasks::{
    CollectedInboxItems, INBOX_CACHE_FRESH_MS, ProjectRepo, collect_inbox_results,
    dedupe_inbox_items, details_cache_key, group_projects_by_repo, inbox_list_cache_key,
    limit_for_state, pr_diff_cache_key, unique_inbox_projects,
};
use super::gitlab::GitlabCache;
use super::inbox_seen::{self, InboxSeenEntry, KnownInboxItems, RememberedInboxItem};
use super::inbox_self_activity::{InboxSelfActivity, InboxSelfActivityTarget};
use super::jira::JiraCache;
use super::linear::LinearCache;
use super::time::now_ms;
use super::types::{
    GithubPrAction, GithubStarStatus, GithubStatus, GithubWorkItem, GithubWorkItemQuery, InboxItem,
    InboxKind, InboxListResult, InboxProvider, InboxProviderErrors, InboxQuery, PrDiff,
    RepositoryWorkItem, WorkItemDetails, WorkItemKind, WorkItemThread, work_kind_str,
};
use crate::runtime::util::project_path::normalize_project_path;

/// A request's result, cloneable so several callers can await one request.
pub type Pending<T> = Shared<BoxFuture<'static, Result<T, String>>>;

/// An already settled `Pending`.
pub fn ready<T: Clone + Send + Sync + 'static>(result: Result<T, String>) -> Pending<T> {
    futures::future::ready(result).boxed().shared()
}

/// The change events the TypeScript dispatched: the seen-store listeners,
/// the self-activity listeners, the linked-session seen listeners, the
/// `monocode:*-change` window events, and the CI repair listeners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InboxSignal {
    Seen,
    SelfActivity,
    LinkedSessionSeen,
    LinearChange,
    JiraChange,
    GitlabChange,
    AzureDevOpsChange,
    CiRepairs,
}

/// In-flight requests by cache key. The id tells a request whether it is
/// still the current one when it settles (`inflight.get(key) === promise`).
pub(crate) struct Flights<T> {
    map: HashMap<String, (u64, Pending<T>)>,
}

impl<T> Default for Flights<T> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl<T: Clone> Flights<T> {
    pub(crate) fn get(&self, key: &str) -> Option<Pending<T>> {
        self.map.get(key).map(|(_, pending)| pending.clone())
    }

    pub(crate) fn insert(&mut self, key: String, id: u64, pending: Pending<T>) {
        self.map.insert(key, (id, pending));
    }

    pub(crate) fn is_current(&self, key: &str, id: u64) -> bool {
        self.map.get(key).is_some_and(|(current, _)| *current == id)
    }

    /// The `finally` block: forget the request if it is still current.
    pub(crate) fn finish(&mut self, key: &str, id: u64) {
        if self.is_current(key, id) {
            self.map.remove(key);
        }
    }

    pub(crate) fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
    }
}

/// The GitHub maps from githubTasks.ts.
#[derive(Default)]
pub(crate) struct GithubCache {
    repo_by_path: HashMap<String, String>,
    repositories_by_path: HashMap<String, Vec<String>>,
    work_item_by_key: HashMap<String, GithubWorkItem>,
    work_item_inflight: Flights<GithubWorkItem>,
    details_by_key: HashMap<String, WorkItemDetails>,
    details_inflight: Flights<WorkItemDetails>,
    thread_by_key: HashMap<String, WorkItemThread>,
    thread_inflight: Flights<WorkItemThread>,
    pr_diff_by_key: HashMap<String, PrDiff>,
    pr_diff_inflight: Flights<PrDiff>,
    /// When each details, thread, and diff entry last arrived, by
    /// `details:`, `thread:`, or `diff:` plus the cache key.
    fetched_at: HashMap<String, i64>,
}

impl GithubCache {
    /// `freshEnough`: whether the entry at `key` arrived less than
    /// `max_age_ms` before `now`. No age means the caller wants a fetch.
    fn fresh_enough(&self, key: &str, max_age_ms: Option<i64>, now: i64) -> bool {
        let Some(max_age_ms) = max_age_ms else {
            return false;
        };
        self.fetched_at
            .get(key)
            .is_some_and(|at| now - at < max_age_ms)
    }
}

/// `InboxListCache`.
struct InboxListCache {
    key: String,
    result: InboxListResult,
    fetched_at: i64,
}

/// Every module-level map the inbox models kept.
#[derive(Default)]
pub(crate) struct ClientState {
    pub(crate) github: GithubCache,
    pub(crate) gitlab: GitlabCache,
    pub(crate) azure_devops: AzureDevOpsCache,
    pub(crate) jira: JiraCache,
    pub(crate) linear: LinearCache,
    pub(crate) self_activity: InboxSelfActivity,
    pub(crate) known: KnownInboxItems,
    pub(crate) media: super::inbox_media::MediaCache,
    list_cache: Option<InboxListCache>,
    list_inflight: Flights<InboxListResult>,
    generation: u64,
    next_request: u64,
}

impl ClientState {
    pub(crate) fn next_id(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }
}

type Listener = Arc<dyn Fn(InboxSignal) + Send + Sync>;
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

struct ClientInner {
    backend: Arc<dyn InboxBackend>,
    executor: BackgroundExecutor,
    kv: Kv,
    clock: Clock,
    state: Mutex<ClientState>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener: Mutex<u64>,
}

/// The inbox caches and requests. Cloning is cheap and clones share state.
#[derive(Clone)]
pub struct InboxClient {
    inner: Arc<ClientInner>,
}

/// Keeps a signal listener registered. Dropping it unsubscribes.
pub struct SignalSubscription {
    client: Weak<ClientInner>,
    id: u64,
}

impl Drop for SignalSubscription {
    fn drop(&mut self) {
        if let Some(client) = self.client.upgrade() {
            client.listeners.lock().retain(|(id, _)| *id != self.id);
        }
    }
}

impl InboxClient {
    pub fn new(backend: Arc<dyn InboxBackend>, kv: Kv, executor: BackgroundExecutor) -> Self {
        Self::with_clock(backend, kv, executor, Arc::new(now_ms))
    }

    /// A client whose `Date.now()` is `clock`, for tests.
    pub fn with_clock(
        backend: Arc<dyn InboxBackend>,
        kv: Kv,
        executor: BackgroundExecutor,
        clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Self {
        Self {
            inner: Arc::new(ClientInner {
                backend,
                executor,
                kv,
                clock,
                state: Mutex::new(ClientState::default()),
                listeners: Mutex::new(Vec::new()),
                next_listener: Mutex::new(0),
            }),
        }
    }

    pub fn kv(&self) -> &Kv {
        &self.inner.kv
    }

    pub fn executor(&self) -> &BackgroundExecutor {
        &self.inner.executor
    }

    pub(crate) fn backend(&self) -> &Arc<dyn InboxBackend> {
        &self.inner.backend
    }

    /// `Date.now()`.
    pub fn now(&self) -> i64 {
        (self.inner.clock)()
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, ClientState> {
        self.inner.state.lock()
    }

    /// Call `listener` after every change signal. It runs on the thread that
    /// made the change, so it should only send to a channel.
    pub fn subscribe(
        &self,
        listener: impl Fn(InboxSignal) + Send + Sync + 'static,
    ) -> SignalSubscription {
        let id = {
            let mut next = self.inner.next_listener.lock();
            *next += 1;
            *next
        };
        self.inner.listeners.lock().push((id, Arc::new(listener)));
        SignalSubscription {
            client: Arc::downgrade(&self.inner),
            id,
        }
    }

    pub(crate) fn emit(&self, signal: InboxSignal) {
        let listeners: Vec<Listener> = self
            .inner
            .listeners
            .lock()
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect();
        for listener in listeners {
            listener(signal);
        }
    }

    /// `invoke<T>(command, args)`. The backend sees the call now; the result
    /// arrives when the returned future runs.
    pub(crate) fn call<T: DeserializeOwned + Send + 'static>(
        &self,
        command: &str,
        args: Value,
    ) -> BoxFuture<'static, Result<T, String>> {
        self.inner
            .backend
            .invoke(command, args)
            .map(|result| {
                result.and_then(|value| {
                    serde_json::from_value(value).map_err(|error| error.to_string())
                })
            })
            .boxed()
    }

    /// Run `work` on the background executor to completion and share its
    /// result.
    pub(crate) fn spawn_pending<T: Clone + Send + Sync + 'static>(
        &self,
        work: impl Future<Output = Result<T, String>> + Send + 'static,
    ) -> Pending<T> {
        let (sender, receiver) = oneshot::channel();
        self.inner
            .executor
            .spawn(async move {
                let _ = sender.send(work.await);
            })
            .detach();
        receiver
            .map(|result| result.unwrap_or_else(|_| Err("The inbox request was dropped".into())))
            .boxed()
            .shared()
    }

    /// `invoke` as a `Pending`, with no cache.
    pub(crate) fn request<T: DeserializeOwned + Clone + Send + Sync + 'static>(
        &self,
        command: &str,
        args: Value,
    ) -> Pending<T> {
        let call = self.call::<T>(command, args);
        self.spawn_pending(call)
    }

    // Self activity and the seen store.

    /// `recordInboxSelfActivity`.
    pub fn record_inbox_self_activity(&self, target: InboxSelfActivityTarget) {
        let now = self.now();
        self.state().self_activity.record(target, now);
        self.emit(InboxSignal::SelfActivity);
    }

    /// `consumeInboxSelfActivity`.
    pub fn consume_inbox_self_activity(&self, item: &InboxItem) -> bool {
        let now = self.now();
        self.state().self_activity.consume(item, now)
    }

    /// `clearPendingInboxSelfActivity`.
    pub fn clear_pending_inbox_self_activity(&self) {
        self.state().self_activity.clear();
    }

    /// `rememberInboxItems`.
    pub fn remember_inbox_items(&self, entries: &[RememberedInboxItem]) {
        let changed = self.state().known.remember(entries);
        if changed {
            self.emit(InboxSignal::Seen);
        }
    }

    /// `knownInboxEntries`.
    pub fn known_inbox_entries(&self, project_paths: &[String]) -> Vec<InboxSeenEntry> {
        self.state().known.entries(project_paths)
    }

    /// `clearKnownInboxItems`.
    pub fn clear_known_inbox_items(&self) {
        self.state().known.clear();
        self.emit(InboxSignal::Seen);
    }

    /// `markInboxItemSeen`.
    pub fn mark_inbox_item_seen(&self, entry: &InboxSeenEntry) {
        if inbox_seen::mark_inbox_item_seen(self.kv(), entry) {
            self.emit(InboxSignal::Seen);
        }
    }

    /// `markInboxItemsSeen`.
    pub fn mark_inbox_items_seen(&self, entries: &[InboxSeenEntry]) -> bool {
        let saved = inbox_seen::mark_inbox_items_seen(self.kv(), entries);
        if saved {
            self.emit(InboxSignal::Seen);
        }
        saved
    }

    /// `seedInboxSeenIfNeeded`.
    pub fn seed_inbox_seen_if_needed(&self, items: &[InboxSeenEntry]) {
        if inbox_seen::seed_inbox_seen_if_needed(self.kv(), items) {
            self.emit(InboxSignal::Seen);
        }
    }

    /// `isInboxEntryUnseen`.
    pub fn is_inbox_entry_unseen(&self, entry: &InboxSeenEntry) -> bool {
        inbox_seen::is_inbox_entry_unseen(self.kv(), entry)
    }

    /// `inboxHasUnseenItems`.
    pub fn inbox_has_unseen_items(&self, items: &[InboxSeenEntry]) -> bool {
        inbox_seen::inbox_has_unseen_items(self.kv(), items)
    }

    // The list cache.

    /// `clearInboxCache`.
    pub fn clear_inbox_cache(&self) {
        {
            let mut state = self.state();
            state.generation += 1;
            state.jira.clear();
            state.known.clear();
            state.list_cache = None;
            state.list_inflight.clear();
            state.github = GithubCache::default();
            state.gitlab.clear();
            state.azure_devops.clear();
        }
        self.emit(InboxSignal::Seen);
    }

    /// `peekInboxList`.
    pub fn peek_inbox_list(
        &self,
        projects: &[String],
        query: &InboxQuery,
    ) -> Option<InboxListResult> {
        let key = inbox_list_cache_key(projects, query);
        let state = self.state();
        let cache = state.list_cache.as_ref().filter(|cache| cache.key == key)?;
        Some(cache.result.clone())
    }

    /// `peekInboxItems`.
    pub fn peek_inbox_items(
        &self,
        projects: &[String],
        query: &InboxQuery,
    ) -> Option<Vec<InboxItem>> {
        self.peek_inbox_list(projects, query).map(|list| list.items)
    }

    /// `inboxListIsFresh`.
    pub fn inbox_list_is_fresh(&self, projects: &[String], query: &InboxQuery, now: i64) -> bool {
        let key = inbox_list_cache_key(projects, query);
        self.state()
            .list_cache
            .as_ref()
            .is_some_and(|cache| cache.key == key && now - cache.fetched_at < INBOX_CACHE_FRESH_MS)
    }

    /// `listInboxItems`: the cached list while fresh, else one shared fetch
    /// per cache key.
    pub fn list_inbox_items(
        &self,
        projects: &[String],
        query: &InboxQuery,
        force: bool,
    ) -> Pending<InboxListResult> {
        let key = inbox_list_cache_key(projects, query);
        if !force && self.inbox_list_is_fresh(projects, query, self.now()) {
            return ready(Ok(self
                .peek_inbox_list(projects, query)
                .unwrap_or_default()));
        }
        let mut state = self.state();
        if let Some(pending) = state.list_inflight.get(&key) {
            return pending;
        }
        let generation = state.generation;
        let id = state.next_id();
        let fetch = self
            .clone()
            .fetch_inbox_items(projects.to_vec(), query.clone());
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = fetch.await;
            let mut state = client.state();
            if let Ok(result) = &result
                && generation == state.generation
            {
                state.list_cache = Some(InboxListCache {
                    key: cache_key.clone(),
                    result: result.clone(),
                    fetched_at: client.now(),
                });
            }
            state.list_inflight.finish(&cache_key, id);
            result
        });
        state.list_inflight.insert(key, id, pending.clone());
        pending
    }

    /// Replace one cached card, as `githubPrAction` does.
    fn update_cached_list(
        &self,
        state: &mut ClientState,
        mut update: impl FnMut(&InboxItem) -> Option<InboxItem>,
    ) {
        if let Some(cache) = state.list_cache.as_mut() {
            for item in &mut cache.result.items {
                if let Some(next) = update(item) {
                    *item = next;
                }
            }
        }
    }

    fn fetch_inbox_items(
        self,
        projects: Vec<String>,
        query: InboxQuery,
    ) -> BoxFuture<'static, Result<InboxListResult, String>> {
        async move {
            let unique = unique_inbox_projects(&projects);
            let preferred_paths = unique.clone();
            let discovery =
                join_all(unique.iter().map(|path| self.github_repositories(path))).await;
            let mut resolved = Vec::new();
            let mut discovery_failures = Vec::new();
            for (index, result) in discovery.into_iter().enumerate() {
                match result {
                    Ok(repositories) => resolved.extend(
                        repositories
                            .into_iter()
                            .map(|repo| ProjectRepo::new(unique[index].clone(), repo)),
                    ),
                    Err(error) => discovery_failures.push(Err(error)),
                }
            }
            let grouped = group_projects_by_repo(&resolved);
            let jobs = grouped.iter().flat_map(|project| {
                [WorkItemKind::Issue, WorkItemKind::Pr].map(|kind| {
                    let listed = self.list_github_work_items(
                        &project.path,
                        &project.repo,
                        &GithubWorkItemQuery {
                            kind,
                            assigned_to_me: query.assigned_to_me,
                            state: query.state,
                            search: query.search.clone(),
                        },
                    );
                    let project = project.clone();
                    async move {
                        listed.await.map(|items| {
                            items
                                .iter()
                                .map(|item| {
                                    let mut inbox = InboxItem::from_github(
                                        item,
                                        &project.path,
                                        InboxProvider::Github,
                                    );
                                    if inbox.repo.is_empty() {
                                        inbox.repo = project.repo.clone();
                                    }
                                    inbox
                                })
                                .collect::<Vec<_>>()
                        })
                    }
                })
            });
            let mut settled = join_all(jobs).await;
            settled.extend(discovery_failures);
            let github = collect_inbox_results(settled, &preferred_paths);
            let mut errors = InboxProviderErrors::new();
            if let Some(error) = github.error
                && !unique.is_empty()
            {
                errors.set(InboxProvider::Github, error);
            }

            let mut linear_items = Vec::new();
            if self.linear_connected().await?.connected {
                match self.fetch_linear_inbox_items(&query).await {
                    Ok(items) => linear_items = items,
                    Err(error) => errors.set(InboxProvider::Linear, error),
                }
            }

            let mut jira_items = Vec::new();
            match self.jira_connected().await {
                Ok(status) if status.connected => match self.fetch_jira_inbox_items(&query).await {
                    Ok(items) => jira_items = items,
                    Err(error) => errors.set(InboxProvider::Jira, error),
                },
                Ok(_) => {}
                Err(error) => errors.set(InboxProvider::Jira, error),
            }

            let mut gitlab_items = Vec::new();
            if self.gitlab_connected().await?.connected {
                let gitlab = self
                    .fetch_repository_inbox_items(
                        InboxProvider::Gitlab,
                        &unique,
                        &query,
                        &preferred_paths,
                    )
                    .await;
                gitlab_items = gitlab.items;
                if let Some(error) = gitlab.error {
                    errors.set(InboxProvider::Gitlab, error);
                }
            }

            let mut azure_devops_items = Vec::new();
            if self.azure_dev_ops_connected().await?.connected {
                let azure = self
                    .fetch_repository_inbox_items(
                        InboxProvider::AzureDevops,
                        &unique,
                        &query,
                        &preferred_paths,
                    )
                    .await;
                azure_devops_items = azure.items;
                if let Some(error) = azure.error {
                    errors.set(InboxProvider::AzureDevops, error);
                }
            }

            let items = github
                .items
                .into_iter()
                .chain(linear_items)
                .chain(jira_items)
                .chain(gitlab_items)
                .chain(azure_devops_items)
                .collect();
            Ok(InboxListResult {
                items: dedupe_inbox_items(items, &preferred_paths),
                errors,
            })
        }
        .boxed()
    }

    /// `fetchRepositoryInboxItems` for GitLab or Azure DevOps.
    async fn fetch_repository_inbox_items(
        &self,
        provider: InboxProvider,
        projects: &[String],
        query: &InboxQuery,
        preferred_paths: &[String],
    ) -> CollectedInboxItems {
        let resolved = join_all(projects.iter().map(|path| {
            let repo = self.provider_repo(provider, path);
            let path = path.clone();
            async move {
                ProjectRepo::new(
                    path,
                    repo.await
                        .map(|repo| js::trim(&repo).to_string())
                        .unwrap_or_default(),
                )
            }
        }))
        .await;
        let grouped = group_projects_by_repo(
            &resolved
                .into_iter()
                .filter(|project| !project.repo.is_empty())
                .collect::<Vec<_>>(),
        );
        let limit = limit_for_state(query.state);
        let to_inbox_item = move |item: &RepositoryWorkItem, project_path: &str, repo: &str| {
            repository_inbox_item(provider, item, project_path, repo)
        };

        if query.assigned_to_me {
            let local_path_by_repo: HashMap<String, String> = grouped
                .iter()
                .map(|project| (project.repo.to_lowercase(), project.path.clone()))
                .collect();
            let jobs = [WorkItemKind::Issue, WorkItemKind::Pr].map(|kind| {
                let listed = self.provider_todos(provider, kind, limit);
                let local_path_by_repo = local_path_by_repo.clone();
                async move {
                    listed.await.map(|items| {
                        items
                            .iter()
                            .map(|item| {
                                let path = local_path_by_repo
                                    .get(&item.repo.to_lowercase())
                                    .cloned()
                                    .unwrap_or_default();
                                to_inbox_item(item, &path, &item.repo)
                            })
                            .collect::<Vec<_>>()
                    })
                }
            });
            return collect_inbox_results(join_all(jobs).await, preferred_paths);
        }

        let jobs = grouped.iter().flat_map(|project| {
            [WorkItemKind::Issue, WorkItemKind::Pr].map(|kind| {
                let listed =
                    self.provider_work_items(provider, &project.path, kind, query.state, limit);
                let project = project.clone();
                async move {
                    listed.await.map(|items| {
                        items
                            .iter()
                            .map(|item| to_inbox_item(item, &project.path, &project.repo))
                            .collect::<Vec<_>>()
                    })
                }
            })
        });
        collect_inbox_results(join_all(jobs).await, preferred_paths)
    }

    fn provider_repo(&self, provider: InboxProvider, cwd: &str) -> Pending<String> {
        if provider == InboxProvider::Gitlab {
            self.gitlab_repo(cwd)
        } else {
            self.azure_dev_ops_repo(cwd)
        }
    }

    fn provider_todos(
        &self,
        provider: InboxProvider,
        kind: WorkItemKind,
        limit: Option<u32>,
    ) -> Pending<Vec<RepositoryWorkItem>> {
        if provider == InboxProvider::Gitlab {
            self.list_gitlab_todos(kind, limit)
        } else {
            self.list_azure_dev_ops_todos(kind, limit)
        }
    }

    fn provider_work_items(
        &self,
        provider: InboxProvider,
        cwd: &str,
        kind: WorkItemKind,
        state: super::types::InboxState,
        limit: Option<u32>,
    ) -> Pending<Vec<RepositoryWorkItem>> {
        if provider == InboxProvider::Gitlab {
            self.list_gitlab_work_items(cwd, kind, false, state, limit)
        } else {
            self.list_azure_dev_ops_work_items(cwd, kind, false, state, limit)
        }
    }

    // GitHub.

    /// `githubStatus`.
    pub fn github_status(&self) -> Pending<GithubStatus> {
        self.request("git_github_status", json!({}))
    }

    /// `githubMonocodeStarStatus`.
    pub fn github_monocode_star_status(&self) -> Pending<GithubStarStatus> {
        self.request("github_monocode_star_status", json!({}))
    }

    /// `starMonocodeOnGithub`.
    pub fn star_monocode_on_github(&self) -> Pending<()> {
        self.request("github_star_monocode", json!({}))
    }

    /// `githubRepo`: the checkout's GitHub remote, cached by path.
    pub fn github_repo(&self, cwd: &str) -> Pending<String> {
        let key = normalize_project_path(cwd);
        {
            let state = self.state();
            if let Some(cached) = state.github.repo_by_path.get(&key) {
                return ready(Ok(cached.clone()));
            }
            if let Some(first) = state
                .github
                .repositories_by_path
                .get(&key)
                .and_then(|repos| repos.first())
            {
                return ready(Ok(first.clone()));
            }
        }
        let call = self.call::<String>("git_github_repo", json!({ "cwd": cwd }));
        let client = self.clone();
        self.spawn_pending(async move {
            let repo = call.await?;
            client.state().github.repo_by_path.insert(key, repo.clone());
            Ok(repo)
        })
    }

    /// `githubRepositories`: the checkout's remote and, for a fork, its
    /// parent.
    pub fn github_repositories(&self, cwd: &str) -> Pending<Vec<String>> {
        let key = normalize_project_path(cwd);
        if let Some(cached) = self.state().github.repositories_by_path.get(&key) {
            return ready(Ok(cached.clone()));
        }
        let call = self.call::<Vec<String>>("git_github_repositories", json!({ "cwd": cwd }));
        let client = self.clone();
        self.spawn_pending(async move {
            let repositories = call.await?;
            let Some(first) = repositories.first().cloned() else {
                return Err("GitHub did not return a repository".into());
            };
            let mut state = client.state();
            state
                .github
                .repositories_by_path
                .insert(key.clone(), repositories.clone());
            state.github.repo_by_path.insert(key, first);
            Ok(repositories)
        })
    }

    /// `listGithubWorkItems`.
    pub fn list_github_work_items(
        &self,
        cwd: &str,
        repo: &str,
        query: &GithubWorkItemQuery,
    ) -> Pending<Vec<GithubWorkItem>> {
        self.request(
            "git_github_work_items",
            args_with_limit(
                json!({
                    "cwd": cwd,
                    "repo": repo,
                    "kind": work_kind_str(query.kind),
                    "assignedToMe": query.assigned_to_me,
                    "state": query.state.as_str(),
                    "search": js::trim(&query.search),
                }),
                limit_for_state(query.state),
            ),
        )
    }

    /// `peekGithubWorkItem`.
    pub fn peek_github_work_item(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<GithubWorkItem> {
        self.state()
            .github
            .work_item_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `githubWorkItem`: one exact item after targeted navigation misses the
    /// list cache.
    pub fn github_work_item(
        &self,
        cwd: &str,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        force: bool,
    ) -> Pending<GithubWorkItem> {
        let key = details_cache_key(repo, kind, number);
        let mut state = self.state();
        if let Some(cached) = state.github.work_item_by_key.get(&key)
            && !force
        {
            return ready(Ok(cached.clone()));
        }
        if let Some(pending) = state.github.work_item_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<GithubWorkItem>(
            "git_github_work_item",
            json!({ "cwd": cwd, "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(item) = &result {
                state
                    .github
                    .work_item_by_key
                    .insert(cache_key.clone(), item.clone());
            }
            state.github.work_item_inflight.finish(&cache_key, id);
            result
        });
        state
            .github
            .work_item_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `peekGithubWorkItemDetails`.
    pub fn peek_github_work_item_details(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<WorkItemDetails> {
        self.state()
            .github
            .details_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `githubWorkItemDetails`: one shared request per item. With
    /// `max_age_ms`, a description fetched that recently answers instead.
    pub fn github_work_item_details(
        &self,
        cwd: &str,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        max_age_ms: Option<i64>,
    ) -> Pending<WorkItemDetails> {
        let key = details_cache_key(repo, kind, number);
        let mut state = self.state();
        if let Some(cached) = state.github.details_by_key.get(&key)
            && state
                .github
                .fresh_enough(&format!("details:{key}"), max_age_ms, self.now())
        {
            return ready(Ok(cached.clone()));
        }
        if let Some(pending) = state.github.details_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<WorkItemDetails>(
            "git_github_work_item_details",
            json!({ "cwd": cwd, "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(details) = &result {
                state
                    .github
                    .details_by_key
                    .insert(cache_key.clone(), details.clone());
                state
                    .github
                    .fetched_at
                    .insert(format!("details:{cache_key}"), client.now());
            }
            state.github.details_inflight.finish(&cache_key, id);
            result
        });
        state
            .github
            .details_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `peekGithubWorkItemThread`.
    pub fn peek_github_work_item_thread(
        &self,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) -> Option<WorkItemThread> {
        self.state()
            .github
            .thread_by_key
            .get(&details_cache_key(repo, kind, number))
            .cloned()
    }

    /// `githubWorkItemThread`. With `max_age_ms`, a thread fetched that
    /// recently answers instead.
    pub fn github_work_item_thread(
        &self,
        cwd: &str,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        force: bool,
        max_age_ms: Option<i64>,
    ) -> Pending<WorkItemThread> {
        let key = details_cache_key(repo, kind, number);
        let mut state = self.state();
        if force {
            state.github.thread_by_key.remove(&key);
            state.github.thread_inflight.remove(&key);
        }
        if let Some(cached) = state.github.thread_by_key.get(&key)
            && state
                .github
                .fresh_enough(&format!("thread:{key}"), max_age_ms, self.now())
        {
            return ready(Ok(cached.clone()));
        }
        if let Some(pending) = state.github.thread_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<WorkItemThread>(
            "git_github_work_item_thread",
            json!({ "cwd": cwd, "repo": repo, "kind": work_kind_str(kind), "number": number }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(thread) = &result {
                state
                    .github
                    .thread_by_key
                    .insert(cache_key.clone(), thread.clone());
                state
                    .github
                    .fetched_at
                    .insert(format!("thread:{cache_key}"), client.now());
            }
            state.github.thread_inflight.finish(&cache_key, id);
            result
        });
        state
            .github
            .thread_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `githubWorkItemComment`: post a comment or a review-thread reply,
    /// then drop the cached thread and remember the mutation.
    pub fn github_work_item_comment(
        &self,
        cwd: &str,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
        body: &str,
        in_reply_to: Option<&str>,
    ) -> Pending<String> {
        let call = self.call::<String>(
            "git_github_work_item_comment",
            json!({
                "cwd": cwd,
                "repo": repo,
                "kind": work_kind_str(kind),
                "number": number,
                "body": js::trim(body),
                "inReplyTo": in_reply_to.map(js::trim).unwrap_or(""),
            }),
        );
        let key = details_cache_key(repo, kind, number);
        let client = self.clone();
        let repo = repo.to_string();
        self.spawn_pending(async move {
            let url = call.await?;
            {
                let mut state = client.state();
                state.github.thread_by_key.remove(&key);
                state.github.thread_inflight.remove(&key);
            }
            client.record_inbox_self_activity(InboxSelfActivityTarget::work_item(
                InboxProvider::Github,
                super::types::inbox_kind(kind),
                &repo,
                number,
            ));
            Ok(url)
        })
    }

    /// `githubPrAction`: run a state-changing pull request action and return
    /// GitHub's fresh PR state.
    pub fn github_pr_action(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
        action: GithubPrAction,
    ) -> Pending<GithubWorkItem> {
        let call = self.call::<GithubWorkItem>(
            "git_github_pr_action",
            json!({ "cwd": cwd, "repo": repo, "number": number, "action": action.as_str() }),
        );
        let client = self.clone();
        let repo = repo.to_string();
        self.spawn_pending(async move {
            let item = call.await?;
            {
                let mut state = client.state();
                state.github.work_item_by_key.insert(
                    details_cache_key(&repo, WorkItemKind::Pr, number),
                    item.clone(),
                );
                let wanted = js::trim(&repo).to_lowercase();
                client.update_cached_list(&mut state, |cached| {
                    (cached.provider == InboxProvider::Github
                        && cached.kind == InboxKind::Pr
                        && cached.repo.to_lowercase() == wanted
                        && cached.number == number)
                        .then(|| cached.merge_github(&item))
                });
            }
            client.record_inbox_self_activity(InboxSelfActivityTarget::work_item(
                InboxProvider::Github,
                InboxKind::Pr,
                &repo,
                number,
            ));
            Ok(item)
        })
    }

    /// `peekGithubPrDiff`.
    pub fn peek_github_pr_diff(
        &self,
        repo: &str,
        number: i64,
        full_context: bool,
    ) -> Option<PrDiff> {
        self.state()
            .github
            .pr_diff_by_key
            .get(&pr_diff_cache_key(repo, number, full_context))
            .cloned()
    }

    /// `githubPrDiff`. With `max_age_ms`, a diff fetched that recently
    /// answers instead.
    pub fn github_pr_diff(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
        full_context: bool,
        max_age_ms: Option<i64>,
    ) -> Pending<PrDiff> {
        let key = pr_diff_cache_key(repo, number, full_context);
        let mut state = self.state();
        if let Some(cached) = state.github.pr_diff_by_key.get(&key)
            && state
                .github
                .fresh_enough(&format!("diff:{key}"), max_age_ms, self.now())
        {
            return ready(Ok(cached.clone()));
        }
        if let Some(pending) = state.github.pr_diff_inflight.get(&key) {
            return pending;
        }
        let id = state.next_id();
        let call = self.call::<PrDiff>(
            "git_github_pr_diff",
            json!({ "cwd": cwd, "repo": repo, "number": number, "fullContext": full_context }),
        );
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = call.await;
            let mut state = client.state();
            if let Ok(diff) = &result {
                state
                    .github
                    .pr_diff_by_key
                    .insert(cache_key.clone(), diff.clone());
                state
                    .github
                    .fetched_at
                    .insert(format!("diff:{cache_key}"), client.now());
            }
            state.github.pr_diff_inflight.finish(&cache_key, id);
            result
        });
        state
            .github
            .pr_diff_inflight
            .insert(key, id, pending.clone());
        pending
    }

    /// `prefetchGithubWorkItem`: start a fetch of everything the linked side
    /// panel reads that is not cached yet, so opening the panel from a
    /// session card can render from cache instead of waiting on `gh`.
    /// Requests settle on their own, so dropping the results is fine, and
    /// errors stay quiet.
    pub fn prefetch_github_work_item(
        &self,
        cwd: &str,
        repo: &str,
        kind: WorkItemKind,
        number: i64,
    ) {
        if self.peek_github_work_item(repo, kind, number).is_none() {
            drop(self.github_work_item(cwd, repo, kind, number, false));
        }
        if self
            .peek_github_work_item_details(repo, kind, number)
            .is_none()
        {
            drop(self.github_work_item_details(cwd, repo, kind, number, None));
        }
        if self
            .peek_github_work_item_thread(repo, kind, number)
            .is_none()
        {
            drop(self.github_work_item_thread(cwd, repo, kind, number, false, None));
        }
        if kind == WorkItemKind::Pr && self.peek_github_pr_diff(repo, number, false).is_none() {
            drop(self.github_pr_diff(cwd, repo, number, false, None));
        }
    }
}

/// `gitlabWorkItemToInboxItem` and `azureDevOpsWorkItemToInboxItem`.
pub fn repository_inbox_item(
    provider: InboxProvider,
    item: &RepositoryWorkItem,
    project_path: &str,
    repo: &str,
) -> InboxItem {
    InboxItem {
        kind: super::types::inbox_kind(item.kind),
        number: item.number,
        title: item.title.clone(),
        url: item.url.clone(),
        state: item.state.clone(),
        state_reason: None,
        created_at: None,
        updated_at: item.updated_at.clone(),
        labels: item.labels.clone(),
        assignees: item.assignees.clone(),
        draft: item.draft,
        repo: if item.repo.is_empty() {
            repo.to_string()
        } else {
            item.repo.clone()
        },
        project_path: project_path.to_string(),
        provider,
        id: None,
        identifier: None,
        team_id: None,
        team_name: None,
        project_id: None,
        project_name: None,
        state_type: None,
        attention_reason: Some(item.attention_reason.clone()),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    use futures::FutureExt;
    use gpui::TestAppContext;
    use monocode_settings::Kv;
    use serde_json::Value;

    use super::{InboxClient, Pending};
    use crate::inbox::backend::fake::FakeBackend;

    /// A client over a scripted backend and an in-memory `Kv`.
    pub fn client(
        cx: &TestAppContext,
        handler: impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static,
    ) -> (InboxClient, Arc<FakeBackend>) {
        let backend = FakeBackend::new(handler);
        let client = InboxClient::new(backend.clone(), Kv::in_memory(), cx.executor());
        (client, backend)
    }

    /// Run every task, then read the settled result.
    pub fn settle<T: Clone>(cx: &mut TestAppContext, pending: Pending<T>) -> Result<T, String> {
        cx.run_until_parked();
        pending.now_or_never().expect("the request settled")
    }

    /// Run a lazy future on the executor until it settles, then read it.
    pub fn settle_future<T: Send + 'static>(
        cx: &mut TestAppContext,
        future: impl std::future::Future<Output = T> + Send + 'static,
    ) -> T {
        let task = cx.executor().spawn(future);
        cx.run_until_parked();
        task.now_or_never().expect("the future settled")
    }

    /// `Unexpected command` for a handler that does not know `command`.
    pub fn unexpected(command: &str) -> Result<Value, String> {
        Err(format!("Unexpected command: {command}"))
    }
}
