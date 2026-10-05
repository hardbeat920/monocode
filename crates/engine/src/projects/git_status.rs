//! Git status per project folder. Port of the shared stores in
//! src/features/source-control/hooks (useGitFileStatuses,
//! useProjectBranches, useProjectDiffStats, useProjectWorktrees) and the
//! changes panel's diff index poll (`useDiffIndex` in GitChangesPanel.tsx).
//!
//! `GitStatuses` keeps one `GitStatus` entity per folder for the life of
//! the app, as the TypeScript module maps did. A view asks for what it
//! shows with `GitStatus::watch`; the returned `GitWatch` keeps that part
//! loading until it is dropped (the hooks' subscribe and unsubscribe).
//! Every git call runs on the background executor.
//!
//! Refresh triggers match the TypeScript:
//! - Window focus and becoming visible reload what is watched (diff stats
//!   only after `DIFF_STATS_RESUME_TTL_MS`).
//! - `git_changed` (notifyGitChanged) reloads statuses, branches, diff
//!   stats, worktrees, and the index. Calls in one effect cycle coalesce.
//! - `dirs_changed` (notifyDirsChanged) reloads the file statuses.
//! - The diff index polls every `GIT_POLL` while watched and visible.
//!
//! The file statuses and the diff index come from the same `git_diff_index`
//! call, so a folder watched by both runs one call per trigger.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AppContext, Context, Entity, Task};
use monocode_core::js;
use monocode_core::paths::slash;
use monocode_git::fs::{GitBranches, GitDiffIndex, GitDiffStats};

use super::Clock;
use super::backend::{ProjectsBackend, Worktrees};
use crate::runtime::engine::Engine;

/// `GIT_POLL_MS`: the changes panel's poll.
pub const GIT_POLL: Duration = Duration::from_millis(2000);
/// `RESUME_TTL_MS`: diff stats younger than this skip a focus reload.
pub const DIFF_STATS_RESUME_TTL_MS: i64 = 30_000;

/// `GitStatusMap`: each changed file's status, and each parent folder's
/// strongest status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

/// `trimSlash`.
fn trim_slash(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_string()
    }
}

fn is_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `parentPath` from src/shared/lib/paths.ts.
pub fn parent_path(path: &str) -> String {
    let trimmed = trim_slash(path);
    // `//server/share` is a UNC root.
    if let Some(rest) = trimmed.strip_prefix("//") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && parts.iter().all(|part| !part.is_empty()) {
            return trimmed;
        }
    }
    if is_drive(&trimmed) {
        return format!("{trimmed}/");
    }
    let Some(index) = trimmed.rfind('/').filter(|index| *index > 0) else {
        return "/".into();
    };
    let parent = &trimmed[..index];
    if is_drive(parent) {
        return format!("{parent}/");
    }
    parent.to_string()
}

/// `buildStatusMaps`.
pub fn build_status_maps(index: &GitDiffIndex, cwd: &str) -> GitStatusMap {
    let mut map = GitStatusMap::default();
    let cwd_len = js::len(cwd);
    for file in &index.files {
        map.files.insert(file.path.clone(), file.status.clone());
        let priority = status_priority(&file.status);
        if priority == 0 {
            continue;
        }
        let mut dir = parent_path(&file.path);
        while js::len(&dir) > cwd_len {
            let existing = map.dirs.get(&dir).map(|status| status_priority(status));
            if priority > existing.unwrap_or(0) {
                map.dirs.insert(dir.clone(), file.status.clone());
            } else {
                break;
            }
            dir = parent_path(&dir);
        }
    }
    map
}

/// `sameIndex`: everything but the absolute file paths.
pub fn same_index(prev: Option<&GitDiffIndex>, next: &GitDiffIndex) -> bool {
    let Some(prev) = prev else {
        return false;
    };
    if prev.branch != next.branch
        || prev.head != next.head
        || prev.additions != next.additions
        || prev.deletions != next.deletions
        || prev.files.len() != next.files.len()
        || prev.remote != next.remote
        || prev.upstream != next.upstream
        || prev.default_branch != next.default_branch
        || prev.ahead != next.ahead
        || prev.behind != next.behind
        || prev.ahead_of_default != next.ahead_of_default
        || prev.head_pushed != next.head_pushed
    {
        return false;
    }
    prev.files.iter().zip(&next.files).all(|(file, other)| {
        file.relative == other.relative
            && file.status == other.status
            && file.additions == other.additions
            && file.deletions == other.deletions
            && file.staged == other.staged
            && file.unstaged == other.unstaged
    })
}

/// `changedFilePaths`: files whose row changed, appeared, or went away.
pub fn changed_file_paths(prev: &GitDiffIndex, next: &GitDiffIndex) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let previous: HashMap<&str, _> = prev
        .files
        .iter()
        .map(|file| (file.relative.as_str(), file))
        .collect();
    let current: std::collections::HashSet<&str> = next
        .files
        .iter()
        .map(|file| file.relative.as_str())
        .collect();
    for file in &next.files {
        let changed = previous.get(file.relative.as_str()).is_none_or(|before| {
            before.status != file.status
                || before.additions != file.additions
                || before.deletions != file.deletions
                || before.staged != file.staged
                || before.unstaged != file.unstaged
        });
        if changed {
            paths.push(file.path.clone());
            seen.insert(file.path.clone());
        }
    }
    for file in &prev.files {
        if !current.contains(file.relative.as_str()) && !seen.contains(&file.path) {
            paths.push(file.path.clone());
        }
    }
    paths
}

/// `ProjectBranchesState`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectBranchesState {
    pub branches: Option<GitBranches>,
    /// The first lookup for this folder finished, repo or not.
    pub settled: bool,
}

/// The `useProjectWorktrees` snapshot: the last good list, and the last
/// error beside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreesSnapshot {
    pub data: Option<Worktrees>,
    pub error: Option<String>,
}

/// The parts of a folder's git status a view can watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatchKind {
    /// `useGitFileStatuses`.
    FileStatuses,
    /// The changes panel's diff index, polled every `GIT_POLL`.
    Index,
    /// `useProjectBranchesState`.
    Branches,
    /// `useProjectDiffStats`.
    DiffStats,
    /// `useProjectWorktrees`.
    Worktrees,
}

impl WatchKind {
    fn slot(self) -> usize {
        match self {
            WatchKind::FileStatuses => 0,
            WatchKind::Index => 1,
            WatchKind::Branches => 2,
            WatchKind::DiffStats => 3,
            WatchKind::Worktrees => 4,
        }
    }
}

type Watchers = Rc<RefCell<[usize; 5]>>;

/// Keeps one part of a folder's git status loading. Drop it to stop.
pub struct GitWatch {
    watchers: Watchers,
    kind: WatchKind,
}

impl Drop for GitWatch {
    fn drop(&mut self) {
        let mut counts = self.watchers.borrow_mut();
        let slot = &mut counts[self.kind.slot()];
        *slot = slot.saturating_sub(1);
    }
}

/// One project folder's git status.
pub struct GitStatus {
    cwd: String,
    backend: Arc<dyn ProjectsBackend>,
    clock: Clock,
    hidden: bool,
    watchers: Watchers,

    index: Option<GitDiffIndex>,
    statuses: GitStatusMap,
    index_in_flight: bool,
    index_pending: bool,
    index_reloads: u64,
    index_task: Option<Task<()>>,
    poll: Option<Task<()>>,

    branches: ProjectBranchesState,
    branches_in_flight: bool,
    branches_task: Option<Task<()>>,

    stats: Option<GitDiffStats>,
    stats_in_flight: bool,
    stats_pending: bool,
    stats_epoch: u64,
    stats_loaded_at: i64,
    stats_task: Option<Task<()>>,

    worktrees: WorktreesSnapshot,
    worktrees_load: Option<Shared<Task<bool>>>,
    worktrees_invalidated: bool,
}

impl GitStatus {
    pub fn new(cwd: &str, backend: Arc<dyn ProjectsBackend>, clock: Clock, hidden: bool) -> Self {
        Self {
            cwd: cwd.to_string(),
            backend,
            clock,
            hidden,
            watchers: Rc::new(RefCell::new([0; 5])),
            index: None,
            statuses: GitStatusMap::default(),
            index_in_flight: false,
            index_pending: false,
            index_reloads: 0,
            index_task: None,
            poll: None,
            branches: ProjectBranchesState::default(),
            branches_in_flight: false,
            branches_task: None,
            stats: None,
            stats_in_flight: false,
            stats_pending: false,
            stats_epoch: 0,
            stats_loaded_at: 0,
            stats_task: None,
            worktrees: WorktreesSnapshot::default(),
            worktrees_load: None,
            worktrees_invalidated: false,
        }
    }

    // Reading.

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The changed files and their folders (`useGitFileStatuses`).
    pub fn file_statuses(&self) -> &GitStatusMap {
        &self.statuses
    }

    /// The last diff index: branch, head, upstream, ahead and behind, and
    /// changed files with line counts.
    pub fn index(&self) -> Option<&GitDiffIndex> {
        self.index.as_ref()
    }

    /// `useProjectBranchesState`.
    pub fn branches_state(&self) -> &ProjectBranchesState {
        &self.branches
    }

    /// `useProjectBranches`.
    pub fn branches(&self) -> Option<&GitBranches> {
        self.branches.branches.as_ref()
    }

    /// `useProjectDiffStats`.
    pub fn diff_stats(&self) -> Option<&GitDiffStats> {
        self.stats.as_ref()
    }

    /// `useProjectWorktrees`.
    pub fn worktrees(&self) -> &WorktreesSnapshot {
        &self.worktrees
    }

    /// How many watchers a part has.
    pub fn watcher_count(&self, kind: WatchKind) -> usize {
        self.watchers.borrow()[kind.slot()]
    }

    fn watched(&self, kind: WatchKind) -> bool {
        self.watcher_count(kind) > 0
    }

    // Watching.

    /// Keep `kind` loading until the returned guard drops. The first watcher
    /// starts it, as the hooks' first subscriber did.
    pub fn watch(&mut self, kind: WatchKind, cx: &mut Context<Self>) -> GitWatch {
        let first = {
            let mut counts = self.watchers.borrow_mut();
            counts[kind.slot()] += 1;
            counts[kind.slot()] == 1
        };
        if first {
            self.start(kind, cx);
        }
        GitWatch {
            watchers: self.watchers.clone(),
            kind,
        }
    }

    fn start(&mut self, kind: WatchKind, cx: &mut Context<Self>) {
        match kind {
            WatchKind::FileStatuses => self.load_index(true, cx),
            WatchKind::Index => {
                let force = self.index_reloads > 0;
                self.load_index(force, cx);
                self.start_poll(cx);
            }
            WatchKind::Branches => self.load_branches(true, cx),
            WatchKind::DiffStats => {
                if !self.stats_in_flight && self.stats_stale() {
                    self.load_stats(true, cx);
                }
            }
            WatchKind::Worktrees => {
                drop(self.load_worktrees(false, cx));
            }
        }
    }

    fn start_poll(&mut self, cx: &mut Context<Self>) {
        if self.poll.is_some() {
            return;
        }
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                let timer = cx.background_executor().timer(GIT_POLL);
                timer.await;
                let Some(this) = this.upgrade() else {
                    break;
                };
                let keep = cx.update(|cx| {
                    this.update(cx, |this, cx| {
                        if !this.watched(WatchKind::Index) {
                            this.poll = None;
                            return false;
                        }
                        if !this.hidden {
                            this.load_index(false, cx);
                        }
                        true
                    })
                });
                if !keep {
                    break;
                }
            }
        }));
    }

    // Triggers.

    /// Window focus, or the window became visible (`onResume`).
    pub fn resume(&mut self, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        if self.watched(WatchKind::FileStatuses) {
            self.load_index(true, cx);
        } else if self.watched(WatchKind::Index) {
            self.load_index(false, cx);
        }
        if self.watched(WatchKind::Branches) {
            self.load_branches(true, cx);
        }
        if self.watched(WatchKind::DiffStats) && !self.stats_in_flight && self.stats_stale() {
            self.load_stats(true, cx);
        }
        if self.watched(WatchKind::Worktrees) {
            drop(self.load_worktrees(false, cx));
        }
    }

    /// `notifyGitChanged` reached this folder.
    pub fn git_changed(&mut self, cx: &mut Context<Self>) {
        if !self.hidden {
            if self.watched(WatchKind::FileStatuses) {
                self.load_index(true, cx);
            } else if self.watched(WatchKind::Index) {
                self.load_index(false, cx);
            }
            if self.watched(WatchKind::Branches) {
                self.load_branches(true, cx);
            }
            if self.watched(WatchKind::Worktrees) {
                drop(self.load_worktrees(true, cx));
            }
        }
        // Diff stats reload on a git change even while hidden.
        if self.watched(WatchKind::DiffStats) {
            self.load_stats(true, cx);
        }
    }

    /// `notifyDirsChanged` reached this folder.
    pub fn dirs_changed(&mut self, cx: &mut Context<Self>) {
        if !self.hidden && self.watched(WatchKind::FileStatuses) {
            self.load_index(true, cx);
        }
    }

    /// `document.hidden` changed.
    pub fn set_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        if !hidden {
            self.resume(cx);
        }
    }

    // The diff index and file statuses.

    /// `reload` in the changes panel: load now, even while hidden.
    pub fn reload_index(&mut self, cx: &mut Context<Self>) {
        self.index_reloads += 1;
        self.load_index(true, cx);
    }

    fn load_index(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.index_in_flight {
            self.index_pending = true;
            return;
        }
        if !force && self.hidden {
            return;
        }
        self.index_in_flight = true;
        let backend = self.backend.clone();
        let cwd = self.cwd.clone();
        let read = cx.background_spawn(async move { backend.git_diff_index(&cwd) });
        self.index_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            cx.update(|cx| {
                this.update(cx, |this, cx| this.index_loaded(result, cx));
            });
        }));
    }

    fn index_loaded(&mut self, result: Result<GitDiffIndex, String>, cx: &mut Context<Self>) {
        let mut changed = false;
        match result {
            Ok(next) => {
                let statuses = build_status_maps(&next, &self.cwd);
                if statuses != self.statuses {
                    self.statuses = statuses;
                    changed = true;
                }
                if self.watched(WatchKind::Index) && !same_index(self.index.as_ref(), &next) {
                    let prev = self.index.replace(next.clone());
                    changed = true;
                    self.apply_diff_stats(
                        GitDiffStats {
                            files: next.files.len() as i64,
                            additions: next.additions,
                            deletions: next.deletions,
                        },
                        cx,
                    );
                    if let Some(prev) = prev {
                        let paths = changed_file_paths(&prev, &next);
                        cx.defer(move |cx| {
                            Engine::hooks(cx)
                                .workspace
                                .invalidate_watched_files(Some(&paths), cx);
                            super::notify_git_changed(cx);
                        });
                    }
                }
            }
            Err(_) => {
                if self.statuses != GitStatusMap::default() {
                    self.statuses = GitStatusMap::default();
                    changed = true;
                }
                if self.watched(WatchKind::Index) && self.index.is_some() {
                    self.index = None;
                    changed = true;
                }
            }
        }
        self.index_in_flight = false;
        if changed {
            cx.notify();
        }
        if self.index_pending {
            self.index_pending = false;
            self.load_index(true, cx);
        }
    }

    // Branches.

    /// `seedProjectBranches`: show a list another window already loaded.
    pub fn seed_branches(&mut self, branches: GitBranches, cx: &mut Context<Self>) {
        self.publish_branches(Some(branches), cx);
    }

    fn publish_branches(&mut self, branches: Option<GitBranches>, cx: &mut Context<Self>) {
        // `settled` still flips on a lookup that found nothing, so an
        // unchanged `None` is only a no-op once the first one has landed.
        if self.branches.settled && self.branches.branches == branches {
            return;
        }
        self.branches = ProjectBranchesState {
            branches,
            settled: true,
        };
        cx.notify();
    }

    fn load_branches(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.branches_in_flight || (!force && self.hidden) {
            return;
        }
        self.branches_in_flight = true;
        let backend = self.backend.clone();
        let cwd = self.cwd.clone();
        let read = cx.background_spawn(async move { backend.git_branches(&cwd) });
        self.branches_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            cx.update(|cx| {
                this.update(cx, |this, cx| {
                    this.branches_in_flight = false;
                    this.publish_branches(result.ok(), cx);
                });
            });
        }));
    }

    // Diff stats.

    fn stats_stale(&self) -> bool {
        (self.clock)() - self.stats_loaded_at >= DIFF_STATS_RESUME_TTL_MS
    }

    fn publish_stats(&mut self, stats: Option<GitDiffStats>, cx: &mut Context<Self>) {
        if self.stats == stats {
            return;
        }
        self.stats = stats;
        cx.notify();
    }

    /// `applyProjectDiffStats`: stats from a fuller index, so the title bar
    /// badge cannot lag behind. A load already running is discarded.
    pub fn apply_diff_stats(&mut self, stats: GitDiffStats, cx: &mut Context<Self>) {
        self.stats_epoch += 1;
        self.stats_loaded_at = (self.clock)();
        self.publish_stats(Some(stats), cx);
    }

    fn load_stats(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.stats_in_flight {
            self.stats_pending = true;
            return;
        }
        if !force && self.hidden {
            return;
        }
        self.stats_in_flight = true;
        let epoch = self.stats_epoch;
        let backend = self.backend.clone();
        let cwd = self.cwd.clone();
        let read = cx.background_spawn(async move { backend.git_diff_stats(&cwd) });
        self.stats_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            cx.update(|cx| {
                this.update(cx, |this, cx| {
                    if epoch == this.stats_epoch {
                        this.stats_loaded_at = (this.clock)();
                        this.publish_stats(result.ok(), cx);
                    }
                    this.stats_in_flight = false;
                    if this.stats_pending {
                        this.stats_pending = false;
                        this.load_stats(true, cx);
                    }
                });
            });
        }));
    }

    // Worktrees.

    /// `refresh` from `useProjectWorktrees`: reload, with a follow-up if a
    /// read is already running. Resolves to whether the list loaded.
    pub fn refresh_worktrees(&mut self, cx: &mut Context<Self>) -> Shared<Task<bool>> {
        self.load_worktrees(true, cx)
    }

    fn load_worktrees(&mut self, invalidated: bool, cx: &mut Context<Self>) -> Shared<Task<bool>> {
        if let Some(in_flight) = &self.worktrees_load {
            // A git change during a read needs a follow-up; focus events can
            // share the read already running.
            self.worktrees_invalidated |= invalidated;
            return in_flight.clone();
        }
        let backend = self.backend.clone();
        let cwd = self.cwd.clone();
        let read = cx.background_spawn(async move { backend.git_worktrees(&cwd) });
        let load = cx
            .spawn(async move |this, cx| {
                let result = read.await;
                let Some(this) = this.upgrade() else {
                    return false;
                };
                let next = cx.update(|cx| {
                    this.update(cx, |this, cx| {
                        let loaded = match result {
                            Ok(data) => {
                                this.worktrees = WorktreesSnapshot {
                                    data: Some(data),
                                    error: None,
                                };
                                true
                            }
                            Err(error) => {
                                // A background failure keeps the usable list;
                                // the error shows beside it.
                                this.worktrees.error = Some(error);
                                false
                            }
                        };
                        cx.notify();
                        this.worktrees_load = None;
                        if this.worktrees_invalidated {
                            this.worktrees_invalidated = false;
                            return Err(this.load_worktrees(false, cx));
                        }
                        Ok(loaded)
                    })
                });
                match next {
                    Ok(loaded) => loaded,
                    Err(follow_up) => follow_up.await,
                }
            })
            .shared();
        self.worktrees_load = Some(load.clone());
        load
    }
}

/// Every folder's `GitStatus`, and the triggers that reach all of them.
pub struct GitStatuses {
    backend: Arc<dyn ProjectsBackend>,
    clock: Clock,
    hidden: bool,
    entries: HashMap<String, Entity<GitStatus>>,
    git_changed_scheduled: bool,
}

impl GitStatuses {
    pub fn new(backend: Arc<dyn ProjectsBackend>, clock: Clock) -> Self {
        Self {
            backend,
            clock,
            hidden: false,
            entries: HashMap::new(),
            git_changed_scheduled: false,
        }
    }

    /// `entryFor`: the folder's entity, created on first use.
    pub fn status(&mut self, cwd: &str, cx: &mut Context<Self>) -> Entity<GitStatus> {
        if let Some(entry) = self.entries.get(cwd) {
            return entry.clone();
        }
        let (backend, clock, hidden) = (self.backend.clone(), self.clock.clone(), self.hidden);
        let entry = cx.new(|_| GitStatus::new(cwd, backend, clock, hidden));
        self.entries.insert(cwd.to_string(), entry.clone());
        entry
    }

    /// The folder's entity, if anything asked for it yet.
    pub fn get(&self, cwd: &str) -> Option<Entity<GitStatus>> {
        self.entries.get(cwd).cloned()
    }

    fn each(&self, cx: &mut App, mut f: impl FnMut(&mut GitStatus, &mut Context<GitStatus>)) {
        let entries: Vec<Entity<GitStatus>> = self.entries.values().cloned().collect();
        for entry in entries {
            entry.update(cx, |status, cx| f(status, cx));
        }
    }

    /// `notifyGitChanged`. Calls within one effect cycle reload once.
    pub fn git_changed(&mut self, cx: &mut Context<Self>) {
        if self.git_changed_scheduled {
            return;
        }
        self.git_changed_scheduled = true;
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let Some(this) = this.upgrade() else {
                return;
            };
            let entries = this.update(cx, |this, _| {
                this.git_changed_scheduled = false;
                this.entries.values().cloned().collect::<Vec<_>>()
            });
            for entry in entries {
                entry.update(cx, |status, cx| status.git_changed(cx));
            }
        });
    }

    /// `notifyDirsChanged`.
    pub fn dirs_changed(&mut self, cx: &mut Context<Self>) {
        self.each(cx, |status, cx| status.dirs_changed(cx));
    }

    /// A window took focus.
    pub fn window_focused(&mut self, cx: &mut Context<Self>) {
        self.each(cx, |status, cx| status.resume(cx));
    }

    /// `document.hidden` changed for the app.
    pub fn set_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        self.each(cx, |status, cx| status.set_hidden(hidden, cx));
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// `applyProjectDiffStats` for a folder, from a fuller index elsewhere.
    pub fn apply_diff_stats(&mut self, cwd: &str, stats: GitDiffStats, cx: &mut Context<Self>) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let entry = self.status(cwd, cx);
        entry.update(cx, |status, cx| status.apply_diff_stats(stats, cx));
    }

    /// `seedProjectBranches`.
    pub fn seed_branches(&mut self, cwd: &str, branches: GitBranches, cx: &mut Context<Self>) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let entry = self.status(cwd, cx);
        entry.update(cx, |status, cx| status.seed_branches(branches, cx));
    }
}

/// Whether a folder can have git status at all (`enabled && cwd && cwd !==
/// "~"`).
pub fn git_status_enabled(cwd: &str) -> bool {
    !cwd.is_empty() && cwd != "~"
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_git::fs::GitChangedFile;

    fn changed(path: &str, relative: &str, status: &str) -> GitChangedFile {
        GitChangedFile {
            path: path.into(),
            relative: relative.into(),
            status: status.into(),
            additions: 1,
            deletions: 0,
            staged: false,
            unstaged: true,
        }
    }

    #[test]
    fn folders_take_their_strongest_child_status() {
        let index = GitDiffIndex {
            files: vec![
                changed("/repo/src/a/new.ts", "src/a/new.ts", "untracked"),
                changed("/repo/src/a/old.ts", "src/a/old.ts", "modified"),
                changed("/repo/src/b.ts", "src/b.ts", "deleted"),
                changed("/repo/top.ts", "top.ts", "renamed"),
            ],
            ..GitDiffIndex::default()
        };
        let map = build_status_maps(&index, "/repo");
        assert_eq!(map.files.len(), 4);
        assert_eq!(
            map.dirs.get("/repo/src/a").map(String::as_str),
            Some("modified")
        );
        assert_eq!(
            map.dirs.get("/repo/src").map(String::as_str),
            Some("modified")
        );
        assert!(!map.dirs.contains_key("/repo"));
    }

    #[test]
    fn parent_paths_follow_the_typescript_helper() {
        assert_eq!(parent_path("/repo/src/a.ts"), "/repo/src");
        assert_eq!(parent_path("/repo"), "/");
        assert_eq!(parent_path("C:/repo"), "C:/");
        assert_eq!(parent_path("C:"), "C:/");
        assert_eq!(parent_path("//server/share"), "//server/share");
        assert_eq!(parent_path("//server/share/x"), "//server/share");
    }

    #[test]
    fn same_index_ignores_absolute_paths_only() {
        let prev = GitDiffIndex {
            files: vec![changed("/a/x.ts", "x.ts", "modified")],
            ..GitDiffIndex::default()
        };
        let moved = GitDiffIndex {
            files: vec![changed("/b/x.ts", "x.ts", "modified")],
            ..GitDiffIndex::default()
        };
        assert!(same_index(Some(&prev), &moved));
        assert!(!same_index(None, &moved));
        let ahead = GitDiffIndex {
            ahead: 1,
            ..moved.clone()
        };
        assert!(!same_index(Some(&prev), &ahead));
    }

    #[test]
    fn changed_paths_cover_new_changed_and_gone_files() {
        let prev = GitDiffIndex {
            files: vec![
                changed("/r/keep.ts", "keep.ts", "modified"),
                changed("/r/gone.ts", "gone.ts", "modified"),
                changed("/r/edit.ts", "edit.ts", "modified"),
            ],
            ..GitDiffIndex::default()
        };
        let mut edit = changed("/r/edit.ts", "edit.ts", "modified");
        edit.additions = 9;
        let next = GitDiffIndex {
            files: vec![
                changed("/r/keep.ts", "keep.ts", "modified"),
                edit,
                changed("/r/new.ts", "new.ts", "untracked"),
            ],
            ..GitDiffIndex::default()
        };
        assert_eq!(
            changed_file_paths(&prev, &next),
            ["/r/edit.ts", "/r/new.ts", "/r/gone.ts"]
        );
    }
}
