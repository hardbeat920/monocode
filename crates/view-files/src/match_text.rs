//! Port of src/shared/ui/MatchText.tsx: fuzzy-match text with the matched
//! characters in the accent color.

use std::ops::Range;

use gpui::{HighlightStyle, Hsla, SharedString, StyledText};

/// Byte ranges of the characters at `positions`, which count UTF-16 code
/// units like the TypeScript. Adjacent characters merge into one range.
pub fn match_ranges(text: &str, positions: &[usize]) -> Vec<Range<usize>> {
    let mut marked: Vec<usize> = positions.to_vec();
    marked.sort_unstable();
    marked.dedup();
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut unit = 0;
    let mut next = 0;
    for (byte, ch) in text.char_indices() {
        while next < marked.len() && marked[next] < unit {
            next += 1;
        }
        if next < marked.len() && marked[next] == unit {
            let end = byte + ch.len_utf8();
            match out.last_mut() {
                Some(last) if last.end == byte => last.end = end,
                _ => out.push(byte..end),
            }
        }
        unit += ch.len_utf16();
    }
    out
}

/// `MatchText`: `text` with the characters at `positions` in `accent` when
/// `active` is set.
pub fn match_text(
    text: impl Into<SharedString>,
    positions: &[usize],
    active: bool,
    accent: Hsla,
) -> StyledText {
    let text: SharedString = text.into();
    if !active || positions.is_empty() {
        return StyledText::new(text);
    }
    let style = HighlightStyle {
        color: Some(accent),
        ..Default::default()
    };
    let highlights: Vec<_> = match_ranges(&text, positions)
        .into_iter()
        .map(|range| (range, style))
        .collect();
    StyledText::new(text).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_adjacent_positions_into_runs() {
        assert_eq!(
            match_ranges("Reload MonoCode", &[0, 7, 11]),
            vec![0..1, 7..8, 11..12]
        );
        assert_eq!(match_ranges("abc", &[0, 1, 2]), vec![0..3]);
        assert_eq!(match_ranges("abc", &[]), Vec::<Range<usize>>::new());
    }

    #[test]
    fn counts_positions_in_utf16_units() {
        // "é" is one UTF-16 unit and two bytes; "😀" is two units, four bytes.
        assert_eq!(match_ranges("é😀x", &[0, 3]), vec![0..2, 6..7]);
    }
}
