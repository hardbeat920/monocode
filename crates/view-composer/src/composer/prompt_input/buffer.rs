//! The editing model behind [`super::PromptInput`]: text, selection, IME
//! marked text, and undo. It knows nothing about layout or GPUI, so cursor
//! movement and composition are unit-tested here.
//!
//! Offsets are UTF-8 byte offsets on char boundaries. The platform input
//! handler speaks UTF-16, so the `*_utf16` methods convert at the edge, the
//! way GPUI's `examples/input.rs` (Apache-2.0) does.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation as _;

/// How an edit joins the undo history. Runs of typing or deleting merge
/// into one undo step, like a browser textarea.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// One typed character.
    Typing,
    /// Backspace or forward delete of one grapheme.
    Deleting,
    /// Anything else: paste, cut, a programmatic replace, a word delete.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
    reversed: bool,
}

/// Text plus a selection. `selection.start <= selection.end` always; when
/// `reversed` is set the head (the caret) sits at `start`.
#[derive(Clone, Debug, Default)]
pub struct PromptBuffer {
    text: String,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last edit's kind and where its caret ended, so the next edit of
    /// the same kind at the same spot extends one undo step.
    last_edit: Option<(EditKind, usize)>,
}

/// The most undo steps kept.
const UNDO_LIMIT: usize = 200;

impl PromptBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.len();
        Self {
            text,
            selection: end..end,
            ..Self::default()
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The selected byte range, start before end.
    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub fn is_reversed(&self) -> bool {
        self.reversed
    }

    /// The caret: the end of the selection that moves.
    pub fn head(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// The fixed end of the selection.
    pub fn anchor(&self) -> usize {
        if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        }
    }

    /// The IME composition range, if a composition is in progress.
    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection.clone()]
    }

    /// Replaces the whole text without an undo step, putting the caret at
    /// `cursor` (clamped). Used for drafts loaded from outside.
    pub fn reset(&mut self, text: impl Into<String>, cursor: usize) {
        self.text = text.into();
        let cursor = self.clamp(cursor);
        self.selection = cursor..cursor;
        self.reversed = false;
        self.marked = None;
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
    }

    /// Like `el.value = next` in the React code: replaces the text as one
    /// undoable step and puts the caret at `cursor`.
    pub fn set_text(&mut self, text: impl Into<String>, cursor: usize) {
        let text = text.into();
        if text == self.text {
            self.move_to(cursor);
            return;
        }
        self.push_undo(EditKind::Other, 0);
        self.text = text;
        let cursor = self.clamp(cursor);
        self.selection = cursor..cursor;
        self.reversed = false;
        self.marked = None;
        self.last_edit = None;
    }

    /// `setSelectionRange(start, end)`.
    pub fn select(&mut self, range: Range<usize>, reversed: bool) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end);
        if start <= end {
            self.selection = start..end;
            self.reversed = reversed && start != end;
        } else {
            self.selection = end..start;
            self.reversed = !reversed;
        }
        self.last_edit = None;
    }

    /// Collapses the selection to `offset`.
    pub fn move_to(&mut self, offset: usize) {
        let offset = self.clamp(offset);
        self.selection = offset..offset;
        self.reversed = false;
        self.last_edit = None;
    }

    /// Moves the head to `offset`, keeping the anchor.
    pub fn select_to(&mut self, offset: usize) {
        let offset = self.clamp(offset);
        let anchor = self.anchor();
        if offset < anchor {
            self.selection = offset..anchor;
            self.reversed = true;
        } else {
            self.selection = anchor..offset;
            self.reversed = false;
        }
        self.last_edit = None;
    }

    pub fn select_all(&mut self) {
        self.selection = 0..self.text.len();
        self.reversed = false;
        self.last_edit = None;
    }

    /// Replaces `range` with `new_text` and leaves the caret after it.
    pub fn replace(&mut self, range: Range<usize>, new_text: &str, kind: EditKind) {
        self.replace_inner(range, new_text, kind, true);
    }

    fn replace_inner(&mut self, range: Range<usize>, new_text: &str, kind: EditKind, record: bool) {
        let start = self.clamp(range.start.min(range.end));
        let end = self.clamp(range.end.max(range.start));
        if record {
            self.push_undo(kind, start);
        }
        self.text.replace_range(start..end, new_text);
        let caret = start + new_text.len();
        self.selection = caret..caret;
        self.reversed = false;
        self.marked = None;
        self.last_edit = Some((kind, caret));
    }

    /// Types or pastes over the selection (or the marked text).
    pub fn insert(&mut self, new_text: &str) {
        let range = self.marked.clone().unwrap_or(self.selection.clone());
        let kind = if new_text.chars().count() == 1 && range.is_empty() {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.replace(range, new_text, kind);
    }

    /// Backspace: deletes the selection, or the grapheme before the caret.
    /// Returns false when there was nothing to delete.
    pub fn backspace(&mut self) -> bool {
        if !self.selection.is_empty() {
            self.replace(self.selection(), "", EditKind::Other);
            return true;
        }
        let head = self.head();
        let previous = self.previous_grapheme(head);
        if previous == head {
            return false;
        }
        self.replace(previous..head, "", EditKind::Deleting);
        true
    }

    /// Forward delete.
    pub fn delete(&mut self) -> bool {
        if !self.selection.is_empty() {
            self.replace(self.selection(), "", EditKind::Other);
            return true;
        }
        let head = self.head();
        let next = self.next_grapheme(head);
        if next == head {
            return false;
        }
        self.replace(head..next, "", EditKind::Deleting);
        true
    }

    /// Deletes from the caret back to `to`, or the selection if there is one.
    pub fn delete_back_to(&mut self, to: usize) -> bool {
        if !self.selection.is_empty() {
            self.replace(self.selection(), "", EditKind::Other);
            return true;
        }
        let head = self.head();
        let to = self.clamp(to.min(head));
        if to == head {
            return false;
        }
        self.replace(to..head, "", EditKind::Other);
        true
    }

    /// Deletes from the caret forward to `to`, or the selection.
    pub fn delete_forward_to(&mut self, to: usize) -> bool {
        if !self.selection.is_empty() {
            self.replace(self.selection(), "", EditKind::Other);
            return true;
        }
        let head = self.head();
        let to = self.clamp(to.max(head));
        if to == head {
            return false;
        }
        self.replace(head..to, "", EditKind::Other);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(previous);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(next);
        true
    }

    // Boundaries.

    /// The grapheme boundary before `offset`.
    pub fn previous_grapheme(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[..offset]
            .grapheme_indices(true)
            .next_back()
            .map(|(ix, _)| ix)
            .unwrap_or(0)
    }

    /// The grapheme boundary after `offset`.
    pub fn next_grapheme(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[offset..]
            .graphemes(true)
            .next()
            .map(|g| offset + g.len())
            .unwrap_or(self.text.len())
    }

    /// Option-Left: the start of the word before `offset`, skipping spaces
    /// and punctuation in between.
    pub fn previous_word_start(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[..offset]
            .split_word_bound_indices()
            .rev()
            .find(|(_, word)| is_word(word))
            .map(|(ix, _)| ix)
            .unwrap_or(0)
    }

    /// Option-Right: the end of the word after `offset`.
    pub fn next_word_end(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[offset..]
            .split_word_bound_indices()
            .find(|(_, word)| is_word(word))
            .map(|(ix, word)| offset + ix + word.len())
            .unwrap_or(self.text.len())
    }

    /// The word around `offset`, for a double click. Whitespace and
    /// punctuation select as their own run.
    pub fn word_range_at(&self, offset: usize) -> Range<usize> {
        let offset = self.clamp(offset);
        let mut previous: Option<Range<usize>> = None;
        for (ix, word) in self.text.split_word_bound_indices() {
            let range = ix..ix + word.len();
            if range.contains(&offset) {
                // A click just past a word's end still picks that word.
                if offset == range.start
                    && !is_word(word)
                    && let Some(prev) = previous.filter(|prev| is_word(&self.text[prev.clone()]))
                {
                    return prev;
                }
                return range;
            }
            previous = Some(range);
        }
        previous.unwrap_or(offset..offset)
    }

    /// The start of the hard line holding `offset`.
    pub fn line_start(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[..offset]
            .rfind('\n')
            .map(|ix| ix + 1)
            .unwrap_or(0)
    }

    /// The end of the hard line holding `offset`, before its newline.
    pub fn line_end(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[offset..]
            .find('\n')
            .map(|ix| offset + ix)
            .unwrap_or(self.text.len())
    }

    /// The hard line holding `offset`, newline included, for a triple click.
    pub fn line_range_at(&self, offset: usize) -> Range<usize> {
        let start = self.line_start(offset);
        let end = self.line_end(offset);
        let end = if end < self.text.len() { end + 1 } else { end };
        start..end
    }

    /// Rounds `offset` down to a char boundary inside the text.
    pub fn clamp(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    // UTF-16, for the platform input handler.

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.text[..offset].encode_utf16().count()
    }

    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        for (ix, ch) in self.text.char_indices() {
            if utf16 >= offset {
                return ix;
            }
            utf16 += ch.len_utf16();
        }
        self.text.len()
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    pub fn len_utf16(&self) -> usize {
        self.text.encode_utf16().count()
    }

    /// `InputHandler::text_for_range`.
    pub fn text_for_range_utf16(
        &self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
    ) -> String {
        let range = self.range_from_utf16(&range);
        adjusted.replace(self.range_to_utf16(&range));
        self.text[range].to_string()
    }

    /// `InputHandler::replace_text_in_range`: commits text, ending any
    /// composition. Without a range it replaces the marked text, else the
    /// selection.
    pub fn replace_text_in_range_utf16(&mut self, range: Option<Range<usize>>, new_text: &str) {
        let range = range
            .map(|range| self.range_from_utf16(&range))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        let composing = self.marked.is_some();
        let kind = if new_text.chars().count() == 1 && range.is_empty() && !composing {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        // Committing a composition joins the undo step that opened it.
        self.replace_inner(range, new_text, kind, !composing);
    }

    /// `InputHandler::replace_and_mark_text_in_range`: updates the IME
    /// composition. `new_selected` is relative to the inserted text.
    pub fn replace_and_mark_utf16(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
    ) {
        let range = range
            .map(|range| self.range_from_utf16(&range))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        let start = self.clamp(range.start.min(range.end));
        let end = self.clamp(range.end.max(range.start));
        // One undo step covers the whole composition.
        if self.marked.is_none() {
            self.push_undo(EditKind::Other, start);
        }
        self.text.replace_range(start..end, new_text);
        self.marked = (!new_text.is_empty()).then(|| start..start + new_text.len());
        let selected = new_selected
            .map(|sel| {
                // The selection is in UTF-16 units of `new_text`.
                let from = utf16_to_byte(new_text, sel.start);
                let to = utf16_to_byte(new_text, sel.end);
                start + from..start + to
            })
            .unwrap_or(start + new_text.len()..start + new_text.len());
        self.selection = selected;
        self.reversed = false;
        self.last_edit = None;
    }

    /// `InputHandler::unmark_text`: keeps the composed text as typed.
    pub fn unmark(&mut self) {
        self.marked = None;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            selection: self.selection.clone(),
            reversed: self.reversed,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.selection = self.clamp(snapshot.selection.start)..self.clamp(snapshot.selection.end);
        self.reversed = snapshot.reversed;
        self.marked = None;
        self.last_edit = None;
    }

    fn push_undo(&mut self, kind: EditKind, at: usize) {
        let merges = matches!(
            (self.last_edit, kind),
            (Some((EditKind::Typing, caret)), EditKind::Typing) if caret == at
        ) || matches!(
            (self.last_edit, kind),
            (Some((EditKind::Deleting, _)), EditKind::Deleting)
        );
        if merges {
            return;
        }
        self.undo.push(self.snapshot());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

fn is_word(segment: &str) -> bool {
    segment.chars().any(|c| c.is_alphanumeric() || c == '_')
}

fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut utf16 = 0;
    for (ix, ch) in text.char_indices() {
        if utf16 >= units {
            return ix;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_moves_the_caret_and_merges_into_one_undo_step() {
        let mut buffer = PromptBuffer::default();
        for ch in "hello".chars() {
            buffer.insert(&ch.to_string());
        }
        assert_eq!(buffer.text(), "hello");
        assert_eq!(buffer.selection(), 5..5);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "");
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "hello");
    }

    #[test]
    fn moving_the_caret_starts_a_new_undo_step() {
        let mut buffer = PromptBuffer::default();
        buffer.insert("a");
        buffer.insert("b");
        buffer.move_to(0);
        buffer.insert("c");
        assert_eq!(buffer.text(), "cab");
        buffer.undo();
        assert_eq!(buffer.text(), "ab");
        buffer.undo();
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn backspace_removes_whole_graphemes() {
        let mut buffer = PromptBuffer::new("ok 👍🏽");
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "ok ");
        buffer.move_to(0);
        assert!(!buffer.backspace());
        assert!(buffer.delete());
        assert_eq!(buffer.text(), "k ");
    }

    #[test]
    fn backspace_deletes_the_selection_first() {
        let mut buffer = PromptBuffer::new("hello world");
        buffer.select(0..6, false);
        buffer.backspace();
        assert_eq!(buffer.text(), "world");
        assert_eq!(buffer.selection(), 0..0);
    }

    #[test]
    fn select_to_flips_direction_across_the_anchor() {
        let mut buffer = PromptBuffer::new("abcdef");
        buffer.move_to(3);
        buffer.select_to(5);
        assert_eq!((buffer.selection(), buffer.is_reversed()), (3..5, false));
        buffer.select_to(1);
        assert_eq!((buffer.selection(), buffer.is_reversed()), (1..3, true));
        assert_eq!(buffer.head(), 1);
        assert_eq!(buffer.anchor(), 3);
    }

    #[test]
    fn word_movement_skips_spaces_and_punctuation() {
        let buffer = PromptBuffer::new("fix the /plan, then @src/App.tsx");
        assert_eq!(buffer.previous_word_start(7), 4);
        assert_eq!(buffer.previous_word_start(4), 0);
        assert_eq!(buffer.next_word_end(0), 3);
        assert_eq!(buffer.next_word_end(7), 13);
        assert_eq!(buffer.next_word_end(buffer.len()), buffer.len());
        assert_eq!(buffer.previous_word_start(0), 0);
    }

    #[test]
    fn word_range_at_picks_the_word_under_a_double_click() {
        let buffer = PromptBuffer::new("say hello there");
        assert_eq!(buffer.word_range_at(5), 4..9);
        assert_eq!(buffer.word_range_at(9), 4..9);
        assert_eq!(buffer.word_range_at(0), 0..3);
    }

    #[test]
    fn line_bounds_follow_hard_newlines() {
        let buffer = PromptBuffer::new("one\ntwo\nthree");
        assert_eq!(buffer.line_start(5), 4);
        assert_eq!(buffer.line_end(5), 7);
        assert_eq!(buffer.line_range_at(5), 4..8);
        assert_eq!(buffer.line_range_at(10), 8..13);
    }

    #[test]
    fn utf16_offsets_round_trip_through_astral_characters() {
        let buffer = PromptBuffer::new("a😀b");
        assert_eq!(buffer.offset_to_utf16(5), 3);
        assert_eq!(buffer.offset_from_utf16(3), 5);
        assert_eq!(buffer.len_utf16(), 4);
        let mut adjusted = None;
        assert_eq!(buffer.text_for_range_utf16(1..3, &mut adjusted), "😀");
        assert_eq!(adjusted, Some(1..3));
    }

    #[test]
    fn ime_composition_marks_then_commits() {
        // Typing "ni" then picking 你 in a pinyin IME.
        let mut buffer = PromptBuffer::new("say ");
        buffer.replace_and_mark_utf16(None, "n", None);
        assert_eq!(buffer.text(), "say n");
        assert_eq!(buffer.marked(), Some(4..5));
        buffer.replace_and_mark_utf16(None, "ni", Some(2..2));
        assert_eq!(buffer.text(), "say ni");
        assert_eq!(buffer.marked(), Some(4..6));
        assert_eq!(buffer.selection(), 6..6);
        buffer.replace_text_in_range_utf16(None, "你");
        assert_eq!(buffer.text(), "say 你");
        assert_eq!(buffer.marked(), None);
        assert_eq!(buffer.selection(), 7..7);
        // The whole composition is one undo step.
        buffer.undo();
        assert_eq!(buffer.text(), "say ");
    }

    #[test]
    fn ime_selection_inside_the_marked_text_is_relative_to_it() {
        let mut buffer = PromptBuffer::new("x");
        buffer.move_to(0);
        buffer.replace_and_mark_utf16(None, "かな", Some(1..1));
        assert_eq!(buffer.text(), "かなx");
        assert_eq!(buffer.marked(), Some(0..6));
        assert_eq!(buffer.selection(), 3..3);
    }

    #[test]
    fn unmark_keeps_the_composed_text() {
        let mut buffer = PromptBuffer::default();
        buffer.replace_and_mark_utf16(None, "é", None);
        buffer.unmark();
        assert_eq!(buffer.text(), "é");
        assert_eq!(buffer.marked(), None);
        buffer.insert("!");
        assert_eq!(buffer.text(), "é!");
    }

    #[test]
    fn an_empty_composition_clears_the_mark() {
        let mut buffer = PromptBuffer::new("ab");
        buffer.replace_and_mark_utf16(None, "x", None);
        buffer.replace_and_mark_utf16(None, "", None);
        assert_eq!(buffer.text(), "ab");
        assert_eq!(buffer.marked(), None);
    }

    #[test]
    fn set_text_is_one_undo_step() {
        let mut buffer = PromptBuffer::new("/cre");
        buffer.set_text("/create-skill ", 14);
        assert_eq!(buffer.selection(), 14..14);
        buffer.undo();
        assert_eq!(buffer.text(), "/cre");
    }

    #[test]
    fn clamp_rounds_into_char_boundaries() {
        let buffer = PromptBuffer::new("é");
        assert_eq!(buffer.clamp(1), 0);
        assert_eq!(buffer.clamp(9), 2);
    }
}
