use base64::prelude::*;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::session_store::{now_millis, validate_id, SessionStore};

pub const TITLE_MAX: usize = 160;
pub const DESCRIPTION_MAX: usize = 12_000;
pub const LINKED_SESSIONS_MAX: usize = 50;
pub const MEDIA_BYTES_MAX: usize = 5 * 1024 * 1024; // 5 MiB = 5,242,880 bytes
/// Largest standard-base64 payload length that can represent MEDIA_BYTES_MAX bytes.
pub const MEDIA_BASE64_BYTES_MAX: usize = MEDIA_BYTES_MAX.div_ceil(3) * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoardStatus {
    Backlog,
    Ready,
    InProgress,
    Blocked,
    Done,
}

impl BoardStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::InProgress => "in-progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "backlog" => Ok(Self::Backlog),
            "ready" => Ok(Self::Ready),
            "in-progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "done" => Ok(Self::Done),
            other => Err(format!("Invalid board status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardPriority {
    Low,
    Medium,
    High,
}

impl BoardPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(format!("Invalid board priority: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardMediaRef {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub byte_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardMedia {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub byte_length: usize,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoardCard {
    pub id: String,
    pub project_cwd: String,
    pub title: String,
    pub description: String,
    pub status: BoardStatus,
    pub priority: BoardPriority,
    pub linked_session_ids: Vec<String>,
    pub media: Vec<BoardMediaRef>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoardCardInput {
    pub id: String,
    pub project_cwd: String,
    pub title: String,
    pub description: String,
    pub status: BoardStatus,
    pub priority: BoardPriority,
    #[serde(default)]
    pub linked_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoardCardPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<BoardStatus>,
    pub priority: Option<BoardPriority>,
}

pub fn validate_card_patch(patch: &ProjectBoardCardPatch) -> Result<(), String> {
    if let Some(title) = &patch.title {
        if title.trim().is_empty() {
            return Err("Card title cannot be empty".into());
        }
        if title.chars().count() > TITLE_MAX {
            return Err(format!(
                "Card title exceeds maximum length of {TITLE_MAX} characters"
            ));
        }
    }
    if let Some(desc) = &patch.description {
        if desc.chars().count() > DESCRIPTION_MAX {
            return Err(format!(
                "Card description exceeds maximum length of {DESCRIPTION_MAX} characters"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMediaInput {
    #[serde(default)]
    pub id: Option<String>,
    pub project_cwd: String,
    pub card_id: String,
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
}

pub fn ensure_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_board_cards (
           id TEXT NOT NULL,
           project_cwd TEXT NOT NULL,
           title TEXT NOT NULL,
           description TEXT NOT NULL,
           status TEXT NOT NULL,
           priority TEXT NOT NULL,
           linked_sessions_json TEXT NOT NULL DEFAULT '[]',
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY (project_cwd, id)
         );
         CREATE INDEX IF NOT EXISTS project_board_cards_project_status_idx
           ON project_board_cards (project_cwd, status, updated_at DESC);
         CREATE TABLE IF NOT EXISTS project_board_media (
           id TEXT NOT NULL,
           project_cwd TEXT NOT NULL,
           card_id TEXT NOT NULL,
           name TEXT NOT NULL,
           mime_type TEXT NOT NULL,
           byte_length INTEGER NOT NULL,
           data_bytes BLOB NOT NULL,
           created_at INTEGER NOT NULL,
           PRIMARY KEY (project_cwd, card_id, id),
           FOREIGN KEY (project_cwd, card_id) REFERENCES project_board_cards(project_cwd, id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS project_board_media_card_idx
           ON project_board_media (project_cwd, card_id);"
    )
}

pub fn normalize_mime_type(raw: &str) -> Result<String, String> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "image/png" => Ok("image/png".into()),
        "image/jpeg" | "image/jpg" => Ok("image/jpeg".into()),
        "image/gif" => Ok("image/gif".into()),
        "image/webp" => Ok("image/webp".into()),
        other => Err(format!(
            "Unsupported media type '{other}'. Allowed types: image/png, image/jpeg, image/gif, image/webp"
        )),
    }
}

pub fn validate_project_cwd(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("Project path cannot be empty".into());
    }
    Ok(())
}

pub fn validate_card_input(input: &ProjectBoardCardInput) -> Result<(), String> {
    validate_id(&input.id, "card")?;
    validate_project_cwd(&input.project_cwd)?;

    if input.title.trim().is_empty() {
        return Err("Card title cannot be empty".into());
    }
    if input.title.chars().count() > TITLE_MAX {
        return Err(format!(
            "Card title exceeds maximum length of {TITLE_MAX} characters"
        ));
    }
    if input.description.chars().count() > DESCRIPTION_MAX {
        return Err(format!(
            "Card description exceeds maximum length of {DESCRIPTION_MAX} characters"
        ));
    }
    if input.linked_session_ids.len() > LINKED_SESSIONS_MAX {
        return Err(format!(
            "Linked sessions count exceeds maximum of {LINKED_SESSIONS_MAX}"
        ));
    }
    for session_id in &input.linked_session_ids {
        validate_id(session_id, "session")?;
    }
    Ok(())
}

fn strip_data_uri_prefix(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(pos) = trimmed.find(";base64,") {
        &trimmed[pos + 8..]
    } else {
        trimmed
    }
}

fn media_too_large_error(byte_length: usize) -> String {
    format!(
        "Media byte length {byte_length} exceeds maximum allowed of {MEDIA_BYTES_MAX} bytes (5 MiB)"
    )
}

/// Reject by the encoded size before base64 decoding can allocate its output.
fn validate_encoded_media_size(encoded: &str) -> Result<(), String> {
    let encoded_length = encoded.len();
    if encoded_length > MEDIA_BASE64_BYTES_MAX {
        return Err(format!(
            "Media base64 length {encoded_length} exceeds maximum allowed encoded length of {MEDIA_BASE64_BYTES_MAX} bytes for a {MEDIA_BYTES_MAX}-byte (5 MiB) image"
        ));
    }

    // Invalid, non-quantized lengths are left to the strict decoder. For a
    // complete base64 quantum, padding distinguishes the exact decoded size
    // when the encoded length is at the maximum boundary.
    if encoded_length.is_multiple_of(4) {
        let trailing_padding = encoded
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'=')
            .count()
            .min(2);
        let estimated_decoded_length = (encoded_length / 4) * 3;
        let decoded_length = estimated_decoded_length.saturating_sub(trailing_padding);
        if decoded_length > MEDIA_BYTES_MAX {
            return Err(media_too_large_error(decoded_length));
        }
    }
    Ok(())
}

pub fn decode_and_validate_media(
    mime_type: &str,
    data_base64: &str,
) -> Result<(String, Vec<u8>), String> {
    let normalized_mime = normalize_mime_type(mime_type)?;
    // Strip the accepted data-URI prefix first so the cap applies to the base64
    // payload, not the prefix bytes.
    let clean_base64 = strip_data_uri_prefix(data_base64);
    if clean_base64.is_empty() {
        return Err("Media payload cannot be empty".into());
    }
    validate_encoded_media_size(clean_base64)?;

    let bytes = BASE64_STANDARD
        .decode(clean_base64.as_bytes())
        .map_err(|e| format!("Invalid base64 payload: {e}"))?;

    if bytes.is_empty() {
        return Err("Media payload cannot be empty".into());
    }
    if bytes.len() > MEDIA_BYTES_MAX {
        return Err(media_too_large_error(bytes.len()));
    }

    Ok((normalized_mime, bytes))
}

pub fn list_cards(conn: &Connection, project_cwd: &str) -> Result<Vec<ProjectBoardCard>, String> {
    validate_project_cwd(project_cwd)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_cwd, title, description, status, priority, linked_sessions_json, created_at, updated_at
             FROM project_board_cards
             WHERE project_cwd = ?1
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_cwd], |row| {
            let id: String = row.get(0)?;
            let project_cwd: String = row.get(1)?;
            let title: String = row.get(2)?;
            let description: String = row.get(3)?;
            let status_raw: String = row.get(4)?;
            let priority_raw: String = row.get(5)?;
            let linked_raw: String = row.get(6)?;
            let created_at: i64 = row.get(7)?;
            let updated_at: i64 = row.get(8)?;
            Ok((
                id,
                project_cwd,
                title,
                description,
                status_raw,
                priority_raw,
                linked_raw,
                created_at,
                updated_at,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut cards = Vec::new();
    for row in rows {
        let (
            id,
            project_cwd,
            title,
            description,
            status_raw,
            priority_raw,
            linked_raw,
            created_at,
            updated_at,
        ) = row.map_err(|e| e.to_string())?;

        let status = BoardStatus::parse(&status_raw)?;
        let priority = BoardPriority::parse(&priority_raw)?;
        let linked_session_ids: Vec<String> = serde_json::from_str(&linked_raw).unwrap_or_default();
        let media = list_card_media_refs(conn, &project_cwd, &id)?;

        cards.push(ProjectBoardCard {
            id,
            project_cwd,
            title,
            description,
            status,
            priority,
            linked_session_ids,
            media,
            created_at,
            updated_at,
        });
    }

    Ok(cards)
}

pub fn list_card_media_refs(
    conn: &Connection,
    project_cwd: &str,
    card_id: &str,
) -> Result<Vec<BoardMediaRef>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, mime_type, byte_length
             FROM project_board_media
             WHERE project_cwd = ?1 AND card_id = ?2
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_cwd, card_id], |row| {
            let byte_length: i64 = row.get(3)?;
            Ok(BoardMediaRef {
                id: row.get(0)?,
                name: row.get(1)?,
                mime_type: row.get(2)?,
                byte_length: byte_length as usize,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

pub fn upsert_card(
    conn: &Connection,
    input: &ProjectBoardCardInput,
) -> Result<ProjectBoardCard, String> {
    validate_card_input(input)?;

    let existing: bool = conn
        .query_row(
            "SELECT 1 FROM project_board_cards WHERE project_cwd = ?1 AND id = ?2",
            params![input.project_cwd, input.id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);

    if existing {
        return Err(format!(
            "Card with ID '{}' already exists in project",
            input.id
        ));
    }

    let now = now_millis();
    let created_at = now;
    let updated_at = now;
    let linked_sessions_json =
        serde_json::to_string(&input.linked_session_ids).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO project_board_cards
         (id, project_cwd, title, description, status, priority, linked_sessions_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            input.id,
            input.project_cwd,
            input.title,
            input.description,
            input.status.as_str(),
            input.priority.as_str(),
            linked_sessions_json,
            created_at,
            updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    let media = list_card_media_refs(conn, &input.project_cwd, &input.id)?;

    Ok(ProjectBoardCard {
        id: input.id.clone(),
        project_cwd: input.project_cwd.clone(),
        title: input.title.clone(),
        description: input.description.clone(),
        status: input.status,
        priority: input.priority,
        linked_session_ids: input.linked_session_ids.clone(),
        media,
        created_at,
        updated_at,
    })
}

pub fn patch_card(
    conn: &Connection,
    project_cwd: &str,
    card_id: &str,
    patch: &ProjectBoardCardPatch,
) -> Result<ProjectBoardCard, String> {
    validate_project_cwd(project_cwd)?;
    validate_id(card_id, "card")?;
    validate_card_patch(patch)?;

    let now = now_millis();

    let rows_affected = conn
        .execute(
            "UPDATE project_board_cards
             SET title = COALESCE(?1, title),
                 description = COALESCE(?2, description),
                 status = COALESCE(?3, status),
                 priority = COALESCE(?4, priority),
                 updated_at = ?5
             WHERE project_cwd = ?6 AND id = ?7",
            params![
                patch.title.as_deref(),
                patch.description.as_deref(),
                patch.status.map(|s| s.as_str()),
                patch.priority.map(|p| p.as_str()),
                now,
                project_cwd,
                card_id,
            ],
        )
        .map_err(|e| e.to_string())?;

    if rows_affected == 0 {
        return Err(format!("Card '{card_id}' not found in authorized project"));
    }

    let row = conn
        .query_row(
            "SELECT title, description, status, priority, linked_sessions_json, created_at, updated_at
             FROM project_board_cards
             WHERE project_cwd = ?1 AND id = ?2",
            params![project_cwd, card_id],
            |r| {
                let title: String = r.get(0)?;
                let description: String = r.get(1)?;
                let status_raw: String = r.get(2)?;
                let priority_raw: String = r.get(3)?;
                let linked_raw: String = r.get(4)?;
                let created_at: i64 = r.get(5)?;
                let updated_at: i64 = r.get(6)?;
                Ok((
                    title,
                    description,
                    status_raw,
                    priority_raw,
                    linked_raw,
                    created_at,
                    updated_at,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let (title, description, status_raw, priority_raw, linked_raw, created_at, updated_at) = row;
    let status = BoardStatus::parse(&status_raw)?;
    let priority = BoardPriority::parse(&priority_raw)?;
    let linked_session_ids: Vec<String> = serde_json::from_str(&linked_raw).unwrap_or_default();
    let media = list_card_media_refs(conn, project_cwd, card_id)?;

    Ok(ProjectBoardCard {
        id: card_id.to_string(),
        project_cwd: project_cwd.to_string(),
        title,
        description,
        status,
        priority,
        linked_session_ids,
        media,
        created_at,
        updated_at,
    })
}

pub fn link_session(
    conn: &mut Connection,
    project_cwd: &str,
    card_id: &str,
    session_id: &str,
) -> Result<Option<ProjectBoardCard>, String> {
    validate_project_cwd(project_cwd)?;
    validate_id(card_id, "card")?;
    validate_id(session_id, "session")?;

    // Acquire SQLite's write reservation before reading the session list. This
    // serializes the read-modify-write sequence across separate connections.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;

    let row = tx
        .query_row(
            "SELECT title, description, priority, linked_sessions_json, created_at
             FROM project_board_cards
             WHERE project_cwd = ?1 AND id = ?2",
            params![project_cwd, card_id],
            |r| {
                let title: String = r.get(0)?;
                let description: String = r.get(1)?;
                let priority_raw: String = r.get(2)?;
                let linked_raw: String = r.get(3)?;
                let created_at: i64 = r.get(4)?;
                Ok((title, description, priority_raw, linked_raw, created_at))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (title, description, priority_raw, linked_raw, created_at) = match row {
        Some(r) => r,
        // Dropping the transaction rolls back; a deleted card is never recreated.
        None => return Ok(None),
    };

    let priority = BoardPriority::parse(&priority_raw)?;
    let mut linked: Vec<String> = serde_json::from_str(&linked_raw).unwrap_or_default();
    if !linked.contains(&session_id.to_string()) {
        if linked.len() >= LINKED_SESSIONS_MAX {
            return Err(format!(
                "Card exceeds maximum of {LINKED_SESSIONS_MAX} linked sessions"
            ));
        }
        linked.push(session_id.to_string());
    }

    let now = now_millis();
    let linked_sessions_json = serde_json::to_string(&linked).map_err(|e| e.to_string())?;
    let in_progress = BoardStatus::InProgress;

    tx.execute(
        "UPDATE project_board_cards
         SET status = ?1, linked_sessions_json = ?2, updated_at = ?3
         WHERE project_cwd = ?4 AND id = ?5",
        params![
            in_progress.as_str(),
            linked_sessions_json,
            now,
            project_cwd,
            card_id,
        ],
    )
    .map_err(|e| e.to_string())?;

    let media = list_card_media_refs(&tx, project_cwd, card_id)?;

    let card = ProjectBoardCard {
        id: card_id.to_string(),
        project_cwd: project_cwd.to_string(),
        title,
        description,
        status: in_progress,
        priority,
        linked_session_ids: linked,
        media,
        created_at,
        updated_at: now,
    };
    tx.commit().map_err(|e| e.to_string())?;

    Ok(Some(card))
}

pub fn delete_card(conn: &mut Connection, project_cwd: &str, card_id: &str) -> Result<(), String> {
    validate_project_cwd(project_cwd)?;
    validate_id(card_id, "card")?;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM project_board_media WHERE project_cwd = ?1 AND card_id = ?2",
        params![project_cwd, card_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM project_board_cards WHERE project_cwd = ?1 AND id = ?2",
        params![project_cwd, card_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

pub fn add_media(conn: &Connection, input: &AddMediaInput) -> Result<BoardMediaRef, String> {
    validate_project_cwd(&input.project_cwd)?;
    validate_id(&input.card_id, "card")?;

    // Card must exist in project_cwd
    let card_exists: bool = conn
        .query_row(
            "SELECT 1 FROM project_board_cards WHERE project_cwd = ?1 AND id = ?2",
            params![input.project_cwd, input.card_id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);

    if !card_exists {
        return Err("Target card was not found in the specified project".into());
    }

    let media_id = match &input.id {
        Some(id) if !id.trim().is_empty() => {
            validate_id(id, "media")?;
            id.clone()
        }
        _ => uuid::Uuid::new_v4().to_string(),
    };

    let existing_media: bool = conn
        .query_row(
            "SELECT 1 FROM project_board_media WHERE id = ?1",
            params![media_id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);

    if existing_media {
        return Err(format!("Media with ID '{media_id}' already exists"));
    }

    let name = if input.name.trim().is_empty() {
        "attachment".to_string()
    } else {
        input.name.trim().to_string()
    };

    let (mime_type, data_bytes) = decode_and_validate_media(&input.mime_type, &input.data_base64)?;
    let byte_length = data_bytes.len();
    let byte_length_i64 = byte_length as i64;
    let now = now_millis();

    conn.execute(
        "INSERT INTO project_board_media
         (id, project_cwd, card_id, name, mime_type, byte_length, data_bytes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            media_id,
            input.project_cwd,
            input.card_id,
            name,
            mime_type,
            byte_length_i64,
            data_bytes,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(BoardMediaRef {
        id: media_id,
        name,
        mime_type,
        byte_length,
    })
}

pub fn get_media(
    conn: &Connection,
    project_cwd: &str,
    card_id: &str,
    media_id: &str,
) -> Result<Option<BoardMedia>, String> {
    validate_project_cwd(project_cwd)?;
    validate_id(card_id, "card")?;
    validate_id(media_id, "media")?;

    let row = conn
        .query_row(
            "SELECT name, mime_type, byte_length, data_bytes
             FROM project_board_media
             WHERE project_cwd = ?1 AND card_id = ?2 AND id = ?3",
            params![project_cwd, card_id, media_id],
            |row| {
                let name: String = row.get(0)?;
                let mime_type: String = row.get(1)?;
                let byte_length: i64 = row.get(2)?;
                let data_bytes: Vec<u8> = row.get(3)?;
                Ok((name, mime_type, byte_length as usize, data_bytes))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    match row {
        Some((name, mime_type, byte_length, data_bytes)) => {
            let data_base64 = BASE64_STANDARD.encode(&data_bytes);
            Ok(Some(BoardMedia {
                id: media_id.to_string(),
                name,
                mime_type,
                byte_length,
                data_base64,
            }))
        }
        None => Ok(None),
    }
}

pub fn delete_media(
    conn: &mut Connection,
    project_cwd: &str,
    card_id: &str,
    media_id: &str,
) -> Result<(), String> {
    validate_project_cwd(project_cwd)?;
    validate_id(card_id, "card")?;
    validate_id(media_id, "media")?;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM project_board_media
         WHERE project_cwd = ?1 AND card_id = ?2 AND id = ?3",
        params![project_cwd, card_id, media_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

// ======================== Tauri Commands ========================

#[tauri::command(async)]
pub fn project_board_list(
    store: State<'_, SessionStore>,
    project_cwd: String,
) -> Result<Vec<ProjectBoardCard>, String> {
    let conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    list_cards(&conn, &project_cwd)
}

// Tauri command maintains backwards compatibility with both nested input objects and flattened arguments.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn project_board_upsert_card(
    store: State<'_, SessionStore>,
    input: Option<ProjectBoardCardInput>,
    card: Option<ProjectBoardCardInput>,
    id: Option<String>,
    project_cwd: Option<String>,
    title: Option<String>,
    description: Option<String>,
    status: Option<BoardStatus>,
    priority: Option<BoardPriority>,
    linked_session_ids: Option<Vec<String>>,
) -> Result<ProjectBoardCard, String> {
    let resolved = if let Some(i) = input {
        i
    } else if let Some(c) = card {
        c
    } else if let (
        Some(id),
        Some(project_cwd),
        Some(title),
        Some(description),
        Some(status),
        Some(priority),
    ) = (id, project_cwd, title, description, status, priority)
    {
        ProjectBoardCardInput {
            id,
            project_cwd,
            title,
            description,
            status,
            priority,
            linked_session_ids: linked_session_ids.unwrap_or_default(),
        }
    } else {
        return Err("Missing required card input fields".into());
    };

    let conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    upsert_card(&conn, &resolved)
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn project_board_patch_card(
    store: State<'_, SessionStore>,
    project_cwd: String,
    card_id: String,
    patch: Option<ProjectBoardCardPatch>,
    title: Option<String>,
    description: Option<String>,
    status: Option<BoardStatus>,
    priority: Option<BoardPriority>,
) -> Result<ProjectBoardCard, String> {
    let resolved_patch = if let Some(p) = patch {
        p
    } else {
        ProjectBoardCardPatch {
            title,
            description,
            status,
            priority,
        }
    };
    let conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    patch_card(&conn, &project_cwd, &card_id, &resolved_patch)
}

#[tauri::command(async)]
pub fn project_board_link_session(
    store: State<'_, SessionStore>,
    project_cwd: String,
    card_id: String,
    session_id: String,
) -> Result<Option<ProjectBoardCard>, String> {
    let mut conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    link_session(&mut conn, &project_cwd, &card_id, &session_id)
}

#[tauri::command(async)]
pub fn project_board_delete_card(
    store: State<'_, SessionStore>,
    project_cwd: String,
    card_id: String,
) -> Result<(), String> {
    let mut conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    delete_card(&mut conn, &project_cwd, &card_id)
}

// Tauri command maintains backwards compatibility with both nested input objects and flattened arguments.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn project_board_add_media(
    store: State<'_, SessionStore>,
    input: Option<AddMediaInput>,
    media: Option<AddMediaInput>,
    id: Option<String>,
    project_cwd: Option<String>,
    card_id: Option<String>,
    name: Option<String>,
    mime_type: Option<String>,
    data_base64: Option<String>,
) -> Result<BoardMediaRef, String> {
    let resolved = if let Some(i) = input {
        i
    } else if let Some(m) = media {
        m
    } else if let (
        Some(project_cwd),
        Some(card_id),
        Some(name),
        Some(mime_type),
        Some(data_base64),
    ) = (project_cwd, card_id, name, mime_type, data_base64)
    {
        AddMediaInput {
            id,
            project_cwd,
            card_id,
            name,
            mime_type,
            data_base64,
        }
    } else {
        return Err("Missing required media input fields".into());
    };

    let conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    add_media(&conn, &resolved)
}

#[tauri::command(async)]
pub fn project_board_get_media(
    store: State<'_, SessionStore>,
    project_cwd: String,
    card_id: String,
    media_id: String,
) -> Result<Option<BoardMedia>, String> {
    let conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    get_media(&conn, &project_cwd, &card_id, &media_id)
}

#[tauri::command(async)]
pub fn project_board_delete_media(
    store: State<'_, SessionStore>,
    project_cwd: String,
    card_id: String,
    media_id: String,
) -> Result<(), String> {
    let mut conn = store.lock_conn()?;
    ensure_tables(&conn).map_err(|e| e.to_string())?;
    delete_media(&mut conn, &project_cwd, &card_id, &media_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_validation() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        // Empty title
        let bad_title = ProjectBoardCardInput {
            id: "card-1".into(),
            project_cwd: "/test/project".into(),
            title: "".into(),
            description: "desc".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::Medium,
            linked_session_ids: vec![],
        };
        assert!(upsert_card(&conn, &bad_title).is_err());

        // Long title > 160 chars
        let long_title = ProjectBoardCardInput {
            title: "a".repeat(161),
            ..bad_title.clone()
        };
        assert!(upsert_card(&conn, &long_title).is_err());

        // Valid title of 160 chars
        let ok_title = ProjectBoardCardInput {
            title: "a".repeat(160),
            ..bad_title.clone()
        };
        assert!(upsert_card(&conn, &ok_title).is_ok());

        // Long description > 12000 chars
        let long_desc = ProjectBoardCardInput {
            title: "Valid".into(),
            description: "d".repeat(12001),
            ..bad_title.clone()
        };
        assert!(upsert_card(&conn, &long_desc).is_err());

        // Max 50 linked sessions
        let too_many_sessions = ProjectBoardCardInput {
            title: "Valid".into(),
            description: "Valid".into(),
            linked_session_ids: (0..51).map(|i| format!("sess-{i}")).collect(),
            ..bad_title.clone()
        };
        assert!(upsert_card(&conn, &too_many_sessions).is_err());

        // Invalid unsafe ID
        let unsafe_id = ProjectBoardCardInput {
            id: "card/bad!id".into(),
            title: "Valid".into(),
            description: "Valid".into(),
            linked_session_ids: vec![],
            ..bad_title.clone()
        };
        assert!(upsert_card(&conn, &unsafe_id).is_err());

        // Empty project cwd
        let empty_cwd = ProjectBoardCardInput {
            id: "card-1".into(),
            project_cwd: "".into(),
            title: "Valid".into(),
            description: "Valid".into(),
            linked_session_ids: vec![],
            ..bad_title.clone()
        };
        assert!(upsert_card(&conn, &empty_cwd).is_err());
    }

    #[test]
    fn test_project_isolation_and_cascade() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        let card_a = ProjectBoardCardInput {
            id: "card-common".into(),
            project_cwd: "/project/a".into(),
            title: "Card in A".into(),
            description: "Desc A".into(),
            status: BoardStatus::Ready,
            priority: BoardPriority::High,
            linked_session_ids: vec!["sess-1".into()],
        };
        let card_b = ProjectBoardCardInput {
            id: "card-common".into(),
            project_cwd: "/project/b".into(),
            title: "Card in B".into(),
            description: "Desc B".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::Low,
            linked_session_ids: vec![],
        };

        upsert_card(&conn, &card_a).unwrap();
        upsert_card(&conn, &card_b).unwrap();

        // Add media to project A's card
        let fake_png = BASE64_STANDARD.encode(b"fake png data for project a");
        let media_a = add_media(
            &conn,
            &AddMediaInput {
                id: Some("media-1".into()),
                project_cwd: "/project/a".into(),
                card_id: "card-common".into(),
                name: "screenshot.png".into(),
                mime_type: "image/png".into(),
                data_base64: fake_png,
            },
        )
        .unwrap();

        // Project B cannot read Project A's media
        let get_from_b = get_media(&conn, "/project/b", "card-common", &media_a.id).unwrap();
        assert!(get_from_b.is_none());

        // Project A can read its own media
        let get_from_a = get_media(&conn, "/project/a", "card-common", &media_a.id).unwrap();
        assert!(get_from_a.is_some());
        assert_eq!(get_from_a.unwrap().name, "screenshot.png");

        // Project B attempting to delete Project A's media does not delete it
        delete_media(&mut conn, "/project/b", "card-common", &media_a.id).unwrap();
        assert!(get_media(&conn, "/project/a", "card-common", &media_a.id)
            .unwrap()
            .is_some());

        // Deleting card A cascades and removes its media
        delete_card(&mut conn, "/project/a", "card-common").unwrap();
        assert!(get_media(&conn, "/project/a", "card-common", &media_a.id)
            .unwrap()
            .is_none());
        assert_eq!(list_cards(&conn, "/project/a").unwrap().len(), 0);

        // Project B's card still intact
        let b_cards = list_cards(&conn, "/project/b").unwrap();
        assert_eq!(b_cards.len(), 1);
        assert_eq!(b_cards[0].title, "Card in B");
    }

    #[test]
    fn test_media_constraints() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        let card = ProjectBoardCardInput {
            id: "card-test".into(),
            project_cwd: "/project/test".into(),
            title: "Test Card".into(),
            description: "Desc".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::Medium,
            linked_session_ids: vec![],
        };
        upsert_card(&conn, &card).unwrap();

        // 1. Malformed base64 rejected
        let malformed = add_media(
            &conn,
            &AddMediaInput {
                id: None,
                project_cwd: "/project/test".into(),
                card_id: "card-test".into(),
                name: "bad.png".into(),
                mime_type: "image/png".into(),
                data_base64: "not valid base64!@#%^".into(),
            },
        );
        assert!(malformed.is_err());
        assert!(malformed.unwrap_err().contains("Invalid base64"));

        let short_malformed = decode_and_validate_media("image/png", "=").unwrap_err();
        assert!(short_malformed.contains("Invalid base64"));

        // 2. Unsupported mime type rejected
        let unsupported_mime = add_media(
            &conn,
            &AddMediaInput {
                id: None,
                project_cwd: "/project/test".into(),
                card_id: "card-test".into(),
                name: "file.pdf".into(),
                mime_type: "application/pdf".into(),
                data_base64: BASE64_STANDARD.encode(b"some pdf content"),
            },
        );
        assert!(unsupported_mime.is_err());
        assert!(unsupported_mime
            .unwrap_err()
            .contains("Unsupported media type"));

        // 3. Media exceeding 5 MiB rejected
        let oversize_bytes = vec![0u8; 5 * 1024 * 1024 + 1];
        let oversize_b64 = BASE64_STANDARD.encode(&oversize_bytes);
        let oversize = add_media(
            &conn,
            &AddMediaInput {
                id: None,
                project_cwd: "/project/test".into(),
                card_id: "card-test".into(),
                name: "huge.jpg".into(),
                mime_type: "image/jpeg".into(),
                data_base64: oversize_b64,
            },
        );
        assert!(oversize.is_err());
        assert!(oversize.unwrap_err().contains("exceeds maximum allowed"));

        // 4. Allowed mime types: PNG, JPEG, GIF, WebP
        for mime in [
            "image/png",
            "image/jpeg",
            "image/jpg",
            "image/gif",
            "image/webp",
        ] {
            let res = add_media(
                &conn,
                &AddMediaInput {
                    id: None,
                    project_cwd: "/project/test".into(),
                    card_id: "card-test".into(),
                    name: format!("test.{mime}"),
                    mime_type: mime.into(),
                    data_base64: BASE64_STANDARD.encode(b"test image data"),
                },
            );
            assert!(res.is_ok(), "Failed for mime {mime}: {:?}", res.err());
        }
    }

    #[test]
    fn test_media_encoded_size_boundary_rejects_before_decode() {
        // The next complete base64 quantum above the largest encoded size that
        // can represent 5 MiB is oversized. A +1 length is rejected at the
        // same pre-decode guard, before the decoder can report malformed input.
        let immediately_over = "A".repeat(MEDIA_BASE64_BYTES_MAX + 1);
        let err = decode_and_validate_media("image/png", &immediately_over).unwrap_err();
        assert!(err.contains("exceeds maximum allowed"));

        let next_base64_quantum = "A".repeat(MEDIA_BASE64_BYTES_MAX + 4);
        let err = decode_and_validate_media("image/png", &next_base64_quantum).unwrap_err();
        assert!(err.contains("exceeds maximum allowed"));
    }

    #[test]
    fn test_media_at_maximum_size_accepts_data_uri_prefix() {
        let expected = vec![0x5a; MEDIA_BYTES_MAX];
        let encoded = BASE64_STANDARD.encode(&expected);
        assert_eq!(encoded.len(), MEDIA_BASE64_BYTES_MAX);

        let data_uri = format!("data:image/png;base64,{encoded}");
        let (mime_type, decoded) = decode_and_validate_media("image/png", &data_uri).unwrap();
        assert_eq!(mime_type, "image/png");
        assert_eq!(decoded.len(), MEDIA_BYTES_MAX);
        assert_eq!(decoded, expected);
    }

    #[test]
    fn test_upsert_card_is_creation_only_and_rejects_duplicate() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        let card = ProjectBoardCardInput {
            id: "card-timing".into(),
            project_cwd: "/project/test".into(),
            title: "Original".into(),
            description: "Original desc".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::Low,
            linked_session_ids: vec![],
        };

        let first = upsert_card(&conn, &card).unwrap();
        assert_eq!(first.title, "Original");

        // Second upsert with same ID must fail
        let duplicate = upsert_card(&conn, &card);
        assert!(duplicate.is_err(), "Duplicate upsert must fail");
        let err = duplicate.unwrap_err();
        assert!(
            err.contains("already exists"),
            "Expected duplicate error, got: {err}"
        );
    }

    #[test]
    fn test_patch_card_atomic_updates_and_preserves_omitted_fields() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        let card = ProjectBoardCardInput {
            id: "card-patch-test".into(),
            project_cwd: "/project/test".into(),
            title: "Initial Title".into(),
            description: "Initial Description".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::Low,
            linked_session_ids: vec!["session-alpha".into()],
        };
        let created = upsert_card(&conn, &card).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Patch only title and status, leaving description and priority untouched
        let patched = patch_card(
            &conn,
            "/project/test",
            "card-patch-test",
            &ProjectBoardCardPatch {
                title: Some("Patched Title".into()),
                status: Some(BoardStatus::InProgress),
                description: None,
                priority: None,
            },
        )
        .unwrap();

        assert_eq!(patched.title, "Patched Title");
        assert_eq!(patched.status, BoardStatus::InProgress);
        assert_eq!(patched.description, "Initial Description");
        assert_eq!(patched.priority, BoardPriority::Low);
        assert_eq!(patched.linked_session_ids, vec!["session-alpha"]);
        assert_eq!(patched.created_at, created.created_at);
        assert!(patched.updated_at >= created.updated_at);

        // Patching nonexistent or deleted card fails
        let missing = patch_card(
            &conn,
            "/project/test",
            "card-nonexistent",
            &ProjectBoardCardPatch {
                title: Some("Fails".into()),
                ..Default::default()
            },
        );
        assert!(missing.is_err());
    }

    #[test]
    fn test_link_session_preserves_user_fields_retains_multiple_and_no_duplicates() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        let card = ProjectBoardCardInput {
            id: "card-link-test".into(),
            project_cwd: "/project/test".into(),
            title: "Task Title".into(),
            description: "Detailed description".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::High,
            linked_session_ids: vec!["sess-1".into()],
        };
        let created = upsert_card(&conn, &card).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        // First link adds sess-2, sets in-progress, preserves fields
        let linked1 = link_session(&mut conn, "/project/test", "card-link-test", "sess-2")
            .unwrap()
            .expect("Card must exist");

        assert_eq!(linked1.status, BoardStatus::InProgress);
        assert_eq!(linked1.linked_session_ids, vec!["sess-1", "sess-2"]);
        assert_eq!(linked1.title, "Task Title");
        assert_eq!(linked1.description, "Detailed description");
        assert_eq!(linked1.priority, BoardPriority::High);
        assert_eq!(linked1.created_at, created.created_at);
        assert!(linked1.updated_at >= created.updated_at);

        // Duplicate link with sess-2 does not duplicate session ID
        let linked_dup = link_session(&mut conn, "/project/test", "card-link-test", "sess-2")
            .unwrap()
            .expect("Card must exist");
        assert_eq!(linked_dup.linked_session_ids, vec!["sess-1", "sess-2"]);

        // Sequential link with sess-3 retains all sessions
        let linked2 = link_session(&mut conn, "/project/test", "card-link-test", "sess-3")
            .unwrap()
            .expect("Card must exist");
        assert_eq!(
            linked2.linked_session_ids,
            vec!["sess-1", "sess-2", "sess-3"]
        );
        assert_eq!(linked2.title, "Task Title");
        assert_eq!(linked2.priority, BoardPriority::High);

        // Linking a card after deletion returns None and does not recreate it.
        delete_card(&mut conn, "/project/test", "card-link-test").unwrap();
        let not_found =
            link_session(&mut conn, "/project/test", "card-link-test", "sess-4").unwrap();
        assert!(not_found.is_none());

        let all = list_cards(&conn, "/project/test").unwrap();
        assert!(all.iter().all(|c| c.id != "card-link-test"));
    }

    #[test]
    fn test_link_session_serializes_read_modify_write_across_connections() {
        use std::sync::{Arc, Barrier};

        let db_path = std::env::temp_dir().join(format!(
            "monocode-project-board-link-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let setup_conn = Connection::open(&db_path).unwrap();
        setup_conn
            .execute_batch("PRAGMA busy_timeout = 5000;")
            .unwrap();
        ensure_tables(&setup_conn).unwrap();
        upsert_card(
            &setup_conn,
            &ProjectBoardCardInput {
                id: "card-concurrent-link".into(),
                project_cwd: "/project/test".into(),
                title: "Concurrent links".into(),
                description: "Keep both sessions".into(),
                status: BoardStatus::Ready,
                priority: BoardPriority::High,
                linked_session_ids: vec![],
            },
        )
        .unwrap();
        drop(setup_conn);

        let mut first_conn = Connection::open(&db_path).unwrap();
        let mut second_conn = Connection::open(&db_path).unwrap();
        first_conn
            .execute_batch("PRAGMA busy_timeout = 5000;")
            .unwrap();
        second_conn
            .execute_batch("PRAGMA busy_timeout = 5000;")
            .unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let first_barrier = Arc::clone(&barrier);
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            link_session(
                &mut first_conn,
                "/project/test",
                "card-concurrent-link",
                "session-concurrent-a",
            )
            .unwrap()
            .expect("card exists")
        });
        let second_barrier = Arc::clone(&barrier);
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            link_session(
                &mut second_conn,
                "/project/test",
                "card-concurrent-link",
                "session-concurrent-b",
            )
            .unwrap()
            .expect("card exists")
        });

        barrier.wait();
        let first_result = first.join().unwrap();
        let second_result = second.join().unwrap();
        assert!(first_result
            .linked_session_ids
            .contains(&"session-concurrent-a".to_string()));
        assert!(second_result
            .linked_session_ids
            .contains(&"session-concurrent-b".to_string()));

        let verify_conn = Connection::open(&db_path).unwrap();
        let saved = list_cards(&verify_conn, "/project/test").unwrap();
        let saved = saved
            .iter()
            .find(|card| card.id == "card-concurrent-link")
            .unwrap();
        assert_eq!(saved.status, BoardStatus::InProgress);
        let mut saved_session_ids = saved.linked_session_ids.clone();
        saved_session_ids.sort();
        assert_eq!(
            saved_session_ids,
            vec![
                "session-concurrent-a".to_string(),
                "session-concurrent-b".to_string()
            ]
        );
        drop(verify_conn);
        std::fs::remove_file(db_path).unwrap();
    }

    #[test]
    fn test_media_id_collision_rejects_without_replacement() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        ensure_tables(&conn).unwrap();

        let card = ProjectBoardCardInput {
            id: "card-media-collision".into(),
            project_cwd: "/project/test".into(),
            title: "Card With Media".into(),
            description: "Test description".into(),
            status: BoardStatus::Backlog,
            priority: BoardPriority::Medium,
            linked_session_ids: vec![],
        };
        upsert_card(&conn, &card).unwrap();

        let original_bytes = b"original media byte content";
        let original_b64 = BASE64_STANDARD.encode(original_bytes);
        let original_media = add_media(
            &conn,
            &AddMediaInput {
                id: Some("media-fixed-id".into()),
                project_cwd: "/project/test".into(),
                card_id: "card-media-collision".into(),
                name: "original_image.png".into(),
                mime_type: "image/png".into(),
                data_base64: original_b64,
            },
        )
        .unwrap();

        assert_eq!(original_media.id, "media-fixed-id");
        assert_eq!(original_media.name, "original_image.png");
        assert_eq!(original_media.mime_type, "image/png");
        assert_eq!(original_media.byte_length, original_bytes.len());

        // Attempt to insert another media with the same ID, different content and metadata
        let colliding_bytes = b"different colliding payload bytes";
        let colliding_b64 = BASE64_STANDARD.encode(colliding_bytes);
        let collision_result = add_media(
            &conn,
            &AddMediaInput {
                id: Some("media-fixed-id".into()),
                project_cwd: "/project/test".into(),
                card_id: "card-media-collision".into(),
                name: "replaced_image.jpg".into(),
                mime_type: "image/jpeg".into(),
                data_base64: colliding_b64.clone(),
            },
        );

        assert!(
            collision_result.is_err(),
            "Colliding media ID insert must fail"
        );
        let err_msg = collision_result.unwrap_err();
        assert!(
            err_msg.contains("already exists"),
            "Expected collision error, got: {err_msg}"
        );

        // Verify original media metadata and bytes remain unchanged
        let fetched = get_media(
            &conn,
            "/project/test",
            "card-media-collision",
            "media-fixed-id",
        )
        .unwrap()
        .expect("Original media must still exist");
        assert_eq!(fetched.id, "media-fixed-id");
        assert_eq!(fetched.name, "original_image.png");
        assert_eq!(fetched.mime_type, "image/png");
        assert_eq!(fetched.byte_length, original_bytes.len());
        let decoded = BASE64_STANDARD.decode(&fetched.data_base64).unwrap();
        assert_eq!(decoded, original_bytes);

        // Attempt to insert media with the same ID under a different card
        let card_b = ProjectBoardCardInput {
            id: "card-second".into(),
            project_cwd: "/project/test".into(),
            title: "Card Two".into(),
            description: "Second card".into(),
            status: BoardStatus::Ready,
            priority: BoardPriority::High,
            linked_session_ids: vec![],
        };
        upsert_card(&conn, &card_b).unwrap();

        let cross_card_result = add_media(
            &conn,
            &AddMediaInput {
                id: Some("media-fixed-id".into()),
                project_cwd: "/project/test".into(),
                card_id: "card-second".into(),
                name: "cross_card.png".into(),
                mime_type: "image/png".into(),
                data_base64: colliding_b64,
            },
        );
        assert!(
            cross_card_result.is_err(),
            "Cross-card media ID collision must fail"
        );

        // Original media remains intact
        let fetched_after = get_media(
            &conn,
            "/project/test",
            "card-media-collision",
            "media-fixed-id",
        )
        .unwrap()
        .expect("Original media must still exist");
        assert_eq!(fetched_after.name, "original_image.png");
        let decoded_after = BASE64_STANDARD.decode(&fetched_after.data_base64).unwrap();
        assert_eq!(decoded_after, original_bytes);
    }
}
