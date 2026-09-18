use rusqlite::{params, Connection, OptionalExtension};

use crate::session_store::{now_millis, validate_id, SessionStore};
use tauri::State;

/// Upper bound for a persisted draft. Composer drafts are short by nature;
/// anything beyond this is more likely an accidental paste than intent.
const DRAFT_MAX: usize = 100_000;

pub fn ensure_drafts_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS composer_drafts (
           session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
           text TEXT NOT NULL DEFAULT '',
           updated_at INTEGER NOT NULL
         );",
    )
}

fn get_draft(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT text FROM composer_drafts WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )
    .optional()
}

fn set_draft(conn: &Connection, session_id: &str, text: &str) -> rusqlite::Result<()> {
    if text.is_empty() {
        conn.execute(
            "DELETE FROM composer_drafts WHERE session_id = ?1",
            [session_id],
        )?;
        return Ok(());
    }
    let truncated: String = text.chars().take(DRAFT_MAX).collect();
    conn.execute(
        "INSERT INTO composer_drafts (session_id, text, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET text = excluded.text,
           updated_at = excluded.updated_at",
        params![session_id, truncated, now_millis()],
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn composer_draft_get(
    store: State<'_, SessionStore>,
    session_id: String,
) -> Result<Option<String>, String> {
    validate_id(&session_id, "session")?;
    let conn = store.lock_conn()?;
    get_draft(&conn, &session_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn composer_draft_set(
    store: State<'_, SessionStore>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    let conn = store.lock_conn()?;
    set_draft(&conn, &session_id, &text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        // The FK references sessions(id); create a minimal parent so the
        // draft table can be created and written against a bare connection.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE sessions (id TEXT PRIMARY KEY)", [])
            .unwrap();
        ensure_drafts_table(&conn).unwrap();
        conn
    }

    #[test]
    fn drafts_roundtrip_and_clear() {
        let conn = test_conn();
        conn.execute("INSERT INTO sessions (id) VALUES ('s-1')", [])
            .unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        assert_eq!(get_draft(&conn, "s-1").unwrap(), None);
        set_draft(&conn, "s-1", "hello").unwrap();
        assert_eq!(get_draft(&conn, "s-1").unwrap().as_deref(), Some("hello"));
        set_draft(&conn, "s-1", "").unwrap();
        assert_eq!(get_draft(&conn, "s-1").unwrap(), None);
    }

    #[test]
    fn drafts_upsert_keeps_single_row() {
        let conn = test_conn();
        conn.execute("INSERT INTO sessions (id) VALUES ('s-1')", [])
            .unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        set_draft(&conn, "s-1", "first").unwrap();
        set_draft(&conn, "s-1", "second").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM composer_drafts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(get_draft(&conn, "s-1").unwrap().as_deref(), Some("second"));
    }

    #[test]
    fn drafts_are_truncated() {
        let conn = test_conn();
        conn.execute("INSERT INTO sessions (id) VALUES ('s-1')", [])
            .unwrap();
        let big = "x".repeat(DRAFT_MAX + 10);
        set_draft(&conn, "s-1", &big).unwrap();
        let stored = get_draft(&conn, "s-1").unwrap().unwrap();
        assert_eq!(stored.chars().count(), DRAFT_MAX);
    }
}
