//! Port of the pure parts of src/features/files/editor/editorGit.ts.
//!
//! CodeMirror builds its chunks with `@codemirror/merge`, which diffs
//! characters and then widens each change to whole lines. This port diffs
//! whole lines (each line keeps its `\n`) with `similar`, which gives the same
//! line-aligned chunks for ordinary edits.
//!
//! All offsets are byte offsets into UTF-8 text, the unit gpui-base's editor
//! uses. CodeMirror used UTF-16 offsets.

use std::ops::Range;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffOp};

/// `DIFF_CONFIG.timeout` in editorGit.ts.
const DIFF_TIMEOUT: Duration = Duration::from_millis(100);

/// A line-aligned change between the original text (A) and the buffer (B).
///
/// `from_*` is the start of the first line. `to_*` equals `from_*` when the
/// chunk covers no lines on that side, otherwise it is one past the newline of
/// the last line, so it can be `len + 1` on a last line without a newline.
/// This matches `Chunk` in `@codemirror/merge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub from_a: usize,
    pub to_a: usize,
    pub from_b: usize,
    pub to_b: usize,
}

impl Chunk {
    /// `from_a` when the chunk is empty in A, otherwise the end of its last line.
    pub fn end_a(&self) -> usize {
        self.from_a.max(self.to_a.saturating_sub(1))
    }

    /// `from_b` when the chunk is empty in B, otherwise the end of its last line.
    pub fn end_b(&self) -> usize {
        self.from_b.max(self.to_b.saturating_sub(1))
    }

    pub fn is_insertion(&self) -> bool {
        self.from_b != self.to_b
    }

    pub fn is_deletion(&self) -> bool {
        self.from_a != self.to_a
    }

    pub fn kind(&self) -> ChangeKind {
        match (self.is_insertion(), self.is_deletion()) {
            (true, true) => ChangeKind::Modified,
            (false, true) => ChangeKind::Deleted,
            _ => ChangeKind::Added,
        }
    }
}

/// The kind of a chunk, as the overview ruler and the gutter show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
}

/// A byte range in the buffer, like CodeMirror's `{ from, to }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub from: usize,
    pub to: usize,
}

impl From<Range<usize>> for TextRange {
    fn from(range: Range<usize>) -> Self {
        Self {
            from: range.start,
            to: range.end,
        }
    }
}

/// One replacement: put `insert` over `from..to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

impl TextChange {
    pub fn apply(&self, text: &str) -> String {
        let to = self.to.min(text.len());
        let from = self.from.min(to);
        let mut out = String::with_capacity(text.len() + self.insert.len());
        out.push_str(&text[..from]);
        out.push_str(&self.insert);
        out.push_str(&text[to..]);
        out
    }
}

/// One line of a [`Doc`], with a 1-based number like CodeMirror's `Line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub number: usize,
    pub from: usize,
    /// End of the line, before its `\n`.
    pub to: usize,
}

/// Line index over a text, the subset of CodeMirror's `Text` this port needs.
///
/// Lines split on `\n`, so a text ending in `\n` has a last empty line, the
/// same as `Text.of(value.split("\n"))`.
#[derive(Debug, Clone)]
pub struct Doc<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Doc<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self { text, starts }
    }

    pub fn text(&self) -> &'a str {
        self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Number of lines, at least 1.
    pub fn lines(&self) -> usize {
        self.starts.len()
    }

    /// Line `number`, 1-based. Clamps to the last line.
    pub fn line(&self, number: usize) -> Line {
        let index = number.clamp(1, self.lines()) - 1;
        let from = self.starts[index];
        let to = self
            .starts
            .get(index + 1)
            .map_or(self.text.len(), |next| next - 1);
        Line {
            number: index + 1,
            from,
            to,
        }
    }

    /// The line that contains `pos`.
    pub fn line_at(&self, pos: usize) -> Line {
        let pos = pos.min(self.text.len());
        let index = self.starts.partition_point(|start| *start <= pos) - 1;
        self.line(index + 1)
    }

    /// Text of line `number`.
    pub fn line_text(&self, number: usize) -> &'a str {
        let line = self.line(number);
        &self.text[line.from..line.to]
    }

    /// `sliceString`, clamped to the text and to char boundaries.
    pub fn slice(&self, from: usize, to: usize) -> &'a str {
        let to = floor_char_boundary(self.text, to.min(self.text.len()));
        let from = floor_char_boundary(self.text, from.min(to));
        &self.text[from..to]
    }

    /// Byte offset where token (line with its newline) `index` starts.
    fn token_start(&self, index: usize) -> usize {
        self.starts.get(index).copied().unwrap_or(self.text.len())
    }

    /// One past the newline of token `index`.
    fn token_end(&self, index: usize) -> usize {
        match self.starts.get(index + 1) {
            Some(next) => *next,
            None => self.text.len() + 1,
        }
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Lines with their terminators, the diff tokens.
fn tokens(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// `chunksFor` / `Chunk.build`: the chunks that turn `original` into `current`.
pub fn chunks_for(original: &str, current: &str) -> Vec<Chunk> {
    if original == current {
        return Vec::new();
    }
    let old_doc = Doc::new(original);
    let new_doc = Doc::new(current);
    let old_tokens = tokens(original);
    let new_tokens = tokens(current);
    let deadline = Instant::now() + DIFF_TIMEOUT;
    let ops = similar::capture_diff_slices_deadline(
        Algorithm::Myers,
        &old_tokens,
        &new_tokens,
        Some(deadline),
    );

    let mut chunks = Vec::new();
    // Old and new token ranges of the chunk being built.
    let mut pending: Option<(Range<usize>, Range<usize>)> = None;
    let mut flush = |pending: &mut Option<(Range<usize>, Range<usize>)>| {
        if let Some((old, new)) = pending.take() {
            let from_a = old_doc.token_start(old.start);
            let to_a = if old.is_empty() {
                from_a
            } else {
                old_doc.token_end(old.end - 1)
            };
            let from_b = new_doc.token_start(new.start);
            let to_b = if new.is_empty() {
                from_b
            } else {
                new_doc.token_end(new.end - 1)
            };
            chunks.push(Chunk {
                from_a,
                to_a,
                from_b,
                to_b,
            });
        }
    };
    for op in ops {
        let (old, new) = match op {
            DiffOp::Equal { .. } => {
                flush(&mut pending);
                continue;
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index..old_index + old_len, new_index..new_index),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index..old_index + old_len,
                new_index..new_index + new_len,
            ),
        };
        pending = Some(match pending.take() {
            Some((pending_old, pending_new)) => (
                pending_old.start..old.end.max(pending_old.end),
                pending_new.start..new.end.max(pending_new.end),
            ),
            None => (old, new),
        });
    }
    flush(&mut pending);
    chunks
}

/// `findChunk`: the chunk that covers `pos`, or a pure deletion on its line.
pub fn find_chunk<'c>(doc: &Doc, chunks: &'c [Chunk], pos: usize) -> Option<&'c Chunk> {
    let at = pos.min(doc.len());
    if let Some(covering) = chunks
        .iter()
        .find(|chunk| chunk.from_b <= at && chunk.end_b() >= at)
    {
        return Some(covering);
    }
    if doc.is_empty() {
        return chunks.first();
    }
    let line = doc.line_at(at);
    chunks.iter().find(|chunk| {
        if chunk.from_b != chunk.to_b {
            return false;
        }
        chunk.from_b >= line.from && chunk.from_b <= line.to + 1
    })
}

/// `Math.max(pos, Math.min(chunk.endB, doc.length) - 1)`, a position on the
/// last inserted line.
fn last_inserted_pos(doc: &Doc, chunk: &Chunk, pos: usize) -> usize {
    pos.max(chunk.end_b().min(doc.len()).saturating_sub(1))
}

/// `widgetPos`: where the deleted lines of a chunk show, and where its actions apply.
pub fn widget_pos(doc: &Doc, chunk: &Chunk) -> usize {
    if doc.is_empty() {
        return 0;
    }
    chunk.from_b.min(doc.len())
}

/// `deletedLineTexts`: the original lines a chunk removes.
pub fn deleted_line_texts(original: &Doc, chunk: &Chunk) -> Vec<String> {
    if chunk.from_a == chunk.to_a {
        return Vec::new();
    }
    original
        .slice(chunk.from_a, chunk.from_a.max(chunk.to_a.saturating_sub(1)))
        .split('\n')
        .map(str::to_owned)
        .collect()
}

/// `diffLineStats`: added and removed line counts.
pub fn diff_line_stats(doc: &Doc, chunks: &[Chunk], original: Option<&Doc>) -> (usize, usize) {
    let Some(original) = original else {
        return (0, 0);
    };
    if chunks.is_empty() {
        return (0, 0);
    }
    let mut additions = 0;
    let mut deletions = 0;
    for chunk in chunks {
        if chunk.is_deletion() {
            deletions += deleted_line_texts(original, chunk).len();
        }
        if chunk.is_insertion() {
            let pos = chunk.from_b.min(doc.len());
            let start_line = doc.line_at(pos).number;
            let end_pos = last_inserted_pos(doc, chunk, pos);
            additions += doc.line_at(end_pos).number - start_line + 1;
        }
    }
    (additions, deletions)
}

/// One mark on the overview ruler. `top` and `size` are fractions of the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewTick {
    pub kind: ChangeKind,
    pub top: f32,
    pub size: f32,
    pub pos: usize,
}

/// `overviewTicks`.
pub fn overview_ticks(doc: &Doc, chunks: &[Chunk], original: Option<&Doc>) -> Vec<OverviewTick> {
    let total = doc.lines().max(1) as f32;
    let mut ticks = Vec::new();
    for chunk in chunks {
        let insertion = chunk.is_insertion();
        let deletion = chunk.is_deletion();
        if !insertion && !deletion {
            continue;
        }
        let pos = chunk.from_b.min(doc.len());
        let start_line = doc.line_at(pos).number;
        let mut line_count = 1;
        if insertion {
            let end_pos = last_inserted_pos(doc, chunk, pos);
            line_count = (doc.line_at(end_pos).number - start_line + 1).max(1);
        } else if let Some(original) = original {
            line_count = deleted_line_texts(original, chunk).len().max(1);
        }
        ticks.push(OverviewTick {
            kind: chunk.kind(),
            top: (start_line - 1) as f32 / total,
            size: line_count as f32 / total,
            pos,
        });
    }
    ticks
}

/// `navigableChunkPositions`.
pub fn navigable_chunk_positions(
    doc: &Doc,
    chunks: &[Chunk],
    original: Option<&Doc>,
) -> Vec<usize> {
    overview_ticks(doc, chunks, original)
        .into_iter()
        .map(|tick| tick.pos)
        .collect()
}

/// Buffer lines (1-based, inclusive) a chunk marks in the gutter.
///
/// An insertion marks its inserted lines. A pure deletion marks the line it
/// sits on, so the marker has a row to draw in.
pub fn marked_lines(doc: &Doc, chunk: &Chunk) -> Range<usize> {
    let start = doc.line_at(widget_pos(doc, chunk)).number;
    if !chunk.is_insertion() {
        return start..start + 1;
    }
    let last = last_inserted_pos(doc, chunk, chunk.from_b.min(doc.len()));
    let end = doc.line_at(last).number.max(start);
    start..end + 1
}

/// `revertChunkText`: the buffer with the chunk at `pos` put back to the original.
pub fn revert_chunk_text(
    original: &str,
    current: &str,
    pos: usize,
    selection: Option<TextRange>,
) -> Option<String> {
    let change = revert_chunk_change(original, current, pos, selection)?;
    Some(change.apply(current))
}

/// `stageChunkText`: the original with the chunk at `pos` applied, the index
/// contents to stage.
pub fn stage_chunk_text(
    original: &str,
    current: &str,
    pos: usize,
    selection: Option<TextRange>,
) -> Option<String> {
    let change = stage_chunk_change(original, current, pos, selection)?;
    Some(change.apply(original))
}

/// `revertChunkChanges`: the edit to the buffer that reverts the chunk at `pos`.
pub fn revert_chunk_change(
    original: &str,
    current: &str,
    pos: usize,
    selection: Option<TextRange>,
) -> Option<TextChange> {
    let orig = Doc::new(original);
    let doc = Doc::new(current);
    let range = action_chunk_range(&orig, &doc, pos, selection)?;
    Some(apply_side(
        &orig,
        &doc,
        range.from_a,
        range.to_a,
        range.from_b,
        range.to_b,
        "\n",
    ))
}

/// `stageChunkChanges`: the edit to the original that stages the chunk at `pos`.
pub fn stage_chunk_change(
    original: &str,
    current: &str,
    pos: usize,
    selection: Option<TextRange>,
) -> Option<TextChange> {
    let orig = Doc::new(original);
    let doc = Doc::new(current);
    let range = action_chunk_range(&orig, &doc, pos, selection)?;
    Some(apply_side(
        &doc,
        &orig,
        range.from_b,
        range.to_b,
        range.from_a,
        range.to_a,
        "\n",
    ))
}

fn apply_side(
    source: &Doc,
    target: &Doc,
    from_s: usize,
    to_s: usize,
    from_t: usize,
    to_t: usize,
    line_break: &str,
) -> TextChange {
    let mut insert = source
        .slice(from_s, from_s.max(to_s.saturating_sub(1)))
        .to_owned();
    if from_s != to_s && to_t <= target.len() {
        insert.push_str(line_break);
    }
    TextChange {
        from: from_t,
        to: target.len().min(to_t),
        insert,
    }
}

fn action_chunk_range(
    original: &Doc,
    doc: &Doc,
    pos: usize,
    selection: Option<TextRange>,
) -> Option<Chunk> {
    let chunks = chunks_for(original.text(), doc.text());
    let chunk = find_chunk(doc, &chunks, pos)?;
    Some(narrow_chunk(original, doc, chunk, selection))
}

/// `narrowChunk`: limit a chunk to the selected lines when the selection
/// covers part of it.
fn narrow_chunk(original: &Doc, doc: &Doc, chunk: &Chunk, selection: Option<TextRange>) -> Chunk {
    let whole = *chunk;
    let selected = selected_lines(doc, selection);
    let hunk_b = hunk_lines(doc, chunk.from_b, chunk.to_b, chunk.end_b());
    let (Some(selected), Some(hunk_b)) = (selected, hunk_b) else {
        return whole;
    };

    let from_line = selected.0.max(hunk_b.0);
    let to_line = selected.1.min(hunk_b.1);
    if from_line > to_line {
        return whole;
    }
    if from_line == hunk_b.0 && to_line == hunk_b.1 {
        return whole;
    }

    let next_b = offsets_for_lines(doc, from_line, to_line);
    let Some(hunk_a) = hunk_lines(original, chunk.from_a, chunk.to_a, chunk.end_a()) else {
        return Chunk {
            from_a: chunk.from_a,
            to_a: chunk.to_a,
            from_b: next_b.0,
            to_b: next_b.1,
        };
    };

    let a_count = hunk_a.1 - hunk_a.0 + 1;
    let b_count = hunk_b.1 - hunk_b.0 + 1;
    if a_count == b_count {
        let delta = from_line - hunk_b.0;
        let length = to_line - from_line;
        let a_from_line = hunk_a.0 + delta;
        let a_to_line = a_from_line + length;
        let next_a = offsets_for_lines(original, a_from_line, a_to_line);
        return Chunk {
            from_a: next_a.0,
            to_a: next_a.1,
            from_b: next_b.0,
            to_b: next_b.1,
        };
    }

    Chunk {
        from_a: chunk.from_a,
        to_a: chunk.to_a,
        from_b: next_b.0,
        to_b: next_b.1,
    }
}

/// `selectedLines`: first and last selected line numbers.
fn selected_lines(doc: &Doc, range: Option<TextRange>) -> Option<(usize, usize)> {
    let range = range?;
    if range.from == range.to {
        return None;
    }
    let from = range.from.min(range.to);
    let to = range.from.max(range.to);
    let start = doc.line_at(from.min(doc.len())).number;
    let mut end = doc.line_at(to.min(doc.len())).number;
    if to > from && doc.line_at(to.min(doc.len())).from == to {
        end = start.max(end - 1);
    }
    Some((start, end))
}

/// `hunkLines`: first and last line numbers a chunk side covers.
fn hunk_lines(doc: &Doc, from: usize, to: usize, end: usize) -> Option<(usize, usize)> {
    if from == to {
        return None;
    }
    if doc.is_empty() {
        return Some((1, 1));
    }
    let start = from.min(doc.len());
    let last = start.max(end.min(doc.len()).saturating_sub(1));
    Some((doc.line_at(start).number, doc.line_at(last).number))
}

/// `offsetsForLines`.
fn offsets_for_lines(doc: &Doc, from_line: usize, to_line: usize) -> (usize, usize) {
    let from = doc.line(from_line).from;
    let last = doc.line(to_line);
    (from, last.to + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_from(text: &str, number: usize) -> usize {
        Doc::new(text).line(number).from
    }

    // describe("stageChunkText")

    #[test]
    fn stages_a_whole_added_hunk_and_leaves_later_hunks_unstaged() {
        let original = "alpha\nbeta\ngamma\ndelta\n";
        let current = "alpha\nBETA\ngamma\nDELTA\n";
        let beta = line_from(current, 2);
        assert_eq!(
            stage_chunk_text(original, current, beta, None).as_deref(),
            Some("alpha\nBETA\ngamma\ndelta\n")
        );
    }

    #[test]
    fn stages_selected_added_lines_and_excludes_the_rest_of_the_hunk() {
        let original = "alpha\ngamma\n";
        let current = "alpha\nbeta\ndelta\ngamma\n";
        let beta = Doc::new(current).line(2);
        assert_eq!(
            stage_chunk_text(
                original,
                current,
                beta.from,
                Some(TextRange {
                    from: beta.from,
                    to: beta.to
                })
            )
            .as_deref(),
            Some("alpha\nbeta\ngamma\n")
        );
    }

    #[test]
    fn stages_a_deleted_hunk() {
        let original = "alpha\nbeta\ngamma\n";
        let current = "alpha\ngamma\n";
        let pos = line_from(current, 2);
        assert_eq!(
            stage_chunk_text(original, current, pos, None).as_deref(),
            Some(current)
        );
    }

    // describe("revertChunkText")

    #[test]
    fn restores_a_deleted_line() {
        let original = "alpha\nbeta\ngamma\n";
        let current = "alpha\ngamma\n";
        assert_eq!(
            revert_chunk_text(original, current, 6, None).as_deref(),
            Some(original)
        );
    }

    #[test]
    fn removes_an_added_line() {
        let original = "alpha\ngamma\n";
        let current = "alpha\nbeta\ngamma\n";
        assert_eq!(
            revert_chunk_text(original, current, 6, None).as_deref(),
            Some(original)
        );
    }

    #[test]
    fn restores_a_modified_hunk() {
        let original = "alpha\nbeta\ngamma\n";
        let current = "alpha\nBETA\ngamma\n";
        assert_eq!(
            revert_chunk_text(original, current, 6, None).as_deref(),
            Some(original)
        );
    }

    #[test]
    fn returns_none_when_the_cursor_is_on_an_unchanged_line() {
        let original = "alpha\nbeta\ngamma\n";
        let current = "alpha\nBETA\ngamma\n";
        assert_eq!(revert_chunk_text(original, current, 0, None), None);
    }

    #[test]
    fn reverts_selected_added_lines_and_keeps_the_rest_of_the_hunk() {
        let original = "alpha\ngamma\n";
        let current = "alpha\nbeta\ndelta\ngamma\n";
        let beta = Doc::new(current).line(2);
        assert_eq!(
            revert_chunk_text(
                original,
                current,
                beta.from,
                Some(TextRange {
                    from: beta.from,
                    to: beta.to
                })
            )
            .as_deref(),
            Some("alpha\ndelta\ngamma\n")
        );
    }

    // describe("deletedLineTexts")

    #[test]
    fn returns_the_removed_lines_for_a_deletion_hunk() {
        let original = "alpha\nbeta\ngamma\n";
        let current = "alpha\ngamma\n";
        let chunks = chunks_for(original, current);
        let chunk = chunks.iter().find(|chunk| chunk.is_deletion()).unwrap();
        assert_eq!(deleted_line_texts(&Doc::new(original), chunk), vec!["beta"]);
    }

    // describe("overviewTicks")

    #[test]
    fn maps_added_deleted_and_modified_hunks() {
        let current = "alpha\nbeta\ngamma\n";
        let added = chunks_for("alpha\ngamma\n", current);
        assert_eq!(
            overview_ticks(&Doc::new(current), &added, None),
            vec![OverviewTick {
                kind: ChangeKind::Added,
                top: 1.0 / 4.0,
                size: 1.0 / 4.0,
                pos: 6,
            }]
        );

        let original = "alpha\nbeta\ngamma\n";
        let deleted_doc = "alpha\ngamma\n";
        let deleted = chunks_for(original, deleted_doc);
        let ticks = overview_ticks(&Doc::new(deleted_doc), &deleted, Some(&Doc::new(original)));
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].kind, ChangeKind::Deleted);

        let modified_doc = "alpha\nBETA\ngamma\n";
        let modified = chunks_for("alpha\nbeta\ngamma\n", modified_doc);
        let ticks = overview_ticks(&Doc::new(modified_doc), &modified, None);
        assert_eq!(ticks[0].kind, ChangeKind::Modified);
    }

    // describe("findChunk")

    #[test]
    fn finds_a_pure_deletion_on_the_following_line() {
        let original = "alpha\nbeta\ngamma\n";
        let current = "alpha\ngamma\n";
        let doc = Doc::new(current);
        let chunks = chunks_for(original, current);
        let chunk = find_chunk(&doc, &chunks, doc.line(2).from).unwrap();
        assert_ne!(chunk.from_a, chunk.to_a);
        assert_eq!(chunk.from_b, chunk.to_b);
    }

    // describe("stateWithGitOriginal")

    fn stats(current: &str, original: &str) -> (usize, usize) {
        let chunks = chunks_for(original, current);
        diff_line_stats(&Doc::new(current), &chunks, Some(&Doc::new(original)))
    }

    #[test]
    fn decorates_added_deleted_and_modified_hunks() {
        assert_eq!(
            stats("alpha\nBETA\ngamma\n", "alpha\nbeta\ngamma\n"),
            (1, 1)
        );
        assert_eq!(stats("alpha\nbeta\ngamma\n", "alpha\ngamma\n"), (1, 0));
        assert_eq!(stats("alpha\ngamma\n", "alpha\nbeta\ngamma\n"), (0, 1));
        assert_eq!(stats("hello\nworld\n", ""), (2, 0));
    }

    #[test]
    fn rebuilds_hunks_when_the_whole_document_is_replaced() {
        let original = "alpha\nbeta\ngamma\n";
        assert_eq!(stats(original, original), (0, 0));
        assert_eq!(stats("alpha\nBETA\ngamma\n", original), (1, 1));
    }

    #[test]
    fn keeps_incremental_edits_on_the_live_hunk_path() {
        let original = "alpha\nbeta\ngamma\n";
        let edited = "alpha\nXbeta\ngamma\n";
        assert!(stats(edited, original).0 > 0);
    }

    #[test]
    fn clears_hunks_when_a_disk_replace_matches_git_original() {
        let original = "alpha\nbeta\ngamma\n";
        assert_eq!(stats("alpha\nBETA\ngamma\n", original), (1, 1));
        assert_eq!(stats(original, original), (0, 0));
    }

    // Port-specific cases.

    #[test]
    fn chunks_cover_whole_lines() {
        let chunks = chunks_for("alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\n");
        assert_eq!(
            chunks,
            vec![Chunk {
                from_a: 6,
                to_a: 11,
                from_b: 6,
                to_b: 11
            }]
        );
    }

    #[test]
    fn a_missing_final_newline_modifies_the_last_line() {
        let chunks = chunks_for("a\nb", "a\nb\nc");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind(), ChangeKind::Modified);
        assert_eq!(chunks[0].to_a, 4);
        assert_eq!(chunks[0].to_b, 6);
        assert_eq!(
            revert_chunk_text("a\nb", "a\nb\nc", 2, None).as_deref(),
            Some("a\nb")
        );
    }

    #[test]
    fn separate_hunks_stay_separate() {
        let chunks = chunks_for("a\nb\nc\nd\ne\n", "A\nb\nc\nd\nE\n");
        assert_eq!(chunks.len(), 2);
        let doc = Doc::new("A\nb\nc\nd\nE\n");
        assert_eq!(marked_lines(&doc, &chunks[0]), 1..2);
        assert_eq!(marked_lines(&doc, &chunks[1]), 5..6);
    }

    #[test]
    fn marks_the_line_after_a_pure_deletion() {
        let current = "alpha\ngamma\n";
        let chunks = chunks_for("alpha\nbeta\ngamma\n", current);
        assert_eq!(marked_lines(&Doc::new(current), &chunks[0]), 2..3);
    }

    #[test]
    fn doc_lines_match_codemirror_text() {
        let doc = Doc::new("alpha\ngamma\n");
        assert_eq!(doc.lines(), 3);
        assert_eq!(doc.line(3).from, 12);
        assert_eq!(doc.line_at(12).number, 3);
        assert_eq!(doc.line_at(5).number, 1);
        assert_eq!(doc.line_at(6).number, 2);
        assert_eq!(doc.line_text(2), "gamma");
    }
}
