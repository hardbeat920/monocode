//! The file system and git data the file views read and change.
//!
//! The React views imported the files model (`fileTree.ts`, `fileIndex.ts`,
//! `fileWatch.ts`) and the Tauri fs wrappers directly. Here they reach both
//! through [`FilesData`]. The engine's workspace package implements it over
//! its `Files` global (`FileTree`, `FileIndex`, `FileWatch`, `GitSignal`).
//!
//! Methods that only do IO have default bodies that call monocode-git on the
//! background executor, the way the engine's `LocalFs` does, so an
//! implementation only has to supply the stateful model methods.
//! [`LocalFiles`] supplies those too, for the gallery and for hosts that run
//! the views without the engine.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext as _, AsyncApp, Subscription, Task};
use monocode_git::fs::{GitChangedFile, GitDiffIndex, GitFileDiff};
use serde::{Deserialize, Serialize};

use crate::fuzzy::score_path;
use crate::paths::{looks_like_project, parent_path};

/// A data call that finishes later. Errors are the messages the UI shows.
pub type DataTask<T> = Task<Result<T, String>>;

/// A change listener. Implementations call it after their own update, so it
/// may read or update the data again.
pub type Listener = Box<dyn Fn(&mut App)>;

/// `FsEntry`: one child of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub ignored: bool,
}

impl FsEntry {
    pub fn file(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            is_dir: false,
            ignored: false,
        }
    }

    pub fn dir(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            is_dir: true,
            ..Self::file(name, path)
        }
    }
}

/// `ProjectFile`: one entry of the project file index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub name: String,
    pub path: String,
    pub relative: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_dir: Option<bool>,
}

impl ProjectFile {
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        relative: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            relative: relative.into(),
            is_dir: None,
        }
    }
}

/// `RankedFile`: a file with its fuzzy score and the matched positions in
/// `relative` (UTF-16 code units).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedFile {
    pub file: ProjectFile,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `FileOpenOptions` from src/features/search/model/search.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileOpenOptions {
    /// The caller got this path from the file system or the file index.
    pub exact: bool,
    /// Open as a permanent tab instead of the pane's preview tab.
    pub pin: bool,
}

impl FileOpenOptions {
    pub const EXACT: Self = Self {
        exact: true,
        pin: false,
    };
    pub const PINNED: Self = Self {
        exact: true,
        pin: true,
    };
}

/// `EditorNavigationTarget`: a 1-based line and column to reveal in `path`.
/// A new `token` asks for the same place again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorNavigation {
    pub path: String,
    pub line: usize,
    pub column: Option<usize>,
    pub token: u64,
}

/// `GitStatusMap` from src/features/source-control/hooks/useGitFileStatuses.ts:
/// the git status of changed files, and the strongest status under each
/// folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatusMap {
    pub files: HashMap<String, String>,
    pub dirs: HashMap<String, String>,
}

/// `STATUS_PRIORITY`.
fn status_priority(status: &str) -> u8 {
    match status {
        "modified" => 3,
        "deleted" => 2,
        "added" | "untracked" => 1,
        _ => 0,
    }
}

impl GitStatusMap {
    /// `buildStatusMaps`.
    pub fn from_changed_files(files: &[GitChangedFile], cwd: &str) -> Self {
        let mut map = Self::default();
        for file in files {
            map.files.insert(file.path.clone(), file.status.clone());
            let priority = status_priority(&file.status);
            if priority == 0 {
                continue;
            }
            let mut dir = parent_path(&file.path);
            while dir.len() > cwd.len() {
                let existing = map
                    .dirs
                    .get(&dir)
                    .map_or(0, |status| status_priority(status));
                if priority > existing {
                    map.dirs.insert(dir.clone(), file.status.clone());
                } else {
                    break;
                }
                dir = parent_path(&dir);
            }
        }
        map
    }
}

/// Everything the file views read and change. See the module docs.
pub trait FilesData: 'static {
    /// Format a buffer and return its new text and byte cursor offset.
    fn format_text(&self, _path: &str, _source: &str, _cursor: usize) -> Option<(String, usize)> {
        None
    }

    // The explorer cache: src/features/files/model/fileTree.ts.

    /// `peekDir`: the cached listing, so a remounted tree draws at once.
    fn peek_dir(&self, path: &str, cx: &App) -> Option<Vec<FsEntry>>;
    /// `listCachedDir`.
    fn list_cached_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>>;
    /// `refreshDir`.
    fn refresh_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>>;
    /// `forgetDir`: drop a folder and everything cached under it.
    fn forget_dir(&self, path: &str, cx: &mut App);
    /// `loadExpanded`: the project's expanded folders, the root by default.
    fn load_expanded(&self, cwd: &str, cx: &App) -> HashSet<String>;
    fn save_expanded(&self, cwd: &str, expanded: HashSet<String>, cx: &mut App);
    fn load_selected(&self, cwd: &str, cx: &App) -> Option<String>;
    fn save_selected(&self, cwd: &str, path: Option<String>, cx: &mut App);
    /// `notifyDirsChanged`: re-list the cache after an agent or shell write.
    fn notify_dirs_changed(&self, cx: &mut App);
    /// `subscribeDirsChanged`. Dropping the subscription unsubscribes.
    fn subscribe_dirs_changed(&self, listener: Listener, cx: &mut App) -> Subscription;

    /// `createParentOf`: the folder to create into, given the selection.
    fn create_parent_of(&self, cwd: &str, selected_path: Option<&str>, cx: &App) -> String {
        let Some(selected) = selected_path.filter(|selected| *selected != cwd) else {
            return cwd.to_string();
        };
        let parent = parent_path(selected);
        let entry = self
            .peek_dir(&parent, cx)
            .and_then(|entries| entries.into_iter().find(|entry| entry.path == selected));
        match entry {
            Some(entry) if entry.is_dir => selected.to_string(),
            Some(_) => parent,
            None if self.peek_dir(selected, cx).is_some() => selected.to_string(),
            None => parent,
        }
    }

    // The project file index: src/features/files/model/fileIndex.ts.

    /// `peekProjectFiles`.
    fn peek_project_files(&self, cwd: &str, cx: &App) -> Option<Arc<Vec<ProjectFile>>>;
    /// `loadProjectFiles`. `refresh` starts a new scan.
    fn load_project_files(
        &self,
        cwd: &str,
        refresh: bool,
        cx: &mut App,
    ) -> DataTask<Arc<Vec<ProjectFile>>>;
    fn recent_opened_files(&self, cwd: &str, cx: &App) -> Vec<String>;
    fn remember_opened_file(&self, cwd: &str, path: &str, cx: &mut App);
    /// `rankProjectFiles`. The default is a port of the TypeScript; the
    /// engine passes its own `file_index::rank_project_files`.
    fn rank_project_files(
        &self,
        files: &[ProjectFile],
        query: &str,
        recents: &[String],
    ) -> Vec<RankedFile> {
        rank_project_files(files, query, recents, MAX_RESULTS)
    }

    // Open-file watching (fileWatch.ts) and the git change signal.

    /// `watchFile`: `listener` runs when the file changes on disk.
    fn watch_file(&self, path: &str, listener: Listener, cx: &mut App) -> Subscription;
    /// `syncWatchedMtime`: take the current mtime as the baseline after our
    /// own write, so it does not read as an outside change.
    fn sync_watched_mtime(&self, _path: &str, _cx: &mut App) {}
    /// `subscribeGitChanged`.
    fn subscribe_git_changed(&self, _listener: Listener, _cx: &mut App) -> Subscription {
        Subscription::new(|| {})
    }
    /// `notifyGitChanged`.
    fn notify_git_changed(&self, _cx: &mut App) {}

    // File operations from src/platform/tauri/fs.ts, through monocode-git.

    fn create_path(
        &self,
        parent: &str,
        name: &str,
        is_dir: bool,
        cx: &mut App,
    ) -> DataTask<String> {
        let (parent, name) = (parent.to_string(), name.to_string());
        cx.background_spawn(async move { monocode_git::fs::create_path(parent, name, is_dir) })
    }

    fn rename_path(&self, path: &str, name: &str, cx: &mut App) -> DataTask<String> {
        let (path, name) = (path.to_string(), name.to_string());
        cx.background_spawn(async move { monocode_git::fs::rename_path(path, name) })
    }

    fn delete_path(&self, path: &str, cx: &mut App) -> DataTask<()> {
        let path = path.to_string();
        cx.background_spawn(async move { monocode_git::fs::delete_path(path) })
    }

    fn copy_path(&self, from: &str, dest_parent: &str, cx: &mut App) -> DataTask<String> {
        let (from, dest) = (from.to_string(), dest_parent.to_string());
        cx.background_spawn(async move { monocode_git::fs::copy_path(from, dest) })
    }

    fn move_path(&self, from: &str, dest_parent: &str, cx: &mut App) -> DataTask<String> {
        let (from, dest) = (from.to_string(), dest_parent.to_string());
        cx.background_spawn(async move { monocode_git::fs::move_path(from, dest) })
    }

    /// Reveal in Finder, File Explorer, or the file manager.
    fn reveal_path(&self, path: &str, cx: &mut App) -> DataTask<()> {
        let path = path.to_string();
        cx.background_spawn(async move { monocode_git::fs::reveal_path(path) })
    }

    /// File paths a file manager put on the system clipboard.
    fn clipboard_file_paths(&self, _cx: &mut App) -> DataTask<Vec<String>> {
        Task::ready(monocode_platform::pasteboard::clipboard_file_paths())
    }

    fn read_text_file(&self, path: &str, cx: &mut App) -> DataTask<String> {
        let path = path.to_string();
        cx.background_spawn(async move { monocode_git::fs::read_text_file(path) })
    }

    fn write_text_file(&self, path: &str, content: String, cx: &mut App) -> DataTask<()> {
        let path = path.to_string();
        cx.background_spawn(async move { monocode_git::fs::write_text_file(path, content) })
    }

    fn read_binary_file(&self, path: &str, cx: &mut App) -> DataTask<Vec<u8>> {
        let path = path.to_string();
        cx.background_spawn(async move { monocode_git::fs::read_binary_file(path) })
    }

    /// `gitDiffFiles`: changed files without branch metadata.
    fn git_diff_files(&self, cwd: &str, cx: &mut App) -> DataTask<GitDiffIndex> {
        let cwd = cwd.to_string();
        cx.background_spawn(async move { Ok(monocode_git::fs::git_diff_files(cwd)) })
    }

    /// `gitFileDiff`: HEAD against the index (`staged`) or the index
    /// against the working tree.
    fn git_file_diff(
        &self,
        cwd: &str,
        relative: &str,
        staged: bool,
        cx: &mut App,
    ) -> DataTask<GitFileDiff> {
        let (cwd, relative) = (cwd.to_string(), relative.to_string());
        cx.background_spawn(async move { monocode_git::fs::git_file_diff(cwd, relative, staged) })
    }

    /// `gitStageContents`: write `contents` to the index for `relative`.
    fn git_stage_contents(
        &self,
        cwd: &str,
        relative: &str,
        contents: String,
        cx: &mut App,
    ) -> DataTask<()> {
        let (cwd, relative) = (cwd.to_string(), relative.to_string());
        cx.background_spawn(
            async move { monocode_git::fs::git_stage_contents(cwd, relative, contents) },
        )
    }
}

/// `MAX_RECENTS` in fileIndex.ts.
pub const MAX_RECENTS: usize = 30;
/// `MAX_RESULTS` in fileIndex.ts.
pub const MAX_RESULTS: usize = 80;

/// `rankProjectFiles`: recents without a query, fuzzy matches with one.
pub fn rank_project_files(
    files: &[ProjectFile],
    query: &str,
    recents: &[String],
    limit: usize,
) -> Vec<RankedFile> {
    let recent_rank: HashMap<&str, usize> = recents
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_str(), index))
        .collect();

    if monocode_core::js::trim(query).is_empty() {
        let by_path: HashMap<&str, &ProjectFile> = files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for path in recents {
            if !seen.insert(path.as_str()) {
                continue;
            }
            let Some(file) = by_path.get(path.as_str()) else {
                continue;
            };
            out.push(RankedFile {
                file: (*file).clone(),
                score: 0,
                positions: Vec::new(),
            });
            if out.len() >= limit {
                break;
            }
        }
        return out;
    }

    let mut scored: Vec<RankedFile> = files
        .iter()
        .filter_map(|file| {
            let hit = score_path(query, &file.relative, &file.name)?;
            let recency = recent_rank
                .get(file.path.as_str())
                .map_or(0, |recency| (MAX_RECENTS as i64 - *recency as i64) * 8);
            Some(RankedFile {
                file: file.clone(),
                score: hit.score + recency,
                positions: hit.positions,
            })
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| {
                monocode_core::js::len(&a.file.relative)
                    .cmp(&monocode_core::js::len(&b.file.relative))
            })
            .then_with(|| locale_compare(&a.file.relative, &b.file.relative))
    });
    scored.truncate(limit);
    scored
}

/// `String.prototype.localeCompare` for paths, close to ICU's root order.
fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let folded = a.to_lowercase().cmp(&b.to_lowercase());
    if folded != std::cmp::Ordering::Equal {
        return folded;
    }
    b.cmp(a)
}

/// Reads monocode-git's serialize-only results back into our types.
fn reshape<T: Serialize, U: for<'de> Deserialize<'de>>(value: T) -> Result<U, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|err| err.to_string())
}

/// Lists one folder for [`ExplorerCache`].
pub type DirLister = Rc<dyn Fn(String, &mut App) -> DataTask<Vec<FsEntry>>>;

/// `REFRESH_MS` in fileTree.ts.
pub const DIRS_REFRESH_DELAY: Duration = Duration::from_millis(150);

/// Change listeners by subscription id.
type Listeners = Vec<(u64, Rc<dyn Fn(&mut App)>)>;

#[derive(Default)]
struct CacheState {
    expanded_by_project: HashMap<String, HashSet<String>>,
    selected_by_project: HashMap<String, Option<String>>,
    dirs: HashMap<String, Vec<FsEntry>>,
    listeners: Listeners,
    next_listener: u64,
    refresh_timer: Option<Task<()>>,
    refreshing: bool,
    refresh_again: bool,
}

/// A standalone port of the explorer cache in fileTree.ts, for
/// [`LocalFiles`] and for test doubles. The engine has its own `FileTree`
/// entity and does not use this.
#[derive(Clone)]
pub struct ExplorerCache {
    state: Rc<RefCell<CacheState>>,
    lister: DirLister,
}

impl ExplorerCache {
    pub fn new(lister: DirLister) -> Self {
        Self {
            state: Rc::default(),
            lister,
        }
    }

    /// Lists folders through monocode-git on the background executor.
    pub fn local() -> Self {
        Self::new(Rc::new(|path, cx: &mut App| {
            cx.background_spawn(async move { reshape(monocode_git::fs::list_dir(path)?) })
        }))
    }

    pub fn peek_dir(&self, path: &str) -> Option<Vec<FsEntry>> {
        self.state.borrow().dirs.get(path).cloned()
    }

    pub fn list_cached_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>> {
        if let Some(hit) = self.peek_dir(path) {
            return Task::ready(Ok(hit));
        }
        let listing = (self.lister)(path.to_string(), cx);
        let state = Rc::downgrade(&self.state);
        let path = path.to_string();
        cx.spawn(async move |_: &mut AsyncApp| {
            let entries = listing.await?;
            if let Some(state) = state.upgrade() {
                state.borrow_mut().dirs.insert(path, entries.clone());
            }
            Ok(entries)
        })
    }

    pub fn refresh_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>> {
        self.state.borrow_mut().dirs.remove(path);
        self.list_cached_dir(path, cx)
    }

    pub fn forget_dir(&self, path: &str) {
        let prefix = format!("{path}/");
        self.state
            .borrow_mut()
            .dirs
            .retain(|key, _| key != path && !key.starts_with(&prefix));
    }

    pub fn load_expanded(&self, cwd: &str) -> HashSet<String> {
        self.state
            .borrow()
            .expanded_by_project
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| HashSet::from([cwd.to_string()]))
    }

    pub fn save_expanded(&self, cwd: &str, expanded: HashSet<String>) {
        self.state
            .borrow_mut()
            .expanded_by_project
            .insert(cwd.to_string(), expanded);
    }

    pub fn load_selected(&self, cwd: &str) -> Option<String> {
        self.state
            .borrow()
            .selected_by_project
            .get(cwd)
            .cloned()
            .flatten()
    }

    pub fn save_selected(&self, cwd: &str, path: Option<String>) {
        self.state
            .borrow_mut()
            .selected_by_project
            .insert(cwd.to_string(), path);
    }

    pub fn subscribe(&self, listener: Listener) -> Subscription {
        let id = {
            let mut state = self.state.borrow_mut();
            state.next_listener += 1;
            let id = state.next_listener;
            state.listeners.push((id, Rc::from(listener)));
            id
        };
        let state = Rc::downgrade(&self.state);
        Subscription::new(move || {
            if let Some(state) = state.upgrade() {
                state
                    .borrow_mut()
                    .listeners
                    .retain(|(entry, _)| *entry != id);
            }
        })
    }

    /// `refreshCachedDirs`: list every cached folder again. A folder that
    /// fails to list is forgotten.
    pub fn refresh_cached_dirs(&self, cx: &mut App) -> Task<()> {
        let paths: Vec<String> = self.state.borrow().dirs.keys().cloned().collect();
        let refreshes: Vec<_> = paths
            .into_iter()
            .map(|path| {
                let task = self.refresh_dir(&path, cx);
                (path, task)
            })
            .collect();
        let this = self.clone();
        cx.spawn(async move |_: &mut AsyncApp| {
            for (path, task) in refreshes {
                if task.await.is_err() {
                    this.forget_dir(&path);
                }
            }
        })
    }

    /// `notifyDirsChanged`, debounced by [`DIRS_REFRESH_DELAY`].
    pub fn notify_dirs_changed(&self, cx: &mut App) {
        schedule_refresh(Rc::downgrade(&self.state), self.lister.clone(), cx);
    }
}

fn schedule_refresh(state: Weak<RefCell<CacheState>>, lister: DirLister, cx: &mut App) {
    let Some(strong) = state.upgrade() else {
        return;
    };
    if strong.borrow().refresh_timer.is_some() {
        return;
    }
    let timer = cx.background_executor().timer(DIRS_REFRESH_DELAY);
    let weak = state.clone();
    let task = cx.spawn(async move |cx: &mut AsyncApp| {
        timer.await;
        cx.update(|cx| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().refresh_timer = None;
                run_refresh(ExplorerCache { state, lister }, cx);
            }
        });
    });
    strong.borrow_mut().refresh_timer = Some(task);
}

fn run_refresh(cache: ExplorerCache, cx: &mut App) {
    {
        let mut state = cache.state.borrow_mut();
        if state.refreshing {
            state.refresh_again = true;
            return;
        }
        state.refreshing = true;
    }
    let refresh = cache.refresh_cached_dirs(cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        refresh.await;
        cx.update(|cx| {
            let listeners: Vec<_> = cache
                .state
                .borrow()
                .listeners
                .iter()
                .map(|(_, listener)| listener.clone())
                .collect();
            for listener in listeners {
                listener(cx);
            }
            let again = {
                let mut state = cache.state.borrow_mut();
                state.refreshing = false;
                std::mem::take(&mut state.refresh_again)
            };
            if again {
                schedule_refresh(Rc::downgrade(&cache.state), cache.lister.clone(), cx);
            }
        });
    })
    .detach();
}

/// How often [`LocalFiles`] polls the mtimes of watched files.
pub const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(1_000);

#[derive(Default)]
struct LocalState {
    project_files: HashMap<String, Arc<Vec<ProjectFile>>>,
    recents: HashMap<String, Vec<String>>,
    watchers: HashMap<String, Listeners>,
    mtimes: HashMap<String, Option<i64>>,
    next_watcher: u64,
    poll: Option<Task<()>>,
    git_listeners: Listeners,
}

/// `FileMtime` from monocode-git.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMtime {
    path: String,
    mtime_ms: Option<i64>,
}

/// A standalone [`FilesData`] on the local disk: the explorer cache, an
/// in-memory project index and recents list, and an mtime poll for watched
/// files. The app uses the engine's implementation instead.
#[derive(Clone)]
pub struct LocalFiles {
    explorer: ExplorerCache,
    state: Rc<RefCell<LocalState>>,
}

impl Default for LocalFiles {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalFiles {
    pub fn new() -> Self {
        Self {
            explorer: ExplorerCache::local(),
            state: Rc::default(),
        }
    }

    pub fn explorer(&self) -> &ExplorerCache {
        &self.explorer
    }

    fn ensure_poll(&self, cx: &mut App) {
        if self.state.borrow().poll.is_some() {
            return;
        }
        let state = Rc::downgrade(&self.state);
        let task = cx.spawn(async move |cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(WATCH_POLL_INTERVAL).await;
                let Some(strong) = state.upgrade() else {
                    return;
                };
                let paths: Vec<String> = strong.borrow().watchers.keys().cloned().collect();
                drop(strong);
                if paths.is_empty() {
                    if let Some(strong) = state.upgrade() {
                        strong.borrow_mut().poll = None;
                    }
                    return;
                }
                let stats = cx
                    .background_spawn(async move { stat(paths) })
                    .await
                    .unwrap_or_default();
                let state = state.clone();
                cx.update(|cx| {
                    let Some(strong) = state.upgrade() else {
                        return;
                    };
                    let mut fire = Vec::new();
                    {
                        let mut local = strong.borrow_mut();
                        for stat in stats {
                            let previous = local.mtimes.insert(stat.path.clone(), stat.mtime_ms);
                            if previous.is_some_and(|previous| previous != stat.mtime_ms)
                                && let Some(listeners) = local.watchers.get(&stat.path)
                            {
                                fire.extend(listeners.iter().map(|(_, listener)| listener.clone()));
                            }
                        }
                    }
                    for listener in fire {
                        listener(cx);
                    }
                });
            }
        });
        self.state.borrow_mut().poll = Some(task);
    }
}

fn stat(paths: Vec<String>) -> Result<Vec<FileMtime>, String> {
    reshape(monocode_git::fs::stat_files(paths)?)
}

impl FilesData for LocalFiles {
    fn peek_dir(&self, path: &str, _: &App) -> Option<Vec<FsEntry>> {
        self.explorer.peek_dir(path)
    }

    fn list_cached_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>> {
        self.explorer.list_cached_dir(path, cx)
    }

    fn refresh_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>> {
        self.explorer.refresh_dir(path, cx)
    }

    fn forget_dir(&self, path: &str, _: &mut App) {
        self.explorer.forget_dir(path);
    }

    fn load_expanded(&self, cwd: &str, _: &App) -> HashSet<String> {
        self.explorer.load_expanded(cwd)
    }

    fn save_expanded(&self, cwd: &str, expanded: HashSet<String>, _: &mut App) {
        self.explorer.save_expanded(cwd, expanded);
    }

    fn load_selected(&self, cwd: &str, _: &App) -> Option<String> {
        self.explorer.load_selected(cwd)
    }

    fn save_selected(&self, cwd: &str, path: Option<String>, _: &mut App) {
        self.explorer.save_selected(cwd, path);
    }

    fn notify_dirs_changed(&self, cx: &mut App) {
        self.explorer.notify_dirs_changed(cx);
    }

    fn subscribe_dirs_changed(&self, listener: Listener, _: &mut App) -> Subscription {
        self.explorer.subscribe(listener)
    }

    fn peek_project_files(&self, cwd: &str, _: &App) -> Option<Arc<Vec<ProjectFile>>> {
        self.state.borrow().project_files.get(cwd).cloned()
    }

    fn load_project_files(
        &self,
        cwd: &str,
        refresh: bool,
        cx: &mut App,
    ) -> DataTask<Arc<Vec<ProjectFile>>> {
        if !looks_like_project(cwd) {
            return Task::ready(Ok(Arc::new(Vec::new())));
        }
        if !refresh && let Some(files) = self.peek_project_files(cwd, cx) {
            return Task::ready(Ok(files));
        }
        let owned = cwd.to_string();
        let scan = cx.background_spawn(async move {
            monocode_git::fs::list_project_files(owned).map(|files| {
                files
                    .into_iter()
                    .map(|file| ProjectFile::new(file.name, file.path, file.relative))
                    .collect::<Vec<_>>()
            })
        });
        let state = Rc::downgrade(&self.state);
        let cwd = cwd.to_string();
        cx.spawn(async move |_: &mut AsyncApp| {
            let files = Arc::new(scan.await?);
            if let Some(state) = state.upgrade() {
                state.borrow_mut().project_files.insert(cwd, files.clone());
            }
            Ok(files)
        })
    }

    fn recent_opened_files(&self, cwd: &str, _: &App) -> Vec<String> {
        self.state
            .borrow()
            .recents
            .get(cwd)
            .cloned()
            .unwrap_or_default()
    }

    fn remember_opened_file(&self, cwd: &str, path: &str, _: &mut App) {
        let mut state = self.state.borrow_mut();
        let recents = state.recents.entry(cwd.to_string()).or_default();
        recents.retain(|item| item != path);
        recents.insert(0, path.to_string());
        recents.truncate(MAX_RECENTS);
    }

    fn watch_file(&self, path: &str, listener: Listener, cx: &mut App) -> Subscription {
        let id = {
            let mut state = self.state.borrow_mut();
            state.next_watcher += 1;
            let id = state.next_watcher;
            state
                .watchers
                .entry(path.to_string())
                .or_default()
                .push((id, Rc::from(listener)));
            id
        };
        self.ensure_poll(cx);
        let state = Rc::downgrade(&self.state);
        let path = path.to_string();
        Subscription::new(move || {
            let Some(state) = state.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            let empty = state.watchers.get_mut(&path).is_some_and(|listeners| {
                listeners.retain(|(entry, _)| *entry != id);
                listeners.is_empty()
            });
            if empty {
                state.watchers.remove(&path);
                state.mtimes.remove(&path);
            }
        })
    }

    fn sync_watched_mtime(&self, path: &str, cx: &mut App) {
        let state = Rc::downgrade(&self.state);
        let owned = path.to_string();
        let job = cx.background_spawn(async move { stat(vec![owned]) });
        cx.spawn(async move |_: &mut AsyncApp| {
            if let (Ok(stats), Some(state)) = (job.await, state.upgrade()) {
                let mut state = state.borrow_mut();
                for stat in stats {
                    if state.watchers.contains_key(&stat.path) {
                        state.mtimes.insert(stat.path, stat.mtime_ms);
                    }
                }
            }
        })
        .detach();
    }

    fn subscribe_git_changed(&self, listener: Listener, _: &mut App) -> Subscription {
        let id = {
            let mut state = self.state.borrow_mut();
            state.next_watcher += 1;
            let id = state.next_watcher;
            state.git_listeners.push((id, Rc::from(listener)));
            id
        };
        let state = Rc::downgrade(&self.state);
        Subscription::new(move || {
            if let Some(state) = state.upgrade() {
                state
                    .borrow_mut()
                    .git_listeners
                    .retain(|(entry, _)| *entry != id);
            }
        })
    }

    fn notify_git_changed(&self, cx: &mut App) {
        let listeners: Vec<_> = self
            .state
            .borrow()
            .git_listeners
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect();
        cx.defer(move |cx| {
            for listener in listeners {
                listener(cx);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(path: &str, status: &str) -> GitChangedFile {
        GitChangedFile {
            path: path.into(),
            relative: path.trim_start_matches("/p/").into(),
            status: status.into(),
            additions: 0,
            deletions: 0,
            staged: false,
            unstaged: true,
        }
    }

    #[test]
    fn folders_take_the_strongest_status_below_them() {
        let map = GitStatusMap::from_changed_files(
            &[
                changed("/p/src/a/new.ts", "untracked"),
                changed("/p/src/b.ts", "modified"),
                changed("/p/README.md", "deleted"),
            ],
            "/p",
        );
        assert_eq!(map.files["/p/src/b.ts"], "modified");
        assert_eq!(map.dirs["/p/src"], "modified");
        assert_eq!(map.dirs["/p/src/a"], "untracked");
        assert!(!map.dirs.contains_key("/p"));
    }

    #[test]
    fn ranks_recents_without_a_query_and_names_first_with_one() {
        let files = vec![
            ProjectFile::new("App.tsx", "/r/src/App.tsx", "src/App.tsx"),
            ProjectFile::new("main.rs", "/r/app/main.rs", "app/main.rs"),
        ];
        let recents = vec!["/r/app/main.rs".to_string(), "/r/gone.ts".to_string()];
        let ranked = rank_project_files(&files, " ", &recents, MAX_RESULTS);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].file.name, "main.rs");

        let ranked = rank_project_files(&files, "app", &[], MAX_RESULTS);
        assert_eq!(ranked[0].file.name, "App.tsx");
        assert_eq!(ranked[0].positions, vec![4, 5, 6]);
    }
}
