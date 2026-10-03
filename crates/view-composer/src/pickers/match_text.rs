//! Port of src/shared/ui/MatchText.tsx: fuzzy-match text with the matched
//! characters in the accent color.
//!
//! Positions count UTF-16 code units, the way the fuzzy matcher in
//! `monocode-engine` (a port of fuzzy.ts) reports them.

use std::ops::Range;

use gpui::{App, HighlightStyle, IntoElement, RenderOnce, SharedString, StyledText, Window};
use monocode_ui::Theme;

/// One run of `MatchText`: consecutive characters that all match or all
/// do not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchRun {
    pub text: String,
    pub matched: bool,
}

/// The runs `MatchText` renders. Inactive or position-free text is one plain
/// run.
pub fn match_runs(text: &str, positions: &[usize], active: bool) -> Vec<MatchRun> {
    if !active || positions.is_empty() {
        return vec![MatchRun {
            text: text.to_string(),
            matched: false,
        }];
    }
    let mut runs: Vec<MatchRun> = Vec::new();
    let mut unit = 0;
    for ch in text.chars() {
        // A character outside the BMP spans two units; either one marks it,
        // as `text[i]` would in JavaScript for each half.
        let width = ch.len_utf16();
        let matched = (unit..unit + width).any(|i| positions.contains(&i));
        unit += width;
        match runs.last_mut() {
            Some(last) if last.matched == matched => last.text.push(ch),
            _ => runs.push(MatchRun {
                text: ch.to_string(),
                matched,
            }),
        }
    }
    runs
}

/// Byte ranges of the matched runs, for `StyledText::with_highlights`.
pub fn match_ranges(text: &str, positions: &[usize], active: bool) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    for run in match_runs(text, positions, active) {
        let end = offset + run.text.len();
        if run.matched {
            ranges.push(offset..end);
        }
        offset = end;
    }
    ranges
}

/// `<MatchText text positions active />`.
#[derive(IntoElement)]
pub struct MatchText {
    text: SharedString,
    positions: Vec<usize>,
    active: bool,
    suffix: Option<&'static str>,
}

pub fn match_text(text: impl Into<SharedString>, positions: Vec<usize>, active: bool) -> MatchText {
    MatchText {
        text: text.into(),
        positions,
        active,
        suffix: None,
    }
}

impl MatchText {
    /// Plain text after the match, such as a folder's trailing `/`, kept in
    /// the same run so the whole label truncates together.
    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = Some(suffix);
        self
    }
}

impl RenderOnce for MatchText {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let accent = Theme::of(cx).colors.accent;
        let highlights = match_ranges(&self.text, &self.positions, self.active)
            .into_iter()
            .map(|range| {
                (
                    range,
                    HighlightStyle {
                        color: Some(accent),
                        ..Default::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let text: SharedString = match self.suffix {
            Some(suffix) => format!("{}{suffix}", self.text).into(),
            None => self.text,
        };
        StyledText::new(text).with_highlights(highlights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str, positions: &[usize], active: bool) -> Vec<(String, bool)> {
        match_runs(text, positions, active)
            .into_iter()
            .map(|run| (run.text, run.matched))
            .collect()
    }

    #[test]
    fn plain_text_when_inactive_or_unmatched() {
        assert_eq!(
            runs("main.rs", &[0, 1], false),
            vec![("main.rs".into(), false)]
        );
        assert_eq!(runs("main.rs", &[], true), vec![("main.rs".into(), false)]);
    }

    #[test]
    fn groups_consecutive_matches() {
        assert_eq!(
            runs("main.rs", &[0, 1, 5], true),
            vec![
                ("ma".into(), true),
                ("in.".into(), false),
                ("r".into(), true),
                ("s".into(), false),
            ]
        );
        assert_eq!(match_ranges("main.rs", &[0, 1, 5], true), vec![0..2, 5..6]);
    }

    #[test]
    fn counts_utf16_units() {
        // "é" is one unit and two bytes; the emoji is two units.
        assert_eq!(match_ranges("é😀x", &[3], true), vec![6..7]);
        assert_eq!(match_ranges("é😀x", &[1], true), vec![2..6]);
    }
}
