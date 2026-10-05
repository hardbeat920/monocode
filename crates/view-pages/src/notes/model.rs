//! The note text helpers the page draws with: `notePreview`,
//! `noteSourceProject`, `noteTitle`, and `normalizeNoteTags` from
//! src/features/notes/notes.ts, plus `isAtxHeadingLine` from
//! src/features/files/model/markdownSource.ts.
//!
//! The engine's history/notes.rs has the same ports. The view keeps a copy
//! because it does not link the engine.

use std::collections::HashSet;
use std::sync::LazyLock;

use monocode_core::js;
use monocode_layout::paths::project_name;
use regex::Regex;

use crate::format::looks_like_project;

/// `MAX_TITLE`.
pub const MAX_NOTE_TITLE: usize = 200;
/// `MAX_NOTE_TAGS`.
pub const MAX_NOTE_TAGS: usize = 20;
/// `MAX_NOTE_TAG_LENGTH`.
pub const MAX_NOTE_TAG_LENGTH: usize = 48;

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s{0,3}#{1,6}\s+([^\n\r\x{2028}\x{2029}]+)").expect("heading regex")
});
static HEADING_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s{0,3}#{1,6}\s+").expect("heading line regex"));
static ATX_HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s{0,3}#{1,6}(?:\s|$)").expect("atx heading regex"));
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*```").expect("fence regex"));
static IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*]\([^)]*\)").expect("image regex"));
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)]\([^)]*\)").expect("link regex"));
static EMPHASIS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[*_`]+").expect("emphasis regex"));
static BULLET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[-*+]\s+").expect("bullet regex"));

/// `isAtxHeadingLine`: `#` through `######` at the start of a source line.
pub fn is_atx_heading_line(line: &str) -> bool {
    ATX_HEADING_RE.is_match(line)
}

/// `normalizeNoteTags`: canonical, case-insensitive tags.
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

/// `noteTitle`: the first markdown heading, or the first non-empty prose
/// line.
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

/// `noteSourceProject`: the folder name for a note saved from a session.
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

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn normalizes_tags() {
        assert_eq!(
            normalize_note_tags(&strings(&["#Ideas", " big  plan ", "ideas", "", "##"])),
            strings(&["ideas", "big-plan"])
        );
    }

    #[test]
    fn titles_and_previews_skip_markdown() {
        let body = "# The **plan**\n\n- First [step](https://x)\n```\ncode\n```\nMore text";
        assert_eq!(note_title(body), "The plan");
        assert_eq!(note_preview(body, "The plan"), "First step More text");
        assert_eq!(note_title(""), "Untitled");
        assert_eq!(
            note_source_project(Some("/work/app")).as_deref(),
            Some("app")
        );
        assert_eq!(note_source_project(Some("~")), None);
        assert!(is_atx_heading_line("## Heading"));
        assert!(is_atx_heading_line("#"));
        assert!(!is_atx_heading_line("#tag"));
    }
}
