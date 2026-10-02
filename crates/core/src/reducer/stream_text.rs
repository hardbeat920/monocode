//! Port of src/integrations/harness/core/streamText.ts: one join for every
//! provider's streamed body text.

use serde_json::Value;

use crate::js;

/// What [`join_stream_text`] does with an incoming chunk.
enum Join {
    Keep,
    Replace,
    Append,
}

fn join_kind(existing: &str, incoming: &str) -> Join {
    if incoming.is_empty() {
        return Join::Keep;
    }
    if existing.is_empty() {
        return Join::Replace;
    }
    if incoming == existing {
        // `\n` then `\n` is a Markdown paragraph break, not a snapshot of a
        // one-character message. Longer exact repeats are completed snapshots.
        return if js::len(incoming) <= 1 {
            Join::Append
        } else {
            Join::Keep
        };
    }
    if incoming.len() > existing.len() && incoming.starts_with(existing) {
        return Join::Replace;
    }
    if existing.len() > incoming.len()
        && existing.starts_with(incoming)
        && js::trim(&existing[incoming.len()..]).is_empty()
    {
        return Join::Keep;
    }
    Join::Append
}

/// `joinStreamText`.
///
/// Providers send either a new token or a resent snapshot of the whole message
/// so far. This function is the only place that tells the two apart, so a new
/// harness does not need its own merge logic.
///
/// Tokens append. A longer chunk that already starts with the current text is
/// a snapshot and replaces. Overlap matching is never used: it ate blank
/// lines, headings, table rows, and doubled letters.
pub fn join_stream_text(existing: &str, incoming: &str) -> String {
    match join_kind(existing, incoming) {
        Join::Keep => existing.to_string(),
        Join::Replace => incoming.to_string(),
        Join::Append => format!("{existing}{incoming}"),
    }
}

/// [`join_stream_text`] into `existing` in place, so a streaming block grows
/// without copying its text for every token. Returns whether the text changed.
pub fn join_stream_text_into(existing: &mut String, incoming: &str) -> bool {
    match join_kind(existing, incoming) {
        Join::Keep => false,
        Join::Replace => {
            existing.clear();
            existing.push_str(incoming);
            true
        }
        Join::Append => {
            existing.push_str(incoming);
            true
        }
    }
}

/// `snapshotRemainder`: how much of a completed snapshot to emit after tokens
/// already landed.
///
/// Claude and Codex send the full message again when a turn (or item)
/// finishes. If that copy is the same as, or already contained in, what
/// streamed, emit nothing. If it only adds a suffix, emit the suffix. If it is
/// a new stretch (text after a tool, no tokens yet), emit the whole snapshot.
pub fn snapshot_remainder<'a>(already: &str, snapshot: &'a str) -> &'a str {
    if snapshot.is_empty() {
        return "";
    }
    if already.is_empty() {
        return snapshot;
    }
    if snapshot == already {
        return "";
    }
    if let Some(rest) = snapshot.strip_prefix(already) {
        return rest;
    }
    if already.starts_with(snapshot) {
        return "";
    }
    snapshot
}

/// `streamTextDelta`: body text from a stream. Whitespace is real content,
/// not a missing field.
pub fn stream_text_delta(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or("")
}

/// `mergeStream`, the old name of [`join_stream_text`].
pub use join_stream_text as merge_stream;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fold(chunks: &[&str]) -> String {
        chunks
            .iter()
            .fold(String::new(), |acc, chunk| join_stream_text(&acc, chunk))
    }

    // joinStreamText
    #[test]
    fn appends_tokens_including_doubled_letters_and_punctuation() {
        assert_eq!(join_stream_text("book", "keeper"), "bookkeeper");
        assert_eq!(join_stream_text("Wait.", ". Next"), "Wait.. Next");
    }

    #[test]
    fn keeps_heading_and_paragraph_on_separate_lines() {
        assert_eq!(
            fold(&["# Result", "\n", "\n", "Here is the answer."]),
            "# Result\n\nHere is the answer."
        );
    }

    #[test]
    fn keeps_gfm_table_row_boundaries() {
        assert_eq!(
            fold(&["| a | b |\n", "\n", "| --- | --- |\n", "| 1 | 2 |"]),
            "| a | b |\n\n| --- | --- |\n| 1 | 2 |"
        );
    }

    #[test]
    fn does_not_drop_a_pipe_that_starts_the_next_table_row() {
        assert_eq!(join_stream_text("| a | b |\n", "|"), "| a | b |\n|");
    }

    #[test]
    fn accepts_a_growing_snapshot_without_doubling() {
        assert_eq!(join_stream_text("hel", "hello"), "hello");
        assert_eq!(join_stream_text("hello", "hello"), "hello");
    }

    #[test]
    fn keeps_trailing_newlines_when_a_snapshot_trims_them() {
        assert_eq!(join_stream_text("hello\n\n", "hello"), "hello\n\n");
    }

    #[test]
    fn joins_in_place_like_the_copying_join() {
        for (existing, incoming) in [
            ("book", "keeper"),
            ("hel", "hello"),
            ("hello", "hello"),
            ("\n", "\n"),
            ("hello\n\n", "hello"),
            ("", "x"),
            ("x", ""),
        ] {
            let mut text = existing.to_string();
            let changed = join_stream_text_into(&mut text, incoming);
            assert_eq!(text, join_stream_text(existing, incoming));
            assert_eq!(changed, text != existing);
        }
    }

    // snapshotRemainder
    #[test]
    fn skips_a_completed_copy_of_text_that_already_streamed() {
        assert_eq!(snapshot_remainder("hello", "hello"), "");
        assert_eq!(snapshot_remainder("hello\n\n", "hello"), "");
    }

    #[test]
    fn emits_only_the_missing_suffix() {
        assert_eq!(snapshot_remainder("hel", "hello"), "lo");
    }

    #[test]
    fn emits_a_later_stretch_after_a_tool_instead_of_pasting_the_first_reply_again() {
        assert_eq!(
            snapshot_remainder("I'll read the file", "Here's what I found"),
            "Here's what I found"
        );
    }

    #[test]
    fn does_not_paste_an_earlier_snapshot_once_later_text_has_already_streamed() {
        assert_eq!(
            snapshot_remainder(
                "I'll read the fileHere's what I found",
                "I'll read the file"
            ),
            ""
        );
    }

    // streamTextDelta
    #[test]
    fn keeps_whitespace_only_body_text() {
        assert_eq!(stream_text_delta(Some(&json!("\n\n"))), "\n\n");
        assert_eq!(stream_text_delta(Some(&json!("  "))), "  ");
        assert_eq!(stream_text_delta(Some(&json!(""))), "");
        assert_eq!(stream_text_delta(None), "");
    }
}
