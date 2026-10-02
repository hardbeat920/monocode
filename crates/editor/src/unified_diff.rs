//! Port of src/features/source-control/model/unifiedDiff.ts, the patch parser
//! in prDiff.ts, the row flattening in unifiedDiffWindow.ts, and the comment
//! location in diffComment.ts.
//!
//! Blocks refer to their lines by index range into [`UnifiedFileDiff::lines`]
//! instead of holding copies.

use std::ops::Range;

use crate::git_diff::{Doc, chunks_for};

pub const UNIFIED_CONTEXT_DEFAULT: usize = 3;
pub const UNIFIED_FOLD_STEP: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnifiedLineKind {
    Add,
    Del,
    Context,
    Hunk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedLine {
    pub kind: UnifiedLineKind,
    pub text: String,
    pub old_number: Option<usize>,
    pub new_number: Option<usize>,
    /// Byte position in the current file, used for hunk stage and revert.
    pub pos: Option<usize>,
}

impl UnifiedLine {
    fn context(text: &str, old_number: usize, new_number: usize) -> Self {
        Self {
            kind: UnifiedLineKind::Context,
            text: text.to_owned(),
            old_number: Some(old_number),
            new_number: Some(new_number),
            pos: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifiedBlock {
    Hunk {
        lines: Range<usize>,
        pos: Option<usize>,
    },
    Fold {
        id: String,
        lines: Range<usize>,
    },
}

impl UnifiedBlock {
    pub fn lines(&self) -> Range<usize> {
        match self {
            Self::Hunk { lines, .. } | Self::Fold { lines, .. } => lines.clone(),
        }
    }

    pub fn is_fold(&self) -> bool {
        matches!(self, Self::Fold { .. })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnifiedFileDiff {
    pub additions: usize,
    pub deletions: usize,
    pub lines: Vec<UnifiedLine>,
    pub blocks: Vec<UnifiedBlock>,
}

/// `buildUnifiedFile`.
pub fn build_unified_file(original: &str, current: &str, context: usize) -> UnifiedFileDiff {
    let lines = unified_lines_from_texts(original, current);
    let additions = lines
        .iter()
        .filter(|line| line.kind == UnifiedLineKind::Add)
        .count();
    let deletions = lines
        .iter()
        .filter(|line| line.kind == UnifiedLineKind::Del)
        .count();
    let blocks = fold_unified_lines(&lines, context);
    UnifiedFileDiff {
        additions,
        deletions,
        lines,
        blocks,
    }
}

/// `blocksFromLines`: blocks for lines that came from a parsed patch.
pub fn blocks_from_lines(lines: Vec<UnifiedLine>, context: usize) -> UnifiedFileDiff {
    let additions = lines
        .iter()
        .filter(|line| line.kind == UnifiedLineKind::Add)
        .count();
    let deletions = lines
        .iter()
        .filter(|line| line.kind == UnifiedLineKind::Del)
        .count();
    let blocks = fold_unified_lines(&lines, context);
    UnifiedFileDiff {
        additions,
        deletions,
        lines,
        blocks,
    }
}

/// `foldUnifiedLines`.
pub fn fold_unified_lines(lines: &[UnifiedLine], context: usize) -> Vec<UnifiedBlock> {
    if lines.is_empty() {
        return Vec::new();
    }
    let visible = visible_context(lines, context);
    let mut blocks = Vec::new();
    let mut index = 0;
    let mut fold_id = 0;
    while index < lines.len() {
        let line = &lines[index];
        if line.kind == UnifiedLineKind::Context && !visible[index] {
            let start = index;
            while index < lines.len()
                && lines[index].kind == UnifiedLineKind::Context
                && !visible[index]
            {
                index += 1;
            }
            blocks.push(UnifiedBlock::Fold {
                id: format!("fold-{fold_id}"),
                lines: start..index,
            });
            fold_id += 1;
            continue;
        }
        let start = index;
        let mut pos = None;
        while index < lines.len() {
            let next = &lines[index];
            let hidden_context = next.kind == UnifiedLineKind::Context && !visible[index];
            if hidden_context {
                break;
            }
            if pos.is_none() && next.pos.is_some() {
                pos = next.pos;
            }
            index += 1;
        }
        blocks.push(UnifiedBlock::Hunk {
            lines: start..index,
            pos,
        });
    }
    blocks
}

/// How much of a fold the user revealed: `start` lines from the top and
/// `end` lines from the bottom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoldReveal {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevealedFold {
    pub head: usize,
    pub tail: usize,
    pub hidden: usize,
}

/// `revealedFold`.
pub fn revealed_fold(total: usize, reveal: Option<FoldReveal>) -> RevealedFold {
    let start = reveal.map_or(0, |reveal| reveal.start).min(total);
    let remaining = total - start;
    let end = reveal.map_or(0, |reveal| reveal.end).min(remaining);
    RevealedFold {
        head: start,
        tail: end,
        hidden: total - start - end,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldDirection {
    Up,
    Down,
    All,
}

/// `expandFold`.
pub fn expand_fold(
    reveal: Option<FoldReveal>,
    total: usize,
    direction: FoldDirection,
    step: usize,
) -> FoldReveal {
    if direction == FoldDirection::All || total == 0 {
        return FoldReveal {
            start: total,
            end: 0,
        };
    }
    let current = reveal.unwrap_or_default();
    match direction {
        FoldDirection::Down => FoldReveal {
            start: current.start + step,
            end: current.end,
        },
        _ => FoldReveal {
            start: current.start,
            end: current.end + step,
        },
    }
}

/// `unifiedLinesFromTexts`.
fn unified_lines_from_texts(original: &str, current: &str) -> Vec<UnifiedLine> {
    let old_doc = Doc::new(original);
    let new_doc = Doc::new(current);
    if original == current {
        return context_lines(&new_doc, &old_doc, 0, new_doc.len(), 0);
    }
    let chunks = chunks_for(original, current);
    if chunks.is_empty() {
        return context_lines(&new_doc, &old_doc, 0, new_doc.len(), 0);
    }

    let mut lines = Vec::new();
    let mut old_pos = 0;
    let mut new_pos = 0;
    for chunk in &chunks {
        if new_pos < chunk.from_b {
            lines.extend(context_lines(
                &new_doc,
                &old_doc,
                new_pos,
                chunk.from_b,
                old_pos,
            ));
        }
        for (text, number) in lines_in_range(&old_doc, chunk.from_a, chunk.to_a) {
            lines.push(UnifiedLine {
                kind: UnifiedLineKind::Del,
                text: text.to_owned(),
                old_number: Some(number),
                new_number: None,
                pos: Some(chunk.from_b),
            });
        }
        for (text, number) in lines_in_range(&new_doc, chunk.from_b, chunk.to_b) {
            lines.push(UnifiedLine {
                kind: UnifiedLineKind::Add,
                text: text.to_owned(),
                old_number: None,
                new_number: Some(number),
                pos: Some(chunk.from_b),
            });
        }
        old_pos = chunk.to_a;
        new_pos = chunk.to_b;
    }
    if new_pos < new_doc.len() {
        lines.extend(context_lines(
            &new_doc,
            &old_doc,
            new_pos,
            new_doc.len(),
            old_pos,
        ));
    }
    lines
}

/// `contextLines`.
fn context_lines(
    new_doc: &Doc,
    old_doc: &Doc,
    from_b: usize,
    to_b: usize,
    from_a: usize,
) -> Vec<UnifiedLine> {
    let inserted = lines_in_range(new_doc, from_b, to_b);
    if inserted.is_empty() {
        return Vec::new();
    }
    let deleted = lines_in_range(old_doc, from_a, from_a + (to_b - from_b));
    inserted
        .iter()
        .enumerate()
        .map(|(index, (text, number))| {
            let old_number = deleted.get(index).map_or(*number, |(_, old)| *old);
            UnifiedLine::context(text, old_number, *number)
        })
        .collect()
}

/// `linesInRange`.
fn lines_in_range<'a>(doc: &Doc<'a>, from: usize, to: usize) -> Vec<(&'a str, usize)> {
    if from >= to || doc.is_empty() {
        return Vec::new();
    }
    let end = from.max(to - 1).min(doc.len().saturating_sub(1));
    let start_line = doc.line_at(from.min(doc.len()));
    let end_line = doc.line_at(end);
    (start_line.number..=end_line.number)
        .map(|number| (doc.line_text(number), number))
        .collect()
}

/// `visibleContext`.
fn visible_context(lines: &[UnifiedLine], context: usize) -> Vec<bool> {
    let mut visible: Vec<bool> = lines
        .iter()
        .map(|line| line.kind != UnifiedLineKind::Context)
        .collect();
    if context == 0 {
        return visible;
    }
    for (index, line) in lines.iter().enumerate() {
        if matches!(line.kind, UnifiedLineKind::Context | UnifiedLineKind::Hunk) {
            continue;
        }
        let from = index.saturating_sub(context);
        let to = (lines.len() - 1).min(index.saturating_add(context));
        for inner in from..=to {
            if lines[inner].kind == UnifiedLineKind::Context {
                visible[inner] = true;
            }
        }
    }
    visible
}

/// One hunk of a file, the unit of the stage, unstage, discard actions.
///
/// A parsed patch has one per `@@` line. A diff built from two texts has one
/// per visible hunk block, with a header made from its line numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    /// Lines of the hunk, header excluded, as an index range into the file's lines.
    pub lines: Range<usize>,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    /// Byte position in the current file of the first change, when known.
    pub pos: Option<usize>,
    /// Whether [`Self::header`] is a real line in [`UnifiedFileDiff::lines`].
    pub parsed: bool,
}

/// Hunks of a file: `@@` sections when the lines have them, else hunk blocks.
pub fn file_hunks(diff: &UnifiedFileDiff) -> Vec<DiffHunk> {
    let lines = &diff.lines;
    if lines.iter().any(|line| line.kind == UnifiedLineKind::Hunk) {
        let mut hunks = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            if lines[index].kind != UnifiedLineKind::Hunk {
                index += 1;
                continue;
            }
            let header = lines[index].text.clone();
            let start = index + 1;
            let mut end = start;
            while end < lines.len() && lines[end].kind != UnifiedLineKind::Hunk {
                end += 1;
            }
            let parsed = parse_hunk_header(&header);
            if let Some((_, old_count, _, new_count)) = parsed {
                // Stop where the header's counts run out, which drops the
                // empty context line `parse_patch` adds at a file's end.
                let (mut old_left, mut new_left) = (old_count, new_count);
                let mut stop = start;
                while stop < end && (old_left > 0 || new_left > 0) {
                    match lines[stop].kind {
                        UnifiedLineKind::Del => old_left = old_left.saturating_sub(1),
                        UnifiedLineKind::Add => new_left = new_left.saturating_sub(1),
                        _ => {
                            old_left = old_left.saturating_sub(1);
                            new_left = new_left.saturating_sub(1);
                        }
                    }
                    stop += 1;
                }
                end = stop;
            }
            let (old_start, old_count, new_start, new_count) =
                parsed.unwrap_or_else(|| counts_for(&lines[start..end]));
            hunks.push(DiffHunk {
                header,
                lines: start..end,
                old_start,
                old_count,
                new_start,
                new_count,
                pos: lines[start..end].iter().find_map(|line| line.pos),
                parsed: true,
            });
            index = end;
        }
        return hunks;
    }
    diff.blocks
        .iter()
        .filter_map(|block| match block {
            UnifiedBlock::Hunk { lines: range, pos } => {
                let slice = &lines[range.clone()];
                if !slice
                    .iter()
                    .any(|line| matches!(line.kind, UnifiedLineKind::Add | UnifiedLineKind::Del))
                {
                    return None;
                }
                let (old_start, old_count, new_start, new_count) = counts_for(slice);
                Some(DiffHunk {
                    header: format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@"),
                    lines: range.clone(),
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    pos: *pos,
                    parsed: false,
                })
            }
            UnifiedBlock::Fold { .. } => None,
        })
        .collect()
}

/// Old and new start lines and counts for a run of lines, like a `@@` header.
fn counts_for(lines: &[UnifiedLine]) -> (usize, usize, usize, usize) {
    let old_count = lines
        .iter()
        .filter(|line| matches!(line.kind, UnifiedLineKind::Context | UnifiedLineKind::Del))
        .count();
    let new_count = lines
        .iter()
        .filter(|line| matches!(line.kind, UnifiedLineKind::Context | UnifiedLineKind::Add))
        .count();
    let old_start = lines
        .iter()
        .find_map(|line| line.old_number)
        .unwrap_or_else(|| {
            // A pure insertion: git numbers it after the line before it.
            lines
                .iter()
                .find_map(|line| line.new_number)
                .map_or(0, |number| number.saturating_sub(1))
        });
    let new_start = lines
        .iter()
        .find_map(|line| line.new_number)
        .unwrap_or_else(|| {
            lines
                .iter()
                .find_map(|line| line.old_number)
                .map_or(0, |number| number.saturating_sub(1))
        });
    (old_start, old_count, new_start, new_count)
}

fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = header.strip_prefix("@@")?.trim_start();
    let mut parts = rest.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |value: &str| -> Option<(usize, usize)> {
        match value.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((value.parse().ok()?, 1)),
        }
    };
    let (old_start, old_count) = range(old)?;
    let (new_start, new_count) = range(new)?;
    Some((old_start, old_count, new_start, new_count))
}

/// A patch with one hunk, for `git apply` (add `--cached` to stage, `--reverse`
/// to unstage or discard).
pub fn hunk_patch(
    path: &str,
    old_path: Option<&str>,
    diff: &UnifiedFileDiff,
    hunk: &DiffHunk,
) -> String {
    let old_path = old_path.unwrap_or(path);
    let mut out = format!("diff --git a/{old_path} b/{path}\n--- a/{old_path}\n+++ b/{path}\n");
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
    ));
    for line in &diff.lines[hunk.lines.clone()] {
        let prefix = match line.kind {
            UnifiedLineKind::Add => '+',
            UnifiedLineKind::Del => '-',
            UnifiedLineKind::Context => ' ',
            UnifiedLineKind::Hunk => continue,
        };
        out.push(prefix);
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchStatus {
    Added,
    Deleted,
    Renamed,
    Modified,
}

/// `PrDiffFile`: one file of a parsed patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: PatchStatus,
    pub binary: bool,
    pub additions: usize,
    pub deletions: usize,
    pub lines: Vec<UnifiedLine>,
}

/// `parsePrPatch`: split `git diff` output into files.
pub fn parse_patch(patch: &str) -> Vec<PatchFile> {
    let text = patch.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut files = Vec::new();
    // `text.split(/^diff --git /m)`.
    let mut parts = Vec::new();
    let mut start = 0;
    let mut search = 0;
    while let Some(found) = text[search..].find("diff --git ") {
        let at = search + found;
        if at == 0 || text.as_bytes()[at - 1] == b'\n' {
            parts.push(&text[start..at]);
            start = at + "diff --git ".len();
        }
        search = at + "diff --git ".len();
    }
    parts.push(&text[start..]);
    for part in parts {
        if part.trim().is_empty() {
            continue;
        }
        if let Some(file) = parse_patch_file(&format!("diff --git {part}")) {
            files.push(file);
        }
    }
    files
}

/// `parsePrFile`.
fn parse_patch_file(block: &str) -> Option<PatchFile> {
    let mut path = String::new();
    let mut previous_path: Option<String> = None;
    let mut status = PatchStatus::Modified;
    let mut binary = false;
    let mut additions = 0;
    let mut deletions = 0;
    let mut diff_lines = Vec::new();
    let mut old_num = 0;
    let mut new_num = 0;
    let mut in_hunk = false;

    for line in block.split('\n') {
        if line.starts_with("diff --git ") {
            if let Some((old, new)) = parse_git_paths(line) {
                previous_path = (old != new).then_some(old);
                path = new;
            }
            continue;
        }
        if line.starts_with("new file mode") {
            status = PatchStatus::Added;
            continue;
        }
        if line.starts_with("deleted file mode") {
            status = PatchStatus::Deleted;
            continue;
        }
        if let Some(rest) = line.strip_prefix("rename from ") {
            previous_path = Some(unquote_diff_path(rest));
            status = PatchStatus::Renamed;
            continue;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            path = unquote_diff_path(rest);
            status = PatchStatus::Renamed;
            continue;
        }
        if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            binary = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("--- ") {
            let next = strip_diff_path(rest);
            if next == "/dev/null" {
                if status == PatchStatus::Modified {
                    status = PatchStatus::Added;
                }
            } else if !next.is_empty() && previous_path.is_none() && next != path {
                previous_path = Some(next);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            let next = strip_diff_path(rest);
            if next == "/dev/null" {
                status = PatchStatus::Deleted;
            } else if !next.is_empty() {
                path = next;
            }
            continue;
        }
        if let Some((old, new)) = hunk_start(line) {
            in_hunk = true;
            old_num = old;
            new_num = new;
            diff_lines.push(UnifiedLine {
                kind: UnifiedLineKind::Hunk,
                text: line.to_owned(),
                old_number: None,
                new_number: None,
                pos: None,
            });
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(text) = line.strip_prefix('+') {
            additions += 1;
            diff_lines.push(UnifiedLine {
                kind: UnifiedLineKind::Add,
                text: text.to_owned(),
                old_number: None,
                new_number: Some(new_num),
                pos: None,
            });
            new_num += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            deletions += 1;
            diff_lines.push(UnifiedLine {
                kind: UnifiedLineKind::Del,
                text: text.to_owned(),
                old_number: Some(old_num),
                new_number: None,
                pos: None,
            });
            old_num += 1;
        } else if line.starts_with('\\') {
            continue;
        } else {
            // TODO(port): the block ends in "\n", so its last split piece is
            // an empty string that lands here as an extra context line, as in
            // prDiff.ts. `file_hunks` stops at the `@@` counts, so patches
            // built from hunks are not affected.
            let text = line.strip_prefix(' ').unwrap_or(line);
            diff_lines.push(UnifiedLine::context(text, old_num, new_num));
            old_num += 1;
            new_num += 1;
        }
    }

    if path.is_empty() {
        return None;
    }
    if status != PatchStatus::Renamed || previous_path.as_deref() == Some(path.as_str()) {
        previous_path = None;
    }
    Some(PatchFile {
        path,
        previous_path,
        status,
        binary,
        additions,
        deletions,
        lines: diff_lines,
    })
}

/// `/^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)/`.
fn hunk_start(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@")?;
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
    let rest = rest.strip_prefix('-')?;
    let digits = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    let old: usize = rest[..digits].parse().ok()?;
    let mut rest = &rest[digits..];
    if let Some(after) = rest.strip_prefix(',') {
        let count = after
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(after.len());
        if count == 0 {
            return None;
        }
        rest = &after[count..];
    }
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
    let rest = rest.strip_prefix('+')?;
    let digits = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    let new: usize = rest[..digits].parse().ok()?;
    Some((old, new))
}

/// `parseGitPaths`.
fn parse_git_paths(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("diff --git ")?.trim();
    if let Some(inner) = rest.strip_prefix("\"a/").and_then(|r| r.strip_suffix('"'))
        && let Some((old, new)) = inner.split_once("\" \"b/")
    {
        return Some((unquote_diff_path(old), unquote_diff_path(new)));
    }
    // `/^a\/(.*) b\/(.*)$/` is greedy, so the split is at the last " b/".
    let inner = rest.strip_prefix("a/")?;
    let split = inner.rfind(" b/")?;
    Some((inner[..split].to_owned(), inner[split + 3..].to_owned()))
}

/// `stripDiffPath`.
fn strip_diff_path(raw: &str) -> String {
    let mut value = raw.trim();
    if let Some(tab) = value.find('\t') {
        value = &value[..tab];
    }
    let mut owned = value.to_owned();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        owned = unquote_diff_path(&value[1..value.len() - 1]);
    }
    if owned == "/dev/null" {
        return owned;
    }
    if let Some(stripped) = owned
        .strip_prefix("a/")
        .or_else(|| owned.strip_prefix("b/"))
    {
        return stripped.to_owned();
    }
    owned
}

/// `unquoteDiffPath`.
fn unquote_diff_path(value: &str) -> String {
    value.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// `DiffCommentTarget`: one line a comment is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffCommentTarget {
    pub path: String,
    pub line: UnifiedLine,
}

impl DiffCommentTarget {
    /// `diffCommentLine`.
    pub fn line_number(&self) -> Option<usize> {
        if self.line.kind == UnifiedLineKind::Del {
            self.line.old_number
        } else {
            self.line.new_number
        }
    }

    /// `diffCommentLocation`: `path:line`.
    pub fn location(&self) -> String {
        match self.line_number() {
            Some(number) => format!("{}:{number}", self.path),
            None => self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(diff: &UnifiedFileDiff, range: Range<usize>) -> Vec<&str> {
        diff.lines[range]
            .iter()
            .map(|line| line.text.as_str())
            .collect()
    }

    fn range_context(from: usize, to: usize) -> Vec<UnifiedLine> {
        (from..=to)
            .map(|number| UnifiedLine::context(&format!("c{number}"), number, number))
            .collect()
    }

    fn change(kind: UnifiedLineKind, text: &str, number: usize) -> UnifiedLine {
        UnifiedLine {
            kind,
            text: text.into(),
            old_number: (kind == UnifiedLineKind::Del).then_some(number),
            new_number: (kind == UnifiedLineKind::Add).then_some(number),
            pos: None,
        }
    }

    // describe("buildUnifiedFile")

    #[test]
    fn marks_a_replacement_as_a_delete_then_an_add() {
        let diff = build_unified_file("alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\n", 3);
        assert_eq!(diff.additions, 1);
        assert_eq!(diff.deletions, 1);
        let changed: Vec<_> = diff
            .lines
            .iter()
            .filter(|line| line.kind != UnifiedLineKind::Context)
            .map(|line| {
                (
                    line.kind,
                    line.text.as_str(),
                    line.old_number,
                    line.new_number,
                )
            })
            .collect();
        assert_eq!(
            changed,
            vec![
                (UnifiedLineKind::Del, "beta", Some(2), None),
                (UnifiedLineKind::Add, "BETA", None, Some(2)),
            ]
        );
    }

    #[test]
    fn treats_a_new_file_as_additions() {
        let diff = build_unified_file("", "one\ntwo\n", 3);
        assert_eq!(diff.deletions, 0);
        assert!(diff.additions > 0);
        assert!(
            diff.lines
                .iter()
                .all(|line| line.kind == UnifiedLineKind::Add)
        );
    }

    #[test]
    fn treats_a_deleted_file_as_deletions() {
        let diff = build_unified_file("one\ntwo\n", "", 3);
        assert_eq!(diff.additions, 0);
        assert!(diff.deletions > 0);
        assert!(
            diff.lines
                .iter()
                .all(|line| line.kind == UnifiedLineKind::Del)
        );
    }

    #[test]
    fn folds_unmodified_runs_outside_the_context_window() {
        let original = (1..=20)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let current = original.replace("line-10", "LINE-10");
        let diff = build_unified_file(&original, &current, 3);
        let folds: Vec<_> = diff.blocks.iter().filter(|block| block.is_fold()).collect();
        assert_eq!(folds.len(), 2);
        let first = texts(&diff, folds[0].lines());
        assert_eq!(first.first(), Some(&"line-1"));
        assert_eq!(first.last(), Some(&"line-6"));
        assert_eq!(texts(&diff, folds[1].lines()).first(), Some(&"line-14"));
    }

    #[test]
    fn keeps_a_tiny_file_fully_visible_when_every_line_is_near_a_change() {
        let diff = build_unified_file("a\nb\nc\n", "a\nB\nc\n", 3);
        assert!(diff.blocks.iter().all(|block| !block.is_fold()));
    }

    #[test]
    fn returns_no_add_del_for_identical_files() {
        let diff = build_unified_file("same\nfile\n", "same\nfile\n", 3);
        assert_eq!(diff.additions, 0);
        assert_eq!(diff.deletions, 0);
        assert!(
            diff.lines
                .iter()
                .all(|line| line.kind == UnifiedLineKind::Context)
        );
    }

    // describe("foldUnifiedLines")

    fn fold_fixture() -> Vec<UnifiedLine> {
        let mut lines = range_context(1, 8);
        lines.push(change(UnifiedLineKind::Del, "old", 9));
        lines.push(change(UnifiedLineKind::Add, "new", 9));
        lines.extend(range_context(10, 16));
        lines
    }

    #[test]
    fn splits_long_context_around_a_change_into_folds() {
        let lines = fold_fixture();
        let blocks = fold_unified_lines(&lines, 2);
        assert_eq!(
            blocks.iter().map(UnifiedBlock::is_fold).collect::<Vec<_>>(),
            vec![true, false, true]
        );
        let hunk: Vec<_> = lines[blocks[1].lines()]
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(hunk, vec!["c7", "c8", "old", "new", "c10", "c11"]);
    }

    #[test]
    fn keeps_every_line_visible_when_context_is_unlimited() {
        let lines = fold_fixture();
        let blocks = fold_unified_lines(&lines, usize::MAX);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines(), 0..lines.len());
    }

    // describe("blocksFromLines")

    #[test]
    fn keeps_hunk_headers_visible_so_they_split_folds() {
        let diff = blocks_from_lines(
            vec![
                UnifiedLine {
                    kind: UnifiedLineKind::Hunk,
                    text: "@@ -1,3 +1,3 @@".into(),
                    old_number: None,
                    new_number: None,
                    pos: None,
                },
                UnifiedLine::context("keep", 1, 1),
                change(UnifiedLineKind::Add, "plus", 2),
            ],
            1,
        );
        assert!(!diff.blocks[0].is_fold());
    }

    // describe("fold expansion")

    #[test]
    fn reveals_from_the_start_when_expanding_down() {
        assert_eq!(
            revealed_fold(10, Some(expand_fold(None, 10, FoldDirection::Down, 3))),
            RevealedFold {
                head: 3,
                tail: 0,
                hidden: 7
            }
        );
    }

    #[test]
    fn reveals_from_the_end_when_expanding_up() {
        let next = expand_fold(
            Some(FoldReveal { start: 2, end: 0 }),
            10,
            FoldDirection::Up,
            3,
        );
        assert_eq!(
            revealed_fold(10, Some(next)),
            RevealedFold {
                head: 2,
                tail: 3,
                hidden: 5
            }
        );
    }

    #[test]
    fn expands_the_whole_fold_at_once() {
        assert_eq!(
            revealed_fold(
                12,
                Some(expand_fold(None, 12, FoldDirection::All, UNIFIED_FOLD_STEP))
            ),
            RevealedFold {
                head: 12,
                tail: 0,
                hidden: 0
            }
        );
    }

    // describe("parsePrPatch")

    const SAMPLE: &str = "diff --git a/next.config.ts b/next.config.ts
index 1111111..2222222 100644
--- a/next.config.ts
+++ b/next.config.ts
@@ -1,6 +1,10 @@
 import type { NextConfig } from \"next\";

 const nextConfig: NextConfig = {
+  cacheComponents: true,
+  partialPrefetching: true,
   transpilePackages: [\"gl-transition\"],
 };
";

    #[test]
    fn reads_added_lines_and_line_numbers() {
        let files = parse_patch(SAMPLE);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "next.config.ts");
        assert_eq!(files[0].status, PatchStatus::Modified);
        assert_eq!(files[0].additions, 2);
        assert_eq!(files[0].deletions, 0);
        let added: Vec<_> = files[0]
            .lines
            .iter()
            .filter(|line| line.kind == UnifiedLineKind::Add)
            .collect();
        assert_eq!(
            added
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["  cacheComponents: true,", "  partialPrefetching: true,"]
        );
        assert_eq!(added[0].new_number, Some(4));
        assert_eq!(added[0].old_number, None);
    }

    #[test]
    fn marks_new_and_deleted_files() {
        let files = parse_patch(
            "diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..abc
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index abc..0000000
--- a/gone.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-bye
",
        );
        let summary: Vec<_> = files
            .iter()
            .map(|file| {
                (
                    file.path.as_str(),
                    file.status,
                    file.additions,
                    file.deletions,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("new.txt", PatchStatus::Added, 2, 0),
                ("gone.txt", PatchStatus::Deleted, 0, 1),
            ]
        );
    }

    #[test]
    fn reads_renames_and_binary_files() {
        let files = parse_patch(
            "diff --git a/old.ts b/new.ts
similarity index 90%
rename from old.ts
rename to new.ts
diff --git a/photo.png b/photo.png
index 111..222
Binary files a/photo.png and b/photo.png differ
",
        );
        assert_eq!(files[0].path, "new.ts");
        assert_eq!(files[0].previous_path.as_deref(), Some("old.ts"));
        assert_eq!(files[0].status, PatchStatus::Renamed);
        assert_eq!(files[1].path, "photo.png");
        assert!(files[1].binary);
        assert!(files[1].lines.is_empty());
    }

    #[test]
    fn returns_an_empty_list_for_a_blank_patch() {
        assert!(parse_patch("").is_empty());
        assert!(parse_patch("   \n").is_empty());
    }

    // Port-specific cases.

    #[test]
    fn hunks_of_a_parsed_patch_follow_its_headers() {
        let files = parse_patch(SAMPLE);
        let diff = blocks_from_lines(files[0].lines.clone(), UNIFIED_CONTEXT_DEFAULT);
        let hunks = file_hunks(&diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!((hunks[0].old_start, hunks[0].old_count), (1, 6));
        assert_eq!((hunks[0].new_start, hunks[0].new_count), (1, 10));
        assert!(hunks[0].parsed);
        let patch = hunk_patch("next.config.ts", None, &diff, &hunks[0]);
        assert!(patch.contains("@@ -1,6 +1,10 @@\n import type"));
        assert!(patch.contains("\n+  cacheComponents: true,\n"));
    }

    #[test]
    fn a_hunk_stops_where_its_header_counts_end() {
        let files =
            parse_patch("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,2 +1,2 @@\n x\n-y\n+Y\n");
        let diff = blocks_from_lines(files[0].lines.clone(), UNIFIED_CONTEXT_DEFAULT);
        let hunks = file_hunks(&diff);
        // The empty piece after the last newline is parsed as context, but
        // it is not part of the hunk.
        assert_eq!(
            diff.lines.last().map(|line| line.kind),
            Some(UnifiedLineKind::Context)
        );
        assert_eq!(hunks[0].lines, 1..4);
        assert!(hunk_patch("a", None, &diff, &hunks[0]).ends_with("@@ -1,2 +1,2 @@\n x\n-y\n+Y\n"));
    }

    #[test]
    fn hunks_of_a_text_diff_get_headers_from_their_lines() {
        let original = (1..=20).map(|i| format!("l{i}\n")).collect::<String>();
        let current = original.replace("l10\n", "L10\n");
        let diff = build_unified_file(&original, &current, 3);
        let hunks = file_hunks(&diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].header, "@@ -7,7 +7,7 @@");
        assert!(!hunks[0].parsed);
        assert_eq!(hunks[0].pos, Some(current.find("L10").unwrap()));
    }

    #[test]
    fn comment_location_uses_the_side_of_the_line() {
        let target = DiffCommentTarget {
            path: "src/a.rs".into(),
            line: change(UnifiedLineKind::Del, "x", 7),
        };
        assert_eq!(target.location(), "src/a.rs:7");
    }

    #[test]
    fn quoted_and_spaced_paths_parse() {
        assert_eq!(
            parse_git_paths("diff --git a/my file.txt b/my file.txt"),
            Some(("my file.txt".into(), "my file.txt".into()))
        );
        assert_eq!(
            parse_git_paths("diff --git \"a/q\\\"x\" \"b/q\\\"x\""),
            Some(("q\"x".into(), "q\"x".into()))
        );
    }
}
