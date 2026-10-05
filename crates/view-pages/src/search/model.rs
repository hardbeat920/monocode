//! The search hits the page shows. The shapes mirror
//! `monocode_engine::history::app_search` (`AppSearchHit` and its five
//! kinds, `SearchScope`), which ports src/features/search/model/appSearch.ts.
//! The display helpers (`Highlight`, `namePositions`, `rowCopy` meta) are
//! ports from SearchView.tsx.

use monocode_core::HarnessId;
use monocode_layout::paths::{pretty_cwd, project_name};
use serde::{Deserialize, Serialize};

/// `SearchScope`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    #[default]
    All,
    Conversations,
    Files,
    Projects,
}

impl SearchScope {
    /// `SCOPES`, in tab order, with their labels.
    pub const ALL_SCOPES: [(SearchScope, &'static str); 4] = [
        (SearchScope::All, "All"),
        (SearchScope::Conversations, "Conversations"),
        (SearchScope::Files, "Files"),
        (SearchScope::Projects, "Projects"),
    ];
}

/// `ConversationHit`: a title match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationHit {
    pub id: String,
    pub session_id: String,
    pub cwd: String,
    pub harness: HarnessId,
    pub title: String,
    pub updated_at: i64,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `MessageHit`: a transcript match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHit {
    pub id: String,
    pub session_id: String,
    pub cwd: String,
    pub harness: HarnessId,
    pub title: String,
    pub updated_at: i64,
    pub block_id: String,
    pub role: String,
    pub preview: String,
    pub score: i64,
}

/// `FileHit`: a file name match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHit {
    pub id: String,
    pub path: String,
    pub relative: String,
    pub name: String,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `ContentHit`: a line in a project file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentHit {
    pub id: String,
    pub path: String,
    pub relative: String,
    pub name: String,
    pub line: i64,
    pub column: i64,
    pub preview: String,
}

/// `ProjectHit`: a recent project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectHit {
    pub id: String,
    pub path: String,
    pub name: String,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `AppSearchHit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppSearchHit {
    Conversation(ConversationHit),
    Message(MessageHit),
    File(FileHit),
    Content(ContentHit),
    Project(ProjectHit),
}

impl AppSearchHit {
    pub fn id(&self) -> &str {
        match self {
            AppSearchHit::Conversation(hit) => &hit.id,
            AppSearchHit::Message(hit) => &hit.id,
            AppSearchHit::File(hit) => &hit.id,
            AppSearchHit::Content(hit) => &hit.id,
            AppSearchHit::Project(hit) => &hit.id,
        }
    }

    /// The text a row shows as its title, before highlighting.
    pub fn title(&self) -> &str {
        match self {
            AppSearchHit::Conversation(hit) => &hit.title,
            AppSearchHit::Message(hit) if !hit.preview.is_empty() => &hit.preview,
            AppSearchHit::Message(hit) => &hit.title,
            AppSearchHit::File(hit) => &hit.name,
            AppSearchHit::Content(hit) => &hit.preview,
            AppSearchHit::Project(hit) => &hit.name,
        }
    }

    /// The dim monospace text on the right of a row (`rowCopy` meta).
    pub fn meta(&self) -> String {
        match self {
            AppSearchHit::Conversation(hit) => project_name(&hit.cwd),
            AppSearchHit::Message(hit) => hit.title.clone(),
            AppSearchHit::File(hit) => hit.relative.clone(),
            AppSearchHit::Content(hit) => format!("{}:{}", hit.relative, hit.line),
            AppSearchHit::Project(hit) => pretty_cwd(&hit.path),
        }
    }
}

/// What the page reads from the search model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchState {
    pub query: String,
    pub scope: SearchScope,
    /// The highlighted hit's index.
    pub active: usize,
    /// The results in display order.
    pub hits: Vec<AppSearchHit>,
    pub loading: bool,
    pub error: Option<String>,
    /// Either search stopped at its result cap.
    pub truncated: bool,
}

/// `namePositions`: fuzzy positions in `relative` moved onto the file name.
pub fn name_positions(name: &str, relative: &str, positions: &[usize]) -> Vec<usize> {
    let name_len = name.encode_utf16().count();
    let offset = relative.encode_utf16().count().saturating_sub(name_len);
    positions
        .iter()
        .filter_map(|index| index.checked_sub(offset))
        .filter(|index| *index < name_len)
        .collect()
}

/// `Highlight`: the byte range of the first case-insensitive match of the
/// trimmed query.
pub fn highlight_range(text: &str, query: &str) -> Option<std::ops::Range<usize>> {
    let needle: Vec<char> = monocode_core::js::trim(query)
        .chars()
        .flat_map(char::to_lowercase)
        .collect();
    if needle.is_empty() {
        return None;
    }
    for (start, _) in text.char_indices() {
        let mut matched = 0;
        for (offset, ch) in text[start..].char_indices() {
            let lower: Vec<char> = ch.to_lowercase().collect();
            if needle.get(matched..matched + lower.len()) != Some(lower.as_slice()) {
                break;
            }
            matched += lower.len();
            if matched == needle.len() {
                return Some(start..start + offset + ch.len_utf8());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_positions_onto_the_name() {
        assert_eq!(
            name_positions("main.rs", "src/main.rs", &[0, 4, 5, 10]),
            vec![0, 1, 6]
        );
    }

    #[test]
    fn highlights_the_first_match() {
        assert_eq!(
            highlight_range("Fix the Parser bug", " parser "),
            Some(8..14)
        );
        assert_eq!(highlight_range("nothing here", "zz"), None);
        assert_eq!(highlight_range("text", "  "), None);
    }

    #[test]
    fn row_meta_follows_the_hit_kind() {
        let content = AppSearchHit::Content(ContentHit {
            id: "c".into(),
            path: "/r/src/a.rs".into(),
            relative: "src/a.rs".into(),
            name: "a.rs".into(),
            line: 12,
            column: 3,
            preview: "let x = 1;".into(),
        });
        assert_eq!(content.meta(), "src/a.rs:12");
        assert_eq!(content.title(), "let x = 1;");
    }
}
