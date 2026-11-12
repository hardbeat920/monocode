//! UI batches stay responsive while the shared JSON transcript is saved at a
//! bounded cadence. Delivery receipts and settled turns always flush immediately.
use super::*;
use std::time::Instant;

// Match the desktop's routine-save window. Accepted messages and settled turns
// still commit immediately, independently of this streaming debounce.
const SHARED_SAVE_INTERVAL: Duration = Duration::from_millis(650);

pub(super) struct PendingSave {
    from: Option<usize>,
    context_dirty: bool,
    last_flush: Instant,
    interval: Duration,
    context_owner: Option<(String, String)>,
}

impl PendingSave {
    pub(super) fn new(shared: bool, now: Instant) -> Self {
        Self {
            from: None,
            context_dirty: false,
            last_flush: now,
            interval: if shared {
                SHARED_SAVE_INTERVAL
            } else {
                Duration::ZERO
            },
            context_owner: None,
        }
    }

    pub(super) fn own_context(&mut self, provider: &str, account: &str) {
        self.context_owner = Some((provider.into(), account.into()));
    }

    pub(super) fn changed(&mut self, row: usize) {
        self.from = Some(self.from.map_or(row, |old| old.min(row)));
    }

    pub(super) fn context_changed(&mut self) {
        self.context_dirty = true;
    }

    pub(super) fn due(&self, boundary: bool, now: Instant) -> bool {
        (self.from.is_some() || self.context_dirty)
            && (boundary || now.saturating_duration_since(self.last_flush) >= self.interval)
    }

    pub(super) fn flush(
        &mut self,
        store: &Database,
        session: i64,
        transcript: &Transcript,
        context: &monocode_core::context::Usage,
        now: Instant,
    ) -> Result<(), String> {
        if self.from.is_none() && !self.context_dirty {
            return Ok(());
        }
        let mut store = locked(store)?;
        if let Some(from) = self.from {
            store.write_rows(session, from, &transcript.rows[from..])?;
        }
        if self.context_dirty {
            let selected = if let Some((provider, account)) = &self.context_owner {
                store
                    .agent_settings(session)?
                    .is_some_and(|settings| settings.account.id == *account)
                    && store
                        .sessions()?
                        .iter()
                        .any(|current| current.id == session && current.provider == *provider)
            } else {
                true
            };
            if selected {
                store.save_context_usage(session, context)?;
            }
        }
        // Keep pending changes after a failed write so final cleanup retries.
        self.from = None;
        self.context_dirty = false;
        self.last_flush = now;
        Ok(())
    }

    pub(super) fn accept(
        &mut self,
        store: &Database,
        session: i64,
        from: usize,
        transcript: &Transcript,
        message: &str,
        now: Instant,
    ) -> Result<(), String> {
        let from = self.from.map_or(from, |old| old.min(from));
        locked(store)?.write_rows_with_message(
            session,
            from,
            &transcript.rows[from..],
            Some(message),
        )?;
        self.from = None;
        self.last_flush = now;
        Ok(())
    }
}

/// A finished turn's rows that the store refused (a full disk, say). The
/// rows stay in memory, retrying the save with backoff, and are journaled to
/// `<database>.gpui-unsaved/<session>.json` so a restart replays them. The
/// next turn of that conversation waits until they're saved, so it can't
/// start from the older history and write over them.
///
/// The limit: if the journal write fails too (the disk is truly full) and
/// Monocode quits before space frees up, the rows existed only in memory and
/// are lost. Everything the store had already committed stays.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Unsaved {
    pub(crate) session: i64,
    /// The turn's first row, its prompt.
    pub(crate) start: usize,
    pub(crate) rows: Vec<Row>,
}

struct Kept {
    unsaved: Unsaved,
    journaled: bool,
    generation: u64,
}

type Key = (usize, i64);
type Registry = (u64, std::collections::HashMap<Key, Kept>);
static KEPT: std::sync::LazyLock<Mutex<Registry>> = std::sync::LazyLock::new(Default::default);
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);

// One app shares one store; a retry thread holds its Arc, so the address
// can't be reused while a turn waits.
fn key(store: &Database, session: i64) -> Key {
    (Arc::as_ptr(store) as *const () as usize, session)
}
fn kept() -> std::sync::MutexGuard<'static, Registry> {
    KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn journal_dir(store: &Store) -> Option<std::path::PathBuf> {
    store.data_dir()?;
    let mut dir = store.database_path().as_os_str().to_owned();
    dir.push(".gpui-unsaved");
    Some(dir.into())
}
fn journal_path(store: &Database, session: i64) -> Option<std::path::PathBuf> {
    journal_dir(&*locked(store).ok()?).map(|dir| dir.join(format!("{session}.json")))
}
fn write_journal(store: &Database, unsaved: &Unsaved) -> Result<(), String> {
    let path = journal_path(store, unsaved.session).ok_or("No data folder for a journal")?;
    let json = serde_json::to_vec(unsaved).map_err(|e| e.to_string())?;
    let partial = path.with_extension("json.partial");
    (|| {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&partial, &json)?;
        std::fs::rename(&partial, &path)
    })()
    .map_err(|error| {
        let _ = std::fs::remove_file(&partial);
        error.to_string()
    })
}
fn register(store: &Database, unsaved: Unsaved, journaled: bool) {
    let mut guard = kept();
    guard.0 += 1;
    let generation = guard.0;
    guard.1.insert(
        key(store, unsaved.session),
        Kept {
            unsaved,
            journaled,
            generation,
        },
    );
}

/// Keeps `unsaved` until a save succeeds; returns whether its journal was
/// written, so it survives a restart.
pub(super) fn keep(store: &Database, unsaved: Unsaved) -> bool {
    let journaled = write_journal(store, &unsaved).is_ok();
    let session = unsaved.session;
    register(store, unsaved, journaled);
    retry_later(store.clone(), session);
    journaled
}

fn retry_later(store: Database, session: i64) {
    std::thread::spawn(move || {
        let mut delay = RETRY_FIRST;
        loop {
            std::thread::sleep(delay);
            let journaled = match kept().1.get(&key(&store, session)) {
                None => return,
                Some(kept) => kept.journaled,
            };
            if save_kept(&store, session).is_ok() {
                continue;
            }
            // Space may free for the small journal before the store.
            if !journaled {
                let unsaved = kept()
                    .1
                    .get(&key(&store, session))
                    .map(|kept| kept.unsaved.clone());
                if let Some(unsaved) = unsaved
                    && write_journal(&store, &unsaved).is_ok()
                    && let Some(kept) = kept().1.get_mut(&key(&store, session))
                {
                    kept.journaled = true;
                }
            }
            delay = (delay * 2).min(RETRY_MAX);
        }
    });
}

/// Saves a turn kept by `keep`, if there is one. A journal whose turn no
/// longer lines up with the saved history (another app continued the
/// conversation) is set aside as `<session>.stale-<time>.json`, not merged.
pub(crate) fn save_kept(store: &Database, session: i64) -> Result<(), String> {
    let Some((unsaved, generation)) = kept()
        .1
        .get(&key(store, session))
        .map(|kept| (kept.unsaved.clone(), kept.generation))
    else {
        return Ok(());
    };
    let fits = {
        let mut db = locked(store)?;
        let fits = lines_up(&db.load(session)?.rows, &unsaved);
        if fits {
            db.write_rows(session, unsaved.start, &unsaved.rows)?;
        }
        fits
    };
    {
        let mut guard = kept();
        if guard
            .1
            .get(&key(store, session))
            .is_some_and(|kept| kept.generation == generation)
        {
            guard.1.remove(&key(store, session));
        }
    }
    if let Some(path) = journal_path(store, session) {
        if fits {
            let _ = std::fs::remove_file(&path);
        } else if path.exists() {
            let stale =
                path.with_extension(format!("stale-{}.json", monocode_core::time::now_ms()));
            let _ = std::fs::rename(&path, stale);
        }
    }
    Ok(())
}

fn lines_up(saved: &[Row], unsaved: &Unsaved) -> bool {
    saved.len() >= unsaved.start
        && saved.len() <= unsaved.start + unsaved.rows.len()
        && saved[unsaved.start..]
            .iter()
            .zip(unsaved.rows.iter().take(1))
            .all(|(saved, kept)| saved.role == kept.role && saved.text == kept.text)
}

/// Shows a kept turn in `rows` just loaded from the store, so reopening the
/// conversation before the save succeeds still shows the reply.
pub(crate) fn show_kept(store: &Database, session: i64, rows: &mut Vec<Row>) {
    if let Some(kept) = kept().1.get(&key(store, session))
        && lines_up(rows, &kept.unsaved)
    {
        rows.truncate(kept.unsaved.start);
        rows.extend(kept.unsaved.rows.iter().cloned());
    }
}

/// At startup, saves the turns journaled by an earlier run, keeping (and
/// retrying) any the store still refuses.
pub(crate) fn recover_kept(store: &Database) {
    let Some(dir) = locked(store).ok().and_then(|db| journal_dir(&db)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(session) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse::<i64>().ok())
        else {
            continue;
        };
        let Some(unsaved) = std::fs::read(&path)
            .ok()
            .and_then(|json| serde_json::from_slice::<Unsaved>(&json).ok())
            .filter(|unsaved| unsaved.session == session)
        else {
            continue;
        };
        register(store, unsaved, true);
        if save_kept(store, session).is_err() {
            retry_later(store.clone(), session);
        }
    }
}

/// Drops every kept turn from memory, as quitting would.
#[cfg(test)]
pub(super) fn forget_kept(store: &Database) {
    let pointer = key(store, 0).0;
    kept().1.retain(|(store, _), _| *store != pointer);
}

pub(super) fn save_turn_receipt(
    store: &Database,
    session: i64,
    turn: usize,
    info: monocode_core::agent::TurnInfo,
) -> Result<(), String> {
    let store = locked(store)?;
    // The picker/composer may have changed during the run. Update only its
    // immutable turn receipts, using the latest settings revision.
    let mut settings = store.agent_settings(session)?.unwrap_or_default();
    settings.turns.insert(turn, info);
    store.save_agent_settings(session, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::follow_up::{Message, State};

    fn conversation() -> (Database, i64, Transcript) {
        let mut store = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project").unwrap();
        let mut transcript = Transcript::default();
        transcript.begin("user");
        transcript.append("Prompt");
        transcript.begin("assistant");
        store.write_rows(session.id, 0, &transcript.rows).unwrap();
        (Arc::new(Mutex::new(store)), session.id, transcript)
    }

    #[test]
    fn token_batches_coalesce_but_final_stop_flush_preserves_all_pending_rows() {
        let (store, id, mut transcript) = conversation();
        let now = Instant::now();
        let mut pending = PendingSave::new(true, now);
        for text in ["One ", "two ", "three"] {
            pending.changed(transcript.append(text));
        }
        assert!(!pending.due(false, now + Duration::from_millis(200)));
        assert!(!pending.due(false, now + Duration::from_millis(600)));
        assert!(pending.due(true, now + Duration::from_millis(200)));
        assert!(pending.due(false, now + SHARED_SAVE_INTERVAL));
        assert!(
            locked(&store).unwrap().load(id).unwrap().rows[1]
                .text
                .is_empty()
        );
        let stopped = transcript.begin("error");
        transcript.append("Stopped");
        pending.changed(stopped);
        pending
            .flush(&store, id, &transcript, &Default::default(), now)
            .unwrap();
        assert_eq!(
            locked(&store).unwrap().load(id).unwrap().rows,
            transcript.rows
        );
        assert!(!pending.due(true, now));
    }

    #[test]
    fn accepted_queue_message_flushes_prior_tokens_and_its_receipt_atomically() {
        let (store, id, mut transcript) = conversation();
        locked(&store)
            .unwrap()
            .save_follow_ups(
                id,
                &State {
                    revision: 1,
                    messages: vec![Message {
                        id: "queued".into(),
                        text: "Next".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
        let now = Instant::now();
        let mut pending = PendingSave::new(true, now);
        pending.changed(transcript.append("Still streaming"));
        let from = transcript.begin("user");
        transcript.append("Next");
        pending
            .accept(&store, id, from, &transcript, "queued", now)
            .unwrap();
        let store = locked(&store).unwrap();
        assert_eq!(store.load(id).unwrap().rows, transcript.rows);
        assert!(store.follow_ups(id).unwrap().messages.is_empty());
        assert_eq!(
            store
                .setting(&format!("mono-delivered:{id}:queued"))
                .unwrap(),
            Some("1".into())
        );
    }

    #[test]
    fn a_journaled_turn_is_saved_by_the_next_run() {
        let dir = std::env::temp_dir().join(format!(
            "monocode-kept-turn-{}",
            monocode_core::mono::new_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.db");
        let mut store = Store::open_monocode(&path).unwrap();
        let session = store.create("codex", "/project").unwrap().id;
        let mut transcript = Transcript::default();
        transcript.begin("user");
        transcript.append("Prompt");
        transcript.begin("assistant");
        store.write_rows(session, 0, &transcript.rows).unwrap();
        let store: Database = Arc::new(Mutex::new(store));
        let full = rusqlite::Connection::open(&path).unwrap();
        full.execute_batch(
            "CREATE TRIGGER test_full BEFORE UPDATE ON sessions
             BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
        )
        .unwrap();
        transcript.append("The answer");
        let unsaved = Unsaved {
            session,
            start: 0,
            rows: transcript.rows.clone(),
        };
        assert!(keep(&store, unsaved), "journaled");
        assert!(save_kept(&store, session).is_err());
        // Quit while the disk is still full.
        forget_kept(&store);
        drop(store);
        full.execute_batch("DROP TRIGGER test_full;").unwrap();

        let store: Database = Arc::new(Mutex::new(Store::open_monocode(&path).unwrap()));
        assert_ne!(
            locked(&store).unwrap().load(session).unwrap().rows,
            transcript.rows
        );
        recover_kept(&store);
        assert_eq!(
            locked(&store).unwrap().load(session).unwrap().rows,
            transcript.rows
        );
        let journal = dir.join("sessions.db.gpui-unsaved");
        assert_eq!(std::fs::read_dir(&journal).unwrap().count(), 0);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_kept_turn_that_no_longer_lines_up_is_set_aside_not_merged() {
        let (store, id, transcript) = conversation();
        let mut kept_rows = transcript.rows.clone();
        kept_rows[0].text = "Another prompt".into();
        register(
            &store,
            Unsaved {
                session: id,
                start: 0,
                rows: kept_rows,
            },
            false,
        );
        save_kept(&store, id).unwrap();
        assert_eq!(
            locked(&store).unwrap().load(id).unwrap().rows,
            transcript.rows
        );
        assert!(save_kept(&store, id).is_ok(), "and is no longer kept");
    }

    #[test]
    fn legacy_row_store_keeps_incremental_save_behavior() {
        let now = Instant::now();
        let mut pending = PendingSave::new(false, now);
        pending.changed(0);
        assert!(pending.due(false, now));
    }

    #[test]
    fn settled_turn_receipt_persists_without_a_view_and_keeps_newer_picker_settings() {
        let (store, id, transcript) = conversation();
        let mut settings = locked(&store).unwrap().agent_settings(id).unwrap().unwrap();
        settings.revision = 42;
        settings.model = Some("next-turn-model".into());
        locked(&store)
            .unwrap()
            .save_agent_settings(id, &settings)
            .unwrap();
        let started_at = transcript.rows[0].sent_at.unwrap();
        save_turn_receipt(
            &store,
            id,
            1,
            monocode_core::agent::TurnInfo {
                provider: "codex".into(),
                model: "Turn Model".into(),
                model_id: Some("turn-model".into()),
                provider_turn_id: Some("turn-id".into()),
                started_at: Some(started_at),
                duration_ms: 9000,
                duration_known: true,
                completed_at: Some(started_at + 9000),
                metrics: Some(monocode_core::agent::TurnMetrics {
                    output_tokens: 5,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let store = locked(&store).unwrap();
        let saved = store.agent_settings(id).unwrap().unwrap();
        assert_eq!(saved.revision, 42);
        assert_eq!(saved.model.as_deref(), Some("next-turn-model"));
        assert_eq!(saved.turns[&1].model, "Turn Model");
        assert_eq!(saved.turns[&1].duration_ms, 9000);
        assert_eq!(saved.turns[&1].provider_turn_id.as_deref(), Some("turn-id"));
        assert_eq!(store.load(id).unwrap().rows, transcript.rows);
    }

    #[test]
    fn cancelled_initial_delivery_replays_accepted_rows_in_the_final_update() {
        let (store, id, history) = conversation();
        let mut summary = locked(&store).unwrap().sessions().unwrap().pop().unwrap();
        // Even a regression in cancellation must never launch a real provider.
        summary.provider = "invalid-test-provider".into();
        let (tx, rx) = async_channel::bounded(8);
        let (_control, controls) = std::sync::mpsc::channel();
        run_turn(
            summary,
            "Accepted before stopping".into(),
            Settings::default(),
            controls,
            store.clone(),
            Arc::new(AtomicBool::new(true)),
            tx,
        );
        let result = rx.try_recv().unwrap();
        assert!(result.finished);
        assert_eq!(result.status, "Ready");
        assert_eq!(result.start, history.rows.len());
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0].text, "Accepted before stopping");
        assert_eq!(result.rows[1].role, "assistant");
        let mut visible = history.rows;
        visible.extend(result.rows);
        assert_eq!(visible, locked(&store).unwrap().load(id).unwrap().rows);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn rejected_preflight_preserves_existing_history_without_a_provider_launch() {
        let (store, id, transcript) = conversation();
        let summary = locked(&store).unwrap().sessions().unwrap().pop().unwrap();
        let (tx, rx) = async_channel::bounded(2);
        let (_control, controls) = std::sync::mpsc::channel();
        run_turn(
            summary,
            "Unsent".into(),
            Settings {
                worktree_removed: true,
                mono_message_id: Some("not-accepted".into()),
                ..Default::default()
            },
            controls,
            store.clone(),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let result = rx.try_recv().unwrap();
        assert!(result.finished);
        assert!(result.status.contains("worktree"));
        assert!(result.rows.is_empty());
        assert!(
            result.delivered.is_none(),
            "Rejected messages must stay queued"
        );
        assert_eq!(
            locked(&store).unwrap().load(id).unwrap().rows,
            transcript.rows
        );
    }

    #[test]
    fn worktree_creation_follows_message_acceptance_and_reports_the_checkout_in_the_final_update() {
        let root = std::env::temp_dir().join(format!(
            "monocode-optimistic-worktree-{}",
            monocode_core::mono::new_id()
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "-q"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-qm",
                "Initial",
            ],
        ] {
            let result = std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut summary = store.create("codex", repo.to_str().unwrap()).unwrap();
        summary.provider = "invalid-test-provider".into();
        let id = summary.id;
        let store = Arc::new(Mutex::new(store));
        let (tx, rx) = async_channel::bounded(8);
        let (_control, controls) = std::sync::mpsc::channel();
        run_turn(
            summary,
            "Create the checkout".into(),
            Settings {
                new_worktree: true,
                ..Default::default()
            },
            controls,
            store.clone(),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let first = rx.try_recv().unwrap();
        assert!(!first.finished);
        assert_eq!(first.status, "Creating worktree…");
        assert_eq!(first.rows[0].text, "Create the checkout");
        assert!(first.worktree_cwd.is_none());
        let created = rx.try_recv().unwrap();
        let cwd = created.worktree_cwd.unwrap();
        assert!(std::path::Path::new(&cwd).is_dir());
        let finished = rx.try_recv().unwrap();
        assert!(finished.finished);
        assert_eq!(finished.worktree_cwd.as_deref(), Some(cwd.as_str()));
        let store = locked(&store).unwrap();
        let settings = store.agent_settings(id).unwrap().unwrap();
        assert!(!settings.new_worktree);
        assert_eq!(settings.worktree_cwd.as_deref(), Some(cwd.as_str()));
        let rows = store.load(id).unwrap().rows;
        assert_eq!(
            rows.iter()
                .filter(|row| row.first && row.is_prompt())
                .count(),
            1
        );
        assert_eq!(rows[0].text, "Create the checkout");
        std::fs::remove_dir_all(root).unwrap();
    }
}
