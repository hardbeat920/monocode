use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::fs::expand_home;
use crate::session_store::{now_millis, validate_id, SessionStore};

const TITLE_MAX: usize = 200;
const BODY_MAX: usize = 1_000_000;
const TAG_MAX: usize = 48;
const TAGS_MAX: usize = 20;
const IMAGE_MAX_BYTES: u64 = 20 * 1024 * 1024;
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
const NOTE_ASSET_DIR: &str = "note-assets";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_cwd: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteUpsert {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_session_id: Option<String>,
    #[serde(default)]
    pub source_cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteImageAsset {
    pub name: String,
    pub markdown_path: String,
}

pub fn ensure_notes_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
           id TEXT PRIMARY KEY,
           slug TEXT NOT NULL UNIQUE,
           title TEXT NOT NULL,
           body TEXT NOT NULL DEFAULT '',
           tags_json TEXT NOT NULL DEFAULT '[]',
           source_session_id TEXT,
           source_cwd TEXT,
           created_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS notes_updated_idx
           ON notes (updated_at DESC, id);",
    )?;
    let tags_present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'tags_json'",
        [],
        |row| row.get(0),
    )?;
    if tags_present == 0 {
        conn.execute(
            "ALTER TABLE notes ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    Ok(())
}

#[tauri::command(async)]
pub fn notes_list(store: State<'_, SessionStore>) -> Result<Vec<Note>, String> {
    let conn = store.lock_conn()?;
    list_notes(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn notes_get(store: State<'_, SessionStore>, id: String) -> Result<Option<Note>, String> {
    validate_id(&id, "note")?;
    let conn = store.lock_conn()?;
    get_note(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn notes_upsert(store: State<'_, SessionStore>, note: NoteUpsert) -> Result<Note, String> {
    validate_id(&note.id, "note")?;
    if let Some(session_id) = note.source_session_id.as_deref() {
        if !session_id.is_empty() {
            validate_id(session_id, "session")?;
        }
    }
    if note.body.len() > BODY_MAX {
        return Err("Note is too large".into());
    }
    let conn = store.lock_conn()?;
    upsert_note(&conn, &note).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn notes_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    validate_id(&id, "note")?;
    let conn = store.lock_conn()?;
    delete_note(&conn, &id).map_err(|e| e.to_string())?;
    drop(conn);
    // The note deletion is authoritative. A cleanup failure should not leave a
    // successfully deleted note visible in the UI.
    let _ = remove_note_assets(&app, &id);
    Ok(())
}

#[tauri::command]
pub async fn notes_save_image(
    app: AppHandle,
    note_id: String,
    source_path: String,
) -> Result<NoteImageAsset, String> {
    tauri::async_runtime::spawn_blocking(move || save_note_image_sync(&app, &note_id, &source_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn notes_image_path(app: AppHandle, asset: String) -> Result<String, String> {
    let relative = validate_note_asset_path(&asset)?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(relative);
    if !path.is_file() {
        return Err("Note image was not found".into());
    }
    Ok(path.to_string_lossy().into_owned())
}

fn note_assets_dir(app: &AppHandle, note_id: &str) -> Result<PathBuf, String> {
    validate_id(note_id, "note")?;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(NOTE_ASSET_DIR)
        .join(note_id))
}

fn save_note_image_sync(
    app: &AppHandle,
    note_id: &str,
    source_path: &str,
) -> Result<NoteImageAsset, String> {
    let source = expand_home(source_path);
    let meta = std::fs::metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    if meta.len() > IMAGE_MAX_BYTES {
        return Err(format!(
            "Image is too large (maximum {} MB).",
            IMAGE_MAX_BYTES / 1024 / 1024
        ));
    }

    let (display_name, safe_name) = note_image_names(&source)?;
    let dir = note_assets_dir(app, note_id)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stored_name = format!("{stamp}-{safe_name}");
    let destination = dir.join(&stored_name);
    std::fs::copy(&source, &destination).map_err(|e| format!("{}: {e}", destination.display()))?;

    Ok(NoteImageAsset {
        name: display_name,
        markdown_path: format!("/{NOTE_ASSET_DIR}/{note_id}/{stored_name}"),
    })
}

fn note_image_names(source: &Path) -> Result<(String, String), String> {
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return Err("Image must be a PNG, JPG, GIF, WebP, or SVG file.".into());
    }
    let display_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("image")
        .to_string();
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let mut safe_stem: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(80)
        .collect();
    safe_stem = safe_stem.trim_matches('-').to_string();
    if safe_stem.is_empty() {
        safe_stem = "image".into();
    }
    Ok((display_name, format!("{safe_stem}.{extension}")))
}

fn validate_note_asset_path(asset: &str) -> Result<PathBuf, String> {
    let relative = asset
        .strip_prefix('/')
        .ok_or_else(|| "Invalid note image path".to_string())?;
    let path = Path::new(relative);
    let parts = path
        .components()
        .map(|part| match part {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Invalid note image path".to_string())?;
    if parts.len() != 3 || parts[0] != NOTE_ASSET_DIR {
        return Err("Invalid note image path".into());
    }
    validate_id(&parts[1], "note")?;
    if parts[2].is_empty()
        || !parts[2]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("Invalid note image path".into());
    }
    Ok(path.to_path_buf())
}

fn remove_note_assets(app: &AppHandle, note_id: &str) -> Result<(), String> {
    let dir = note_assets_dir(app, note_id)?;
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn list_notes(conn: &Connection) -> rusqlite::Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, slug, title, body, source_session_id, source_cwd, tags_json,
                created_at, updated_at
         FROM notes
         ORDER BY updated_at DESC, id ASC",
    )?;
    let rows = stmt.query_map([], read_note)?;
    rows.collect()
}

fn get_note(conn: &Connection, id: &str) -> rusqlite::Result<Option<Note>> {
    conn.query_row(
        "SELECT id, slug, title, body, source_session_id, source_cwd, tags_json,
                created_at, updated_at
         FROM notes
         WHERE id = ?1",
        params![id],
        read_note,
    )
    .optional()
}

fn upsert_note(conn: &Connection, note: &NoteUpsert) -> rusqlite::Result<Note> {
    let title = normalize_title(&note.title);
    let body = note.body.replace("\r\n", "\n").replace('\r', "\n");
    let tags = normalize_tags(&note.tags);
    let tags_json = serde_json::to_string(&tags)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let source_session_id = note
        .source_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let source_cwd = note
        .source_cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let now = now_millis();

    let existing: Option<(String, i64, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT slug, created_at, source_session_id, source_cwd
             FROM notes WHERE id = ?1",
            params![note.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    if let Some((slug, created_at, existing_session, existing_cwd)) = existing {
        conn.execute(
            "UPDATE notes
             SET title = ?1, body = ?2, tags_json = ?3, updated_at = ?4
             WHERE id = ?5",
            params![title, body, tags_json, now, note.id],
        )?;
        Ok(Note {
            id: note.id.clone(),
            slug,
            title,
            body,
            tags,
            source_session_id: existing_session,
            source_cwd: existing_cwd,
            created_at,
            updated_at: now,
        })
    } else {
        let slug = unique_slug(conn, &title)?;
        conn.execute(
            "INSERT INTO notes (
               id, slug, title, body, source_session_id, source_cwd, tags_json,
               created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                note.id,
                slug,
                title,
                body,
                source_session_id,
                source_cwd,
                tags_json,
                now,
                now
            ],
        )?;
        Ok(Note {
            id: note.id.clone(),
            slug,
            title,
            body,
            tags,
            source_session_id: source_session_id.map(str::to_string),
            source_cwd: source_cwd.map(str::to_string),
            created_at: now,
            updated_at: now,
        })
    }
}

fn delete_note(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

fn read_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    let tags_json: String = row.get(6)?;
    let tags = serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default();
    Ok(Note {
        id: row.get(0)?,
        slug: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        tags,
        source_session_id: row.get(4)?,
        source_cwd: row.get(5)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for input in tags {
        let tag = input
            .trim()
            .trim_start_matches('#')
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .to_lowercase();
        let tag: String = tag.chars().take(TAG_MAX).collect();
        let tag = tag.trim_end_matches('-').to_string();
        if tag.is_empty() || normalized.contains(&tag) {
            continue;
        }
        normalized.push(tag);
        if normalized.len() == TAGS_MAX {
            break;
        }
    }
    normalized
}

fn normalize_title(title: &str) -> String {
    let trimmed = title.trim();
    let sliced: String = trimmed.chars().take(TITLE_MAX).collect();
    let sliced = sliced.trim().to_string();
    if sliced.is_empty() {
        "Untitled".into()
    } else {
        sliced
    }
}

fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in title.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    let slug = out.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "note".into()
    } else {
        slug
    }
}

fn unique_slug(conn: &Connection, title: &str) -> rusqlite::Result<String> {
    let base = slugify(title);
    for index in 0..1000 {
        let candidate = if index == 0 {
            base.clone()
        } else {
            format!("{base}-{}", index + 1)
        };
        let taken: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE slug = ?1",
            params![candidate],
            |row| row.get(0),
        )?;
        if taken == 0 {
            return Ok(candidate);
        }
    }
    Ok(format!("{base}-{}", now_millis()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::SessionStore;

    fn upsert(store: &SessionStore, id: &str, title: &str, body: &str) -> Note {
        let conn = store.lock_conn().unwrap();
        upsert_note(
            &conn,
            &NoteUpsert {
                id: id.into(),
                title: title.into(),
                body: body.into(),
                tags: Vec::new(),
                source_session_id: None,
                source_cwd: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn migrate_creates_notes_table() {
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'notes'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, 1);
        let version: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 13",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn ensure_table_adds_tags_to_an_existing_notes_database() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE notes (
               id TEXT PRIMARY KEY,
               slug TEXT NOT NULL UNIQUE,
               title TEXT NOT NULL,
               body TEXT NOT NULL DEFAULT '',
               source_session_id TEXT,
               source_cwd TEXT,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );",
        )
        .unwrap();

        ensure_notes_table(&conn).unwrap();

        let tags_column: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'tags_json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tags_column, 1);
    }

    #[test]
    fn insert_update_and_list_newest_first() {
        let store = SessionStore::open_in_memory().unwrap();
        let first = upsert(&store, "n1", "Alpha", "one");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = upsert(&store, "n2", "Beta", "two");
        assert_eq!(first.slug, "alpha");
        assert_eq!(second.slug, "beta");

        let conn = store.lock_conn().unwrap();
        let listed = list_notes(&conn).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>(),
            vec!["n2", "n1"]
        );

        std::thread::sleep(std::time::Duration::from_millis(5));
        let updated = upsert_note(
            &conn,
            &NoteUpsert {
                id: "n1".into(),
                title: "Alpha renamed".into(),
                body: "changed".into(),
                tags: vec!["Ideas".into(), "project docs".into(), "ideas".into()],
                source_session_id: Some("sess-1".into()),
                source_cwd: Some("/tmp/a".into()),
            },
        )
        .unwrap();
        assert_eq!(updated.slug, "alpha");
        assert_eq!(updated.title, "Alpha renamed");
        assert_eq!(updated.body, "changed");
        assert_eq!(updated.tags, vec!["ideas", "project-docs"]);
        assert_eq!(updated.created_at, first.created_at);
        assert!(updated.updated_at > first.updated_at);
        // Provenance is capture-time only; later edits must not rewrite it.
        assert_eq!(updated.source_session_id, None);
        assert_eq!(updated.source_cwd, None);
    }

    #[test]
    fn slug_collisions_get_a_numeric_suffix() {
        let store = SessionStore::open_in_memory().unwrap();
        let first = upsert(&store, "n1", "Auth approach", "a");
        let second = upsert(&store, "n2", "Auth approach", "b");
        assert_eq!(first.slug, "auth-approach");
        assert_eq!(second.slug, "auth-approach-2");
    }

    #[test]
    fn empty_title_becomes_untitled() {
        let store = SessionStore::open_in_memory().unwrap();
        let note = upsert(&store, "n1", "   ", "");
        assert_eq!(note.title, "Untitled");
        assert_eq!(note.slug, "untitled");
    }

    #[test]
    fn note_image_names_are_safe_and_keep_supported_extensions() {
        assert_eq!(
            note_image_names(Path::new("/tmp/Architecture draft [2].PNG")).unwrap(),
            (
                "Architecture draft [2].PNG".into(),
                "Architecture-draft--2.png".into()
            )
        );
        assert!(note_image_names(Path::new("/tmp/archive.zip")).is_err());
    }

    #[test]
    fn note_asset_paths_cannot_escape_app_data() {
        assert_eq!(
            validate_note_asset_path("/note-assets/note-1/123-image.png").unwrap(),
            PathBuf::from("note-assets/note-1/123-image.png")
        );
        assert!(validate_note_asset_path("/note-assets/note-1/../secret.png").is_err());
        assert!(validate_note_asset_path("/other/note-1/image.png").is_err());
    }

    #[test]
    fn delete_removes_the_row() {
        let store = SessionStore::open_in_memory().unwrap();
        upsert(&store, "n1", "Gone", "bye");
        let conn = store.lock_conn().unwrap();
        delete_note(&conn, "n1").unwrap();
        assert!(get_note(&conn, "n1").unwrap().is_none());
        assert!(list_notes(&conn).unwrap().is_empty());
    }

    #[test]
    fn slugify_strips_punctuation() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("***"), "note");
        assert_eq!(slugify("Ä"), "note");
    }
}
