//! The file system calls the files model makes: `listProjectFiles`,
//! `statFiles`, and `listDir` from src/platform/tauri/fs.ts.
//!
//! `LocalFs` runs `monocode_git::fs` on a background thread. Tests use a
//! fake that resolves when the test says so, the way the TypeScript tests
//! used deferred promises.

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

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

    /// `Boolean(file.isDir)`.
    pub fn is_dir(&self) -> bool {
        self.is_dir == Some(true)
    }
}

/// `FsEntry`: one child of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub ignored: bool,
}

/// `FileMtime`: a stat result. `mtime_ms` is `None` when the path is missing
/// or not a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMtime {
    pub path: String,
    pub mtime_ms: Option<i64>,
}

/// A file system call that finishes off the UI thread.
pub type FsFuture<T> = BoxFuture<'static, Result<T, String>>;

/// The calls the files model makes. Every method returns a future the
/// caller runs on the background executor.
pub trait FsBackend: Send + Sync {
    /// `listProjectFiles`.
    fn list_project_files(&self, cwd: String) -> FsFuture<Vec<ProjectFile>>;
    /// `statFiles`, at most 64 paths per call.
    fn stat_files(&self, paths: Vec<String>) -> FsFuture<Vec<FileMtime>>;
    /// `listDir`.
    fn list_dir(&self, path: String) -> FsFuture<Vec<FsEntry>>;
}

/// The local disk through `monocode_git::fs`.
// TODO(port): statFiles and listProjectFiles routed `remote://` paths to the
// connected host. The remote package should wrap this backend for them.
pub struct LocalFs;

/// `monocode_git` keeps the fields of its results private and only
/// serializes them, so read them back through JSON.
fn reshape<T: Serialize, U: for<'de> Deserialize<'de>>(value: T) -> Result<U, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|err| err.to_string())
}

impl FsBackend for LocalFs {
    fn list_project_files(&self, cwd: String) -> FsFuture<Vec<ProjectFile>> {
        Box::pin(async move {
            let files = monocode_git::fs::list_project_files(cwd)?;
            Ok(files
                .into_iter()
                .map(|file| ProjectFile::new(file.name, file.path, file.relative))
                .collect())
        })
    }

    fn stat_files(&self, paths: Vec<String>) -> FsFuture<Vec<FileMtime>> {
        Box::pin(async move { reshape(monocode_git::fs::stat_files(paths)?) })
    }

    fn list_dir(&self, path: String) -> FsFuture<Vec<FsEntry>> {
        Box::pin(async move { reshape(monocode_git::fs::list_dir(path)?) })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An `FsBackend` whose answers the test scripts, and that records every
    //! call. A call with no scripted answer waits until `resolve_*` runs.

    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;

    use futures::channel::oneshot;
    use parking_lot::Mutex;

    use super::*;

    type Reply<T> = oneshot::Sender<Result<T, String>>;

    #[derive(Default)]
    struct State {
        list_calls: Vec<String>,
        stat_calls: Vec<Vec<String>>,
        dir_calls: Vec<String>,
        /// The answer every `list_project_files` call gets, unless one is queued.
        list_default: Option<Result<Vec<ProjectFile>, String>>,
        list_queue: VecDeque<Option<Result<Vec<ProjectFile>, String>>>,
        list_pending: VecDeque<Reply<Vec<ProjectFile>>>,
        mtimes: HashMap<String, VecDeque<Option<i64>>>,
        dir_queue: HashMap<String, VecDeque<Vec<FsEntry>>>,
        dir_default: HashMap<String, Vec<FsEntry>>,
    }

    #[derive(Default, Clone)]
    pub struct FakeFs {
        state: Arc<Mutex<State>>,
    }

    impl FakeFs {
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        pub fn set_files(&self, files: Vec<ProjectFile>) {
            self.state.lock().list_default = Some(Ok(files));
        }

        pub fn fail_files(&self, message: &str) {
            self.state.lock().list_default = Some(Err(message.into()));
        }

        /// The next call waits for `resolve_files`.
        pub fn defer_next_files(&self) {
            self.state.lock().list_queue.push_back(None);
        }

        /// Answer the oldest waiting `list_project_files` call.
        pub fn resolve_files(&self, files: Vec<ProjectFile>) {
            if let Some(reply) = self.state.lock().list_pending.pop_front() {
                let _ = reply.send(Ok(files));
            }
        }

        pub fn list_calls(&self) -> Vec<String> {
            self.state.lock().list_calls.clone()
        }

        #[allow(dead_code)]
        pub fn clear_calls(&self) {
            let mut state = self.state.lock();
            state.list_calls.clear();
            state.stat_calls.clear();
            state.dir_calls.clear();
        }

        /// Queue mtimes for a path, one per stat. The last one repeats.
        pub fn push_mtime(&self, path: &str, mtime: Option<i64>) {
            self.state
                .lock()
                .mtimes
                .entry(path.into())
                .or_default()
                .push_back(mtime);
        }

        pub fn stat_calls(&self) -> Vec<Vec<String>> {
            self.state.lock().stat_calls.clone()
        }

        /// Queue the next listing of a folder.
        pub fn push_dir(&self, path: &str, entries: Vec<FsEntry>) {
            self.state
                .lock()
                .dir_queue
                .entry(path.into())
                .or_default()
                .push_back(entries);
        }

        pub fn dir_calls(&self) -> Vec<String> {
            self.state.lock().dir_calls.clone()
        }
    }

    impl FsBackend for FakeFs {
        fn list_project_files(&self, cwd: String) -> FsFuture<Vec<ProjectFile>> {
            let mut state = self.state.lock();
            state.list_calls.push(cwd);
            let answer = match state.list_queue.pop_front() {
                Some(queued) => queued,
                None => state.list_default.clone(),
            };
            match answer {
                Some(answer) => Box::pin(async move { answer }),
                None => {
                    let (sender, receiver) = oneshot::channel();
                    state.list_pending.push_back(sender);
                    Box::pin(
                        async move { receiver.await.unwrap_or_else(|_| Err("dropped".into())) },
                    )
                }
            }
        }

        fn stat_files(&self, paths: Vec<String>) -> FsFuture<Vec<FileMtime>> {
            let mut state = self.state.lock();
            state.stat_calls.push(paths.clone());
            let stats = paths
                .into_iter()
                .map(|path| {
                    let queue = state.mtimes.entry(path.clone()).or_default();
                    let mtime_ms = if queue.len() > 1 {
                        queue.pop_front().flatten()
                    } else {
                        queue.front().copied().flatten()
                    };
                    FileMtime { path, mtime_ms }
                })
                .collect();
            Box::pin(async move { Ok(stats) })
        }

        fn list_dir(&self, path: String) -> FsFuture<Vec<FsEntry>> {
            let mut state = self.state.lock();
            state.dir_calls.push(path.clone());
            let entries = match state.dir_queue.get_mut(&path).and_then(VecDeque::pop_front) {
                Some(entries) => {
                    state.dir_default.insert(path, entries.clone());
                    Ok(entries)
                }
                None => state
                    .dir_default
                    .get(&path)
                    .cloned()
                    .ok_or_else(|| format!("{path}: No such directory")),
            };
            Box::pin(async move { entries })
        }
    }
}
