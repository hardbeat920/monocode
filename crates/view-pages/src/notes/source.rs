//! The note's Source tab: NotesView.tsx `NoteSource`, a 13px monospace
//! field with a line number gutter, where ATX heading lines take the
//! markdown heading color (`MarkdownSourceHighlight`).
//!
//! React laid a transparent textarea over a highlighted mirror. Here it is
//! gpui-base's code editor with line numbers, soft wrap, and a small
//! highlighter that colors heading lines.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Entity, HighlightStyle, Hsla, SharedString, Window};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputEditorStyle, InputHighlighter,
    Rope,
};
use monocode_ui::Theme;

use super::model::is_atx_heading_line;

/// The highlighter's language name.
const LANGUAGE: &str = "monocode-markdown-source";
const HEADING: &str = "heading";

/// Colors heading lines.
#[derive(Default)]
struct MarkdownSourceHighlighter {
    headings: Vec<Range<usize>>,
}

impl InputHighlighter for MarkdownSourceHighlighter {
    fn language(&self) -> SharedString {
        LANGUAGE.into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.headings = heading_ranges(&text.to_string());
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let heading = resolver.style(HEADING).unwrap_or_default();
        let mut runs = Vec::new();
        let mut cursor = range.start;
        let first = self
            .headings
            .partition_point(|heading| heading.end <= range.start);
        for line in &self.headings[first..] {
            if line.start >= range.end {
                break;
            }
            let start = line.start.max(range.start);
            let end = line.end.min(range.end);
            if start >= end {
                continue;
            }
            if cursor < start {
                runs.push((cursor..start, HighlightStyle::default()));
            }
            runs.push((start..end, heading));
            cursor = end;
        }
        if cursor < range.end {
            runs.push((cursor..range.end, HighlightStyle::default()));
        }
        runs
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

/// The byte ranges of heading lines, without their line breaks.
pub fn heading_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        if is_atx_heading_line(line) && !line.is_empty() {
            ranges.push(offset..offset + line.len());
        }
        offset += line.len() + 1;
    }
    ranges
}

struct HeadingStyle(Hsla);

impl HighlightStyleResolver for HeadingStyle {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        (name == HEADING).then(|| HighlightStyle {
            color: Some(self.0),
            ..Default::default()
        })
    }
}

/// The source field for `body`.
pub fn source_editor(body: &str, window: &mut Window, cx: &mut App) -> Entity<EditorState> {
    let body = body.to_string();
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(LANGUAGE)
            .line_number(true)
            .folding(false)
            .indent_guides(false)
            .soft_wrap(true)
            .searchable(false)
            .placeholder("Write markdown…")
            .default_value(body)
    });
    editor.update(cx, |editor, cx| {
        editor.set_highlighter_factory(
            Rc::new(|language| {
                (language == LANGUAGE).then(|| {
                    Box::new(MarkdownSourceHighlighter::default()) as Box<dyn InputHighlighter>
                })
            }),
            cx,
        );
    });
    editor
}

/// Page colors: `content/85` text, a `content/40` gutter and placeholder,
/// and the markdown heading color.
pub fn source_style(theme: &Theme) -> InputEditorStyle {
    InputEditorStyle {
        foreground: theme.content(0.85),
        muted_foreground: theme.content(0.40),
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        highlight_styles: Arc::new(HeadingStyle(theme.colors.markdown_heading)),
        editor_gutter_background: Some(gpui::transparent_black()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_heading_lines() {
        assert_eq!(heading_ranges("# One\ntext\n## Two"), vec![0..5, 11..17]);
        assert!(heading_ranges("#tag\n    # code").is_empty());
    }
}
