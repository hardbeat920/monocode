//! Port of the composer half of src/features/files/model/fileMentions.ts:
//! `@token` parsing, the label index, and the highlight split. Ranking and
//! the project file index stay with the engine (`fileIndex.ts`), behind
//! [`crate::composer::host::ComposerHost::rank_mentions`].
//!
//! Positions are byte offsets. Length limits count UTF-16 units, as the
//! TypeScript did.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use monocode_core::js;
use regex::Regex;

use super::quote_draft::is_markdown_blockquote_position;

/// `ProjectFile` from src/platform/tauri/fs.ts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProjectFile {
    pub name: String,
    pub path: String,
    pub relative: String,
    pub is_dir: bool,
}

impl ProjectFile {
    pub fn new(name: &str, path: &str, relative: &str) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            relative: relative.into(),
            is_dir: false,
        }
    }

    pub fn dir(name: &str, path: &str, relative: &str) -> Self {
        Self {
            is_dir: true,
            ..Self::new(name, path, relative)
        }
    }
}

/// `RankedFile`: a picker row with its match score and highlighted
/// positions (UTF-16 indexes into `relative`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RankedFile {
    pub file: ProjectFile,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `MentionToken`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionToken {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

/// `MentionIndex`: `@label` to the file it points at, plus the label to
/// write for each file.
#[derive(Clone, Debug, Default)]
pub struct MentionIndex {
    /// Every writable label (basename when unique, always the relative path).
    pub labels: HashMap<String, ProjectFile>,
    /// Preferred label per file path, the shortest unambiguous one.
    pub label_of: HashMap<String, String>,
}

/// One `@mention` hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionHit {
    pub start: usize,
    pub end: usize,
    pub label: String,
    pub file: ProjectFile,
}

/// `MentionTextPart`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionTextPart {
    pub text: String,
    pub file: Option<ProjectFile>,
}

const MAX_QUERY: usize = 120;
const TRAILING_PUNCTUATION: [char; 11] = [',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '/'];

static UNSAFE_CHARS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]").unwrap());
static LINE_LOCATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ \((?:line \d+|lines \d+[-–]\d+)\)").unwrap());

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r')
}

/// `mentionTokenAt`: the mention token that contains `cursor`, if the user
/// is typing `@file`.
pub fn mention_token_at(text: &str, cursor: usize) -> Option<MentionToken> {
    let mut i = cursor.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    let mut start = i;
    while let Some(previous) = text[..start].chars().next_back() {
        if is_space(previous) {
            break;
        }
        start -= previous.len_utf8();
    }
    if !text[start..].starts_with('@') {
        return None;
    }
    if is_markdown_blockquote_position(text, start) {
        return None;
    }
    let mut end = start + 1;
    while let Some(next) = text[end..].chars().next() {
        if is_space(next) {
            break;
        }
        end += next.len_utf8();
    }
    let typed = &text[(start + 1).min(i)..i];
    if typed.contains('@') || js::len(typed) > MAX_QUERY {
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
    let rest = &text[token.end.min(text.len())..];
    let spacer = if rest.starts_with(' ') { "" } else { " " };
    format!(
        "{}@{label}{spacer}{rest}",
        &text[..token.start.min(text.len())]
    )
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
    for file in &entries {
        let unique =
            !file.is_dir && counts.get(file.name.as_str()) == Some(&1) && is_token_safe(&file.name);
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

fn claim(
    index: &mut MentionIndex,
    used: &mut HashSet<String>,
    label: &str,
    file: &ProjectFile,
    preferred: bool,
) -> bool {
    if !is_token_safe(label) || used.contains(label) {
        return false;
    }
    used.insert(label.to_string());
    index.labels.insert(label.to_string(), file.clone());
    if preferred || !index.label_of.contains_key(&file.path) {
        index.label_of.insert(file.path.clone(), label.to_string());
    }
    true
}

/// `mentionLabel`.
pub fn mention_label(file: &ProjectFile, index: &MentionIndex) -> String {
    index
        .label_of
        .get(&file.path)
        .cloned()
        .unwrap_or_else(|| file.relative.clone())
}

/// Known `@mention` hits in `text`, in order.
pub fn scan_mentions(text: &str, labels: &HashMap<String, ProjectFile>) -> Vec<MentionHit> {
    if labels.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    // `/(^|\s)@(\S+)/g`.
    let mut search = 0;
    while let Some(offset) = text[search..].find('@') {
        let at = search + offset;
        search = at + 1;
        let lead_ok = at == 0 || text[..at].chars().next_back().is_some_and(js::is_space);
        if !lead_ok {
            continue;
        }
        let raw_end = text[at + 1..]
            .char_indices()
            .find(|(_, c)| js::is_space(*c))
            .map_or(text.len(), |(ix, _)| at + 1 + ix);
        let raw = &text[at + 1..raw_end];
        if raw.is_empty() {
            continue;
        }
        // The regex consumes the whole `\S+` run before looking again.
        search = raw_end;
        if is_markdown_blockquote_position(text, at) {
            continue;
        }
        let Some((label, file)) = resolve_label(raw, labels) else {
            continue;
        };
        let mention_end = at + 1 + label.len();
        let location = LINE_LOCATION
            .find(&text[mention_end..])
            .map_or(0, |m| m.end());
        hits.push(MentionHit {
            start: at,
            end: mention_end + location,
            label: label.to_string(),
            file: file.clone(),
        });
    }
    hits
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
        return vec![MentionTextPart {
            text: text.to_string(),
            file: None,
        }];
    }
    let mut parts = Vec::new();
    let mut cursor = 0;
    for hit in hits {
        if hit.start > cursor {
            parts.push(MentionTextPart {
                text: text[cursor..hit.start].to_string(),
                file: None,
            });
        }
        parts.push(MentionTextPart {
            text: text[hit.start..hit.end].to_string(),
            file: Some(hit.file),
        });
        cursor = hit.end;
    }
    if cursor < text.len() {
        parts.push(MentionTextPart {
            text: text[cursor..].to_string(),
            file: None,
        });
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

/// Byte ranges of mention hits, for the highlight layer.
pub fn mention_ranges(
    text: &str,
    labels: &HashMap<String, ProjectFile>,
) -> Vec<(Range<usize>, ProjectFile)> {
    scan_mentions(text, labels)
        .into_iter()
        .map(|hit| (hit.start..hit.end, hit.file))
        .collect()
}

/// `withMentionDirectories`: parent folders of indexed files, so `@` can
/// point at a directory.
pub fn with_mention_directories(files: &[ProjectFile]) -> Vec<ProjectFile> {
    let mut order: Vec<String> = Vec::new();
    let mut dirs: HashMap<String, ProjectFile> = HashMap::new();
    for file in files {
        if file.is_dir {
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
        for segment in &parts[..parts.len().saturating_sub(1)] {
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
                ProjectFile::dir(segment, &mention_dir_path(file, &rel), &rel),
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

/// `@App.tsx,` still points at `App.tsx`: peel trailing punctuation.
fn resolve_label<'a>(
    raw: &'a str,
    labels: &'a HashMap<String, ProjectFile>,
) -> Option<(&'a str, &'a ProjectFile)> {
    let mut value = raw;
    while !value.is_empty() {
        if let Some(file) = labels.get(value) {
            return Some((value, file));
        }
        let last = value.chars().next_back()?;
        if !TRAILING_PUNCTUATION.contains(&last) {
            return None;
        }
        value = &value[..value.len() - last.len_utf8()];
    }
    None
}

fn mention_dir_path(file: &ProjectFile, relative_dir: &str) -> String {
    let trail = &file.relative[relative_dir.len()..];
    if trail.is_empty() {
        return file.path.clone();
    }
    if let Some(head) = file.path.strip_suffix(trail) {
        return head.to_string();
    }
    let win_trail = trail.replace('/', "\\");
    if let Some(head) = file.path.strip_suffix(win_trail.as_str()) {
        return head.to_string();
    }
    let keep = file.path.len().saturating_sub(trail.len());
    let mut keep = keep.min(file.path.len());
    while !file.path.is_char_boundary(keep) {
        keep -= 1;
    }
    file.path[..keep].to_string()
}

/// Workspace-relative only: no absolute paths, `..`, or control and format
/// characters.
fn is_safe_project_relative(relative: &str) -> bool {
    if relative.is_empty() || js::len(relative) > MAX_QUERY * 4 {
        return false;
    }
    if UNSAFE_CHARS.is_match(relative) {
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

/// `encodeMentionToken`: collapse whitespace so a spaced path still inserts
/// as one `@label`.
fn encode_mention_token(relative: &str) -> Option<String> {
    let mut encoded = String::with_capacity(relative.len());
    let mut in_space = false;
    for c in relative.chars() {
        if js::is_space(c) {
            if !in_space {
                encoded.push('-');
            }
            in_space = true;
        } else {
            encoded.push(c);
            in_space = false;
        }
    }
    is_token_safe(&encoded).then_some(encoded)
}

fn is_token_safe(value: &str) -> bool {
    !value.is_empty()
        && js::len(value) <= MAX_QUERY
        && !value
            .chars()
            .any(|c| js::is_space(c) || c == '@' || c == '\\')
        && !UNSAFE_CHARS.is_match(value)
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

    fn plain(text: &str) -> MentionTextPart {
        MentionTextPart {
            text: text.into(),
            file: None,
        }
    }

    fn hit(text: &str, file: &ProjectFile) -> MentionTextPart {
        MentionTextPart {
            text: text.into(),
            file: Some(file.clone()),
        }
    }

    // mentionTokenAt
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

    // replaceMentionToken
    #[test]
    fn inserts_at_label_and_a_trailing_space() {
        assert_eq!(
            replace_mention_token("@Comp", &token(0, 5, "Comp"), "Composer.tsx"),
            "@Composer.tsx "
        );
        assert_eq!(
            replace_mention_token("x @a y", &token(2, 4, "a"), "App.tsx"),
            "x @App.tsx y"
        );
    }

    // buildMentionIndex
    #[test]
    fn labels_unique_basenames_short_and_ambiguous_ones_by_path() {
        let files = files();
        let index = index();
        assert_eq!(mention_label(&files[2], &index), "Composer.tsx");
        assert_eq!(mention_label(&files[0], &index), "apps/desktop/src/App.tsx");
    }

    #[test]
    fn keeps_a_spaced_path_as_a_single_at_token() {
        let files = files();
        let index = index();
        assert_eq!(mention_label(&files[3], &index), "docs/read-me.md");
        assert_eq!(index.labels.get("docs/read-me.md"), Some(&files[3]));
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
        let unsafe_index = build_mention_index(&[
            ProjectFile::new("secret", "/etc/secret", "../secret"),
            ProjectFile::new("abs", "/tmp/abs", "/tmp/abs"),
            ProjectFile::new("at.md", "/p/at.md", "see@me.md"),
            ProjectFile::new("nul", "/p/nul", "bad\0name.md"),
            ProjectFile::new("ls.md", "/p/ls.md", "docs/read\u{2028}me.md"),
            ProjectFile::new("ps.md", "/p/ps.md", "docs/read\u{2029}me.md"),
            ProjectFile::new("nel.md", "/p/nel.md", "a\u{0085}b.md"),
            ProjectFile::new("bidi.md", "/p/bidi.md", "photo\u{202E}gpj.md"),
            ProjectFile::new("zwsp.ts", "/p/zwsp.ts", "file\u{200B}.ts"),
        ]);
        assert_eq!(unsafe_index.labels.len(), 0);
    }

    #[test]
    fn always_accepts_the_relative_path_as_a_label() {
        let files = files();
        let index = index();
        assert_eq!(index.labels.get("apps/web/src/App.tsx"), Some(&files[1]));
        assert_eq!(index.labels.get("src/chrome/Composer.tsx"), Some(&files[2]));
    }

    #[test]
    fn indexes_parent_folders_so_they_can_be_mentioned() {
        let index = index();
        let chrome = index.labels.get("src/chrome").unwrap();
        assert_eq!(
            chrome,
            &ProjectFile::dir("chrome", "/p/src/chrome", "src/chrome")
        );
        assert_eq!(mention_label(chrome, &index), "src/chrome");
        assert!(!index.labels.contains_key("chrome"));
        assert_eq!(index.labels.get("src").unwrap().path, "/p/src");
        assert!(index.labels.get("apps/web/src").unwrap().is_dir);
    }

    #[test]
    fn still_mentions_a_folder_when_its_only_file_has_a_space_in_the_name() {
        let index = index();
        let docs = index.labels.get("docs").unwrap();
        assert_eq!((docs.path.as_str(), docs.is_dir), ("/p/docs", true));
    }

    #[test]
    fn inserts_a_spaced_folder_as_a_normal_at_token() {
        let cat = ProjectFile::new("cat.png", "/p/My Photos/cat.png", "My Photos/cat.png");
        let photos = build_mention_index(std::slice::from_ref(&cat));
        let folder = photos
            .labels
            .values()
            .find(|file| file.relative == "My Photos")
            .unwrap();
        assert!(folder.is_dir);
        assert_eq!(mention_label(folder, &photos), "My-Photos");
        assert_eq!(mention_label(&cat, &photos), "cat.png");
    }

    // fileMentionParts
    #[test]
    fn splits_known_mentions_out_of_the_surrounding_text() {
        let files = files();
        assert_eq!(
            file_mention_parts("fix @Composer.tsx now", &index().labels),
            vec![
                plain("fix "),
                hit("@Composer.tsx", &files[2]),
                plain(" now")
            ]
        );
    }

    #[test]
    fn leaves_unknown_mentions_and_emails_alone() {
        let labels = index().labels;
        assert_eq!(
            file_mention_parts("ping @nobody.tsx", &labels),
            vec![plain("ping @nobody.tsx")]
        );
        assert_eq!(
            file_mention_parts("nick@example.com", &labels),
            vec![plain("nick@example.com")]
        );
    }

    #[test]
    fn keeps_trailing_punctuation_outside_the_mention() {
        let files = files();
        assert_eq!(
            file_mention_parts("see @Composer.tsx, then", &index().labels),
            vec![
                plain("see "),
                hit("@Composer.tsx", &files[2]),
                plain(", then")
            ]
        );
    }

    #[test]
    fn includes_an_editor_line_location_in_the_highlighted_mention() {
        let files = files();
        let labels = index().labels;
        assert_eq!(
            file_mention_parts(
                "@apps/web/src/App.tsx (lines 115-123)\n\nwhat is this?",
                &labels
            ),
            vec![
                hit("@apps/web/src/App.tsx (lines 115-123)", &files[1]),
                plain("\n\nwhat is this?")
            ]
        );
        assert_eq!(
            file_mention_parts("check @Composer.tsx (line 7)", &labels),
            vec![plain("check "), hit("@Composer.tsx (line 7)", &files[2])]
        );
    }

    #[test]
    fn highlights_folder_mentions_including_a_trailing_slash() {
        let index = index();
        let chrome = index.labels.get("src/chrome").unwrap();
        assert_eq!(
            file_mention_parts("look in @src/chrome please", &index.labels),
            vec![
                plain("look in "),
                hit("@src/chrome", chrome),
                plain(" please")
            ]
        );
        let src = index.labels.get("src").unwrap();
        assert_eq!(
            file_mention_parts("look in @src/ next", &index.labels),
            vec![plain("look in "), hit("@src", src), plain("/ next")]
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
        assert_eq!(labels, vec!["Composer.tsx".to_string()]);
        assert_eq!(
            file_mention_parts(text, &index.labels)
                .iter()
                .filter(|part| part.file.is_some())
                .count(),
            1
        );
    }

    // fileMentionsInText
    #[test]
    fn highlights_the_encoded_token_for_a_spaced_path() {
        let files = files();
        assert_eq!(
            file_mention_parts("see @docs/read-me.md now", &index().labels),
            vec![
                plain("see "),
                hit("@docs/read-me.md", &files[3]),
                plain(" now")
            ]
        );
    }

    #[test]
    fn collects_each_referenced_file_once() {
        let labels: Vec<String> = file_mentions_in_text(
            "@Composer.tsx and @apps/web/src/App.tsx and @Composer.tsx",
            &index().labels,
        )
        .into_iter()
        .map(|hit| hit.label)
        .collect();
        assert_eq!(labels, ["Composer.tsx", "apps/web/src/App.tsx"]);
    }

    // withMentionDirectories
    #[test]
    fn derives_unique_parent_folders_from_file_paths() {
        let entries = with_mention_directories(&files());
        let dirs: Vec<&ProjectFile> = entries.iter().filter(|file| file.is_dir).collect();
        for expected in [
            ProjectFile::dir("src", "/p/src", "src"),
            ProjectFile::dir("chrome", "/p/src/chrome", "src/chrome"),
            ProjectFile::dir("apps", "/p/apps", "apps"),
        ] {
            assert!(dirs.contains(&&expected), "{expected:?}");
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
                .any(|file| file.relative == "My Photos" && file.is_dir)
        );
    }

    #[test]
    fn rebuilds_folder_paths_on_windows_separators() {
        let entries = with_mention_directories(&[ProjectFile::new(
            "App.tsx",
            "C:\\p\\apps\\web\\src\\App.tsx",
            "apps/web/src/App.tsx",
        )]);
        let dir = entries
            .iter()
            .find(|file| file.relative == "apps/web")
            .unwrap();
        assert_eq!(
            dir,
            &ProjectFile::dir("web", "C:\\p\\apps\\web", "apps/web")
        );
    }
}
