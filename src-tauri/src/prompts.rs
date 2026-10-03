//! Saved prompts: reusable text (keywords, phrases, whole prompts) that any
//! composer can insert with `!`. Global, not per project. Pinned prompts lead,
//! then the most used, so the picker gets better with use.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::session_store::{now_millis, validate_id, SessionStore};

/// Every window refreshes its prompt list on this event (the floating
/// composer is a separate window).
pub(crate) const CHANGED: &str = "monocode:prompts-changed";
const TITLE_MAX: usize = 200;
const BODY_MAX: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPrompt {
    pub id: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub use_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPromptUpsert {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
}

pub(crate) fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS saved_prompts (
           id TEXT PRIMARY KEY,
           title TEXT NOT NULL,
           body TEXT NOT NULL,
           pinned INTEGER NOT NULL DEFAULT 0,
           use_count INTEGER NOT NULL DEFAULT 0,
           last_used_at INTEGER,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL
         );",
    )
}

const SELECT: &str =
    "SELECT id, title, body, pinned, use_count, last_used_at, created_at, updated_at
     FROM saved_prompts";

fn row_to_prompt(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedPrompt> {
    Ok(SavedPrompt {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        pinned: row.get::<_, i64>(3)? != 0,
        use_count: row.get(4)?,
        last_used_at: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// Pinned first, then most used, then most recently used or edited.
fn list(conn: &Connection) -> rusqlite::Result<Vec<SavedPrompt>> {
    let mut statement = conn.prepare(&format!(
        "{SELECT} ORDER BY pinned DESC, use_count DESC,
         COALESCE(last_used_at, 0) DESC, updated_at DESC, id"
    ))?;
    let rows = statement.query_map([], row_to_prompt)?;
    rows.collect()
}

fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<SavedPrompt>> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], row_to_prompt)
        .optional()
}

fn validate(prompt: &SavedPromptUpsert) -> Result<(), String> {
    validate_id(&prompt.id, "prompt")?;
    if prompt.body.trim().is_empty() {
        return Err("A prompt needs some text".into());
    }
    if prompt.body.len() > BODY_MAX {
        return Err("Prompt is too long".into());
    }
    if prompt.title.chars().count() > TITLE_MAX {
        return Err("Prompt title is too long".into());
    }
    Ok(())
}

/// Saves a prompt; usage counts survive edits.
fn upsert(
    conn: &Connection,
    prompt: &SavedPromptUpsert,
    now: i64,
) -> rusqlite::Result<SavedPrompt> {
    conn.execute(
        "INSERT INTO saved_prompts (id, title, body, pinned, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           body = excluded.body,
           pinned = excluded.pinned,
           updated_at = excluded.updated_at",
        params![
            prompt.id,
            prompt.title.trim(),
            prompt.body,
            prompt.pinned,
            now
        ],
    )?;
    get(conn, &prompt.id).map(|saved| saved.expect("prompt was just saved"))
}

fn mark_used(conn: &Connection, id: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE saved_prompts SET use_count = use_count + 1, last_used_at = ?2
         WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn prompts_list(store: State<'_, SessionStore>) -> Result<Vec<SavedPrompt>, String> {
    let conn = store.lock_conn()?;
    list(&conn).map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn prompts_upsert(
    app: AppHandle,
    store: State<'_, SessionStore>,
    prompt: SavedPromptUpsert,
) -> Result<SavedPrompt, String> {
    validate(&prompt)?;
    let conn = store.lock_conn()?;
    let saved = upsert(&conn, &prompt, now_millis()).map_err(|error| error.to_string())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(saved)
}

#[tauri::command(async)]
pub fn prompts_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    validate_id(&id, "prompt")?;
    let conn = store.lock_conn()?;
    conn.execute("DELETE FROM saved_prompts WHERE id = ?1", [&id])
        .map_err(|error| error.to_string())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

/// Counts an insert so frequently used prompts rise in the picker. Quiet: the
/// order changes on the next list, not while a picker is open.
#[tauri::command(async)]
pub fn prompts_mark_used(store: State<'_, SessionStore>, id: String) -> Result<(), String> {
    validate_id(&id, "prompt")?;
    let conn = store.lock_conn()?;
    mark_used(&conn, &id, now_millis()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(id: &str, title: &str, pinned: bool) -> SavedPromptUpsert {
        SavedPromptUpsert {
            id: id.into(),
            title: title.into(),
            body: format!("Body of {title}"),
            pinned,
        }
    }

    #[test]
    fn lists_pinned_then_most_used_and_keeps_counts_across_edits() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        upsert(&conn, &prompt("a", "Review", false), 1).unwrap();
        upsert(&conn, &prompt("b", "Explain", false), 2).unwrap();
        upsert(&conn, &prompt("c", "Pinned", true), 3).unwrap();
        mark_used(&conn, "a", 10).unwrap();
        mark_used(&conn, "a", 11).unwrap();
        mark_used(&conn, "b", 12).unwrap();
        let ids: Vec<_> = list(&conn).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["c", "a", "b"]);

        let edited = upsert(&conn, &prompt("a", "  Review PR  ", false), 20).unwrap();
        assert_eq!(edited.title, "Review PR");
        assert_eq!(edited.use_count, 2);
        assert_eq!(edited.last_used_at, Some(11));
        assert_eq!(edited.created_at, 1);
        assert_eq!(edited.updated_at, 20);
    }

    #[test]
    fn rejects_empty_or_oversized_prompts_and_bad_ids() {
        let mut empty = prompt("a", "Empty", false);
        empty.body = "   ".into();
        assert!(validate(&empty).is_err());
        let mut long = prompt("a", "Long", false);
        long.body = "x".repeat(BODY_MAX + 1);
        assert!(validate(&long).is_err());
        assert!(validate(&prompt("../x", "Bad", false)).is_err());
        assert!(validate(&prompt("ok-id", "Fine", false)).is_ok());
    }

    #[test]
    fn deleting_and_marking_unknown_prompts_is_harmless() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        mark_used(&conn, "missing", 1).unwrap();
        upsert(&conn, &prompt("a", "Keep", false), 1).unwrap();
        conn.execute("DELETE FROM saved_prompts WHERE id = 'a'", [])
            .unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }
}
