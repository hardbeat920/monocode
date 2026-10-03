//! Port of host/changes.ts.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::js;
use super::protocol::{SessionChange, SessionChanges};

const KEPT: usize = 2_000;
// A streaming turn saves every 120 ms. Waiting briefly after the first change
// lets one response carry a burst instead of one response per save.
const COALESCE: Duration = Duration::from_millis(40);
pub const MAX_WAIT_MS: i64 = 25_000;
/// How often a wait checks whether its desktop left.
const LEFT_POLL: Duration = Duration::from_millis(250);

struct Log {
    cursor: i64,
    entries: std::collections::VecDeque<(i64, SessionChange)>,
    /// Bumped on every write and on `close`, so waiters can tell they were
    /// woken by something new.
    generation: u64,
}

/// In-memory log of session writes, so desktops learn about changes by
/// waiting on one request instead of polling every session. `boot` changes
/// when the host restarts; a desktop holding an old boot or a cursor older
/// than the log receives `reset` and reloads what it shows.
pub struct ChangeFeed {
    pub boot: String,
    log: Mutex<Log>,
    wake: Condvar,
}

impl Default for ChangeFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeFeed {
    pub fn new() -> Self {
        Self {
            boot: uuid::Uuid::new_v4().to_string(),
            log: Mutex::new(Log {
                cursor: 0,
                entries: Default::default(),
                generation: 0,
            }),
            wake: Condvar::new(),
        }
    }

    pub fn record(&self, change: SessionChange) {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        log.cursor += 1;
        let cursor = log.cursor;
        log.entries.push_back((cursor, change));
        while log.entries.len() > KEPT {
            log.entries.pop_front();
        }
        log.generation += 1;
        self.wake.notify_all();
    }

    /// `boot` and `after` are the request's raw values.
    pub fn read(&self, boot: Option<&Value>, after: Option<&Value>) -> SessionChanges {
        let log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        self.read_log(&log, boot, after)
    }

    fn read_log(&self, log: &Log, boot: Option<&Value>, after: Option<&Value>) -> SessionChanges {
        let reset = SessionChanges {
            boot: self.boot.clone(),
            cursor: log.cursor,
            sessions: Vec::new(),
            reset: true,
        };
        if boot.and_then(Value::as_str) != Some(self.boot.as_str()) {
            return reset;
        }
        let Some(after) = js::safe_integer(after) else {
            return reset;
        };
        let oldest = log.entries.front().map(|(cursor, _)| *cursor);
        if after < 0
            || after > log.cursor
            || (after < log.cursor && oldest.is_none_or(|oldest| oldest > after + 1))
        {
            return reset;
        }
        let mut latest: Vec<SessionChange> = Vec::new();
        for (cursor, change) in &log.entries {
            if *cursor <= after {
                continue;
            }
            latest.retain(|seen| seen.id != change.id);
            latest.push(change.clone());
        }
        SessionChanges {
            boot: self.boot.clone(),
            cursor: log.cursor,
            sessions: latest,
            reset: false,
        }
    }

    /// Returns once changes after `after` exist, or with none at the
    /// timeout. `left` ends the wait early, such as when the desktop
    /// disconnects; it is checked a few times a second.
    pub fn wait(
        &self,
        boot: Option<&Value>,
        after: Option<&Value>,
        timeout_ms: i64,
        left: &dyn Fn() -> bool,
    ) -> SessionChanges {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        let now = self.read_log(&log, boot, after);
        if now.reset || !now.sessions.is_empty() {
            return now;
        }
        let start = log.generation;
        let mut deadline =
            Instant::now() + Duration::from_millis(timeout_ms.clamp(0, MAX_WAIT_MS) as u64);
        let mut coalescing = false;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            if !coalescing && log.generation != start {
                coalescing = true;
                deadline = now + COALESCE;
                continue;
            }
            let pause = (deadline - now).min(LEFT_POLL);
            log = self
                .wake
                .wait_timeout(log, pause)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            if !coalescing && log.generation == start && left() {
                break;
            }
        }
        self.read_log(&log, boot, after)
    }

    /// Releases every waiting request, such as when the host stops.
    pub fn close(&self) {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        log.generation += 1;
        self.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn change(id: &str, revision: i64) -> SessionChange {
        SessionChange {
            id: id.into(),
            project_id: "p".into(),
            revision,
            deleted: None,
            status: None,
            busy: None,
        }
    }

    #[test]
    fn resets_a_desktop_from_another_host_run_then_reports_only_newer_writes() {
        let feed = ChangeFeed::new();
        assert_eq!(
            feed.read(None, Some(&json!(0))),
            SessionChanges {
                boot: feed.boot.clone(),
                cursor: 0,
                sessions: vec![],
                reset: true
            }
        );
        feed.record(change("a", 1));
        feed.record(change("b", 1));
        feed.record(change("a", 2));
        let boot = json!(feed.boot);
        assert_eq!(
            feed.read(Some(&boot), Some(&json!(1))),
            SessionChanges {
                boot: feed.boot.clone(),
                cursor: 3,
                sessions: vec![change("b", 1), change("a", 2)],
                reset: false
            }
        );
        assert!(feed.read(Some(&boot), Some(&json!(3))).sessions.is_empty());
        assert!(feed.read(Some(&json!("other boot")), Some(&json!(3))).reset);
        assert!(feed.read(Some(&boot), Some(&json!(4))).reset);
    }

    #[test]
    fn resets_a_cursor_older_than_the_kept_log() {
        let feed = ChangeFeed::new();
        for revision in 1..=2_100 {
            feed.record(change("a", revision));
        }
        let boot = json!(feed.boot);
        assert!(feed.read(Some(&boot), Some(&json!(10))).reset);
        assert_eq!(
            feed.read(Some(&boot), Some(&json!(2_000))).sessions,
            vec![change("a", 2_100)]
        );
    }

    #[test]
    fn wakes_a_waiting_request_once_a_burst_of_writes_settles() {
        let feed = Arc::new(ChangeFeed::new());
        let started = Instant::now();
        let writer = feed.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            writer.record(change("a", 1));
            std::thread::sleep(Duration::from_millis(10));
            writer.record(change("a", 2));
        });
        let boot = json!(feed.boot);
        let result = feed.wait(Some(&boot), Some(&json!(0)), 5_000, &|| false);
        thread.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(result.sessions, vec![change("a", 2)]);
    }

    #[test]
    fn returns_no_changes_at_the_timeout_or_when_the_desktop_leaves() {
        let feed = Arc::new(ChangeFeed::new());
        let boot = json!(feed.boot);
        assert!(
            feed.wait(Some(&boot), Some(&json!(0)), 20, &|| false)
                .sessions
                .is_empty()
        );
        let left = Arc::new(AtomicBool::new(false));
        let flag = left.clone();
        let started = Instant::now();
        let leaver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            flag.store(true, Ordering::SeqCst);
        });
        let result = feed.wait(Some(&boot), Some(&json!(0)), 20_000, &|| {
            left.load(Ordering::SeqCst)
        });
        leaver.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!result.reset);
        assert!(result.sessions.is_empty());
    }

    #[test]
    fn close_releases_waiters() {
        let feed = Arc::new(ChangeFeed::new());
        let closer = feed.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            closer.close();
        });
        let started = Instant::now();
        let boot = json!(feed.boot);
        feed.wait(Some(&boot), Some(&json!(0)), 20_000, &|| false);
        thread.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
