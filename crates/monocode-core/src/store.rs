//! Session storage. Desktop launches use Monocode's existing schema directly.
mod app_access;
mod contributions;
mod managed_sessions;
mod monocode_import;
mod monos;
mod notes;
mod remote;
mod remote_legacy;
pub use remote::{RemoteProject, RemoteSession, RemoteUpload};
pub(crate) use shared::handle as desktop_session_handle;
mod session_metadata;
pub use session_metadata::{MobileSessionMetadata, SessionSidebarMetadata};
mod session_preview;
pub use session_preview::SessionPreviewData;
mod edit;
mod shared;
mod side_threads;
mod thread_files;
use crate::transcript::{Row, Transcript};
pub use edit::EditSnapshot;
pub use monocode_import::native_model;
pub use monocode_import::{monocode_database, ImportReport};
use rusqlite::{params, Connection};
pub use shared::{HistoryPage, PAGE_TURNS};
pub use side_threads::DesktopSideThread;
use std::path::Path;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: i64,
    pub title: String,
    pub provider: String,
    pub cwd: String,
    pub provider_id: Option<String>,
    pub has_messages: bool,
    /// Unix milliseconds; zero means an older session has no recorded time.
    pub updated_at: u64,
    pub archived: bool,
    pub pinned: bool,
    pub work_item: Option<crate::session_groups::WorkItem>,
}

/// A session's pending or delivered reminder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub session_id: i64,
    /// Unix milliseconds.
    pub due_at: u64,
    pub fired_at: Option<u64>,
}

pub struct Store {
    conn: Connection,
    path: std::path::PathBuf,
    shared: Option<shared::Shared>,
}
impl Store {
    pub fn reset_provider_id_if(&self, id: i64, expected: Option<&str>) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_reset_provider_id_if(id, expected);
        }

        self.conn
            .execute(
                "UPDATE native_sessions SET provider_id=NULL WHERE id=?1 AND provider_id IS ?2",
                params![id, expected],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn provider_accounts(&self) -> Result<crate::accounts::Book, String> {
        if self.shared.is_some() {
            return self.shared_accounts();
        }

        use rusqlite::OptionalExtension;
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM native_settings WHERE key='provider-accounts'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        value
            .map(|v| {
                serde_json::from_str(&v).map_err(|e| format!("Cannot read provider accounts: {e}"))
            })
            .unwrap_or_else(|| Ok(Default::default()))
    }
    pub fn save_provider_accounts(&self, book: &crate::accounts::Book) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_save_accounts(book);
        }

        let value = serde_json::to_string(book).map_err(|e| e.to_string())?;
        self.conn.execute("INSERT INTO native_settings(key,value) VALUES('provider-accounts',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value]).map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Legacy prototype fixtures. Desktop launches use open_monocode, including
    /// isolated development launches, so production history needs no conversion.
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS native_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS native_sessions(id INTEGER PRIMARY KEY, title TEXT NOT NULL,
            provider TEXT NOT NULL, cwd TEXT NOT NULL, provider_id TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS native_rows(session_id INTEGER NOT NULL REFERENCES native_sessions(id),
            row_ix INTEGER NOT NULL, role TEXT NOT NULL, body TEXT NOT NULL, first INTEGER NOT NULL,
            PRIMARY KEY(session_id,row_ix));
          CREATE TABLE IF NOT EXISTS native_reminders(session_id INTEGER PRIMARY KEY REFERENCES native_sessions(id),
            due_at INTEGER NOT NULL, fired_at INTEGER);").map_err(|e| e.to_string())?;
        let columns = conn
            .prepare("PRAGMA table_info(native_sessions)")
            .map_err(|e| e.to_string())?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for (column, kind) in [
            ("updated_at", "INTEGER NOT NULL DEFAULT 0"),
            ("archived", "INTEGER NOT NULL DEFAULT 0"),
            ("pinned", "INTEGER NOT NULL DEFAULT 0"),
            ("work_item", "TEXT"),
        ] {
            if !columns.iter().any(|name| name == column) {
                conn.execute(
                    &format!("ALTER TABLE native_sessions ADD COLUMN {column} {kind}"),
                    [],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        let has_sent_at = conn
            .prepare("PRAGMA table_info(native_rows)")
            .map_err(|e| e.to_string())?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|name| name == "sent_at");
        if !has_sent_at {
            conn.execute("ALTER TABLE native_rows ADD COLUMN sent_at INTEGER", [])
                .map_err(|e| e.to_string())?;
        }
        // Older native histories have completion times in their turn receipts.
        // Preserve unknown ages instead of stamping all old conversations now.
        let recorded = conn.prepare("SELECT s.id,a.value FROM native_sessions s JOIN native_settings a ON a.key='agent:'||s.id WHERE s.updated_at=0")
            .map_err(|e| e.to_string())?
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        for (id, settings) in recorded {
            if let Ok(settings) = serde_json::from_str::<crate::agent::Settings>(&settings) {
                if let Some(time) = settings
                    .turns
                    .values()
                    .filter_map(|turn| turn.completed_at)
                    .max()
                {
                    conn.execute(
                        "UPDATE native_sessions SET updated_at=?2 WHERE id=?1",
                        params![id, time.min(i64::MAX as u64) as i64],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(Self {
            conn,
            path: path.into(),
            shared: None,
        })
    }
    /// The selected desktop or isolated-development database.
    pub fn database_path(&self) -> &Path {
        &self.path
    }
    pub fn data_dir(&self) -> Option<&Path> {
        (self.path != Path::new(":memory:"))
            .then(|| self.path.parent())
            .flatten()
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>, String> {
        if self.shared.is_some() {
            return self.shared_setting(key);
        }

        use rusqlite::OptionalExtension;
        self.conn
            .query_row(
                "SELECT value FROM native_settings WHERE key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn save_setting(&self, key: &str, value: &str) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_save_setting(key, value);
        }

        self.conn.execute("INSERT INTO native_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn ui_state(&self) -> Result<Option<String>, String> {
        if self.shared.is_some() {
            return self.shared_ui_state();
        }

        use rusqlite::OptionalExtension;
        self.conn
            .query_row(
                "SELECT value FROM native_settings WHERE key='workspace'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn agent_settings(&self, id: i64) -> Result<Option<crate::agent::Settings>, String> {
        let remote = id < 0 || self.remote_session(id)?.is_some();
        if remote {
            if let Some(raw) = self.setting(&format!("remote-agent:{id}"))? {
                return serde_json::from_str(&raw)
                    .map(Some)
                    .map_err(|e| format!("Cannot read remote settings: {e}"));
            }
            if id < 0 || self.shared.is_none() {
                return Ok(None);
            }
        }
        if self.shared.is_some() {
            let mut settings = self.shared_agent(id)?;
            if remote {
                if let Some(settings) = settings.as_mut() {
                    if settings.worktree_cwd.is_none() {
                        settings.worktree_cwd = self.legacy_remote_worktree(id)?;
                    }
                }
            }
            return Ok(settings);
        }

        use rusqlite::OptionalExtension;
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM native_settings WHERE key=?1",
                [format!("agent:{id}")],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        text.map(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
            .transpose()
    }
    /// Read workspace membership without loading every session's full agent settings.
    pub fn session_worktrees(
        &self,
    ) -> Result<std::collections::HashMap<i64, Option<String>>, String> {
        if self.shared.is_some() {
            return self.shared_worktrees();
        }

        self.conn
            .prepare("SELECT s.id, CASE WHEN json_valid(a.value) THEN CASE WHEN json_type(a.value, '$.worktree_cwd')='text' THEN json_extract(a.value, '$.worktree_cwd') END END FROM native_sessions s LEFT JOIN native_settings a ON a.key='agent:'||s.id")
            .map_err(|e| e.to_string())?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())
    }
    pub fn context_usage(&self, id: i64) -> Result<Option<crate::context::Usage>, String> {
        if self.shared.is_some() {
            return self.shared_context(id);
        }

        use rusqlite::OptionalExtension;
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM native_settings WHERE key=?1",
                [format!("context:{id}")],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        text.map(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
            .transpose()
    }
    pub fn save_context_usage(&self, id: i64, usage: &crate::context::Usage) -> Result<(), String> {
        if self.shared.is_some() {
            self.shared_patch(
                id,
                &[
                    ("context_used", serde_json::json!(usage.used)),
                    ("context_window", serde_json::json!(usage.window)),
                ],
            )?;
            // The desktop schema has no column for the cache lifetime; keep it
            // in native metadata so a reopened chat keeps its countdown.
            return match usage.cache_ttl_ms {
                Some(ttl) => self.shared_save_setting(&format!("cache-ttl:{id}"), &ttl.to_string()),
                None => Ok(()),
            };
        }

        self.conn.execute("INSERT INTO native_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("context:{id}"), serde_json::to_string(usage).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn save_note(&self, note: &crate::response::Note) -> Result<(), String> {
        self.save_response_note(note)
    }
    pub fn notes(&self) -> Result<Vec<crate::response::Note>, String> {
        Ok(self
            .list_notes()?
            .into_iter()
            .map(|note| {
                let source_session = self
                    .note_source_session(&note)
                    .or_else(|| {
                        self.shared
                            .is_none()
                            .then(|| note.source_session_id.as_deref()?.parse().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                crate::response::Note {
                    id: note
                        .id
                        .strip_prefix("gpui-note-")
                        .and_then(|id| id.parse().ok())
                        .unwrap_or_else(|| shared::handle(&note.id) as u64),
                    title: note.title,
                    body: note.body,
                    tags: note.tags,
                    source_session,
                    source_cwd: note.source_cwd.unwrap_or_default(),
                }
            })
            .collect())
    }
    /// Read-only fallback for early native settings; list_notes owns merging it
    /// with the desktop table, so this must never call the notes facade.
    fn legacy_notes(&self) -> Result<Vec<crate::response::Note>, String> {
        let mut query = self
            .conn
            .prepare("SELECT value FROM native_settings WHERE key LIKE 'note:%' ORDER BY key DESC")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
    }
    pub fn save_agent_settings(
        &self,
        id: i64,
        settings: &crate::agent::Settings,
    ) -> Result<(), String> {
        if id < 0 || self.remote_session(id)?.is_some() {
            return self.save_setting(
                &format!("remote-agent:{id}"),
                &serde_json::to_string(settings).map_err(|e| e.to_string())?,
            );
        }
        if self.shared.is_some() {
            return self.shared_save_agent(id, settings);
        }

        self.conn.execute("INSERT INTO native_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("agent:{id}"), serde_json::to_string(settings).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn configure_agent(
        &self,
        id: i64,
        provider: &str,
        settings: &crate::agent::Settings,
    ) -> Result<(), String> {
        if self
            .agent_settings(id)?
            .is_some_and(|saved| saved.revision > settings.revision)
        {
            return Ok(());
        }
        self.change_provider(id, provider)?;
        self.save_agent_settings(id, settings)
    }
    pub fn change_provider(&self, id: i64, provider: &str) -> Result<(), String> {
        if let Some(mut remote) = self.remote_session(id)? {
            if remote.summary.provider != provider {
                // The host fixes the harness at creation, as React's remote view does.
                if remote.session_id.is_some() {
                    return Err("Start a new remote conversation to change provider".into());
                }
                remote.summary.provider = provider.into();
                self.save_remote_session(&remote)?;
            }
            return Ok(());
        }
        if self.shared.is_some() {
            return self.shared_change_provider(id, provider);
        }

        self.conn.execute("UPDATE native_sessions SET provider=?2,provider_id=NULL WHERE id=?1 AND provider<>?2", params![id,provider]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn save_ui_state(&self, state: &str) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_save_setting("workspace", state);
        }

        self.conn.execute("INSERT INTO native_settings(key,value) VALUES('workspace',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[state]).map_err(|e|e.to_string())?;
        Ok(())
    }
    fn local_sessions(&self) -> Result<Vec<Session>, String> {
        if self.shared.is_some() {
            return self.shared_sessions();
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id,title,provider,cwd,provider_id,
                 (EXISTS(SELECT 1 FROM native_rows WHERE session_id=native_sessions.id)
                  OR EXISTS(SELECT 1 FROM native_settings WHERE key='follow-up:'||native_sessions.id AND json_valid(value) AND (json_type(value,'$.draft')='object' OR json_array_length(value,'$.messages')>0))),updated_at,archived,
                 pinned,work_item
                 FROM native_sessions WHERE NOT EXISTS(SELECT 1 FROM native_settings WHERE key='inbox-ask:'||native_sessions.id) ORDER BY updated_at DESC,id DESC",
            )
            .map_err(|e| e.to_string())?;
        let result = stmt
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    provider: r.get(2)?,
                    cwd: r.get(3)?,
                    provider_id: r.get(4)?,
                    has_messages: r.get(5)?,
                    updated_at: r.get::<_, i64>(6)?.max(0) as u64,
                    archived: r.get(7)?,
                    pinned: r.get(8)?,
                    work_item: r
                        .get::<_, Option<String>>(9)?
                        .and_then(|text| serde_json::from_str(&text).ok()),
                })
            })
            .map_err(|e| e.to_string())?;
        result
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    /// Each session another one started through the app CLI, and its caller.
    pub fn started_links(&self) -> Result<std::collections::HashMap<i64, i64>, String> {
        let mut links = if self.shared.is_some() {
            self.shared_started_links()?
        } else {
            std::collections::HashMap::new()
        };
        let saved: std::collections::HashMap<_, _> = self
            .settings_with_prefix("started-by:")?
            .into_iter()
            .map(|(child, value)| Ok((child, value.trim().parse().map_err(|_| "Invalid caller")?)))
            .collect::<Result<_, String>>()?;
        links.extend(saved);
        Ok(links)
    }
    pub fn save_started_link(&self, child: i64, caller: i64) -> Result<(), String> {
        self.save_setting(&format!("started-by:{child}"), &caller.to_string())
    }
    /// Every lead's last orchestration summary.
    pub fn orchestration_summaries(
        &self,
    ) -> Result<std::collections::HashMap<i64, crate::orchestration::Summary>, String> {
        Ok(self
            .settings_with_prefix("orchestration:")?
            .into_iter()
            .filter_map(|(lead, value)| Some((lead, serde_json::from_str(&value).ok()?)))
            .collect())
    }
    pub fn save_orchestration_summary(
        &self,
        lead: i64,
        summary: &crate::orchestration::Summary,
    ) -> Result<(), String> {
        let value = serde_json::to_string(summary).map_err(|e| e.to_string())?;
        self.save_setting(&format!("orchestration:{lead}"), &value)
    }
    /// `prefix<session id>` settings, by session id.
    fn settings_with_prefix(&self, prefix: &str) -> Result<Vec<(i64, String)>, String> {
        if self.shared.is_some() {
            return self.shared_settings_with_prefix(prefix);
        }

        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM native_settings WHERE key LIKE ?1 || '%'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([prefix], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let (key, value) = row.map_err(|e| e.to_string())?;
            if let Some(id) = key.strip_prefix(prefix).and_then(|id| id.parse().ok()) {
                out.push((id, value));
            }
        }
        Ok(out)
    }
    /// Sessions whose latest message is saved as a draft and not yet sent.
    pub fn draft_sessions(&self) -> Result<std::collections::HashSet<i64>, String> {
        if self.shared.is_some() {
            return self.shared_drafts();
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT CAST(substr(key,11) AS INTEGER) FROM native_settings
                 WHERE key LIKE 'follow-up:%' AND json_valid(value)
                 AND json_type(value,'$.draft')='object'",
            )
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        ids.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }
    fn local_create(&self, provider: &str, cwd: &str) -> Result<Session, String> {
        if self.shared.is_some() {
            return self.shared_create(provider, cwd);
        }

        let updated_at = crate::response::now_ms();
        let title = format!(
            "New {} session",
            crate::provider::Kind::parse(provider)
                .ok_or_else(|| format!("Unknown provider: {provider}"))?
                .label()
        );
        self.conn
            .execute(
                "INSERT INTO native_sessions(title,provider,cwd,updated_at) VALUES(?1,?2,?3,?4)",
                params![title, provider, cwd, updated_at.min(i64::MAX as u64) as i64],
            )
            .map_err(|e| e.to_string())?;
        Ok(Session {
            id: self.conn.last_insert_rowid(),
            title,
            provider: provider.into(),
            cwd: cwd.into(),
            provider_id: None,
            has_messages: false,
            updated_at,
            archived: false,
            pinned: false,
            work_item: None,
        })
    }
    /// A WAL reader for background search, independent of the writer's mutex.
    /// In-memory fixture stores must continue using their original connection.
    pub fn search_reader(&self) -> Result<Option<Self>, String> {
        if self.path == Path::new(":memory:") {
            return Ok(None);
        }
        if self.shared.is_some() {
            return self.shared_search_reader().map(Some);
        }
        let conn =
            Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        Ok(Some(Self {
            conn,
            path: self.path.clone(),
            shared: None,
        }))
    }
    /// Stream saved message bodies without loading conversations into the UI.
    /// Rejoin native row fragments so phrase matches span storage boundaries.
    pub fn search_sessions(
        &self,
        query: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(Vec<crate::search::Hit>, bool), String> {
        if self.shared.is_some() {
            return self.shared_search(query, cancel);
        }

        use std::sync::atomic::Ordering;
        if query.trim().is_empty() || cancel.load(Ordering::Acquire) {
            return Ok((vec![], false));
        }
        let flag = cancel.clone();
        self.conn
            .progress_handler(2000, Some(move || flag.load(Ordering::Acquire)))
            .map_err(|e| e.to_string())?;
        let result = (|| {
            let summaries = self.sessions()?;
            // Walk each conversation's primary-key range in recency order.
            // Sorting a join containing every message body would copy the entire
            // transcript database into SQLite's sorter on every keystroke.
            let mut statement=self.conn.prepare("SELECT row_ix,role,body,first FROM native_rows WHERE session_id=?1 ORDER BY row_ix").map_err(|e|e.to_string())?;
            let mut user = self
                .conn
                .prepare(
                    "SELECT EXISTS(SELECT 1 FROM native_rows WHERE session_id=?1 AND role='user')",
                )
                .map_err(|e| e.to_string())?;
            let mut hits = Vec::new();
            let mut truncated = false;
            'sessions: for summary in summaries.iter().filter(|s| !s.archived && s.has_messages) {
                if cancel.load(Ordering::Acquire) {
                    return Ok((vec![], false));
                }
                if !user
                    .query_row([summary.id], |r| r.get::<_, bool>(0))
                    .map_err(|e| e.to_string())?
                {
                    continue;
                }
                let flush = |current: Option<(usize, String, String)>,
                             hits: &mut Vec<crate::search::Hit>| {
                    if let Some((row, role, text)) = current {
                        if let Some(hit) =
                            crate::search::message_hit(summary, row, &role, &text, query, false)
                        {
                            hits.push(hit);
                        }
                    }
                };
                let mut current: Option<(usize, String, String)> = None;
                let mut rows = statement.query([summary.id]).map_err(|e| e.to_string())?;
                while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                    if cancel.load(Ordering::Acquire) {
                        return Ok((vec![], false));
                    }
                    let ix = row.get::<_, i64>(0).map_err(|e| e.to_string())?.max(0) as usize;
                    let role: String = row.get(1).map_err(|e| e.to_string())?;
                    let body: String = row.get(2).map_err(|e| e.to_string())?;
                    let first: bool = row.get(3).map_err(|e| e.to_string())?;
                    if first || current.is_none() {
                        flush(current.take(), &mut hits);
                        if hits.len() >= 400 {
                            truncated = true;
                            break 'sessions;
                        }
                        current = Some((ix, role, String::new()));
                    }
                    if let Some((_, _, text)) = &mut current {
                        let mut end = body
                            .len()
                            .min((2 * 1024 * 1024usize).saturating_sub(text.len()));
                        while !body.is_char_boundary(end) {
                            end -= 1;
                        }
                        text.push_str(&body[..end]);
                        truncated |= end < body.len();
                    }
                }
                flush(current, &mut hits);
                if hits.len() >= 400 {
                    truncated = true;
                    break;
                }
            }
            let titles = summaries
                .into_iter()
                .filter(|s| !s.archived && s.has_messages)
                .collect::<Vec<_>>();
            hits.extend(crate::search::titles(&titles, query));
            Ok((hits, truncated))
        })();
        self.conn
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|e| e.to_string())?;
        if cancel.load(Ordering::Acquire) {
            Ok((vec![], false))
        } else {
            result
        }
    }
    /// The cursor for the turns before `load`'s rows, when a Mono's history
    /// is paged rather than loaded whole.
    pub fn history_before(&self, session: i64) -> Result<Option<i64>, String> {
        if self.shared.is_none() || self.remote_session(session)?.is_some() {
            return Ok(None);
        }
        self.shared_history_before(session)
    }
    /// The page of turns before `before`, or with `all` every older turn.
    pub fn load_before(&self, session: i64, before: i64, all: bool) -> Result<HistoryPage, String> {
        if self.shared.is_none() {
            return Ok(HistoryPage::default());
        }
        self.shared_load_before(session, before, all)
    }
    pub fn load(&self, session: i64) -> Result<Transcript, String> {
        if let Some(remote) = self.remote_session(session)? {
            let Some(id) = remote.session_id.as_deref() else {
                return Ok(Transcript::default());
            };
            let client = crate::remote::default_connections()?;
            let machine = remote.machine(&client)?;
            let mut snapshot = client.load_session(&machine.id, id, None)?;
            crate::remote::attachments::qualify_snapshot(
                &mut snapshot,
                &remote.project.environment_id,
                id,
            );
            return Self::project_remote_history(&snapshot).map(|(history, _)| history);
        }
        if self.shared.is_some() {
            return self.shared_load(session);
        }

        let mut stmt = self
            .conn
            .prepare("SELECT role,body,first,sent_at FROM native_rows WHERE session_id=? ORDER BY row_ix")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([session], |r| {
                Ok(Row {
                    role: r.get(0)?,
                    text: r.get(1)?,
                    first: r.get(2)?,
                    sent_at: r
                        .get::<_, Option<i64>>(3)?
                        .and_then(|at| u64::try_from(at).ok()),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut rows = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        // Pre-timestamp histories recorded completion and duration per run.
        // Recover their start time where known, leaving undated history undated.
        if let Ok(Some(settings)) = self.agent_settings(session) {
            for (turn, info) in settings.turns {
                if turn > rows.len() {
                    continue;
                }
                let mut prompt = turn;
                while prompt > 0 && rows[prompt - 1].is_prompt() {
                    prompt -= 1;
                }
                if let Some(row) = rows.get_mut(prompt) {
                    if row.sent_at.is_none() {
                        row.sent_at = info
                            .completed_at
                            .and_then(|at| at.checked_sub(info.duration_ms));
                    }
                }
            }
        }
        Ok(Transcript { rows })
    }
    pub fn write_rows(&mut self, session: i64, start: usize, rows: &[Row]) -> Result<(), String> {
        self.write_rows_with_message(session, start, rows, None)
    }
    pub fn write_rows_with_message(
        &mut self,
        session: i64,
        start: usize,
        rows: &[Row],
        message_id: Option<&str>,
    ) -> Result<(), String> {
        if self.remote_session(session)?.is_some() {
            return Err("Remote transcripts are owned by the host".into());
        }
        if self.shared.is_some() {
            return self.shared_write_rows(session, start, rows, message_id);
        }

        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO native_rows(session_id,row_ix,role,body,first,sent_at) VALUES(?1,?2,?3,?4,?5,?6)
                ON CONFLICT(session_id,row_ix) DO UPDATE SET body=excluded.body",
                )
                .map_err(|e| e.to_string())?;
            for (offset, row) in rows.iter().enumerate() {
                stmt.execute(params![
                    session,
                    (start + offset) as i64,
                    row.role,
                    row.text,
                    row.first,
                    row.sent_at.and_then(|at| i64::try_from(at).ok())
                ])
                .map_err(|e| e.to_string())?;
            }
        }
        if !rows.is_empty() {
            tx.execute(
                "UPDATE native_sessions SET updated_at=?2 WHERE id=?1",
                params![
                    session,
                    crate::response::now_ms().min(i64::MAX as u64) as i64
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(id) = message_id {
            tx.execute("INSERT INTO native_settings(key,value) VALUES(?1,'1') ON CONFLICT(key) DO UPDATE SET value='1'",[format!("mono-delivered:{session}:{id}")]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    /// Archiving preserves the transcript, provider context and configuration.
    pub fn set_archived(&self, id: i64, archived: bool) -> Result<(), String> {
        if self.remote_session(id)?.is_some() {
            return self.patch_remote_session(id, serde_json::json!({"archived": archived}));
        }
        if self.shared.is_some() {
            return self.shared_patch(id, &[("archived", serde_json::json!(archived))]);
        }

        self.conn
            .execute(
                "UPDATE native_sessions SET archived=?2 WHERE id=?1",
                params![id, archived],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn set_pinned(&self, ids: &[i64], pinned: bool) -> Result<(), String> {
        let any_remote = ids
            .iter()
            .map(|id| self.remote_session(*id).map(|remote| remote.is_some()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|remote| remote);
        if any_remote {
            for id in ids {
                if self.remote_session(*id)?.is_some() {
                    self.patch_remote_session(*id, serde_json::json!({"pinned": pinned}))?;
                } else {
                    self.set_pinned(&[*id], pinned)?;
                }
            }
            return Ok(());
        }
        if self.shared.is_some() {
            return self.shared_pins(ids, pinned);
        }

        for id in ids {
            self.conn
                .execute(
                    "UPDATE native_sessions SET pinned=?2 WHERE id=?1",
                    params![id, pinned],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn set_work_item(
        &self,
        id: i64,
        item: Option<&crate::session_groups::WorkItem>,
    ) -> Result<(), String> {
        if self.remote_session(id)?.is_some() {
            return self.patch_remote_session(id, serde_json::json!({"linkedWorkItem": item}));
        }
        if self.shared.is_some() {
            return self.shared_patch(
                id,
                &[(
                    "linked_work_item_json",
                    item.map(shared::encode)
                        .transpose()?
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                )],
            );
        }

        let text = item
            .map(|item| serde_json::to_string(item).map_err(|e| e.to_string()))
            .transpose()?;
        self.conn
            .execute(
                "UPDATE native_sessions SET work_item=?2 WHERE id=?1",
                params![id, text],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// A project's folders and folded groups.
    pub fn groups(&self, cwd: &str) -> Result<crate::session_groups::Groups, String> {
        if self.shared.is_some() {
            return self.shared_groups(cwd);
        }

        use rusqlite::OptionalExtension;
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM native_settings WHERE key=?1",
                [format!("groups:{cwd}")],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(text
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default())
    }
    pub fn save_groups(
        &self,
        cwd: &str,
        groups: &crate::session_groups::Groups,
    ) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_save_setting(&format!("groups:{cwd}"), &shared::encode(groups)?);
        }

        self.conn.execute("INSERT INTO native_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("groups:{cwd}"), serde_json::to_string(groups).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn reminders(&self) -> Result<Vec<Reminder>, String> {
        if self.shared.is_some() {
            return self.shared_reminders();
        }

        let mut stmt = self
            .conn
            .prepare("SELECT session_id,due_at,fired_at FROM native_reminders ORDER BY due_at,session_id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Reminder {
                    session_id: r.get(0)?,
                    due_at: r.get::<_, i64>(1)?.max(0) as u64,
                    fired_at: r.get::<_, Option<i64>>(2)?.map(|t| t.max(0) as u64),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    /// Schedules (or reschedules, re-arming) each session's reminder.
    pub fn set_reminders(&mut self, ids: &[i64], due_at: u64) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_set_reminders(ids, due_at);
        }

        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        for id in ids {
            tx.execute(
                "INSERT INTO native_reminders(session_id,due_at,fired_at) VALUES(?1,?2,NULL)
                 ON CONFLICT(session_id) DO UPDATE SET due_at=excluded.due_at,fired_at=NULL",
                params![id, due_at.min(i64::MAX as u64) as i64],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    /// Re-arms only the reminder occurrence the user snoozed. A stale popup
    /// must not overwrite a newer schedule created in another window.
    pub fn snooze_reminder_if(
        &self,
        id: i64,
        expected_due: u64,
        new_due: u64,
    ) -> Result<bool, String> {
        if self.shared.is_some() {
            return self.shared_snooze_reminder_if(id, expected_due, new_due);
        }
        self.conn.execute("UPDATE native_reminders SET due_at=?3,fired_at=NULL WHERE session_id=?1 AND due_at=?2",
            params![id, expected_due.min(i64::MAX as u64) as i64, new_due.min(i64::MAX as u64) as i64]).map(|changed|changed==1).map_err(|e|e.to_string())
    }
    /// Clears the sessions' reminders; with `expected`, only that one, so a
    /// stale delivery never clears a newer reminder.
    pub fn clear_reminders(&self, ids: &[i64], expected: Option<u64>) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_clear_reminders(ids, expected);
        }

        for id in ids {
            match expected {
                Some(due) => self.conn.execute(
                    "DELETE FROM native_reminders WHERE session_id=?1 AND due_at=?2",
                    params![id, due.min(i64::MAX as u64) as i64],
                ),
                None => self
                    .conn
                    .execute("DELETE FROM native_reminders WHERE session_id=?1", [id]),
            }
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// Marks due, undelivered reminders delivered and returns them, once.
    pub fn take_due_reminders(&mut self, now: u64) -> Result<Vec<Reminder>, String> {
        let due: Vec<_> = self
            .reminders()?
            .into_iter()
            .filter(|r| r.due_at <= now && r.fired_at.is_none())
            .collect();
        if self.shared.is_some() {
            return self.shared_fire_reminders(&due, now);
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        for reminder in &due {
            tx.execute(
                "UPDATE native_reminders SET fired_at=?2 WHERE session_id=?1",
                params![reminder.session_id, now.min(i64::MAX as u64) as i64],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(due)
    }
    /// Deletes the conversation, its settings, reminder and folder for good.
    pub fn delete(&mut self, id: i64) -> Result<(), String> {
        // Resolve the folder first; the persisted id is gone after deletion.
        let dir = self.thread_dir(id).ok().flatten();
        self.delete_rows(id)?;
        thread_files::remove_dir(dir);
        Ok(())
    }
    fn delete_rows(&mut self, id: i64) -> Result<(), String> {
        if self.remote_session(id)?.is_some() {
            return self.delete_remote_session(id);
        }
        if self.shared.is_some() {
            return self.shared_delete(id);
        }

        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM native_rows WHERE session_id=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM native_reminders WHERE session_id=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM native_settings WHERE key IN (?1,?2,?3,?5,?6,?7,?8) OR key LIKE ?4",
            params![
                format!("agent:{id}"),
                format!("context:{id}"),
                format!("follow-up:{id}"),
                format!("mono-delivered:{id}:%"),
                format!("started-by:{id}"),
                format!("orchestration:{id}"),
                format!("inbox-ask:{id}"),
                format!("automation:{id}")
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM native_sessions WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn provider_id(&self, id: i64, provider_id: &str) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_patch(
                id,
                &[("provider_session_id", serde_json::json!(provider_id))],
            );
        }

        self.conn
            .execute(
                "UPDATE native_sessions SET provider_id=?2 WHERE id=?1",
                params![id, provider_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Keep conversation history while detaching a removed working copy.
    pub fn detach_worktree(
        &self,
        path: &str,
        main: &str,
    ) -> Result<Vec<(Session, crate::agent::Settings)>, String> {
        if self.shared.is_some() {
            return self.shared_detach_worktree(path, main);
        }

        let sessions = self.sessions()?;
        let mut changed = Vec::new();
        for mut session in sessions {
            let mut settings = self.agent_settings(session.id)?.unwrap_or_default();
            let execution = settings.worktree_cwd.as_deref().unwrap_or(&session.cwd);
            if !crate::files::is_equal_or_inside(execution, path) {
                continue;
            }
            settings.worktree_cwd = Some(execution.to_owned());
            settings.worktree_removed = true;
            settings.revision += 1;
            settings.new_worktree = false;
            settings.worktree_base = None;
            if crate::files::is_equal_or_inside(&session.cwd, path) {
                session.cwd = main.into();
            }
            session.provider_id = None;
            changed.push((session, settings));
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        for (session, settings) in &changed {
            tx.execute(
                "UPDATE native_sessions SET cwd=?2,provider_id=NULL WHERE id=?1",
                params![session.id, session.cwd],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO native_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("agent:{}",session.id),serde_json::to_string(settings).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
            tx.execute(
                "DELETE FROM native_settings WHERE key=?1",
                [format!("context:{}", session.id)],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(changed)
    }
    pub fn reset_provider_id(&self, id: i64) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_patch(id, &[("provider_session_id", serde_json::Value::Null)]);
        }

        self.conn
            .execute(
                "UPDATE native_sessions SET provider_id=NULL WHERE id=?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn title(&self, id: i64, title: &str) -> Result<(), String> {
        if self.remote_session(id)?.is_some() {
            return self.patch_remote_session(id, serde_json::json!({"title": title}));
        }
        if self.shared.is_some() {
            return self.shared_patch(id, &[("title", serde_json::json!(title))]);
        }

        self.conn
            .execute(
                "UPDATE native_sessions SET title=?2 WHERE id=?1",
                params![id, title],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// A late generated title cannot replace a manual rename or recreate a deleted session.
    pub fn replace_title(&self, id: i64, expected: &str, title: &str) -> Result<bool, String> {
        if self.shared.is_some() {
            return self.shared_replace_title(id, expected, title);
        }

        self.conn
            .execute(
                "UPDATE native_sessions SET title=?3 WHERE id=?1 AND title=?2",
                params![id, expected, title],
            )
            .map(|count| count == 1)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn removed_worktree_keeps_history_and_requires_reattachment() {
        let mut store = super::Store::open(std::path::Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project-worktrees/feature").unwrap();
        let settings = crate::agent::Settings {
            worktree_cwd: Some("/project-worktrees/feature".into()),
            ..Default::default()
        };
        store.save_agent_settings(session.id, &settings).unwrap();
        store
            .provider_id(session.id, "old-provider-thread")
            .unwrap();
        store
            .write_rows(
                session.id,
                0,
                &[crate::transcript::Row {
                    role: "user".into(),
                    text: "Keep my conversation".into(),
                    first: true,
                    sent_at: None,
                }],
            )
            .unwrap();
        let detached = store
            .detach_worktree("/project-worktrees/feature", "/project")
            .unwrap();
        assert_eq!(detached.len(), 1);
        assert_eq!(detached[0].0.cwd, "/project");
        assert!(detached[0].0.provider_id.is_none());
        assert!(detached[0].1.worktree_removed);
        assert_eq!(
            store.load(session.id).unwrap().rows[0].text,
            "Keep my conversation"
        );
        assert!(
            store
                .agent_settings(session.id)
                .unwrap()
                .unwrap()
                .worktree_removed
        );
    }

    #[test]
    fn workspace_membership_reads_main_and_worktree_sessions_without_loading_history() {
        let store = super::Store::open(std::path::Path::new(":memory:")).unwrap();
        let main = store.create("codex", "/project").unwrap();
        let feature = store.create("claude", "/project").unwrap();
        let corrupt = store.create("codex", "/project").unwrap();
        store
            .save_agent_settings(
                feature.id,
                &crate::agent::Settings {
                    worktree_cwd: Some("/project-feature".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .save_setting(&format!("agent:{}", corrupt.id), "invalid json")
            .unwrap();
        let scopes = store.session_worktrees().unwrap();
        assert_eq!(scopes.len(), 3);
        assert_eq!(scopes[&main.id], None);
        assert_eq!(scopes[&feature.id].as_deref(), Some("/project-feature"));
        assert_eq!(scopes[&corrupt.id], None);
    }

    use super::*;
    #[test]
    fn archive_and_activity_time_survive_restart_without_changing_history() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("native.db");
        let mut store = Store::open(&path).unwrap();
        let session = store.create("codex", "/project").unwrap();
        assert!(session.updated_at > 0);
        store
            .conn
            .execute(
                "UPDATE native_sessions SET updated_at=1 WHERE id=?1",
                [session.id],
            )
            .unwrap();
        let rows = vec![Row {
            role: "user".into(),
            text: "Keep this conversation".into(),
            first: true,
            sent_at: None,
        }];
        store.write_rows(session.id, 0, &rows).unwrap();
        store.provider_id(session.id, "resume-this-thread").unwrap();
        let updated_at = store.sessions().unwrap()[0].updated_at;
        assert!(updated_at > 1);
        store.set_archived(session.id, true).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let saved = &store.sessions().unwrap()[0];
        assert!(saved.archived);
        assert_eq!(
            saved.updated_at, updated_at,
            "archiving does not make an old session look new"
        );
        assert_eq!(saved.provider_id.as_deref(), Some("resume-this-thread"));
        assert_eq!(store.load(session.id).unwrap().rows, rows);
        store.set_archived(session.id, false).unwrap();
        assert!(!store.sessions().unwrap()[0].archived);
    }
    #[test]
    fn started_links_and_orchestration_summaries_survive_restart_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sqlite3");
        let store = Store::open(&path).unwrap();
        let lead = store.create("codex", "/p").unwrap().id;
        let child = store.create("codex", "/p").unwrap().id;
        store.save_started_link(child, lead).unwrap();
        let summary = crate::orchestration::Summary {
            tasks: vec![crate::orchestration::SummaryTask {
                session: Some(child),
                title: "Tests".into(),
                provider: "codex".into(),
                model: None,
                status: "running".into(),
            }],
            live: true,
        };
        store.save_orchestration_summary(lead, &summary).unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.started_links().unwrap()[&child], lead);
        assert_eq!(store.orchestration_summaries().unwrap()[&lead], summary);
        assert_eq!(summary.tasks[0].label(&summary, false), "Working");
        store.delete(child).unwrap();
        store.delete(lead).unwrap();
        assert!(store.started_links().unwrap().is_empty());
        assert!(store.orchestration_summaries().unwrap().is_empty());
    }
    #[test]
    fn pins_links_groups_and_reminders_survive_restart_and_delete_clears_them() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("native.db");
        let mut store = Store::open(&path).unwrap();
        let keep = store.create("codex", "/project").unwrap();
        let gone = store.create("claude", "/project").unwrap();
        store.set_pinned(&[keep.id], true).unwrap();
        let item =
            crate::session_groups::parse_github_work_item("https://github.com/acme/app/pull/9")
                .unwrap();
        store.set_work_item(keep.id, Some(&item)).unwrap();
        let mut groups = crate::session_groups::Groups::default();
        groups.create_folder(&[keep.id, gone.id], "f".into());
        groups.pinned_collapsed = true;
        store.save_groups("/project", &groups).unwrap();
        store.set_reminders(&[keep.id, gone.id], 100).unwrap();
        store.set_reminders(&[keep.id], 200).unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        let saved = store
            .sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.id == keep.id)
            .unwrap();
        assert!(saved.pinned);
        assert_eq!(saved.work_item, Some(item));
        assert_eq!(store.groups("/project").unwrap(), groups);
        assert_eq!(store.groups("/other").unwrap(), Default::default());
        let due = store.take_due_reminders(150).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].session_id, gone.id);
        assert!(
            store.take_due_reminders(150).unwrap().is_empty(),
            "delivered once"
        );
        store.clear_reminders(&[keep.id], Some(100)).unwrap();
        assert_eq!(
            store.reminders().unwrap().len(),
            2,
            "a stale time clears nothing"
        );
        store.delete(gone.id).unwrap();
        assert_eq!(store.reminders().unwrap().len(), 1);
        assert!(store.sessions().unwrap().iter().all(|s| s.id != gone.id));
        assert!(store.load(gone.id).unwrap().rows.is_empty());
        store.clear_reminders(&[keep.id], None).unwrap();
        assert!(store.reminders().unwrap().is_empty());
        store.set_work_item(keep.id, None).unwrap();
        store.set_pinned(&[keep.id], false).unwrap();
        let saved = &store.sessions().unwrap()[0];
        assert!(!saved.pinned && saved.work_item.is_none());
    }
    #[test]
    fn old_native_schema_migrates_idempotently_and_recovers_recorded_times() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("legacy-native.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE native_sessions(id INTEGER PRIMARY KEY,title TEXT NOT NULL,provider TEXT NOT NULL,cwd TEXT NOT NULL,provider_id TEXT);
            CREATE TABLE native_settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE native_rows(session_id INTEGER NOT NULL,row_ix INTEGER NOT NULL,role TEXT NOT NULL,body TEXT NOT NULL,first INTEGER NOT NULL,PRIMARY KEY(session_id,row_ix));
            INSERT INTO native_sessions VALUES(1,'Has a receipt','codex','/project','thread'),(2,'Unknown age','claude','/project',NULL);
            INSERT INTO native_rows VALUES(1,0,'user','Old prompt',1),(1,1,'assistant','Old answer',1),(2,0,'user','Undated prompt',1);
            INSERT INTO native_settings VALUES('agent:1','{\"turns\":{\"1\":{\"completed_at\":1700000000000,\"duration_ms\":5000}}}');").unwrap();
        drop(conn);
        for _ in 0..2 {
            let store = Store::open(&path).unwrap();
            let sessions = store.sessions().unwrap();
            assert_eq!(sessions[0].updated_at, 1_700_000_000_000);
            assert_eq!(sessions[1].updated_at, 0, "unknown ages are not invented");
            assert!(sessions.iter().all(|session| !session.archived));
            assert_eq!(
                store.load(1).unwrap().rows[0].sent_at,
                Some(1_699_999_995_000)
            );
            assert_eq!(store.load(2).unwrap().rows[0].sent_at, None);
        }
    }
    #[test]
    fn send_times_survive_streaming_restart_and_queued_delivery() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("native.db");
        let mut store = Store::open(&path).unwrap();
        let session = store.create("codex", "/project").unwrap();
        let sent_at = Some(1_700_000_000_000);
        let mut transcript = Transcript::default();
        transcript.begin("user");
        transcript.rows[0].sent_at = sent_at;
        transcript.append(&"A long queued prompt. ".repeat(300));
        assert!(transcript.rows.len() > 1);
        assert!(transcript.rows[1..].iter().all(|r| r.sent_at.is_none()));
        let answer = transcript.begin("assistant");
        transcript.append("Starting");
        store.write_rows(session.id, 0, &transcript.rows).unwrap();
        let answer_time = transcript.rows[answer].sent_at;
        transcript.append(" and finished");
        store
            .write_rows(session.id, answer, &transcript.rows[answer..])
            .unwrap();
        let mut roster = crate::mono::Roster::default();
        let mono = roster.create(Vec::new()).id.clone();
        roster
            .get_mut(&mono)
            .unwrap()
            .outbox
            .push(crate::mono::Message {
                id: "queued".into(),
                text: "Tomorrow".into(),
                sent_at,
                internal: false,
                error: None,
                attachments: Vec::new(),
            });
        roster.save(&store).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let saved = store.load(session.id).unwrap();
        assert_eq!(saved.rows, transcript.rows);
        assert_eq!(saved.rows[answer].sent_at, answer_time);
        assert_eq!(
            crate::mono::Roster::load(&store).unwrap().monos[0].outbox[0].sent_at,
            sent_at
        );
        let legacy: Row =
            serde_json::from_str(r#"{"role":"user","text":"Old","first":true}"#).unwrap();
        assert_eq!(legacy.sent_at, None);
    }
    #[test]
    fn delayed_configuration_cannot_restore_an_old_model_or_provider() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/tmp").unwrap();
        let old = crate::agent::Settings {
            revision: 1,
            model: Some("first-model".into()),
            ..Default::default()
        };
        let current = crate::agent::Settings {
            revision: 2,
            model: Some("selected-model".into()),
            effort: Some("high".into()),
            ..Default::default()
        };
        store
            .configure_agent(session.id, "claude", &current)
            .unwrap();
        store.provider_id(session.id, "new-thread").unwrap();
        store.configure_agent(session.id, "codex", &old).unwrap();
        let saved = store.agent_settings(session.id).unwrap().unwrap();
        assert_eq!(saved.model, current.model);
        assert_eq!(saved.effort, current.effort);
        let session = store.sessions().unwrap().remove(0);
        assert_eq!(session.provider, "claude");
        assert_eq!(session.provider_id.as_deref(), Some("new-thread"));
    }
    #[test]
    fn wal_restart_preserves_partial_output_and_provider_context() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("native.db");
        let mut store = Store::open(&path).unwrap();
        let s = store.create("codex", "/tmp").unwrap();
        assert!(!s.has_messages);
        assert!(!store.sessions().unwrap()[0].has_messages);
        let mut t = Transcript::default();
        t.begin("user");
        t.append("hello");
        t.begin("assistant");
        t.append("partial 🙂");
        store.write_rows(s.id, 0, &t.rows).unwrap();
        store.provider_id(s.id, "thread-1").unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(store.sessions().unwrap()[0].has_messages);
        assert_eq!(store.load(s.id).unwrap().rows, t.rows);
        assert_eq!(
            store.sessions().unwrap()[0].provider_id.as_deref(),
            Some("thread-1")
        );
    }
}
