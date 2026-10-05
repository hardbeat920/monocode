//! Port of src/features/files/editor/editorDoc.ts: line-ending handling, and
//! keeping the selection on the same text when the document is replaced.
//!
//! The editor works on LF-only text, like CodeMirror. A file's own line
//! ending is detected on load and restored on save.

use std::ops::Range;

use crate::git_diff::{Chunk, chunks_for};

/// `normalizeLineBreaks`.
pub fn normalize_line_breaks(value: &str) -> String {
    if !value.contains('\r') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// `detectLineEnding`: mixed files report the first flavor matched, and CRLF
/// wins over a lone CR.
pub fn detect_line_ending(value: &str) -> LineEnding {
    if value.contains("\r\n") {
        LineEnding::CrLf
    } else if value.contains('\r') {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

/// `restoreLineEnding`.
pub fn restore_line_ending(value: &str, eol: LineEnding) -> String {
    match eol {
        LineEnding::Lf => value.to_owned(),
        _ => value.replace('\n', eol.as_str()),
    }
}

/// Map `pos` in `old` to the matching position in `new`.
///
/// CodeMirror's `replaceEditorDoc` applied the diff as changes, so the
/// selection moved with the text around it. This gives the same result for
/// a whole-document replacement: positions in unchanged lines keep their
/// place, positions inside a changed chunk keep their column when they can.
pub fn map_offset(chunks: &[Chunk], old_len: usize, new_len: usize, pos: usize) -> usize {
    let mut delta: isize = 0;
    for chunk in chunks {
        let to_a = chunk.to_a.min(old_len + 1);
        if pos < chunk.from_a {
            break;
        }
        if pos < to_a {
            let inside = pos - chunk.from_a;
            let span_b = chunk.to_b.saturating_sub(chunk.from_b);
            return (chunk.from_b + inside.min(span_b.saturating_sub(1))).min(new_len);
        }
        delta +=
            chunk.to_b as isize - chunk.from_b as isize - (to_a as isize - chunk.from_a as isize);
    }
    ((pos as isize + delta).max(0) as usize).min(new_len)
}

/// Map a selection from `old` to `new`.
pub fn map_selection(old: &str, new: &str, selection: Range<usize>) -> Range<usize> {
    let chunks = chunks_for(old, new);
    let start = map_offset(&chunks, old.len(), new.len(), selection.start);
    let end = map_offset(&chunks, old.len(), new.len(), selection.end);
    let end = end.max(start);
    floor_boundary(new, start)..floor_boundary(new, end)
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    // describe("line-ending round trip")

    #[test]
    fn load_then_save_leaves_the_file_byte_identical() {
        for raw in ["alpha\nbeta\n", "alpha\r\nbeta\r\n", "alpha\rbeta\r"] {
            let restored =
                restore_line_ending(&normalize_line_breaks(raw), detect_line_ending(raw));
            assert_eq!(restored, raw);
        }
    }

    // describe("editorDocChanges"): the selection follows the text.

    #[test]
    fn treats_crlf_disk_content_as_identical_to_the_lf_document() {
        assert_eq!(normalize_line_breaks("alpha\r\nbeta\r\n"), "alpha\nbeta\n");
    }

    #[test]
    fn keeps_a_later_search_selection_on_the_same_match_after_an_earlier_edit() {
        let from = "const hello = 1;\nconst hello = 2;\n";
        let to = "const hallo = 1;\nconst hello = 2;\n";
        let second = from[from.find("hello").unwrap() + 1..]
            .find("hello")
            .unwrap()
            + from.find("hello").unwrap()
            + 1;
        let next = map_selection(from, to, second..second + 5);
        assert_eq!(&to[next.clone()], "hello");
        assert!(next.start > to.find('\n').unwrap());
    }

    #[test]
    fn keeps_the_cursor_after_lines_inserted_above() {
        let from = "a\nb\nc\n";
        let to = "x\ny\na\nb\nc\n";
        let at_c = from.find('c').unwrap();
        let next = map_selection(from, to, at_c..at_c);
        assert_eq!(&to[next.start..next.start + 1], "c");
    }

    #[test]
    fn keeps_the_column_inside_a_changed_line() {
        let from = "alpha\nbeta\ngamma\n";
        let to = "alpha\nBETA!\ngamma\n";
        let next = map_selection(from, to, 8..8);
        assert_eq!(next, 8..8);
    }
}
