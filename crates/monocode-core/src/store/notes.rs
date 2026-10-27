//! Desktop-compatible note persistence, deliberately separate from session handles.
use super::Store;
use crate::notes::{Note, NoteImageAsset, NoteUpsert, MAX_BODY};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Component, Path, PathBuf};
const TITLE_MAX: usize = 200;
const TAG_MAX: usize = 48;
const TAGS_MAX: usize = 20;
fn now_millis() -> i64 {
    crate::time::now_ms().min(i64::MAX as u64) as i64
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
           slug_pending INTEGER NOT NULL DEFAULT 0,
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
    let pending_present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'slug_pending'",
        [],
        |row| row.get(0),
    )?;
    if pending_present == 0 {
        // Existing slugs may already be referenced, so every existing row
        // starts established (0) whatever its slug looks like.
        conn.execute(
            "ALTER TABLE notes ADD COLUMN slug_pending INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    let kind_present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'content_kind'",
        [],
        |row| row.get(0),
    )?;
    if kind_present == 0 {
        conn.execute(
            "ALTER TABLE notes ADD COLUMN content_kind TEXT NOT NULL DEFAULT 'note'",
            [],
        )?;
    }
    let artifact_kind_present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'artifact_kind'",
        [],
        |row| row.get(0),
    )?;
    if artifact_kind_present == 0 {
        conn.execute("ALTER TABLE notes ADD COLUMN artifact_kind TEXT", [])?;
    }
    Ok(())
}

fn get_content(conn: &Connection, id: &str, kind: &str) -> rusqlite::Result<Option<Note>> {
    conn.query_row(
        "SELECT id, slug, title, body, source_session_id, source_cwd, tags_json,
                created_at, updated_at, slug_pending
         FROM notes
         WHERE id = ?1 AND content_kind = ?2",
        params![id, kind],
        read_note,
    )
    .optional()
}

fn upsert_note(conn: &Connection, note: &NoteUpsert) -> rusqlite::Result<Note> {
    upsert_content(conn, note, "note", None)
}

pub(crate) fn upsert_content(
    conn: &Connection,
    note: &NoteUpsert,
    kind: &str,
    artifact_kind: Option<&str>,
) -> rusqlite::Result<Note> {
    let existing_kind: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT content_kind, artifact_kind FROM notes WHERE id = ?1",
            params![note.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if existing_kind
        .as_ref()
        .is_some_and(|(category, subtype)| category != kind || subtype.as_deref() != artifact_kind)
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "Content belongs to a different kind".into(),
        ));
    }
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

    if let Some(existing) = get_content(conn, &note.id, kind)? {
        let project_cwd = source_cwd.map(str::to_string).or(existing.source_cwd);
        // Project changes keep the note in its current position in the list.
        let updated_at =
            if title == existing.title && body == existing.body && tags == existing.tags {
                existing.updated_at
            } else {
                now
            };
        // A pending slug is replaced once, by the first finalizing save with
        // a real title; every other slug stays put so @note/ references keep
        // working. Nothing here sets pending back on.
        let (slug, slug_pending) =
            if note.finalize_slug && existing.slug_pending && !is_placeholder_title(&title) {
                (unique_slug(conn, &title)?, false)
            } else {
                (existing.slug, existing.slug_pending)
            };
        conn.execute(
            "UPDATE notes
             SET title = ?1, body = ?2, tags_json = ?3, updated_at = ?4,
                 source_cwd = ?6, slug = ?7, slug_pending = ?8
             WHERE id = ?5",
            params![
                title,
                body,
                tags_json,
                updated_at,
                note.id,
                project_cwd,
                slug,
                slug_pending
            ],
        )?;
        Ok(Note {
            id: note.id.clone(),
            slug,
            title,
            body,
            tags,
            source_session_id: existing.source_session_id,
            source_cwd: project_cwd,
            slug_pending,
            created_at: existing.created_at,
            updated_at,
        })
    } else {
        // Decided before normalization: a blank title gets a generated slug,
        // while an explicit "Untitled" or "Untitled 2" owns the slug it gets.
        let slug_pending = note.title.trim().is_empty();
        let slug = unique_slug(conn, &title)?;
        conn.execute(
            "INSERT INTO notes (
               id, slug, title, body, source_session_id, source_cwd, tags_json,
               created_at, updated_at, slug_pending, content_kind, artifact_kind
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                note.id,
                slug,
                title,
                body,
                source_session_id,
                source_cwd,
                tags_json,
                now,
                now,
                slug_pending,
                kind,
                artifact_kind
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
            slug_pending,
            created_at: now,
            updated_at: now,
        })
    }
}

fn delete_note(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM notes WHERE id = ?1 AND content_kind = 'note'",
        params![id],
    )?;
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
        slug_pending: row.get(9)?,
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

/// Lowercase ASCII words of `title` joined by single hyphens, untruncated.
fn slug_words(title: &str) -> String {
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
    }
    out.trim_end_matches('-').to_string()
}

fn slugify(title: &str) -> String {
    let words = slug_words(title);
    let slug = words[..words.len().min(48)].trim_end_matches('-');
    if slug.is_empty() {
        "note".into()
    } else {
        slug.into()
    }
}

/// "Untitled" and "Untitled 2" read like generated slugs, so finalizing a
/// pending note under such a title would just trade one placeholder for
/// another; the note stays pending until it gets a real title. Checks the
/// whole title so text past the slug length cutoff still counts.
fn is_placeholder_title(title: &str) -> bool {
    let words = slug_words(title);
    match words.strip_prefix("untitled") {
        Some("") => true,
        Some(rest) => rest
            .strip_prefix('-')
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())),
        None => false,
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

fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 200
        || !id
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
    {
        return Err("Invalid note ID".into());
    }
    Ok(())
}

impl Store {
    /// Full-fidelity desktop notes. Artifacts in the same table are excluded.
    pub fn list_notes(&self) -> Result<Vec<Note>, String> {
        let mut notes = if super::shared::has_table(&self.conn, "notes")? {
            let columns = self
                .conn
                .prepare("PRAGMA table_info(notes)")
                .map_err(|e| e.to_string())?
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<std::collections::HashSet<_>>>()
                .map_err(|e| e.to_string())?;
            let pending = if columns.contains("slug_pending") {
                "slug_pending"
            } else {
                "0"
            };
            let tags = if columns.contains("tags_json") {
                "tags_json"
            } else {
                "'[]'"
            };
            let kind = if columns.contains("content_kind") {
                "WHERE content_kind='note'"
            } else {
                ""
            };
            let mut query = self.conn.prepare(&format!("SELECT id,slug,title,body,source_session_id,source_cwd,{tags},created_at,updated_at,{pending} FROM notes {kind} ORDER BY updated_at DESC,id ASC")).map_err(|e| e.to_string())?;
            let rows = query.query_map([], read_note).map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        // Keep early native prototype notes accessible without modifying data on read.
        if self.shared.is_none() {
            for note in self.legacy_notes()? {
                let id = format!("gpui-note-{}", note.id);
                if notes.iter().any(|saved| saved.id == id) {
                    continue;
                }
                notes.push(Note {
                    id,
                    slug: format!("note-{}", note.id),
                    title: note.title,
                    body: note.body,
                    tags: note.tags,
                    source_session_id: (note.source_session != 0)
                        .then(|| note.source_session.to_string()),
                    source_cwd: (!note.source_cwd.is_empty()).then_some(note.source_cwd),
                    slug_pending: false,
                    created_at: note.id.min(i64::MAX as u64) as i64,
                    updated_at: note.id.min(i64::MAX as u64) as i64,
                });
            }
        }
        notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok(notes)
    }

    pub fn get_note(&self, id: &str) -> Result<Option<Note>, String> {
        valid_id(id)?;
        Ok(self.list_notes()?.into_iter().find(|note| note.id == id))
    }

    pub fn create_note(&self, cwd: Option<String>) -> Result<Note, String> {
        self.upsert_note(&NoteUpsert {
            id: format!(
                "note-{}-{:016x}",
                crate::time::now_ms(),
                rand::random::<u64>()
            ),
            source_cwd: cwd.filter(|cwd| crate::notes::is_project(cwd)),
            ..Default::default()
        })
    }

    pub fn upsert_note(&self, note: &NoteUpsert) -> Result<Note, String> {
        valid_id(&note.id)?;
        if note.body.len() > MAX_BODY {
            return Err("Content is too large".into());
        }
        if let Some(id) = note
            .source_session_id
            .as_deref()
            .filter(|id| !id.is_empty())
        {
            valid_id(id)?;
        }
        if self.shared.is_some() {
            self.shared_writable()?;
        }
        ensure_notes_table(&self.conn).map_err(|e| e.to_string())?;
        upsert_note(&self.conn, note).map_err(|e| e.to_string())
    }

    pub fn delete_note(&self, id: &str) -> Result<(), String> {
        valid_id(id)?;
        if self.shared.is_some() {
            self.shared_writable()?;
        }
        if super::shared::has_table(&self.conn, "notes")? {
            ensure_notes_table(&self.conn).map_err(|e| e.to_string())?;
            delete_note(&self.conn, id).map_err(|e| e.to_string())?;
        }
        if self.shared.is_none() {
            if let Some(id) = id.strip_prefix("gpui-note-") {
                self.conn
                    .execute(
                        "DELETE FROM native_settings WHERE key=?1",
                        [format!("note:{id}")],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        // A deleted row stays deleted even if best-effort asset cleanup fails.
        if let Some(root) = self.data_dir() {
            let _ = std::fs::remove_dir_all(root.join("note-assets").join(id));
        }
        Ok(())
    }

    pub fn note_source_session(&self, note: &Note) -> Option<i64> {
        note.source_session_id
            .as_deref()
            .and_then(|id| self.resolve_existing_session_id(id))
    }

    /// Resolves @note/slug mentions in the same order as the desktop composer.
    pub fn apply_notes_to_turn(&self, text: &str) -> Result<String, String> {
        let slugs = crate::notes::slugs_in_text(text);
        if slugs.is_empty() {
            return Ok(text.into());
        }
        let notes = self.list_notes()?;
        let picked = slugs
            .iter()
            .filter_map(|slug| notes.iter().find(|note| &note.slug == slug).cloned())
            .collect::<Vec<_>>();
        Ok(crate::notes::inject_prompt(text, &picked))
    }

    pub(super) fn save_response_note(&self, note: &crate::response::Note) -> Result<(), String> {
        if note.body.len() > MAX_BODY {
            return Err("Content is too large".into());
        }
        let existing = self.list_notes()?.into_iter().find(|saved| {
            saved.id == format!("gpui-note-{}", note.id)
                || super::shared::handle(&saved.id) as u64 == note.id
        });
        let update = NoteUpsert {
            id: existing
                .as_ref()
                .map(|note| note.id.clone())
                .unwrap_or_else(|| format!("gpui-note-{}", note.id)),
            title: note.title.clone(),
            body: note.body.clone(),
            tags: note.tags.clone(),
            source_session_id: (note.source_session != 0)
                .then(|| self.persisted_session_id(note.source_session).ok())
                .flatten(),
            source_cwd: (!note.source_cwd.is_empty()).then(|| note.source_cwd.clone()),
            finalize_slug: true,
        };
        // The legacy facade also edits older note rows carrying an artifact
        // subtype. Preserve it instead of reclassifying or duplicating the row.
        if existing.is_some() && super::shared::has_table(&self.conn, "notes")? {
            if self.shared.is_some() {
                self.shared_writable()?;
            }
            ensure_notes_table(&self.conn).map_err(|e| e.to_string())?;
            let subtype: Option<String> = self
                .conn
                .query_row(
                    "SELECT artifact_kind FROM notes WHERE id=?1",
                    [&update.id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .flatten();
            upsert_content(&self.conn, &update, "note", subtype.as_deref())
                .map_err(|e| e.to_string())?;
        } else {
            self.upsert_note(&update)?;
        }
        Ok(())
    }

    pub fn save_note_image(&self, note_id: &str, source: &Path) -> Result<NoteImageAsset, String> {
        valid_id(note_id)?;
        if self.shared.is_some() {
            self.shared_writable()?;
        }
        if self.get_note(note_id)?.is_none() {
            return Err("Note no longer exists".into());
        }
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let meta = source.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err("Drop an image file.".into());
        }
        if meta.len() > 20 * 1024 * 1024 {
            return Err("Image is too large (maximum 20 MB).".into());
        }
        let extension = source
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !["png", "jpg", "jpeg", "gif", "webp", "svg"].contains(&extension.as_str()) {
            return Err("Drop a PNG, JPG, GIF, WebP, or SVG image.".into());
        }
        let name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let stem = source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '-'
                }
            })
            .take(80)
            .collect::<String>()
            .trim_matches('-')
            .to_owned();
        let stem = if stem.is_empty() { "image" } else { &stem };
        let filename = format!(
            "{}-{:016x}-{stem}.{extension}",
            crate::time::now_ms(),
            rand::random::<u64>()
        );
        let root = self.data_dir().ok_or("Note image storage is unavailable")?;
        let directory = root.join("note-assets").join(note_id);
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        std::fs::copy(source, directory.join(&filename)).map_err(|e| e.to_string())?;
        Ok(NoteImageAsset {
            name,
            markdown_path: format!("/note-assets/{note_id}/{filename}"),
        })
    }

    pub fn note_image_path(&self, asset: &str) -> Result<PathBuf, String> {
        let path = Path::new(asset.strip_prefix('/').ok_or("Invalid note image path")?);
        let parts = path
            .components()
            .map(|part| match part {
                Component::Normal(value) => value.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("Invalid note image path")?;
        if parts.len() != 3 || parts[0] != "note-assets" {
            return Err("Invalid note image path".into());
        }
        valid_id(parts[1])?;
        if !parts[2]
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.'))
        {
            return Err("Invalid note image path".into());
        }
        let path = self
            .data_dir()
            .ok_or("Note image storage is unavailable")?
            .join(path);
        if !path.is_file() {
            return Err("Note image was not found".into());
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open(Path::new(":memory:")).unwrap()
    }
    fn input(id: &str, title: &str, body: &str) -> NoteUpsert {
        NoteUpsert {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            ..Default::default()
        }
    }

    #[test]
    fn desktop_note_crud_keeps_source_tags_and_project_moves() {
        let store = store();
        let session = store.create("codex", "/project").unwrap();
        let mut request = input("saved-note", "  A title  ", "# Heading\r\n\r\nBody\rtext");
        request.tags = vec![
            " #Design Notes ".into(),
            "design-notes".into(),
            "Rust".into(),
        ];
        request.source_session_id = Some(session.id.to_string());
        request.source_cwd = Some("/project".into());
        let saved = store.upsert_note(&request).unwrap();
        assert_eq!(saved.title, "A title");
        assert_eq!(saved.body, "# Heading\n\nBody\ntext");
        assert_eq!(saved.tags, ["design-notes", "rust"]);
        assert_eq!(store.note_source_session(&saved), Some(session.id));
        store
            .conn
            .execute("UPDATE notes SET updated_at=42 WHERE id=?1", [&saved.id])
            .unwrap();
        let saved = store.get_note(&saved.id).unwrap().unwrap();
        let mut update = saved.upsert(false);
        update.source_cwd = Some("/different".into());
        update.source_session_id = Some("other-source".into());
        let moved = store.upsert_note(&update).unwrap();
        assert_eq!(moved.source_cwd.as_deref(), Some("/different"));
        assert_eq!(moved.updated_at, 42);
        assert_eq!(moved.source_session_id, saved.source_session_id);
        update.source_cwd = None;
        assert_eq!(
            store.upsert_note(&update).unwrap().source_cwd,
            moved.source_cwd
        );
        update.source_cwd = Some("~".into());
        assert!(store.upsert_note(&update).unwrap().project_name().is_none());
        store.delete_note(&saved.id).unwrap();
        assert!(store.list_notes().unwrap().is_empty());
    }

    #[test]
    fn pending_slugs_finalize_once_and_do_not_steal_existing_references() {
        let store = store();
        let established = store
            .upsert_note(&input("established", "Real title", "Body"))
            .unwrap();
        let mut pending = store.upsert_note(&input("pending", "", "")).unwrap();
        assert!(pending.slug_pending);
        pending.title = "Real title".into();
        let typed = store.upsert_note(&pending.upsert(false)).unwrap();
        assert_eq!(typed.slug, pending.slug);
        assert!(typed.slug_pending);
        let finalized = store.upsert_note(&typed.upsert(true)).unwrap();
        assert_eq!(finalized.slug, "real-title-2");
        assert!(!finalized.slug_pending);
        assert_eq!(
            store.get_note(&established.id).unwrap().unwrap().slug,
            "real-title"
        );
        let mut rename = finalized.upsert(true);
        rename.title = "Different title".into();
        assert_eq!(store.upsert_note(&rename).unwrap().slug, finalized.slug);
        let mut placeholder = store
            .upsert_note(&input("placeholder", "", ""))
            .unwrap()
            .upsert(true);
        placeholder.title = "Untitled 2".into();
        assert!(store.upsert_note(&placeholder).unwrap().slug_pending);
    }

    #[test]
    fn artifacts_are_excluded_and_cannot_be_overwritten_by_notes() {
        let store = store();
        ensure_notes_table(&store.conn).unwrap();
        let artifact = input("artifact", "Document", "Artifact text");
        upsert_content(&store.conn, &artifact, "artifact", Some("document")).unwrap();
        let note = store
            .upsert_note(&input("note", "Note", "Note text"))
            .unwrap();
        assert_eq!(store.list_notes().unwrap(), [note]);
        assert!(store.get_note("artifact").unwrap().is_none());
        assert!(store.upsert_note(&artifact).is_err());
        store.delete_note("artifact").unwrap();
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM notes WHERE id='artifact'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn note_mentions_expand_in_reference_order_once_and_ignore_missing_slugs() {
        let store = store();
        let a = store.upsert_note(&input("alpha", "Alpha", "A")).unwrap();
        let b = store.upsert_note(&input("beta", "Beta", "B")).unwrap();
        let prompt = "Use @note/beta and @note/alpha and @note/beta @note/missing";
        assert_eq!(
            store.apply_notes_to_turn(prompt).unwrap(),
            crate::notes::inject_prompt(prompt, &[b, a])
        );
        assert_eq!(
            store.apply_notes_to_turn("@note/missing").unwrap(),
            "@note/missing"
        );
    }

    #[test]
    fn response_note_facade_and_legacy_settings_share_one_saved_identity() {
        let store = store();
        let session = store.create("codex", "/project").unwrap();
        let mut response = crate::response::Note {
            id: 123,
            title: "Response".into(),
            body: "Saved response".into(),
            tags: vec!["tag".into()],
            source_session: session.id,
            source_cwd: "/project".into(),
        };
        store
            .conn
            .execute(
                "INSERT INTO native_settings(key,value) VALUES('note:123',?1)",
                [serde_json::to_string(&response).unwrap()],
            )
            .unwrap();
        assert_eq!(store.list_notes().unwrap().len(), 1);
        store.save_note(&response).unwrap();
        response.body = "Edited response".into();
        store.save_note(&response).unwrap();
        assert_eq!(store.list_notes().unwrap().len(), 1);
        let compatible = store.notes().unwrap();
        assert_eq!(compatible.len(), 1);
        assert_eq!(compatible[0].id, response.id);
        assert_eq!(compatible[0].body, response.body);
        assert_eq!(compatible[0].source_session, session.id);
        store.delete_note("gpui-note-123").unwrap();
        assert!(store.notes().unwrap().is_empty());
    }

    #[test]
    fn shared_response_bridge_preserves_original_identity_slug_and_subtype() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_monocode(&directory.path().join("monocode.db")).unwrap();
        store.conn.execute("INSERT INTO notes(id,slug,title,body,created_at,updated_at,artifact_kind) VALUES('source-id','stable','Saved','Body',1,2,'document')", []).unwrap();
        let mut compatible = store.notes().unwrap().pop().unwrap();
        compatible.body = "Changed".into();
        store.save_note(&compatible).unwrap();
        let saved = store.get_note("source-id").unwrap().unwrap();
        assert_eq!(saved.slug, "stable");
        assert_eq!(saved.body, "Changed");
        assert_eq!(store.list_notes().unwrap().len(), 1);
        let subtype: String = store
            .conn
            .query_row(
                "SELECT artifact_kind FROM notes WHERE id='source-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(subtype, "document");
    }
}
