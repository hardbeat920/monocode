//! Port of src/features/files/model/fileWatch.ts: open editors poll the
//! mtime of their file and reload when it changes on disk.
//!
//! `watch_file` returns a `gpui::Subscription` in place of the TypeScript
//! unsubscribe function. Listeners run after the entity update that found
//! the change, so a listener may read or update `FileWatch`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Context, Subscription};

use super::backend::FsBackend;
use crate::workspace::paths::editor_paths_equal;

const MAX_PATHS: usize = 64;

type Listener = Rc<dyn Fn(&mut App)>;

/// Which watched paths a poll stats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollPaths {
    All,
    Some(Vec<String>),
}

#[derive(Default)]
struct Watched {
    /// Watched paths in the order they were first watched, like the keys of
    /// the TypeScript `Map`.
    order: Vec<String>,
    listeners: HashMap<String, Vec<(u64, Listener)>>,
    /// `None` is the TypeScript `undefined`: no baseline yet. `Some(None)`
    /// is a stat that found no file.
    mtimes: HashMap<String, Option<Option<i64>>>,
    unobserved: HashSet<String>,
    next_id: u64,
}

impl Watched {
    fn has(&self, path: &str) -> bool {
        self.listeners.contains_key(path)
    }

    fn listeners_of(&self, path: &str) -> Vec<Listener> {
        self.listeners
            .get(path)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(_, listener)| listener.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Polls the mtime of every open file.
pub struct FileWatch {
    backend: Arc<dyn FsBackend>,
    watched: Rc<RefCell<Watched>>,
    in_flight: bool,
    queued: Option<PollPaths>,
    /// `document.hidden`.
    hidden: bool,
}

impl FileWatch {
    pub fn new(backend: Arc<dyn FsBackend>) -> Self {
        Self {
            backend,
            watched: Rc::default(),
            in_flight: false,
            queued: None,
            hidden: false,
        }
    }

    /// `watchFile`: watch a currently open file and reconcile it after the
    /// first mtime sample. Dropping the subscription stops watching.
    pub fn watch_file(
        &mut self,
        path: &str,
        on_change: impl Fn(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let id = {
            let mut watched = self.watched.borrow_mut();
            if !watched.has(path) {
                watched.order.push(path.to_string());
                watched.listeners.insert(path.to_string(), Vec::new());
                watched.mtimes.insert(path.to_string(), None);
                watched.unobserved.insert(path.to_string());
            }
            watched.next_id += 1;
            let id = watched.next_id;
            if let Some(entries) = watched.listeners.get_mut(path) {
                entries.push((id, Rc::new(on_change)));
            }
            id
        };
        if !self.hidden {
            self.poll(PollPaths::Some(vec![path.to_string()]), cx);
        }
        let watched = Rc::downgrade(&self.watched);
        let path = path.to_string();
        Subscription::new(move || {
            let Some(watched) = watched.upgrade() else {
                return;
            };
            let mut watched = watched.borrow_mut();
            let empty = match watched.listeners.get_mut(&path) {
                Some(entries) => {
                    entries.retain(|(entry, _)| *entry != id);
                    entries.is_empty()
                }
                None => false,
            };
            if !empty {
                return;
            }
            watched.listeners.remove(&path);
            watched.order.retain(|entry| *entry != path);
            watched.mtimes.remove(&path);
            watched.unobserved.remove(&path);
        })
    }

    /// The watched paths, in watch order.
    pub fn watched_paths(&self) -> Vec<String> {
        self.watched.borrow().order.clone()
    }

    /// `nudgeWatchedFiles`: re-stat watched paths now, after an agent edit,
    /// a shell command, or window focus. `None` re-stats all of them.
    pub fn nudge_watched_files(&mut self, paths: Option<&[String]>, cx: &mut Context<Self>) {
        if self.watched.borrow().listeners.is_empty() || self.hidden {
            return;
        }
        let watched = self.matching_paths(paths);
        if !watched.is_empty() {
            self.poll(
                if paths.is_some() {
                    PollPaths::Some(watched)
                } else {
                    PollPaths::All
                },
                cx,
            );
        }
    }

    /// `invalidateWatchedFiles`: reload open editors even when the mtime
    /// looks unchanged. A git restore can rewrite a file in the same second
    /// as the last save.
    pub fn invalidate_watched_files(&mut self, paths: Option<&[String]>, cx: &mut Context<Self>) {
        if self.watched.borrow().listeners.is_empty() {
            return;
        }
        let watched = self.matching_paths(paths);
        let mut notify = Vec::new();
        {
            let mut state = self.watched.borrow_mut();
            for path in &watched {
                state.mtimes.insert(path.clone(), None);
                // The direct notification below already reconciles the
                // editor. The poll only needs a new baseline.
                state.unobserved.remove(path);
                notify.extend(state.listeners_of(path));
            }
        }
        if !notify.is_empty() {
            cx.defer(move |cx| {
                for listener in notify {
                    listener(cx);
                }
            });
        }
        if !watched.is_empty() && !self.hidden {
            self.poll(
                if paths.is_some() {
                    PollPaths::Some(watched)
                } else {
                    PollPaths::All
                },
                cx,
            );
        }
    }

    /// `watchedPaths`: the watched keys that match `paths`, or every key.
    fn matching_paths(&self, paths: Option<&[String]>) -> Vec<String> {
        let state = self.watched.borrow();
        let Some(paths) = paths else {
            return state.order.clone();
        };
        let mut watched: Vec<String> = Vec::new();
        for path in paths {
            for key in &state.order {
                if editor_paths_equal(key, path) && !watched.contains(key) {
                    watched.push(key.clone());
                }
            }
        }
        watched
    }

    /// `syncWatchedMtime`: take the mtime after our own save as the new
    /// baseline so the save does not reload the editor.
    pub fn sync_watched_mtime(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.watched.borrow().has(path) {
            return;
        }
        let stat = cx
            .background_executor()
            .spawn(self.backend.stat_files(vec![path.to_string()]));
        let path = path.to_string();
        cx.spawn(async move |this, cx| {
            // The next nudge retries a failed stat.
            let Ok(stats) = stat.await else {
                return;
            };
            let Some(stat) = stats.into_iter().next() else {
                return;
            };
            this.update(cx, |this, _| {
                let mut state = this.watched.borrow_mut();
                if state.has(&path) {
                    state.mtimes.insert(path.clone(), Some(stat.mtime_ms));
                    state.unobserved.remove(&path);
                }
            })
            .ok();
        })
        .detach();
    }

    /// A window became visible or took focus.
    pub fn window_shown(&mut self, cx: &mut Context<Self>) {
        if !self.hidden {
            self.nudge_watched_files(None, cx);
        }
    }

    /// `document.hidden` changed.
    pub fn set_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        self.hidden = hidden;
        if !hidden {
            self.nudge_watched_files(None, cx);
        }
    }

    /// `poll`.
    fn poll(&mut self, paths: PollPaths, cx: &mut Context<Self>) {
        if self.in_flight {
            self.queued = Some(match (paths, self.queued.take()) {
                (PollPaths::All, _) | (_, Some(PollPaths::All)) => PollPaths::All,
                (PollPaths::Some(paths), None) => PollPaths::Some(dedupe(paths)),
                (PollPaths::Some(paths), Some(PollPaths::Some(mut queued))) => {
                    queued.extend(paths);
                    PollPaths::Some(dedupe(queued))
                }
            });
            return;
        }

        let list: Vec<String> = {
            let state = self.watched.borrow();
            match paths {
                PollPaths::All => state.order.clone(),
                PollPaths::Some(paths) => {
                    paths.into_iter().filter(|path| state.has(path)).collect()
                }
            }
        };
        if list.is_empty() {
            return;
        }

        self.in_flight = true;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            for batch in list.chunks(MAX_PATHS) {
                let stat = cx
                    .background_executor()
                    .spawn(backend.stat_files(batch.to_vec()));
                // The next nudge retries a failed stat.
                let Ok(stats) = stat.await else {
                    break;
                };
                let Ok(notify) = this.update(cx, |this, _| {
                    let mut state = this.watched.borrow_mut();
                    let mut notify = Vec::new();
                    for stat in stats {
                        if !state.has(&stat.path) {
                            continue;
                        }
                        let previous = state.mtimes.get(&stat.path).copied().flatten();
                        let first_observation = state.unobserved.remove(&stat.path);
                        state.mtimes.insert(stat.path.clone(), Some(stat.mtime_ms));
                        // The file may have changed after its first read but
                        // before this baseline. Reconcile on the first sample
                        // so that race cannot leave an editor stale.
                        if !first_observation
                            && previous.is_none_or(|previous| previous == stat.mtime_ms)
                        {
                            continue;
                        }
                        notify.extend(state.listeners_of(&stat.path));
                    }
                    notify
                }) else {
                    return;
                };
                cx.update(|cx| {
                    for listener in notify {
                        listener(cx);
                    }
                });
            }
            this.update(cx, |this, cx| {
                this.in_flight = false;
                if let Some(next) = this.queued.take() {
                    this.poll(next, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

fn dedupe(paths: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::super::backend::fake::FakeFs;
    use super::*;
    use gpui::{AppContext, Entity, TestAppContext};

    const README: &str = "/repo/README.md";

    fn setup(cx: &mut TestAppContext) -> (Arc<FakeFs>, Entity<FileWatch>) {
        let fs = FakeFs::new();
        let watch = cx.new(|_| FileWatch::new(fs.clone()));
        (fs, watch)
    }

    fn watch(
        watch: &Entity<FileWatch>,
        cx: &mut TestAppContext,
    ) -> (Rc<Cell<usize>>, Subscription) {
        let changed = Rc::new(Cell::new(0));
        let count = changed.clone();
        let subscription = watch.update(cx, |watch, cx| {
            watch.watch_file(README, move |_| count.set(count.get() + 1), cx)
        });
        cx.run_until_parked();
        (changed, subscription)
    }

    #[gpui::test]
    fn reconciles_the_file_after_establishing_its_initial_mtime_baseline(cx: &mut TestAppContext) {
        let (fs, entity) = setup(cx);
        fs.push_mtime(README, Some(2));
        let (changed, _stop) = watch(&entity, cx);
        assert_eq!(changed.get(), 1);
    }

    #[gpui::test]
    fn does_not_notify_again_while_the_sampled_mtime_is_unchanged(cx: &mut TestAppContext) {
        let (fs, entity) = setup(cx);
        fs.push_mtime(README, Some(2));
        let (changed, _stop) = watch(&entity, cx);
        assert_eq!(changed.get(), 1);

        entity.update(cx, |watch, cx| {
            watch.nudge_watched_files(Some(&[README.to_string()]), cx)
        });
        cx.run_until_parked();
        assert_eq!(fs.stat_calls().len(), 2);
        assert_eq!(changed.get(), 1);
    }

    #[gpui::test]
    fn notifies_when_a_later_sample_observes_a_different_mtime(cx: &mut TestAppContext) {
        let (fs, entity) = setup(cx);
        fs.push_mtime(README, Some(2));
        fs.push_mtime(README, Some(3));
        let (changed, _stop) = watch(&entity, cx);
        assert_eq!(changed.get(), 1);

        entity.update(cx, |watch, cx| {
            watch.nudge_watched_files(Some(&[README.to_string()]), cx)
        });
        cx.run_until_parked();
        assert_eq!(changed.get(), 2);
    }

    #[gpui::test]
    fn does_not_duplicate_an_invalidation_while_resetting_its_baseline(cx: &mut TestAppContext) {
        let (fs, entity) = setup(cx);
        fs.push_mtime(README, Some(2));
        let (changed, _stop) = watch(&entity, cx);
        assert_eq!(changed.get(), 1);

        entity.update(cx, |watch, cx| {
            watch.invalidate_watched_files(Some(&[README.to_string()]), cx)
        });
        cx.run_until_parked();
        assert_eq!(fs.stat_calls().len(), 2);
        assert_eq!(changed.get(), 2);
    }

    #[gpui::test]
    fn stops_watching_when_the_subscription_drops(cx: &mut TestAppContext) {
        let (fs, entity) = setup(cx);
        fs.push_mtime(README, Some(2));
        let (_, stop) = watch(&entity, cx);
        drop(stop);
        assert!(
            entity
                .read_with(cx, |watch, _| watch.watched_paths())
                .is_empty()
        );
        entity.update(cx, |watch, cx| watch.nudge_watched_files(None, cx));
        cx.run_until_parked();
        assert_eq!(fs.stat_calls().len(), 1);
    }

    #[gpui::test]
    fn matches_watched_paths_by_path_key(cx: &mut TestAppContext) {
        let (fs, entity) = setup(cx);
        fs.push_mtime(README, Some(2));
        fs.push_mtime(README, Some(5));
        let (changed, _stop) = watch(&entity, cx);
        entity.update(cx, |watch, cx| {
            watch.nudge_watched_files(Some(&["/repo/README.md/".to_string()]), cx)
        });
        cx.run_until_parked();
        assert_eq!(changed.get(), 2);
    }
}
