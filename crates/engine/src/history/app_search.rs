//! Port of src/features/search/model/appSearch.ts: the hit types of the
//! app-wide search page, ranking, merging, and grouping.
//!
//! The TypeScript read `Date.now()` for the recency bonus. Here the
//! functions that need it take `now`.

use std::cmp::Ordering;
use std::collections::HashMap;

use monocode_core::block::{Block, BlockRole};
use monocode_core::js;
use monocode_core::session::session_display_title;
use monocode_core::transcript::find::transcript_block_text;
use monocode_core::{HarnessId, Session};
use monocode_git::search::SearchMatch;
use monocode_layout::paths::project_name;
use serde::{Deserialize, Serialize};

use crate::runtime::session_store::{SearchHit, SessionSummary};
use crate::runtime::util::fuzzy::fuzzy_match;
use crate::runtime::util::project_path::same_project_path;

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

    /// `scoreOf`: content hits have no score.
    fn score(&self) -> i64 {
        match self {
            AppSearchHit::Conversation(hit) => hit.score,
            AppSearchHit::Message(hit) => hit.score,
            AppSearchHit::File(hit) => hit.score,
            AppSearchHit::Content(_) => 0,
            AppSearchHit::Project(hit) => hit.score,
        }
    }
}

/// `GroupedHits`: one list per result section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupedHits {
    pub conversations: Vec<ConversationHit>,
    pub messages: Vec<MessageHit>,
    pub files: Vec<FileHit>,
    pub content: Vec<ContentHit>,
    pub projects: Vec<ProjectHit>,
}

/// Per-section limits: conversations, messages, files, content, projects.
struct Limits {
    conversations: usize,
    messages: usize,
    files: usize,
    content: usize,
    projects: usize,
}

/// `SCOPE_LIMITS`.
fn scope_limits(scope: SearchScope) -> Limits {
    match scope {
        SearchScope::All => Limits {
            conversations: 8,
            messages: 8,
            files: 10,
            content: 12,
            projects: 6,
        },
        SearchScope::Conversations => Limits {
            conversations: 24,
            messages: 24,
            files: 0,
            content: 0,
            projects: 0,
        },
        SearchScope::Files => Limits {
            conversations: 0,
            messages: 0,
            files: 40,
            content: 48,
            projects: 0,
        },
        SearchScope::Projects => Limits {
            conversations: 0,
            messages: 0,
            files: 0,
            content: 0,
            projects: 24,
        },
    }
}

/// `asHarness`: unknown ids read as Cursor.
pub fn as_harness(value: &str) -> HarnessId {
    value.parse().unwrap_or(HarnessId::Cursor)
}

/// `snippetAround` with the default radius.
pub fn snippet_around(text: &str, query: &str) -> String {
    snippet_around_radius(text, query, 42)
}

/// `snippetAround`: a short excerpt centered on the first match. Lengths and
/// offsets count UTF-16 code units, as JavaScript strings do.
pub fn snippet_around_radius(text: &str, query: &str, radius: usize) -> String {
    let compact = collapse_space(text);
    let compact = js::trim(&compact);
    let needle = js::trim(query).to_lowercase();
    if compact.is_empty() {
        return String::new();
    }
    let units: Vec<u16> = compact.encode_utf16().collect();
    let head = |units: &[u16]| {
        if units.len() > radius * 2 {
            format!("{}…", String::from_utf16_lossy(&units[..radius * 2]))
        } else {
            String::from_utf16_lossy(units)
        }
    };
    if needle.is_empty() {
        return head(&units);
    }
    let lower: Vec<u16> = compact.to_lowercase().encode_utf16().collect();
    let needle: Vec<u16> = needle.encode_utf16().collect();
    let Some(index) = find_units(&lower, &needle) else {
        return head(&units);
    };
    let start = index.saturating_sub(radius);
    let end = units.len().min(index + needle.len() + radius);
    // TODO(port): like the TypeScript, offsets found in the lowercased text
    // slice the original, which drifts if lowercasing changes the length.
    let start = start.min(units.len());
    let end = end.max(start);
    let mut snippet = js::trim(&String::from_utf16_lossy(&units[start..end])).to_string();
    if start > 0 {
        snippet = format!("…{snippet}");
    }
    if end < units.len() {
        snippet = format!("{snippet}…");
    }
    snippet
}

fn collapse_space(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if js::is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

fn find_units(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// One conversation row `searchConversationTitles` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationRow {
    pub id: String,
    pub cwd: String,
    pub harness: HarnessId,
    pub title: String,
    pub updated_at: i64,
}

/// `searchConversationTitles`: fuzzy match on the display title, then the
/// raw title.
pub fn search_conversation_titles(
    rows: &[ConversationRow],
    query: &str,
    now: i64,
) -> Vec<ConversationHit> {
    let needle = js::trim(query);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for row in rows {
        let title = session_display_title(&row.title, row.harness);
        let display_hit = fuzzy_match(needle, &title);
        let raw_hit = if display_hit.is_some() {
            None
        } else {
            fuzzy_match(needle, &row.title)
        };
        let from_display = display_hit.is_some();
        let Some(found) = display_hit.or(raw_hit) else {
            continue;
        };
        hits.push(ConversationHit {
            id: format!("conversation:{}", row.id),
            session_id: row.id.clone(),
            cwd: row.cwd.clone(),
            harness: row.harness,
            title,
            updated_at: row.updated_at,
            score: found.score + recency_bonus(row.updated_at, now),
            positions: if from_display {
                found.positions
            } else {
                Vec::new()
            },
        });
    }
    hits.sort_by(by_score_then_recency);
    hits
}

/// One open session `searchSessionMessages` reads.
#[derive(Debug, Clone, Copy)]
pub struct MessageSource<'a> {
    pub id: &'a str,
    pub cwd: &'a str,
    pub harness: HarnessId,
    pub title: &'a str,
    pub updated_at: i64,
    pub blocks: &'a [Block],
}

/// `searchSessionMessages`: substring match over the searchable blocks.
pub fn search_session_messages(
    sessions: &[MessageSource<'_>],
    query: &str,
    now: i64,
) -> Vec<MessageHit> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for session in sessions {
        let title = session_display_title(session.title, session.harness);
        for block in session.blocks {
            if !is_searchable_role(block.role) {
                continue;
            }
            let text = transcript_block_text(block);
            if !text.to_lowercase().contains(&needle) {
                continue;
            }
            hits.push(MessageHit {
                id: format!("message:{}:{}", session.id, block.id),
                session_id: session.id.to_string(),
                cwd: session.cwd.to_string(),
                harness: session.harness,
                title: title.clone(),
                updated_at: session.updated_at,
                block_id: block.id.clone(),
                role: role_name(block.role).to_string(),
                preview: snippet_around(&text, query),
                score: 20 + recency_bonus(session.updated_at, now),
            });
        }
    }
    hits.sort_by(by_score_then_recency);
    hits
}

/// `searchRecentProjects`: folder name matches rank above path matches.
pub fn search_recent_projects(recents: &[String], query: &str) -> Vec<ProjectHit> {
    let needle = js::trim(query);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for path in recents {
        let name = project_name(path);
        let name_hit = fuzzy_match(needle, &name);
        let path_hit = if name_hit.is_some() {
            None
        } else {
            fuzzy_match(needle, path)
        };
        let from_name = name_hit.is_some();
        let Some(found) = name_hit.or(path_hit) else {
            continue;
        };
        hits.push(ProjectHit {
            id: format!("project:{path}"),
            path: path.clone(),
            name,
            score: found.score + if from_name { 80 } else { 0 },
            positions: if from_name {
                found.positions
            } else {
                Vec::new()
            },
        });
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| locale_compare(&a.name, &b.name))
    });
    hits
}

/// A ranked file from the project file index (`RankedFile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRank {
    pub path: String,
    pub relative: String,
    pub name: String,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `hitsFromFileRanks`.
pub fn hits_from_file_ranks(files: &[FileRank]) -> Vec<FileHit> {
    files
        .iter()
        .map(|file| FileHit {
            id: format!("file:{}", file.path),
            path: file.path.clone(),
            relative: file.relative.clone(),
            name: file.name.clone(),
            score: file.score,
            positions: file.positions.clone(),
        })
        .collect()
}

/// `hitsFromContentMatches`.
pub fn hits_from_content_matches(matches: &[SearchMatch]) -> Vec<ContentHit> {
    matches
        .iter()
        .map(|found| ContentHit {
            id: format!("content:{}:{}:{}", found.path, found.line, found.column),
            path: found.path.clone(),
            relative: found.relative.clone(),
            name: found
                .relative
                .rsplit('/')
                .next()
                .unwrap_or(&found.relative)
                .to_string(),
            line: i64::from(found.line),
            column: i64::from(found.column),
            preview: js::trim_end(&found.preview).to_string(),
        })
        .collect()
}

/// `hitsFromSessionSearch`: the store's full-text hits as search hits.
pub fn hits_from_session_search(rows: &[SearchHit], now: i64) -> Vec<AppSearchHit> {
    let mut hits = Vec::new();
    for row in rows {
        let harness = as_harness(&row.harness);
        let title = session_display_title(&row.title, harness);
        if row.kind == "conversation" {
            hits.push(AppSearchHit::Conversation(ConversationHit {
                id: format!("conversation:{}", row.session_id),
                session_id: row.session_id.clone(),
                cwd: row.cwd.clone(),
                harness,
                title,
                updated_at: row.updated_at,
                score: 10 + recency_bonus(row.updated_at, now),
                positions: Vec::new(),
            }));
            continue;
        }
        let Some(block_id) = row.block_id.as_ref().filter(|id| !id.is_empty()) else {
            continue;
        };
        if row.kind != "message" {
            continue;
        }
        hits.push(AppSearchHit::Message(MessageHit {
            id: format!("message:{}:{}", row.session_id, block_id),
            session_id: row.session_id.clone(),
            cwd: row.cwd.clone(),
            harness,
            title,
            updated_at: row.updated_at,
            block_id: block_id.clone(),
            role: row.role.clone().unwrap_or_else(|| "assistant".into()),
            preview: row.preview.clone(),
            score: 16 + recency_bonus(row.updated_at, now),
        }));
    }
    hits
}

/// `conversationRowsFrom`: history rows, with open sessions' titles on top.
/// A live session with no saved row counts as updated `now`.
pub fn conversation_rows_from(
    history: &[SessionSummary],
    sessions: &[Session],
    now: i64,
) -> Vec<ConversationRow> {
    let mut rows: Vec<ConversationRow> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut set = |row: ConversationRow, rows: &mut Vec<ConversationRow>| match index.get(&row.id) {
        Some(at) => rows[*at] = row,
        None => {
            index.insert(row.id.clone(), rows.len());
            rows.push(row);
        }
    };
    for row in history {
        set(
            ConversationRow {
                id: row.id.clone(),
                cwd: row.cwd.clone(),
                harness: row.harness,
                title: row.title.clone(),
                updated_at: row.updated_at,
            },
            &mut rows,
        );
    }
    for session in sessions {
        let previous = rows
            .iter()
            .find(|row| row.id == session.id)
            .map(|row| row.updated_at);
        set(
            ConversationRow {
                id: session.id.clone(),
                cwd: session.cwd.clone(),
                harness: session.harness,
                title: session.title.clone(),
                updated_at: previous.unwrap_or(now),
            },
            &mut rows,
        );
    }
    rows
}

/// `mergeHits`: one hit per id, keeping the higher score in the first slot.
pub fn merge_hits(lists: &[&[AppSearchHit]]) -> Vec<AppSearchHit> {
    let mut merged: Vec<AppSearchHit> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for list in lists {
        for hit in *list {
            match index.get(hit.id()) {
                None => {
                    index.insert(hit.id().to_string(), merged.len());
                    merged.push(hit.clone());
                }
                Some(&at) => {
                    if hit.score() > merged[at].score() {
                        merged[at] = hit.clone();
                    }
                }
            }
        }
    }
    merged
}

/// `filterHitsByProject`: conversations and projects of this project, and
/// every file hit.
pub fn filter_hits_by_project(hits: &[AppSearchHit], cwd: Option<&str>) -> Vec<AppSearchHit> {
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty() && *cwd != "~") else {
        return hits.to_vec();
    };
    hits.iter()
        .filter(|hit| match hit {
            AppSearchHit::File(_) | AppSearchHit::Content(_) => true,
            AppSearchHit::Project(hit) => same_project_path(&hit.path, cwd),
            AppSearchHit::Conversation(hit) => same_project_path(&hit.cwd, cwd),
            AppSearchHit::Message(hit) => same_project_path(&hit.cwd, cwd),
        })
        .cloned()
        .collect()
}

/// `groupHits`: split by kind, sort each section, and apply the scope's
/// limits.
pub fn group_hits(hits: &[AppSearchHit], scope: SearchScope) -> GroupedHits {
    let mut grouped = GroupedHits::default();
    for hit in hits {
        match hit {
            AppSearchHit::Conversation(hit) => grouped.conversations.push(hit.clone()),
            AppSearchHit::Message(hit) => grouped.messages.push(hit.clone()),
            AppSearchHit::File(hit) => grouped.files.push(hit.clone()),
            AppSearchHit::Content(hit) => grouped.content.push(hit.clone()),
            AppSearchHit::Project(hit) => grouped.projects.push(hit.clone()),
        }
    }
    grouped.conversations.sort_by(by_score_then_recency);
    grouped.messages.sort_by(by_score_then_recency);
    grouped.files.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| locale_compare(&a.relative, &b.relative))
    });
    grouped.projects.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| locale_compare(&a.name, &b.name))
    });
    let limits = scope_limits(scope);
    grouped.conversations.truncate(limits.conversations);
    grouped.messages.truncate(limits.messages);
    grouped.files.truncate(limits.files);
    grouped.content.truncate(limits.content);
    grouped.projects.truncate(limits.projects);
    grouped
}

/// `flattenGrouped`: sections in display order.
pub fn flatten_grouped(grouped: &GroupedHits) -> Vec<AppSearchHit> {
    let mut hits = Vec::new();
    hits.extend(
        grouped
            .conversations
            .iter()
            .cloned()
            .map(AppSearchHit::Conversation),
    );
    hits.extend(grouped.messages.iter().cloned().map(AppSearchHit::Message));
    hits.extend(grouped.files.iter().cloned().map(AppSearchHit::File));
    hits.extend(grouped.content.iter().cloned().map(AppSearchHit::Content));
    hits.extend(grouped.projects.iter().cloned().map(AppSearchHit::Project));
    hits
}

/// `groupedCount`.
pub fn grouped_count(grouped: &GroupedHits) -> usize {
    grouped.conversations.len()
        + grouped.messages.len()
        + grouped.files.len()
        + grouped.content.len()
        + grouped.projects.len()
}

fn is_searchable_role(role: BlockRole) -> bool {
    matches!(
        role,
        BlockRole::User
            | BlockRole::Assistant
            | BlockRole::Tool
            | BlockRole::Tasks
            | BlockRole::Plan
            | BlockRole::Image
    )
}

fn role_name(role: BlockRole) -> &'static str {
    match role {
        BlockRole::User => "user",
        BlockRole::Assistant => "assistant",
        BlockRole::Image => "image",
        BlockRole::Reasoning => "reasoning",
        BlockRole::Tool => "tool",
        BlockRole::Approval => "approval",
        BlockRole::Tasks => "tasks",
        BlockRole::Plan => "plan",
        BlockRole::System => "system",
        BlockRole::Handoff => "handoff",
    }
}

/// `recencyBonus`: up to 24 points, fading over a week.
fn recency_bonus(updated_at: i64, now: i64) -> i64 {
    if updated_at <= 0 {
        return 0;
    }
    let age = now - updated_at;
    if age <= 0 {
        return 24;
    }
    let week = 7 * 24 * 60 * 60 * 1000;
    let fraction = age.min(week) as f64 / week as f64;
    (js::round(24.0 * (1.0 - fraction)) as i64).max(0)
}

fn by_score_then_recency<T: Scored>(a: &T, b: &T) -> Ordering {
    b.score()
        .cmp(&a.score())
        .then_with(|| b.updated_at().cmp(&a.updated_at()))
}

trait Scored {
    fn score(&self) -> i64;
    fn updated_at(&self) -> i64;
}

impl Scored for ConversationHit {
    fn score(&self) -> i64 {
        self.score
    }
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
}

impl Scored for MessageHit {
    fn score(&self) -> i64 {
        self.score
    }
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
}

/// `localeCompare` with the OS default locale.
fn locale_compare(a: &str, b: &str) -> Ordering {
    monocode_locale::compare(a, b)
}

#[cfg(test)]
#[path = "app_search_tests.rs"]
mod tests;
