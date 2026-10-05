//! Shared setup and a scripted [`FilesData`] for the view tests.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Subscription, Task};
use monocode_git::fs::{GitDiffIndex, GitFileDiff};

use crate::data::{DataTask, ExplorerCache, FilesData, FsEntry, Listener, ProjectFile};

/// Installs the theme, the editor keys, and this crate's keys.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
    monocode_editor::init(cx);
    crate::init(cx);
}

/// What a [`FakeFiles`] call did, for assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Copy {
        from: String,
        dest_parent: String,
    },
    Move {
        from: String,
        dest_parent: String,
    },
    Create {
        parent: String,
        name: String,
        is_dir: bool,
    },
    Rename {
        path: String,
        name: String,
    },
    Delete {
        path: String,
    },
    Write {
        path: String,
        content: String,
    },
    ClipboardFiles,
    Stage {
        relative: String,
        contents: String,
    },
}

/// Listeners by subscription id.
pub type Listeners = Vec<(u64, Rc<dyn Fn(&mut App)>)>;

#[derive(Default)]
pub struct FakeState {
    pub dirs: HashMap<String, Vec<FsEntry>>,
    pub calls: Vec<Call>,
    pub clipboard_files: Vec<String>,
    pub files: HashMap<String, String>,
    pub read_errors: HashMap<String, String>,
    pub write_error: Option<String>,
    /// Writes wait until the test releases them.
    pub hold_writes: bool,
    pub held: Vec<futures::channel::oneshot::Sender<()>>,
    pub binary: HashMap<String, Vec<u8>>,
    pub project_files: HashMap<String, Arc<Vec<ProjectFile>>>,
    pub project_errors: HashMap<String, String>,
    /// What `load_project_files` returns, when it differs from the peek.
    pub load_results: HashMap<String, Arc<Vec<ProjectFile>>>,
    /// Calls to `rank_project_files`, and the TypeScript test's ranking:
    /// every file unless a non-empty query lacks "app".
    pub rank_calls: usize,
    pub mock_ranking: bool,
    /// Project loads never finish, like a scan that is still running.
    pub hang_project_loads: bool,
    pub load_calls: Vec<(String, bool)>,
    pub watchers: HashMap<String, Listeners>,
    pub next_watcher: u64,
    pub git_files: Vec<monocode_git::fs::GitChangedFile>,
    pub git_diffs: HashMap<String, GitFileDiff>,
    pub git_listeners: Listeners,
    pub recents: Vec<String>,
}

/// A [`FilesData`] over in-memory folders and files. The explorer cache is
/// the real [`ExplorerCache`], listing from `dirs`.
#[derive(Clone)]
pub struct FakeFiles {
    pub state: Rc<RefCell<FakeState>>,
    pub explorer: ExplorerCache,
}

impl FakeFiles {
    pub fn new() -> Rc<Self> {
        let state: Rc<RefCell<FakeState>> = Rc::default();
        let lister_state = state.clone();
        let explorer = ExplorerCache::new(Rc::new(move |path, _cx: &mut App| {
            let result = lister_state
                .borrow()
                .dirs
                .get(&path)
                .cloned()
                .ok_or_else(|| format!("{path}: No such directory"));
            Task::ready(result)
        }));
        Rc::new(Self { state, explorer })
    }

    pub fn set_dir(&self, path: &str, entries: Vec<FsEntry>) {
        self.state
            .borrow_mut()
            .dirs
            .insert(path.to_string(), entries);
    }

    pub fn set_file(&self, path: &str, content: &str) {
        self.state
            .borrow_mut()
            .files
            .insert(path.to_string(), content.to_string());
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.borrow().calls.clone()
    }

    pub fn copies(&self) -> Vec<(String, String)> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Copy { from, dest_parent } => Some((from, dest_parent)),
                _ => None,
            })
            .collect()
    }

    pub fn writes(&self) -> Vec<(String, String)> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Write { path, content } => Some((path, content)),
                _ => None,
            })
            .collect()
    }

    /// Fire the watchers of `path`, like `invalidateWatchedFiles`.
    pub fn touch(&self, path: &str, cx: &mut App) {
        let listeners: Vec<_> = self
            .state
            .borrow()
            .watchers
            .get(path)
            .map(|listeners| listeners.iter().map(|(_, l)| l.clone()).collect())
            .unwrap_or_default();
        for listener in listeners {
            listener(cx);
        }
    }

    pub fn release_writes(&self) {
        let held = std::mem::take(&mut self.state.borrow_mut().held);
        for sender in held {
            let _ = sender.send(());
        }
    }

    fn record(&self, call: Call) {
        self.state.borrow_mut().calls.push(call);
    }
}

fn base_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

impl FilesData for FakeFiles {
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
        let mut state = self.state.borrow_mut();
        state.load_calls.push((cwd.to_string(), refresh));
        if let Some(error) = state.project_errors.get(cwd) {
            return Task::ready(Err(error.clone()));
        }
        if let Some(files) = state.load_results.get(cwd) {
            return Task::ready(Ok(files.clone()));
        }
        if state.hang_project_loads {
            let (sender, receiver) = futures::channel::oneshot::channel::<()>();
            state.held.push(sender);
            return cx.spawn(async move |_| {
                let _ = receiver.await;
                Err("cancelled".to_string())
            });
        }
        Task::ready(Ok(state
            .project_files
            .get(cwd)
            .cloned()
            .unwrap_or_default()))
    }

    fn rank_project_files(
        &self,
        files: &[ProjectFile],
        query: &str,
        recents: &[String],
    ) -> Vec<crate::data::RankedFile> {
        let mock = {
            let mut state = self.state.borrow_mut();
            state.rank_calls += 1;
            state.mock_ranking
        };
        if !mock {
            return crate::data::rank_project_files(
                files,
                query,
                recents,
                crate::data::MAX_RESULTS,
            );
        }
        if !query.trim().is_empty() && !query.to_lowercase().contains("app") {
            return Vec::new();
        }
        files
            .iter()
            .map(|file| crate::data::RankedFile {
                file: file.clone(),
                score: 1,
                positions: Vec::new(),
            })
            .collect()
    }

    fn recent_opened_files(&self, _: &str, _: &App) -> Vec<String> {
        self.state.borrow().recents.clone()
    }

    fn remember_opened_file(&self, _: &str, path: &str, _: &mut App) {
        let mut state = self.state.borrow_mut();
        state.recents.retain(|item| item != path);
        state.recents.insert(0, path.to_string());
    }

    fn watch_file(&self, path: &str, listener: Listener, _: &mut App) -> Subscription {
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
        let state = Rc::downgrade(&self.state);
        let path = path.to_string();
        Subscription::new(move || {
            if let Some(state) = state.upgrade()
                && let Some(listeners) = state.borrow_mut().watchers.get_mut(&path)
            {
                listeners.retain(|(entry, _)| *entry != id);
            }
        })
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

    fn create_path(&self, parent: &str, name: &str, is_dir: bool, _: &mut App) -> DataTask<String> {
        self.record(Call::Create {
            parent: parent.into(),
            name: name.into(),
            is_dir,
        });
        let created = format!("{parent}/{name}");
        let entry = if is_dir {
            FsEntry::dir(base_name(&created), created.clone())
        } else {
            FsEntry::file(base_name(&created), created.clone())
        };
        self.state
            .borrow_mut()
            .dirs
            .entry(parent.to_string())
            .or_default()
            .push(entry);
        Task::ready(Ok(created))
    }

    fn rename_path(&self, path: &str, name: &str, _: &mut App) -> DataTask<String> {
        self.record(Call::Rename {
            path: path.into(),
            name: name.into(),
        });
        let parent = crate::paths::parent_path(path);
        let next = format!("{parent}/{name}");
        if let Some(entries) = self.state.borrow_mut().dirs.get_mut(&parent) {
            for entry in entries.iter_mut().filter(|entry| entry.path == path) {
                entry.path = next.clone();
                entry.name = name.to_string();
            }
        }
        Task::ready(Ok(next))
    }

    fn delete_path(&self, path: &str, _: &mut App) -> DataTask<()> {
        self.record(Call::Delete { path: path.into() });
        let parent = crate::paths::parent_path(path);
        if let Some(entries) = self.state.borrow_mut().dirs.get_mut(&parent) {
            entries.retain(|entry| entry.path != path);
        }
        Task::ready(Ok(()))
    }

    fn copy_path(&self, from: &str, dest_parent: &str, _: &mut App) -> DataTask<String> {
        self.record(Call::Copy {
            from: from.into(),
            dest_parent: dest_parent.into(),
        });
        Task::ready(Ok(format!("{dest_parent}/{}", base_name(from))))
    }

    fn move_path(&self, from: &str, dest_parent: &str, _: &mut App) -> DataTask<String> {
        self.record(Call::Move {
            from: from.into(),
            dest_parent: dest_parent.into(),
        });
        Task::ready(Ok(format!("{dest_parent}/{}", base_name(from))))
    }

    fn reveal_path(&self, _: &str, _: &mut App) -> DataTask<()> {
        Task::ready(Ok(()))
    }

    fn clipboard_file_paths(&self, _: &mut App) -> DataTask<Vec<String>> {
        self.record(Call::ClipboardFiles);
        Task::ready(Ok(self.state.borrow().clipboard_files.clone()))
    }

    fn read_text_file(&self, path: &str, _: &mut App) -> DataTask<String> {
        let state = self.state.borrow();
        if let Some(error) = state.read_errors.get(path) {
            return Task::ready(Err(error.clone()));
        }
        Task::ready(
            state
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{path}: No such file")),
        )
    }

    fn write_text_file(&self, path: &str, content: String, cx: &mut App) -> DataTask<()> {
        self.record(Call::Write {
            path: path.into(),
            content: content.clone(),
        });
        let mut state = self.state.borrow_mut();
        if let Some(error) = state.write_error.take() {
            return Task::ready(Err(error));
        }
        state.files.insert(path.to_string(), content);
        if state.hold_writes {
            let (sender, receiver) = futures::channel::oneshot::channel();
            state.held.push(sender);
            return cx.spawn(async move |_| {
                let _ = receiver.await;
                Ok(())
            });
        }
        Task::ready(Ok(()))
    }

    fn read_binary_file(&self, path: &str, _: &mut App) -> DataTask<Vec<u8>> {
        Task::ready(
            self.state
                .borrow()
                .binary
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{path}: No such file")),
        )
    }

    fn git_diff_files(&self, _: &str, _: &mut App) -> DataTask<GitDiffIndex> {
        Task::ready(Ok(GitDiffIndex {
            files: self.state.borrow().git_files.clone(),
            ..Default::default()
        }))
    }

    fn git_file_diff(
        &self,
        _: &str,
        relative: &str,
        _: bool,
        _: &mut App,
    ) -> DataTask<GitFileDiff> {
        Task::ready(
            self.state
                .borrow()
                .git_diffs
                .get(relative)
                .cloned()
                .ok_or_else(|| "no diff".to_string()),
        )
    }

    fn git_stage_contents(
        &self,
        _: &str,
        relative: &str,
        contents: String,
        _: &mut App,
    ) -> DataTask<()> {
        self.record(Call::Stage {
            relative: relative.into(),
            contents,
        });
        Task::ready(Ok(()))
    }
}
