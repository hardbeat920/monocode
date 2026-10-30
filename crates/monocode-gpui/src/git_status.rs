//! The project's Git status, shared by the right panel and the title bar:
//! GitChangesPanel's `useDiffIndex`, useGitFileStatuses and
//! useProjectDiffStats over one off-thread reader. Reads never overlap; a
//! read requested meanwhile runs once the current one returns, and only a
//! changed snapshot notifies.
use gpui::{Context, EventEmitter, Task};
use monocode_core::scm::{self, DiffIndex, DiffStats};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// Without a file watcher, visible status polls on this interval.
const POLL: Duration = Duration::from_secs(2);
const RESUME_TTL: Duration = Duration::from_secs(30);
/// A burst of writes settles into one read this long after the last.
const SETTLE: Duration = Duration::from_millis(300);
/// Watched status still reads this often, in case an event went missing.
const REPAIR: Duration = Duration::from_secs(30);

/// A disk or Git change: Changes, History and the Explorer reread.
pub(crate) struct GitChanged;

#[derive(Default, Clone, PartialEq)]
pub(crate) struct StatusMap {
    pub files: HashMap<String, String>,
    pub dirs: HashMap<String, String>,
}
fn priority(status: &str) -> u8 {
    match status {
        "modified" => 3,
        "deleted" => 2,
        "added" | "untracked" => 1,
        _ => 0,
    }
}
/// useGitFileStatuses's `buildStatusMaps`: each folder takes its most
/// significant descendant status.
pub(crate) fn status_maps(index: &DiffIndex, root: &str) -> StatusMap {
    let mut map = StatusMap::default();
    for file in &index.files {
        map.files.insert(file.path.clone(), file.status.clone());
        let prio = priority(&file.status);
        if prio == 0 {
            continue;
        }
        let mut dir = parent(&file.path);
        while dir.len() > root.len() {
            let existing = map.dirs.get(dir).map_or(0, |s| priority(s));
            if prio <= existing {
                break;
            }
            map.dirs.insert(dir.to_owned(), file.status.clone());
            dir = parent(dir);
        }
    }
    map
}
fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

#[derive(Clone)]
struct PreparedIndex {
    index: Arc<DiffIndex>,
    statuses: Arc<StatusMap>,
    changes: Arc<crate::changes::PreparedChanges>,
}

pub(crate) struct ProjectGit {
    /// The active session's working folder (its worktree, if it has one):
    /// Sidebar.tsx's `gitRoot`.
    project: String,
    pub root: String,
    pub index: Option<Arc<DiffIndex>>,
    pub stats: Option<DiffStats>,
    pub statuses: Arc<StatusMap>,
    pub changes: Arc<crate::changes::PreparedChanges>,
    /// Bumped on every observed change, for views that reread on it.
    pub epoch: u64,
    pub review_cache: crate::review::ReviewCache,
    versions: Option<Arc<HashMap<String, scm::FileDiffVersion>>>,
    remembered: HashMap<String, (u64, PreparedIndex)>,
    remembered_files: usize,
    clock: u64,
    in_flight: bool,
    pending: bool,
    stats_loaded: Option<Instant>,
    /// Reads while status is visible: a file watcher, or a timer without one.
    poll: Option<[Task<()>; 2]>,
}
impl EventEmitter<GitChanged> for ProjectGit {}

impl ProjectGit {
    pub(crate) fn new(project: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            root: project.clone(),
            project: String::new(),
            index: None,
            stats: None,
            statuses: Arc::default(),
            changes: Arc::default(),
            epoch: 0,
            review_cache: Default::default(),
            versions: None,
            remembered: HashMap::new(),
            remembered_files: 0,
            clock: 0,
            in_flight: false,
            pending: false,
            stats_loaded: None,
            poll: None,
        };
        this.set_project(project, cx);
        this
    }
    pub(crate) fn project(&self) -> &str {
        &self.project
    }
    pub(crate) fn available(&self) -> bool {
        !self.project.is_empty() && self.project != "~"
    }
    /// The first frame of a switch already shows the new checkout's last
    /// snapshot, never the previous one's files and actions.
    pub(crate) fn set_project(&mut self, project: String, cx: &mut Context<Self>) {
        if project == self.project {
            return;
        }
        self.project = project.clone();
        self.root = project.clone();
        let remembered = self.remembered.get(&project).map(|(_, snapshot)| snapshot);
        self.index = remembered.map(|snapshot| snapshot.index.clone());
        self.stats = self.index.as_deref().map(stats_of);
        self.statuses = remembered
            .map(|snapshot| snapshot.statuses.clone())
            .unwrap_or_default();
        self.changes = remembered
            .map(|snapshot| snapshot.changes.clone())
            .unwrap_or_default();
        self.stats_loaded = None;
        self.versions = None;
        self.epoch += 1;
        cx.emit(GitChanged);
        cx.notify();
        self.warm_choices(cx);
        if self.poll.take().is_some() {
            // Watch the new checkout instead; this reads it straight away.
            self.set_polling(true, cx);
        } else {
            self.reload(cx);
        }
    }
    /// While the project's Git status is visible, read it again whenever the
    /// checkout or its Git directory changes on disk, and every 30s besides.
    /// Without a file watcher, poll every 2s instead.
    pub(crate) fn set_polling(&mut self, polling: bool, cx: &mut Context<Self>) {
        if polling == self.poll.is_some() {
            return;
        }
        if !polling {
            self.poll = None;
            return;
        }
        self.reload(cx);
        let project = PathBuf::from(&self.project);
        let watched = cx.spawn(async move |this, cx| {
            let watch = cx
                .background_executor()
                .spawn(async move {
                    let mut roots = vec![project.clone()];
                    roots.extend(
                        scm::git_dirs(&project)
                            .into_iter()
                            .filter(|dir| !dir.starts_with(&project)),
                    );
                    crate::fs_watch::watch(&roots)
                })
                .await;
            let Some((_watch, changes)) = watch else {
                loop {
                    cx.background_executor().timer(POLL).await;
                    if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                        return;
                    }
                }
            };
            while changes.recv().await.is_ok() {
                cx.background_executor().timer(SETTLE).await;
                while changes.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    return;
                }
            }
        });
        let repair = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REPAIR).await;
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    break;
                }
            }
        });
        self.poll = Some([watched, repair]);
    }
    /// useProjectDiffStats reads again on focus once its snapshot is 30s old.
    pub(crate) fn resume(&mut self, cx: &mut Context<Self>) {
        if self
            .stats_loaded
            .is_none_or(|at| at.elapsed() >= RESUME_TTL)
        {
            self.warm_choices(cx);
            self.reload(cx);
        }
    }
    /// A mutation or file operation: reread now and tell every view.
    pub(crate) fn changed(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        cx.emit(GitChanged);
        self.warm_choices(cx);
        self.reload(cx);
    }
    /// Project selection warms both menus before the user opens either one.
    fn warm_choices(&self, cx: &Context<Self>) {
        if !self.available() {
            return;
        }
        let project = self.project.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = crate::git_cache::load(std::path::Path::new(&project));
            })
            .detach();
    }
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.available() {
            return;
        }
        if self.in_flight {
            self.pending = true;
            return;
        }
        self.in_flight = true;
        let project = self.project.clone();
        let path = PathBuf::from(&project);
        let previous = self.index.clone();
        let previous_versions = self.versions.clone();
        let work = cx.background_executor().spawn(async move {
            let index = scm::diff_index(&path)?;
            let versions = scm::file_diff_versions(&path, &index).ok().map(Arc::new);
            let changed = previous.as_deref() != Some(&index) || previous_versions != versions;
            let prepared = changed.then(|| PreparedIndex {
                statuses: Arc::new(status_maps(&index, &path.to_string_lossy())),
                changes: Arc::new(crate::changes::PreparedChanges::new(&index)),
                index: Arc::new(index),
            });
            Ok::<_, String>((prepared, versions))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.in_flight = false;
                // A slow or unavailable checkout keeps its last snapshot, and
                // never takes another checkout's.
                if this.project == project
                    && let Ok((prepared, versions)) = result
                {
                    this.stats_loaded = Some(Instant::now());
                    if let Some(next) = prepared {
                        let had = this.index.is_some();
                        this.remember(&project, next.clone());
                        this.stats = Some(stats_of(&next.index));
                        this.statuses = next.statuses;
                        this.changes = next.changes;
                        this.index = Some(next.index);
                        this.versions = versions;
                        if had {
                            this.epoch += 1;
                            cx.emit(GitChanged);
                        }
                        cx.notify();
                    }
                }
                if std::mem::take(&mut this.pending) {
                    this.reload(cx);
                }
            });
        })
        .detach();
    }
    /// rememberIndex: recent checkouts open on their last snapshot, within 24
    /// checkouts and 20,000 files.
    fn remember(&mut self, project: &str, index: PreparedIndex) {
        self.clock += 1;
        self.remembered_files += index.index.files.len();
        if let Some((_, old)) = self
            .remembered
            .insert(project.to_owned(), (self.clock, index))
        {
            self.remembered_files -= old.index.files.len();
        }
        while self.remembered.len() > 24 || self.remembered_files > 20_000 {
            let Some(oldest) = self
                .remembered
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((_, gone)) = self.remembered.remove(&oldest) {
                self.remembered_files -= gone.index.files.len();
            }
        }
    }
    pub(crate) fn has_changes(&self) -> bool {
        self.stats.as_ref().is_some_and(|s| s.files > 0)
    }
}
fn stats_of(index: &DiffIndex) -> DiffStats {
    DiffStats {
        files: index.files.len() as i64,
        additions: index.additions,
        deletions: index.deletions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::scm::ChangedFile;
    fn file(path: &str, status: &str) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            relative: path.trim_start_matches("/r/").into(),
            status: status.into(),
            additions: 0,
            deletions: 0,
            staged: false,
            unstaged: true,
        }
    }
    #[test]
    fn folders_take_their_most_significant_status() {
        let index = DiffIndex {
            files: vec![
                file("/r/a/b/new.rs", "untracked"),
                file("/r/a/b/old.rs", "modified"),
                file("/r/a/gone.rs", "deleted"),
            ],
            ..Default::default()
        };
        let map = status_maps(&index, "/r");
        assert_eq!(map.dirs["/r/a/b"], "modified");
        assert_eq!(map.dirs["/r/a"], "modified");
        assert!(!map.dirs.contains_key("/r"));
        assert_eq!(map.files["/r/a/gone.rs"], "deleted");
    }
}
