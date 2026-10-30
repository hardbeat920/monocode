//! SearchView.tsx/appSearch.ts: fuzzy titles and paths, literal message/content
//! matches, stable identities, and the same per-scope ordering and limits.
pub mod project;
use crate::{files::ProjectFile, store::Session, transcript::Row};
use std::{
    collections::HashMap,
    ops::Range,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Conversations,
    Files,
    Projects,
}
impl Scope {
    pub const ALL: [(Self, &'static str); 4] = [
        (Self::All, "All"),
        (Self::Conversations, "Conversations"),
        (Self::Files, "Files"),
        (Self::Projects, "Projects"),
    ];
    pub fn sessions(self) -> bool {
        matches!(self, Self::All | Self::Conversations)
    }
    pub fn files(self) -> bool {
        matches!(self, Self::All | Self::Files)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Conversation(i64),
    Message {
        session: i64,
        row: usize,
    },
    File {
        path: String,
        location: Option<(u32, u32)>,
    },
    Project(String),
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub id: String,
    pub target: Target,
    pub title: String,
    pub meta: String,
    pub provider: String,
    pub highlights: Vec<Range<usize>>,
    pub score: i64,
    pub updated_at: u64,
}
impl Hit {
    fn group(&self) -> usize {
        match self.target {
            Target::Conversation(_) => 0,
            Target::Message { .. } => 1,
            Target::File { location: None, .. } => 2,
            Target::File {
                location: Some(_), ..
            } => 3,
            Target::Project(_) => 4,
        }
    }
}
pub fn grouped(lists: impl IntoIterator<Item = Hit>, scope: Scope) -> Vec<Hit> {
    let mut unique: HashMap<String, Hit> = HashMap::new();
    for hit in lists {
        if unique.get(&hit.id).is_none_or(|old| hit.score > old.score) {
            unique.insert(hit.id.clone(), hit);
        }
    }
    let mut groups: [Vec<Hit>; 5] = Default::default();
    for hit in unique.into_values() {
        groups[hit.group()].push(hit);
    }
    for (ix, group) in groups.iter_mut().enumerate() {
        group.sort_by(|a, b| {
            if ix == 3 {
                a.id.cmp(&b.id)
            } else {
                b.score
                    .cmp(&a.score)
                    .then(b.updated_at.cmp(&a.updated_at))
                    .then(a.meta.cmp(&b.meta))
                    .then(a.id.cmp(&b.id))
            }
        });
    }
    let limits = match scope {
        Scope::All => [8, 8, 10, 12, 6],
        Scope::Conversations => [24, 24, 0, 0, 0],
        Scope::Files => [0, 0, 40, 48, 0],
        Scope::Projects => [0, 0, 0, 0, 24],
    };
    groups
        .into_iter()
        .zip(limits)
        .flat_map(|(mut g, n)| {
            g.truncate(n);
            g
        })
        .collect()
}
pub fn recency(updated: u64) -> i64 {
    if updated == 0 {
        return 0;
    }
    let week = 7 * 24 * 60 * 60 * 1000;
    (24. * (1. - (crate::time::now_ms().saturating_sub(updated).min(week) as f64 / week as f64)))
        .round() as i64
}
pub fn project_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_owned()
}
fn pretty_path(path: &str) -> String {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .and_then(|home| {
            std::path::Path::new(path)
                .strip_prefix(home)
                .ok()
                .map(|relative| {
                    if relative.as_os_str().is_empty() {
                        "~".into()
                    } else {
                        format!("~/{}", relative.display())
                    }
                })
        })
        .unwrap_or_else(|| path.to_owned())
}
/// Port of shared/lib/fuzzy.ts. Positions are native UTF-8 byte ranges.
pub fn fuzzy(query: &str, text: &str) -> Option<(i64, Vec<Range<usize>>)> {
    fuzzy_prepared(&query_tokens(query), &lower_chars(text))
}
fn lower_chars(text: &str) -> Vec<(usize, char, char)> {
    text.char_indices()
        .flat_map(|(at, ch)| ch.to_lowercase().map(move |lower| (at, ch, lower)))
        .collect()
}
fn query_tokens(query: &str) -> Vec<Vec<char>> {
    query
        .split_whitespace()
        .map(|token| token.chars().flat_map(char::to_lowercase).collect())
        .collect()
}
fn fuzzy_prepared(
    tokens: &[Vec<char>],
    chars: &[(usize, char, char)],
) -> Option<(i64, Vec<Range<usize>>)> {
    let mut score = 0;
    let mut ranges = Vec::new();
    for needle in tokens {
        let mut qi = 0;
        let mut consecutive = 0;
        for (ix, &(at, ch, lower)) in chars.iter().enumerate() {
            if qi == needle.len() {
                break;
            }
            if lower != needle[qi] {
                consecutive = 0;
                continue;
            }
            consecutive += 1;
            score += 1 + consecutive * 4;
            if ix == 0 || "/\\-_. ".contains(chars[ix - 1].1) {
                score += 14;
            } else if ch.is_ascii_uppercase() && !chars[ix - 1].1.is_ascii_uppercase() {
                score += 10;
            }
            ranges.push(at..at + ch.len_utf8());
            qi += 1;
        }
        if qi != needle.len() {
            return None;
        }
        score -= chars.len() as i64 - needle.len() as i64;
    }
    ranges.sort_by_key(|r| r.start);
    ranges.dedup();
    Some((score, ranges))
}
pub fn titles(rows: &[Session], query: &str) -> Vec<Hit> {
    if query.trim().is_empty() {
        return vec![];
    }
    rows.iter()
        .filter_map(|s| {
            let title = crate::session_title::display_title(&s.title, &s.provider);
            let (score, highlights) = fuzzy(query, title)?;
            Some(Hit {
                id: format!("conversation:{}", s.id),
                target: Target::Conversation(s.id),
                title: title.into(),
                meta: project_name(&s.cwd),
                provider: s.provider.clone(),
                highlights,
                score: score + recency(s.updated_at),
                updated_at: s.updated_at,
            })
        })
        .collect()
}
pub fn projects(paths: &[String], query: &str) -> Vec<Hit> {
    paths
        .iter()
        .filter_map(|path| {
            let title = project_name(path);
            let (score, highlights) = if let Some((s, h)) = fuzzy(query, &title) {
                (s + 80, h)
            } else {
                (fuzzy(query, path)?.0, vec![])
            };
            Some(Hit {
                id: format!("project:{path}"),
                target: Target::Project(path.clone()),
                title,
                meta: pretty_path(path),
                provider: String::new(),
                highlights,
                score,
                updated_at: 0,
            })
        })
        .collect()
}
#[derive(Default)]
pub struct FilenameIndex {
    files: Vec<IndexedFile>,
}
struct IndexedFile {
    file: ProjectFile,
    name: Vec<(usize, char, char)>,
    relative: Vec<(usize, char, char)>,
}
impl FilenameIndex {
    pub fn new(files: Vec<ProjectFile>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|file| IndexedFile {
                    name: lower_chars(&file.name),
                    relative: lower_chars(&file.relative),
                    file,
                })
                .collect(),
        }
    }
    pub fn rank(&self, query: &str, recent: &[String]) -> Vec<Hit> {
        let tokens = query_tokens(query);
        let mut hits: Vec<_> = self
            .files
            .iter()
            .filter_map(|indexed| {
                let f = &indexed.file;
                let (mut score, highlights) =
                    if let Some((s, h)) = fuzzy_prepared(&tokens, &indexed.name) {
                        (s + 400, h)
                    } else {
                        (fuzzy_prepared(&tokens, &indexed.relative)?.0, vec![])
                    };
                if let Some(ix) = recent.iter().position(|p| p == &f.path) {
                    score += (30 - ix.min(30)) as i64 * 8;
                }
                Some(Hit {
                    id: format!("file:{}", f.path),
                    target: Target::File {
                        path: f.path.clone(),
                        location: None,
                    },
                    title: f.name.clone(),
                    meta: f.relative.clone(),
                    provider: String::new(),
                    highlights,
                    score,
                    updated_at: 0,
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.meta.len().cmp(&b.meta.len()))
                .then(a.meta.cmp(&b.meta))
        });
        hits.truncate(40);
        hits
    }
}
pub fn literal_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return vec![];
    }
    regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
        .map(|r| r.find_iter(text).map(|m| m.range()).collect())
        .unwrap_or_default()
}
pub fn snippet(text: &str, query: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<_> = compact
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(compact.len()))
        .collect();
    let found = literal_ranges(&compact, query).first().cloned();
    let (start, end) = if let Some(found) = found {
        let from = chars
            .partition_point(|&i| i < found.start)
            .saturating_sub(42);
        let to = (chars.partition_point(|&i| i < found.end) + 42).min(chars.len() - 1);
        (chars[from], chars[to])
    } else {
        (0, chars[(chars.len() - 1).min(84)])
    };
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        compact[start..end].trim(),
        if end < compact.len() { "…" } else { "" }
    )
}
pub fn message_hit(
    session: &Session,
    row: usize,
    role: &str,
    text: &str,
    query: &str,
    live: bool,
) -> Option<Hit> {
    if !matches!(
        role,
        "user" | "assistant" | "tool" | "tasks" | "plan" | "image"
    ) {
        return None;
    }
    let text = if role == "tool" {
        serde_json::from_str::<crate::tool::ToolCall>(text)
            .ok()
            .map(|t| {
                let (a, b) = t.summary();
                format!("{a}\n{b}\n{}\n{}", t.input, t.output)
            })
            .unwrap_or_else(|| text.to_owned())
    } else {
        text.to_owned()
    };
    if !text.to_lowercase().contains(&query.to_lowercase()) {
        return None;
    }
    let title = snippet(&text, query);
    Some(Hit {
        id: format!("message:{}:{row}", session.id),
        target: Target::Message {
            session: session.id,
            row,
        },
        highlights: literal_ranges(&title, query).into_iter().take(1).collect(),
        title,
        meta: crate::session_title::display_title(&session.title, &session.provider).into(),
        provider: session.provider.clone(),
        score: if live { 20 } else { 16 } + recency(session.updated_at),
        updated_at: session.updated_at,
    })
}
pub fn messages(session: &Session, rows: &[Row], query: &str, cancel: &AtomicBool) -> Vec<Hit> {
    let mut hits = vec![];
    let mut start = 0;
    while start < rows.len() && !cancel.load(Ordering::Acquire) {
        let mut end = start + 1;
        while end < rows.len() && !rows[end].first {
            end += 1;
        }
        let text = rows[start..end]
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>();
        if let Some(hit) = message_hit(session, start, &rows[start].role, &text, query, true) {
            hits.push(hit);
        }
        start = end;
    }
    hits
}
pub fn content(matches: Vec<project::SearchMatch>, query: &str) -> Vec<Hit> {
    matches
        .into_iter()
        .map(|m| Hit {
            id: format!("content:{}:{:010}:{:010}", m.path, m.line, m.column),
            target: Target::File {
                path: m.path,
                location: Some((m.line, m.column)),
            },
            title: m.preview.trim_end().to_owned(),
            meta: format!("{}:{}", m.relative, m.line),
            provider: String::new(),
            highlights: literal_ranges(&m.preview, query)
                .into_iter()
                .take(1)
                .collect(),
            score: 0,
            updated_at: 0,
        })
        .collect()
}

#[cfg(test)]
mod tests;
