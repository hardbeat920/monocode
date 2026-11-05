//! Native find/replace for the file editor.
//!
//! The previous implementation evaluated the editor bundle in QuickJS and used
//! `@codemirror/search`. This module keeps the same result shape and follows
//! the semantics of the pinned CodeMirror 6.5.11 bundle:
//!
//! * literal search is NFKD-normalized (query as a whole, source per code
//!   point) and optionally lowercased;
//! * regex search is ECMAScript, with `gmu` flags and optional `i`;
//! * whole-word uses Unicode `Alphabetic`, `Number`, and `_` on grapheme
//!   boundaries, not `\b`;
//! * zero-length matches are dropped, as the old bridge did;
//! * regex replacement expands the CodeMirror subset (`$$`, `$&`, `$1..$99`).

use corvus_regex_ecma262::{Captures, Flags, MatchError, Regex};
use memchr::memmem;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::GraphemeCursor;

const MATCH_TIMEOUT: Duration = Duration::from_millis(50);
const SEARCH_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SearchMatch {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub invalid: bool,
    #[serde(default)]
    pub timed_out: bool,
}

pub(crate) fn search(
    source: &str,
    query: &str,
    replacement: &str,
    case: bool,
    word: bool,
    regex: bool,
) -> Result<SearchResult, String> {
    if query.is_empty() {
        return Ok(SearchResult::default());
    }
    if regex {
        search_regex(source, query, replacement, case, word)
    } else {
        Ok(search_literal(source, query, replacement, case, word))
    }
}

/// Applies the already-computed matches in one pass. This replaces the old
/// reverse `String::replace_range` loop, which moved the tail of the file for
/// every match.
pub(crate) fn replace_all(source: &str, matches: &[SearchMatch]) -> Result<String, String> {
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for m in matches {
        if m.from < at
            || m.to < m.from
            || m.to > source.len()
            || !source.is_char_boundary(m.from)
            || !source.is_char_boundary(m.to)
        {
            return Err("search match is outside the document".into());
        }
        out.push_str(&source[at..m.from]);
        out.push_str(&m.insert);
        at = m.to;
    }
    out.push_str(&source[at..]);
    Ok(out)
}

fn search_literal(
    source: &str,
    query: &str,
    replacement: &str,
    case: bool,
    word: bool,
) -> SearchResult {
    let needle = normalize_query(query, case);
    if needle.is_empty() {
        return SearchResult::default();
    }
    let mut matches = Vec::new();
    if source.is_ascii() && needle.is_ascii() {
        if case {
            find_ascii(source, &needle, replacement, word, &mut matches);
        } else {
            let haystack = source.to_ascii_lowercase();
            find_ascii(&haystack, &needle, replacement, word, &mut matches);
        }
    } else {
        find_normalized(source, &needle, replacement, case, word, &mut matches);
    }
    SearchResult {
        matches,
        invalid: false,
        ..SearchResult::default()
    }
}

fn find_ascii(
    source: &str,
    needle: &str,
    replacement: &str,
    word: bool,
    matches: &mut Vec<SearchMatch>,
) {
    let mut at = 0;
    while at <= source.len() {
        let Some(pos) = memmem::find(&source.as_bytes()[at..], needle.as_bytes()) else {
            break;
        };
        let from = at + pos;
        let to = from + needle.len();
        if !word || whole_word(source, from, to) {
            matches.push(SearchMatch {
                from,
                to,
                insert: replacement.to_owned(),
            });
        }
        at = to;
    }
}

fn find_normalized(
    source: &str,
    needle: &str,
    replacement: &str,
    case: bool,
    word: bool,
    matches: &mut Vec<SearchMatch>,
) {
    let normalized = Normalized::new(source, case);
    let mut at = 0;
    while at <= normalized.text.len() {
        let Some(pos) = memmem::find(&normalized.text.as_bytes()[at..], needle.as_bytes()) else {
            break;
        };
        let from_norm = at + pos;
        let to_norm = from_norm + needle.len();
        let (Some(first), Some(last)) = (normalized.at(from_norm), normalized.before(to_norm))
        else {
            break;
        };
        if last.at_source_end {
            let from = first.source_start;
            let to = last.source_end;
            if !word || whole_word(source, from, to) {
                matches.push(SearchMatch {
                    from,
                    to,
                    insert: replacement.to_owned(),
                });
            }
            at = to_norm;
        } else {
            // CodeMirror only accepts a match when its end lands on a source
            // code-point boundary. Advance one normalized code point so a
            // valid suffix inside the same expansion can still match.
            at = next_char_boundary(&normalized.text, from_norm);
        }
    }
}

fn search_regex(
    source: &str,
    query: &str,
    replacement: &str,
    case: bool,
    word: bool,
) -> Result<SearchResult, String> {
    let flags = Flags {
        global: true,
        ignore_case: !case,
        multiline: true,
        unicode: true,
        ..Flags::default()
    };
    let regex = match Regex::with_timeout(query, flags, Some(MATCH_TIMEOUT)) {
        Ok(regex) => regex,
        Err(_) => {
            return Ok(SearchResult {
                matches: Vec::new(),
                invalid: true,
                ..SearchResult::default()
            });
        }
    };
    let replacement = Replacement::new(replacement);
    let mut state = Scan {
        source,
        regex,
        replacement,
        word,
        matches: Vec::new(),
        timed_out: false,
        deadline: Instant::now() + SEARCH_DEADLINE,
    };
    if needs_multiline(query) {
        state.scan_slice(source, 0)?;
    } else {
        let mut line_start = 0;
        for line in source.split_inclusive('\n') {
            let end = line_start + line.len();
            let content_end = if line.ends_with('\n') { end - 1 } else { end };
            state.scan_slice(&source[line_start..content_end], line_start)?;
            line_start = end;
        }
    }
    Ok(SearchResult {
        matches: state.matches,
        invalid: false,
        timed_out: state.timed_out,
    })
}

struct Scan<'a> {
    source: &'a str,
    regex: Regex,
    replacement: Replacement,
    word: bool,
    matches: Vec<SearchMatch>,
    timed_out: bool,
    deadline: Instant,
}

impl Scan<'_> {
    fn scan_slice(&mut self, text: &str, base: usize) -> Result<(), String> {
        for captures in self.regex.captures_iter(text) {
            if Instant::now() > self.deadline {
                self.timed_out = true;
                break;
            }
            let captures = match captures {
                Ok(captures) => captures,
                Err(MatchError::Time | MatchError::Memory) => {
                    self.timed_out = true;
                    break;
                }
            };
            let matched = captures.get_match();
            if matched.start() == matched.end() {
                continue;
            }
            let from = base + matched.start();
            let to = base + matched.end();
            if self.word && !whole_word(self.source, from, to) {
                continue;
            }
            self.matches.push(SearchMatch {
                from,
                to,
                insert: self.replacement.expand(&captures),
            });
        }
        Ok(())
    }
}

fn needs_multiline(query: &str) -> bool {
    let bytes = query.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b'\\' && matches!(bytes[i + 1], b's' | b'S' | b'W' | b'D' | b'n' | b'r') {
            return true;
        }
    }
    query.contains('\n') || query.contains('\r') || query.contains("[^")
}

fn normalize_query(query: &str, case: bool) -> String {
    let normalized: String = query.nfkd().collect();
    if case {
        normalized
    } else {
        normalized.to_lowercase()
    }
}

fn next_char_boundary(text: &str, at: usize) -> usize {
    text[at..]
        .chars()
        .next()
        .map_or(text.len(), |ch| at + ch.len_utf8())
}

struct Normalized {
    text: String,
    spans: Vec<NormSpan>,
}

struct NormSpan {
    norm_start: usize,
    norm_end: usize,
    source_start: usize,
    source_end: usize,
    at_source_end: bool,
}

impl Normalized {
    fn new(source: &str, case: bool) -> Self {
        let mut text = String::with_capacity(source.len());
        let mut spans = Vec::with_capacity(source.len());
        for (source_start, ch) in source.char_indices() {
            let source_end = source_start + ch.len_utf8();
            let piece: String = ch.to_string().nfkd().collect();
            let piece = if case { piece } else { piece.to_lowercase() };
            let chars = piece.chars().count();
            for (index, ch) in piece.chars().enumerate() {
                let norm_start = text.len();
                text.push(ch);
                spans.push(NormSpan {
                    norm_start,
                    norm_end: text.len(),
                    source_start,
                    source_end,
                    at_source_end: index + 1 == chars,
                });
            }
        }
        Self { text, spans }
    }

    fn at(&self, at: usize) -> Option<&NormSpan> {
        let index = self.spans.partition_point(|span| span.norm_end <= at);
        self.spans.get(index)
    }

    fn before(&self, at: usize) -> Option<&NormSpan> {
        let index = self.spans.partition_point(|span| span.norm_start < at);
        self.spans.get(index.checked_sub(1)?)
    }
}

fn whole_word(source: &str, from: usize, to: usize) -> bool {
    let before = grapheme_before(source, from);
    let first = grapheme_after(source, from);
    let last = grapheme_before(source, to);
    let after = grapheme_after(source, to);
    (!is_word(before) || !is_word(first)) && (!is_word(after) || !is_word(last))
}

fn is_word(cluster: &str) -> bool {
    cluster
        .chars()
        .any(|ch| ch == '_' || ch.is_alphabetic() || ch.is_numeric())
}

fn grapheme_before(source: &str, at: usize) -> &str {
    if at == 0 {
        return "";
    }
    let mut cursor = GraphemeCursor::new(at, source.len(), true);
    match cursor.prev_boundary(source, 0) {
        Ok(Some(start)) => &source[start..at],
        _ => {
            let start = source[..at]
                .char_indices()
                .next_back()
                .map_or(0, |(at, _)| at);
            &source[start..at]
        }
    }
}

fn grapheme_after(source: &str, at: usize) -> &str {
    if at >= source.len() {
        return "";
    }
    let mut cursor = GraphemeCursor::new(at, source.len(), true);
    match cursor.next_boundary(source, 0) {
        Ok(Some(end)) => &source[at..end],
        _ => {
            let end = next_char_boundary(source, at);
            &source[at..end]
        }
    }
}

#[derive(Default)]
struct Replacement {
    pieces: Vec<Piece>,
}

enum Piece {
    Literal(String),
    Whole,
    Digits(String),
}

impl Replacement {
    fn new(template: &str) -> Self {
        let mut pieces = Vec::new();
        let mut literal = String::new();
        let mut at = 0;
        while at < template.len() {
            let ch = template[at..]
                .chars()
                .next()
                .expect("template is not empty");
            if ch != '$' {
                literal.push(ch);
                at += ch.len_utf8();
                continue;
            }
            match template.as_bytes().get(at + 1).copied() {
                Some(b'$') => {
                    flush(&mut literal, &mut pieces);
                    pieces.push(Piece::Literal("$".into()));
                    at += 2;
                }
                Some(b'&') => {
                    flush(&mut literal, &mut pieces);
                    pieces.push(Piece::Whole);
                    at += 2;
                }
                Some(b'0'..=b'9') => {
                    flush(&mut literal, &mut pieces);
                    let mut end = at + 1;
                    while template.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                        end += 1;
                    }
                    pieces.push(Piece::Digits(template[at + 1..end].to_owned()));
                    at = end;
                }
                _ => {
                    literal.push('$');
                    at += 1;
                }
            }
        }
        flush(&mut literal, &mut pieces);
        Self { pieces }
    }

    fn expand(&self, captures: &Captures) -> String {
        let mut out = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Literal(text) => out.push_str(text),
                Piece::Whole => out.push_str(captures.get_match().as_str()),
                Piece::Digits(digits) => {
                    let mut expanded = None;
                    for length in (1..=digits.len()).rev() {
                        let Ok(number) = digits[..length].parse::<usize>() else {
                            continue;
                        };
                        if number > 0 && number < captures.len() {
                            let value = captures.get(number).map_or("undefined", |m| m.as_str());
                            expanded = Some(format!("{value}{}", &digits[length..]));
                            break;
                        }
                    }
                    out.push_str(&expanded.unwrap_or_else(|| format!("${digits}")));
                }
            }
        }
        out
    }
}

fn flush(literal: &mut String, pieces: &mut Vec<Piece>) {
    if !literal.is_empty() {
        pieces.push(Piece::Literal(std::mem::take(literal)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(
        source: &str,
        query: &str,
        replacement: &str,
        case: bool,
        word: bool,
        regex: bool,
    ) -> SearchResult {
        search(source, query, replacement, case, word, regex).unwrap()
    }

    #[test]
    fn literal_search_handles_case_whole_words_and_offsets() {
        let source = "foo foo_bar FOO";
        let result = found(source, "foo", "x", false, true, false);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(&source[result.matches[0].from..result.matches[0].to], "foo");
        assert_eq!(&source[result.matches[1].from..result.matches[1].to], "FOO");
        assert_eq!(result.matches[0].insert, "x");
    }

    #[test]
    fn literal_search_uses_codemirror_nfkd_mapping() {
        let source = "a🙂b ﬃ café";
        let result = found(source, "🙂", "x", true, false, false);
        assert_eq!(&source[result.matches[0].from..result.matches[0].to], "🙂");
        let result = found(source, "ffi", "x", true, false, false);
        assert_eq!(&source[result.matches[0].from..result.matches[0].to], "ﬃ");
        assert!(
            found(source, "ff", "x", true, false, false)
                .matches
                .is_empty()
        );
        assert!(
            found(source, "cafe", "x", true, false, false)
                .matches
                .is_empty()
        );
        assert!(found("É", "é", "x", true, false, false).matches.is_empty());
        assert_eq!(found("É", "é", "x", false, false, false).matches.len(), 1);
    }

    #[test]
    fn regex_search_handles_captures_and_zero_length_matches() {
        let source = "a1 b2";
        let result = found(source, "(\\w)(\\d)", "$2$1", false, false, true);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].insert, "1a");
        assert!(
            found(source, "^", "x", false, false, true)
                .matches
                .is_empty()
        );
        assert!(
            found(source, "a*?\\b", "x", false, false, true)
                .matches
                .is_empty()
        );
    }

    #[test]
    fn regex_search_uses_javascript_word_classes() {
        assert!(
            found("é", "\\w", "x", false, false, true)
                .matches
                .is_empty()
        );
        assert!(
            found("١٢٣", "\\d", "x", false, false, true)
                .matches
                .is_empty()
        );
        assert_eq!(
            found("abc", "a(?=b)", "x", false, false, true)
                .matches
                .len(),
            1
        );
        assert_eq!(
            found("ab", "(?<=a)b", "x", false, false, true)
                .matches
                .len(),
            1
        );
        assert_eq!(
            found("aa", "(a)\\1", "x", false, false, true).matches.len(),
            1
        );
    }

    #[test]
    fn regex_replacement_matches_codemirror_subset() {
        let source = "a";
        let result = found(source, "(a)(b)?", "$2|$1|$0|$<x>|$&|$$", false, false, true);
        assert_eq!(result.matches[0].insert, "undefined|a|$0|$<x>|a|$");
    }

    #[test]
    fn invalid_regex_is_reported() {
        assert!(found("a", "[", "", false, false, true).invalid);
        assert!(found("a", "[[:alpha:]]", "", false, false, true).invalid);
    }

    #[test]
    fn regex_line_and_multiline_modes_follow_javascript() {
        let source = "a\nb";
        assert_eq!(
            found(source, "^.", "x", false, false, true).matches.len(),
            2
        );
        assert_eq!(
            found(source, "\\s", "x", false, false, true).matches.len(),
            1
        );
        assert_eq!(
            found("a\u{2028}b", "^b", "x", false, false, true)
                .matches
                .len(),
            1
        );
    }

    #[test]
    fn replace_all_applies_matches_in_one_pass() {
        let source = "a🙂b a🙂b";
        let result = found(source, "🙂", "-", true, false, false);
        assert_eq!(replace_all(source, &result.matches).unwrap(), "a-b a-b");
        assert_eq!(replace_all(source, &[]).unwrap(), source);
        assert!(
            replace_all(
                "abc",
                &[SearchMatch {
                    from: 0,
                    to: 9,
                    insert: "x".into()
                }]
            )
            .is_err()
        );
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct ParityFixture {
        source: String,
        query: String,
        replacement: String,
        case: bool,
        word: bool,
        regex: bool,
        expected: SearchResult,
    }

    #[test]
    fn matches_the_search_parity_fixtures() {
        let fixtures: Vec<ParityFixture> =
            serde_json::from_str(include_str!("editor_search/fixtures.json")).unwrap();
        for fixture in fixtures {
            let actual = search(
                &fixture.source,
                &fixture.query,
                &fixture.replacement,
                fixture.case,
                fixture.word,
                fixture.regex,
            )
            .unwrap();
            assert_eq!(
                actual,
                fixture.expected,
                "source={:?} query={:?} replacement={:?} case={} word={} regex={}",
                fixture.source,
                fixture.query,
                fixture.replacement,
                fixture.case,
                fixture.word,
                fixture.regex
            );
        }
    }
}
