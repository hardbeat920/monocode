//! Port of src/features/files/model: the project file index, open-file
//! watching, the explorer cache, file names, previews, and `@` mentions.
//!
//! `Files` is the app global that holds the three stateful pieces. The
//! TypeScript modules were singletons per window; one set per app serves
//! every window here.

pub mod backend;
pub mod editor_selection;
pub mod file_index;
pub mod file_mentions;
pub mod file_name;
pub mod file_preview;
pub mod file_tree;
pub mod file_watch;
pub mod markdown_file_links;
pub mod markdown_source;
pub mod pdf_document;

use std::sync::Arc;

use gpui::{App, AppContext, Entity, EventEmitter, Global};

pub use backend::{FileMtime, FsBackend, FsEntry, LocalFs, ProjectFile};
pub use file_index::{FileIndex, FilesLoad, FilesResult, RankedFile};
pub use file_tree::{DirsChanged, FileTree};
pub use file_watch::FileWatch;

/// `notifyGitChanged`: something may have changed the working tree, so git
/// views should reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitChanged;

/// Emits `GitChanged`. Views subscribe to `Files::global(cx).git`.
pub struct GitSignal;

impl EventEmitter<GitChanged> for GitSignal {}

/// The files model of the app.
pub struct Files {
    pub index: Entity<FileIndex>,
    pub watch: Entity<FileWatch>,
    pub tree: Entity<FileTree>,
    pub git: Entity<GitSignal>,
}

impl Global for Files {}

impl Files {
    /// Create the entities and install the global. A directory change
    /// re-lists the project index, as `subscribeDirsChanged` did.
    pub fn init(backend: Arc<dyn FsBackend>, cx: &mut App) {
        let index = cx.new(|_| FileIndex::new(backend.clone()));
        let watch = cx.new(|_| FileWatch::new(backend.clone()));
        let tree = cx.new(|_| FileTree::new(backend));
        let git = cx.new(|_| GitSignal);
        let refresh = index.downgrade();
        cx.subscribe(&tree, move |_, _: &DirsChanged, cx| {
            refresh
                .update(cx, |index, cx| index.schedule_index_refresh(cx))
                .ok();
        })
        .detach();
        cx.set_global(Files {
            index,
            watch,
            tree,
            git,
        });
    }

    pub fn global(cx: &App) -> &Files {
        cx.global::<Files>()
    }

    pub fn try_global(cx: &App) -> Option<&Files> {
        cx.try_global::<Files>()
    }

    /// `document.hidden` changed for the app.
    pub fn set_hidden(hidden: bool, cx: &mut App) {
        let Some(files) = Self::handles(cx) else {
            return;
        };
        files.tree.update(cx, |tree, _| tree.set_hidden(hidden));
        files
            .index
            .update(cx, |index, cx| index.set_hidden(hidden, cx));
        files
            .watch
            .update(cx, |watch, cx| watch.set_hidden(hidden, cx));
    }

    /// A window took focus: re-check open files and the index.
    pub fn window_focused(cx: &mut App) {
        let Some(files) = Self::handles(cx) else {
            return;
        };
        files.index.update(cx, |index, cx| index.window_shown(cx));
        files.watch.update(cx, |watch, cx| watch.window_shown(cx));
    }

    /// `nudgeWorkspace`: `invalidateProjectFiles(cwd)` and
    /// `notifyDirsChanged`.
    pub fn nudge_workspace(cwd: Option<&str>, cx: &mut App) {
        let Some(files) = Self::handles(cx) else {
            return;
        };
        files
            .index
            .update(cx, |index, cx| index.invalidate_project_files(cwd, cx));
        files
            .tree
            .update(cx, |tree, cx| tree.notify_dirs_changed(cx));
    }

    /// `notifyGitChanged`.
    pub fn notify_git_changed(cx: &mut App) {
        if let Some(files) = Self::handles(cx) {
            files.git.update(cx, |_, cx| cx.emit(GitChanged));
        }
    }

    /// `nudgeWatchedFiles`.
    pub fn nudge_watched_files(paths: Option<&[String]>, cx: &mut App) {
        if let Some(files) = Self::handles(cx) {
            files
                .watch
                .update(cx, |watch, cx| watch.nudge_watched_files(paths, cx));
        }
    }

    /// `invalidateWatchedFiles`.
    pub fn invalidate_watched_files(paths: Option<&[String]>, cx: &mut App) {
        if let Some(files) = Self::handles(cx) {
            files
                .watch
                .update(cx, |watch, cx| watch.invalidate_watched_files(paths, cx));
        }
    }

    /// `invalidateProjectFiles`.
    pub fn invalidate_project_files(cwd: Option<&str>, cx: &mut App) {
        if let Some(files) = Self::handles(cx) {
            files
                .index
                .update(cx, |index, cx| index.invalidate_project_files(cwd, cx));
        }
    }

    /// `notifyDirsChanged`.
    pub fn notify_dirs_changed(cx: &mut App) {
        if let Some(files) = Self::handles(cx) {
            files
                .tree
                .update(cx, |tree, cx| tree.notify_dirs_changed(cx));
        }
    }

    fn handles(cx: &App) -> Option<FilesHandles> {
        Self::try_global(cx).map(|files| FilesHandles {
            index: files.index.clone(),
            watch: files.watch.clone(),
            tree: files.tree.clone(),
            git: files.git.clone(),
        })
    }
}

/// The entity handles, cloned out so the global is not borrowed while one
/// updates.
struct FilesHandles {
    index: Entity<FileIndex>,
    watch: Entity<FileWatch>,
    tree: Entity<FileTree>,
    git: Entity<GitSignal>,
}

#[cfg(test)]
mod tests {
    use super::backend::fake::FakeFs;
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn reloads_the_index_after_a_directory_change(cx: &mut TestAppContext) {
        let fs = FakeFs::new();
        let files = vec![ProjectFile::new("a.ts", "/Users/me/project/a.ts", "a.ts")];
        fs.set_files(files.clone());
        cx.update(|cx| Files::init(fs.clone(), cx));
        let index = cx.update(|cx| Files::global(cx).index.clone());
        index.update(cx, |index, cx| {
            drop(index.load_project_files("/Users/me/project", false, cx));
        });
        cx.run_until_parked();

        let mut more = files.clone();
        more.push(ProjectFile::new(
            "pasted.ts",
            "/Users/me/project/pasted.ts",
            "pasted.ts",
        ));
        fs.set_files(more.clone());
        let changes = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = changes.clone();
        let _observe = cx.update(|cx| cx.observe(&index, move |_, _| seen.set(seen.get() + 1)));
        cx.update(Files::notify_dirs_changed);
        cx.run_until_parked();
        cx.executor().advance_clock(file_tree::REFRESH_DELAY);
        cx.run_until_parked();
        cx.executor().advance_clock(file_index::REFRESH_DELAY);
        cx.run_until_parked();
        assert_eq!(
            index
                .read_with(cx, |index, _| index.peek_project_files("/Users/me/project"))
                .map(|files| files.to_vec()),
            Some(more)
        );
        assert!(changes.get() > 0);
    }
}
