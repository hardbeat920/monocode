//! `monocode_view_files::FilesData` over the workspace package's `Files`
//! global: the explorer cache (`FileTree`), the project file index and its
//! recents (`FileIndex`), open-file watching (`FileWatch`), and the git
//! change signal (`GitSignal`). File IO, git diffs, and staging keep the
//! trait's monocode-git defaults, as the engine's `LocalFs` does.
//!
//! A window that runs without the engine has no `Files` global; every call
//! then goes to view-files' `LocalFiles`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, AsyncApp, Global, Subscription, Task};
use monocode_engine::projects::{self, ProjectsGlobal};
use monocode_engine::workspace::Files;
use monocode_engine::workspace::files::{
    DirsChanged, FilesLoad, FsEntry as EngineEntry, GitChanged, ProjectFile as EngineFile,
};
use monocode_view_files::{DataTask, FilesData, FsEntry, Listener, LocalFiles, ProjectFile};

/// The app's one `FilesData`, shared by the explorer and every file pane.
pub fn app_files(cx: &mut App) -> Rc<dyn FilesData> {
    if let Some(global) = cx.try_global::<AppFilesGlobal>() {
        return global.0.clone();
    }
    let data: Rc<dyn FilesData> = Rc::new(AppFiles::new());
    cx.set_global(AppFilesGlobal(data.clone()));
    data
}

struct AppFilesGlobal(Rc<dyn FilesData>);

impl Global for AppFilesGlobal {}

/// The index listing last converted, so a large project is copied once per
/// scan instead of once per read.
#[derive(Default)]
struct ConvertedFiles {
    last: Option<ConvertedSnapshot>,
}

struct ConvertedSnapshot {
    source: Arc<Vec<EngineFile>>,
    files: Arc<Vec<ProjectFile>>,
}

impl ConvertedFiles {
    fn convert(&mut self, files: &Arc<Vec<EngineFile>>) -> Arc<Vec<ProjectFile>> {
        if let Some(snapshot) = &self.last
            && Arc::ptr_eq(&snapshot.source, files)
        {
            return snapshot.files.clone();
        }
        let converted = Arc::new(files.iter().map(project_file).collect::<Vec<_>>());
        self.last = Some(ConvertedSnapshot {
            source: files.clone(),
            files: converted.clone(),
        });
        converted
    }
}

/// [`FilesData`] over the engine's `Files`.
pub struct AppFiles {
    local: LocalFiles,
    converted: Rc<RefCell<ConvertedFiles>>,
}

impl AppFiles {
    pub fn new() -> Self {
        Self {
            local: LocalFiles::new(),
            converted: Rc::default(),
        }
    }

    fn convert(&self, files: &Arc<Vec<EngineFile>>) -> Arc<Vec<ProjectFile>> {
        self.converted.borrow_mut().convert(files)
    }
}

impl Default for AppFiles {
    fn default() -> Self {
        Self::new()
    }
}

fn fs_entry(entry: &EngineEntry) -> FsEntry {
    FsEntry {
        name: entry.name.clone(),
        path: entry.path.clone(),
        is_dir: entry.is_dir,
        ignored: entry.ignored,
    }
}

fn fs_entries(entries: &[EngineEntry]) -> Vec<FsEntry> {
    entries.iter().map(fs_entry).collect()
}

fn project_file(file: &EngineFile) -> ProjectFile {
    ProjectFile {
        name: file.name.clone(),
        path: file.path.clone(),
        relative: file.relative.clone(),
        is_dir: file.is_dir,
    }
}

/// An engine listing as the view's entries.
fn map_listing(
    listing: Task<Result<Vec<EngineEntry>, String>>,
    cx: &mut App,
) -> DataTask<Vec<FsEntry>> {
    cx.spawn(async move |_: &mut AsyncApp| listing.await.map(|entries| fs_entries(&entries)))
}

/// An index load as the view's files.
fn map_load(
    load: FilesLoad,
    converted: Rc<RefCell<ConvertedFiles>>,
    cx: &mut App,
) -> DataTask<Arc<Vec<ProjectFile>>> {
    cx.spawn(async move |_: &mut AsyncApp| {
        let files = load.await?;
        Ok(converted.borrow_mut().convert(&files))
    })
}

impl FilesData for AppFiles {
    fn create_path(
        &self,
        parent: &str,
        name: &str,
        is_dir: bool,
        cx: &mut App,
    ) -> DataTask<String> {
        let args = serde_json::json!({ "parent": parent, "name": name, "isDir": is_dir })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("create_path", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.create_path(parent, name, is_dir, cx)
    }

    fn rename_path(&self, path: &str, name: &str, cx: &mut App) -> DataTask<String> {
        let args = serde_json::json!({ "path": path, "name": name })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("rename_path", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.rename_path(path, name, cx)
    }

    fn delete_path(&self, path: &str, cx: &mut App) -> DataTask<()> {
        let args = serde_json::json!({ "path": path })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("delete_path", &args, cx) {
            return cx.background_spawn(async move { run.await.map(|_| ()) });
        }
        self.local.delete_path(path, cx)
    }

    fn copy_path(&self, from: &str, dest_parent: &str, cx: &mut App) -> DataTask<String> {
        let args = serde_json::json!({ "from": from, "destParent": dest_parent })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("copy_path", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.copy_path(from, dest_parent, cx)
    }

    fn move_path(&self, from: &str, dest_parent: &str, cx: &mut App) -> DataTask<String> {
        let args = serde_json::json!({ "from": from, "destParent": dest_parent })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("move_path", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.move_path(from, dest_parent, cx)
    }

    fn read_text_file(&self, path: &str, cx: &mut App) -> DataTask<String> {
        let args = serde_json::json!({ "path": path })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("read_text_file", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.read_text_file(path, cx)
    }

    fn write_text_file(&self, path: &str, content: String, cx: &mut App) -> DataTask<()> {
        let args = serde_json::json!({ "path": path, "content": content })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("write_text_file", &args, cx) {
            return cx.background_spawn(async move { run.await.map(|_| ()) });
        }
        self.local.write_text_file(path, content, cx)
    }

    fn read_binary_file(&self, path: &str, cx: &mut App) -> DataTask<Vec<u8>> {
        let args = serde_json::json!({ "path": path })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("read_binary_file", &args, cx)
        {
            return cx.background_spawn(async move {
                monocode_engine::remote::remote_commands::decode_remote_binary(&run.await?)
            });
        }
        self.local.read_binary_file(path, cx)
    }

    fn git_diff_files(&self, cwd: &str, cx: &mut App) -> DataTask<monocode_git::fs::GitDiffIndex> {
        let args = serde_json::json!({ "cwd": cwd })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("git_diff_files", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.git_diff_files(cwd, cx)
    }

    fn git_file_diff(
        &self,
        cwd: &str,
        relative: &str,
        staged: bool,
        cx: &mut App,
    ) -> DataTask<monocode_git::fs::GitFileDiff> {
        let args = serde_json::json!({ "cwd": cwd, "relative": relative, "staged": staged })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("git_file_diff", &args, cx) {
            return cx.background_spawn(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        self.local.git_file_diff(cwd, relative, staged, cx)
    }

    fn git_stage_contents(
        &self,
        cwd: &str,
        relative: &str,
        contents: String,
        cx: &mut App,
    ) -> DataTask<()> {
        let args = serde_json::json!({ "cwd": cwd, "relative": relative, "contents": contents })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) =
            monocode_engine::remote::invoke_workspace("git_stage_contents", &args, cx)
        {
            return cx.background_spawn(async move { run.await.map(|_| ()) });
        }
        self.local.git_stage_contents(cwd, relative, contents, cx)
    }

    fn format_text(&self, path: &str, source: &str, cursor: usize) -> Option<(String, usize)> {
        let boundary = source
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(source.len()))
            .take_while(|offset| *offset <= cursor)
            .last()
            .unwrap_or(0);
        let cursor_utf16 = source[..boundary].encode_utf16().count();
        let formatted =
            monocode_engine::runtime::util::format::format_text(path, source, cursor_utf16)?;
        let mut units = 0;
        let cursor = formatted
            .formatted
            .char_indices()
            .find_map(|(offset, ch)| {
                if units >= formatted.cursor_offset {
                    Some(offset)
                } else {
                    units += ch.len_utf16();
                    None
                }
            })
            .unwrap_or(formatted.formatted.len());
        Some((formatted.formatted, cursor))
    }

    fn peek_dir(&self, path: &str, cx: &App) -> Option<Vec<FsEntry>> {
        match Files::try_global(cx) {
            Some(files) => files.tree.read(cx).peek_dir(path).map(fs_entries),
            None => self.local.peek_dir(path, cx),
        }
    }

    fn list_cached_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>> {
        let Some(tree) = Files::try_global(cx).map(|files| files.tree.clone()) else {
            return self.local.list_cached_dir(path, cx);
        };
        let listing = tree.update(cx, |tree, cx| tree.list_cached_dir(path, cx));
        map_listing(listing, cx)
    }

    fn refresh_dir(&self, path: &str, cx: &mut App) -> DataTask<Vec<FsEntry>> {
        let Some(tree) = Files::try_global(cx).map(|files| files.tree.clone()) else {
            return self.local.refresh_dir(path, cx);
        };
        let listing = tree.update(cx, |tree, cx| tree.refresh_dir(path, cx));
        map_listing(listing, cx)
    }

    fn forget_dir(&self, path: &str, cx: &mut App) {
        match Files::try_global(cx).map(|files| files.tree.clone()) {
            Some(tree) => tree.update(cx, |tree, _| tree.forget_dir(path)),
            None => self.local.forget_dir(path, cx),
        }
    }

    fn load_expanded(&self, cwd: &str, cx: &App) -> HashSet<String> {
        match Files::try_global(cx) {
            Some(files) => files.tree.read(cx).load_expanded(cwd),
            None => self.local.load_expanded(cwd, cx),
        }
    }

    fn save_expanded(&self, cwd: &str, expanded: HashSet<String>, cx: &mut App) {
        match Files::try_global(cx).map(|files| files.tree.clone()) {
            Some(tree) => tree.update(cx, |tree, _| tree.save_expanded(cwd, expanded)),
            None => self.local.save_expanded(cwd, expanded, cx),
        }
    }

    fn load_selected(&self, cwd: &str, cx: &App) -> Option<String> {
        match Files::try_global(cx) {
            Some(files) => files.tree.read(cx).load_selected(cwd),
            None => self.local.load_selected(cwd, cx),
        }
    }

    fn save_selected(&self, cwd: &str, path: Option<String>, cx: &mut App) {
        match Files::try_global(cx).map(|files| files.tree.clone()) {
            Some(tree) => tree.update(cx, |tree, _| tree.save_selected(cwd, path)),
            None => self.local.save_selected(cwd, path, cx),
        }
    }

    fn notify_dirs_changed(&self, cx: &mut App) {
        if Files::try_global(cx).is_some() {
            Files::notify_dirs_changed(cx);
        } else {
            self.local.notify_dirs_changed(cx);
        }
    }

    fn subscribe_dirs_changed(&self, listener: Listener, cx: &mut App) -> Subscription {
        match Files::try_global(cx).map(|files| files.tree.clone()) {
            Some(tree) => cx.subscribe(&tree, move |_, _: &DirsChanged, cx| listener(cx)),
            None => self.local.subscribe_dirs_changed(listener, cx),
        }
    }

    fn create_parent_of(&self, cwd: &str, selected_path: Option<&str>, cx: &App) -> String {
        match Files::try_global(cx) {
            Some(files) => files.tree.read(cx).create_parent_of(cwd, selected_path),
            None => self.local.create_parent_of(cwd, selected_path, cx),
        }
    }

    fn peek_project_files(&self, cwd: &str, cx: &App) -> Option<Arc<Vec<ProjectFile>>> {
        match Files::try_global(cx) {
            Some(files) => files
                .index
                .read(cx)
                .peek_project_files(cwd)
                .map(|files| self.convert(&files)),
            None => self.local.peek_project_files(cwd, cx),
        }
    }

    fn load_project_files(
        &self,
        cwd: &str,
        refresh: bool,
        cx: &mut App,
    ) -> DataTask<Arc<Vec<ProjectFile>>> {
        let Some(index) = Files::try_global(cx).map(|files| files.index.clone()) else {
            return self.local.load_project_files(cwd, refresh, cx);
        };
        let load = index.update(cx, |index, cx| index.load_project_files(cwd, refresh, cx));
        map_load(load, self.converted.clone(), cx)
    }

    fn recent_opened_files(&self, cwd: &str, cx: &App) -> Vec<String> {
        match Files::try_global(cx) {
            Some(files) => files.index.read(cx).recent_opened_files(cwd),
            None => self.local.recent_opened_files(cwd, cx),
        }
    }

    fn remember_opened_file(&self, cwd: &str, path: &str, cx: &mut App) {
        match Files::try_global(cx).map(|files| files.index.clone()) {
            Some(index) => index.update(cx, |index, _| index.remember_opened_file(cwd, path)),
            None => self.local.remember_opened_file(cwd, path, cx),
        }
    }

    fn watch_file(&self, path: &str, listener: Listener, cx: &mut App) -> Subscription {
        match Files::try_global(cx).map(|files| files.watch.clone()) {
            Some(watch) => watch.update(cx, |watch, cx| {
                watch.watch_file(path, move |cx| listener(cx), cx)
            }),
            None => self.local.watch_file(path, listener, cx),
        }
    }

    fn sync_watched_mtime(&self, path: &str, cx: &mut App) {
        match Files::try_global(cx).map(|files| files.watch.clone()) {
            Some(watch) => watch.update(cx, |watch, cx| watch.sync_watched_mtime(path, cx)),
            None => self.local.sync_watched_mtime(path, cx),
        }
    }

    fn subscribe_git_changed(&self, listener: Listener, cx: &mut App) -> Subscription {
        match Files::try_global(cx).map(|files| files.git.clone()) {
            Some(git) => cx.subscribe(&git, move |_, _: &GitChanged, cx| listener(cx)),
            None => self.local.subscribe_git_changed(listener, cx),
        }
    }

    /// `notifyGitChanged` for the whole app: the workspace's signal (file
    /// editors, git views) and every project's git status.
    fn notify_git_changed(&self, cx: &mut App) {
        if ProjectsGlobal::try_global(cx).is_some() {
            projects::notify_git_changed(cx);
        } else if Files::try_global(cx).is_some() {
            Files::notify_git_changed(cx);
        } else {
            self.local.notify_git_changed(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_file(name: &str) -> EngineFile {
        EngineFile::new(name, format!("/p/{name}"), name)
    }

    #[test]
    fn native_formatter_keeps_utf8_cursor_on_the_same_unicode_token() {
        let source = r#"{"emoji":"😀","name":"é"}"#;
        let cursor = source.find('é').unwrap();
        let (formatted, cursor) = AppFiles::new()
            .format_text("data.json", source, cursor)
            .unwrap();
        assert!(formatted.is_char_boundary(cursor));
        assert_eq!(formatted[cursor..].chars().next(), Some('é'));
        assert!(formatted.contains("😀"));
    }

    #[test]
    fn native_formatter_rejects_invalid_source_without_replacing_it() {
        assert!(
            AppFiles::new()
                .format_text("data.json", "{broken", 3)
                .is_none()
        );
    }

    #[test]
    fn entries_keep_every_field() {
        let entry = EngineEntry {
            name: "src".into(),
            path: "/p/src".into(),
            is_dir: true,
            ignored: true,
        };
        assert_eq!(
            fs_entries(std::slice::from_ref(&entry)),
            vec![FsEntry {
                name: "src".into(),
                path: "/p/src".into(),
                is_dir: true,
                ignored: true,
            }]
        );
    }

    #[test]
    fn project_files_keep_the_folder_flag() {
        let mut file = engine_file("lib");
        file.is_dir = Some(true);
        let converted = project_file(&file);
        assert_eq!(converted.relative, "lib");
        assert_eq!(converted.is_dir, Some(true));
    }

    #[test]
    fn converts_one_listing_once() {
        let mut cache = ConvertedFiles::default();
        let listing = Arc::new(vec![engine_file("a.ts"), engine_file("b.ts")]);
        let first = cache.convert(&listing);
        let again = cache.convert(&listing);
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.len(), 2);

        // A new scan with equal contents is a new listing.
        let rescanned = Arc::new(listing.to_vec());
        let third = cache.convert(&rescanned);
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(*first, *third);
    }
}
