//! Port of src/features/skills/ui/SkillDocumentPreview.tsx and the parts of
//! src/features/sessions/ui/MarkdownDocumentPreview.tsx it uses: a SKILL.md
//! rendered as Markdown, with its YAML header folded into a "Skill metadata"
//! disclosure instead of being read as Markdown.

use std::sync::LazyLock;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_markdown::{MarkdownStyle, MarkdownView, SyntaxColors};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use regex::Regex;

/// `MarkdownDocumentParts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownDocumentParts<'a> {
    pub metadata: Option<String>,
    pub body: &'a str,
}

static OPENING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\x{FEFF}?---[ \t]*\r?\n").expect("opening pattern"));
static CLOSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)").expect("closing pattern"));

/// `splitMarkdownFrontmatter`: split a leading YAML header without reading
/// it. An unfinished header stays in the body.
// TODO(port): monocode-engine has the same function in runtime::util. Move
// one copy to monocode-core so views and the engine share it.
pub fn split_markdown_frontmatter(text: &str) -> MarkdownDocumentParts<'_> {
    let Some(opening) = OPENING.find(text) else {
        return MarkdownDocumentParts {
            metadata: None,
            body: text,
        };
    };
    let remaining = &text[opening.end()..];
    let Some(closing) = CLOSING.find(remaining) else {
        return MarkdownDocumentParts {
            metadata: None,
            body: text,
        };
    };
    let header = &remaining[..closing.start()];
    let header = header
        .strip_suffix("\r\n")
        .or_else(|| header.strip_suffix('\n'))
        .unwrap_or(header);
    MarkdownDocumentParts {
        metadata: Some(header.to_string()),
        body: &remaining[closing.end()..],
    }
}

/// The Markdown style for the current theme.
pub fn markdown_style(theme: &Theme) -> MarkdownStyle {
    let c = theme.colors;
    let syntax = if theme.is_dark() {
        SyntaxColors::github_dark()
    } else {
        SyntaxColors::github_light()
    };
    let mut style = MarkdownStyle::with_content(
        c.content,
        c.markdown_heading,
        monocode_ui::color::with_alpha(c.mention, 0.9),
        c.link,
        syntax,
    );
    style.font_family = theme.fonts.sans.clone();
    style.mono_font_family = theme.fonts.mono.clone();
    style
}

/// `SkillDocumentPreview`.
pub struct SkillDocumentPreview {
    metadata: Option<SharedString>,
    metadata_label: SharedString,
    open: bool,
    body: Entity<MarkdownView>,
}

impl SkillDocumentPreview {
    pub fn new(text: &str, cx: &mut Context<Self>) -> Self {
        let parts = split_markdown_frontmatter(text);
        let body_text = parts.body.to_string();
        let style = markdown_style(Theme::of(cx));
        let body = cx.new(|cx| {
            let mut view = MarkdownView::with_text(body_text, cx);
            view.set_style(style, cx);
            // A skill doc's lines stay on their own lines.
            view.set_hard_breaks(true, cx);
            view
        });
        Self {
            metadata: parts.metadata.map(SharedString::from),
            metadata_label: "Skill metadata".into(),
            open: false,
            body,
        }
    }

    /// Show another document.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let parts = split_markdown_frontmatter(text);
        self.metadata = parts.metadata.map(SharedString::from);
        let body = parts.body.to_string();
        self.body.update(cx, |view, cx| view.set_text(&body, cx));
        cx.notify();
    }

    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    pub fn body(&self) -> &Entity<MarkdownView> {
        &self.body
    }

    pub fn is_metadata_open(&self) -> bool {
        self.open
    }

    pub fn toggle_metadata(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.notify();
    }
}

impl Render for SkillDocumentPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let ink = theme.colors.content;
        let open = self.open;
        div()
            .flex()
            .flex_col()
            .font_family(theme.fonts.sans.clone())
            .when_some(self.metadata.clone(), |el, metadata| {
                el.child(
                    div()
                        .mb(u(24.))
                        .rounded(u(theme.radius.lg))
                        .border_1()
                        .border_color(theme.content(0.10))
                        .bg(theme.content(0.03))
                        .child(
                            div()
                                .id("skill-metadata-summary")
                                .debug_selector(|| "skill-metadata-summary".into())
                                .flex()
                                .items_center()
                                .gap(u(6.))
                                .px(u(12.))
                                .py(u(8.))
                                .rounded(u(theme.radius.lg))
                                .text_px(theme.text.label)
                                .text_color(theme.content(0.60))
                                .hover(move |s| s.text_color(ink))
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_metadata(cx)))
                                .child(
                                    icon(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(u(14.))
                                    .text_color(theme.content(0.50)),
                                )
                                .child(self.metadata_label.clone()),
                        )
                        .when(open, |el| {
                            el.child(
                                div()
                                    .border_t_1()
                                    .border_color(theme.colors.stroke)
                                    .px(u(12.))
                                    .py(u(8.))
                                    .font_family(theme.fonts.mono.clone())
                                    .text_px(theme.text.label)
                                    .line_height(u(20.))
                                    .text_color(theme.content(0.70))
                                    .child(metadata),
                            )
                        }),
                )
            })
            .child(self.body.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `skill document metadata` › recognizes frontmatter delimiters without
    /// losing the body.
    #[test]
    fn recognizes_frontmatter_delimiters_without_losing_the_body() {
        for text in [
            "---\r\nname: windows\r\n---\r\n\r\n# Instructions",
            "\u{FEFF}---\nname: windows\n---\n\n# Instructions",
            "---\nname: windows\n...\n\n# Instructions",
            "---\n---\n\n# Instructions",
        ] {
            let parts = split_markdown_frontmatter(text);
            assert!(parts.metadata.is_some(), "{text:?}");
            let document = monocode_markdown::parse(parts.body);
            let headings: Vec<_> = document
                .blocks
                .iter()
                .filter_map(|top| match &top.block {
                    monocode_markdown::Block::Heading { level, .. } => Some(*level),
                    _ => None,
                })
                .collect();
            assert_eq!(headings, vec![1], "{text:?}");
        }
    }

    #[test]
    fn leaves_unfinished_headers_in_the_body() {
        let open = "---\ntitle: x\n";
        assert_eq!(split_markdown_frontmatter(open).metadata, None);
        assert_eq!(split_markdown_frontmatter(open).body, open);
        let parts = split_markdown_frontmatter("---\nname: a\n---\nbody");
        assert_eq!(parts.metadata.as_deref(), Some("name: a"));
        assert_eq!(parts.body, "body");
    }
}
