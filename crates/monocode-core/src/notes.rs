//! Notes share their identities, slugs and storage format with the desktop app.
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

pub const MAX_TITLE: usize = 200;
pub const MAX_BODY: usize = 1_000_000;
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_LENGTH: usize = 48;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_cwd: Option<String>,
    #[serde(default)]
    pub slug_pending: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteUpsert {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub source_session_id: Option<String>,
    /// Omit on update to keep the saved project directory. `~` means no project.
    pub source_cwd: Option<String>,
    pub finalize_slug: bool,
}

impl Note {
    pub fn upsert(&self, finalize_slug: bool) -> NoteUpsert {
        NoteUpsert {
            id: self.id.clone(),
            title: self.title.clone(),
            body: self.body.clone(),
            tags: self.tags.clone(),
            source_session_id: self.source_session_id.clone(),
            source_cwd: self.source_cwd.clone(),
            finalize_slug,
        }
    }

    /// The same filter as the notes view's `visible` memo: `NotesView.tsx`.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || [&self.title, &self.body, &self.slug]
                .iter()
                .any(|value| value.to_lowercase().contains(&query))
            || self
                .tags
                .iter()
                .any(|tag| tag.contains(query.strip_prefix('#').unwrap_or(&query)))
            || self
                .project_name()
                .is_some_and(|name| name.to_lowercase().contains(&query))
    }

    pub fn project_name(&self) -> Option<String> {
        self.source_cwd
            .as_deref()
            .filter(|cwd| is_project(cwd))
            .map(crate::files::leaf_name)
    }

    pub fn in_project(&self, scope: Option<&str>) -> bool {
        let Some(scope) = scope else { return true };
        self.source_cwd
            .as_deref()
            .filter(|cwd| is_project(cwd))
            .is_none_or(|cwd| crate::files::path_key(cwd) == crate::files::path_key(scope))
    }

    pub fn preview(&self) -> String {
        let mut parts = Vec::new();
        let mut fence = false;
        for line in self.body.lines() {
            // `notePreview` matches its heading pattern against the raw line, so a
            // heading indented past three spaces is not a heading and stays in the
            // preview. Trimming first would hide it.
            let is_heading = heading(line).is_some();
            let line = line.trim();
            if line.starts_with("```") {
                fence = !fence;
                continue;
            }
            if fence || is_heading || line.is_empty() || line == "---" {
                continue;
            }
            let text = unwrap_markdown(
                line.strip_prefix("- ")
                    .or_else(|| line.strip_prefix("* "))
                    .or_else(|| line.strip_prefix("+ "))
                    .unwrap_or(line),
            );
            if text.is_empty() || text == self.title {
                continue;
            }
            parts.push(text);
            if parts.join(" ").chars().count() >= 120 {
                break;
            }
        }
        let text = parts.join(" ");
        if text.chars().count() > 120 {
            format!("{}…", text.chars().take(119).collect::<String>())
        } else {
            text
        }
    }
}

pub fn is_project(cwd: &str) -> bool {
    let path = crate::files::path_key(cwd);
    if path.trim().is_empty()
        || path == "~"
        || path == "/"
        || path.contains(".app/")
        || cwd.contains(".app\\")
    {
        return false;
    }
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let drive = parts.first().is_some_and(|part| {
        part.len() == 2 && part.as_bytes()[0].is_ascii_alphabetic() && part.ends_with(':')
    });
    if (drive && parts.len() == 1)
        || (parts.len() == 2 && matches!(parts[0], "Users" | "home"))
        || (drive && parts.len() == 3 && parts[1].eq_ignore_ascii_case("Users"))
    {
        return false;
    }
    crate::accounts::home()
        .is_none_or(|home| crate::files::path_key(&home.to_string_lossy()) != path)
}

pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for input in tags {
        let tag = input
            .trim()
            .trim_start_matches('#')
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .to_lowercase()
            .chars()
            .take(MAX_TAG_LENGTH)
            .collect::<String>()
            .trim_end_matches('-')
            .to_owned();
        if !tag.is_empty() && !result.contains(&tag) {
            result.push(tag);
        }
        if result.len() == MAX_TAGS {
            break;
        }
    }
    result
}

pub fn title(text: &str) -> String {
    for line in text.lines() {
        if let Some(heading) = heading(line) {
            let title = unwrap_markdown(heading);
            if !title.is_empty() {
                return title.chars().take(MAX_TITLE).collect();
            }
        }
    }
    let mut fence = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence || line.is_empty() || line == "---" {
            continue;
        }
        let title = unwrap_markdown(line);
        if !title.is_empty() {
            return title.chars().take(MAX_TITLE).collect();
        }
    }
    "Untitled".into()
}

fn heading(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let text = &line[indent..];
    let count = text.bytes().take_while(|c| *c == b'#').count();
    if (1..=6).contains(&count) && text[count..].starts_with(char::is_whitespace) {
        Some(text[count..].trim())
    } else {
        None
    }
}

fn unwrap_markdown(text: &str) -> String {
    static IMAGE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap());
    static LINK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap());
    LINK.replace_all(&IMAGE.replace_all(text, ""), "$1")
        .chars()
        .filter(|ch| !matches!(ch, '*' | '_' | '`'))
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The same prompt boundary as `injectNotePrompt` in the desktop composer.
pub fn inject_prompt(text: &str, notes: &[Note]) -> String {
    if notes.is_empty() {
        return text.into();
    }
    let blocks = notes
        .iter()
        .map(|note| {
            format!(
                "Referenced note \"{}\":\n\n{}",
                if note.title.trim().is_empty() {
                    "Untitled"
                } else {
                    note.title.trim()
                },
                note.body.trim()
            )
        })
        .collect::<Vec<_>>();
    format!("{}\n\n---\n{}", text.trim_end(), blocks.join("\n"))
}

pub fn compose_message(note: &Note, text: &str) -> String {
    inject_prompt(
        if text.trim().is_empty() {
            "Use this note."
        } else {
            text.trim()
        },
        std::slice::from_ref(note),
    )
}

pub fn slugs_in_text(text: &str) -> Vec<String> {
    static SLUG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:^|\s)@note/([A-Za-z0-9_-]+)").unwrap());
    let mut result = Vec::new();
    for capture in SLUG.captures_iter(text) {
        let slug = capture[1].to_owned();
        if !result.contains(&slug) {
            result.push(slug);
        }
    }
    result
}

#[derive(Clone, Debug)]
pub struct NoteImageAsset {
    pub name: String,
    pub markdown_path: String,
}

pub fn image_markdown(asset: &NoteImageAsset) -> String {
    let alt = single_line(&asset.name)
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    format!("![{alt}]({})", asset.markdown_path)
}

/// A run of line breaks becomes one space, matching `noteImageMarkdown`.
fn single_line(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut in_break = false;
    for ch in name.chars() {
        if matches!(ch, '\r' | '\n') {
            if !in_break {
                result.push(' ');
                in_break = true;
            }
            continue;
        }
        in_break = false;
        result.push(ch);
    }
    result
}
