//! Port of the search model in src/features/files/editor/editorSearch.ts, with
//! the `SearchQuery`, `findNext`, `findPrevious`, `replaceNext`, and
//! `replaceAll` behavior of `@codemirror/search` that the find panel relies on
//! (configured with `literal: true`).
//!
//! Offsets are UTF-8 byte offsets. Regular expressions use Rust's `regex`
//! syntax, which has no lookaround or backreferences, so a JavaScript pattern
//! that needs them reports "Invalid regex".

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use crate::git_diff::TextChange;

/// `MATCH_CAP`: the count stops here and shows a `+`.
pub const MATCH_CAP: usize = 999;

/// The find panel's query. The defaults match a fresh CodeMirror panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub search: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regexp: bool,
}

/// A query compiled for matching.
#[derive(Debug, Clone)]
pub struct CompiledQuery {
    query: SearchQuery,
    regex: Regex,
}

/// One match, with the capture groups a regex replacement needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub range: Range<usize>,
    groups: Vec<Option<Range<usize>>>,
}

impl SearchMatch {
    pub fn from(&self) -> usize {
        self.range.start
    }

    pub fn to(&self) -> usize {
        self.range.end
    }
}

impl SearchQuery {
    /// `query.valid`: a non-empty search that compiles.
    pub fn valid(&self) -> bool {
        self.compile().is_some()
    }

    pub fn compile(&self) -> Option<CompiledQuery> {
        if self.search.is_empty() {
            return None;
        }
        let pattern = if self.regexp {
            self.search.clone()
        } else {
            regex::escape(&self.search)
        };
        let regex = RegexBuilder::new(&pattern)
            .multi_line(true)
            .case_insensitive(!self.case_sensitive)
            .build()
            .ok()?;
        Some(CompiledQuery {
            query: self.clone(),
            regex,
        })
    }
}

/// CodeMirror's default word categorizer, close enough for whole-word tests.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn char_before(text: &str, pos: usize) -> Option<char> {
    text.get(..pos)?.chars().next_back()
}

fn char_after(text: &str, pos: usize) -> Option<char> {
    text.get(pos..)?.chars().next()
}

fn is_word(ch: Option<char>) -> bool {
    ch.is_some_and(is_word_char)
}

/// `stringWordTest` and `regexpWordTest`: the match must not cut a word in
/// half at either end.
fn word_test(text: &str, from: usize, to: usize) -> bool {
    if from == to {
        return true;
    }
    (!is_word(char_before(text, from)) || !is_word(char_after(text, from)))
        && (!is_word(char_after(text, to)) || !is_word(char_before(text, to)))
}

fn next_char_boundary(text: &str, pos: usize) -> usize {
    let mut next = pos + 1;
    while next < text.len() && !text.is_char_boundary(next) {
        next += 1;
    }
    next
}

impl CompiledQuery {
    pub fn query(&self) -> &SearchQuery {
        &self.query
    }

    /// First accepted match that starts at or after `pos` and ends by `to`.
    fn find_from(&self, text: &str, mut pos: usize, to: usize) -> Option<SearchMatch> {
        let to = to.min(text.len());
        while pos <= to {
            let captures = self.regex.captures_at(text, pos)?;
            let whole = captures.get(0)?;
            if whole.end() > to {
                return None;
            }
            if !self.query.whole_word || word_test(text, whole.start(), whole.end()) {
                return Some(SearchMatch {
                    range: whole.range(),
                    groups: captures
                        .iter()
                        .map(|group| group.map(|g| g.range()))
                        .collect(),
                });
            }
            if whole.start() >= text.len() {
                return None;
            }
            pos = next_char_boundary(text, whole.start());
        }
        None
    }

    /// Non-overlapping matches inside `from..to`, the cursor `getCursor` returns.
    pub fn matches_in(&self, text: &str, from: usize, to: usize, limit: usize) -> Vec<SearchMatch> {
        let mut out = Vec::new();
        let mut pos = from;
        let mut last_end: Option<usize> = None;
        while out.len() < limit {
            let Some(found) = self.find_from(text, pos, to) else {
                break;
            };
            let empty = found.range.is_empty();
            // An empty match is only kept past the end of the previous one.
            if empty && last_end.is_some_and(|end| found.range.start <= end) {
                if found.range.start >= text.len() {
                    break;
                }
                pos = next_char_boundary(text, found.range.start);
                continue;
            }
            pos = if empty {
                if found.range.start >= text.len() {
                    out.push(found);
                    break;
                }
                next_char_boundary(text, found.range.start)
            } else {
                found.range.end
            };
            last_end = Some(found.range.end);
            out.push(found);
        }
        out
    }

    /// `countMatches`: every match up to [`MATCH_CAP`], and whether it was capped.
    pub fn count_matches(&self, text: &str) -> (Vec<Range<usize>>, bool) {
        let matches = self.matches_in(text, 0, text.len(), MATCH_CAP);
        let capped = matches.len() >= MATCH_CAP;
        (matches.into_iter().map(|m| m.range).collect(), capped)
    }

    /// `nextMatch(state, curFrom, curTo)`: the next match after `cur_to`,
    /// wrapping to the start. Returns `None` when the only match is the
    /// current selection.
    pub fn next_match(&self, text: &str, cur_from: usize, cur_to: usize) -> Option<SearchMatch> {
        let found = self.find_from(text, cur_to, text.len()).or_else(|| {
            let end = if self.query.regexp {
                cur_from
            } else {
                text.len().min(cur_from + self.query.search.len())
            };
            self.find_from(text, 0, end)
        })?;
        if !self.query.regexp && found.range.start == cur_from && found.range.end == cur_to {
            return None;
        }
        Some(found)
    }

    /// `prevMatchInRange`: the last match that fits inside `from..to`.
    fn prev_match_in_range(&self, text: &str, from: usize, to: usize) -> Option<SearchMatch> {
        let mut last = None;
        let mut pos = from;
        while let Some(found) = self.find_from(text, pos, to) {
            pos = if found.range.start >= text.len() {
                last = Some(found);
                break;
            } else {
                next_char_boundary(text, found.range.start)
            };
            last = Some(found);
        }
        last
    }

    /// `prevMatch(state, curFrom, curTo)`: the last match before `cur_from`,
    /// wrapping to the end.
    pub fn prev_match(&self, text: &str, cur_from: usize, cur_to: usize) -> Option<SearchMatch> {
        let found = self.prev_match_in_range(text, 0, cur_from).or_else(|| {
            let start = if self.query.regexp {
                cur_to
            } else {
                cur_to.saturating_sub(self.query.search.len())
            };
            self.prev_match_in_range(text, start, text.len())
        })?;
        if found.range.start == cur_from && found.range.end == cur_to {
            return None;
        }
        Some(found)
    }

    /// `getReplacement`: the replace text, with `$&`, `$$`, and `$1`..`$99`
    /// expanded for a regex query.
    pub fn replacement(&self, text: &str, found: &SearchMatch) -> String {
        let replace = &self.query.replace;
        if !self.query.regexp {
            return replace.clone();
        }
        let group_text = |index: usize| -> &str {
            found
                .groups
                .get(index)
                .and_then(|group| group.clone())
                .and_then(|range| text.get(range))
                .unwrap_or("")
        };
        let mut out = String::with_capacity(replace.len());
        let bytes = replace.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'$' && index + 1 < bytes.len() {
                let next = bytes[index + 1];
                if next == b'&' {
                    out.push_str(group_text(0));
                    index += 2;
                    continue;
                }
                if next == b'$' {
                    out.push('$');
                    index += 2;
                    continue;
                }
                if next.is_ascii_digit() {
                    let digits_end = bytes[index + 1..]
                        .iter()
                        .position(|byte| !byte.is_ascii_digit())
                        .map_or(bytes.len(), |offset| index + 1 + offset);
                    let digits = &replace[index + 1..digits_end];
                    let mut matched = false;
                    for length in (1..=digits.len()).rev() {
                        let number: usize = digits[..length].parse().unwrap_or(0);
                        if number > 0 && number < found.groups.len() {
                            out.push_str(group_text(number));
                            out.push_str(&digits[length..]);
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        out.push_str(&replace[index..digits_end]);
                    }
                    index = digits_end;
                    continue;
                }
            }
            let ch = replace[index..].chars().next().unwrap_or('$');
            out.push(ch);
            index += ch.len_utf8();
        }
        out
    }

    /// `replaceNext`: when the selection is a match, replace it and select the
    /// next one; otherwise select the next match.
    pub fn replace_next(&self, text: &str, selection: Range<usize>) -> Option<ReplaceNext> {
        let (from, to) = (selection.start, selection.end);
        let first = self.next_match(text, from, from)?;
        let mut next = Some(first.clone());
        let mut change = None;
        if first.range.start == from && first.range.end == to {
            change = Some(TextChange {
                from: first.range.start,
                to: first.range.end,
                insert: self.replacement(text, &first),
            });
            next = self.next_match(text, first.range.start, first.range.end);
        }
        let select = next.map(|next| match &change {
            Some(change) => map_pos(change, next.range.start)..map_pos(change, next.range.end),
            None => next.range,
        });
        Some(ReplaceNext { change, select })
    }

    /// `replaceAll`: the text with every match replaced, and the number replaced.
    pub fn replace_all(&self, text: &str) -> (String, usize) {
        let matches = self.matches_in(text, 0, text.len(), usize::MAX);
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for found in &matches {
            out.push_str(&text[last..found.range.start]);
            out.push_str(&self.replacement(text, found));
            last = found.range.end;
        }
        out.push_str(&text[last..]);
        (out, matches.len())
    }

    /// The match `reveal` selects after the query changes: the first match at
    /// or after the selection start, wrapping to the start of the text.
    pub fn reveal_match(&self, text: &str, selection: Range<usize>) -> Option<Range<usize>> {
        let found = self
            .matches_in(text, selection.start, text.len(), 1)
            .into_iter()
            .next()
            .or_else(|| {
                self.matches_in(text, 0, selection.start, 1)
                    .into_iter()
                    .next()
            })?;
        if found.range == selection {
            return None;
        }
        Some(found.range)
    }
}

/// What `replaceNext` does: an optional edit, then the range to select in
/// the edited text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceNext {
    pub change: Option<TextChange>,
    pub select: Option<Range<usize>>,
}

/// Map a position in the old text through `change` to the new text.
fn map_pos(change: &TextChange, pos: usize) -> usize {
    if pos <= change.from {
        pos
    } else if pos >= change.to {
        pos - (change.to - change.from) + change.insert.len()
    } else {
        change.from + change.insert.len()
    }
}

/// `matchIndexAtSelection`: the 1-based index of the match equal to the
/// selection, or 0.
pub fn match_index_at_selection(matches: &[Range<usize>], selection: &Range<usize>) -> usize {
    let (mut low, mut high) = (0isize, matches.len() as isize - 1);
    while low <= high {
        let middle = ((low + high) >> 1) as usize;
        let found = &matches[middle];
        if found.start < selection.start
            || (found.start == selection.start && found.end < selection.end)
        {
            low = middle as isize + 1;
        } else if found.start > selection.start
            || (found.start == selection.start && found.end > selection.end)
        {
            high = middle as isize - 1;
        } else {
            return middle + 1;
        }
    }
    0
}

/// What the count slot shows, from `syncCount`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountLabel {
    /// Empty search field.
    Idle,
    Invalid,
    Empty,
    /// `"2 of 14"`, `"14"`, or `"999+"`.
    Ok(String),
}

impl CountLabel {
    pub fn text(&self) -> &str {
        match self {
            Self::Idle => "",
            Self::Invalid => "Invalid regex",
            Self::Empty => "No results",
            Self::Ok(text) => text,
        }
    }

    /// The empty and invalid states color the count and the field border red.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Invalid | Self::Empty)
    }
}

/// `syncCount`.
pub fn count_label(
    query: &SearchQuery,
    matches: &[Range<usize>],
    capped: bool,
    selection: &Range<usize>,
) -> CountLabel {
    if query.search.is_empty() {
        return CountLabel::Idle;
    }
    if !query.valid() {
        return CountLabel::Invalid;
    }
    let total = matches.len();
    if total == 0 {
        return CountLabel::Empty;
    }
    let current = match_index_at_selection(matches, selection);
    let suffix = if capped { "+" } else { "" };
    CountLabel::Ok(if current > 0 {
        format!("{current} of {total}{suffix}")
    } else {
        format!("{total}{suffix}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(search: &str) -> SearchQuery {
        SearchQuery {
            search: search.into(),
            ..Default::default()
        }
    }

    fn ranges(compiled: &CompiledQuery, text: &str) -> Vec<Range<usize>> {
        compiled.count_matches(text).0
    }

    // describe("matchIndexAtSelection")

    #[test]
    fn returns_the_one_based_match_index_for_the_current_selection() {
        let matches = [2..5, 8..11, 14..18];
        assert_eq!(match_index_at_selection(&matches, &(8..11)), 2);
        assert_eq!(match_index_at_selection(&matches, &(14..18)), 3);
    }

    #[test]
    fn returns_zero_when_the_selection_is_not_a_match() {
        let matches = [2..5, 8..11, 14..18];
        assert_eq!(match_index_at_selection(&matches, &(6..8)), 0);
        assert_eq!(match_index_at_selection(&[], &(2..5)), 0);
    }

    // @codemirror/search behavior the panel depends on.

    #[test]
    fn matches_case_insensitively_by_default() {
        let compiled = query("hello").compile().unwrap();
        assert_eq!(
            ranges(&compiled, "Hello hello HELLO"),
            vec![0..5, 6..11, 12..17]
        );
    }

    #[test]
    fn match_case_restricts_to_exact_case() {
        let compiled = SearchQuery {
            case_sensitive: true,
            ..query("hello")
        }
        .compile()
        .unwrap();
        assert_eq!(ranges(&compiled, "Hello hello HELLO"), vec![6..11]);
    }

    #[test]
    fn whole_word_skips_matches_inside_words() {
        let compiled = SearchQuery {
            whole_word: true,
            ..query("cat")
        }
        .compile()
        .unwrap();
        assert_eq!(
            ranges(&compiled, "cat concat cats cat_ (cat)"),
            vec![0..3, 22..25]
        );
    }

    #[test]
    fn whole_word_allows_a_query_that_starts_or_ends_with_punctuation() {
        let compiled = SearchQuery {
            whole_word: true,
            ..query(".x")
        }
        .compile()
        .unwrap();
        assert_eq!(ranges(&compiled, "a.x a.xy"), vec![1..3]);
    }

    #[test]
    fn literal_search_does_not_interpret_regex_syntax() {
        let compiled = query("a.b").compile().unwrap();
        assert_eq!(ranges(&compiled, "a.b axb"), vec![0..3]);
        let compiled = query("\\n").compile().unwrap();
        assert_eq!(ranges(&compiled, "a\\nb\n"), vec![1..3]);
    }

    #[test]
    fn regex_search_uses_line_anchors() {
        let compiled = SearchQuery {
            regexp: true,
            ..query("^b\\w+")
        }
        .compile()
        .unwrap();
        assert_eq!(ranges(&compiled, "alpha\nbeta\nbob\n"), vec![6..10, 11..14]);
    }

    #[test]
    fn an_invalid_regex_is_not_valid() {
        let invalid = SearchQuery {
            regexp: true,
            ..query("(")
        };
        assert!(!invalid.valid());
        assert_eq!(
            count_label(&invalid, &[], false, &(0..0)),
            CountLabel::Invalid
        );
        assert!(!query("").valid());
    }

    #[test]
    fn find_next_wraps_to_the_start() {
        let compiled = query("ab").compile().unwrap();
        let text = "ab ab ab";
        assert_eq!(compiled.next_match(text, 0, 2).unwrap().range, 3..5);
        assert_eq!(compiled.next_match(text, 6, 8).unwrap().range, 0..2);
        // The only match is the selection.
        assert_eq!(compiled.next_match("ab", 0, 2), None);
    }

    #[test]
    fn find_previous_wraps_to_the_end() {
        let compiled = query("ab").compile().unwrap();
        let text = "ab ab ab";
        assert_eq!(compiled.prev_match(text, 3, 3).unwrap().range, 0..2);
        assert_eq!(compiled.prev_match(text, 0, 0).unwrap().range, 6..8);
    }

    #[test]
    fn replace_next_replaces_the_selected_match_and_selects_the_next() {
        let compiled = SearchQuery {
            replace: "xyz".into(),
            ..query("ab")
        }
        .compile()
        .unwrap();
        let text = "ab ab ab";
        let step = compiled.replace_next(text, 0..2).unwrap();
        let change = step.change.unwrap();
        let next = change.apply(text);
        assert_eq!(next, "xyz ab ab");
        assert_eq!(step.select, Some(4..6));
        assert_eq!(&next[4..6], "ab");
    }

    #[test]
    fn replace_next_without_a_selected_match_only_selects() {
        let compiled = SearchQuery {
            replace: "xyz".into(),
            ..query("ab")
        }
        .compile()
        .unwrap();
        let step = compiled.replace_next("zz ab", 0..0).unwrap();
        assert_eq!(step.change, None);
        assert_eq!(step.select, Some(3..5));
    }

    #[test]
    fn replace_all_replaces_every_match() {
        let compiled = SearchQuery {
            replace: "-".into(),
            ..query("a")
        }
        .compile()
        .unwrap();
        assert_eq!(compiled.replace_all("banana"), ("b-n-n-".into(), 3));
    }

    #[test]
    fn regex_replacement_expands_groups() {
        let compiled = SearchQuery {
            regexp: true,
            replace: "$2 $1 [$&] $$ $9".into(),
            ..query("(\\w+)=(\\w+)")
        }
        .compile()
        .unwrap();
        assert_eq!(compiled.replace_all("a=b").0, "b a [a=b] $ $9");
        let compiled = SearchQuery {
            regexp: true,
            replace: "<$10>".into(),
            ..query("(x)")
        }
        .compile()
        .unwrap();
        // `$10` falls back to group 1 followed by "0".
        assert_eq!(compiled.replace_all("x").0, "<x0>");
    }

    #[test]
    fn regex_can_insert_at_every_line_start() {
        let compiled = SearchQuery {
            regexp: true,
            replace: "// ".into(),
            ..query("^")
        }
        .compile()
        .unwrap();
        assert_eq!(compiled.replace_all("a\nb\n").0, "// a\n// b\n// ");
    }

    #[test]
    fn literal_replacement_keeps_dollar_signs() {
        let compiled = SearchQuery {
            replace: "$&".into(),
            ..query("a")
        }
        .compile()
        .unwrap();
        assert_eq!(compiled.replace_all("a").0, "$&");
    }

    #[test]
    fn count_caps_at_999() {
        let compiled = query("a").compile().unwrap();
        let text = "a".repeat(1200);
        let (matches, capped) = compiled.count_matches(&text);
        assert_eq!(matches.len(), MATCH_CAP);
        assert!(capped);
        assert_eq!(
            count_label(&query("a"), &matches, capped, &(0..0)),
            CountLabel::Ok("999+".into())
        );
    }

    #[test]
    fn count_label_states() {
        let q = query("ab");
        assert_eq!(
            count_label(&query(""), &[], false, &(0..0)),
            CountLabel::Idle
        );
        assert_eq!(count_label(&q, &[], false, &(0..0)), CountLabel::Empty);
        assert_eq!(
            count_label(&q, &[0..2, 3..5], false, &(3..5)),
            CountLabel::Ok("2 of 2".into())
        );
        assert_eq!(
            count_label(&q, &[0..2, 3..5], false, &(1..1)),
            CountLabel::Ok("2".into())
        );
    }

    #[test]
    fn reveal_selects_the_first_match_after_the_selection() {
        let compiled = query("ab").compile().unwrap();
        assert_eq!(compiled.reveal_match("ab ab", 1..1), Some(3..5));
        assert_eq!(compiled.reveal_match("ab ab", 4..4), Some(0..2));
        assert_eq!(compiled.reveal_match("ab ab", 3..5), None);
    }

    #[test]
    fn matches_respect_utf8_boundaries() {
        let compiled = query("é").compile().unwrap();
        assert_eq!(ranges(&compiled, "éÉe"), vec![0..2, 2..4]);
        let compiled = SearchQuery {
            whole_word: true,
            ..query("ü")
        }
        .compile()
        .unwrap();
        assert_eq!(ranges(&compiled, "üb ü"), vec![4..6]);
    }
}
