//! Port of src/features/sessions/ui/MarkdownDocumentPreview.tsx and
//! src/shared/lib/markdownFrontmatter.ts: a Markdown file rendered as a
//! document, with a leading YAML header folded into a disclosure instead of
//! being read as Markdown (`MarkdownPreview` with a `header`).

use std::sync::LazyLock;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};
use monocode_markdown::MarkdownView;
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{IconName, Theme, icon, u};
use regex::Regex;

use crate::transcript::view::style::{MarkdownVariant, markdown_style};

/// `MarkdownDocumentParts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDocumentParts<'a> {
    pub metadata: Option<String>,
    pub body: &'a str,
}

static OPENING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\x{FEFF}?---[ \t]*\r?\n").expect("opening pattern"));
static CLOSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)").expect("closing pattern"));

/// `splitMarkdownFrontmatter`: split a leading YAML block without reading
/// it. An unfinished header stays visible as ordinary document text.
// TODO(port): view-composer's skill preview has the same function. Move one
// copy to monocode-core so the views share it.
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

/// A link clicked in the document, for the host to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownDocumentEvent {
    OpenLink { url: String },
}

/// `<MarkdownDocumentPreview text metadataLabel />`.
pub struct MarkdownDocumentPreview {
    text: String,
    metadata: Option<SharedString>,
    metadata_label: SharedString,
    metadata_open: bool,
    body: Entity<MarkdownView>,
    scroll: ScrollHandle,
    _theme: gpui::Subscription,
}

impl EventEmitter<MarkdownDocumentEvent> for MarkdownDocumentPreview {}

impl MarkdownDocumentPreview {
    pub fn new(
        text: &str,
        metadata_label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let parts = split_markdown_frontmatter(text);
        let style = markdown_style(Theme::of(cx), MarkdownVariant::Normal);
        let weak = cx.entity().downgrade();
        let body_text = parts.body.to_string();
        let body = cx.new(|cx| {
            let mut view = MarkdownView::with_text(body_text, cx);
            view.set_style(style, cx);
            // A document's lines stay on their own lines.
            view.set_hard_breaks(true, cx);
            view.on_link_click(move |link, _, cx| {
                let url = link.url.to_string();
                weak.update(cx, |_, cx| cx.emit(MarkdownDocumentEvent::OpenLink { url }))
                    .ok();
            });
            view
        });
        let theme = cx.observe_global::<Theme>(|this: &mut Self, cx| {
            let style = markdown_style(Theme::of(cx), MarkdownVariant::Normal);
            this.body.update(cx, |view, cx| view.set_style(style, cx));
            cx.notify();
        });
        Self {
            text: text.to_string(),
            metadata: parts.metadata.map(SharedString::from),
            metadata_label: metadata_label.into(),
            metadata_open: false,
            body,
            scroll: ScrollHandle::new(),
            _theme: theme,
        }
    }

    /// Show new document text. The disclosure keeps its open state.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.text == text {
            return;
        }
        let parts = split_markdown_frontmatter(text);
        self.metadata = parts.metadata.map(SharedString::from);
        let body = parts.body.to_string();
        self.body.update(cx, |view, cx| view.set_text(&body, cx));
        self.text = text.to_string();
        cx.notify();
    }

    pub fn has_metadata(&self) -> bool {
        self.metadata.is_some()
    }

    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    pub fn is_metadata_open(&self) -> bool {
        self.metadata_open
    }

    pub fn toggle_metadata(&mut self, cx: &mut Context<Self>) {
        self.metadata_open = !self.metadata_open;
        cx.notify();
    }

    /// The body's markdown view.
    pub fn body(&self) -> &Entity<MarkdownView> {
        &self.body
    }
}

impl Render for MarkdownDocumentPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let open = self.metadata_open;
        let header = self.metadata.clone().map(|metadata| {
            div()
                .mb(u(24.))
                .rounded(u(8.))
                .border_1()
                .border_color(theme.content(0.1))
                .bg(theme.content(0.03))
                .child(
                    div()
                        .id("markdown-metadata-summary")
                        .flex()
                        .items_center()
                        .gap(u(6.))
                        .rounded(u(8.))
                        .px(u(12.))
                        .py(u(8.))
                        .font_family(theme.fonts.sans.clone())
                        .text_px(12.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.6))
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.colors.content))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_metadata(cx)))
                        .child(
                            icon(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(u(14.))
                            .flex_none()
                            .text_color(theme.content(0.5)),
                        )
                        .child(self.metadata_label.clone()),
                )
                .when(open, |el| {
                    el.child(
                        div()
                            .border_t(px(1.))
                            .border_color(theme.colors.stroke)
                            .px(u(12.))
                            .py(u(8.))
                            .font_family(theme.fonts.mono.clone())
                            .text_px(12.)
                            .line_height(u(20.))
                            .text_color(theme.content(0.7))
                            .child(metadata),
                    )
                })
        });
        div()
            .id("markdown-preview")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .px(u(24.))
                    .py(u(32.))
                    .children(header)
                    .child(self.body.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_leading_frontmatter_from_the_markdown_body() {
        for text in [
            "---\r\ntitle: Windows\r\n---\r\n\r\n# Heading",
            "\u{FEFF}---\ntitle: BOM\n---\n\n# Heading",
            "---\ntitle: End marker\n...\n\n# Heading",
            "---\n---\n\n# Heading",
        ] {
            let parts = split_markdown_frontmatter(text);
            assert!(parts.metadata.is_some(), "{text:?}");
            assert_eq!(monocode_core::js::trim(parts.body), "# Heading", "{text:?}");
            assert!(!parts.body.contains("---"), "{text:?}");
        }
    }

    #[test]
    fn leaves_ordinary_or_unfinished_markdown_untouched() {
        for text in [
            "# No frontmatter",
            "---\ntitle: unfinished\n\n# Still ordinary Markdown",
            "Before\n\n---\ntitle: not at the start\n---",
        ] {
            let parts = split_markdown_frontmatter(text);
            assert_eq!(parts.metadata, None, "{text:?}");
            assert_eq!(parts.body, text);
        }
    }

    #[test]
    fn shows_frontmatter_verbatim_in_the_disclosure() {
        let parts = split_markdown_frontmatter(
            "---\ntitle: Example Note\ntags:\n  - alpha\n  - beta\n---\n\nBody.",
        );
        assert_eq!(
            parts.metadata.as_deref(),
            Some("title: Example Note\ntags:\n  - alpha\n  - beta")
        );
        assert_eq!(parts.body, "\nBody.");
    }

    #[test]
    fn an_empty_header_is_still_a_header() {
        let parts = split_markdown_frontmatter("---\n---\n\n# Heading");
        assert_eq!(parts.metadata.as_deref(), Some(""));
    }
}
