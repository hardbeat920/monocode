use std::borrow::Cow;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::notes::{save_content, Note, NoteUpsert};
use crate::session_store::{validate_id, SessionStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Document,
}

impl ArtifactKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub kind: ArtifactKind,
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_cwd: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactUpsert {
    pub id: String,
    pub kind: ArtifactKind,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub source_session_id: Option<String>,
    #[serde(default)]
    pub source_cwd: Option<String>,
}

impl Artifact {
    fn from_saved(note: Note, kind: ArtifactKind) -> Self {
        Self {
            id: note.id,
            kind,
            title: note.title,
            body: note.body,
            source_session_id: note.source_session_id,
            source_cwd: note.source_cwd,
            created_at: note.created_at,
            updated_at: note.updated_at,
        }
    }
}

#[tauri::command(async)]
pub fn artifacts_list(store: State<'_, SessionStore>) -> Result<Vec<Artifact>, String> {
    let conn = store.lock_conn()?;
    list_artifacts(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn artifacts_get(
    store: State<'_, SessionStore>,
    id: String,
) -> Result<Option<Artifact>, String> {
    validate_id(&id, "artifact")?;
    let conn = store.lock_conn()?;
    get_artifact(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn artifacts_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    validate_id(&id, "artifact")?;
    let conn = store.lock_conn()?;
    delete_artifact(&conn, &id).map_err(|e| e.to_string())?;
    drop(conn);
    let _ = app.emit("monocode:artifact-deleted", &id);
    Ok(())
}

fn delete_artifact(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM notes WHERE id = ?1 AND content_kind = 'artifact'",
        [id],
    )?;
    // Update paginated Mono history and older full-session transcripts in the
    // same transaction. Reply text and all unrelated metadata stay intact.
    tx.execute(
        "UPDATE mono_blocks SET block_json = json_set(block_json, '$.artifactCards',
           (SELECT json_group_array(json(card.value))
            FROM json_each(block_json, '$.artifactCards') AS card
            WHERE json_extract(card.value, '$.id') != ?1))
         WHERE instr(block_json, '\"artifactCards\"') > 0
           AND EXISTS (SELECT 1 FROM json_each(block_json, '$.artifactCards') AS card
                       WHERE json_extract(card.value, '$.id') = ?1)",
        [id],
    )?;
    tx.execute(
        "UPDATE sessions SET blocks_json = (
           SELECT json_group_array(json(CASE WHEN json_type(block.value, '$.artifactCards') = 'array'
             THEN json_set(block.value, '$.artifactCards',
               (SELECT json_group_array(json(card.value))
                FROM json_each(block.value, '$.artifactCards') AS card
                WHERE json_extract(card.value, '$.id') != ?1))
             ELSE block.value END))
           FROM json_each(blocks_json) AS block)
         WHERE instr(blocks_json, '\"artifactCards\"') > 0
           AND EXISTS (SELECT 1 FROM json_each(blocks_json) AS block,
                         json_each(block.value, '$.artifactCards') AS card
                       WHERE json_extract(card.value, '$.id') = ?1)",
        [id],
    )?;
    tx.commit()
}

/// A late save from another window must not bring deleted cards back.
pub(crate) fn retain_existing_cards<'a>(
    conn: &Connection,
    value: &'a Value,
) -> rusqlite::Result<Cow<'a, Value>> {
    let mut cleaned = Cow::Borrowed(value);
    if let Some(blocks) = value.as_array() {
        for (index, block) in blocks.iter().enumerate() {
            let next = retain_existing_cards(conn, block)?;
            if let Cow::Owned(next) = next {
                cleaned.to_mut()[index] = next;
            }
        }
    } else if let Some(cards) = value.get("artifactCards").and_then(Value::as_array) {
        let mut existing = Vec::new();
        for card in cards {
            let Some(id) = card["id"].as_str() else {
                continue;
            };
            let present: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND content_kind = 'artifact')",
                [id],
                |row| row.get(0),
            )?;
            if present {
                existing.push(card.clone());
            }
        }
        if existing.len() != cards.len() {
            cleaned.to_mut()["artifactCards"] = Value::Array(existing);
        }
    }
    Ok(cleaned)
}

#[tauri::command(async)]
pub fn artifacts_upsert(
    store: State<'_, SessionStore>,
    artifact: ArtifactUpsert,
) -> Result<Artifact, String> {
    let kind = artifact.kind;
    // Reuse the content store while keeping the public artifact contract free
    // of note-specific tags and slugs. The category cannot be reclassified.
    let note = NoteUpsert {
        id: artifact.id,
        title: artifact.title,
        body: artifact.body,
        tags: Vec::new(),
        source_session_id: artifact.source_session_id,
        source_cwd: artifact.source_cwd,
        finalize_slug: false,
    };
    save_content(store, note, "artifact", Some(kind.as_str()))
        .map(|saved| Artifact::from_saved(saved, kind))
}

fn list_artifacts(conn: &Connection) -> rusqlite::Result<Vec<Artifact>> {
    let mut stmt = conn.prepare(
        "SELECT id, artifact_kind, title, body, source_session_id, source_cwd,
                created_at, updated_at
         FROM notes WHERE content_kind = 'artifact'
         ORDER BY updated_at DESC, id ASC",
    )?;
    let rows = stmt.query_map([], read_artifact)?;
    rows.collect()
}

fn get_artifact(conn: &Connection, id: &str) -> rusqlite::Result<Option<Artifact>> {
    conn.query_row(
        "SELECT id, artifact_kind, title, body, source_session_id, source_cwd,
                created_at, updated_at
         FROM notes WHERE id = ?1 AND content_kind = 'artifact'",
        params![id],
        read_artifact,
    )
    .optional()
}

fn read_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Artifact> {
    let kind: String = row.get(1)?;
    let kind = match kind.as_str() {
        "document" => ArtifactKind::Document,
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unsupported artifact kind".into(),
            ));
        }
    };
    Ok(Artifact {
        id: row.get(0)?,
        kind,
        title: row.get(2)?,
        body: row.get(3)?,
        source_session_id: row.get(4)?,
        source_cwd: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::upsert_content;

    #[test]
    fn artifact_contract_preserves_kind_and_excludes_note_fields() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let note = NoteUpsert {
            id: "artifact-1".into(),
            title: "PR report".into(),
            body: "# Report".into(),
            tags: Vec::new(),
            source_session_id: Some("mono-1".into()),
            source_cwd: Some("/repo".into()),
            finalize_slug: false,
        };
        upsert_content(&conn, &note, "artifact", Some("document")).unwrap();
        let artifact = get_artifact(&conn, &note.id).unwrap().unwrap();
        assert_eq!(artifact.kind, ArtifactKind::Document);
        assert_eq!(artifact.body, note.body);
        assert_eq!(list_artifacts(&conn).unwrap()[0].id, note.id);
        let json = serde_json::to_value(artifact).unwrap();
        assert_eq!(json["kind"], "document");
        assert_eq!(json["sourceSessionId"], "mono-1");
        assert!(json.get("tags").is_none());
        assert!(json.get("slug").is_none());
        assert!(get_artifact(&conn, "missing").unwrap().is_none());
        assert!(serde_json::from_value::<ArtifactUpsert>(serde_json::json!({
            "id": "artifact-2", "kind": "code", "title": "Code", "body": "code"
        }))
        .is_err());
    }

    #[test]
    fn deletion_cleans_full_and_paginated_chats_and_keeps_notes_and_replies() {
        use crate::session_store::{get_session, upsert_session, SessionUpsert};
        use serde_json::json;
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let content = NoteUpsert {
            id: "remove".into(),
            title: "Report".into(),
            body: "Full report".into(),
            tags: Vec::new(),
            source_session_id: None,
            source_cwd: None,
            finalize_slug: false,
        };
        for id in ["remove", "keep"] {
            upsert_content(
                &conn,
                &NoteUpsert {
                    id: id.into(),
                    ..content.clone()
                },
                "artifact",
                Some("document"),
            )
            .unwrap();
        }
        upsert_content(
            &conn,
            &NoteUpsert {
                id: "personal".into(),
                ..content.clone()
            },
            "note",
            None,
        )
        .unwrap();
        let session: SessionUpsert = serde_json::from_value(json!({
            "id": "normal", "cwd": "/tmp", "harness": "codex", "model": "model",
            "modelSettings": {}, "runtimeMode": "default", "title": "Chat",
            "blocks": [
                { "id": "turn", "role": "user", "text": "Review", "artifactCards": [
                    { "id": "remove", "kind": "document", "title": "Report" },
                    { "id": "keep", "kind": "document", "title": "Other" }
                ] },
                { "id": "reply", "role": "assistant", "text": "I created a report." }
            ]
        }))
        .unwrap();
        upsert_session(&conn, &session).unwrap();
        let mono = SessionUpsert {
            id: "mono".into(),
            ..session.clone()
        };
        upsert_session(&conn, &mono).unwrap();
        crate::mono_transcript::migrate_transcript(&conn, "mono").unwrap();
        delete_artifact(&conn, "remove").unwrap();
        delete_artifact(&conn, "remove").unwrap();
        delete_artifact(&conn, "personal").unwrap();
        assert!(get_artifact(&conn, "remove").unwrap().is_none());
        assert!(get_artifact(&conn, "keep").unwrap().is_some());
        let kept_note: String = conn
            .query_row(
                "SELECT body FROM notes WHERE id = 'personal' AND content_kind = 'note'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept_note, content.body);
        let blocks = get_session(&conn, "normal").unwrap().unwrap().blocks;
        assert_eq!(
            blocks[0]["artifactCards"],
            json!([{ "id": "keep", "kind": "document", "title": "Other" }])
        );
        assert_eq!(blocks[1], session.blocks[1]);
        let raw: String = conn.query_row("SELECT block_json FROM mono_blocks WHERE session_id = 'mono' AND block_id = 'turn'", [], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), blocks[0]);
        // An older client save still contains both references, but the deleted
        // card is filtered before it reaches storage again.
        upsert_session(&conn, &session).unwrap();
        assert_eq!(
            get_session(&conn, "normal").unwrap().unwrap().blocks,
            blocks
        );
        let stale = retain_existing_cards(&conn, &mono.blocks).unwrap();
        assert_eq!(stale.as_ref(), &blocks);
    }
}
