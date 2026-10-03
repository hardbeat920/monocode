//! Port of src/features/search/model/search.ts (project search, its
//! cancellation, and editor path helpers) and the `Search` entity: the state
//! and effects of src/features/search/ui/SearchView.tsx.
//!
//! A search runs 200 ms after the query or scope settles. Full-text session
//! search and project content search run off the UI thread with owner ids,
//! and a newer query or closing the page cancels both.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use gpui::{App, BackgroundExecutor, Context, Entity, Subscription, Task};
use monocode_core::js;
use monocode_core::paths::{path_key, slash};
use monocode_git::search::{SearchOptions, SearchResult};

use super::app_search::{
    AppSearchHit, FileRank, MessageSource, SearchScope, conversation_rows_from, flatten_grouped,
    group_hits, hits_from_content_matches, hits_from_file_ranks, hits_from_session_search,
    merge_hits, search_conversation_titles, search_recent_projects, search_session_messages,
};
use super::history::History;
use super::paths::is_local_project;
use crate::runtime::reducer::now_ms;
use crate::runtime::{Engine, StoreFuture};

/// The pause after typing before a search runs.
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(200);
/// How many file name matches the page ranks.
pub const FILE_HIT_LIMIT: usize = 40;

/// `EditorNavigation`: where to put the cursor in an opened file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorNavigation {
    pub line: i64,
    pub column: Option<i64>,
}

/// `FileOpenOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileOpenOptions {
    /// The caller got this concrete path from the filesystem or file index.
    pub exact: bool,
    /// Open as a permanent tab instead of the pane's preview tab.
    pub pin: bool,
}

/// `normalizeEditorPath`.
pub fn normalize_editor_path(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        path.to_string()
    } else {
        trimmed.to_string()
    }
}

/// `editorPathsEqual`.
pub fn editor_paths_equal(a: &str, b: &str) -> bool {
    path_key(a) == path_key(b)
}

/// The `search_project` and `cancel_project_search` commands.
pub trait ProjectSearchBackend: Send + Sync + 'static {
    /// `searchProject`.
    fn search(&self, options: SearchOptions) -> StoreFuture<SearchResult>;
    /// The `cancel_project_search` command.
    fn cancel(&self, cwd: String, search_id: String);
}

/// `ProjectSearchBackend` over `monocode-git`, on the background executor.
pub struct GitProjectSearch {
    executor: BackgroundExecutor,
}

impl GitProjectSearch {
    pub fn new(executor: BackgroundExecutor) -> Self {
        Self { executor }
    }
}

impl ProjectSearchBackend for GitProjectSearch {
    fn search(&self, options: SearchOptions) -> StoreFuture<SearchResult> {
        self.executor
            .spawn(async move { monocode_git::search::search_project(options) })
            .boxed()
    }

    fn cancel(&self, cwd: String, search_id: String) {
        monocode_git::search::cancel_project_search(cwd, search_id);
    }
}

/// `cancelProjectSearch`: remote hosts cannot cancel yet, and their paths
/// must never reach local search.
pub fn cancel_project_search(backend: &dyn ProjectSearchBackend, cwd: &str, search_id: &str) {
    if !is_local_project(cwd) {
        return;
    }
    backend.cancel(cwd.to_string(), search_id.to_string());
}

/// The project file index (`loadProjectFiles`, `peekProjectFiles`,
/// `rankProjectFiles`, `recentOpenedFiles`), which the workspace package
/// owns. The app installs it with `Search::set_files`.
pub trait SearchFiles {
    /// Rank the cached listing of `cwd` for `query`, with the project's
    /// recently opened files first on ties.
    fn rank(&self, _cwd: &str, _query: &str, _limit: usize, _cx: &App) -> Vec<FileRank> {
        Vec::new()
    }

    /// List the project. Search ranks again when the task finishes.
    fn load(&self, _cwd: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }
}

/// No file index: file name hits stay empty.
pub struct NoSearchFiles;

impl SearchFiles for NoSearchFiles {}

/// The search page.
pub struct Search {
    history: Entity<History>,
    project_search: Arc<dyn ProjectSearchBackend>,
    files: Rc<dyn SearchFiles>,
    open: bool,
    cwd: String,
    recents: Vec<String>,
    query: String,
    scope: SearchScope,
    active: usize,
    content_hits: Vec<AppSearchHit>,
    remote_hits: Vec<AppSearchHit>,
    content_truncated: bool,
    session_truncated: bool,
    loading: bool,
    error: Option<String>,
    /// The scheduled or running search. Dropping it stops both jobs.
    run: Option<Task<()>>,
    /// Owner ids of the jobs in flight, for cancellation.
    active_session_owner: Option<String>,
    active_project_search: Option<(String, String)>,
    files_load: Option<Task<()>>,
    _observers: Vec<Subscription>,
}

impl Search {
    /// A closed search page over `history`'s rows and the engine's sessions.
    pub fn new(
        history: Entity<History>,
        project_search: Arc<dyn ProjectSearchBackend>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observers = vec![
            cx.observe(&history, |_, _, cx| cx.notify()),
            cx.observe(&Engine::sessions(cx), |_, _, cx| cx.notify()),
        ];
        Self {
            history,
            project_search,
            files: Rc::new(NoSearchFiles),
            open: false,
            cwd: String::new(),
            recents: Vec::new(),
            query: String::new(),
            scope: SearchScope::All,
            active: 0,
            content_hits: Vec::new(),
            remote_hits: Vec::new(),
            content_truncated: false,
            session_truncated: false,
            loading: false,
            error: None,
            run: None,
            active_session_owner: None,
            active_project_search: None,
            files_load: None,
            _observers: observers,
        }
    }

    /// Install the project file index.
    pub fn set_files(&mut self, files: Rc<dyn SearchFiles>) {
        self.files = files;
    }

    // Reading.

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn scope(&self) -> SearchScope {
        self.scope
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Either search stopped at its result cap.
    pub fn truncated(&self) -> bool {
        self.content_truncated || self.session_truncated
    }

    /// The results in display order: conversation titles and open
    /// transcripts searched here, the store's full-text hits, file names,
    /// file contents, and recent projects.
    pub fn hits(&self, cx: &App) -> Vec<AppSearchHit> {
        let trimmed = js::trim(&self.query);
        if trimmed.is_empty() {
            return Vec::new();
        }
        let now = now_ms();
        let history = self.history.read(cx);
        let sessions = Engine::sessions(cx);
        let open = sessions.read(cx).all();
        let rows = conversation_rows_from(history.rows(), open, now);
        let titles: Vec<AppSearchHit> = search_conversation_titles(&rows, trimmed, now)
            .into_iter()
            .map(AppSearchHit::Conversation)
            .collect();
        let sources: Vec<MessageSource<'_>> = open
            .iter()
            .map(|session| MessageSource {
                id: &session.id,
                cwd: &session.cwd,
                harness: session.harness,
                title: &session.title,
                updated_at: now,
                blocks: &session.blocks,
            })
            .collect();
        let messages: Vec<AppSearchHit> = search_session_messages(&sources, trimmed, now)
            .into_iter()
            .map(AppSearchHit::Message)
            .collect();
        let files: Vec<AppSearchHit> = if is_local_project(&self.cwd) {
            hits_from_file_ranks(&self.files.rank(&self.cwd, trimmed, FILE_HIT_LIMIT, cx))
                .into_iter()
                .map(AppSearchHit::File)
                .collect()
        } else {
            Vec::new()
        };
        let projects: Vec<AppSearchHit> = search_recent_projects(&self.recents, trimmed)
            .into_iter()
            .map(AppSearchHit::Project)
            .collect();
        let merged = merge_hits(&[
            &titles,
            &messages,
            &self.remote_hits,
            &files,
            &self.content_hits,
            &projects,
        ]);
        flatten_grouped(&group_hits(&merged, self.scope))
    }

    /// The highlighted hit (`hits[active]`). When results shrink below the
    /// highlight, SearchView moved it back to the first hit.
    pub fn active_hit(&self, cx: &App) -> Option<AppSearchHit> {
        let hits = self.hits(cx);
        let index = if self.active < hits.len() {
            self.active
        } else {
            0
        };
        hits.into_iter().nth(index)
    }

    // Changing.

    /// Open the page for a project: a fresh query, the All scope, and the
    /// project's file listing.
    pub fn open(&mut self, cwd: &str, recents: Vec<String>, cx: &mut Context<Self>) {
        self.open = true;
        self.recents = recents;
        self.query.clear();
        self.scope = SearchScope::All;
        self.active = 0;
        self.content_hits.clear();
        self.remote_hits.clear();
        self.content_truncated = false;
        self.session_truncated = false;
        self.error = None;
        self.set_cwd(cwd, cx);
        self.schedule(cx);
    }

    /// Close the page and cancel any search in flight.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.files_load = None;
        self.schedule(cx);
    }

    /// The project the page searches. Loads its file listing while open.
    pub fn set_cwd(&mut self, cwd: &str, cx: &mut Context<Self>) {
        let changed = self.cwd != cwd;
        self.cwd = cwd.to_string();
        if self.open && (changed || self.files_load.is_none()) {
            self.files_load = None;
            if is_local_project(cwd) {
                let load = self.files.load(cwd, cx);
                self.files_load = Some(cx.spawn(async move |this, cx| {
                    load.await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                }));
            }
        }
        if changed {
            self.schedule(cx);
        }
        cx.notify();
    }

    /// Recent projects for the Projects section.
    pub fn set_recents(&mut self, recents: Vec<String>, cx: &mut Context<Self>) {
        self.recents = recents;
        cx.notify();
    }

    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed_changed = js::trim(&self.query) != js::trim(query);
        self.query = query.to_string();
        if trimmed_changed {
            self.active = 0;
            self.schedule(cx);
        }
        cx.notify();
    }

    pub fn set_scope(&mut self, scope: SearchScope, cx: &mut Context<Self>) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.active = 0;
        self.schedule(cx);
        cx.notify();
    }

    /// Move the highlight, wrapping at both ends.
    pub fn move_active(&mut self, delta: i64, cx: &mut Context<Self>) {
        let count = self.hits(cx).len() as i64;
        if count == 0 {
            return;
        }
        self.active = (self.active as i64 + delta).rem_euclid(count) as usize;
        cx.notify();
    }

    pub fn set_active(&mut self, index: usize, cx: &mut Context<Self>) {
        let count = self.hits(cx).len();
        self.active = if index >= count { 0 } else { index };
        cx.notify();
    }

    /// The effect cleanup: stop the scheduled search and cancel both jobs.
    fn cancel_jobs(&mut self, cx: &mut Context<Self>) {
        self.run = None;
        if let Some(owner) = self.active_session_owner.take() {
            Engine::writer(cx).cancel_session_search(&owner).detach();
        }
        if let Some((cwd, search_id)) = self.active_project_search.take() {
            cancel_project_search(self.project_search.as_ref(), &cwd, &search_id);
        }
    }

    /// The search effect: clear everything for an empty query or a closed
    /// page, otherwise search sessions and project files after the debounce.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        self.cancel_jobs(cx);
        let trimmed = js::trim(&self.query).to_string();
        if !self.open || trimmed.is_empty() {
            self.remote_hits.clear();
            self.content_hits.clear();
            self.session_truncated = false;
            self.content_truncated = false;
            self.loading = false;
            self.error = None;
            cx.notify();
            return;
        }
        let timer = cx.background_executor().timer(SEARCH_DEBOUNCE);
        self.run = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let Ok(jobs) = this.update(cx, |this, cx| this.start_jobs(&trimmed, cx)) else {
                return;
            };
            futures::future::join_all(jobs).await;
            this.update(cx, |this, cx| {
                this.loading = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Start the session and project searches the scope asks for. Each job
    /// applies its own result when it lands.
    fn start_jobs(&mut self, trimmed: &str, cx: &mut Context<Self>) -> Vec<Task<()>> {
        let want_sessions = matches!(self.scope, SearchScope::All | SearchScope::Conversations);
        let want_files = matches!(self.scope, SearchScope::All | SearchScope::Files);
        let mut jobs = Vec::new();
        if want_sessions {
            let owner = uuid::Uuid::new_v4().to_string();
            self.active_session_owner = Some(owner.clone());
            self.loading = true;
            let search = Engine::writer(cx).search_sessions(trimmed, &owner, None, false);
            jobs.push(cx.spawn(async move |this, cx| {
                let result = search.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(result) => {
                            this.remote_hits = hits_from_session_search(&result.hits, now_ms());
                            this.session_truncated = result.truncated;
                        }
                        Err(_) => {
                            this.remote_hits.clear();
                            this.session_truncated = false;
                        }
                    }
                    if this.active_session_owner.as_deref() == Some(owner.as_str()) {
                        this.active_session_owner = None;
                    }
                    cx.notify();
                })
                .ok();
            }));
        } else {
            self.remote_hits.clear();
            self.session_truncated = false;
        }
        if want_files && is_local_project(&self.cwd) {
            let search_id = uuid::Uuid::new_v4().to_string();
            self.active_project_search = Some((self.cwd.clone(), search_id.clone()));
            self.loading = true;
            let search = self.project_search.search(SearchOptions {
                cwd: self.cwd.clone(),
                query: trimmed.to_string(),
                case_sensitive: false,
                whole_word: false,
                regex: false,
                include: None,
                exclude: None,
                search_id: search_id.clone(),
            });
            jobs.push(cx.spawn(async move |this, cx| {
                let result = search.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(result) => {
                            this.content_hits = hits_from_content_matches(&result.matches)
                                .into_iter()
                                .map(AppSearchHit::Content)
                                .collect();
                            this.content_truncated = result.truncated;
                            this.error = None;
                        }
                        Err(error) => {
                            this.content_hits.clear();
                            this.content_truncated = false;
                            this.error = Some(error);
                        }
                    }
                    if this
                        .active_project_search
                        .as_ref()
                        .is_some_and(|(_, id)| *id == search_id)
                    {
                        this.active_project_search = None;
                    }
                    cx.notify();
                })
                .ok();
            }));
        } else {
            self.content_hits.clear();
            self.content_truncated = false;
        }
        cx.notify();
        jobs
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
