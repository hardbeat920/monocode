//! Port of `ComposerHighlight`, `MentionRuns`, and `FileMentionRuns` in
//! src/features/sessions/ui/Composer.tsx, and `ModeCommandText` in
//! modeCommands.tsx: which ranges of the draft are colored, which glyphs
//! hide under an icon, and where the icons go.
//!
//! The React code split the text into nested spans. Here the same split
//! produces byte ranges for [`crate::composer::prompt_input::PromptInput`].

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::mcp::{McpTag, mcp_tag_parts};
use super::mentions::{ProjectFile, scan_mentions};
use super::mode_commands::{Mode, ModeCommandToken};
use super::skills::skill_ranges;

/// What a colored range is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// A leading mode command's name, in the mode's color.
    Mode(Mode),
    /// A known `/skill` (`text-skill`).
    Skill,
    /// An inline `@mcp/...` tag (`text-mention`).
    McpTag,
    /// A known `@file` mention, line location included (`text-mention`).
    Mention,
}

/// An icon drawn over one hidden glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSlot {
    /// The mode icon, left of the mode command's `/` in the first-line
    /// indent.
    Mode { at: Range<usize>, mode: Mode },
    /// A file-type icon (or a note icon) centered on a mention's `@`.
    File { at: Range<usize>, file: ProjectFile },
}

/// The highlight layer for one draft.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Highlight {
    pub spans: Vec<(Range<usize>, SpanKind)>,
    /// Glyphs that keep their width but paint nothing.
    pub hidden: Vec<Range<usize>>,
    pub icons: Vec<IconSlot>,
    /// The first row is indented by `MODE_COMMAND_INDENT`.
    pub indented: bool,
}

/// `NOTE_PATH_PREFIX`: notes join the mention index with this path prefix.
pub const NOTE_PATH_PREFIX: &str = "note:";

/// `isNoteMentionPath`.
pub fn is_note_mention_path(path: &str) -> bool {
    path.starts_with(NOTE_PATH_PREFIX)
}

/// `ComposerHighlight`.
pub fn composer_highlight(
    text: &str,
    mode: Option<ModeCommandToken>,
    names: &HashSet<String>,
    mentions: &HashMap<String, ProjectFile>,
    mcp_tags: &[McpTag],
) -> Highlight {
    let mut out = Highlight::default();
    let mut rest_start = 0;
    if let Some(mode) = mode {
        // `ModeCommandText`: the `/` keeps its width under the icon.
        out.indented = true;
        out.hidden.push(0..1);
        out.icons.push(IconSlot::Mode {
            at: 0..1,
            mode: mode.mode,
        });
        out.spans.push((1..mode.end, SpanKind::Mode(mode.mode)));
        rest_start = mode.end;
    }
    let rest = &text[rest_start..];
    let mut cursor = 0;
    let mut runs: Vec<Range<usize>> = Vec::new();
    for range in skill_ranges(rest, names) {
        if range.start > cursor {
            runs.push(cursor..range.start);
        }
        out.spans.push((
            rest_start + range.start..rest_start + range.end,
            SpanKind::Skill,
        ));
        cursor = range.end;
    }
    if cursor < rest.len() {
        runs.push(cursor..rest.len());
    }
    // Skill tokens always end on whitespace, so each remaining run still
    // starts on a boundary `@mention` matching can rely on.
    for run in runs {
        let base = rest_start + run.start;
        mention_runs(&rest[run], base, mentions, mcp_tags, &mut out);
    }
    out.spans.sort_by_key(|(range, _)| range.start);
    out
}

/// `MentionRuns`: MCP tags first, then file mentions in what is left.
fn mention_runs(
    text: &str,
    base: usize,
    mentions: &HashMap<String, ProjectFile>,
    mcp_tags: &[McpTag],
    out: &mut Highlight,
) {
    for part in mcp_tag_parts(text, mcp_tags) {
        let range = base + part.range.start..base + part.range.end;
        if part.tag.is_some() {
            out.spans.push((range, SpanKind::McpTag));
        } else {
            file_mention_runs(&text[part.range.clone()], range.start, mentions, out);
        }
    }
}

/// `FileMentionRuns`: the `@` keeps its width so the text stays in place;
/// the file icon sits on top of it.
fn file_mention_runs(
    text: &str,
    base: usize,
    mentions: &HashMap<String, ProjectFile>,
    out: &mut Highlight,
) {
    for hit in scan_mentions(text, mentions) {
        let at = base + hit.start..base + hit.start + 1;
        out.spans
            .push((base + hit.start..base + hit.end, SpanKind::Mention));
        out.hidden.push(at.clone());
        out.icons.push(IconSlot::File { at, file: hit.file });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::model::mcp::McpConnection;
    use crate::composer::model::mentions::build_mention_index;
    use crate::composer::model::mode_commands::leading_mode_command;

    fn names() -> HashSet<String> {
        ["plan", "review-pr", "operator"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn mentions() -> HashMap<String, ProjectFile> {
        build_mention_index(&[
            ProjectFile::new("App.tsx", "/p/src/App.tsx", "src/App.tsx"),
            ProjectFile::new("Plan", "note:abc", "note/plan"),
        ])
        .labels
    }

    fn spans(text: &str, highlight: &Highlight) -> Vec<(String, SpanKind)> {
        highlight
            .spans
            .iter()
            .map(|(range, kind)| (text[range.clone()].to_string(), kind.clone()))
            .collect()
    }

    #[test]
    fn colors_a_leading_mode_command_and_hides_its_slash() {
        let text = "/plan fix @App.tsx with /review-pr";
        let mode = leading_mode_command(text, &names());
        let highlight = composer_highlight(text, mode, &names(), &mentions(), &[]);
        assert!(highlight.indented);
        assert_eq!(
            spans(text, &highlight),
            vec![
                ("plan".to_string(), SpanKind::Mode(Mode::Plan)),
                ("@App.tsx".to_string(), SpanKind::Mention),
                ("/review-pr".to_string(), SpanKind::Skill),
            ]
        );
        assert_eq!(highlight.hidden, vec![0..1, 10..11]);
        assert_eq!(
            highlight.icons[0],
            IconSlot::Mode {
                at: 0..1,
                mode: Mode::Plan
            }
        );
        match &highlight.icons[1] {
            IconSlot::File { at, file } => {
                assert_eq!(at, &(10..11));
                assert_eq!(file.relative, "src/App.tsx");
            }
            other => panic!("expected a file icon, got {other:?}"),
        }
    }

    #[test]
    fn without_a_mode_the_first_row_is_not_indented() {
        let text = "/review-pr please";
        let highlight = composer_highlight(text, None, &names(), &mentions(), &[]);
        assert!(!highlight.indented);
        assert_eq!(
            spans(text, &highlight),
            vec![("/review-pr".to_string(), SpanKind::Skill)]
        );
        assert!(highlight.hidden.is_empty());
    }

    #[test]
    fn colors_mcp_tags_before_file_mentions() {
        let tag = McpTag {
            server: McpConnection {
                provider: "claude".into(),
                name: "docs".into(),
                ..McpConnection::default()
            },
            token: "@mcp/docs".into(),
        };
        let text = "ask @mcp/docs and @src/App.tsx (line 3)";
        let highlight = composer_highlight(text, None, &names(), &mentions(), &[tag]);
        assert_eq!(
            spans(text, &highlight),
            vec![
                ("@mcp/docs".to_string(), SpanKind::McpTag),
                ("@src/App.tsx (line 3)".to_string(), SpanKind::Mention),
            ]
        );
        assert_eq!(highlight.icons.len(), 1);
    }

    #[test]
    fn notes_mention_with_a_note_icon_path() {
        let text = "see @note/plan";
        let highlight = composer_highlight(text, None, &names(), &mentions(), &[]);
        match &highlight.icons[0] {
            IconSlot::File { file, .. } => assert!(is_note_mention_path(&file.path)),
            other => panic!("expected a file icon, got {other:?}"),
        }
    }

    #[test]
    fn mentions_inside_a_blockquote_stay_plain() {
        let text = "> @App.tsx";
        let highlight = composer_highlight(text, None, &names(), &mentions(), &[]);
        assert!(highlight.spans.is_empty());
    }
}
