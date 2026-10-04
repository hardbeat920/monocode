//! Port of src/features/sessions/model/promptOutline.ts: which prompt the
//! outline rail marks, which bars fit, how a hovered bar lifts its
//! neighbours, and the preview card's lines.

use std::sync::LazyLock;

use monocode_core::block::SecondOpinionKind;
use monocode_core::{Block, BlockRole};
use regex::Regex;

use crate::transcript::model::chat_context::{
    ChatContextItem, context_excerpt, context_file_name, line_range, split_chat_context,
};

/// A vertical span in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineBand {
    pub top: f32,
    pub bottom: f32,
}

/// `OutlineAnchor`: a prompt's band.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineAnchor {
    pub id: String,
    pub top: f32,
    pub bottom: f32,
}

/// `NEAR_END_PX`.
pub const NEAR_END_PX: f32 = 16.;

/// `promptBlocks`: the prompts the reader typed, without the app's own.
pub fn prompt_blocks<B>(blocks: &[B]) -> Vec<&B>
where
    B: std::ops::Deref<Target = Block>,
{
    blocks
        .iter()
        .filter(|block| block.role == BlockRole::User && !block.is_internal())
        .collect()
}

/// `activePromptId`: the topmost prompt inside the viewport. With none
/// inside, the last prompt above it, else the first prompt. Near the end of
/// the transcript, the last prompt: the prompts on the final screen cannot
/// reach the top.
pub fn active_prompt_id(
    viewport: OutlineBand,
    anchors: &[OutlineAnchor],
    distance_to_end: f32,
) -> Option<String> {
    let last = anchors.last()?;
    if distance_to_end <= NEAR_END_PX {
        return Some(last.id.clone());
    }
    if let Some(inside) = anchors
        .iter()
        .find(|anchor| anchor.bottom > viewport.top && anchor.top < viewport.bottom)
    {
        return Some(inside.id.clone());
    }
    let above = anchors.iter().rfind(|anchor| anchor.bottom <= viewport.top);
    Some(above.unwrap_or(&anchors[0]).id.clone())
}

/// `barWindow`: at most `max` prompts. The window slides to keep the
/// active prompt inside and prefers the newest prompts.
pub fn bar_window(count: usize, active_index: Option<usize>, max: usize) -> (usize, usize) {
    if count <= max {
        return (0, count);
    }
    let newest = count - max;
    let start = match active_index {
        None => newest,
        Some(index) => index.min(newest),
    };
    (start, start + max)
}

/// `chatContextLabel` from chatContext.ts: one line naming an item.
pub fn chat_context_label(item: &ChatContextItem) -> String {
    match item {
        ChatContextItem::Quote { text } => context_excerpt(text),
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => format!(
            "{}:{}",
            context_file_name(path),
            line_range(*start_line, *end_line)
        ),
        ChatContextItem::Comment {
            path,
            line,
            comment,
            ..
        } => {
            let location = match line {
                Some(line) => format!("{}:{line}", context_file_name(path)),
                None => context_file_name(path),
            };
            format!("{location} {}", context_excerpt(comment))
        }
    }
}

/// `chatContextSummary`: the first item's label and how many follow it.
pub fn chat_context_summary(items: &[ChatContextItem]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let label = chat_context_label(first);
    if items.len() > 1 {
        format!("{label} +{}", items.len() - 1)
    } else {
        label
    }
}

/// `promptLabel`: a prompt's first line of typed text, or what it carried.
pub fn prompt_label(block: &Block) -> String {
    let card = block.second_opinion.as_ref();
    let text_shown = card.is_none_or(|card| card.kind == Some(SecondOpinionKind::Handoff));
    let prompt = if text_shown {
        split_chat_context(&block.text)
    } else {
        crate::transcript::model::chat_context::ChatContextMessage {
            text: String::new(),
            items: Vec::new(),
        }
    };
    let text = first_line(&prompt.text);
    if !text.is_empty() {
        return text;
    }
    if let Some(card) = card {
        if card.kind == Some(SecondOpinionKind::Handoff) {
            return "Handoff".into();
        }
        let request = first_line(card.request.as_deref().unwrap_or(""));
        return if request.is_empty() {
            "Second opinion".into()
        } else {
            format!("Second opinion: {request}")
        };
    }
    if let Some(note) = block
        .note_card
        .as_ref()
        .filter(|note| !note.title.is_empty())
    {
        return note.title.clone();
    }
    if !prompt.items.is_empty() {
        return chat_context_summary(&prompt.items);
    }
    let files = block.attachments.as_deref().unwrap_or(&[]);
    if let Some(first) = files.first() {
        return if files.len() > 1 {
            format!("{} +{}", first.name, files.len() - 1)
        } else {
            first.name.clone()
        };
    }
    "Empty message".into()
}

/// `firstLine`: the first non-blank line with its whitespace collapsed.
fn first_line(text: &str) -> String {
    let line = text
        .split('\n')
        .map(|part| monocode_core::js::trim(part.strip_suffix('\r').unwrap_or(part)))
        .find(|part| !part.is_empty())
        .unwrap_or("");
    collapse_whitespace(line)
}

/// `.replace(/\s+/g, " ")`.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if monocode_core::js::is_space(ch) || monocode_core::js::is_line_terminator(ch) {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(ch);
            space = false;
        }
    }
    out
}

/// `RIPPLE_SPAN`: how far the hover ripple reaches, in bars on each side.
pub const RIPPLE_SPAN: usize = 2;

/// `barLift`: dock-style magnification, 1 on the hovered bar, tapering to
/// 0 past the ripple span.
pub fn bar_lift(index: usize, hover_index: Option<usize>) -> f32 {
    let Some(hover) = hover_index else {
        return 0.;
    };
    let distance = index.abs_diff(hover);
    if distance > RIPPLE_SPAN {
        return 0.;
    }
    (RIPPLE_SPAN + 1 - distance) as f32 / (RIPPLE_SPAN + 1) as f32
}

/// `PromptPreview`: the prompt and the head of its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPreview {
    pub title: String,
    pub reply: Option<String>,
    pub detail: Option<String>,
}

const REPLY_SCAN_CHARS: usize = 2000;

/// `promptPreview`.
pub fn prompt_preview<B>(blocks: &[B], prompt_id: &str) -> Option<PromptPreview>
where
    B: std::ops::Deref<Target = Block>,
{
    let index = blocks.iter().position(|block| block.id == prompt_id)?;
    let mut reply: Vec<String> = Vec::new();
    for block in &blocks[index + 1..] {
        if block.role == BlockRole::User {
            break;
        }
        if block.role != BlockRole::Assistant {
            continue;
        }
        reply = preview_lines(&block.text, 2);
        if !reply.is_empty() {
            break;
        }
    }
    let mut lines = reply.into_iter();
    Some(PromptPreview {
        title: prompt_label(&blocks[index]),
        reply: lines.next(),
        detail: lines.next(),
    })
}

static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[#>]+|[-*+]|\d+[.)])\s+").expect("marker pattern"));
static EMPHASIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*|`").expect("emphasis"));
static WORDY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]").expect("wordy"));

/// `previewLines`: the first `max` prose lines of a reply. Markers, fenced
/// code, and rules drop out.
pub fn preview_lines(text: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut fenced = false;
    // `text.slice(0, REPLY_SCAN_CHARS)` counts UTF-16 units.
    let head = monocode_core::js::slice_prefix(text, REPLY_SCAN_CHARS);
    for raw in head.split('\n') {
        let line = monocode_core::js::trim(raw.strip_suffix('\r').unwrap_or(raw));
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let plain = MARKER.replace(line, "");
        let plain = EMPHASIS.replace_all(&plain, "");
        let plain = collapse_whitespace(&plain);
        let plain = monocode_core::js::trim(&plain).to_string();
        // Rules, table separators, and lone punctuation read as noise.
        if !WORDY.is_match(&plain) {
            continue;
        }
        lines.push(plain);
        if lines.len() == max {
            break;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::model::chat_context::compose_chat_context;

    const VIEWPORT: OutlineBand = OutlineBand {
        top: 100.,
        bottom: 500.,
    };

    fn anchor(id: &str, top: f32, bottom: f32) -> OutlineAnchor {
        OutlineAnchor {
            id: id.into(),
            top,
            bottom,
        }
    }

    fn active(anchors: &[OutlineAnchor], distance: f32) -> Option<String> {
        active_prompt_id(VIEWPORT, anchors, distance)
    }

    const FAR: f32 = f32::INFINITY;

    #[test]
    fn returns_null_with_no_anchors() {
        assert_eq!(active(&[], FAR), None);
    }

    #[test]
    fn picks_the_topmost_prompt_inside_the_viewport() {
        let anchors = [
            anchor("a", 0., 40.),
            anchor("b", 150., 190.),
            anchor("c", 300., 340.),
            anchor("d", 600., 640.),
        ];
        assert_eq!(active(&anchors, FAR).as_deref(), Some("b"));
    }

    #[test]
    fn counts_a_prompt_cut_by_the_viewport_top_as_inside() {
        let anchors = [anchor("a", 80., 120.), anchor("b", 200., 240.)];
        assert_eq!(active(&anchors, FAR).as_deref(), Some("a"));
    }

    #[test]
    fn lets_a_prompt_that_peeks_in_at_the_bottom_win_over_the_reply_above() {
        let anchors = [anchor("a", 0., 40.), anchor("b", 480., 520.)];
        assert_eq!(active(&anchors, FAR).as_deref(), Some("b"));
    }

    #[test]
    fn falls_back_to_the_last_prompt_above_the_viewport() {
        let anchors = [
            anchor("a", 0., 20.),
            anchor("b", 40., 60.),
            anchor("c", 600., 640.),
        ];
        assert_eq!(active(&anchors, FAR).as_deref(), Some("b"));
    }

    #[test]
    fn treats_a_prompt_that_ends_at_the_viewport_top_as_above() {
        let anchors = [anchor("a", 60., 100.), anchor("b", 700., 740.)];
        assert_eq!(active(&anchors, FAR).as_deref(), Some("a"));
    }

    #[test]
    fn falls_back_to_the_first_prompt_when_all_sit_below() {
        let anchors = [anchor("a", 600., 640.), anchor("b", 700., 740.)];
        assert_eq!(active(&anchors, FAR).as_deref(), Some("a"));
    }

    #[test]
    fn marks_the_last_prompt_at_the_end_of_the_transcript() {
        let anchors = [
            anchor("a", 120., 160.),
            anchor("b", 260., 300.),
            anchor("c", 400., 440.),
        ];
        assert_eq!(active(&anchors, 0.).as_deref(), Some("c"));
        assert_eq!(active(&anchors, 16.).as_deref(), Some("c"));
    }

    #[test]
    fn keeps_the_topmost_visible_prompt_while_there_is_room_to_scroll() {
        let anchors = [anchor("a", 120., 160.), anchor("b", 400., 440.)];
        assert_eq!(active(&anchors, 17.).as_deref(), Some("a"));
    }

    #[test]
    fn bar_lift_is_flat_with_no_hovered_bar() {
        assert_eq!(bar_lift(3, None), 0.);
    }

    #[test]
    fn bar_lift_peaks_on_the_hovered_bar_and_tapers_over_the_ripple_span() {
        assert_eq!(bar_lift(3, Some(3)), 1.);
        assert!((bar_lift(2, Some(3)) - 2. / 3.).abs() < 1e-6);
        assert!((bar_lift(5, Some(3)) - 1. / 3.).abs() < 1e-6);
        assert_eq!(bar_lift(6, Some(3)), 0.);
    }

    #[test]
    fn bar_window_slides_to_keep_the_active_prompt() {
        assert_eq!(bar_window(5, None, 10), (0, 5));
        assert_eq!(bar_window(20, None, 10), (10, 20));
        assert_eq!(bar_window(20, Some(3), 10), (3, 13));
        assert_eq!(bar_window(20, Some(15), 10), (10, 20));
    }

    #[test]
    fn preview_lines_strip_markers_and_collapse_blank_lines() {
        let text = "# Heading\n\n- **first** point\n\n2) second point\n";
        assert_eq!(preview_lines(text, 2), ["Heading", "first point"]);
    }

    #[test]
    fn preview_lines_skip_fenced_code_and_rules() {
        let text = "---\n```ts\nconst a = 1;\n```\nAfter the code.";
        assert_eq!(preview_lines(text, 2), ["After the code."]);
    }

    #[test]
    fn preview_lines_stop_at_the_line_budget() {
        assert_eq!(preview_lines("a\nb\nc", 2), ["a", "b"]);
    }

    fn block(id: &str, role: BlockRole, text: &str) -> std::sync::Arc<Block> {
        std::sync::Arc::new(Block::new(id, role, text))
    }

    #[test]
    fn pairs_a_prompt_with_the_head_of_its_reply() {
        let blocks = [
            block("u1", BlockRole::User, "First ask"),
            block("t1", BlockRole::Tool, "ran something"),
            block("a1", BlockRole::Assistant, "Yes.\nBuild the control plane."),
            block("u2", BlockRole::User, "Second ask"),
            block("a2", BlockRole::Assistant, "Later reply"),
        ];
        assert_eq!(
            prompt_preview(&blocks, "u1"),
            Some(PromptPreview {
                title: "First ask".into(),
                reply: Some("Yes.".into()),
                detail: Some("Build the control plane.".into()),
            })
        );
    }

    #[test]
    fn leaves_the_reply_out_when_the_turn_has_none_yet() {
        let blocks = [block("u1", BlockRole::User, "Only ask")];
        assert_eq!(
            prompt_preview(&blocks, "u1"),
            Some(PromptPreview {
                title: "Only ask".into(),
                reply: None,
                detail: None,
            })
        );
    }

    #[test]
    fn returns_null_for_an_unknown_prompt() {
        let blocks: [std::sync::Arc<Block>; 0] = [];
        assert_eq!(prompt_preview(&blocks, "missing"), None);
    }

    #[test]
    fn titles_a_prompt_by_its_typed_text_else_by_its_attached_context() {
        let code = ChatContextItem::Code {
            path: "src/app/App.tsx".into(),
            start_line: 12,
            end_line: 40,
        };
        let blocks = [
            block(
                "u1",
                BlockRole::User,
                &compose_chat_context("Explain this", std::slice::from_ref(&code)),
            ),
            block("u2", BlockRole::User, &compose_chat_context("", &[code])),
        ];
        assert_eq!(prompt_preview(&blocks, "u1").unwrap().title, "Explain this");
        assert_eq!(
            prompt_preview(&blocks, "u2").unwrap().title,
            "App.tsx:12-40"
        );
    }

    #[test]
    fn labels_cards_notes_and_files_when_there_is_no_text() {
        let mut note = Block::new("n", BlockRole::User, "");
        note.note_card = Some(monocode_core::notes::NoteCardMeta {
            title: "Release plan".into(),
            ..Default::default()
        });
        assert_eq!(prompt_label(&note), "Release plan");
        let empty = Block::new("e", BlockRole::User, "  \n ");
        assert_eq!(prompt_label(&empty), "Empty message");
        let typed = Block::new("t", BlockRole::User, "\n  two   words \nmore");
        assert_eq!(prompt_label(&typed), "two words");
    }
}
