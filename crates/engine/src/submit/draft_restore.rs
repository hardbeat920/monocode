//! Port of src/shared/lib/draftRestore.ts: put back text a paste handler
//! withheld from the composer. The native clipboard read is asynchronous, so
//! the draft can change by the time the paste resolves; the captured range
//! is reused only while the draft is still the one that was captured.
//!
//! The TypeScript worked on a DOM text field. Here the field is anything
//! that implements [`DraftField`]; offsets are byte offsets.

/// A text field with a selection, as a textarea was.
pub trait DraftField {
    fn value(&self) -> &str;
    /// `selectionStart` and `selectionEnd`.
    fn selection(&self) -> (usize, usize);
    fn set_value(&mut self, value: String);
    fn set_selection_range(&mut self, start: usize, end: usize);
    /// The `input` event React listened for: the value changed.
    fn notify_input(&mut self);

    /// `setRangeText(text, start, end, "end")`: replace the range and put
    /// the caret after the new text.
    fn set_range_text(&mut self, text: &str, start: usize, end: usize) {
        let value = self.value();
        let start = floor_boundary(value, start);
        let end = floor_boundary(value, end).max(start);
        let next = format!("{}{text}{}", &value[..start], &value[end..]);
        let caret = start + text.len();
        self.set_value(next);
        self.set_selection_range(caret, caret);
    }
}

/// A plain [`DraftField`]: a value, a selection, and a count of input
/// notifications.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    pub value: String,
    pub selection_start: usize,
    pub selection_end: usize,
    pub input_events: usize,
}

impl TextField {
    /// A field with the caret at `start..end`, or at the end of `value`.
    pub fn new(value: &str, start: Option<usize>, end: Option<usize>) -> Self {
        let start = start.unwrap_or(value.len());
        Self {
            value: value.to_string(),
            selection_start: start,
            selection_end: end.unwrap_or(start),
            input_events: 0,
        }
    }
}

impl DraftField for TextField {
    fn value(&self) -> &str {
        &self.value
    }

    fn selection(&self) -> (usize, usize) {
        (self.selection_start, self.selection_end)
    }

    fn set_value(&mut self, value: String) {
        // Setting a textarea's value moves the caret to the end.
        self.selection_start = value.len();
        self.selection_end = value.len();
        self.value = value;
    }

    fn set_selection_range(&mut self, start: usize, end: usize) {
        let len = self.value.len();
        self.selection_start = start.min(len);
        self.selection_end = end.min(len).max(self.selection_start);
    }

    fn notify_input(&mut self) {
        self.input_events += 1;
    }
}

fn floor_boundary(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// `CapturedDraft`: the field's value and selection when the paste landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedDraft {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

/// `captureDraft`: `None` when the paste did not land in a text field.
pub fn capture_draft<F: DraftField + ?Sized>(field: Option<&F>) -> Option<CapturedDraft> {
    let field = field?;
    let (start, end) = field.selection();
    Some(CapturedDraft {
        value: field.value().to_string(),
        start,
        end,
    })
}

/// `valueWithPaste`: the draft if `text` were pasted at the captured range.
fn value_with_paste(captured: &CapturedDraft, text: &str) -> String {
    let start = floor_boundary(&captured.value, captured.start);
    let end = floor_boundary(&captured.value, captured.end).max(start);
    format!(
        "{}{text}{}",
        &captured.value[..start],
        &captured.value[end..]
    )
}

/// `dropPastedText`: take a withheld paste back out of the field. A file URI
/// that became an attachment has to leave; anything typed after it stays.
pub fn drop_pasted_text(field: &mut dyn DraftField, captured: &CapturedDraft, text: &str) {
    if text.is_empty() {
        return;
    }
    let inserted = value_with_paste(captured, text);
    if let Some(extra) = field.value().strip_prefix(inserted.as_str()) {
        let extra = extra.to_string();
        let restored = format!("{}{extra}", captured.value);
        if field.value() == restored {
            return;
        }
        let caret = if extra.is_empty() {
            captured.start
        } else {
            restored.len()
        };
        field.set_value(restored);
        field.set_selection_range(caret, caret);
        field.notify_input();
        return;
    }
    // Inserted before this handler observed the field, so the capture already
    // contains it. A paste leaves the caret at the end of that text.
    let end = field.selection().1.min(field.value().len());
    let Some(start) = end.checked_sub(text.len()) else {
        return;
    };
    if field.value().get(start..end) != Some(text) {
        return;
    }
    field.set_range_text("", start, end);
    field.notify_input();
}

/// `insertRestoredText`: insert at the captured range, or at the caret if
/// the draft has moved on.
pub fn insert_restored_text(field: &mut dyn DraftField, captured: &CapturedDraft, text: &str) {
    // The paste already landed. Putting it in again would double it.
    if !text.is_empty() && field.value().starts_with(&value_with_paste(captured, text)) {
        return;
    }
    let unchanged = field.value() == captured.value;
    let start = if unchanged {
        captured.start
    } else {
        field.selection().0
    };
    let end = if unchanged { captured.end } else { start };
    field.set_range_text(text, start, end);
    field.notify_input();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(value: &str, start: Option<usize>, end: Option<usize>) -> TextField {
        TextField::new(value, start, end)
    }

    // captureDraft
    #[test]
    fn records_the_value_and_selection_a_paste_landed_against() {
        let el = field("hello world", Some(6), Some(11));
        assert_eq!(
            capture_draft(Some(&el)),
            Some(CapturedDraft {
                value: "hello world".into(),
                start: 6,
                end: 11
            })
        );
    }

    #[test]
    fn ignores_a_target_that_is_not_a_text_field() {
        assert_eq!(capture_draft::<TextField>(None), None);
    }

    // dropPastedText
    #[test]
    fn removes_a_uri_inserted_at_the_captured_caret_and_keeps_a_later_suffix() {
        let mut el = field("keep", Some(4), Some(4));
        let captured = capture_draft(Some(&el)).unwrap();
        el.set_range_text("file:///tmp/a.png", 4, 4);
        el.value.push('!');
        drop_pasted_text(&mut el, &captured, "file:///tmp/a.png");
        assert_eq!(el.value, "keep!");
    }

    #[test]
    fn removes_a_uri_the_webview_inserted_before_the_paste_was_observed() {
        let uri = "file:///tmp/a.png";
        let mut el = field(uri, Some(uri.len()), Some(uri.len()));
        let captured = capture_draft(Some(&el)).unwrap();
        drop_pasted_text(&mut el, &captured, uri);
        assert_eq!(el.value, "");
    }

    // insertRestoredText
    #[test]
    fn restores_at_the_range_the_draft_had_replacing_only_that_range() {
        let mut el = field("hello world", Some(6), Some(11));
        let captured = capture_draft(Some(&el)).unwrap();
        insert_restored_text(&mut el, &captured, "URI");
        assert_eq!(el.value, "hello URI");
    }

    #[test]
    fn inserts_at_the_caret_rather_than_clobbering_a_later_selection() {
        let mut el = field("hello world", Some(6), Some(11));
        let captured = capture_draft(Some(&el)).unwrap();
        // The draft moved on while the clipboard read was in flight.
        el.set_value("hello brave world".into());
        el.set_selection_range(6, 11);
        insert_restored_text(&mut el, &captured, "URI");
        // "brave" survives: reusing the current range would have eaten it.
        assert_eq!(el.value, "hello URIbrave world");
    }

    #[test]
    fn still_inserts_when_the_draft_changed_and_the_caret_is_at_the_end() {
        let mut el = field("", Some(0), Some(0));
        let captured = capture_draft(Some(&el)).unwrap();
        el.set_value("typed meanwhile".into());
        el.set_selection_range(15, 15);
        insert_restored_text(&mut el, &captured, "URI");
        assert_eq!(el.value, "typed meanwhileURI");
    }

    #[test]
    fn does_not_insert_a_paste_the_field_already_contains() {
        let mut el = field("hello world", Some(6), Some(11));
        let captured = capture_draft(Some(&el)).unwrap();
        el.set_value("hello URI".into());
        el.set_selection_range(9, 9);
        insert_restored_text(&mut el, &captured, "URI");
        assert_eq!(el.value, "hello URI");
    }

    #[test]
    fn announces_the_change_so_the_view_sees_it() {
        let mut el = field("", None, None);
        let captured = capture_draft(Some(&el)).unwrap();
        insert_restored_text(&mut el, &captured, "URI");
        assert_eq!(el.input_events, 1);
    }
}
