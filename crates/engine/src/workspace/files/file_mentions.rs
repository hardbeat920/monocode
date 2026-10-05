//! Port of src/features/files/model/fileMentions.ts: `@file` tokens in the
//! composer, the label index that resolves them, and the note that spells
//! out each mentioned path on send.
//!
//! Positions are byte offsets into the text. The TypeScript counted UTF-16
//! units; the composer passes byte offsets in the native app.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::backend::ProjectFile;
use super::file_index::{RankedFile, locale_compare, rank_project_files_limit};
use crate::workspace::chat_context::is_markdown_blockquote_position;

/// JavaScript's `\s` class. Rust's `\s` also matches U+0085, which
/// JavaScript does not.
const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

static MENTION_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(^|[{JS_SPACE}])@([^{JS_SPACE}]+)")).expect("mention pattern")
});
static LINE_LOCATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ \((?:line \d+|lines \d+[-–]\d+)\)").expect("line location pattern")
});
static LOOKS_MENTIONED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(^|[{JS_SPACE}])@[^{JS_SPACE}]")).expect("mentioned pattern")
});
static SPACE_RUN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[{JS_SPACE}]+")).expect("space pattern"));
static TOKEN_UNSAFE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[{JS_SPACE}@\\]")).expect("token pattern"));
static UNSAFE_CHARS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]").expect("unsafe pattern"));

const TRAILING_PUNCTUATION: [char; 11] = [',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '/'];
const MAX_QUERY: usize = 120;
const MAX_PICKER: usize = 30;

/// `MentionToken`: the `@token` the cursor is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionToken {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

/// `MentionIndex`: `@label` to the project file it points at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MentionIndex {
    /// Every writable label: the basename when unique, always the relative
    /// path.
    pub labels: HashMap<String, ProjectFile>,
    /// The preferred label per file path, the shortest unambiguous one.
    pub label_of: HashMap<String, String>,
}

/// `MentionHit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionHit {
    pub start: usize,
    pub end: usize,
    pub label: String,
    pub file: ProjectFile,
}

/// `MentionTextPart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionTextPart {
    pub text: String,
    pub file: Option<ProjectFile>,
}

impl MentionTextPart {
    fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            file: None,
        }
    }
}

/// `isSpace`.
fn is_space(ch: char) -> bool {
    matches!(ch, ' ' | '\n' | '\t' | '\r')
}

/// `mentionTokenAt`: the mention token that contains `cursor`, if the user
/// is typing `@file`.
pub fn mention_token_at(text: &str, cursor: usize) -> Option<MentionToken> {
    let mut i = cursor.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    let start = text[..i]
        .char_indices()
        .rev()
        .find(|(_, ch)| is_space(*ch))
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    if !text[start..].starts_with('@') {
        return None;
    }
    if is_markdown_blockquote_position(text, start) {
        return None;
    }

    let end = text[start + 1..]
        .find(is_space)
        .map_or(text.len(), |offset| start + 1 + offset);

    let typed = &text[start + 1..i.max(start + 1)];
    if typed.contains('@') || monocode_core::js::len(typed) > MAX_QUERY {
        return None;
    }

    Some(MentionToken {
        start,
        end,
        query: typed.to_string(),
    })
}

/// `replaceMentionToken`.
pub fn replace_mention_token(text: &str, token: &MentionToken, label: &str) -> String {
    let rest = &text[token.end..];
    let spacer = if rest.starts_with(' ') { "" } else { " " };
    format!("{}@{label}{spacer}{rest}", &text[..token.start])
}

/// `buildMentionIndex`.
pub fn build_mention_index(files: &[ProjectFile]) -> MentionIndex {
    let entries: Vec<ProjectFile> = with_mention_directories(files)
        .into_iter()
        .filter(|file| is_mentionable_relative(&file.relative))
        .collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for file in &entries {
        *counts.entry(file.name.as_str()).or_default() += 1;
    }

    let mut index = MentionIndex::default();
    let mut used: HashSet<String> = HashSet::new();
    let claim = |index: &mut MentionIndex,
                 used: &mut HashSet<String>,
                 label: &str,
                 file: &ProjectFile,
                 preferred: bool| {
        if !is_token_safe(label) || used.contains(label) {
            return false;
        }
        used.insert(label.to_string());
        index.labels.insert(label.to_string(), file.clone());
        if preferred || !index.label_of.contains_key(&file.path) {
            index.label_of.insert(file.path.clone(), label.to_string());
        }
        true
    };

    for file in &entries {
        let unique = !file.is_dir()
            && counts.get(file.name.as_str()) == Some(&1)
            && is_token_safe(&file.name);
        if unique {
            claim(&mut index, &mut used, &file.name, file, true);
        }
        if is_token_safe(&file.relative) {
            claim(&mut index, &mut used, &file.relative, file, false);
        }
    }

    for file in &entries {
        if index.label_of.contains_key(&file.path) {
            continue;
        }
        let Some(encoded) = encode_mention_token(&file.relative) else {
            continue;
        };
        let mut label = encoded.clone();
        let mut n = 2;
        while used.contains(&label) && n < 1000 {
            label = format!("{encoded}~{n}");
            n += 1;
        }
        claim(&mut index, &mut used, &label, file, true);
    }

    index
}

/// `mentionLabel`.
pub fn mention_label(file: &ProjectFile, index: &MentionIndex) -> String {
    index
        .label_of
        .get(&file.path)
        .cloned()
        .unwrap_or_else(|| file.relative.clone())
}

/// `rankMentionFiles` with the picker's limit.
pub fn rank_mention_files(
    files: &[ProjectFile],
    query: &str,
    recents: &[String],
) -> Vec<RankedFile> {
    rank_mention_files_limit(files, query, recents, MAX_PICKER)
}

/// `rankMentionFiles`: recents first without a query, fuzzy after.
pub fn rank_mention_files_limit(
    files: &[ProjectFile],
    query: &str,
    recents: &[String],
    limit: usize,
) -> Vec<RankedFile> {
    let usable: Vec<ProjectFile> = with_mention_directories(files)
        .into_iter()
        .filter(|file| is_mentionable_relative(&file.relative))
        .collect();
    let needle = monocode_core::js::trim(query.trim_end_matches('/'));
    if !needle.is_empty() {
        return rank_project_files_limit(&usable, needle, recents, limit);
    }

    let by_path: HashMap<&str, &ProjectFile> = usable
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let ranked = |file: &ProjectFile| RankedFile {
        file: file.clone(),
        score: 0,
        positions: Vec::new(),
    };
    for path in recents {
        let Some(file) = by_path.get(path.as_str()) else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        out.push(ranked(file));
        if out.len() >= limit {
            return out;
        }
    }

    let mut rest: Vec<&ProjectFile> = usable.iter().collect();
    rest.sort_by(|a, b| {
        path_depth(&a.relative)
            .cmp(&path_depth(&b.relative))
            .then_with(|| b.is_dir().cmp(&a.is_dir()))
            .then_with(|| locale_compare(&a.relative, &b.relative))
    });
    for file in rest {
        if !seen.insert(file.path.clone()) {
            continue;
        }
        out.push(ranked(file));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// `fileMentionParts`: split composer text so known `@file` tokens can be
/// highlighted.
pub fn file_mention_parts(
    text: &str,
    labels: &HashMap<String, ProjectFile>,
) -> Vec<MentionTextPart> {
    if text.is_empty() {
        return Vec::new();
    }
    let hits = scan_mentions(text, labels);
    if hits.is_empty() {
        return vec![MentionTextPart::plain(text)];
    }

    let mut parts = Vec::new();
    let mut cursor = 0;
    for hit in hits {
        if hit.start > cursor {
            parts.push(MentionTextPart::plain(&text[cursor..hit.start]));
        }
        parts.push(MentionTextPart {
            text: text[hit.start..hit.end].to_string(),
            file: Some(hit.file),
        });
        cursor = hit.end;
    }
    if cursor < text.len() {
        parts.push(MentionTextPart::plain(&text[cursor..]));
    }
    parts
}

/// `fileMentionsInText`: each referenced file once.
pub fn file_mentions_in_text(text: &str, labels: &HashMap<String, ProjectFile>) -> Vec<MentionHit> {
    let mut seen = HashSet::new();
    scan_mentions(text, labels)
        .into_iter()
        .filter(|hit| seen.insert(hit.file.path.clone()))
        .collect()
}

/// The pure part of `applyFileMentionsToTurn`: spell out where each
/// `@name` lives, so the harness does not have to guess which `App.tsx` the
/// user meant. Tokens already written as a project-relative path need no
/// help. `FileIndex::apply_file_mentions_to_turn` loads `files`.
pub fn apply_file_mentions(text: &str, files: &[ProjectFile]) -> String {
    if files.is_empty() {
        return text.to_string();
    }
    let index = build_mention_index(files);
    let lines: Vec<String> = file_mentions_in_text(text, &index.labels)
        .into_iter()
        .filter(|hit| hit.label != hit.file.relative)
        .map(|hit| format!("- @{} → {}", hit.label, hit.file.relative))
        .collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let mut out = vec![
        text.to_string(),
        String::new(),
        "---".into(),
        "Referenced with @ above:".into(),
    ];
    out.extend(lines);
    out.join("\n")
}

/// `looksMentioned`.
pub fn looks_mentioned(text: &str) -> bool {
    LOOKS_MENTIONED_RE.is_match(text)
}

/// `withMentionDirectories`: parent folders of indexed files, so `@` can
/// point at a directory.
pub fn with_mention_directories(files: &[ProjectFile]) -> Vec<ProjectFile> {
    let mut order: Vec<String> = Vec::new();
    let mut dirs: HashMap<String, ProjectFile> = HashMap::new();
    for file in files {
        if file.is_dir() {
            if is_safe_project_relative(&file.relative) {
                if !dirs.contains_key(&file.relative) {
                    order.push(file.relative.clone());
                }
                dirs.insert(file.relative.clone(), file.clone());
            }
            continue;
        }
        if !is_safe_project_relative(&file.relative) {
            continue;
        }
        let parts: Vec<&str> = file.relative.split('/').collect();
        let mut rel = String::new();
        for segment in &parts[..parts.len() - 1] {
            rel = if rel.is_empty() {
                segment.to_string()
            } else {
                format!("{rel}/{segment}")
            };
            if dirs.contains_key(&rel) || !is_safe_project_relative(&rel) {
                continue;
            }
            order.push(rel.clone());
            dirs.insert(
                rel.clone(),
                ProjectFile {
                    name: segment.to_string(),
                    path: mention_dir_path(file, &rel),
                    relative: rel.clone(),
                    is_dir: Some(true),
                },
            );
        }
    }
    if dirs.is_empty() {
        return files.to_vec();
    }

    let seen: HashSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let extra: Vec<ProjectFile> = order
        .iter()
        .filter_map(|rel| dirs.get(rel))
        .filter(|dir| !seen.contains(dir.path.as_str()))
        .cloned()
        .collect();
    let mut out = files.to_vec();
    out.extend(extra);
    out
}

fn scan_mentions(text: &str, labels: &HashMap<String, ProjectFile>) -> Vec<MentionHit> {
    if labels.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for captures in MENTION_TOKEN_RE.captures_iter(text) {
        let Some(raw) = captures.get(2) else {
            continue;
        };
        let lead = captures.get(1).map_or(0, |lead| lead.len());
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let start = whole.start() + lead;
        if is_markdown_blockquote_position(text, start) {
            continue;
        }
        let Some((label, file)) = resolve_label(raw.as_str(), labels) else {
            continue;
        };
        let mention_end = start + 1 + label.len();
        let location = LINE_LOCATION_RE
            .find(&text[mention_end..])
            .map_or(0, |found| found.len());
        hits.push(MentionHit {
            start,
            end: mention_end + location,
            label: label.to_string(),
            file: file.clone(),
        });
    }
    hits
}

/// `resolveLabel`: `@App.tsx,` still points at `App.tsx`, so peel trailing
/// punctuation.
fn resolve_label<'a>(
    raw: &'a str,
    labels: &'a HashMap<String, ProjectFile>,
) -> Option<(&'a str, &'a ProjectFile)> {
    let mut value = raw;
    while !value.is_empty() {
        if let Some(file) = labels.get(value) {
            return Some((value, file));
        }
        let last = value.chars().last()?;
        if !TRAILING_PUNCTUATION.contains(&last) {
            return None;
        }
        value = &value[..value.len() - last.len_utf8()];
    }
    None
}

/// `mentionDirPath`: the folder path, cut from the file path.
fn mention_dir_path(file: &ProjectFile, relative_dir: &str) -> String {
    let trail = &file.relative[relative_dir.len()..];
    if trail.is_empty() {
        return file.path.clone();
    }
    if let Some(dir) = file.path.strip_suffix(trail) {
        return dir.to_string();
    }
    let win_trail = trail.replace('/', "\\");
    if let Some(dir) = file.path.strip_suffix(win_trail.as_str()) {
        return dir.to_string();
    }
    let mut cut = file.path.len().saturating_sub(trail.len());
    while cut > 0 && !file.path.is_char_boundary(cut) {
        cut -= 1;
    }
    file.path[..cut].to_string()
}

fn path_depth(relative: &str) -> usize {
    relative.matches('/').count()
}

/// `isSafeProjectRelative`: workspace-relative only, with no absolute
/// paths, no `..`, and no control or format characters.
fn is_safe_project_relative(relative: &str) -> bool {
    if relative.is_empty() || monocode_core::js::len(relative) > MAX_QUERY * 4 {
        return false;
    }
    if has_unsafe_chars(relative) {
        return false;
    }
    if relative.starts_with('/') || relative.starts_with('\\') {
        return false;
    }
    if relative.contains("://") {
        return false;
    }
    relative
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// `encodeMentionToken`: `@` tokens are one whitespace-delimited word, so a
/// spaced path inserts with its whitespace collapsed to `-`. Send recovers
/// the real path from the index.
fn encode_mention_token(relative: &str) -> Option<String> {
    let encoded = SPACE_RUN_RE.replace_all(relative, "-").into_owned();
    is_token_safe(&encoded).then_some(encoded)
}

/// `isTokenSafe`.
fn is_token_safe(value: &str) -> bool {
    !value.is_empty()
        && monocode_core::js::len(value) <= MAX_QUERY
        && !TOKEN_UNSAFE_RE.is_match(value)
        && !has_unsafe_chars(value)
}

/// `hasUnsafeChars`: ASCII and C1 controls, bidi and format marks, and the
/// Unicode line and paragraph separators.
fn has_unsafe_chars(value: &str) -> bool {
    UNSAFE_CHARS_RE.is_match(value)
}

fn is_mentionable_relative(relative: &str) -> bool {
    is_safe_project_relative(relative)
        && (is_token_safe(relative) || encode_mention_token(relative).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<ProjectFile> {
        vec![
            ProjectFile::new(
                "App.tsx",
                "/p/apps/desktop/src/App.tsx",
                "apps/desktop/src/App.tsx",
            ),
            ProjectFile::new("App.tsx", "/p/apps/web/src/App.tsx", "apps/web/src/App.tsx"),
            ProjectFile::new(
                "Composer.tsx",
                "/p/src/chrome/Composer.tsx",
                "src/chrome/Composer.tsx",
            ),
            ProjectFile::new("read me.md", "/p/docs/read me.md", "docs/read me.md"),
        ]
    }

    fn index() -> MentionIndex {
        build_mention_index(&files())
    }

    fn token(start: usize, end: usize, query: &str) -> MentionToken {
        MentionToken {
            start,
            end,
            query: query.into(),
        }
    }

    fn part(text: &str, file: Option<&ProjectFile>) -> MentionTextPart {
        MentionTextPart {
            text: text.into(),
            file: file.cloned(),
        }
    }

    fn dir(name: &str, path: &str, relative: &str) -> ProjectFile {
        ProjectFile {
            name: name.into(),
            path: path.into(),
            relative: relative.into(),
            is_dir: Some(true),
        }
    }

    #[test]
    fn reads_the_token_the_cursor_is_in() {
        assert_eq!(mention_token_at("@Comp", 5), Some(token(0, 5, "Comp")));
        assert_eq!(
            mention_token_at("look at @src/App", 16),
            Some(token(8, 16, "src/App"))
        );
    }

    #[test]
    fn ignores_emails_and_mid_word_at() {
        assert_eq!(mention_token_at("nick@example.com", 8), None);
        assert_eq!(mention_token_at("a@b", 3), None);
    }

    #[test]
    fn closes_after_a_space() {
        assert_eq!(mention_token_at("@App.tsx now", 12), None);
    }

    #[test]
    fn inserts_the_label_and_a_trailing_space() {
        assert_eq!(
            replace_mention_token("@Comp", &token(0, 5, "Comp"), "Composer.tsx"),
            "@Composer.tsx "
        );
        assert_eq!(
            replace_mention_token("x @a y", &token(2, 4, "a"), "App.tsx"),
            "x @App.tsx y"
        );
    }

    #[test]
    fn labels_unique_basenames_short_and_ambiguous_ones_by_path() {
        let index = index();
        assert_eq!(mention_label(&files()[2], &index), "Composer.tsx");
        assert_eq!(
            mention_label(&files()[0], &index),
            "apps/desktop/src/App.tsx"
        );
    }

    #[test]
    fn keeps_a_spaced_path_as_a_single_token() {
        let index = index();
        assert_eq!(mention_label(&files()[3], &index), "docs/read-me.md");
        assert_eq!(index.labels.get("docs/read-me.md"), Some(&files()[3]));
        assert!(!index.labels.contains_key("read me.md"));
    }

    #[test]
    fn does_not_steal_a_token_already_owned_by_a_real_path() {
        let spaced = ProjectFile::new("read me.md", "/p/notes/read me.md", "notes/read me.md");
        let existing = ProjectFile::new("read-me.md", "/p/notes/read-me.md", "notes/read-me.md");
        let mixed = build_mention_index(&[spaced.clone(), existing.clone()]);
        assert_eq!(mention_label(&existing, &mixed), "read-me.md");
        assert_eq!(mixed.labels.get("notes/read-me.md"), Some(&existing));
        assert_eq!(mention_label(&spaced, &mixed), "notes/read-me.md~2");
        assert_eq!(mixed.labels.get("notes/read-me.md~2"), Some(&spaced));
    }

    #[test]
    fn ignores_paths_that_leave_the_project_or_break_the_tokenizer() {
        let unsafe_files = [
            ProjectFile::new("secret", "/etc/secret", "../secret"),
            ProjectFile::new("abs", "/tmp/abs", "/tmp/abs"),
            ProjectFile::new("at.md", "/p/at.md", "see@me.md"),
            ProjectFile::new("nul", "/p/nul", "bad\0name.md"),
            ProjectFile::new("ls.md", "/p/ls.md", "docs/read\u{2028}me.md"),
            ProjectFile::new("ps.md", "/p/ps.md", "docs/read\u{2029}me.md"),
            ProjectFile::new("nel.md", "/p/nel.md", "a\u{0085}b.md"),
            ProjectFile::new("bidi.md", "/p/bidi.md", "photo\u{202E}gpj.md"),
            ProjectFile::new("zwsp.ts", "/p/zwsp.ts", "file\u{200B}.ts"),
        ];
        assert!(build_mention_index(&unsafe_files).labels.is_empty());
    }

    #[test]
    fn always_accepts_the_relative_path_as_a_label() {
        let index = index();
        assert_eq!(index.labels.get("apps/web/src/App.tsx"), Some(&files()[1]));
        assert_eq!(
            index.labels.get("src/chrome/Composer.tsx"),
            Some(&files()[2])
        );
    }

    #[test]
    fn indexes_parent_folders_so_they_can_be_mentioned() {
        let index = index();
        let chrome = index.labels.get("src/chrome").unwrap();
        assert_eq!(chrome, &dir("chrome", "/p/src/chrome", "src/chrome"));
        assert_eq!(mention_label(chrome, &index), "src/chrome");
        assert!(!index.labels.contains_key("chrome"));
        let src = index.labels.get("src").unwrap();
        assert_eq!(
            (src.relative.as_str(), src.path.as_str(), src.is_dir()),
            ("src", "/p/src", true)
        );
        let web = index.labels.get("apps/web/src").unwrap();
        assert!(web.is_dir());
    }

    #[test]
    fn still_mentions_a_folder_when_its_only_file_has_a_space_in_the_name() {
        let docs = index().labels.get("docs").cloned().unwrap();
        assert_eq!(docs, dir("docs", "/p/docs", "docs"));
    }

    #[test]
    fn inserts_a_spaced_folder_as_a_normal_token() {
        let cat = ProjectFile::new("cat.png", "/p/My Photos/cat.png", "My Photos/cat.png");
        let photos = build_mention_index(std::slice::from_ref(&cat));
        let folder = photos
            .labels
            .values()
            .find(|file| file.relative == "My Photos")
            .cloned()
            .unwrap();
        assert!(folder.is_dir());
        assert_eq!(mention_label(&folder, &photos), "My-Photos");
        assert_eq!(mention_label(&cat, &photos), "cat.png");
    }

    #[test]
    fn splits_known_mentions_out_of_the_surrounding_text() {
        let index = index();
        assert_eq!(
            file_mention_parts("fix @Composer.tsx now", &index.labels),
            vec![
                part("fix ", None),
                part("@Composer.tsx", Some(&files()[2])),
                part(" now", None)
            ]
        );
    }

    #[test]
    fn leaves_unknown_mentions_and_emails_alone() {
        let index = index();
        assert_eq!(
            file_mention_parts("ping @nobody.tsx", &index.labels),
            vec![part("ping @nobody.tsx", None)]
        );
        assert_eq!(
            file_mention_parts("nick@example.com", &index.labels),
            vec![part("nick@example.com", None)]
        );
    }

    #[test]
    fn keeps_trailing_punctuation_outside_the_mention() {
        let index = index();
        assert_eq!(
            file_mention_parts("see @Composer.tsx, then", &index.labels),
            vec![
                part("see ", None),
                part("@Composer.tsx", Some(&files()[2])),
                part(", then", None)
            ]
        );
    }

    #[test]
    fn includes_an_editor_line_location_in_the_highlighted_mention() {
        let index = index();
        assert_eq!(
            file_mention_parts(
                "@apps/web/src/App.tsx (lines 115-123)\n\nwhat is this?",
                &index.labels
            ),
            vec![
                part("@apps/web/src/App.tsx (lines 115-123)", Some(&files()[1])),
                part("\n\nwhat is this?", None)
            ]
        );
        assert_eq!(
            file_mention_parts("check @Composer.tsx (line 7)", &index.labels),
            vec![
                part("check ", None),
                part("@Composer.tsx (line 7)", Some(&files()[2]))
            ]
        );
    }

    #[test]
    fn highlights_folder_mentions_including_a_trailing_slash() {
        let index = index();
        let chrome = index.labels.get("src/chrome");
        assert_eq!(
            file_mention_parts("look in @src/chrome please", &index.labels),
            vec![
                part("look in ", None),
                part("@src/chrome", chrome),
                part(" please", None)
            ]
        );
        assert_eq!(
            file_mention_parts("look in @src/ next", &index.labels),
            vec![
                part("look in ", None),
                part("@src", index.labels.get("src")),
                part("/ next", None)
            ]
        );
    }

    #[test]
    fn ignores_file_mentions_inside_markdown_blockquotes() {
        let index = index();
        let text = "@Composer.tsx\n> @apps/web/src/App.tsx";
        assert_eq!(
            mention_token_at(text, text.find("@apps/web").unwrap() + 4),
            None
        );
        let labels: Vec<String> = file_mentions_in_text(text, &index.labels)
            .into_iter()
            .map(|hit| hit.label)
            .collect();
        assert_eq!(labels, vec!["Composer.tsx"]);
        assert_eq!(
            file_mention_parts(text, &index.labels)
                .iter()
                .filter(|part| part.file.is_some())
                .count(),
            1
        );
    }

    #[test]
    fn highlights_the_encoded_token_for_a_spaced_path() {
        let index = index();
        assert_eq!(
            file_mention_parts("see @docs/read-me.md now", &index.labels),
            vec![
                part("see ", None),
                part("@docs/read-me.md", Some(&files()[3])),
                part(" now", None)
            ]
        );
    }

    #[test]
    fn collects_each_referenced_file_once() {
        let index = index();
        let hits: Vec<String> = file_mentions_in_text(
            "@Composer.tsx and @apps/web/src/App.tsx and @Composer.tsx",
            &index.labels,
        )
        .into_iter()
        .map(|hit| hit.label)
        .collect();
        assert_eq!(hits, vec!["Composer.tsx", "apps/web/src/App.tsx"]);
    }

    #[test]
    fn offers_recents_first_when_nothing_is_typed() {
        let ranked = rank_mention_files(&files(), "", &[files()[1].path.clone()]);
        assert_eq!(ranked[0].file.path, files()[1].path);
    }

    #[test]
    fn fuzzy_matches_spaced_names_from_a_space_free_query() {
        assert_eq!(
            rank_mention_files(&files(), "read", &[])[0].file.path,
            files()[3].path
        );
        assert_eq!(
            rank_mention_files(&files(), "compo", &[])[0].file.path,
            files()[2].path
        );
    }

    #[test]
    fn lists_a_folder_whose_name_has_spaces() {
        let photos = [ProjectFile::new(
            "cat.png",
            "/p/My Photos/cat.png",
            "My Photos/cat.png",
        )];
        assert!(
            rank_mention_files(&photos, "photos", &[])
                .iter()
                .any(|ranked| ranked.file.relative == "My Photos" && ranked.file.is_dir())
        );
        assert_eq!(
            rank_mention_files(&photos, "cat", &[])[0].file.path,
            photos[0].path
        );
    }

    #[test]
    fn offers_folders_alongside_files() {
        let ranked = rank_mention_files(&files(), "chrome", &[]);
        assert_eq!(
            (ranked[0].file.relative.as_str(), ranked[0].file.is_dir()),
            ("src/chrome", true)
        );
        let ranked = rank_mention_files(&files(), "src/", &[]);
        assert_eq!(
            (ranked[0].file.relative.as_str(), ranked[0].file.is_dir()),
            ("src", true)
        );
    }

    #[test]
    fn puts_folders_first_when_nothing_is_typed() {
        let ranked = rank_mention_files(&files(), "", &[]);
        let dirs: Vec<&str> = ranked
            .iter()
            .filter(|ranked| ranked.file.is_dir())
            .map(|ranked| ranked.file.relative.as_str())
            .collect();
        for expected in ["apps", "docs", "src", "src/chrome"] {
            assert!(dirs.contains(&expected), "{expected} missing from {dirs:?}");
        }
        assert!(ranked[0].file.is_dir());
    }

    #[test]
    fn derives_unique_parent_folders_from_file_paths() {
        let entries = with_mention_directories(&files());
        let dirs: Vec<&ProjectFile> = entries.iter().filter(|file| file.is_dir()).collect();
        for expected in [
            dir("src", "/p/src", "src"),
            dir("chrome", "/p/src/chrome", "src/chrome"),
            dir("apps", "/p/apps", "apps"),
        ] {
            assert!(dirs.contains(&&expected), "{expected:?} missing");
        }
        assert_eq!(with_mention_directories(&entries), entries);
    }

    #[test]
    fn keeps_parent_folders_that_contain_spaces() {
        let entries = with_mention_directories(&[ProjectFile::new(
            "cat.png",
            "/p/My Photos/cat.png",
            "My Photos/cat.png",
        )]);
        assert!(
            entries
                .iter()
                .any(|file| file.relative == "My Photos" && file.is_dir())
        );
    }

    #[test]
    fn rebuilds_folder_paths_on_windows_separators() {
        let entries = with_mention_directories(&[ProjectFile::new(
            "App.tsx",
            "C:\\p\\apps\\web\\src\\App.tsx",
            "apps/web/src/App.tsx",
        )]);
        let web = entries
            .iter()
            .find(|file| file.relative == "apps/web")
            .unwrap();
        assert_eq!(
            (web.name.as_str(), web.path.as_str(), web.is_dir()),
            ("web", "C:\\p\\apps\\web", true)
        );
    }

    #[test]
    fn maps_a_spaced_file_token_to_the_real_relative_path_on_send() {
        let out = apply_file_mentions("look at @docs/read-me.md", &files());
        assert!(out.contains("look at @docs/read-me.md"));
        assert!(out.contains("- @docs/read-me.md → docs/read me.md"));
    }

    #[test]
    fn does_not_invent_a_path_from_a_token_that_is_not_in_the_index() {
        assert_eq!(
            apply_file_mentions("look at @notes/read-me.md~2", &files()),
            "look at @notes/read-me.md~2"
        );
        assert!(looks_mentioned("hi @x"));
        assert!(!looks_mentioned("nick@example.com"));
    }
}
