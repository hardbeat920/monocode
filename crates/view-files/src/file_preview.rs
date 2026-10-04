//! Port of src/features/files/ui/FilePreview.tsx: the file card a tool call
//! shows for an edit or write, with its first changed lines.

use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{
    App, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, StyledText, Window, div, prelude::FluentBuilder as _, px,
};
use monocode_core::block::{ToolPreview, ToolPreviewLine, ToolPreviewLineKind};
use monocode_core::reducer::MAX_PREVIEW_LINES;
use monocode_core::transcript::paths::resolve_workspace_path;
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::styled::format_integer;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};
use regex::Regex;

use crate::paths::display_path;

/// The tool call's review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewStatus {
    Pending,
    Accepted,
    Rejected,
}

/// `variant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewVariant {
    /// A bordered card in the transcript.
    #[default]
    Card,
    /// Inside a popover: no frame, and the lines scroll.
    Popover,
}

/// `KEYWORDS`.
const KEYWORDS: &[&str] = &[
    "import",
    "export",
    "from",
    "type",
    "interface",
    "const",
    "let",
    "var",
    "function",
    "return",
    "if",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "class",
    "struct",
    "enum",
    "extends",
    "implements",
    "new",
    "async",
    "await",
    "try",
    "catch",
    "throw",
    "true",
    "false",
    "null",
    "undefined",
    "this",
    "in",
    "of",
    "as",
    "is",
    "void",
    "public",
    "private",
    "protected",
    "static",
    "default",
    "package",
    "def",
    "func",
];

/// Tailwind colors the card names directly (`text-teal-300`,
/// `text-amber-200/90`). monocode-ui has no tokens for these shades. The
/// added and removed rows use the diff palette's tokens.
struct Palette {
    keyword: Hsla,
    type_name: Hsla,
}

fn palette() -> Palette {
    Palette {
        keyword: hex(0x5eead4),
        type_name: with_alpha(hex(0xfde68a), 0.90),
    }
}

/// `fileNameOf`.
fn file_name_of(path: Option<&str>) -> Option<String> {
    path?
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .map(str::to_string)
}

/// A token of a highlighted preview line: its text and its color, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Plain(String),
    Keyword(String),
    TypeName(String),
}

/// What `highlight` paints: a comment line, or tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Highlighted {
    Comment(String),
    Tokens(Vec<Token>),
}

/// `highlight`: comments dimmed, keywords and capitalized names tinted.
pub fn highlight(text: &str) -> Highlighted {
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\b").expect("word pattern"));
    let trimmed = text.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return Highlighted::Comment(text.to_string());
    }
    let mut parts = Vec::new();
    let mut last = 0;
    for found in WORD.find_iter(text) {
        if found.start() > last {
            parts.push(Token::Plain(text[last..found.start()].to_string()));
        }
        let token = found.as_str();
        parts.push(if KEYWORDS.contains(&token) {
            Token::Keyword(token.to_string())
        } else if token.starts_with(|c: char| c.is_ascii_uppercase()) {
            Token::TypeName(token.to_string())
        } else {
            Token::Plain(token.to_string())
        });
        last = found.end();
    }
    if last < text.len() {
        parts.push(Token::Plain(text[last..].to_string()));
    }
    Highlighted::Tokens(parts)
}

type OpenHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// `FilePreview`.
#[derive(IntoElement)]
pub struct FilePreview {
    id: SharedString,
    preview: ToolPreview,
    status: PreviewStatus,
    cwd: Option<String>,
    on_open_file: Option<OpenHandler>,
    variant: PreviewVariant,
}

/// A preview card. `id` keys the card's interactive elements.
pub fn file_preview(
    id: impl Into<SharedString>,
    preview: ToolPreview,
    status: PreviewStatus,
) -> FilePreview {
    FilePreview {
        id: id.into(),
        preview,
        status,
        cwd: None,
        on_open_file: None,
        variant: PreviewVariant::Card,
    }
}

impl FilePreview {
    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Makes the path a link that opens the file.
    pub fn on_open_file(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_file = Some(Rc::new(handler));
        self
    }

    pub fn variant(mut self, variant: PreviewVariant) -> Self {
        self.variant = variant;
        self
    }

    /// The lines the card shows: changed and context lines, at most
    /// `MAX_PREVIEW_LINES`.
    pub fn shown_lines(preview: &ToolPreview) -> Vec<ToolPreviewLine> {
        preview
            .lines
            .as_deref()
            .unwrap_or_default()
            .iter()
            .take(MAX_PREVIEW_LINES)
            .cloned()
            .collect()
    }
}

fn render_line(line: &ToolPreviewLine, scrollable: bool, theme: &Theme) -> impl IntoElement {
    let colors = palette();
    let c = &theme.colors;
    let (bg, bar, mark, mark_color) = match line.kind {
        ToolPreviewLineKind::Add => (Some(c.diff_add_bg), c.diff_add, "+", c.diff_add_fg),
        ToolPreviewLineKind::Del => (Some(c.diff_del_bg), c.diff_del, "−", c.diff_del_fg),
        ToolPreviewLineKind::Context => (
            None,
            gpui::transparent_black(),
            " ",
            gpui::transparent_black(),
        ),
    };
    let dimmed = line.kind == ToolPreviewLineKind::Context;
    let body = match highlight(&line.text) {
        Highlighted::Comment(text) => div()
            .text_color(theme.content(0.45))
            .when(dimmed, |text| text.opacity(0.7))
            .child(text),
        Highlighted::Tokens(tokens) => {
            let mut text = String::new();
            let mut highlights = Vec::new();
            for token in tokens {
                let (part, color) = match token {
                    Token::Plain(part) => (part, None),
                    Token::Keyword(part) => (part, Some(colors.keyword)),
                    Token::TypeName(part) => (part, Some(colors.type_name)),
                };
                if let Some(color) = color {
                    highlights.push((
                        text.len()..text.len() + part.len(),
                        gpui::HighlightStyle {
                            color: Some(color),
                            ..Default::default()
                        },
                    ));
                }
                text.push_str(&part);
            }
            div()
                .text_color(theme.content(0.80))
                .when(dimmed, |text| text.opacity(0.7))
                .child(StyledText::new(text).with_highlights(highlights))
        }
    };
    div()
        .relative()
        .flex()
        .items_baseline()
        .when_some(bg, |row, bg| row.bg(bg))
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(2.))
                .bg(bar),
        )
        .child(
            div()
                .w(u(28.))
                .flex_none()
                .pr(u(4.))
                .flex()
                .justify_end()
                .font_family(theme.fonts.mono.clone())
                .text_px(theme.text.micro)
                .text_color(theme.content(0.35))
                .child(
                    line.number
                        .map_or(" ".to_string(), |number| number.to_string()),
                ),
        )
        .child(
            div()
                .w(u(12.))
                .flex_none()
                .flex()
                .justify_center()
                .font_family(theme.fonts.mono.clone())
                .text_px(theme.text.micro)
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(mark_color)
                .child(mark),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .pr(u(8.))
                .font_family(theme.fonts.mono.clone())
                .text_px(theme.text.caption)
                .line_height(u(18.))
                .map(|text| {
                    if scrollable {
                        text.whitespace_nowrap()
                    } else {
                        text.truncate()
                    }
                })
                .child(body),
        )
}

impl RenderOnce for FilePreview {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let preview = &self.preview;
        let path = preview.path.clone();
        let cwd = self.cwd.as_deref();
        let file_path = path
            .as_deref()
            .map(|path| resolve_workspace_path(path, cwd).unwrap_or_else(|| path.to_string()));
        let file_name = preview
            .file_name
            .clone()
            .filter(|name| !name.is_empty())
            .or_else(|| file_name_of(path.as_deref()));
        let lines = Self::shown_lines(preview);
        let show_diff = preview.content_only == Some(true)
            || lines
                .iter()
                .any(|line| line.kind != ToolPreviewLineKind::Context);
        let added = preview.additions.unwrap_or(0);
        let deleted = preview.deletions.unwrap_or(0);
        let label = match &path {
            Some(path) => display_path(path, cwd),
            None => file_name
                .clone()
                .filter(|name| !name.is_empty())
                .or_else(|| preview.title.clone())
                .unwrap_or_else(|| "File".into()),
        };
        let link_color = theme.colors.link;
        let label_element = match (file_path, self.on_open_file.clone()) {
            (Some(file_path), Some(open)) => div()
                .id((self.id.clone(), 0usize))
                .min_w_0()
                .flex_1()
                .truncate()
                .cursor_pointer()
                .hover(move |style| style.text_color(link_color).underline())
                .on_click(move |_, window, cx| open(&file_path, window, cx))
                .child(label)
                .into_any_element(),
            _ => div()
                .min_w_0()
                .flex_1()
                .truncate()
                .child(label)
                .into_any_element(),
        };
        let stats = if added > 0 || deleted > 0 {
            div()
                .flex_none()
                .flex()
                .gap(u(4.))
                .text_px(theme.text.caption)
                .semibold()
                .tabular()
                .when(added > 0, |stats| {
                    stats.child(
                        div()
                            .text_color(theme.colors.diff_add_fg)
                            .child(format!("+{}", format_integer(added))),
                    )
                })
                .when(deleted > 0, |stats| {
                    stats.child(
                        div()
                            .text_color(theme.colors.diff_del_fg)
                            .child(format!("-{}", format_integer(deleted))),
                    )
                })
                .into_any_element()
        } else {
            match self.status {
                PreviewStatus::Rejected => icon(IconName::X)
                    .size(u(14.))
                    .text_color(theme.colors.danger)
                    .into_any_element(),
                PreviewStatus::Pending => icon(IconName::CircleDashed)
                    .size(u(14.))
                    .text_color(theme.content(0.40))
                    .into_any_element(),
                PreviewStatus::Accepted => div().into_any_element(),
            }
        };
        let header = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .px(u(10.))
            .py(u(8.))
            .child(file_type_icon(file_name.unwrap_or_else(|| "file".into())))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .font_family(theme.fonts.mono.clone())
                    .text_px(theme.text.label)
                    .medium()
                    .text_color(theme.content(0.85))
                    .child(label_element),
            )
            .child(stats);
        let popover = self.variant == PreviewVariant::Popover;
        let diff = show_diff.then(|| {
            let mut body = div()
                .id((self.id.clone(), 1usize))
                .flex()
                .flex_col()
                .when(popover, |body| body.max_h(u(180.)).overflow_scroll());
            if preview.content_only == Some(true) && lines.is_empty() {
                body = body.child(
                    div()
                        .px(u(12.))
                        .py(u(8.))
                        .font_family(theme.fonts.mono.clone())
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.50))
                        .child("Empty file"),
                );
            }
            for line in &lines {
                body = body.child(render_line(line, popover, &theme));
            }
            div()
                .flex()
                .flex_col()
                .child(div().h(px(1.)).bg(theme.content(0.10)))
                .child(body)
        });
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .when(!popover, |card| {
                card.overflow_hidden()
                    .rounded(u(10.))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.06))
            })
            .child(header)
            .children(diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_comments_and_tints_keywords_and_type_names() {
        assert_eq!(
            highlight("  // note"),
            Highlighted::Comment("  // note".into())
        );
        assert_eq!(
            highlight("# heading"),
            Highlighted::Comment("# heading".into())
        );
        assert_eq!(
            highlight("const App = x;"),
            Highlighted::Tokens(vec![
                Token::Keyword("const".into()),
                Token::Plain(" ".into()),
                Token::TypeName("App".into()),
                Token::Plain(" = ".into()),
                Token::Plain("x".into()),
                Token::Plain(";".into()),
            ])
        );
    }

    #[test]
    fn names_the_file_from_the_path() {
        assert_eq!(
            file_name_of(Some("src\\lib/main.rs")),
            Some("main.rs".into())
        );
        assert_eq!(file_name_of(Some("/")), None);
        assert_eq!(file_name_of(None), None);
    }
}
