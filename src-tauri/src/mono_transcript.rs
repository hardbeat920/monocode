//! Mono-only transcripts. Ordinary sessions continue to use blocks_json.
//! Turn boundaries have their own index, so opening a chat never scans old blocks.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::session_store::{self, SessionRecord, SessionStore, SessionSummary, SessionUpsert};

const PAGE_TURNS: i64 = 10;

pub(crate) fn ensure_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mono_transcripts (
           session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE
         );
         CREATE TABLE IF NOT EXISTS mono_turns (
           session_id TEXT NOT NULL REFERENCES mono_transcripts(session_id) ON DELETE CASCADE,
           seq INTEGER NOT NULL,
           PRIMARY KEY (session_id, seq)
         );
         CREATE TABLE IF NOT EXISTS mono_blocks (
           session_id TEXT NOT NULL REFERENCES mono_transcripts(session_id) ON DELETE CASCADE,
           seq INTEGER NOT NULL,
           block_id TEXT NOT NULL,
           block_json TEXT NOT NULL,
           search_text TEXT NOT NULL,
           is_user INTEGER NOT NULL,
           is_draft INTEGER NOT NULL,
           PRIMARY KEY (session_id, seq),
           UNIQUE (session_id, block_id)
         );
         CREATE INDEX IF NOT EXISTS mono_blocks_user ON mono_blocks(session_id, is_user, is_draft);",
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonoPage {
    pub blocks: Vec<Value>,
    pub before: Option<i64>,
    pub has_newer: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonoTranscript {
    before: Option<i64>,
    first_block_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonoRecord {
    #[serde(flatten)]
    session: SessionRecord,
    mono_transcript: MonoTranscript,
}

fn invalid(message: &str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(message.to_string().into())
}

pub(crate) fn is_indexed(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mono_transcripts WHERE session_id = ?1)",
        [id],
        |row| row.get(0),
    )
}

pub(crate) fn matching_blocks(
    conn: &Connection,
    id: &str,
    needle: &str,
    limit: usize,
) -> rusqlite::Result<Value> {
    let mut statement = conn.prepare("SELECT block_json FROM mono_blocks WHERE session_id = ?1 AND instr(search_text, ?2) > 0 ORDER BY seq LIMIT ?3")?;
    let blocks = statement
        .query_map(params![id, needle, limit as i64], |row| {
            let raw: String = row.get(0)?;
            serde_json::from_str::<Value>(&raw).map_err(|e| invalid(&e.to_string()))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!(blocks))
}

pub(crate) fn generated_image_paths(conn: &Connection, id: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT json_extract(block_json, '$.image.path') FROM mono_blocks WHERE session_id = ?1 AND json_extract(block_json, '$.role') = 'image' AND json_type(block_json, '$.image.path') = 'text'")?;
    let paths = statement
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths)
}

/// A legacy Mono is converted once, transactionally, the first time it opens.
/// No ordinary session is visited by this migration.
pub(crate) fn migrate_transcript(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    if is_indexed(conn, id)? {
        return Ok(());
    }
    let Some(record) = session_store::get_session(conn, id)? else {
        return Ok(());
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute("INSERT INTO mono_transcripts(session_id) VALUES (?1)", [id])?;
    insert_blocks(
        &tx,
        id,
        0,
        record
            .blocks
            .as_array()
            .ok_or_else(|| invalid("Invalid transcript"))?,
    )?;
    tx.execute("UPDATE sessions SET blocks_json = '[]' WHERE id = ?1", [id])?;
    tx.commit()
}

fn insert_blocks(
    conn: &Connection,
    id: &str,
    after: i64,
    blocks: &[Value],
) -> rusqlite::Result<()> {
    let mut block_insert = conn.prepare("INSERT INTO mono_blocks(session_id, seq, block_id, block_json, search_text, is_user, is_draft) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
    let mut turn_insert =
        conn.prepare("INSERT INTO mono_turns(session_id, seq) VALUES (?1, ?2)")?;
    // A handoff occupies its own turn, including when a suffix is edited.
    let mut after_handoff = conn.query_row("SELECT json_extract(block_json, '$.role') = 'handoff' FROM mono_blocks WHERE session_id = ?1 AND seq = ?2", params![id, after], |row| row.get::<_, bool>(0)).optional()?.unwrap_or(false);
    for (index, block) in blocks.iter().enumerate() {
        let seq = after + index as i64 + 1;
        let block_id = block["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid("Block needs an id"))?;
        let role = block["role"].as_str().unwrap_or("");
        let user = role == "user";
        let internal = block["internal"].as_bool().unwrap_or(false);
        if seq == 1
            || after_handoff
            || (!internal && matches!(role, "user" | "handoff"))
            || block.get("monoHabit").is_some()
        {
            turn_insert.execute(params![id, seq])?;
        }
        after_handoff = role == "handoff";
        block_insert.execute(params![
            id,
            seq,
            block_id,
            block.to_string(),
            search_text(block),
            user,
            user && block["draft"].as_bool().unwrap_or(false)
        ])?;
    }
    Ok(())
}

fn search_text(block: &Value) -> String {
    if !matches!(
        block["role"].as_str(),
        Some("user" | "assistant" | "tool" | "tasks" | "plan" | "image")
    ) {
        return String::new();
    }
    [
        block["text"].as_str(),
        block["tool"]["title"].as_str(),
        block["image"]["name"].as_str(),
        block["image"]["alt"].as_str(),
        block["tool"]["detail"].as_str(),
        block["tool"]["preview"]["query"].as_str(),
        block["tool"]["preview"]["path"].as_str(),
        block["tool"]["preview"]["output"].as_str(),
        block["tool"]["preview"]["title"].as_str(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n")
    .to_lowercase()
}

fn page(
    conn: &Connection,
    id: &str,
    before: Option<i64>,
    around: Option<&str>,
) -> rusqlite::Result<MonoPage> {
    let end = if let Some(block) = around {
        let seq: i64 = conn.query_row(
            "SELECT seq FROM mono_blocks WHERE session_id = ?1 AND block_id = ?2",
            params![id, block],
            |row| row.get(0),
        )?;
        conn.query_row(
            "SELECT seq FROM mono_turns WHERE session_id = ?1 AND seq > ?2 ORDER BY seq LIMIT 1",
            params![id, seq],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(i64::MAX)
    } else {
        before.unwrap_or(i64::MAX)
    };
    let mut statement = conn.prepare(
        "SELECT seq FROM mono_turns WHERE session_id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3",
    )?;
    let starts = statement
        .query_map(params![id, end, PAGE_TURNS + 1], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let start = starts
        .iter()
        .take(PAGE_TURNS as usize)
        .next_back()
        .copied()
        .unwrap_or(end);
    let mut statement = conn.prepare("SELECT block_json FROM mono_blocks WHERE session_id = ?1 AND seq >= ?2 AND seq < ?3 ORDER BY seq")?;
    let blocks = statement
        .query_map(params![id, start, end], |row| {
            let raw: String = row.get(0)?;
            serde_json::from_str(&raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })?
        .collect::<rusqlite::Result<Vec<Value>>>()?;
    let has_newer = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mono_blocks WHERE session_id = ?1 AND seq >= ?2)",
        params![id, end],
        |row| row.get(0),
    )?;
    Ok(MonoPage {
        blocks,
        before: (starts.len() > PAGE_TURNS as usize).then_some(start),
        has_newer,
    })
}

#[tauri::command(async)]
pub fn mono_session_get(
    store: State<'_, SessionStore>,
    session_id: String,
) -> Result<Option<MonoRecord>, String> {
    session_store::validate_id(&session_id, "session")?;
    let conn = store.lock_conn()?;
    migrate_transcript(&conn, &session_id).map_err(|e| e.to_string())?;
    let Some(mut session) =
        session_store::get_session_metadata(&conn, &session_id).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let page = page(&conn, &session_id, None, None).map_err(|e| e.to_string())?;
    let first_block_id = page
        .blocks
        .first()
        .and_then(|block| block["id"].as_str())
        .map(str::to_string);
    session.blocks = json!(page.blocks);
    Ok(Some(MonoRecord {
        session,
        mono_transcript: MonoTranscript {
            before: page.before,
            first_block_id,
        },
    }))
}

#[tauri::command(async)]
pub fn mono_session_page(
    store: State<'_, SessionStore>,
    session_id: String,
    before: Option<i64>,
    before_block_id: Option<String>,
    around_block_id: Option<String>,
) -> Result<MonoPage, String> {
    session_store::validate_id(&session_id, "session")?;
    let conn = store.lock_conn()?;
    migrate_transcript(&conn, &session_id).map_err(|e| e.to_string())?;
    let before = if let Some(block) = before_block_id {
        Some(
            conn.query_row(
                "SELECT seq FROM mono_blocks WHERE session_id = ?1 AND block_id = ?2",
                params![session_id, block],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?,
        )
    } else {
        before
    };
    page(&conn, &session_id, before, around_block_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn mono_session_find(
    store: State<'_, SessionStore>,
    session_id: String,
    query: String,
) -> Result<Vec<String>, String> {
    session_store::validate_id(&session_id, "session")?;
    let conn = store.lock_conn()?;
    let mut statement = conn.prepare("SELECT block_id FROM mono_blocks WHERE session_id = ?1 AND instr(search_text, ?2) > 0 ORDER BY seq LIMIT 1000").map_err(|e| e.to_string())?;
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let ids = statement
        .query_map(params![session_id, needle], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

/// Replace only the changed suffix. Anchors prevent a partially loaded client
/// from ever deleting history outside its window. Missing anchors fail closed.
fn upsert_with_mode(
    conn: &Connection,
    session: &SessionUpsert,
    after: Option<&str>,
    from: Option<&str>,
    changed: bool,
    append_only: bool,
) -> rusqlite::Result<SessionSummary> {
    migrate_transcript(conn, &session.id)?;
    let tx = conn.unchecked_transaction()?;
    let mut header = session.clone();
    header.blocks = json!([]);
    let mut summary = session_store::upsert_session(&tx, &header)?;
    tx.execute(
        "INSERT OR IGNORE INTO mono_transcripts(session_id) VALUES (?1)",
        [&session.id],
    )?;
    if changed {
        let anchor = if append_only {
            tx.query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM mono_blocks WHERE session_id = ?1",
                [&session.id],
                |row| row.get::<_, i64>(0),
            )?
        } else if let Some(id) = after {
            tx.query_row(
                "SELECT seq FROM mono_blocks WHERE session_id = ?1 AND block_id = ?2",
                params![session.id, id],
                |row| row.get::<_, i64>(0),
            )?
        } else if let Some(id) = from {
            tx.query_row(
                "SELECT seq - 1 FROM mono_blocks WHERE session_id = ?1 AND block_id = ?2",
                params![session.id, id],
                |row| row.get::<_, i64>(0),
            )?
        } else {
            0
        };
        tx.execute(
            "DELETE FROM mono_turns WHERE session_id = ?1 AND seq > ?2",
            params![session.id, anchor],
        )?;
        tx.execute(
            "DELETE FROM mono_blocks WHERE session_id = ?1 AND seq > ?2",
            params![session.id, anchor],
        )?;
        insert_blocks(
            &tx,
            &session.id,
            anchor,
            session
                .blocks
                .as_array()
                .ok_or_else(|| invalid("blocks must be an array"))?,
        )?;
        tx.execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![session.id, session_store::now_millis()],
        )?;
        summary.updated_at = tx.query_row(
            "SELECT updated_at FROM sessions WHERE id = ?1",
            [&session.id],
            |row| row.get(0),
        )?;
    }
    let (has_user, draft): (bool, bool) = tx.query_row("SELECT EXISTS(SELECT 1 FROM mono_blocks WHERE session_id = ?1 AND is_user = 1), EXISTS(SELECT 1 FROM mono_blocks WHERE session_id = ?1 AND is_user = 1 AND is_draft = 1)", [&session.id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    tx.execute(
        "UPDATE sessions SET has_user_message = ?2, is_draft = ?3 WHERE id = ?1",
        params![session.id, has_user, draft],
    )?;
    summary.draft = draft;
    tx.commit()?;
    Ok(summary)
}

#[tauri::command(async)]
pub fn mono_session_upsert(
    store: State<'_, SessionStore>,
    session: SessionUpsert,
    after_block_id: Option<String>,
    from_block_id: Option<String>,
    blocks_changed: bool,
    append_only: bool,
) -> Result<SessionSummary, String> {
    session_store::validate_id(&session.id, "session")?;
    for id in [
        &session.provider_session_id,
        &session.provider_account_id,
        &session.automation_id,
    ]
    .into_iter()
    .flatten()
    {
        session_store::validate_id(id, "provider")?;
    }
    if session.cwd.trim().is_empty()
        || !session.model_settings.is_object()
        || !session.blocks.is_array()
    {
        return Err("Invalid Mono session".into());
    }
    let conn = store.lock_conn()?;
    upsert_with_mode(
        &conn,
        &session,
        after_block_id.as_deref(),
        from_block_id.as_deref(),
        blocks_changed,
        append_only,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn upsert(
        conn: &Connection,
        session: &SessionUpsert,
        after: Option<&str>,
        from: Option<&str>,
        changed: bool,
    ) -> rusqlite::Result<SessionSummary> {
        upsert_with_mode(conn, session, after, from, changed, false)
    }

    fn sample(id: &str, turns: usize) -> SessionUpsert {
        let blocks: Vec<Value> = (0..turns)
            .flat_map(|i| {
                [
                    json!({"id":format!("u{i}"), "role":"user", "text":format!("Question {i}")}),
                    json!({"id":format!("a{i}"), "role":"assistant", "text":format!("Answer {i}")}),
                ]
            })
            .collect();
        serde_json::from_value(json!({"id":id, "cwd":"/tmp", "harness":"codex", "model":"model", "modelSettings":{}, "runtimeMode":"default", "title":"Mono", "blocks":blocks})).unwrap()
    }

    #[test]
    fn pending_messages_survive_header_only_writes_and_clear_after_delivery() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let mut session = sample("queued-mono", 25);
        upsert(&conn, &session, None, None, true).unwrap();
        session.blocks = json!([]);
        session.queued_messages =
            vec![json!({"id":"pending", "text":"Follow up", "attachments":[]})];
        session.queue_status = Some("active".into());
        upsert(&conn, &session, None, None, false).unwrap();
        let stored = session_store::get_session_metadata(&conn, "queued-mono")
            .unwrap()
            .unwrap();
        assert_eq!(stored.queued_messages, session.queued_messages);
        assert_eq!(stored.queue_status.as_deref(), Some("active"));
        let history = page(&conn, "queued-mono", None, None).unwrap();
        assert_eq!(history.blocks.len(), PAGE_TURNS as usize * 2);

        session.queued_messages.clear();
        session.queue_status = None;
        upsert(&conn, &session, None, None, false).unwrap();
        let stored = session_store::get_session_metadata(&conn, "queued-mono")
            .unwrap()
            .unwrap();
        assert!(stored.queued_messages.is_empty());
        assert_eq!(
            page(&conn, "queued-mono", None, None).unwrap().blocks,
            history.blocks
        );
    }

    #[test]
    fn migrates_only_the_requested_mono_and_pages_ten_complete_turns() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let mono = sample("mono", 25);
        let normal = sample("normal", 25);
        session_store::upsert_session(&conn, &mono).unwrap();
        session_store::upsert_session(&conn, &normal).unwrap();
        migrate_transcript(&conn, "mono").unwrap();
        let latest = page(&conn, "mono", None, None).unwrap();
        assert_eq!(latest.blocks.len(), 20);
        assert_eq!(latest.blocks[0]["id"], "u15");
        let earlier = page(&conn, "mono", latest.before, None).unwrap();
        assert_eq!(earlier.blocks[0]["id"], "u5");
        assert_eq!(earlier.blocks.last().unwrap()["id"], "a14");
        let oldest = page(&conn, "mono", earlier.before, None).unwrap();
        assert_eq!(oldest.blocks.len(), 10);
        assert_eq!(oldest.before, None);
        assert!(oldest.has_newer);
        let jump = page(&conn, "mono", None, Some("a3")).unwrap();
        assert_eq!(jump.blocks.last().unwrap()["id"], "a3");
        assert_eq!(
            session_store::get_session(&conn, "normal")
                .unwrap()
                .unwrap()
                .blocks,
            normal.blocks
        );
        assert_eq!(
            session_store::get_session(&conn, "mono")
                .unwrap()
                .unwrap()
                .blocks,
            json!([])
        );
    }

    #[test]
    fn cold_page_reads_are_bounded_and_never_read_the_legacy_blob() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        upsert(&conn, &sample("mono", 10_000), None, None, true).unwrap();
        conn.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
            AuthAction::Read {
                table_name: "sessions",
                column_name: "blocks_json",
            } => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
        let steps = Arc::new(AtomicUsize::new(0));
        let count = steps.clone();
        conn.progress_handler(
            1,
            Some(move || {
                count.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )
        .unwrap();
        migrate_transcript(&conn, "mono").unwrap();
        assert!(session_store::get_session_metadata(&conn, "mono")
            .unwrap()
            .is_some());
        assert_eq!(page(&conn, "mono", None, None).unwrap().blocks.len(), 20);
        assert!(
            steps.load(Ordering::Relaxed) < 2_000,
            "Opening must not scan old messages"
        );
    }

    #[test]
    fn suffix_edits_appends_and_rewinds_preserve_unloaded_history() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let mut session = sample("mono", 25);
        upsert(&conn, &session, None, None, true).unwrap();
        session.blocks = json!([{ "id":"a24", "role":"assistant", "text":"Edited" }, {"id":"u25", "role":"user", "text":"Next"}]);
        upsert(&conn, &session, Some("u24"), None, true).unwrap();
        assert_eq!(
            page(&conn, "mono", None, Some("a0")).unwrap().blocks[1]["text"],
            "Answer 0"
        );
        assert_eq!(
            page(&conn, "mono", None, None)
                .unwrap()
                .blocks
                .last()
                .unwrap()["id"],
            "u25"
        );
        session.blocks = json!([]);
        upsert(&conn, &session, Some("a24"), None, true).unwrap();
        assert_eq!(
            page(&conn, "mono", None, None)
                .unwrap()
                .blocks
                .last()
                .unwrap()["text"],
            "Edited"
        );
        session.blocks = json!([{ "id":"u15", "role":"user", "text":"Replaced loaded tail" }]);
        upsert(&conn, &session, None, Some("u15"), true).unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT count(*) FROM mono_blocks WHERE session_id = 'mono'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 31);
        session.title = "Metadata only".into();
        session.blocks = json!([]);
        upsert(&conn, &session, None, None, false).unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM mono_blocks", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            total
        );
    }

    #[test]
    fn failed_anchors_roll_back_and_reset_cascades_all_history() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let mut session = sample("mono", 12);
        upsert(&conn, &session, None, None, true).unwrap();
        session.title = "Should roll back".into();
        assert!(upsert(&conn, &session, Some("missing"), None, true).is_err());
        assert_eq!(
            session_store::get_session_metadata(&conn, "mono")
                .unwrap()
                .unwrap()
                .title,
            "Mono"
        );
        conn.execute("DELETE FROM sessions WHERE id = 'mono'", [])
            .unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM mono_blocks", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM mono_turns", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn appending_after_deleting_a_loaded_window_preserves_the_archive() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let mut session = sample("mono", 20);
        upsert(&conn, &session, None, None, true).unwrap();
        session.blocks = json!([]);
        upsert(&conn, &session, None, Some("u10"), true).unwrap();
        session.blocks = json!([{ "id":"fresh", "role":"user", "text":"Fresh" }]);
        upsert_with_mode(&conn, &session, None, None, true, true).unwrap();
        assert_eq!(
            page(&conn, "mono", None, Some("a0")).unwrap().blocks[1]["text"],
            "Answer 0"
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM mono_blocks", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            21
        );
    }
}
