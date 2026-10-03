//! Port of src/features/files/model/fileTree.ts: the explorer's folder
//! cache, its expanded and selected rows per project, and the debounced
//! re-list after an agent or shell writes.
//!
//! `FileTree` emits `DirsChanged` where the TypeScript called the
//! `subscribeDirsChanged` listeners.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use gpui::{Context, EventEmitter, Task};

use super::backend::{FsBackend, FsEntry};
use super::file_name::path_segments;
use crate::workspace::paths::{join_path, parent_path};

/// `REFRESH_MS`.
pub const REFRESH_DELAY: Duration = Duration::from_millis(150);

/// The cached folders were listed again after a change on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirsChanged;

/// The explorer cache.
pub struct FileTree {
    backend: Arc<dyn FsBackend>,
    expanded_by_project: HashMap<String, HashSet<String>>,
    selected_by_project: HashMap<String, Option<String>>,
    dirs: HashMap<String, Vec<FsEntry>>,
    refresh_timer: Option<Task<()>>,
    refreshing: bool,
    refresh_again: bool,
    /// `document.hidden`.
    hidden: bool,
}

impl EventEmitter<DirsChanged> for FileTree {}

impl FileTree {
    pub fn new(backend: Arc<dyn FsBackend>) -> Self {
        Self {
            backend,
            expanded_by_project: HashMap::new(),
            selected_by_project: HashMap::new(),
            dirs: HashMap::new(),
            refresh_timer: None,
            refreshing: false,
            refresh_again: false,
            hidden: false,
        }
    }

    /// `loadExpanded`: the project's expanded folders, the root by default.
    pub fn load_expanded(&self, cwd: &str) -> HashSet<String> {
        self.expanded_by_project
            .get(cwd)
            .cloned()
            .unwrap_or_else(|| HashSet::from([cwd.to_string()]))
    }

    /// `saveExpanded`.
    pub fn save_expanded(&mut self, cwd: &str, expanded: HashSet<String>) {
        self.expanded_by_project.insert(cwd.to_string(), expanded);
    }

    /// `loadSelected`.
    pub fn load_selected(&self, cwd: &str) -> Option<String> {
        self.selected_by_project.get(cwd).cloned().flatten()
    }

    /// `saveSelected`.
    pub fn save_selected(&mut self, cwd: &str, path: Option<String>) {
        self.selected_by_project.insert(cwd.to_string(), path);
    }

    /// `peekDir`: the cached listing, so a remounted tree draws at once.
    pub fn peek_dir(&self, path: &str) -> Option<&[FsEntry]> {
        self.dirs.get(path).map(Vec::as_slice)
    }

    /// `listCachedDir`.
    pub fn list_cached_dir(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<FsEntry>, String>> {
        if let Some(hit) = self.dirs.get(path) {
            return Task::ready(Ok(hit.clone()));
        }
        let listing = cx
            .background_executor()
            .spawn(self.backend.list_dir(path.to_string()));
        let path = path.to_string();
        cx.spawn(async move |this, cx| {
            let entries = listing.await?;
            this.update(cx, |this, cx| {
                this.dirs.insert(path, entries.clone());
                cx.notify();
            })
            .ok();
            Ok(entries)
        })
    }

    /// `refreshDir`.
    pub fn refresh_dir(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<FsEntry>, String>> {
        self.dirs.remove(path);
        self.list_cached_dir(path, cx)
    }

    /// `forgetDir`: drop a folder and everything cached under it.
    pub fn forget_dir(&mut self, path: &str) {
        let prefix = format!("{path}/");
        self.dirs
            .retain(|key, _| key != path && !key.starts_with(&prefix));
    }

    /// `refreshCachedDirs`: list every cached folder again. A folder that
    /// fails to list is forgotten.
    pub fn refresh_cached_dirs(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let paths: Vec<String> = self.dirs.keys().cloned().collect();
        if paths.is_empty() {
            return Task::ready(());
        }
        let refreshes: Vec<_> = paths
            .into_iter()
            .map(|path| {
                let refresh = self.refresh_dir(&path, cx);
                (path, refresh)
            })
            .collect();
        cx.spawn(async move |this, cx| {
            let results = join_all(
                refreshes
                    .into_iter()
                    .map(async |(path, refresh)| (path, refresh.await)),
            )
            .await;
            this.update(cx, |this, _| {
                for (path, result) in results {
                    if result.is_err() {
                        this.forget_dir(&path);
                    }
                }
            })
            .ok();
        })
    }

    /// `notifyDirsChanged`: re-list the explorer cache after an agent or
    /// shell write, debounced.
    pub fn notify_dirs_changed(&mut self, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        self.schedule_refresh(cx);
    }

    /// `document.hidden` changed.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_timer.is_some() {
            return;
        }
        let timer = cx.background_executor().timer(REFRESH_DELAY);
        self.refresh_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| {
                this.refresh_timer = None;
                this.run_refresh(cx);
            })
            .ok();
        }));
    }

    fn run_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            self.refresh_again = true;
            return;
        }
        self.refreshing = true;
        let refresh = self.refresh_cached_dirs(cx);
        cx.spawn(async move |this, cx| {
            refresh.await;
            this.update(cx, |this, cx| {
                cx.emit(DirsChanged);
                this.refreshing = false;
                if this.refresh_again {
                    this.refresh_again = false;
                    this.schedule_refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// `createParentOf`: the folder to create into, given the explorer
    /// selection.
    pub fn create_parent_of(&self, cwd: &str, selected_path: Option<&str>) -> String {
        let Some(selected) = selected_path.filter(|selected| *selected != cwd) else {
            return cwd.to_string();
        };
        let parent = parent_path(selected);
        let entry = self
            .peek_dir(&parent)
            .and_then(|entries| entries.iter().find(|entry| entry.path == selected));
        match entry {
            Some(entry) if entry.is_dir => selected.to_string(),
            Some(_) => parent,
            None if self.peek_dir(selected).is_some() => selected.to_string(),
            None => parent,
        }
    }
}

/// `dirsTouchedByCreate`: folders whose children change when creating
/// `name` under `parent`.
pub fn dirs_touched_by_create(parent: &str, name: &str) -> Vec<String> {
    let segments = path_segments(name);
    let mut out = vec![parent.to_string()];
    let mut cur = parent.to_string();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        cur = join_path(&cur, segment);
        out.push(cur.clone());
    }
    out
}

/// `dirsTouchedByMove`.
pub fn dirs_touched_by_move(from: &str, to: &str) -> Vec<String> {
    let from_parent = parent_path(from);
    let to_parent = parent_path(to);
    if from_parent == to_parent {
        vec![from_parent]
    } else {
        vec![from_parent, to_parent]
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::super::backend::fake::FakeFs;
    use super::*;
    use futures::FutureExt;
    use gpui::{AppContext, Entity, TestAppContext};

    const ROOT: &str = "/tmp/empty-project";

    fn entry(name: &str) -> FsEntry {
        FsEntry {
            name: name.into(),
            path: format!("{ROOT}/{name}"),
            is_dir: false,
            ignored: false,
        }
    }

    fn setup(cx: &mut TestAppContext) -> (Arc<FakeFs>, Entity<FileTree>) {
        let fs = FakeFs::new();
        let tree = cx.new(|_| FileTree::new(fs.clone()));
        (fs, tree)
    }

    fn list(tree: &Entity<FileTree>, refresh: bool, cx: &mut TestAppContext) -> Vec<FsEntry> {
        let task = tree.update(cx, |tree, cx| {
            if refresh {
                tree.refresh_dir(ROOT, cx)
            } else {
                tree.list_cached_dir(ROOT, cx)
            }
        });
        cx.run_until_parked();
        task.now_or_never().expect("listed").unwrap()
    }

    fn peek(tree: &Entity<FileTree>, cx: &mut TestAppContext) -> Option<Vec<FsEntry>> {
        tree.read_with(cx, |tree, _| tree.peek_dir(ROOT).map(<[FsEntry]>::to_vec))
    }

    #[gpui::test]
    fn keeps_the_first_listing_until_refresh_dir(cx: &mut TestAppContext) {
        let (fs, tree) = setup(cx);
        fs.push_dir(ROOT, vec![]);
        list(&tree, false, cx);
        assert_eq!(peek(&tree, cx), Some(vec![]));

        fs.push_dir(ROOT, vec![entry("hello.ts")]);
        assert_eq!(list(&tree, false, cx), vec![]);
        assert_eq!(fs.dir_calls().len(), 1);

        assert_eq!(list(&tree, true, cx), vec![entry("hello.ts")]);
        assert_eq!(peek(&tree, cx), Some(vec![entry("hello.ts")]));
    }

    #[gpui::test]
    fn refresh_cached_dirs_re_lists_every_cached_folder(cx: &mut TestAppContext) {
        let (fs, tree) = setup(cx);
        fs.push_dir(ROOT, vec![]);
        list(&tree, false, cx);
        fs.push_dir(ROOT, vec![entry("created.ts")]);
        let refresh = tree.update(cx, |tree, cx| tree.refresh_cached_dirs(cx));
        cx.run_until_parked();
        refresh.now_or_never().expect("refreshed");
        assert_eq!(peek(&tree, cx), Some(vec![entry("created.ts")]));
    }

    #[gpui::test]
    fn notify_dirs_changed_refreshes_the_cache_and_tells_listeners(cx: &mut TestAppContext) {
        let (fs, tree) = setup(cx);
        fs.push_dir(ROOT, vec![]);
        list(&tree, false, cx);

        let changed = Rc::new(Cell::new(0));
        let count = changed.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&tree, move |_, _: &DirsChanged, _| {
                count.set(count.get() + 1)
            })
        });
        fs.push_dir(ROOT, vec![entry("from-agent.ts")]);
        tree.update(cx, |tree, cx| tree.notify_dirs_changed(cx));
        cx.run_until_parked();
        assert_eq!(peek(&tree, cx), Some(vec![]));

        cx.executor().advance_clock(REFRESH_DELAY);
        cx.run_until_parked();
        assert_eq!(peek(&tree, cx), Some(vec![entry("from-agent.ts")]));
        assert_eq!(changed.get(), 1);
    }

    #[gpui::test]
    fn forgets_a_folder_that_no_longer_lists(cx: &mut TestAppContext) {
        let (fs, tree) = setup(cx);
        fs.push_dir(ROOT, vec![entry("a.ts")]);
        list(&tree, false, cx);
        tree.update(cx, |tree, _| tree.forget_dir(ROOT));
        assert_eq!(peek(&tree, cx), None);
    }

    #[gpui::test]
    fn picks_the_folder_to_create_into(cx: &mut TestAppContext) {
        let (fs, tree) = setup(cx);
        fs.push_dir(
            ROOT,
            vec![
                entry("a.ts"),
                FsEntry {
                    name: "src".into(),
                    path: format!("{ROOT}/src"),
                    is_dir: true,
                    ignored: false,
                },
            ],
        );
        list(&tree, false, cx);
        tree.read_with(cx, |tree, _| {
            assert_eq!(tree.create_parent_of(ROOT, None), ROOT);
            assert_eq!(
                tree.create_parent_of(ROOT, Some(&format!("{ROOT}/a.ts"))),
                ROOT
            );
            assert_eq!(
                tree.create_parent_of(ROOT, Some(&format!("{ROOT}/src"))),
                format!("{ROOT}/src")
            );
            assert_eq!(tree.load_expanded(ROOT), HashSet::from([ROOT.to_string()]));
        });
    }

    #[test]
    fn lists_the_folders_a_create_or_move_touches() {
        assert_eq!(
            dirs_touched_by_create("/p", "a/b/c.ts"),
            vec!["/p".to_string(), "/p/a".into(), "/p/a/b".into()]
        );
        assert_eq!(
            dirs_touched_by_move("/p/a.ts", "/p/b.ts"),
            vec!["/p".to_string()]
        );
        assert_eq!(
            dirs_touched_by_move("/p/a.ts", "/q/a.ts"),
            vec!["/p".to_string(), "/q".into()]
        );
    }
}
