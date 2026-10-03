//! Port of src/features/notes/notes.ts: note titles, tags, previews, `@note`
//! mentions, and the prompt text a note adds to a turn.
//!
//! The module-level cache and the store calls (`loadNotes`, `upsertNote`,
//! `deleteNote`, `createNote`) live in the `Notes` entity. The two window
//! events become `NotesEvent`.

use std::collections::HashSet;
use std::sync::LazyLock;

use monocode_core::js;
use monocode_layout::paths::project_name;
use regex::Regex;

pub use monocode_core::notes::{NoteCardMeta, NoteComposerCard, note_card_meta};
pub use monocode_store::notes::{Note, NoteImageAsset, NoteUpsert};

use super::app_search::FileRank;
use super::paths::looks_like_project;
use crate::runtime::util::fuzzy::fuzzy_match;

/// `NOTE_MENTION_PREFIX`.
pub const NOTE_MENTION_PREFIX: &str = "note/";
/// `NOTE_PATH_PREFIX`.
pub const NOTE_PATH_PREFIX: &str = "note:";
/// `MAX_TITLE`.
pub const MAX_NOTE_TITLE: usize = 200;
/// `MAX_NOTE_TAGS`.
pub const MAX_NOTE_TAGS: usize = 20;
/// `MAX_NOTE_TAG_LENGTH`.
pub const MAX_NOTE_TAG_LENGTH: usize = 48;
/// `MAX_NOTE_PICKER`.
pub const MAX_NOTE_PICKER: usize = 8;

/// `/(^|\s)@note\/([A-Za-z0-9_-]+)/g`.
static NOTE_SLUG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)@note/([A-Za-z0-9_-]+)").expect("note slug regex"));
/// `/^\s{0,3}#{1,6}\s+(.+)$/m`. JavaScript's `.` stops at every line
/// terminator, so the capture excludes them all.
static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s{0,3}#{1,6}\s+([^\n\r\x{2028}\x{2029}]+)").expect("heading regex")
});
static HEADING_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s{0,3}#{1,6}\s+").expect("heading line regex"));
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*```").expect("fence regex"));
static IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*]\([^)]*\)").expect("image regex"));
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)]\([^)]*\)").expect("link regex"));
static EMPHASIS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[*_`]+").expect("emphasis regex"));
static BULLET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[-*+]\s+").expect("bullet regex"));

/// The input to `createNote`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewNote {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source_session_id: Option<String>,
    pub source_cwd: Option<String>,
}

/// The upsert `createNote` sends for a new note id.
pub fn create_note_upsert(id: String, input: &NewNote) -> NoteUpsert {
    let body = normalize_newlines(input.body.as_deref().unwrap_or(""));
    let title = match input.title.as_deref() {
        Some(title) => title.to_string(),
        None => note_title(&body),
    };
    NoteUpsert {
        id,
        title: js::slice_prefix(&title, MAX_NOTE_TITLE).to_string(),
        body,
        tags: normalize_note_tags(input.tags.as_deref().unwrap_or(&[])),
        source_session_id: input.source_session_id.clone().filter(|id| !id.is_empty()),
        source_cwd: input.source_cwd.clone().filter(|cwd| !cwd.is_empty()),
    }
}

/// `normalizeNoteTags`: canonical, case-insensitive tags for storage and
/// filtering.
pub fn normalize_note_tags(tags: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for input in tags {
        let trimmed = js::trim(input).trim_start_matches('#');
        let dashed = collapse_space_to_dash(trimmed).to_lowercase();
        let sliced = js::slice_prefix(&dashed, MAX_NOTE_TAG_LENGTH);
        let tag = sliced.trim_end_matches('-').to_string();
        if tag.is_empty() || !seen.insert(tag.clone()) {
            continue;
        }
        normalized.push(tag);
        if normalized.len() == MAX_NOTE_TAGS {
            break;
        }
    }
    normalized
}

fn collapse_space_to_dash(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if js::is_space(c) {
            if !in_space {
                out.push('-');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `isNoteMentionPath`.
pub fn is_note_mention_path(path: &str) -> bool {
    path.starts_with(NOTE_PATH_PREFIX)
}

/// `noteMentionLabel`.
pub fn note_mention_label(note: &Note) -> String {
    format!("{NOTE_MENTION_PREFIX}{}", note.slug)
}

/// A note as an entry of the `@` mention picker (`ProjectFile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteFile {
    pub name: String,
    pub path: String,
    pub relative: String,
}

/// `notesAsProjectFiles`.
pub fn notes_as_project_files(notes: &[Note]) -> Vec<NoteFile> {
    notes.iter().map(note_file).collect()
}

fn note_file(note: &Note) -> NoteFile {
    NoteFile {
        name: note.title.clone(),
        path: format!("{NOTE_PATH_PREFIX}{}", note.id),
        relative: note_mention_label(note),
    }
}

fn ranked(note: &Note, score: i64, positions: Vec<usize>) -> FileRank {
    let file = note_file(note);
    FileRank {
        path: file.path,
        relative: file.relative,
        name: file.name,
        score,
        positions,
    }
}

/// `rankNoteFiles` with the default limit.
pub fn rank_note_files(notes: &[Note], query: &str) -> Vec<FileRank> {
    rank_note_files_limit(notes, query, MAX_NOTE_PICKER)
}

/// `rankNoteFiles`: newest first without a query; with one, title matches
/// rank first, then tag matches, then slug matches.
pub fn rank_note_files_limit(notes: &[Note], query: &str, limit: usize) -> Vec<FileRank> {
    let needle = js::trim(query.trim_end_matches('/')).to_lowercase();
    if needle.is_empty() {
        let mut sorted: Vec<&Note> = notes.iter().collect();
        sorted.sort_by_key(|note| std::cmp::Reverse(note.updated_at));
        return sorted
            .into_iter()
            .take(limit)
            .map(|note| ranked(note, 0, Vec::new()))
            .collect();
    }
    let tag_needle = needle.strip_prefix('#').unwrap_or(&needle);
    let mut scored: Vec<(&Note, i64, Vec<usize>)> = Vec::new();
    for note in notes {
        let title_hit = fuzzy_match(&needle, &note.title);
        let slug_hit = if title_hit.is_some() {
            None
        } else {
            fuzzy_match(&needle, &note.slug)
        };
        let tag_hit = if title_hit.is_some() || slug_hit.is_some() {
            None
        } else {
            note.tags
                .iter()
                .find_map(|tag| fuzzy_match(tag_needle, tag))
        };
        let (from_title, from_tag) = (title_hit.is_some(), tag_hit.is_some());
        let Some(hit) = title_hit.or(slug_hit).or(tag_hit) else {
            continue;
        };
        let score = if from_title {
            hit.score + 400
        } else if from_tag {
            hit.score + 200
        } else {
            hit.score
        };
        scored.push((
            note,
            score,
            if from_title {
                hit.positions
            } else {
                Vec::new()
            },
        ));
    }
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.updated_at.cmp(&a.0.updated_at))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(note, score, positions)| ranked(note, score, positions))
        .collect()
}

/// `noteTitle`: the first markdown heading, or the first non-empty prose line.
pub fn note_title(text: &str) -> String {
    if let Some(heading) = HEADING_RE.captures(text).and_then(|found| found.get(1)) {
        let title = unwrap_markdown(heading.as_str());
        let title = js::slice_prefix(&title, MAX_NOTE_TITLE);
        if !title.is_empty() {
            return title.to_string();
        }
    }
    let mut in_fence = false;
    for line in split_lines(text) {
        if FENCE_RE.is_match(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = js::trim(line);
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        let title = unwrap_markdown(trimmed);
        let title = js::slice_prefix(&title, MAX_NOTE_TITLE);
        if !title.is_empty() {
            return title.to_string();
        }
    }
    "Untitled".into()
}

/// `noteSourceProject`: the folder name for a note saved from a session, or
/// `None` when there is no project.
pub fn note_source_project(cwd: Option<&str>) -> Option<String> {
    let cwd = cwd.filter(|cwd| looks_like_project(cwd))?;
    let name = project_name(cwd);
    (!name.is_empty()).then_some(name)
}

/// `notePreview`: up to 120 characters of prose after the title.
pub fn note_preview(text: &str, title: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in split_lines(text) {
        if FENCE_RE.is_match(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || HEADING_LINE_RE.is_match(line) {
            continue;
        }
        let trimmed = js::trim(line);
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        let next = unwrap_markdown(&BULLET_RE.replace(trimmed, ""));
        if next.is_empty() || next == title {
            continue;
        }
        parts.push(next);
        if js::len(&parts.join(" ")) >= 120 {
            break;
        }
    }
    let preview = parts.join(" ");
    if preview.is_empty() {
        return String::new();
    }
    if js::len(&preview) > 120 {
        format!("{}…", js::slice_prefix(&preview, 119))
    } else {
        preview
    }
}

/// `appendNoteReference`: add a note to a draft as plain text.
pub fn append_note_reference(draft: &str, title: &str, body: &str) -> String {
    let content = normalize_newlines(body);
    let content = js::trim(&content);
    if content.is_empty() {
        return draft.to_string();
    }
    let heading = js::trim(title);
    let heading = if heading.is_empty() {
        "Untitled"
    } else {
        heading
    };
    let block = format!("Note: {heading}\n\n{content}");
    let separator = if draft.is_empty() || draft.ends_with("\n\n") {
        ""
    } else if draft.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{draft}{separator}{block}\n\n")
}

/// `noteSlugsInText`: unique `@note/slug` tokens in order.
pub fn note_slugs_in_text(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for captures in NOTE_SLUG_RE.captures_iter(text) {
        let Some(slug) = captures.get(1).map(|m| m.as_str()) else {
            continue;
        };
        if slug.is_empty() || !seen.insert(slug.to_string()) {
            continue;
        }
        found.push(slug.to_string());
    }
    found
}

/// `injectNotePrompt`: the referenced note bodies after the prompt.
pub fn inject_note_prompt(text: &str, notes: &[(&str, &str)]) -> String {
    if notes.is_empty() {
        return text.to_string();
    }
    let mut lines = vec![
        js::trim_end(text).to_string(),
        String::new(),
        "---".to_string(),
    ];
    for (title, body) in notes {
        let heading = js::trim(title);
        let heading = if heading.is_empty() {
            "Untitled"
        } else {
            heading
        };
        lines.push(
            [
                format!("Referenced note \"{heading}\":"),
                String::new(),
                js::trim(body).to_string(),
            ]
            .join("\n"),
        );
    }
    lines.join("\n")
}

/// The pure part of `applyNotesToTurn`: inject every note a `@note/slug`
/// mentions, each once.
pub fn apply_notes_to_text(text: &str, notes: &[Note]) -> String {
    let slugs = note_slugs_in_text(text);
    if slugs.is_empty() {
        return text.to_string();
    }
    let mut picked: Vec<&Note> = Vec::new();
    let mut seen = HashSet::new();
    for slug in &slugs {
        let Some(note) = notes.iter().find(|note| note.slug == *slug) else {
            continue;
        };
        if !seen.insert(note.id.as_str()) {
            continue;
        }
        picked.push(note);
    }
    if picked.is_empty() {
        return text.to_string();
    }
    let pairs: Vec<(&str, &str)> = picked
        .iter()
        .map(|note| (note.title.as_str(), note.body.as_str()))
        .collect();
    inject_note_prompt(text, &pairs)
}

/// `noteComposerCard`.
pub fn note_composer_card(note: &Note) -> NoteComposerCard {
    NoteComposerCard {
        id: note.id.clone(),
        slug: note.slug.clone(),
        title: note.title.clone(),
        source_cwd: note.source_cwd.clone().filter(|cwd| !cwd.is_empty()),
        body: note.body.clone(),
    }
}

/// The check in `requestAddNoteToChat`: a note with no text adds nothing.
pub fn can_add_note_to_chat(note: &Note) -> bool {
    !js::trim(&normalize_newlines(&note.body)).is_empty()
}

/// `composeNoteMessage`: the message a note chip sends.
pub fn compose_note_message(card: Option<&NoteComposerCard>, text: &str) -> String {
    let Some(card) = card else {
        return js::trim(text).to_string();
    };
    let trimmed = js::trim(text);
    let lead = if trimmed.is_empty() {
        "Use this note."
    } else {
        trimmed
    };
    inject_note_prompt(lead, &[(card.title.as_str(), card.body.as_str())])
}

/// `body.replace(/\r\n?/g, "\n")`.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `text.split(/\r?\n/)`.
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// `unwrapMarkdown`: drop images, keep link text, drop emphasis marks.
fn unwrap_markdown(value: &str) -> String {
    let value = IMAGE_RE.replace_all(value, "");
    let value = LINK_RE.replace_all(&value, "$1");
    let value = EMPHASIS_RE.replace_all(&value, "");
    js::trim(&value).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn note(id: &str, slug: &str, title: &str) -> Note {
        Note {
            id: id.into(),
            slug: slug.into(),
            title: title.into(),
            body: String::new(),
            tags: Vec::new(),
            source_session_id: None,
            source_cwd: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn uses_the_first_heading() {
        assert_eq!(
            note_title("intro\n# Auth approach\n\nbody"),
            "Auth approach"
        );
    }

    #[test]
    fn falls_back_to_the_first_prose_line() {
        assert_eq!(
            note_title("Ship the notes overlay first."),
            "Ship the notes overlay first."
        );
    }

    #[test]
    fn returns_untitled_when_empty() {
        assert_eq!(note_title("   \n```\ncode\n```\n"), "Untitled");
    }

    #[test]
    fn unwraps_links_and_emphasis_in_titles() {
        assert_eq!(
            note_title("## **Read** [the docs](https://x)"),
            "Read the docs"
        );
        assert_eq!(note_title("![img](a.png)\n\nNext line"), "Next line");
    }

    #[test]
    fn normalizes_deduplicates_and_drops_empty_tags() {
        assert_eq!(
            normalize_note_tags(&strings(&[" Ideas ", "#Project Docs", "ideas", "###"])),
            strings(&["ideas", "project-docs"])
        );
    }

    #[test]
    fn caps_tag_length_and_count() {
        let long = "a".repeat(60);
        assert_eq!(normalize_note_tags(&[long])[0].len(), MAX_NOTE_TAG_LENGTH);
        let many: Vec<String> = (0..30).map(|n| format!("t{n}")).collect();
        assert_eq!(normalize_note_tags(&many).len(), MAX_NOTE_TAGS);
        assert_eq!(normalize_note_tags(&strings(&["a  -  "])), strings(&["a"]));
    }

    #[test]
    fn skips_the_title_heading_in_the_preview() {
        assert_eq!(
            note_preview("# Auth\n\nKeep it global.", "Auth"),
            "Keep it global."
        );
    }

    #[test]
    fn cuts_a_long_preview() {
        let preview = note_preview(&"word ".repeat(40), "");
        assert_eq!(js::len(&preview), 120);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn uses_the_folder_name_for_the_source_project() {
        assert_eq!(
            note_source_project(Some("/Users/me/code/agent-terminal")).as_deref(),
            Some("agent-terminal")
        );
    }

    #[test]
    fn has_no_source_project_without_a_project() {
        assert_eq!(note_source_project(None), None);
        assert_eq!(note_source_project(Some("~")), None);
        assert_eq!(note_source_project(Some("/")), None);
    }

    #[test]
    fn collects_unique_note_slug_tokens() {
        assert_eq!(
            note_slugs_in_text("See @note/auth and @note/auth plus @note/plan-2."),
            strings(&["auth", "plan-2"])
        );
        assert!(note_slugs_in_text("mail@note/x").is_empty());
    }

    #[test]
    fn injects_referenced_bodies_after_the_prompt() {
        assert_eq!(
            inject_note_prompt("Use this.", &[("Auth", "Use a cookie.")]),
            "Use this.\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
    }

    #[test]
    fn applies_mentioned_notes_once() {
        let mut auth = note("n1", "auth", "Auth");
        auth.body = "Use a cookie.".into();
        assert_eq!(
            apply_notes_to_text("Use @note/auth and @note/auth @note/missing", &[auth]),
            "Use @note/auth and @note/auth @note/missing\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
        assert_eq!(apply_notes_to_text("no mentions", &[]), "no mentions");
    }

    #[test]
    fn maps_notes_to_mention_files() {
        let files = notes_as_project_files(&[note("abc", "auth", "Auth")]);
        assert_eq!(
            files[0],
            NoteFile {
                name: "Auth".into(),
                path: "note:abc".into(),
                relative: "note/auth".into(),
            }
        );
        assert!(is_note_mention_path(&files[0].path));
        assert_eq!(
            note_mention_label(&note("abc", "auth", "Auth")),
            "note/auth"
        );
    }

    fn card() -> NoteComposerCard {
        NoteComposerCard {
            id: "n1".into(),
            slug: "auth".into(),
            title: "Auth".into(),
            source_cwd: None,
            body: "Use a cookie.".into(),
        }
    }

    #[test]
    fn sends_a_lead_in_plus_the_note_when_the_textarea_is_empty() {
        assert_eq!(
            compose_note_message(Some(&card()), "  "),
            "Use this note.\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
    }

    #[test]
    fn keeps_the_users_message_and_appends_the_note() {
        assert_eq!(
            compose_note_message(Some(&card()), "Start with the cookie."),
            "Start with the cookie.\n\n---\nReferenced note \"Auth\":\n\nUse a cookie."
        );
    }

    #[test]
    fn passes_the_draft_through_when_there_is_no_card() {
        assert_eq!(compose_note_message(None, " hello "), "hello");
    }

    #[test]
    fn drops_the_body_so_the_thread_chip_can_persist_without_the_prompt_dump() {
        let meta = note_card_meta(&NoteComposerCard {
            id: "n1".into(),
            slug: "overview".into(),
            title: "Overview".into(),
            source_cwd: Some("/tmp/project".into()),
            body: "long secret body".into(),
        });
        assert_eq!(
            serde_json::to_value(&meta).unwrap(),
            serde_json::json!({
                "id": "n1",
                "slug": "overview",
                "title": "Overview",
                "sourceCwd": "/tmp/project",
            })
        );
    }

    #[test]
    fn separates_from_existing_draft_text() {
        assert_eq!(
            append_note_reference("hello", "Auth", "Use a cookie."),
            "hello\n\nNote: Auth\n\nUse a cookie.\n\n"
        );
        assert_eq!(
            append_note_reference("", "Auth", "Use a cookie."),
            "Note: Auth\n\nUse a cookie.\n\n"
        );
        assert_eq!(append_note_reference("keep", "Auth", " \r\n "), "keep");
    }

    fn ranked_paths(files: &[FileRank]) -> Vec<&str> {
        files.iter().map(|file| file.path.as_str()).collect()
    }

    fn rank_fixture() -> Vec<Note> {
        vec![
            Note {
                updated_at: 2,
                ..note("a", "alpha", "Alpha")
            },
            Note {
                updated_at: 3,
                ..note("b", "beta", "Beta plan")
            },
        ]
    }

    #[test]
    fn keeps_recency_order_without_a_query() {
        assert_eq!(
            ranked_paths(&rank_note_files(&rank_fixture(), "")),
            vec!["note:b", "note:a"]
        );
    }

    #[test]
    fn fuzzy_matches_titles() {
        assert_eq!(
            ranked_paths(&rank_note_files(&rank_fixture(), "plan")),
            vec!["note:b"]
        );
    }

    #[test]
    fn fuzzy_matches_tags() {
        let tagged = vec![Note {
            tags: strings(&["important-links"]),
            ..note("a", "reference", "Reference")
        }];
        assert_eq!(
            ranked_paths(&rank_note_files(&tagged, "important")),
            vec!["note:a"]
        );
    }

    #[test]
    fn prefixes_the_slug() {
        assert_eq!(note_mention_label(&note("n", "auth", "Auth")), "note/auth");
    }

    #[test]
    fn builds_the_create_upsert() {
        let upsert = create_note_upsert(
            "id-1".into(),
            &NewNote {
                body: Some("# Plan\r\nbody".into()),
                tags: Some(strings(&["Ideas"])),
                source_cwd: Some("/tmp/p".into()),
                ..Default::default()
            },
        );
        assert_eq!(upsert.title, "Plan");
        assert_eq!(upsert.body, "# Plan\nbody");
        assert_eq!(upsert.tags, strings(&["ideas"]));
        assert_eq!(upsert.source_cwd.as_deref(), Some("/tmp/p"));
        assert_eq!(upsert.source_session_id, None);
    }
}
