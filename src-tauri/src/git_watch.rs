//! Git refreshes are driven by native filesystem notifications. The worker
//! sleeps until a write arrives; it never scans a repository on a timer.
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

const BATCH_DELAY: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct GitWatchHost {
    watches: Mutex<HashMap<(String, String), ActiveWatch>>,
}

impl GitWatchHost {
    pub fn close_all(&self) {
        self.watches.lock().unwrap().clear();
    }

    pub fn close_window(&self, label: &str) {
        self.watches
            .lock()
            .unwrap()
            .retain(|(owner, _), _| owner != label);
    }
}

#[derive(Clone, Serialize)]
struct Change {
    id: String,
    failed: bool,
}

#[tauri::command]
pub async fn watch_git_changes(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    cwd: String,
    id: String,
) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 {
        return Err("Invalid Git watcher ID".into());
    }
    let label = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let root = crate::fs::expand_home(&cwd)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !root.is_dir() {
            return Err("Git watcher requires a directory".into());
        }
        let emitter = app.clone();
        let target = label.clone();
        let event_id = id.clone();
        let watch = ActiveWatch::start(root, move |failed| {
            let _ = emitter.emit_to(
                &target,
                "git-watch-changed",
                Change {
                    id: event_id.clone(),
                    failed,
                },
            );
        })?;
        let host = app.state::<GitWatchHost>();
        let mut watches = host.watches.lock().unwrap();
        // A window can close while native watcher setup is running.
        if app.get_webview_window(&label).is_some() {
            watches.insert((label, id), watch);
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn unwatch_git_changes(
    host: tauri::State<'_, GitWatchHost>,
    window: tauri::WebviewWindow,
    id: String,
) {
    host.watches
        .lock()
        .unwrap()
        .remove(&(window.label().to_owned(), id));
}

enum Message {
    Event(notify::Result<Event>),
    Stop,
}

struct ActiveWatch {
    _watcher: RecommendedWatcher,
    sender: SyncSender<Message>,
    stopped: Arc<AtomicBool>,
}

impl Drop for ActiveWatch {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Stop);
    }
}

impl ActiveWatch {
    fn start(root: PathBuf, emit: impl Fn(bool) + Send + 'static) -> Result<Self, String> {
        let mut filter = GitFilter::new(root.clone());
        let (sender, receiver) = mpsc::sync_channel(512);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        let callback_sender = sender.clone();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            if event
                .as_ref()
                .is_ok_and(|event| matches!(event.kind, EventKind::Access(_)))
            {
                return;
            }
            if matches!(
                callback_sender.try_send(Message::Event(event)),
                Err(mpsc::TrySendError::Full(_))
            ) {
                callback_overflow.store(true, Ordering::Release);
            }
        })
        .map_err(|error| error.to_string())?;
        let mut roots = vec![root];
        for dir in &filter.git_dirs {
            if dir.is_dir() && !roots.iter().any(|parent| dir.starts_with(parent)) {
                // A linked worktree's metadata and shared refs live outside it.
                roots.retain(|parent| !parent.starts_with(dir));
                roots.push(dir.clone());
            }
        }
        for dir in roots {
            watcher
                .watch(&dir, RecursiveMode::Recursive)
                .map_err(|error| error.to_string())?;
        }
        // Register first, then load ignore rules, so setup cannot lose edits.
        filter.reload_ignored();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        std::thread::Builder::new()
            .name("git-watch".into())
            .spawn(move || {
                while let Ok(first) = receiver.recv() {
                    if worker_stopped.load(Ordering::Acquire) || matches!(first, Message::Stop) {
                        break;
                    }
                    let mut batch = vec![first];
                    let deadline = Instant::now() + BATCH_DELAY;
                    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                        match receiver.recv_timeout(remaining) {
                            Ok(Message::Stop) => return,
                            Ok(event) => batch.push(event),
                            Err(_) => break,
                        }
                    }
                    if worker_stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let rescan = overflow.swap(false, Ordering::AcqRel);
                    if rescan
                        || batch.iter().any(|message| match message {
                            Message::Event(Ok(event)) => {
                                event.need_rescan()
                                    || event.paths.is_empty()
                                    || filter.rules_changed(event)
                            }
                            _ => false,
                        })
                    {
                        filter.reload_ignored();
                    }
                    let failed = batch
                        .iter()
                        .any(|message| matches!(message, Message::Event(Err(_))));
                    let changed = rescan
                        || batch.iter().any(|message| match message {
                            Message::Event(Ok(event)) => filter.relevant(event),
                            _ => false,
                        });
                    if changed || failed {
                        emit(failed);
                    }
                    if failed {
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _watcher: watcher,
            sender,
            stopped,
        })
    }
}

struct GitFilter {
    root: PathBuf,
    git_dirs: Vec<PathBuf>,
    ignored: HashSet<PathBuf>,
    tracked: HashSet<PathBuf>,
}

impl GitFilter {
    fn new(root: PathBuf) -> Self {
        let mut git_dirs = crate::fs::git_output(
            &root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ],
        )
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(|path| {
                    let path = PathBuf::from(path);
                    path.canonicalize().unwrap_or(path)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
        git_dirs.push(root.join(".git"));
        git_dirs.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        git_dirs.dedup();
        Self {
            root,
            git_dirs,
            ignored: HashSet::new(),
            tracked: HashSet::new(),
        }
    }

    fn reload_ignored(&mut self) {
        self.ignored = self.git_paths(&[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
            "--",
            ".",
        ]);
        self.tracked = self.git_paths(&["ls-files", "--cached", "-z", "--", "."]);
    }

    fn git_paths(&self, args: &[&str]) -> HashSet<PathBuf> {
        crate::fs::git_output(&self.root, args)
            .unwrap_or_default()
            .split(|byte| *byte == 0)
            .filter(|bytes| !bytes.is_empty())
            .map(|bytes| {
                self.root
                    .join(String::from_utf8_lossy(bytes).trim_end_matches('/'))
            })
            .collect()
    }

    fn metadata<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.git_dirs
            .iter()
            .find_map(|dir| path.strip_prefix(dir).ok())
    }

    fn rules_changed(&self, event: &Event) -> bool {
        !matches!(event.kind, EventKind::Access(_))
            && event.paths.iter().any(|path| {
                path.file_name()
                    .is_some_and(|name| name == ".gitignore" || name == ".git")
                    || self.metadata(path).is_some_and(|relative| {
                        matches!(
                            relative.to_str(),
                            Some("index" | "config" | "config.worktree")
                        ) || relative == Path::new("info/exclude")
                    })
                    || (self.metadata(path).is_none()
                        && matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Modify(notify::event::ModifyKind::Name(_))
                        )
                        && path.starts_with(&self.root)
                        && !path.ancestors().any(|parent| self.ignored.contains(parent)))
            })
    }

    fn relevant(&self, event: &Event) -> bool {
        if matches!(event.kind, EventKind::Access(_)) {
            return false;
        }
        if event.need_rescan() || event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| {
            if let Some(relative) = self.metadata(path) {
                return metadata_relevant(relative);
            }
            if !path.starts_with(&self.root)
                || path.components().any(|part| part.as_os_str() == ".git")
            {
                return false;
            }
            self.tracked.contains(path)
                || !path.ancestors().any(|parent| self.ignored.contains(parent))
        })
    }
}

fn metadata_relevant(relative: &Path) -> bool {
    if relative
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".lock"))
    {
        return false;
    }
    relative.as_os_str().is_empty()
        || matches!(
            relative.to_str(),
            Some("HEAD" | "index" | "config" | "config.worktree" | "packed-refs" | "shallow")
        )
        || relative == Path::new("info/exclude")
        || relative.starts_with("refs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, ModifyKind};
    use std::{fs, process::Command};

    struct Repo(PathBuf);
    impl Repo {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("monocode-git-watch-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            let repo = Self(path.canonicalize().unwrap());
            repo.git(&["init", "-q", "-b", "main"]);
            fs::write(repo.0.join("file.txt"), "initial\n").unwrap();
            repo.git(&["add", "."]);
            repo.git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "initial",
            ]);
            repo
        }
        fn git(&self, args: &[&str]) {
            let result = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn change(path: PathBuf) -> Event {
        Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(path)
    }

    #[test]
    fn ignores_git_reads_locks_objects_and_build_output_but_keeps_tracked_ignored_files() {
        let repo = Repo::new();
        fs::write(repo.0.join(".gitignore"), "build/\n").unwrap();
        fs::create_dir(repo.0.join("build")).unwrap();
        fs::write(repo.0.join("build/generated.txt"), "ignored").unwrap();
        fs::write(repo.0.join("build/tracked.txt"), "tracked").unwrap();
        repo.git(&["add", "-f", "build/tracked.txt"]);
        let mut filter = GitFilter::new(repo.0.clone());
        filter.reload_ignored();
        assert!(!filter.relevant(
            &Event::new(EventKind::Access(AccessKind::Read)).add_path(repo.0.join("file.txt"))
        ));
        for path in [
            ".git/index.lock",
            ".git/objects/ab/cd",
            ".git/logs/HEAD",
            "build/generated.txt",
        ] {
            assert!(!filter.relevant(&change(repo.0.join(path))), "{path}");
        }
        for path in [
            "file.txt",
            ".git/HEAD",
            ".git/index",
            ".git/refs/heads/main",
            "build/tracked.txt",
            ".gitignore",
        ] {
            assert!(filter.relevant(&change(repo.0.join(path))), "{path}");
        }
        // A newly created ignored directory is recognized before refreshing Git.
        fs::write(repo.0.join(".gitignore"), "build/\nnew-build/\n").unwrap();
        fs::create_dir(repo.0.join("new-build")).unwrap();
        let created =
            Event::new(EventKind::Create(CreateKind::Folder)).add_path(repo.0.join("new-build"));
        assert!(filter.rules_changed(&created));
        filter.reload_ignored();
        assert!(!filter.relevant(&created));
    }

    #[test]
    fn native_watcher_reports_external_edits_and_staging_without_self_triggering_on_git_reads() {
        let repo = Repo::new();
        let (sender, receiver) = mpsc::channel();
        let watch = ActiveWatch::start(repo.0.clone(), move |failed| {
            let _ = sender.send(failed);
        })
        .unwrap();
        // Drain any startup events from the native backend.
        while receiver.recv_timeout(Duration::from_millis(500)).is_ok() {}
        fs::write(repo.0.join("file.tmp"), "agent edit\n").unwrap();
        fs::rename(repo.0.join("file.tmp"), repo.0.join("file.txt")).unwrap();
        assert!(!receiver.recv_timeout(Duration::from_secs(5)).unwrap());
        while receiver.recv_timeout(Duration::from_millis(300)).is_ok() {}
        repo.git(&["add", "file.txt"]);
        assert!(!receiver.recv_timeout(Duration::from_secs(5)).unwrap());
        while receiver.recv_timeout(Duration::from_millis(300)).is_ok() {}
        let _ = crate::fs::git_output(&repo.0, &["status", "--porcelain"]);
        let _ = crate::fs::git_output(&repo.0, &["diff", "--cached"]);
        assert!(
            receiver.recv_timeout(Duration::from_millis(600)).is_err(),
            "Git reads must not schedule another refresh"
        );
        drop(watch);
    }

    #[test]
    fn linked_worktree_watches_shared_refs_outside_its_working_directory() {
        let repo = Repo::new();
        let linked = Repo::new();
        fs::remove_dir_all(&linked.0).unwrap();
        repo.git(&[
            "worktree",
            "add",
            "-qb",
            "linked",
            linked.0.to_str().unwrap(),
        ]);
        let (sender, receiver) = mpsc::channel();
        let watch = ActiveWatch::start(linked.0.clone(), move |failed| {
            let _ = sender.send(failed);
        })
        .unwrap();
        while receiver.recv_timeout(Duration::from_millis(500)).is_ok() {}
        repo.git(&["branch", "external-branch"]);
        assert!(!receiver.recv_timeout(Duration::from_secs(5)).unwrap());
        drop(watch);
    }
}
