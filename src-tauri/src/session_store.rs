use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

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
        crate::worktrees::reconcile_removals(&conn)?;
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
    #[serde(default)]
    pub provider_account_id: Option<String>,
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
    #[serde(default)]
    pub worktree_removed: bool,
    #[serde(default)]
    pub linked_work_item: Option<Value>,
    #[serde(default)]
    pub automation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_lead_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration: Option<Value>,
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
    pub worktree_cwd: Option<String>,
    #[serde(default)]
    pub worktree_removed: bool,
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
    #[serde(default)]
    pub draft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_work_item: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_lead_id: Option<String>,
    pub cwd: String,
    pub harness: String,
    pub model: String,
    pub model_settings: Value,
    pub runtime_mode: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_account_id: Option<String>,
    pub blocks: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_used: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    #[serde(default)]
    pub worktree_removed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_work_item: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
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
    if let Some(provider_account_id) = &session.provider_account_id {
        if !provider_account_id.is_empty() {
            validate_id(provider_account_id, "provider account")?;
        }
    }
    if let Some(automation_id) = &session.automation_id {
        if !automation_id.is_empty() {
            validate_id(automation_id, "automation")?;
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
pub fn session_rebase_project(
    store: State<'_, SessionStore>,
    from_cwd: String,
    to_cwd: String,
) -> Result<(), String> {
    if from_cwd.trim().is_empty() || to_cwd.trim().is_empty() {
        return Err("project paths are required".into());
    }
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    rebase_project(&conn, &from_cwd, &to_cwd).map_err(|error| error.to_string())
}

fn rebase_project(conn: &Connection, from_cwd: &str, to_cwd: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET cwd = ?2 WHERE cwd = ?1",
        params![from_cwd, to_cwd],
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn session_list_linked(store: State<'_, SessionStore>) -> Result<Vec<SessionSummary>, String> {
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    list_linked(&conn).map_err(|e| e.to_string())
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
const SEARCH_PROGRESS_INTERVAL: usize = 1_000;
const MAX_SEARCH_BUFFER_BYTES: usize = 8 * 1024 * 1024;
static SESSION_SEARCH_TOKENS: Mutex<Option<HashMap<String, Arc<AtomicU64>>>> = Mutex::new(None);

#[derive(Clone)]
struct SessionSearchToken {
    counter: Arc<AtomicU64>,
    generation: u64,
}

impl SessionSearchToken {
    fn is_current(&self) -> bool {
        self.counter.load(Ordering::Acquire) == self.generation
    }
}

fn begin_session_search(owner: &str) -> Option<SessionSearchToken> {
    if owner.is_empty() {
        return None;
    }
    let mut tokens = SESSION_SEARCH_TOKENS.lock().ok()?;
    let tokens = tokens.get_or_insert_with(HashMap::new);
    let counter = tokens
        .entry(owner.to_string())
        .or_insert_with(|| Arc::new(AtomicU64::new(0)))
        .clone();
    let generation = counter.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    Some(SessionSearchToken {
        counter,
        generation,
    })
}

fn cancel_owned_session_search(owner: &str) {
    if owner.is_empty() {
        return;
    }
    if let Ok(mut tokens) = SESSION_SEARCH_TOKENS.lock() {
        if let Some(counter) = tokens.as_mut().and_then(|tokens| tokens.remove(owner)) {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }
}

fn session_search_is_current(token: Option<&SessionSearchToken>) -> bool {
    token.is_none_or(SessionSearchToken::is_current)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchOptions {
    pub query: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub search_owner: String,
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
    let token = begin_session_search(&options.search_owner);
    let (candidates, truncated) = {
        let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
        search_session_candidates(&conn, &options, token.as_ref()).map_err(|e| e.to_string())?
    };
    if !session_search_is_current(token.as_ref()) {
        return Ok(SessionSearchResult {
            hits: Vec::new(),
            truncated: false,
        });
    }
    Ok(search_session_candidates_hits(
        candidates, &options, truncated,
    ))
}

#[tauri::command(async)]
pub fn cancel_session_search(search_owner: String) {
    cancel_owned_session_search(&search_owner);
}

#[tauri::command(async)]
pub fn session_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    session_id: String,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    delete_session(&conn, &session_id).map_err(|e| e.to_string())?;
    drop(conn);
    let _ = app.emit(crate::reminders::CHANGED, ());
    Ok(())
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

#[tauri::command(async)]
pub fn session_set_linked_work_item(
    store: State<'_, SessionStore>,
    session_id: String,
    linked_work_item: Option<Value>,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let conn = store.conn.lock().map_err(|_| "Session store is locked")?;
    set_linked_work_item(&conn, &session_id, linked_work_item.as_ref()).map_err(|e| e.to_string())
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
        ("linked_work_item_json", "TEXT"),
        ("provider_account_id", "TEXT"),
        ("worktree_removed", "INTEGER NOT NULL DEFAULT 0"),
        ("is_draft", "INTEGER NOT NULL DEFAULT 0"),
        ("automation_id", "TEXT"),
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
    if current < 12 {
        // The sidebar renders this metadata for every row, so keep it in the
        // same covering index as the rest of the session-card projection.
        ensure_column(conn, "linked_work_item_json", "TEXT")?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_cover_idx;
             CREATE INDEX IF NOT EXISTS sessions_cwd_cover_idx
               ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                            model, runtime_mode, title, provider_session_id,
                            created_at, branch, archived, pinned,
                            linked_work_item_json);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (12, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 13 {
        crate::notes::ensure_notes_table(conn)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (13, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 14 {
        ensure_session_column(conn, "provider_account_id", "TEXT")?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (14, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 15 {
        ensure_session_column(conn, "worktree_removed", "INTEGER NOT NULL DEFAULT 0")?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (15, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 16 {
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_cover_idx;
             CREATE INDEX sessions_cwd_cover_idx
               ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                            model, runtime_mode, title, provider_session_id,
                            created_at, branch, archived, pinned,
                            linked_work_item_json, worktree_cwd, worktree_removed);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (16, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 17 {
        // Draft is rendered on every session card. Materialize it alongside
        // `has_user_message` so listing never has to read transcript blobs.
        ensure_session_column(conn, "is_draft", "INTEGER NOT NULL DEFAULT 0")?;
        conn.execute(
            "UPDATE sessions SET is_draft =
               CASE WHEN blocks_json LIKE '%\"role\":\"user\"%'
                          AND blocks_json LIKE '%\"draft\":true%'
                    THEN 1 ELSE 0 END",
            [],
        )?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_cover_idx;
             CREATE INDEX sessions_cwd_cover_idx
               ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                            model, runtime_mode, title, provider_session_id,
                            created_at, branch, archived, pinned,
                            linked_work_item_json, worktree_cwd, worktree_removed,
                            is_draft);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (17, ?1)",
            params![now_millis()],
        )?;
    }
    if current < 18 {
        // Sidebar cards show whether a session came from an automation.
        ensure_session_column(conn, "automation_id", "TEXT")?;
        conn.execute_batch(
            "DROP INDEX IF EXISTS sessions_cwd_cover_idx;
             CREATE INDEX sessions_cwd_cover_idx
               ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                            model, runtime_mode, title, provider_session_id,
                            created_at, branch, archived, pinned,
                            linked_work_item_json, worktree_cwd, worktree_removed,
                            is_draft, automation_id);",
        )?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (18, ?1)",
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
         );
         CREATE TABLE IF NOT EXISTS worktree_removals (
           path TEXT PRIMARY KEY,
           sessions_json TEXT NOT NULL
         );",
    )?;
    // Compatibility only: earlier Inbox Ask builds saved temporary chats here.
    // Keep those records off normal surfaces without deleting their transcripts.
    ensure_session_column(conn, "inbox_ask", "TEXT")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS sessions_legacy_inbox
         ON sessions (id) WHERE inbox_ask IS NOT NULL;",
    )?;
    crate::notes::ensure_notes_table(conn)?;
    crate::reminders::ensure_table(conn)?;
    crate::automations::ensure_tables(conn)?;
    ensure_orchestration_history(conn)?;
    Ok(())
}

fn ensure_orchestration_history(conn: &Connection) -> rusqlite::Result<()> {
    let indexed: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'orchestration_sidebar')",
        [],
        |row| row.get(0),
    )?;
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS orchestration_runs (lead_id TEXT PRIMARY KEY, state TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS orchestration_sidebar (lead_id TEXT PRIMARY KEY, summary TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS orchestration_workers (session_id TEXT PRIMARY KEY, lead_id TEXT NOT NULL);",
    )?;
    if !indexed {
        // One-time compatibility pass for the preview that listed workers as
        // separate chats. Normal sidebar reads never scan transcripts/run blobs.
        let runs = tx
            .prepare("SELECT lead_id, state FROM orchestration_runs")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (lead, state) in runs {
            if let Ok(run) = serde_json::from_str::<Value>(&state) {
                index_orchestration(&tx, &lead, &run)?;
            }
        }
        let workers = tx.prepare("SELECT id, blocks_json FROM sessions WHERE blocks_json LIKE '%orchestrationLeadId%'")?
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, blocks) in workers {
            if let Ok(blocks) = serde_json::from_str::<Value>(&blocks) {
                remember_worker_from_blocks(&tx, &id, &blocks)?;
            }
        }
    }
    tx.commit()
}

fn remember_worker(conn: &Connection, id: &str, lead: &str) -> rusqlite::Result<()> {
    if id != lead && validate_id(id, "Worker").is_ok() && validate_id(lead, "Lead").is_ok() {
        // Keep earlier workers indexed when a lead starts a subsequent run.
        conn.execute(
            "INSERT OR IGNORE INTO orchestration_workers(session_id, lead_id) VALUES (?1, ?2)",
            params![id, lead],
        )?;
    }
    Ok(())
}

fn remember_worker_from_blocks(
    conn: &Connection,
    id: &str,
    blocks: &Value,
) -> rusqlite::Result<()> {
    if let Some(blocks) = blocks.as_array() {
        for block in blocks {
            if block["role"] == "user" {
                if let Some(lead) = block["orchestrationLeadId"].as_str() {
                    remember_worker(conn, id, lead)?;
                    break;
                }
            }
        }
    }
    Ok(())
}

fn index_orchestration(conn: &Connection, lead: &str, run: &Value) -> rusqlite::Result<()> {
    let Some(tasks) = run["tasks"].as_array() else {
        return Ok(());
    };
    let mut summaries = Vec::new();
    for task in tasks {
        let Some(id) = task["sessionId"].as_str() else {
            continue;
        };
        remember_worker(conn, id, lead)?;
        summaries.push(serde_json::json!({
            "sessionId": id, "title": task["title"], "harness": task["harness"],
            "model": task["model"], "status": task["status"],
        }));
    }
    let summary = serde_json::json!({ "status": run["status"], "tasks": summaries });
    conn.execute("INSERT INTO orchestration_sidebar(lead_id, summary) VALUES (?1, ?2) ON CONFLICT(lead_id) DO UPDATE SET summary = excluded.summary", params![lead, summary.to_string()])?;
    Ok(())
}

pub(crate) fn save_orchestration(
    conn: &Connection,
    lead: &str,
    run: &Value,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("INSERT INTO orchestration_runs(lead_id, state) VALUES (?1, ?2) ON CONFLICT(lead_id) DO UPDATE SET state = excluded.state", params![lead, run.to_string()])?;
    index_orchestration(&tx, lead, run)?;
    tx.commit()
}

fn worker_parent(conn: &Connection, id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT lead_id FROM orchestration_workers WHERE session_id = ?1",
        [id],
        |row| row.get(0),
    )
    .optional()
}

fn orchestration_summary(conn: &Connection, id: &str) -> rusqlite::Result<Option<Value>> {
    Ok(optional_json(
        conn.query_row(
            "SELECT summary FROM orchestration_sidebar WHERE lead_id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?,
    ))
}

fn upsert_session(conn: &Connection, session: &SessionUpsert) -> rusqlite::Result<SessionSummary> {
    let now = now_millis();
    let model_settings = serde_json::to_string(&session.model_settings)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let blocks_json = serde_json::to_string(&session.blocks)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let linked_work_item_json = session
        .linked_work_item
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let automation_id = session
        .automation_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let provider_session_id = session
        .provider_session_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let provider_account_id = session
        .provider_account_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let git = crate::fs::git_info_for(&crate::fs::expand_home(
        session
            .worktree_cwd
            .as_deref()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or(&session.cwd),
    ));
    let branch = if session.worktree_removed {
        None
    } else {
        git.branch
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                session
                    .branch
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
    };
    let worktree_cwd = session
        .worktree_cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let has_user_message = has_user_block(&session.blocks);
    let is_draft = has_draft_block(&session.blocks);

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
           context_used, context_window, worktree_cwd, has_user_message,
           linked_work_item_json, provider_account_id, worktree_removed, is_draft,
           automation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
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
           has_user_message = excluded.has_user_message,
           linked_work_item_json = excluded.linked_work_item_json,
           provider_account_id = excluded.provider_account_id,
           worktree_removed = excluded.worktree_removed,
           is_draft = excluded.is_draft,
           automation_id = excluded.automation_id",
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
            linked_work_item_json,
            provider_account_id,
            i64::from(session.worktree_removed),
            i64::from(is_draft),
            automation_id,
        ],
    )?;

    remember_worker_from_blocks(conn, &session.id, &session.blocks)?;
    Ok(SessionSummary {
        id: session.id.clone(),
        orchestration_lead_id: worker_parent(conn, &session.id)?,
        orchestration: orchestration_summary(conn, &session.id)?,
        cwd: session.cwd.clone(),
        harness: session.harness.clone(),
        model: session.model.clone(),
        runtime_mode: session.runtime_mode.clone(),
        title: session.title.clone(),
        provider_session_id: provider_session_id.map(str::to_owned),
        branch: branch.map(str::to_owned),
        worktree_cwd: worktree_cwd.map(str::to_owned),
        worktree_removed: session.worktree_removed,
        repo: git.repo,
        additions: 0,
        deletions: 0,
        created_at,
        updated_at,
        archived,
        pinned,
        draft: is_draft,
        linked_work_item: session.linked_work_item.clone(),
        automation_id: automation_id.map(str::to_owned),
    })
}

type SearchCandidate = (String, String, String, String, i64, String);

struct SearchProgressGuard<'a>(&'a Connection);

impl Drop for SearchProgressGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.progress_handler(1, None::<fn() -> bool>);
    }
}

#[cfg(test)]
fn search_sessions(
    conn: &Connection,
    options: &SessionSearchOptions,
) -> rusqlite::Result<SessionSearchResult> {
    let token = begin_session_search(&options.search_owner);
    let (candidates, truncated) = search_session_candidates(conn, options, token.as_ref())?;
    Ok(search_session_candidates_hits(
        candidates, options, truncated,
    ))
}

fn search_session_candidates(
    conn: &Connection,
    options: &SessionSearchOptions,
    token: Option<&SessionSearchToken>,
) -> rusqlite::Result<(Vec<SearchCandidate>, bool)> {
    let query = options.query.trim();
    if query.is_empty() {
        return Ok((Vec::new(), false));
    }
    let pattern = like_pattern(query);
    let cwd = options
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut sql = String::from(
        "SELECT id, cwd, harness, title, updated_at, archived, blocks_json
         FROM sessions
         WHERE inbox_ask IS NULL AND has_user_message = 1
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

    let progress_token = token.cloned();
    conn.progress_handler(
        SEARCH_PROGRESS_INTERVAL as i32,
        Some(move || !session_search_is_current(progress_token.as_ref())),
    )?;
    let _progress_guard = SearchProgressGuard(conn);
    let mut candidates = Vec::new();
    let mut buffered_bytes = 0usize;
    let mut cancelled = false;
    let mut truncated = false;
    for row in rows {
        if !session_search_is_current(token) {
            cancelled = true;
            break;
        }
        match row {
            Ok(candidate) => {
                if candidates.len() >= MAX_SEARCH_SCAN {
                    truncated = true;
                    break;
                }
                let size = candidate.5.len();
                if buffered_bytes + size > MAX_SEARCH_BUFFER_BYTES {
                    truncated = true;
                    continue;
                }
                buffered_bytes += size;
                candidates.push(candidate);
            }
            Err(_) if !session_search_is_current(token) => {
                cancelled = true;
                break;
            }
            Err(error) => return Err(error),
        }
    }
    if cancelled {
        return Ok((Vec::new(), false));
    }
    Ok((candidates, truncated))
}

fn search_session_candidates_hits(
    candidates: Vec<SearchCandidate>,
    options: &SessionSearchOptions,
    mut truncated: bool,
) -> SessionSearchResult {
    let needle = options.query.trim().to_lowercase();
    if needle.is_empty() {
        return SessionSearchResult {
            hits: Vec::new(),
            truncated: false,
        };
    }

    let mut conversations = Vec::new();
    let mut messages = Vec::new();
    for (id, cwd, harness, title, updated_at, blocks_raw) in candidates {
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
    SessionSearchResult { hits, truncated }
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
                created_at, updated_at, branch, archived, pinned,
                linked_work_item_json,
                (SELECT summary FROM orchestration_sidebar WHERE lead_id = sessions.id), worktree_cwd,
                worktree_removed, is_draft, automation_id
         FROM sessions
         WHERE cwd = ?1
           AND has_user_message = 1
           AND id NOT IN (SELECT id FROM sessions WHERE inbox_ask IS NOT NULL)
           AND id NOT IN (SELECT session_id FROM orchestration_workers)
         ORDER BY updated_at DESC, id ASC",
    )?;
    let rows = statement.query_map(params![cwd], |row| {
        let stored_branch: Option<String> = row.get(9)?;
        let archived: i64 = row.get(10)?;
        let pinned: i64 = row.get(11)?;
        let linked_work_item = optional_json(row.get(12)?);
        Ok(SessionSummary {
            id: row.get(0)?,
            orchestration_lead_id: None,
            orchestration: optional_json(row.get(13)?),
            cwd: row.get(1)?,
            harness: row.get(2)?,
            model: row.get(3)?,
            runtime_mode: row.get(4)?,
            title: row.get(5)?,
            provider_session_id: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            branch: if row.get::<_, i64>(15)? != 0 {
                None
            } else {
                nonempty(stored_branch).or_else(|| git.branch.clone())
            },
            worktree_cwd: row.get(14)?,
            worktree_removed: row.get::<_, i64>(15)? != 0,
            repo: git.repo.clone(),
            additions: 0,
            deletions: 0,
            archived: archived != 0,
            pinned: pinned != 0,
            draft: row.get::<_, i64>(16)? != 0,
            linked_work_item,
            automation_id: nonempty(row.get(17)?),
        })
    })?;
    rows.collect()
}

fn list_linked(conn: &Connection) -> rusqlite::Result<Vec<SessionSummary>> {
    let mut statement = conn.prepare(
        "SELECT id, cwd, harness, model, runtime_mode, title, provider_session_id,
                created_at, updated_at, branch, archived, pinned,
                linked_work_item_json,
                (SELECT summary FROM orchestration_sidebar WHERE lead_id = sessions.id), worktree_cwd,
                worktree_removed, is_draft, automation_id
         FROM sessions
         WHERE has_user_message = 1
           AND linked_work_item_json IS NOT NULL
           AND id NOT IN (SELECT id FROM sessions WHERE inbox_ask IS NOT NULL)
           AND id NOT IN (SELECT session_id FROM orchestration_workers)
         ORDER BY updated_at DESC, id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        let archived: i64 = row.get(10)?;
        let pinned: i64 = row.get(11)?;
        Ok(SessionSummary {
            id: row.get(0)?,
            orchestration_lead_id: None,
            orchestration: optional_json(row.get(13)?),
            cwd: row.get(1)?,
            harness: row.get(2)?,
            model: row.get(3)?,
            runtime_mode: row.get(4)?,
            title: row.get(5)?,
            provider_session_id: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            branch: nonempty(row.get(9)?),
            worktree_cwd: row.get(14)?,
            worktree_removed: row.get::<_, i64>(15)? != 0,
            repo: None,
            additions: 0,
            deletions: 0,
            archived: archived != 0,
            pinned: pinned != 0,
            draft: row.get::<_, i64>(16)? != 0,
            linked_work_item: optional_json(row.get(12)?),
            automation_id: nonempty(row.get(17)?),
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

fn has_draft_block(blocks: &Value) -> bool {
    blocks.as_array().is_some_and(|blocks| {
        blocks.iter().any(|block| {
            block.get("role").and_then(Value::as_str) == Some("user")
                && block.get("draft").and_then(Value::as_bool) == Some(true)
        })
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

fn optional_json(raw: Option<String>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
}

fn delete_session(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let parent = worker_parent(&tx, session_id)?;
    // Ownership is also carried in transcripts for older clients. Release
    // that metadata along with the index so reopening a worker stays detached.
    let workers = tx.prepare("SELECT id, blocks_json FROM sessions WHERE id IN (SELECT session_id FROM orchestration_workers WHERE lead_id = ?1)")?
        .query_map([session_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, raw) in workers {
        let mut blocks: Value = serde_json::from_str(&raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        if let Some(blocks) = blocks.as_array_mut() {
            for block in blocks {
                if block["orchestrationLeadId"] == session_id {
                    if let Some(block) = block.as_object_mut() {
                        block.remove("orchestrationLeadId");
                    }
                }
            }
        }
        tx.execute(
            "UPDATE sessions SET blocks_json = ?1 WHERE id = ?2",
            params![blocks.to_string(), id],
        )?;
    }
    tx.execute(
        "DELETE FROM orchestration_runs WHERE lead_id = ?1",
        [session_id],
    )?;
    tx.execute(
        "DELETE FROM orchestration_sidebar WHERE lead_id = ?1",
        [session_id],
    )?;
    if let Some(parent) = parent.filter(|id| id != session_id) {
        let raw: Option<String> = tx
            .query_row(
                "SELECT state FROM orchestration_runs WHERE lead_id = ?1",
                [&parent],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            let mut run: Value = serde_json::from_str(&raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let mut removed = false;
            if let Some(tasks) = run["tasks"].as_array_mut() {
                let ids: Vec<String> = tasks
                    .iter()
                    .filter(|task| task["sessionId"] == session_id)
                    .filter_map(|task| task["id"].as_str().map(str::to_string))
                    .collect();
                let before = tasks.len();
                tasks.retain(|task| task["sessionId"] != session_id);
                removed = before != tasks.len();
                if removed {
                    for task in tasks {
                        if let Some(deps) = task["dependsOn"].as_array_mut() {
                            deps.retain(|dep| !ids.iter().any(|id| dep == id));
                        }
                        // Removing a prerequisite must not release queued work.
                        // The app stops the run before deleting any current member.
                        if matches!(
                            task["status"].as_str(),
                            Some("queued" | "running" | "cancelling")
                        ) {
                            task["status"] = json!("cancelled");
                            task["accepted"] = json!(false);
                            task["delivered"] = json!(true);
                        }
                    }
                }
            }
            if removed {
                if matches!(run["status"].as_str(), Some("active" | "paused")) {
                    run["status"] = json!("stopped");
                    run["error"] =
                        json!("A worker conversation was deleted. Start a new run to continue.");
                }
                run["requests"] = json!({});
                tx.execute(
                    "UPDATE orchestration_runs SET state = ?1 WHERE lead_id = ?2",
                    params![run.to_string(), parent],
                )?;
                index_orchestration(&tx, &parent, &run)?;
            }
        }
        // Also handle summaries left by an older client without a run record.
        if let Some(mut summary) = orchestration_summary(&tx, &parent)? {
            if let Some(tasks) = summary["tasks"].as_array_mut() {
                tasks.retain(|task| task["sessionId"] != session_id);
            }
            tx.execute(
                "UPDATE orchestration_sidebar SET summary = ?1 WHERE lead_id = ?2",
                params![summary.to_string(), parent],
            )?;
        }
    }
    tx.execute(
        "DELETE FROM orchestration_workers WHERE session_id = ?1 OR lead_id = ?1",
        [session_id],
    )?;
    tx.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
    tx.commit()
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

fn set_linked_work_item(
    conn: &Connection,
    session_id: &str,
    linked_work_item: Option<&Value>,
) -> rusqlite::Result<()> {
    let linked_work_item_json = linked_work_item
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.execute(
        "UPDATE sessions SET linked_work_item_json = ?1 WHERE id = ?2",
        params![linked_work_item_json, session_id],
    )?;
    Ok(())
}

fn get_session(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<SessionRecord>> {
    conn.query_row(
        "SELECT id, cwd, harness, model, model_settings, runtime_mode, title,
                provider_session_id, blocks_json, created_at, updated_at,
                context_used, context_window, branch, worktree_cwd,
                linked_work_item_json, provider_account_id, worktree_removed,
                automation_id
         FROM sessions
         WHERE id = ?1 AND inbox_ask IS NULL",
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
                orchestration_lead_id: worker_parent(conn, session_id)?,
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
                worktree_removed: row.get::<_, i64>(17)? != 0,
                linked_work_item: optional_json(row.get(15)?),
                provider_account_id: row.get(16)?,
                automation_id: nonempty(row.get(18)?),
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
            provider_account_id: None,
            blocks: json!([{ "id": "b1", "role": "user", "text": "hello" }]),
            context_used: None,
            context_window: None,
            branch: None,
            worktree_cwd: None,
            worktree_removed: false,
            linked_work_item: None,
            automation_id: None,
        }
    }

    #[test]
    fn legacy_inbox_chats_are_not_normal_sessions() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        upsert_session(&conn, &sample("project", "/tmp/project", "Login")).unwrap();
        upsert_session(&conn, &sample("ask", "/tmp/project", "Login")).unwrap();
        conn.execute("UPDATE sessions SET inbox_ask = '{}' WHERE id = 'ask'", [])
            .unwrap();
        migrate(&conn).unwrap();
        let rows = list_by_project(&conn, "/tmp/project").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "project");
        assert!(get_session(&conn, "ask").unwrap().is_none());
        let search = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "Login".into(),
                cwd: None,
                include_archived: true,
                search_owner: String::new(),
            },
        )
        .unwrap();
        assert!(search.hits.iter().all(|hit| hit.session_id == "project"));
        // Hiding the old implementation's records does not delete their data.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn orchestration_history_groups_workers_and_keeps_their_transcripts() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        for id in ["lead", "worker-a", "worker-b", "unrelated"] {
            upsert_session(&conn, &sample(id, "/tmp/a", id)).unwrap();
        }
        let run = json!({"status": "active", "tasks": [
            {"sessionId": "worker-a", "title": "UI", "harness": "codex", "model": "one", "status": "running", "prompt": "private instructions", "result": "large result"},
            {"sessionId": "worker-b", "title": "Tests", "harness": "claude", "model": "two", "status": "queued"}
        ]});
        save_orchestration(&conn, "lead", &run).unwrap();
        // Reopening/migration must not hydrate, pause or otherwise mutate runs.
        migrate(&conn).unwrap();
        let rows = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(rows.len(), 2);
        let lead = rows.iter().find(|row| row.id == "lead").unwrap();
        let summary = lead.orchestration.as_ref().unwrap();
        assert_eq!(summary["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(summary["status"], "active");
        assert!(summary["tasks"][0].get("prompt").is_none());
        assert!(summary["tasks"][0].get("result").is_none());
        let worker = get_session(&conn, "worker-a").unwrap().unwrap();
        assert_eq!(worker.orchestration_lead_id.as_deref(), Some("lead"));
        assert!(has_user_block(&worker.blocks));
        // A later run replaces the card's agents without resurfacing old chats.
        save_orchestration(&conn, "lead", &json!({"status": "active", "tasks": []})).unwrap();
        assert_eq!(list_by_project(&conn, "/tmp/a").unwrap().len(), 2);
    }

    #[test]
    fn worker_upserts_carry_ownership_before_the_run_is_saved() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut worker = sample("worker", "/tmp/a", "Worker");
        worker.blocks =
            json!([{"id": "u", "role": "user", "text": "Task", "orchestrationLeadId": "lead"}]);
        worker.linked_work_item = Some(json!({"kind": "pr", "number": 1}));
        let summary = upsert_session(&conn, &worker).unwrap();
        assert_eq!(summary.orchestration_lead_id.as_deref(), Some("lead"));
        assert!(list_by_project(&conn, "/tmp/a").unwrap().is_empty());
        assert!(list_linked(&conn).unwrap().is_empty());
        // An older renderer's next write cannot accidentally detach a worker.
        worker.blocks = json!([{"id": "u", "role": "user", "text": "Task"}]);
        upsert_session(&conn, &worker).unwrap();
        assert!(list_by_project(&conn, "/tmp/a").unwrap().is_empty());
    }

    #[test]
    fn migration_groups_workers_from_the_earlier_preview() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        for id in ["lead", "old-worker", "current-worker"] {
            upsert_session(&conn, &sample(id, "/tmp/a", id)).unwrap();
        }
        conn.execute(
            "UPDATE sessions SET blocks_json = ?1 WHERE id = 'old-worker'",
            [
                json!([{"role": "user", "orchestrationLeadId": "lead", "text": "Old task"}])
                    .to_string(),
            ],
        )
        .unwrap();
        conn.execute("INSERT INTO orchestration_runs(lead_id, state) VALUES ('lead', ?1)", [json!({"status": "paused", "tasks": [{"sessionId": "current-worker", "title": "Task", "harness": "codex", "model": "one", "status": "completed"}]}).to_string()]).unwrap();
        conn.execute_batch("DROP TABLE orchestration_sidebar; DROP TABLE orchestration_workers;")
            .unwrap();
        migrate(&conn).unwrap();
        let rows = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "lead");
        assert_eq!(
            rows[0].orchestration.as_ref().unwrap()["tasks"][0]["title"],
            "Task"
        );
        assert_eq!(
            worker_parent(&conn, "old-worker").unwrap().as_deref(),
            Some("lead")
        );
        assert_eq!(
            worker_parent(&conn, "current-worker").unwrap().as_deref(),
            Some("lead")
        );
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
        for upgraded in [false, true] {
            if upgraded {
                // Exercise an existing v15 database with the old projection.
                conn.execute_batch(
                    "DROP INDEX sessions_cwd_cover_idx;
                     CREATE INDEX sessions_cwd_cover_idx
                       ON sessions (cwd, has_user_message, updated_at DESC, id, harness,
                                    model, runtime_mode, title, provider_session_id,
                                    created_at, branch, archived, pinned, linked_work_item_json);
                     DELETE FROM schema_migrations WHERE version IN (16, 17, 18);",
                )
                .unwrap();
                migrate(&conn).unwrap();
            }
            let plan: String = conn
                .query_row(
                    "EXPLAIN QUERY PLAN
                 SELECT id, cwd, harness, model, runtime_mode, title, provider_session_id,
                        created_at, updated_at, branch, archived, pinned,
                        linked_work_item_json, worktree_cwd, worktree_removed, is_draft,
                        automation_id,
                        (SELECT summary FROM orchestration_sidebar WHERE lead_id = sessions.id)
                 FROM sessions
                 WHERE cwd = ?1
                   AND has_user_message = 1
                   AND id NOT IN (SELECT id FROM sessions WHERE inbox_ask IS NOT NULL)
                   AND id NOT IN (SELECT session_id FROM orchestration_workers)
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
    fn draft_marker_round_trips_through_session_summaries() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "Draft");
        session.blocks = json!([
            { "id": "b1", "role": "user", "text": "start" },
            { "id": "b2", "role": "assistant", "text": "done" },
            { "id": "b3", "role": "user", "text": "hello", "draft": true }
        ]);

        let saved = upsert_session(&conn, &session).unwrap();
        assert!(saved.draft);
        assert!(list_by_project(&conn, "/tmp/a").unwrap()[0].draft);

        session.blocks = json!([
            { "id": "b1", "role": "user", "text": "start" },
            { "id": "b2", "role": "assistant", "text": "done" },
            { "id": "b3", "role": "user", "text": "hello" }
        ]);
        let sent = upsert_session(&conn, &session).unwrap();
        assert!(!sent.draft);
        assert!(!list_by_project(&conn, "/tmp/a").unwrap()[0].draft);
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
    fn linked_work_item_round_trips() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut row = sample("s1", "/tmp/a", "Fix PR");
        row.linked_work_item = Some(json!({
            "kind": "pr",
            "repo": "openai/codex",
            "number": 42,
            "url": "https://github.com/openai/codex/pull/42"
        }));

        let summary = upsert_session(&conn, &row).unwrap();
        assert_eq!(summary.linked_work_item, row.linked_work_item);
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(listed[0].linked_work_item, row.linked_work_item);
        let stored = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(stored.linked_work_item, row.linked_work_item);
    }

    #[test]
    fn linked_work_item_can_be_set_and_removed_without_rewriting_the_session() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "Fix PR")).unwrap();
        let linked = json!({
            "kind": "pr",
            "repo": "openai/codex",
            "number": 42,
            "url": "https://github.com/openai/codex/pull/42"
        });

        set_linked_work_item(&conn, "s1", Some(&linked)).unwrap();
        assert_eq!(
            get_session(&conn, "s1").unwrap().unwrap().linked_work_item,
            Some(linked)
        );

        set_linked_work_item(&conn, "s1", None).unwrap();
        assert_eq!(
            get_session(&conn, "s1").unwrap().unwrap().linked_work_item,
            None
        );
    }

    #[test]
    fn automation_id_round_trips() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut row = sample("s1", "/tmp/a", "Nightly review");
        row.automation_id = Some("automation-id".into());

        let summary = upsert_session(&conn, &row).unwrap();
        assert_eq!(summary.automation_id.as_deref(), Some("automation-id"));
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(listed[0].automation_id.as_deref(), Some("automation-id"));
        let stored = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(stored.automation_id.as_deref(), Some("automation-id"));
    }

    #[test]
    fn list_linked_finds_threads_across_projects() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let linked = json!({
            "kind": "pr",
            "repo": "openai/codex",
            "number": 42,
            "url": "https://github.com/openai/codex/pull/42"
        });
        let mut first = sample("s1", "/tmp/a", "First");
        first.linked_work_item = Some(linked.clone());
        let mut second = sample("s2", "/tmp/b", "Second");
        second.linked_work_item = Some(linked);
        upsert_session(&conn, &first).unwrap();
        upsert_session(&conn, &second).unwrap();
        upsert_session(&conn, &sample("s3", "/tmp/a", "Unlinked")).unwrap();

        let rows = list_linked(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.id == "s1"));
        assert!(rows.iter().any(|row| row.id == "s2"));
        assert!(rows.iter().all(|row| row.linked_work_item.is_some()));
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
    fn deleting_a_lead_releases_current_and_earlier_workers() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        for id in ["lead", "earlier", "current", "other-lead", "other-worker"] {
            let mut session = sample(id, "/tmp/a", id);
            session.linked_work_item = Some(json!({"kind": "pr", "number": 1}));
            if ["earlier", "current"].contains(&id) {
                session.blocks = json!([{"id":"u","role":"user","text":"Keep this transcript","orchestrationLeadId":"lead"}]);
            }
            upsert_session(&conn, &session).unwrap();
        }
        save_orchestration(
            &conn,
            "lead",
            &json!({"status":"stopped","tasks":[{"id":"task","sessionId":"current"}]}),
        )
        .unwrap();
        let other =
            json!({"status":"active","tasks":[{"id":"other-task","sessionId":"other-worker"}]});
        save_orchestration(&conn, "other-lead", &other).unwrap();
        delete_session(&conn, "lead").unwrap();

        for id in ["earlier", "current"] {
            let worker = get_session(&conn, id).unwrap().unwrap();
            assert!(worker.orchestration_lead_id.is_none());
            assert!(worker.blocks[0].get("orchestrationLeadId").is_none());
            assert_eq!(worker.blocks[0]["text"], "Keep this transcript");
            assert!(list_by_project(&conn, "/tmp/a")
                .unwrap()
                .iter()
                .any(|row| row.id == id));
            assert!(list_linked(&conn).unwrap().iter().any(|row| row.id == id));
        }
        assert!(orchestration_summary(&conn, "lead").unwrap().is_none());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orchestration_runs WHERE lead_id = 'lead'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            worker_parent(&conn, "other-worker").unwrap().as_deref(),
            Some("other-lead")
        );
        let raw: String = conn
            .query_row(
                "SELECT state FROM orchestration_runs WHERE lead_id = 'other-lead'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), other);
        migrate(&conn).unwrap();
        assert!(worker_parent(&conn, "current").unwrap().is_none());
    }

    #[test]
    fn deleting_a_worker_prunes_parent_state_and_does_not_release_dependencies() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        for id in ["lead", "worker", "dependent", "earlier"] {
            upsert_session(&conn, &sample(id, "/tmp/a", id)).unwrap();
        }
        remember_worker(&conn, "earlier", "lead").unwrap();
        save_orchestration(
            &conn,
            "lead",
            &json!({"status":"active", "requests":{"old":{"result":"worker"}}, "tasks":[
                {"id":"task", "sessionId":"worker", "status":"running", "dependsOn":[]},
                {"id":"next", "sessionId":"dependent", "status":"queued", "dependsOn":["task"]}
            ]}),
        )
        .unwrap();
        delete_session(&conn, "worker").unwrap();
        assert!(get_session(&conn, "worker").unwrap().is_none());
        assert!(worker_parent(&conn, "worker").unwrap().is_none());
        assert_eq!(
            worker_parent(&conn, "earlier").unwrap().as_deref(),
            Some("lead")
        );
        let raw: String = conn
            .query_row(
                "SELECT state FROM orchestration_runs WHERE lead_id = 'lead'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let run: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(run["status"], "stopped");
        assert_eq!(run["requests"], json!({}));
        assert_eq!(run["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(run["tasks"][0]["sessionId"], "dependent");
        assert_eq!(run["tasks"][0]["status"], "cancelled");
        assert_eq!(run["tasks"][0]["dependsOn"], json!([]));
        let summary = orchestration_summary(&conn, "lead").unwrap().unwrap();
        assert_eq!(summary["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(summary["tasks"][0]["sessionId"], "dependent");
        assert_eq!(summary["tasks"][0]["status"], "cancelled");
    }

    #[test]
    fn failed_session_deletion_rolls_back_orchestration_cleanup() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        for id in ["lead", "worker"] {
            upsert_session(&conn, &sample(id, "/tmp/a", id)).unwrap();
        }
        save_orchestration(
            &conn,
            "lead",
            &json!({"status":"stopped", "tasks":[{"id":"task", "sessionId":"worker"}]}),
        )
        .unwrap();
        conn.execute_batch("CREATE TRIGGER reject_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        for id in ["lead", "worker"] {
            assert!(delete_session(&conn, id).is_err());
            assert!(get_session(&conn, id).unwrap().is_some());
            assert_eq!(
                worker_parent(&conn, "worker").unwrap().as_deref(),
                Some("lead")
            );
            assert_eq!(
                orchestration_summary(&conn, "lead").unwrap().unwrap()["tasks"][0]["sessionId"],
                "worker"
            );
        }
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
    fn removed_worktree_state_round_trips_and_can_be_reselected() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "First");
        session.worktree_cwd = Some("/tmp/a-worktrees/feature".into());
        session.worktree_removed = true;
        session.branch = Some("stale-branch".into());
        let summary = upsert_session(&conn, &session).unwrap();
        assert!(summary.worktree_removed);
        assert!(summary.branch.is_none());
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert!(listed[0].worktree_removed);
        assert!(listed[0].branch.is_none());
        let restored = get_session(&conn, "s1").unwrap().unwrap();
        assert!(restored.worktree_removed);
        assert_eq!(restored.blocks, session.blocks);
        session.worktree_removed = false;
        session.worktree_cwd = None;
        session.branch = Some("main".into());
        upsert_session(&conn, &session).unwrap();
        let restored = get_session(&conn, "s1").unwrap().unwrap();
        assert!(!restored.worktree_removed);
        assert_eq!(restored.branch.as_deref(), Some("main"));
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
    fn get_round_trips_blocks_provider_session_and_account() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "First");
        session.provider_account_id = Some("account-work".into());
        upsert_session(&conn, &session).unwrap();
        let record = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(record.id, "s1");
        assert_eq!(record.provider_session_id.as_deref(), Some("acp-session-1"));
        assert_eq!(record.provider_account_id.as_deref(), Some("account-work"));
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
        assert_eq!(summary.worktree_cwd.as_deref(), Some("/tmp/a-feat"));
        let record = get_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(record.branch.as_deref(), Some("feat/picker"));
        assert_eq!(record.worktree_cwd.as_deref(), Some("/tmp/a-feat"));
        let listed = list_by_project(&conn, "/tmp/a").unwrap();
        assert_eq!(listed[0].branch.as_deref(), Some("feat/picker"));
        assert_eq!(listed[0].worktree_cwd.as_deref(), Some("/tmp/a-feat"));
    }

    #[test]
    fn rebase_project_moves_saved_sessions_to_the_renamed_path() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("moved", "/tmp/old", "Moved")).unwrap();
        upsert_session(&conn, &sample("other", "/tmp/other", "Other")).unwrap();

        rebase_project(&conn, "/tmp/old", "/tmp/new").unwrap();

        assert!(list_by_project(&conn, "/tmp/old").unwrap().is_empty());
        assert_eq!(list_by_project(&conn, "/tmp/new").unwrap()[0].id, "moved");
        assert_eq!(list_by_project(&conn, "/tmp/other").unwrap()[0].id, "other");
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
    fn a_new_session_search_supersedes_the_same_owner() {
        let owner = "session-search-owner-test";
        let first = begin_session_search(owner).unwrap();
        assert!(first.is_current());
        let second = begin_session_search(owner).unwrap();
        assert!(!first.is_current());
        assert!(second.is_current());
        cancel_owned_session_search(owner);
        assert!(!second.is_current());
    }

    #[test]
    fn cancelling_one_session_search_does_not_cancel_another() {
        let first = begin_session_search("first-session-owner").unwrap();
        let second = begin_session_search("second-session-owner").unwrap();

        cancel_owned_session_search("first-session-owner");

        assert!(!first.is_current());
        assert!(second.is_current());
    }

    #[test]
    fn a_superseded_session_search_returns_no_candidates() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        upsert_session(&conn, &sample("s1", "/tmp/a", "Searchable title")).unwrap();
        let owner = "superseded-session-search-test";
        let stale = begin_session_search(owner).unwrap();
        begin_session_search(owner);
        let options = SessionSearchOptions {
            query: "Searchable".into(),
            cwd: None,
            include_archived: false,
            search_owner: owner.into(),
        };

        let (candidates, truncated) =
            search_session_candidates(&conn, &options, Some(&stale)).unwrap();

        assert!(candidates.is_empty());
        assert!(!truncated);
        // The progress handler belongs to the search, not the shared connection.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn session_search_buffers_a_bounded_number_of_transcript_bytes() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let filler = "needle ".repeat(250_000);
        for index in 0..5 {
            let mut session = sample(&format!("s{index}"), "/tmp/a", "Needle title");
            session.blocks = json!([
                { "id": format!("u{index}"), "role": "user", "text": filler }
            ]);
            upsert_session(&conn, &session).unwrap();
        }
        let (candidates, truncated) = search_session_candidates(
            &conn,
            &SessionSearchOptions {
                query: "needle".into(),
                cwd: None,
                include_archived: false,
                search_owner: String::new(),
            },
            None,
        )
        .unwrap();

        assert!(truncated);
        let buffered: usize = candidates.iter().map(|candidate| candidate.5.len()).sum();
        assert!(buffered <= MAX_SEARCH_BUFFER_BYTES);
    }

    #[test]
    fn session_search_skips_an_oversized_newest_transcript() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut small = sample("small", "/tmp/a", "Needle title");
        small.blocks = json!([{ "id": "small", "role": "user", "text": "needle" }]);
        upsert_session(&conn, &small).unwrap();
        let mut huge = sample("huge", "/tmp/a", "Needle title");
        huge.blocks = json!([{
            "id": "huge",
            "role": "user",
            "text": "needle ".repeat(1_500_000),
        }]);
        upsert_session(&conn, &huge).unwrap();
        conn.execute("UPDATE sessions SET updated_at = 1 WHERE id = 'small'", [])
            .unwrap();
        conn.execute("UPDATE sessions SET updated_at = 2 WHERE id = 'huge'", [])
            .unwrap();

        let (candidates, truncated) = search_session_candidates(
            &conn,
            &SessionSearchOptions {
                query: "needle".into(),
                cwd: None,
                include_archived: false,
                search_owner: String::new(),
            },
            None,
        )
        .unwrap();

        assert!(truncated);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, "small");
    }

    #[test]
    fn search_uses_top_level_user_blocks_for_the_materialized_filter() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        let mut session = sample("s1", "/tmp/a", "Unrelated title");
        session.blocks = json!([
            { "id": "t1", "role": "tool", "text": "wrapper", "tool": {
                "preview": { "output": "nested role: user needle" }
            } }
        ]);
        upsert_session(&conn, &session).unwrap();

        let result = search_sessions(
            &conn,
            &SessionSearchOptions {
                query: "needle".into(),
                cwd: None,
                include_archived: false,
                search_owner: String::new(),
            },
        )
        .unwrap();

        assert!(result.hits.is_empty());
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
                search_owner: String::new(),
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
                search_owner: String::new(),
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
                search_owner: String::new(),
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
                search_owner: String::new(),
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
                search_owner: String::new(),
            },
        )
        .unwrap();
        assert!(result.hits.is_empty());
        assert!(!result.truncated);
    }
}
