//! Port of the matching half of src/features/sessions/model/transcriptHighlights.ts.
//!
//! The TypeScript walked DOM text nodes and painted CSS highlights. Here the
//! view asks for match ranges inside each text it lays out and paints them
//! itself, so only the pattern and the range search are ported. Mutation
//! tracking is not needed: GPUI repaints visible rows every frame.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

/// `MATCH_CAP`: the most matches a search paints.
pub const MATCH_CAP: usize = 1000;

/// `transcriptSearchPattern`: the query as a literal, case-insensitive,
/// Unicode-aware pattern. `None` for a blank query.
pub fn transcript_search_pattern(query: &str) -> Option<Regex> {
    let needle = crate::js::trim(query);
    if needle.is_empty() {
        return None;
    }
    RegexBuilder::new(&regex::escape(needle))
        .case_insensitive(true)
        .unicode(true)
        .build()
        .ok()
}

/// Byte ranges of every match of `pattern` in `text`, at most `cap`.
pub fn match_ranges(pattern: &Regex, text: &str, cap: usize) -> Vec<Range<usize>> {
    pattern
        .find_iter(text)
        .filter(|found| !found.as_str().is_empty())
        .take(cap)
        .map(|found| found.range())
        .collect()
}

/// `transcriptWordRanges` for a list of searchable texts in reading order:
/// each text's match ranges, sharing one [`MATCH_CAP`] across all of them.
pub fn transcript_word_ranges<'a>(
    texts: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Vec<Vec<Range<usize>>> {
    let Some(pattern) = transcript_search_pattern(query) else {
        return texts.into_iter().map(|_| Vec::new()).collect();
    };
    let mut left = MATCH_CAP;
    texts
        .into_iter()
        .map(|text| {
            let ranges = match_ranges(&pattern, text, left);
            left -= ranges.len();
            ranges
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched<'a>(text: &'a str, query: &str) -> Vec<&'a str> {
        transcript_word_ranges([text], query)
            .remove(0)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn highlights_only_matching_words() {
        assert_eq!(
            matched("Hey can you check the notes?", "hey can you"),
            ["Hey can you"]
        );
        assert_eq!(
            matched("Hey can you review it?", "hey can you"),
            ["Hey can you"]
        );
    }

    #[test]
    fn treats_punctuation_in_a_query_literally() {
        assert_eq!(matched("foo.bar fooXbar", "foo.bar"), ["foo.bar"]);
    }

    #[test]
    fn uses_unicode_case_folding() {
        assert_eq!(matched("ſ", "s"), ["ſ"]);
    }

    #[test]
    fn does_nothing_without_a_query() {
        assert!(transcript_search_pattern("  ").is_none());
        assert_eq!(
            transcript_word_ranges(["a"], " "),
            vec![Vec::<Range<usize>>::new()]
        );
    }

    #[test]
    fn caps_matches_across_texts() {
        let text = "a".repeat(MATCH_CAP + 10);
        let ranges = transcript_word_ranges([text.as_str(), "a"], "a");
        assert_eq!(ranges[0].len(), MATCH_CAP);
        assert!(ranges[1].is_empty());
    }
}
