//! Port of host/store.ts.
//!
//! The host's SQLite database, `<data dir>/host.db`, with the schema the
//! TypeScript host created, so an existing data directory keeps its
//! sessions, paired devices, and environment ID.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use monocode_core::BlockRole;
use monocode_core::session::{LinkedWorkItem, session_needs_input};
use parking_lot::ReentrantMutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::changes::ChangeFeed;
use super::protocol::{
    CommandReceipt, HostProject, HostSession, HostSessionStatus, HostSessionSummary, SessionChange,
};
use super::without_key::WithoutKey;

const CACHED_SESSIONS: usize = 32;
/// How long a pairing link from `connect` can be redeemed.
pub const PAIRING_TTL_MS: i64 = 15 * 60_000;

/// A device credential. The host keeps only the token's hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssuedDevice {
    pub id: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub code: String,
    pub expires_at: i64,
}

/// The fields `sessions.update` may change. `linked_work_item` is
/// `Some(None)` to clear the item.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_work_item: Option<Option<LinkedWorkItem>>,
}

impl SessionPatch {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.archived.is_none()
            && self.pinned.is_none()
            && self.linked_work_item.is_none()
    }
}

/// What `sessions.sync` answers, holding the saved value instead of a copy.
/// It serializes as the protocol's `SessionSync`.
#[derive(Debug, Clone)]
pub enum StoredSync {
    Unchanged { revision: i64 },
    Snapshot(Arc<HostSession>),
    Delta { base: i64, value: Arc<HostSession> },
}

impl Serialize for StoredSync {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind")]
        enum Wire<'a> {
            #[serde(rename = "unchanged")]
            Unchanged { revision: i64 },
            #[serde(rename = "snapshot")]
            Snapshot { value: WithoutKey<'a, HostSession> },
            #[serde(rename = "delta", rename_all = "camelCase")]
            Delta {
                base: i64,
                value: DeltaValue<'a>,
                block_ids: Vec<&'a str>,
                blocks: Vec<&'a monocode_core::Block>,
            },
        }
        struct DeltaValue<'a>(&'a HostSession);
        impl Serialize for DeltaValue<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Value<'a> {
                    session: WithoutKey<'a, monocode_core::Session>,
                    project_id: &'a str,
                    revision: i64,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    run_id: &'a Option<String>,
                    status: HostSessionStatus,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    created_at: Option<i64>,
                    updated_at: i64,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    archived: Option<bool>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    pinned: Option<bool>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    auto_worktree_branch: &'a Option<String>,
                    #[serde(flatten)]
                    extra: &'a monocode_core::Extra,
                }
                let value = self.0;
                Value {
                    session: WithoutKey {
                        value: &value.session,
                        key: "blocks",
                    },
                    project_id: &value.project_id,
                    revision: value.revision,
                    run_id: &value.run_id,
                    status: value.status,
                    created_at: value.created_at,
                    updated_at: value.updated_at,
                    archived: value.archived,
                    pinned: value.pinned,
                    auto_worktree_branch: &value.auto_worktree_branch,
                    extra: &value.extra,
                }
                .serialize(serializer)
            }
        }
        match self {
            Self::Unchanged { revision } => Wire::Unchanged {
                revision: *revision,
            }
            .serialize(serializer),
            Self::Snapshot(value) => Wire::Snapshot {
                value: WithoutKey {
                    value: value.as_ref(),
                    key: "blockRevisions",
                },
            }
            .serialize(serializer),
            Self::Delta { base, value } => {
                let revisions = value.block_revisions.as_ref();
                Wire::Delta {
                    base: *base,
                    value: DeltaValue(value),
                    block_ids: value
                        .session
                        .blocks
                        .iter()
                        .map(|block| block.id.as_str())
                        .collect(),
                    blocks: value
                        .session
                        .blocks
                        .iter()
                        .filter(|block| {
                            revisions
                                .and_then(|revisions| revisions.get(&block.id))
                                .copied()
                                .unwrap_or(value.revision)
                                > *base
                        })
                        .collect(),
                }
                .serialize(serializer)
            }
        }
    }
}

/// One entry of `events.read`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredEvent {
    pub revision: i64,
    pub event: Value,
}

/// `events.read`: the events after a cursor, or a snapshot when the journal
/// no longer covers it.
#[derive(Debug, Clone, Serialize)]
pub struct StoredEvents {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Arc<HostSession>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<StoredEvent>>,
    pub revision: i64,
}

struct Inner {
    db: RefCell<Option<Connection>>,
    // This process is the only session writer, so recently used snapshots are
    // served from memory instead of re-parsing whole transcripts.
    cache: RefCell<VecDeque<(String, Arc<HostSession>)>>,
}

pub struct HostStore {
    inner: ReentrantMutex<Inner>,
    pub environment_id: String,
    pub attachment_dir: PathBuf,
    /// Session writes by this process, for `changes.wait`.
    pub changes: ChangeFeed,
    #[cfg(test)]
    pub(crate) auth_checks: std::sync::atomic::AtomicUsize,
}

fn sql(error: rusqlite::Error) -> String {
    error.to_string()
}

/// 32 random bytes as base64url, like `randomBytes(32).toString("base64url")`.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .expect("the system random source failed");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl HostStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let attachment_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("attachments");
        let db = Connection::open(path).map_err(sql)?;
        db.execute_batch(
            "PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, cwd TEXT NOT NULL UNIQUE, name TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id), snapshot TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS receipts (id TEXT PRIMARY KEY, signature TEXT NOT NULL, receipt TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS events (session_id TEXT NOT NULL REFERENCES sessions(id), revision INTEGER NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(session_id, revision));
      CREATE TABLE IF NOT EXISTS devices (id TEXT PRIMARY KEY, name TEXT NOT NULL, hash TEXT NOT NULL UNIQUE);
      CREATE TABLE IF NOT EXISTS pairings (hash TEXT PRIMARY KEY, expires_at INTEGER NOT NULL);",
        )
        .map_err(sql)?;
        let has_summary = {
            let mut statement = db.prepare("PRAGMA table_info(sessions)").map_err(sql)?;
            let names = statement
                .query_map([], |row| row.get::<_, String>("name"))
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            names.iter().any(|name| name == "summary")
        };
        if !has_summary {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN summary TEXT")
                .map_err(sql)?;
        }
        db.execute(
            "INSERT OR IGNORE INTO metadata VALUES ('environmentId', ?)",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(sql)?;
        let environment_id: String = db
            .query_row(
                "SELECT value FROM metadata WHERE key='environmentId'",
                [],
                |row| row.get(0),
            )
            .map_err(sql)?;
        Ok(Self {
            inner: ReentrantMutex::new(Inner {
                db: RefCell::new(Some(db)),
                cache: RefCell::new(VecDeque::new()),
            }),
            environment_id,
            attachment_dir,
            changes: ChangeFeed::new(),
            #[cfg(test)]
            auth_checks: Default::default(),
        })
    }

    fn with_db<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let inner = self.inner.lock();
        let db = inner.db.borrow();
        let db = db.as_ref().ok_or("The host store is closed")?;
        f(db).map_err(sql)
    }

    fn with_cache<T>(&self, f: impl FnOnce(&mut VecDeque<(String, Arc<HostSession>)>) -> T) -> T {
        let inner = self.inner.lock();
        let mut cache = inner.cache.borrow_mut();
        f(&mut cache)
    }

    /// Runs `f` in one SQLite transaction. Other threads wait until it ends,
    /// as they would have on the TypeScript host's single thread. `f` may
    /// call any other store method.
    pub fn transaction<T>(&self, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        struct Rollback<'a> {
            store: &'a HostStore,
            armed: bool,
        }
        impl Drop for Rollback<'_> {
            fn drop(&mut self) {
                if !self.armed {
                    return;
                }
                self.store.with_cache(|cache| cache.clear());
                if let Err(error) = self.store.with_db(|db| db.execute_batch("ROLLBACK")) {
                    eprintln!("Could not roll back host transaction: {error}");
                }
            }
        }
        let _guard = self.inner.lock();
        self.with_db(|db| db.execute_batch("BEGIN IMMEDIATE"))?;
        let mut rollback = Rollback {
            store: self,
            armed: true,
        };
        let value = f()?;
        self.with_db(|db| db.execute_batch("COMMIT"))?;
        rollback.armed = false;
        Ok(value)
    }

    pub fn project(&self, id: &str) -> Result<HostProject, String> {
        self.with_db(|db| {
            db.query_row("SELECT * FROM projects WHERE id=?", [id], project_row)
                .optional()
        })?
        .ok_or_else(|| "Project is not registered on this machine".into())
    }

    pub fn projects(&self) -> Result<Vec<HostProject>, String> {
        self.with_db(|db| {
            db.prepare("SELECT * FROM projects ORDER BY name")?
                .query_map([], project_row)?
                .collect()
        })
    }

    pub fn add_project(&self, cwd: &str, name: &str) -> Result<HostProject, String> {
        self.with_db(|db| {
            db.execute(
                "INSERT OR IGNORE INTO projects VALUES (?, ?, ?)",
                params![uuid::Uuid::new_v4().to_string(), cwd, name],
            )?;
            db.query_row("SELECT * FROM projects WHERE cwd=?", [cwd], project_row)
        })
    }

    fn remember(&self, value: Arc<HostSession>) -> Arc<HostSession> {
        self.with_cache(|cache| {
            cache.retain(|(id, _)| *id != value.session.id);
            cache.push_back((value.session.id.clone(), value.clone()));
            if cache.len() > CACHED_SESSIONS {
                cache.pop_front();
            }
        });
        value
    }

    fn find(&self, id: &str) -> Result<Option<Arc<HostSession>>, String> {
        let _guard = self.inner.lock();
        let cached = self.with_cache(|cache| {
            cache
                .iter()
                .find(|(cached, _)| cached == id)
                .map(|(_, value)| value.clone())
        });
        if let Some(cached) = cached {
            return Ok(Some(self.remember(cached)));
        }
        let snapshot: Option<String> = self.with_db(|db| {
            db.query_row("SELECT snapshot FROM sessions WHERE id=?", [id], |row| {
                row.get(0)
            })
            .optional()
        })?;
        match snapshot {
            Some(snapshot) => {
                let value: HostSession =
                    serde_json::from_str(&snapshot).map_err(|error| error.to_string())?;
                Ok(Some(self.remember(Arc::new(value))))
            }
            None => Ok(None),
        }
    }

    /// Callers must treat the returned value as immutable; save a changed
    /// copy instead.
    pub fn session(&self, id: &str) -> Result<Arc<HostSession>, String> {
        self.find(id)?
            .ok_or_else(|| "Session not found on this machine".into())
    }

    pub fn summaries(&self, project_id: &str) -> Result<Vec<HostSessionSummary>, String> {
        let _guard = self.inner.lock();
        let rows: Vec<(String, Option<String>)> = self.with_db(|db| {
            db.prepare("SELECT id, summary FROM sessions WHERE project_id=?")?
                .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect()
        })?;
        let mut summaries = Vec::with_capacity(rows.len());
        for (id, cached) in rows {
            if let Some(cached) = cached.as_deref().and_then(complete_summary) {
                summaries.push(cached);
                continue;
            }
            let fresh = summary(&*self.session(&id)?);
            let text = serde_json::to_string(&fresh).map_err(|error| error.to_string())?;
            self.with_db(|db| {
                db.execute(
                    "UPDATE sessions SET summary=? WHERE id=?",
                    params![text, id],
                )
            })?;
            summaries.push(fresh);
        }
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
        Ok(summaries)
    }

    pub fn sync(&self, id: &str, revision: Option<i64>) -> Result<StoredSync, String> {
        let value = self.session(id)?;
        if revision == Some(value.revision) {
            return Ok(StoredSync::Unchanged {
                revision: value.revision,
            });
        }
        match revision {
            Some(revision) if revision <= value.revision && value.block_revisions.is_some() => {
                Ok(StoredSync::Delta {
                    base: revision,
                    value,
                })
            }
            _ => Ok(StoredSync::Snapshot(value)),
        }
    }

    pub fn sessions(&self, project_id: Option<&str>) -> Result<Vec<HostSession>, String> {
        let rows: Vec<String> = self.with_db(|db| match project_id {
            Some(project_id) => db
                .prepare("SELECT snapshot FROM sessions WHERE project_id=?")?
                .query_map([project_id], |row| row.get(0))?
                .collect(),
            None => db
                .prepare("SELECT snapshot FROM sessions")?
                .query_map([], |row| row.get(0))?
                .collect(),
        })?;
        let mut sessions = rows
            .iter()
            .map(|row| serde_json::from_str::<HostSession>(row).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        Ok(sessions)
    }

    /// Returns the saved value, stamped with per-block change revisions.
    pub fn save(&self, input: HostSession, event: &Value) -> Result<Arc<HostSession>, String> {
        let _guard = self.inner.lock();
        let previous = self.find(&input.session.id)?;
        let mut value = input;
        value.created_at = value
            .created_at
            .or_else(|| previous.as_ref().and_then(|p| p.created_at))
            .or_else(|| previous.as_ref().map(|p| p.updated_at))
            .or(Some(value.updated_at));
        value.block_revisions = Some(block_revisions(previous.as_deref(), &value));
        let snapshot = serde_json::to_string(&value).map_err(|error| error.to_string())?;
        let summary = serde_json::to_string(&summary(&value)).map_err(|error| error.to_string())?;
        let event = serde_json::to_string(event).map_err(|error| error.to_string())?;
        self.with_db(|db| {
            db.execute(
                "INSERT INTO sessions (id, project_id, snapshot, summary) VALUES (?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET snapshot=excluded.snapshot, summary=excluded.summary",
                params![value.session.id, value.project_id, snapshot, summary],
            )?;
            db.execute(
                "INSERT INTO events VALUES (?, ?, ?)",
                params![value.session.id, value.revision, event],
            )?;
            db.execute(
                "DELETE FROM events WHERE session_id=? AND revision<?",
                params![value.session.id, value.revision - 2_000],
            )
        })?;
        self.changes.record(SessionChange {
            id: value.session.id.clone(),
            project_id: value.project_id.clone(),
            revision: value.revision,
            deleted: None,
            status: Some(value.status),
            busy: Some(value.session.busy == Some(true)),
        });
        Ok(self.remember(Arc::new(value)))
    }

    pub fn update_session(
        &self,
        id: &str,
        patch: &SessionPatch,
    ) -> Result<HostSessionSummary, String> {
        self.transaction(|| {
            let current = self.session(id)?;
            if let Some(title) = &patch.title
                && (monocode_core::js::trim(title).is_empty()
                    || monocode_core::js::len(title) > 200)
            {
                return Err("Invalid session title".into());
            }
            let mut next = (*current).clone();
            next.revision = current.revision + 1;
            next.archived = patch.archived.or(current.archived);
            next.pinned = patch.pinned.or(current.pinned);
            if let Some(title) = &patch.title {
                next.session.title = monocode_core::js::trim(title).to_string();
            }
            if let Some(item) = &patch.linked_work_item {
                next.session.linked_work_item = item.clone();
            }
            let event = serde_json::json!({ "type": "session.metadata", "patch": patch });
            let next = self.save(next, &event)?;
            Ok(summary(&next))
        })
    }

    pub fn delete_session(&self, id: &str) -> Result<(), String> {
        self.transaction(|| {
            let current = self.session(id)?;
            if current.status == HostSessionStatus::Running {
                return Err("Stop this session before deleting it".into());
            }
            self.with_db(|db| {
                db.execute("DELETE FROM events WHERE session_id=?", [id])?;
                db.execute("DELETE FROM sessions WHERE id=?", [id])
            })?;
            self.with_cache(|cache| cache.retain(|(cached, _)| cached != id));
            self.changes.record(SessionChange {
                id: id.into(),
                project_id: current.project_id.clone(),
                revision: current.revision,
                deleted: Some(true),
                status: Some(current.status),
                busy: Some(false),
            });
            Ok(())
        })
    }

    pub fn receipt(&self, id: &str, signature: &str) -> Result<Option<CommandReceipt>, String> {
        let row: Option<(String, String)> = self.with_db(|db| {
            db.query_row(
                "SELECT signature, receipt FROM receipts WHERE id=?",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })?;
        let Some((saved, receipt)) = row else {
            return Ok(None);
        };
        if saved != signature {
            return Err("Command ID was already used with a different payload".into());
        }
        serde_json::from_str(&receipt)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    pub fn record_receipt(&self, signature: &str, receipt: &CommandReceipt) -> Result<(), String> {
        let text = serde_json::to_string(receipt).map_err(|error| error.to_string())?;
        self.with_db(|db| {
            db.execute(
                "INSERT INTO receipts VALUES (?, ?, ?)",
                params![receipt.command_id, signature, text],
            )
        })?;
        Ok(())
    }

    pub fn events(&self, id: &str, after: i64) -> Result<StoredEvents, String> {
        let snapshot = self.session(id)?;
        let rows: Vec<(i64, String)> = self.with_db(|db| {
            db.prepare(
                "SELECT revision, payload FROM events WHERE session_id=? AND revision>? ORDER BY revision",
            )?
            .query_map(params![id, after], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
        })?;
        if after > snapshot.revision
            || (after < snapshot.revision && rows.first().map(|row| row.0) != Some(after + 1))
        {
            let revision = snapshot.revision;
            return Ok(StoredEvents {
                snapshot: Some(snapshot),
                events: None,
                revision,
            });
        }
        let events = rows
            .into_iter()
            .map(|(revision, payload)| {
                Ok(StoredEvent {
                    revision,
                    event: serde_json::from_str(&payload).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(StoredEvents {
            snapshot: None,
            events: Some(events),
            revision: snapshot.revision,
        })
    }

    pub fn issue_device(&self, name: &str) -> Result<IssuedDevice, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let token = random_token();
        self.with_db(|db| {
            db.execute(
                "INSERT INTO devices VALUES (?, ?, ?)",
                params![id, name, hash(&token)],
            )
        })?;
        Ok(IssuedDevice { id, token })
    }

    /// A one-time code that a desktop exchanges for a device credential.
    pub fn issue_pairing(&self, now: i64) -> Result<Pairing, String> {
        let code = random_token();
        let expires_at = now + PAIRING_TTL_MS;
        self.with_db(|db| {
            db.execute("DELETE FROM pairings WHERE expires_at<=?", [now])?;
            db.execute(
                "INSERT INTO pairings VALUES (?, ?)",
                params![hash(&code), expires_at],
            )
        })?;
        Ok(Pairing { code, expires_at })
    }

    /// Consumes a pairing code. Returns `None` for an unknown, used, or
    /// expired code.
    pub fn redeem_pairing(
        &self,
        code: &str,
        device_name: &str,
        now: i64,
    ) -> Result<Option<IssuedDevice>, String> {
        self.transaction(|| {
            let removed = self.with_db(|db| {
                db.execute(
                    "DELETE FROM pairings WHERE hash=? AND expires_at>?",
                    params![hash(code), now],
                )
            })?;
            if removed > 0 {
                self.issue_device(device_name).map(Some)
            } else {
                Ok(None)
            }
        })
    }

    /// Sessions with a turn in progress, from the saved summaries.
    pub fn running_turns(&self) -> Result<i64, String> {
        self.with_db(|db| {
            db.query_row(
                "SELECT count(*) AS n FROM sessions WHERE json_extract(summary, '$.status')='running'",
                [],
                |row| row.get(0),
            )
        })
    }

    pub fn pending_pairings(&self, now: i64) -> Result<i64, String> {
        self.with_db(|db| {
            db.query_row(
                "SELECT count(*) AS n FROM pairings WHERE expires_at>?",
                [now],
                |row| row.get(0),
            )
        })
    }

    pub fn devices(&self) -> Result<Vec<Device>, String> {
        self.with_db(|db| {
            db.prepare("SELECT id, name FROM devices ORDER BY name")?
                .query_map([], |row| {
                    Ok(Device {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                })?
                .collect()
        })
    }

    pub fn revoke_device(&self, id: &str) -> Result<bool, String> {
        Ok(self.with_db(|db| db.execute("DELETE FROM devices WHERE id=?", [id]))? > 0)
    }

    /// Lets a desktop revoke only the credential it is using.
    pub fn revoke_token(&self, token: &str) -> Result<bool, String> {
        Ok(self.with_db(|db| db.execute("DELETE FROM devices WHERE hash=?", [hash(token)]))? > 0)
    }

    pub fn authenticated(&self, token: &str) -> Result<bool, String> {
        #[cfg(test)]
        self.auth_checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.with_db(|db| {
            db.query_row("SELECT id FROM devices WHERE hash=?", [hash(token)], |_| {
                Ok(())
            })
            .optional()
        })
        .map(|row| row.is_some())
    }

    /// Runs a statement directly, for tests and maintenance.
    pub fn execute(
        &self,
        statement: &str,
        values: &[&dyn rusqlite::ToSql],
    ) -> Result<usize, String> {
        self.with_db(|db| db.execute(statement, values))
    }

    /// Closes the database. Later calls fail.
    pub fn close(&self) {
        let inner = self.inner.lock();
        inner.db.borrow_mut().take();
        inner.cache.borrow_mut().clear();
    }
}

fn project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HostProject> {
    Ok(HostProject {
        id: row.get("id")?,
        cwd: row.get("cwd")?,
        name: row.get("name")?,
    })
}

/// A cached summary written by this or a recent host version. Older ones
/// lack fields and are rebuilt from the snapshot.
fn complete_summary(text: &str) -> Option<HostSessionSummary> {
    let value: Value = serde_json::from_str(text).ok()?;
    let fields = value.as_object()?;
    if !super::js::truthy(fields.get("model"))
        || !fields.contains_key("needsInput")
        || !fields.contains_key("providerSessionId")
    {
        return None;
    }
    serde_json::from_value(value).ok()
}

pub fn summary(value: &HostSession) -> HostSessionSummary {
    let session = &value.session;
    HostSessionSummary {
        project_id: value.project_id.clone(),
        revision: value.revision,
        run_id: value.run_id.clone(),
        status: value.status,
        updated_at: value.updated_at,
        id: session.id.clone(),
        cwd: Some(session.cwd.clone()),
        title: session.title.clone(),
        harness: session.harness,
        model: Some(session.model.clone()),
        runtime_mode: Some(session.runtime_mode),
        provider_session_id: session.provider_session_id.clone(),
        created_at: Some(value.created_at.unwrap_or(value.updated_at)),
        archived: value.archived,
        pinned: value.pinned,
        auto_worktree_branch: None,
        linked_work_item: session.linked_work_item.clone(),
        needs_input: Some(session_needs_input(session)),
        branch: None,
        worktree_cwd: None,
        repo: None,
        draft: Some(
            session
                .blocks
                .iter()
                .any(|block| block.role == BlockRole::User && block.is_draft()),
        ),
        extra: Default::default(),
    }
}

/// Unchanged blocks keep their previous stamp.
pub fn block_revisions(
    previous: Option<&HostSession>,
    next: &HostSession,
) -> BTreeMap<String, i64> {
    let before: std::collections::HashMap<&str, &monocode_core::Block> = previous
        .map(|previous| {
            previous
                .session
                .blocks
                .iter()
                .map(|block| (block.id.as_str(), block))
                .collect()
        })
        .unwrap_or_default();
    let stamps = previous.and_then(|previous| previous.block_revisions.as_ref());
    next.session
        .blocks
        .iter()
        .map(|block| {
            let stamp = stamps.and_then(|stamps| stamps.get(&block.id)).copied();
            let unchanged = before
                .get(block.id.as_str())
                .is_some_and(|old| *old == block);
            let revision = match stamp {
                Some(stamp) if unchanged => stamp,
                _ => next.revision,
            };
            (block.id.clone(), revision)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn temporary(prefix: &str) -> tempfile::TempDir {
        tempfile::Builder::new().prefix(prefix).tempdir().unwrap()
    }

    pub(crate) fn host_session(project_id: &str, id: &str, revision: i64) -> HostSession {
        serde_json::from_value(json!({
            "projectId": project_id,
            "revision": revision,
            "status": "idle",
            "updatedAt": 1,
            "session": {
                "id": id,
                "title": "Demo",
                "harness": "codex",
                "model": "codex:test",
                "modelSettings": {},
                "runtimeMode": "supervised",
                "cwd": "/repo",
                "busy": false,
                "blocks": []
            }
        }))
        .unwrap()
    }

    #[test]
    fn records_saves_and_deletions_from_the_store() {
        let directory = temporary("monocode-changes-");
        let store = HostStore::open(&directory.path().join("host.db")).unwrap();
        let project = store
            .add_project(directory.path().to_str().unwrap(), "demo")
            .unwrap();
        store
            .save(
                host_session(&project.id, "s1", 1),
                &json!({ "type": "test" }),
            )
            .unwrap();
        store.delete_session("s1").unwrap();
        let boot = json!(store.changes.boot);
        assert_eq!(
            store.changes.read(Some(&boot), Some(&json!(0))).sessions,
            vec![SessionChange {
                id: "s1".into(),
                project_id: project.id.clone(),
                revision: 1,
                deleted: Some(true),
                status: Some(HostSessionStatus::Idle),
                busy: Some(false),
            }]
        );
    }

    #[test]
    fn exchanges_a_pairing_code_once_before_it_expires() {
        let directory = temporary("monocode-pairing-");
        let store = HostStore::open(&directory.path().join("host.db")).unwrap();
        let now = 1_000_000;
        let Pairing { code, expires_at } = store.issue_pairing(now).unwrap();
        assert_eq!(code.len(), 43);
        assert!(
            code.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        );
        assert_eq!(store.pending_pairings(now).unwrap(), 1);
        assert_eq!(store.redeem_pairing("wrong", "Laptop", now).unwrap(), None);
        let device = store.redeem_pairing(&code, "Laptop", now).unwrap().unwrap();
        assert!(store.authenticated(&device.token).unwrap());
        assert_eq!(
            store.devices().unwrap(),
            vec![Device {
                id: device.id.clone(),
                name: "Laptop".into()
            }]
        );
        assert_eq!(store.redeem_pairing(&code, "Again", now).unwrap(), None);
        let late = store.issue_pairing(now).unwrap();
        assert_eq!(
            store
                .redeem_pairing(&late.code, "Late", expires_at)
                .unwrap(),
            None
        );
        assert_eq!(store.pending_pairings(expires_at).unwrap(), 0);
    }

    #[test]
    fn keeps_block_stamps_for_unchanged_blocks_and_sends_only_changed_ones() {
        let directory = temporary("monocode-store-");
        let store = HostStore::open(&directory.path().join("host.db")).unwrap();
        let project = store.add_project("/repo", "repo").unwrap();
        let mut value = host_session(&project.id, "s", 1);
        value.session.blocks = vec![
            monocode_core::Block::new("a", BlockRole::User, "hi"),
            monocode_core::Block::new("b", BlockRole::Assistant, "par"),
        ];
        store.save(value.clone(), &json!({})).unwrap();
        value.revision = 2;
        value.session.blocks[1].text = "partial".into();
        store.save(value.clone(), &json!({})).unwrap();
        let saved = store.session("s").unwrap();
        assert_eq!(
            saved.block_revisions.clone().unwrap(),
            BTreeMap::from([("a".to_string(), 1), ("b".to_string(), 2)])
        );
        assert_eq!(saved.created_at, Some(1));
        let sync = serde_json::to_value(store.sync("s", Some(1)).unwrap()).unwrap();
        assert_eq!(sync["kind"], "delta");
        assert_eq!(sync["blockIds"], json!(["a", "b"]));
        assert_eq!(
            sync["blocks"],
            json!([{ "id": "b", "role": "assistant", "text": "partial" }])
        );
        assert!(sync["value"]["session"].get("blocks").is_none());
        assert!(sync["value"].get("blockRevisions").is_none());
        assert_eq!(sync["value"]["session"]["title"], "Demo");
        let snapshot = serde_json::to_value(store.sync("s", None).unwrap()).unwrap();
        assert_eq!(snapshot["kind"], "snapshot");
        assert!(snapshot["value"].get("blockRevisions").is_none());
        assert_eq!(
            snapshot["value"]["session"]["blocks"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            serde_json::to_value(store.sync("s", Some(2)).unwrap()).unwrap(),
            json!({ "kind": "unchanged", "revision": 2 })
        );
        // Re-reading from disk compares blocks by value.
        let reopened = HostStore::open(&directory.path().join("host.db")).unwrap();
        value.revision = 3;
        reopened.save(value, &json!({})).unwrap();
        assert_eq!(
            reopened
                .session("s")
                .unwrap()
                .block_revisions
                .clone()
                .unwrap(),
            BTreeMap::from([("a".to_string(), 1), ("b".to_string(), 2)])
        );
        assert_eq!(reopened.running_turns().unwrap(), 0);
    }

    #[test]
    fn rolls_back_a_failed_transaction_and_reports_journal_gaps_as_snapshots() {
        let directory = temporary("monocode-store-");
        let store = HostStore::open(&directory.path().join("host.db")).unwrap();
        let project = store.add_project("/repo", "repo").unwrap();
        store
            .save(host_session(&project.id, "s", 1), &json!({ "n": 1 }))
            .unwrap();
        let failed: Result<(), String> = store.transaction(|| {
            store.save(host_session(&project.id, "s", 2), &json!({ "n": 2 }))?;
            Err("boom".into())
        });
        assert_eq!(failed.unwrap_err(), "boom");
        assert_eq!(store.session("s").unwrap().revision, 1);
        store
            .save(host_session(&project.id, "s", 2), &json!({ "n": 2 }))
            .unwrap();
        let events = serde_json::to_value(store.events("s", 1).unwrap()).unwrap();
        assert_eq!(
            events,
            json!({ "events": [{ "revision": 2, "event": { "n": 2 } }], "revision": 2 })
        );
        assert_eq!(store.events("s", 0).unwrap().events.unwrap().len(), 2);
        store
            .execute("DELETE FROM events WHERE revision=1", &[])
            .unwrap();
        let gap = store.events("s", 0).unwrap();
        assert!(gap.events.is_none());
        assert_eq!(gap.snapshot.unwrap().revision, 2);
        assert!(store.events("s", 5).unwrap().snapshot.is_some());
    }

    #[test]
    fn updates_metadata_and_rebuilds_old_summaries() {
        let directory = temporary("monocode-store-");
        let store = HostStore::open(&directory.path().join("host.db")).unwrap();
        let project = store.add_project("/repo", "repo").unwrap();
        store
            .save(host_session(&project.id, "s", 1), &json!({}))
            .unwrap();
        let summary = store
            .update_session(
                "s",
                &SessionPatch {
                    title: Some("  Renamed ".into()),
                    pinned: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(summary.title, "Renamed");
        assert_eq!(summary.pinned, Some(true));
        assert_eq!(summary.revision, 2);
        assert!(
            store
                .update_session(
                    "s",
                    &SessionPatch {
                        title: Some(" ".into()),
                        ..Default::default()
                    }
                )
                .is_err()
        );
        let events = store.events("s", 1).unwrap().events.unwrap();
        assert_eq!(
            events[0].event,
            json!({ "type": "session.metadata", "patch": { "title": "  Renamed ", "pinned": true } })
        );
        // A summary from an older host is rebuilt and saved.
        store
            .execute(
                "UPDATE sessions SET summary=? WHERE id='s'",
                &[&r#"{"id":"s"}"#],
            )
            .unwrap();
        let listed = store.summaries(&project.id).unwrap();
        assert_eq!(listed[0].model.as_deref(), Some("codex:test"));
        assert_eq!(listed[0].needs_input, Some(false));
        let text: String = store
            .with_db(|db| db.query_row("SELECT summary FROM sessions", [], |row| row.get(0)))
            .unwrap();
        let saved: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["providerSessionId"], Value::Null);
        assert_eq!(saved["title"], "Renamed");
    }
}
