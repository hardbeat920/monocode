use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  cwd TEXT NOT NULL,
  harness TEXT NOT NULL,
  model TEXT NOT NULL,
  model_settings TEXT NOT NULL DEFAULT '{}',
  runtime_mode TEXT NOT NULL,
  title TEXT NOT NULL,
  provider_session_id TEXT,
  blocks_json TEXT NOT NULL DEFAULT '[]',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS sessions_cwd_updated_idx
  ON sessions (cwd, updated_at DESC);
"#;

pub struct SessionStore {
    conn: Mutex<Connection>,
}

impl SessionStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(|e| e.to_string())?;
        migrate(&conn).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| e.to_string())?;
        migrate(&conn).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub(crate) fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "Session store is locked".into())
    }
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let store = SessionStore::open(data_dir.join("monocode.db"))?;
    app.manage(store);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpsert {
    pub id: String,
    pub cwd: String,
    pub harness: String,
    pub model: String,
    pub model_settings: Value,
    pub runtime_mode: String,
    pub title: String,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    pub blocks: Value,
    /// Last context-window reading reported by the harness, if any.
    #[serde(default)]
    pub context_used: Option<i64>,
    #[serde(default)]
    pub context_window: Option<i64>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub worktree_cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub cwd: String,
    pub harness: String,
    pub model: String,
    pub runtime_mode: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub additions: i64,
    pub deletions: i64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub cwd: String,
    pub harness: String,
    pub model: String,
    pub model_settings: Value,
    pub runtime_mode: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    pub blocks: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_used: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[tauri::command(async)]
pub fn session_upsert(
    store: State<'_, SessionStore>,
    session: SessionUpsert,
) -> Result<SessionSummary, String> {
    validate_id(&session.id, "session")?;
    if session.cwd.trim().is_empty() {
        return Err("cwd is required".into());
    }
    if let Some(provider_session_id) = &session.provider_session_id {
        if !provider_session_id.is_empty() {
            validate_id(provider_session_id, "provider session")?;
        }
    }
    if !session.model_settings.is_object() {
        return Err("modelSettings must be an object".into());
    }
    if !session.blocks.is_array() {
        return Err("blocks must be an array".into());
    }

    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    upsert_session(&conn, &session).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn session_list_by_project(
    store: State<'_, SessionStore>,
    cwd: String,
) -> Result<Vec<SessionSummary>, String> {
    if cwd.trim().is_empty() {
        return Err("cwd is required".into());
    }
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    list_by_project(&conn, &cwd).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn session_get(
    store: State<'_, SessionStore>,
    session_id: String,
) -> Result<Option<SessionRecord>, String> {
    validate_id(&session_id, "session")?;
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    get_session(&conn, &session_id).map_err(|e| e.to_string())
}

const MAX_SEARCH_SCAN: usize = 400;
const MAX_CONVERSATION_HITS: usize = 40;
const MAX_MESSAGE_HITS: usize = 40;
const SNIPPET_RADIUS: usize = 42;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchOptions {
    pub query: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    pub kind: String,
    pub session_id: String,
    pub cwd: String,
    pub harness: String,
    pub title: String,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchResult {
    pub hits: Vec<SessionSearchHit>,
    pub truncated: bool,
}

#[tauri::command(async)]
pub fn session_search(
    store: State<'_, SessionStore>,
    options: SessionSearchOptions,
) -> Result<SessionSearchResult, String> {
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    search_sessions(&conn, &options).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn session_delete(store: State<'_, SessionStore>, session_id: String) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    delete_session(&conn, &session_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn session_set_archived(
    store: State<'_, SessionStore>,
    session_id: String,
    archived: bool,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    set_archived(&conn, &session_id, archived).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn session_set_pinned(
    store: State<'_, SessionStore>,
    session_id: String,
    pinned: bool,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    set_pinned(&conn, &session_id, pinned).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlightSession {
    pub session_id: String,
    pub cwd: String,
}

#[tauri::command(async)]
pub fn session_set_in_flight(
    store: State<'_, SessionStore>,
    sessions: Vec<InFlightSession>,
) -> Result<(), String> {
    for session in &sessions {
        validate_id(&session.session_id, "session")?;
        if session.cwd.trim().is_empty() {
            return Err("cwd is required".into());
        }
    }
    let mut conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    replace_in_flight(&mut conn, &sessions).map_err(|e| e.to_string())
}

/// Read the quit snapshot without clearing it. Vite/dev reloads must not
/// consume the only copy.
#[tauri::command(async)]
pub fn session_list_in_flight(
    store: State<'_, SessionStore>,
) -> Result<Vec<InFlightSession>, String> {
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    list_in_flight(&conn).map_err(|e| e.to_string())
}

/// Read and clear the quit snapshot so a restored window cannot take it twice.
#[tauri::command(async)]
pub fn session_take_in_flight(
    store: State<'_, SessionStore>,
) -> Result<Vec<InFlightSession>, String> {
    let mut conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    take_in_flight(&mut conn).map_err(|e| e.to_string())
}

const WORKSPACE_SNAPSHOT_MAX_BYTES: usize = 2_000_000;

#[tauri::command(async)]
pub fn workspace_set_snapshot(
    store: State<'_, SessionStore>,
    snapshot: Value,
) -> Result<(), String> {
    if !snapshot.is_object() {
        return Err("workspace snapshot must be an object".into());
    }
    let json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    if json.len() > WORKSPACE_SNAPSHOT_MAX_BYTES {
        return Err("workspace snapshot is too large".into());
    }
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    set_workspace_snapshot(&conn, &json).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn workspace_get_snapshot(store: State<'_, SessionStore>) -> Result<Option<Value>, String> {
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    let json = get_workspace_snapshot(&conn).map_err(|e| e.to_string())?;
    match json {
        None => Ok(None),
        Some(raw) => serde_json::from_str(&raw).map_err(|e| e.to_string()),
    }
}

/// Add a `sessions` column when it is absent, so a half-applied history cannot
/// leave the schema short of what the queries select.
fn ensure_session_column(conn: &Connection, column: &str, decl: &str) -> rusqlite::Result<()> {
    let present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = ?1",
        params![column],
        |row| row.get(0),
    )?;
    if present == 0 {
        conn.execute(
            &format!("ALTER TABLE sessions ADD COLUMN {column} {decl}"),
            [],
        )?;
    }
    Ok(())
}

/// Add a `sessions` column when it is missing, tolerating the case where a
/// recorded migration version has no matching column.
fn ensure_column(conn: &Connection, name: &str, decl: &str) -> rusqlite::Result<()> {
    let present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )?;
    if present > 0 {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE sessions ADD COLUMN {name} {decl}"),
        [],
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at INTEGER NOT NULL
         )",
        [],
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if current < 1 {
        conn.execute_batch(MIGRATION_V1)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 2 {
        conn.execute("ALTER TABLE sessions ADD COLUMN branch TEXT", [])?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (2, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 3 {
        conn.execute("ALTER TABLE sessions ADD COLUMN context_used INTEGER", [])?;
        conn.execute("ALTER TABLE sessions ADD COLUMN context_window INTEGER", [])?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (3, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 4 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS in_flight_sessions (
               session_id TEXT PRIMARY KEY,
               cwd TEXT NOT NULL,
               sort_index INTEGER NOT NULL
             );",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (4, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 5 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspace_snapshot (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               snapshot_json TEXT NOT NULL,
               updated_at INTEGER NOT NULL
             );",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (5, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 6 {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (6, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 7 {
        conn.execute("ALTER TABLE sessions ADD COLUMN worktree_cwd TEXT", [])?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (7, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 8 {
        // `list_by_project` used to filter with
        // `blocks_json LIKE '%"role":"user"%'`, which reads and substring-scans
        // every transcript in the project on each switch. Materialize the
        // predicate so the covering index can answer it instead.
        ensure_column(conn, "has_user_message", "INTEGER NOT NULL DEFAULT 0")?;
        conn.execute(
            "UPDATE sessions SET has_user_message =
               CASE WHEN blocks_json != '[]' AND blocks_json LIKE '%\"role\":\"user\"%'
                    THEN 1 ELSE 0 END",
            [],
        )?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_updated_idx;
             CREATE INDEX IF NOT EXISTS sessions_cwd_listed_idx
               ON sessions (cwd, has_user_message, updated_at DESC);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (8, ?1)",
            params![now_millis()],
        )?;
    }
    // A recorded version row can outlive the schema it describes when another
    // build reuses the same numbers, leaving columns that every listing query
    // (and the covering index below) depends on missing. Repair before use.
    for (column, decl) in [
        ("branch", "TEXT"),
        ("context_used", "INTEGER"),
        ("context_window", "INTEGER"),
        ("archived", "INTEGER NOT NULL DEFAULT 0"),
        ("worktree_cwd", "TEXT"),
        ("has_user_message", "INTEGER NOT NULL DEFAULT 0"),
        ("pinned", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        ensure_session_column(conn, column, decl)?;
    }
    if current < 9 {
        // v8 stopped the blob scan but still cost a table seek per row, and
        // every summary column (`created_at`, `updated_at`, `branch`,
        // `archived`) is stored *after* `blocks_json` in the record — so
        // reaching them meant walking past a ~180 KB transcript's overflow
        // pages, 120 times over, on each project switch. Widening the index to
        // cover the whole projection keeps the query inside the index and off
        // the table entirely: measured 7.3 ms -> 0.24 ms for 120 sessions.
        // Version rows are not proof the columns landed: a DB can carry a
        // recorded version from another build without the ALTER that went with
        // it, and indexing a missing column aborts the whole migration.
        ensure_column(conn, "branch", "TEXT")?;
        ensure_column(conn, "archived", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(conn, "has_user_message", "INTEGER NOT NULL DEFAULT 0")?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_listed_idx;
             CREATE INDEX IF NOT EXISTS sessions_cwd_cover_idx
               ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                            model, runtime_mode, title, provider_session_id,
                            created_at, branch, archived);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (9, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 10 {
        crate::notes::ensure_notes_table(conn)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (10, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 11 {
        // Pinning is a listing column, so it has to live in the covering
        // index with `archived`. Selecting it off the table would walk past
        // `blocks_json` on every project switch.
        ensure_column(conn, "pinned", "INTEGER NOT NULL DEFAULT 0")?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_cover_idx;
             CREATE INDEX IF NOT EXISTS sessions_cwd_cover_idx
               ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                            model, runtime_mode, title, provider_session_id,
                            created_at, branch, archived, pinned);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (11, ?1)",
            params![now_millis()],
        )?;
    }
    // Create even when a version row already exists (another build may have
    // used the same numbers, or a previous run recorded the version without
    // the table). Restore writes into these; missing tables look like a
    // blank homepage.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS in_flight_sessions (
           session_id TEXT PRIMARY KEY,
           cwd TEXT NOT NULL,
           sort_index INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS workspace_snapshot (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           snapshot_json TEXT NOT NULL,
           updated_at INTEGER NOT NULL
         );",
    )?;
    crate::notes::ensure_notes_table(conn)?;
    Ok(())
}

fn upsert_session(conn: &Connection, session: &SessionUpsert) -> rusqlite::Result<SessionSummary> {
    let now = now_millis();
    let model_settings = serde_json::to_string(&session.model_settings)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let blocks_json = serde_json::to_string(&session.blocks)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let provider_session_id = session
        .provider_session_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let git = crate::fs::git_info_for(&crate::fs::expand_home(&session.cwd));
    let branch = session
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| git.branch.as_deref().filter(|value| !value.is_empty()));
    let worktree_cwd = session
        .worktree_cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let has_user_message = has_user_block(&session.blocks);

    let existing: Option<(i64, i64, String, i64, i64)> = conn
        .query_row(
            "SELECT created_at, updated_at, blocks_json, archived, pinned FROM sessions WHERE id = ?1",
            params![session.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()?;
    let created_at = existing
        .as_ref()
        .map(|(value, _, _, _, _)| *value)
        .unwrap_or(now);
    let updated_at = match &existing {
        Some((_, prev_updated, prev_blocks, _, _)) if json_eq(prev_blocks, &session.blocks) => {
            *prev_updated
        }
        _ => now,
    };
    let archived = existing
        .as_ref()
        .map(|(_, _, _, value, _)| *value != 0)
        .unwrap_or(false);
    let pinned = existing
        .as_ref()
        .map(|(_, _, _, _, value)| *value != 0)
        .unwrap_or(false);

    conn.execute(
        "INSERT INTO sessions (
           id, cwd, harness, model, model_settings, runtime_mode, title,
           provider_session_id, blocks_json, created_at, updated_at, branch,
           context_used, context_window, worktree_cwd, has_user_message
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(id) DO UPDATE SET
           cwd = excluded.cwd,
           harness = excluded.harness,
           model = excluded.model,
           model_settings = excluded.model_settings,
           runtime_mode = excluded.runtime_mode,
           title = excluded.title,
           provider_session_id = excluded.provider_session_id,
           blocks_json = excluded.blocks_json,
           updated_at = excluded.updated_at,
           branch = excluded.branch,
           context_used = excluded.context_used,
           context_window = excluded.context_window,
           worktree_cwd = excluded.worktree_cwd,
           has_user_message = excluded.has_user_message",
        params![
            session.id,
            session.cwd,
            session.harness,
            session.model,
            model_settings,
            session.runtime_mode,
            session.title,
            provider_session_id,
            blocks_json,
            created_at,
            updated_at,
            branch,
            session.context_used,
            session.context_window,
            worktree_cwd,
            i64::from(has_user_message),
        ],
    )?;

    Ok(SessionSummary {
        id: session.id.clone(),
        cwd: session.cwd.clone(),
        harness: session.harness.clone(),
        model: session.model.clone(),
        runtime_mode: session.runtime_mode.clone(),
        title: session.title.clone(),
        provider_session_id: provider_session_id.map(str::to_owned),
        branch: branch.map(str::to_owned),
        repo: git.repo,
        additions: 0,
        deletions: 0,
        created_at,
        updated_at,
        archived,
        pinned,
    })
}

fn search_sessions(
    conn: &Connection,
    options: &SessionSearchOptions,
) -> rusqlite::Result<SessionSearchResult> {
    let query = options.query.trim();
    if query.is_empty() {
        return Ok(SessionSearchResult {
            hits: Vec::new(),
            truncated: false,
        });
    }
    let needle = query.to_lowercase();
    let pattern = like_pattern(query);
    let cwd = options
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut sql = String::from(
        "SELECT id, cwd, harness, title, updated_at, archived, blocks_json
         FROM sessions
         WHERE blocks_json != '[]'
           AND blocks_json LIKE '%\"role\":\"user\"%'
           AND (LOWER(title) LIKE LOWER(?1) ESCAPE '\\'
                OR LOWER(blocks_json) LIKE LOWER(?1) ESCAPE '\\')",
    );
    if !options.include_archived {
        sql.push_str(" AND archived = 0");
    }
    if cwd.is_some() {
        sql.push_str(" AND cwd = ?2");
        sql.push_str(" ORDER BY updated_at DESC, id ASC LIMIT ?3");
    } else {
        sql.push_str(" ORDER BY updated_at DESC, id ASC LIMIT ?2");
    }

    let mut statement = conn.prepare(&sql)?;
    let limit = (MAX_SEARCH_SCAN as i64) + 1;
    let rows = if let Some(cwd) = cwd {
        statement.query_map(params![pattern, cwd, limit], search_row)?
    } else {
        statement.query_map(params![pattern, limit], search_row)?
    };

    let mut conversations = Vec::new();
    let mut messages = Vec::new();
    let mut scanned = 0;
    let mut truncated = false;
    for row in rows {
        let (id, cwd, harness, title, updated_at, blocks_raw) = row?;
        scanned += 1;
        if scanned > MAX_SEARCH_SCAN {
            truncated = true;
            break;
        }

        let title_hit = title.to_lowercase().contains(&needle);
        if title_hit && conversations.len() < MAX_CONVERSATION_HITS {
            conversations.push(SessionSearchHit {
                kind: "conversation".into(),
                session_id: id.clone(),
                cwd: cwd.clone(),
                harness: harness.clone(),
                title: title.clone(),
                updated_at,
                block_id: None,
                role: None,
                preview: String::new(),
            });
        }

        if messages.len() >= MAX_MESSAGE_HITS {
            if title_hit {
                continue;
            }
            if conversations.len() >= MAX_CONVERSATION_HITS {
                truncated = true;
                break;
            }
            continue;
        }

        let Ok(blocks) = serde_json::from_str::<Value>(&blocks_raw) else {
            continue;
        };
        for hit in block_hits(&blocks, &needle) {
            messages.push(SessionSearchHit {
                kind: "message".into(),
                session_id: id.clone(),
                cwd: cwd.clone(),
                harness: harness.clone(),
                title: title.clone(),
                updated_at,
                block_id: Some(hit.0),
                role: Some(hit.1),
                preview: hit.2,
            });
            if messages.len() >= MAX_MESSAGE_HITS {
                truncated = true;
                break;
            }
        }
    }

    if conversations.len() >= MAX_CONVERSATION_HITS {
        truncated = true;
    }

    let mut hits = conversations;
    hits.extend(messages);
    Ok(SessionSearchResult { hits, truncated })
}

fn search_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, String, i64, String)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(6)?,
    ))
}

fn like_pattern(query: &str) -> String {
    let mut out = String::from("%");
    for ch in query.chars().take(200) {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('%');
    out
}

fn block_hits(blocks: &Value, needle: &str) -> Vec<(String, String, String)> {
    let Some(items) = blocks.as_array() else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for block in items {
        let role = block
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(role, "user" | "assistant" | "tool" | "tasks" | "plan") {
            continue;
        }
        let id = block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() {
            continue;
        }
        for text in block_texts(block) {
            if !text.to_lowercase().contains(needle) {
                continue;
            }
            hits.push((id.clone(), role.to_string(), snippet_around(&text, needle)));
            break;
        }
    }
    hits
}

fn block_texts(block: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    push_text(&mut texts, block.get("text"));
    if let Some(tool) = block.get("tool") {
        push_text(&mut texts, tool.get("title"));
        push_text(&mut texts, tool.get("detail"));
        if let Some(preview) = tool.get("preview") {
            push_text(&mut texts, preview.get("query"));
            push_text(&mut texts, preview.get("path"));
            push_text(&mut texts, preview.get("output"));
            push_text(&mut texts, preview.get("title"));
        }
    }
    texts
}

fn push_text(texts: &mut Vec<String>, value: Option<&Value>) {
    if let Some(text) = value.and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            texts.push(trimmed.to_string());
        }
    }
}

fn snippet_around(text: &str, needle: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = compact.to_lowercase();
    let Some(index) = lower.find(needle) else {
        if compact.chars().count() <= SNIPPET_RADIUS * 2 {
            return compact;
        }
        return format!(
            "{}…",
            compact.chars().take(SNIPPET_RADIUS * 2).collect::<String>()
        );
    };
    let start = floor_char_boundary(&compact, index.saturating_sub(SNIPPET_RADIUS));
    let end = ceil_char_boundary(
        &compact,
        (index + needle.len() + SNIPPET_RADIUS).min(compact.len()),
    );
    let mut snippet = compact[start..end].trim().to_string();
    if start > 0 {
        snippet = format!("…{snippet}");
    }
    if end < compact.len() {
        snippet = format!("{snippet}…");
    }
    snippet
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn list_by_project(conn: &Connection, cwd: &str) -> rusqlite::Result<Vec<SessionSummary>> {
    let git = crate::fs::git_info_for(&crate::fs::expand_home(cwd));
    let mut statement = conn.prepare(
        "SELECT id, cwd, harness, model, runtime_mode, title, provider_session_id,
                created_at, updated_at, branch, archived, pinned
         FROM sessions
         WHERE cwd = ?1
           AND has_user_message = 1
         ORDER BY updated_at DESC, id ASC",
    )?;
    let rows = statement.query_map(params![cwd], |row| {
        let stored_branch: Option<String> = row.get(9)?;
        let archived: i64 = row.get(10)?;
        let pinned: i64 = row.get(11)?;
        Ok(SessionSummary {
            id: row.get(0)?,
            cwd: row.get(1)?,
            harness: row.get(2)?,
            model: row.get(3)?,
            runtime_mode: row.get(4)?,
            title: row.get(5)?,
            provider_session_id: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            branch: nonempty(stored_branch).or_else(|| git.branch.clone()),
            repo: git.repo.clone(),
            additions: 0,
            deletions: 0,
            archived: archived != 0,
            pinned: pinned != 0,
        })
    })?;
    rows.collect()
}

/// Mirrors the sidebar's notion of a listable session: a transcript that the
/// user has actually said something in.
fn has_user_block(blocks: &Value) -> bool {
    blocks.as_array().is_some_and(|blocks| {
        blocks
            .iter()
            .any(|block| block.get("role").and_then(Value::as_str) == Some("user"))
    })
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn json_eq(raw: &str, incoming: &Value) -> bool {
    match serde_json::from_str::<Value>(raw) {
        Ok(previous) => previous == *incoming,
        Err(_) => false,
    }
}

fn delete_session(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
    Ok(())
}

fn set_archived(conn: &Connection, session_id: &str, archived: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET archived = ?1 WHERE id = ?2",
        params![if archived { 1 } else { 0 }, session_id],
    )?;
    Ok(())
}

fn set_pinned(conn: &Connection, session_id: &str, pinned: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET pinned = ?1 WHERE id = ?2",
        params![if pinned { 1 } else { 0 }, session_id],
    )?;
    Ok(())
}

fn get_session(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<SessionRecord>> {
    conn.query_row(
        "SELECT id, cwd, harness, model, model_settings, runtime_mode, title,
                provider_session_id, blocks_json, created_at, updated_at,
                context_used, context_window, branch, worktree_cwd
         FROM sessions
         WHERE id = ?1",
        params![session_id],
        |row| {
            let model_settings_raw: String = row.get(4)?;
            let blocks_raw: String = row.get(8)?;
            let model_settings = serde_json::from_str(&model_settings_raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let blocks = serde_json::from_str(&blocks_raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(SessionRecord {
                id: row.get(0)?,
                cwd: row.get(1)?,
                harness: row.get(2)?,
                model: row.get(3)?,
                model_settings,
                runtime_mode: row.get(5)?,
                title: row.get(6)?,
                provider_session_id: row.get(7)?,
                blocks,
                context_used: row.get(11)?,
                context_window: row.get(12)?,
                branch: row.get(13)?,
                worktree_cwd: row.get(14)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
    .optional()
}

fn list_in_flight(conn: &Connection) -> rusqlite::Result<Vec<InFlightSession>> {
    let mut statement = conn.prepare(
        "SELECT session_id, cwd FROM in_flight_sessions ORDER BY sort_index ASC, session_id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(InFlightSession {
            session_id: row.get(0)?,
            cwd: row.get(1)?,
        })
    })?;
    rows.collect()
}

fn replace_in_flight(conn: &mut Connection, sessions: &[InFlightSession]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM in_flight_sessions", [])?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO in_flight_sessions (session_id, cwd, sort_index)
             VALUES (?1, ?2, ?3)",
        )?;
        for (index, session) in sessions.iter().enumerate() {
            insert.execute(params![session.session_id, session.cwd, index as i64])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn take_in_flight(conn: &mut Connection) -> rusqlite::Result<Vec<InFlightSession>> {
    let tx = conn.transaction()?;
    let sessions = {
        let mut statement = tx.prepare(
            "SELECT session_id, cwd FROM in_flight_sessions ORDER BY sort_index ASC, session_id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(InFlightSession {
                session_id: row.get(0)?,
                cwd: row.get(1)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    tx.execute("DELETE FROM in_flight_sessions", [])?;
    tx.commit()?;
    Ok(sessions)
}

fn set_workspace_snapshot(conn: &Connection, json: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO workspace_snapshot (id, snapshot_json, updated_at)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
           snapshot_json = excluded.snapshot_json,
           updated_at = excluded.updated_at",
        params![json, now_millis()],
    )?;
    Ok(())
}

fn get_workspace_snapshot(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT snapshot_json FROM workspace_snapshot WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .optional()
}

pub(crate) fn validate_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("Invalid {label} id"));
    }
    Ok(())
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(id: &str, cwd: &str, title: &str) -> SessionUpsert {
        SessionUpsert {
            id: id.into(),
            cwd: cwd.into(),
            harness: "cursor".into(),
            model: "gpt-5".into(),
            model_settings: json!({ "thinking": "high" }),
            runtime_mode: "supervised".into(),
            title: title.into(),
            provider_session_id: Some("acp-session-1".into()),
            blocks: json!([{ "id": "b1", "role": "user", "text": "hello" }]),
            context_used: None,
            context_window: None,
            branch: None,
            worktree_cwd: None,
        }
    }

    /// The sidebar query must stay answerable from the index alone. Selecting a
    /// column the index does not carry silently reintroduces a table seek per
    /// row, and every summary column sits behind a ~180 KB `blocks_json` blob
    /// in the record, so that regression is worth ~30x on a real project.
    #[test]
    fn list_by_project_is_served_by_a_covering_index() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "A1")).unwrap();
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN
                 SELECT id, cwd, harness, model, runtime_mode, title, provider_session_id,
                        created_at, updated_at, branch, archived, pinned
                 FROM sessions
                 WHERE cwd = ?1
                   AND has_user_message = 1
                 ORDER BY updated_at DESC, id ASC",
                params!["/tmp/a"],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            plan.contains("COVERING INDEX"),
            "sidebar listing fell back to table seeks: {plan}"
        );
    }

    #[test]
    fn upsert_tracks_whether_a_user_has_spoken() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut quiet = sample("s1", "/tmp/a", "Quiet");
        quiet.blocks = json!([{ "id": "b1", "role": "assistant", "text": "hi" }]);
        upsert_session(&conn, &quiet).unwrap();
        assert!(list_by_project(&conn, "/tmp/a").unwrap().is_empty());

        // The flag has to follow the transcript, not just the first write.
        quiet.blocks = json!([
            { "id": "b1", "role": "assistant", "text": "hi" },
            { "id": "b2", "role": "user", "text": "hello" }
        ]);
        upsert_session(&conn, &quiet).unwrap();
        assert_eq!(list_by_project(&conn, "/tmp/a").unwrap().len(), 1);
    }

    #[test]
    fn migrate_creates_sessions_table() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, 1);
        let branch: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'branch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(branch, 1);
    }

    #[test]
    fn upsert_preserves_created_at_and_updates_fields() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let first = upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut next = sample("s1", "/tmp/a", "Updated");
        next.provider_session_id = Some("acp-session-2".into());
        next.blocks = json!([
            { "id": "b1", "role": "user", "text": "hello" },
            { "id": "b2", "role": "assistant", "text": "world" }
        ]);
        let second = upsert_session(&conn, &next).unwrap();
        assert_eq!(second.created_at, first.created_at);
        assert!(second.updated_at > first.updated_at);
        assert_eq!(second.title, "Updated");
        assert_eq!(second.provider_session_id.as_deref(), Some("acp-session-2"));
    }

    #[test]
    fn context_usage_round_trips() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut row = sample("s1", "/tmp/a", "First");
        row.context_used = Some(29_821);
        row.context_window = Some(1_000_000);
        upsert_session(&conn, &row).unwrap();
        let stored = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(stored.context_used, Some(29_821));
        assert_eq!(stored.context_window, Some(1_000_000));
    }

    #[test]
    fn context_usage_is_absent_until_a_harness_reports() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        let stored = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(stored.context_used, None);
        assert_eq!(stored.context_window, None);
    }

    #[test]
    fn migration_v3_adds_context_columns() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn upsert_same_blocks_keeps_updated_at() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let first = upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut next = sample("s1", "/tmp/a", "First");
        next.model = "gpt-5.4".into();
        let second = upsert_session(&conn, &next).unwrap();
        assert_eq!(second.updated_at, first.updated_at);
        assert_eq!(second.model, "gpt-5.4");
    }

    #[test]
    fn list_does_not_sum_write_preview_diffs() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "Diffs");
        session.blocks = json!([
            { "id": "b1", "role": "user", "text": "edit it" },
            {
                "id": "b2",
                "role": "tool",
                "text": "",
                "tool": {
                    "preview": {
                        "kind": "write",
                        "path": "src/a.ts",
                        "additions": 12,
                        "deletions": 3
                    }
                }
            }
        ]);
        let summary = upsert_session(&conn, &session).unwrap();
        assert_eq!(summary.additions, 0);
        assert_eq!(summary.deletions, 0);
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(listed[0].additions, 0);
        assert_eq!(listed[0].deletions, 0);
    }

    #[test]
    fn list_by_project_filters_and_orders() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "A1")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        upsert_session(&conn, &sample("s2", "/tmp/a", "A2")).unwrap();
        upsert_session(&conn, &sample("s3", "/tmp/b", "B1")).unwrap();
        let mut empty = sample("s4", "/tmp/a", "Empty");
        empty.blocks = json!([]);
        upsert_session(&conn, &empty).unwrap();
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "s2");
        assert_eq!(listed[1].id, "s1");
    }

    #[test]
    fn delete_removes_session() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        delete_session(&conn, "s1").unwrap();
        assert!(get_session(&conn, "s1").unwrap().is_none());
        assert!(list_by_project(&conn, "/tmp/a").unwrap().is_empty());
    }

    #[test]
    fn migration_v6_adds_archived_column() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 6",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let archived: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'archived'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archived, 1);
    }

    #[test]
    fn migration_v7_adds_worktree_cwd_column() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 7",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let column: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'worktree_cwd'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(column, 1);
    }

    #[test]
    fn archive_round_trips_and_survives_upsert() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        set_archived(&conn, "s1", true).unwrap();
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert!(listed[0].archived);
        let mut next = sample("s1", "/tmp/a", "Updated");
        next.blocks = json!([
            { "id": "b1", "role": "user", "text": "hello" },
            { "id": "b2", "role": "assistant", "text": "world" }
        ]);
        let summary = upsert_session(&conn, &next).unwrap();
        assert!(summary.archived);
        assert_eq!(summary.title, "Updated");
        set_archived(&conn, "s1", false).unwrap();
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert!(!listed[0].archived);
    }

    #[test]
    fn migration_v11_adds_pinned_column() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 11",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let pinned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'pinned'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pinned, 1);
    }

    #[test]
    fn pin_round_trips_and_survives_upsert() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        set_pinned(&conn, "s1", true).unwrap();
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert!(listed[0].pinned);
        let mut next = sample("s1", "/tmp/a", "Updated");
        next.blocks = json!([
            { "id": "b1", "role": "user", "text": "hello" },
            { "id": "b2", "role": "assistant", "text": "world" }
        ]);
        let summary = upsert_session(&conn, &next).unwrap();
        assert!(summary.pinned);
        assert_eq!(summary.title, "Updated");
        set_pinned(&conn, "s1", false).unwrap();
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert!(!listed[0].pinned);
    }

    #[test]
    fn get_round_trips_blocks_and_provider_session_id() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "First")).unwrap();
        let record = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(record.id, "s1");
        assert_eq!(record.provider_session_id.as_deref(), Some("acp-session-1"));
        assert_eq!(record.model_settings["thinking"], "high");
        assert_eq!(record.blocks.as_array().unwrap().len(), 1);
        assert_eq!(record.blocks[0]["text"], "hello");
    }

    #[test]
    fn upsert_snapshots_git_branch() {
        let dir = std::env::temp_dir().join(format!(
            "monocode-session-git-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let init = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output();
        let Ok(init) = init else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        if !init.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let head = std::process::Command::new("git")
            .args(["symbolic-ref", "HEAD", "refs/heads/fix-sidebar"])
            .current_dir(&dir)
            .status();
        if head.map(|status| !status.success()).unwrap_or(true) {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let origin = std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git",
            ])
            .current_dir(&dir)
            .status();
        if origin.map(|status| !status.success()).unwrap_or(true) {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        let cwd = dir.to_string_lossy().into_owned();
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let summary = upsert_session(&conn, &sample("s1", &cwd, "First")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(summary.branch.as_deref(), Some("fix-sidebar"));
        assert_eq!(summary.repo.as_deref(), Some("widget"));
    }

    #[test]
    fn upsert_keeps_session_branch_and_worktree() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "First");
        session.branch = Some("feat/picker".into());
        session.worktree_cwd = Some("/tmp/a-feat".into());
        let summary = upsert_session(&conn, &session).unwrap();
        assert_eq!(summary.branch.as_deref(), Some("feat/picker"));
        let record = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(record.branch.as_deref(), Some("feat/picker"));
        assert_eq!(record.worktree_cwd.as_deref(), Some("/tmp/a-feat"));
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(listed[0].branch.as_deref(), Some("feat/picker"));
    }

    #[test]
    fn migration_v4_creates_in_flight_table() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 4",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'in_flight_sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, 1);
    }

    #[test]
    fn in_flight_set_take_is_ordered_and_destructive() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.conn.lock().unwrap();
        replace_in_flight(
            &mut conn,
            &[
                InFlightSession {
                    session_id: "s2".into(),
                    cwd: "/tmp/b".into(),
                },
                InFlightSession {
                    session_id: "s1".into(),
                    cwd: "/tmp/a".into(),
                },
            ],
        )
        .unwrap();
        let first = take_in_flight(&mut conn).unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].session_id, "s2");
        assert_eq!(first[0].cwd, "/tmp/b");
        assert_eq!(first[1].session_id, "s1");
        let second = take_in_flight(&mut conn).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn in_flight_list_does_not_clear() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.conn.lock().unwrap();
        replace_in_flight(
            &mut conn,
            &[InFlightSession {
                session_id: "s1".into(),
                cwd: "/tmp/a".into(),
            }],
        )
        .unwrap();
        let listed = list_in_flight(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, "s1");
        let listed_again = list_in_flight(&conn).unwrap();
        assert_eq!(listed_again.len(), 1);
    }

    #[test]
    fn in_flight_replace_clears_previous() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.conn.lock().unwrap();
        replace_in_flight(
            &mut conn,
            &[InFlightSession {
                session_id: "old".into(),
                cwd: "/tmp/old".into(),
            }],
        )
        .unwrap();
        replace_in_flight(&mut conn, &[]).unwrap();
        assert!(take_in_flight(&mut conn).unwrap().is_empty());
    }

    #[test]
    fn migration_v5_creates_workspace_snapshot_table() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 5",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'workspace_snapshot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, 1);
    }

    #[test]
    fn workspace_snapshot_round_trips_and_replaces() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        set_workspace_snapshot(&conn, r#"{"tabs":[{"id":"t1"}]}"#).unwrap();
        let first = get_workspace_snapshot(&conn).unwrap().unwrap();
        assert!(first.contains("t1"));
        set_workspace_snapshot(&conn, r#"{"tabs":[{"id":"t2"}]}"#).unwrap();
        let second = get_workspace_snapshot(&conn).unwrap().unwrap();
        assert!(second.contains("t2"));
        assert!(!second.contains("t1"));
    }

    #[test]
    fn migrate_creates_workspace_tables_when_versions_already_recorded() {
        let path = std::env::temp_dir().join(format!(
            "monocode-stale-migrations-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at INTEGER NOT NULL
                 );
                 CREATE TABLE sessions (
                   id TEXT PRIMARY KEY,
                   cwd TEXT NOT NULL,
                   harness TEXT NOT NULL,
                   model TEXT NOT NULL,
                   model_settings TEXT NOT NULL DEFAULT '{}',
                   runtime_mode TEXT NOT NULL,
                   title TEXT NOT NULL,
                   provider_session_id TEXT,
                   blocks_json TEXT NOT NULL DEFAULT '[]',
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 INSERT INTO schema_migrations (version, applied_at)
                   VALUES (1, 1), (2, 1), (3, 1), (4, 1), (5, 1);",
            )
            .unwrap();
        }
        let store = SessionStore::open(path.clone()).unwrap();
        let conn = store.conn.lock().unwrap();
        let inflight: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'in_flight_sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'workspace_snapshot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);
        drop(store);
        let _ = std::fs::remove_file(&path);
        assert_eq!(inflight, 1);
        assert_eq!(snapshot, 1);
    }

    #[test]
    fn search_finds_title_and_message_hits() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "Fix sidebar search");
        session.blocks = json!([
            { "id": "u1", "role": "user", "text": "Please search the sidebar filter chips" },
            { "id": "a1", "role": "assistant", "text": "I will look through the explorer next." }
        ]);
        upsert_session(&conn, &session).unwrap();
        upsert_session(&conn, &sample("s2", "/tmp/b", "Unrelated title")).unwrap();

        let result = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "sidebar".into(),
                cwd: None,
                include_archived: false,
            },
        )
        .unwrap();
        let kinds: Vec<_> = result
            .hits
            .iter()
            .map(|hit| {
                (
                    hit.kind.as_str(),
                    hit.session_id.as_str(),
                    hit.block_id.as_deref(),
                )
            })
            .collect();
        assert!(kinds.contains(&("conversation", "s1", None)));
        assert!(kinds.contains(&("message", "s1", Some("u1"))));
        assert!(!result.hits.iter().any(|hit| hit.session_id == "s2"));
    }

    #[test]
    fn search_respects_cwd_and_skips_archived() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "Search project a")).unwrap();
        upsert_session(&conn, &sample("s2", "/tmp/b", "Search project b")).unwrap();
        set_archived(&conn, "s1", true).unwrap();

        let scoped = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "Search project".into(),
                cwd: Some("/tmp/a".into()),
                include_archived: false,
            },
        )
        .unwrap();
        assert!(scoped.hits.is_empty());

        let with_archived = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "Search project".into(),
                cwd: Some("/tmp/a".into()),
                include_archived: true,
            },
        )
        .unwrap();
        assert!(with_archived
            .hits
            .iter()
            .any(|hit| hit.session_id == "s1" && hit.kind == "conversation"));

        let other = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "Search project".into(),
                cwd: Some("/tmp/b".into()),
                include_archived: false,
            },
        )
        .unwrap();
        assert!(other.hits.iter().any(|hit| hit.session_id == "s2"));
    }

    #[test]
    fn search_empty_query_is_empty() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "Anything")).unwrap();
        let result = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "   ".into(),
                cwd: None,
                include_archived: false,
            },
        )
        .unwrap();
        assert!(result.hits.is_empty());
        assert!(!result.truncated);
    }
}
