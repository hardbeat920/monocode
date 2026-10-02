//! Tree-sitter highlighting for the editor and the diff view.
//!
//! gpui-component keeps its own `InputHighlighter` adapter private, so this is
//! a small adapter over its public `SyntaxHighlighter`, modeled on
//! gpui-component's `highlighter/input_adapter.rs` (Apache-2.0). Edits parse
//! on the main thread with a 2 ms budget. A parse that runs over the budget is
//! redone from scratch on a background thread and swapped in when it is done.
//!
//! [`highlight_diff_sides`] is the port of `highlightDiffFile` in
//! src/features/files/editor/syntaxTokens.ts.

use std::{cell::RefCell, ops::Range, rc::Rc, time::Duration};

use gpui::{Context, HighlightStyle, SharedString, Task, Window};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_component::highlighter::{LanguageRegistry, SyntaxHighlighter};

const SYNC_PARSE_TIMEOUT: Duration = Duration::from_millis(2);
const SYNC_PARSE_MAX_BYTES: usize = 256 * 1024;
const PARSE_DEBOUNCE: Duration = Duration::from_millis(150);

/// `MAX_DIFF_HIGHLIGHT_CHARS`: past this, a diff side shows unstyled.
pub const MAX_DIFF_HIGHLIGHT_CHARS: usize = 250_000;

/// Whether gpui-component has a grammar for `language`.
pub fn has_grammar(language: &str) -> bool {
    LanguageRegistry::singleton()
        .language(language)
        .is_some_and(|config| config.has_grammar())
}

/// The factory to install with `EditorState::set_highlighter_factory`.
pub fn highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        has_grammar(language)
            .then(|| Box::new(TreeSitterHighlighter::new(language)) as Box<dyn InputHighlighter>)
    })
}

struct TreeSitterHighlighter {
    language: SharedString,
    inner: Rc<RefCell<SyntaxHighlighter>>,
    parse_task: Rc<RefCell<Option<Task<()>>>>,
}

impl TreeSitterHighlighter {
    fn new(language: &str) -> Self {
        Self {
            language: language.to_owned().into(),
            inner: Rc::new(RefCell::new(SyntaxHighlighter::new(language))),
            parse_task: Rc::new(RefCell::new(None)),
        }
    }
}

fn to_tree_sitter_edit(edit: InputEdit) -> tree_sitter::InputEdit {
    tree_sitter::InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: tree_sitter::Point::new(
            edit.start_position.row,
            edit.start_position.column,
        ),
        old_end_position: tree_sitter::Point::new(
            edit.old_end_position.row,
            edit.old_end_position.column,
        ),
        new_end_position: tree_sitter::Point::new(
            edit.new_end_position.row,
            edit.new_end_position.column,
        ),
    }
}

impl InputHighlighter for TreeSitterHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        let edit = edit.map(to_tree_sitter_edit);
        let completed = {
            let mut highlighter = self.inner.borrow_mut();
            if text.len() > SYNC_PARSE_MAX_BYTES {
                highlighter.edit_tree(edit, text);
                false
            } else {
                highlighter.update(edit, text, Some(SYNC_PARSE_TIMEOUT))
            }
        };
        if completed {
            self.parse_task.borrow_mut().take();
            return;
        }

        let inner = self.inner.clone();
        let language = self.language.clone();
        let text = text.clone();
        let task = cx.spawn_in(window, async move |entity, cx| {
            cx.background_executor().timer(PARSE_DEBOUNCE).await;
            let (fresh, folds) = cx
                .background_executor()
                .spawn(async move {
                    let mut fresh = SyntaxHighlighter::new(&language);
                    fresh.update(None, &text, None);
                    let folds = if folding {
                        fresh.tree().map(extract_fold_ranges).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    (fresh, folds)
                })
                .await;
            *inner.borrow_mut() = fresh;
            let _ = entity.update(cx, |state, cx| {
                state.apply_highlighter_fold_candidates(folds, cx);
            });
        });
        self.parse_task.borrow_mut().replace(task);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.inner.borrow().styles(range, resolver)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(extract_fold_ranges)
            .unwrap_or_default()
    }
}

/// Fold candidates: every named node that spans at least three lines.
/// Same rule as gpui-component's adapter.
fn extract_fold_ranges(tree: &tree_sitter::Tree) -> Vec<FoldRange> {
    fn collect(node: tree_sitter::Node, ranges: &mut Vec<FoldRange>) {
        let start = node.start_position().row;
        let end = node.end_position().row;
        if end.saturating_sub(start) < 2 {
            return;
        }
        ranges.push(FoldRange::new(start, end));
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect(child, ranges);
        }
    }

    let root = tree.root_node();
    let mut ranges = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        collect(child, &mut ranges);
    }
    ranges.sort_by_key(|range| range.start_line);
    ranges.dedup_by_key(|range| range.start_line);
    ranges
}

/// Style runs for one line, with ranges relative to the line start.
pub type LineStyles = Vec<(Range<usize>, HighlightStyle)>;

/// `highlightSource`: style runs per line of `text`. Plain text (no grammar)
/// gives empty runs.
pub fn highlight_source(
    text: &str,
    language: Option<&str>,
    resolver: &dyn HighlightStyleResolver,
) -> Vec<LineStyles> {
    let line_ranges = line_ranges(text);
    let Some(language) = language.filter(|language| has_grammar(language)) else {
        return line_ranges.iter().map(|_| Vec::new()).collect();
    };
    let rope = Rope::from(text);
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &rope, None);
    line_ranges
        .into_iter()
        .map(|range| {
            if range.is_empty() {
                return Vec::new();
            }
            let start = range.start;
            highlighter
                .styles(&range, resolver)
                .into_iter()
                .filter(|(_, style)| *style != HighlightStyle::default())
                .map(|(run, style)| (run.start - start..run.end - start, style))
                .collect()
        })
        .collect()
}

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            ranges.push(start..index);
            start = index + 1;
        }
    }
    ranges.push(start..text.len());
    ranges
}

/// One side of a diff to highlight: the texts of its lines, in order.
pub struct DiffSide<'a> {
    pub lines: Vec<&'a str>,
}

/// `highlightDiffFile`: style runs for each side, line by line, or `None`
/// when a side is past [`MAX_DIFF_HIGHLIGHT_CHARS`].
pub fn highlight_diff_sides(
    old: &DiffSide,
    new: &DiffSide,
    language: Option<&str>,
    resolver: &dyn HighlightStyleResolver,
) -> Option<(Vec<LineStyles>, Vec<LineStyles>)> {
    let size = |side: &DiffSide| side.lines.iter().map(|line| line.len() + 1).sum::<usize>();
    if size(old) > MAX_DIFF_HIGHLIGHT_CHARS || size(new) > MAX_DIFF_HIGHLIGHT_CHARS {
        return None;
    }
    let highlight = |side: &DiffSide| {
        if side.lines.is_empty() {
            return Vec::new();
        }
        let mut styles = highlight_source(&side.lines.join("\n"), language, resolver);
        styles.truncate(side.lines.len());
        styles
    };
    Some((highlight(old), highlight(new)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{SyntaxPalette, SyntaxResolver};
    use crate::unified_diff::{UnifiedLineKind, build_unified_file};

    fn color_of(text: &str, styles: &LineStyles, needle: &str) -> Option<gpui::Hsla> {
        let start = text.find(needle)?;
        styles
            .iter()
            .find(|(range, _)| range.start <= start && start < range.end)
            .and_then(|(_, style)| style.color)
    }

    #[test]
    fn colors_typescript_keywords_strings_and_comments() {
        let palette = SyntaxPalette::dark();
        let resolver = SyntaxResolver::new(palette, "typescript");
        let source = "const name = \"agent\";\n// note";
        let lines = highlight_source(source, Some("typescript"), &resolver);
        let first = "const name = \"agent\";";
        assert_eq!(color_of(first, &lines[0], "const"), Some(palette.keyword));
        assert_eq!(
            color_of(first, &lines[0], "\"agent\""),
            Some(palette.string)
        );
        assert_eq!(
            color_of("// note", &lines[1], "// note"),
            Some(palette.comment)
        );
    }

    #[test]
    fn colors_comments_in_jsonc_files() {
        let palette = SyntaxPalette::dark();
        let language = crate::language::language_for_path("settings.jsonc");
        let resolver = SyntaxResolver::new(palette, language.unwrap());
        let lines = highlight_source("{\n  // note\n  \"a\": 1\n}", language, &resolver);
        assert_eq!(
            color_of("  // note", &lines[1], "// note"),
            Some(palette.comment)
        );
    }

    #[test]
    fn leaves_unknown_languages_unstyled() {
        let resolver = SyntaxResolver::new(SyntaxPalette::dark(), "text");
        assert_eq!(
            highlight_source("plain text", None, &resolver),
            vec![Vec::new()]
        );
    }

    #[test]
    fn highlights_added_and_deleted_lines_from_each_side() {
        let palette = SyntaxPalette::dark();
        let resolver = SyntaxResolver::new(palette, "typescript");
        let diff = build_unified_file("const alpha = 1;\n", "const beta = 1;\n", 3);
        let old = DiffSide {
            lines: diff
                .lines
                .iter()
                .filter(|line| line.kind != UnifiedLineKind::Add)
                .map(|line| line.text.as_str())
                .collect(),
        };
        let new = DiffSide {
            lines: diff
                .lines
                .iter()
                .filter(|line| line.kind != UnifiedLineKind::Del)
                .map(|line| line.text.as_str())
                .collect(),
        };
        let (old_styles, new_styles) =
            highlight_diff_sides(&old, &new, Some("typescript"), &resolver).unwrap();
        assert_eq!(
            color_of("const alpha", &old_styles[0], "const"),
            Some(palette.keyword)
        );
        assert_eq!(
            color_of("const beta", &new_styles[0], "const"),
            Some(palette.keyword)
        );
    }

    #[test]
    fn skips_decorative_parsing_for_a_very_large_diff() {
        let resolver = SyntaxResolver::new(SyntaxPalette::dark(), "typescript");
        let long = "x".repeat(250_001);
        let new = DiffSide { lines: vec![&long] };
        let old = DiffSide { lines: Vec::new() };
        assert!(highlight_diff_sides(&old, &new, Some("typescript"), &resolver).is_none());
    }
}
