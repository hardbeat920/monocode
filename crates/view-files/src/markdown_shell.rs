//! The Preview and Source switch around a Markdown document: a port of
//! `useMarkdownMode`, `MarkdownModeToggle`, and `MarkdownViewShell` from
//! src/features/sessions/ui/MarkdownModeToggle.tsx, and of
//! `splitMarkdownFrontmatter` from src/shared/lib/markdownFrontmatter.ts.
//! The file editor and plan surfaces need them; the sessions views can
//! move to this copy or keep their own.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{
    AnyElement, App, Global, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::{Theme, UiStyled as _, u};
use regex::Regex;

/// `MarkdownViewMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownViewMode {
    #[default]
    Preview,
    Source,
}

/// `remembered`: the mode each document was last shown in.
#[derive(Default)]
struct RememberedModes(HashMap<String, MarkdownViewMode>);

impl Global for RememberedModes {}

/// `useMarkdownMode`'s initial value: the remembered mode, else Preview.
pub fn remembered_mode(key: &str, cx: &App) -> MarkdownViewMode {
    remembered_mode_or(key, MarkdownViewMode::Preview, cx)
}

/// `useMarkdownMode(key, fallback)`: the remembered mode, else `fallback`.
pub fn remembered_mode_or(key: &str, fallback: MarkdownViewMode, cx: &App) -> MarkdownViewMode {
    cx.try_global::<RememberedModes>()
        .and_then(|modes| modes.0.get(key).copied())
        .unwrap_or(fallback)
}

/// `useMarkdownMode`'s setter.
pub fn remember_mode(key: &str, mode: MarkdownViewMode, cx: &mut App) {
    cx.default_global::<RememberedModes>()
        .0
        .insert(key.to_string(), mode);
}

type ModeHandler = Rc<dyn Fn(MarkdownViewMode, &mut Window, &mut App)>;

/// `MarkdownModeToggle`.
#[derive(IntoElement)]
pub struct MarkdownModeToggle {
    mode: MarkdownViewMode,
    on_change: ModeHandler,
}

pub fn markdown_mode_toggle(
    mode: MarkdownViewMode,
    on_change: impl Fn(MarkdownViewMode, &mut Window, &mut App) + 'static,
) -> MarkdownModeToggle {
    MarkdownModeToggle {
        mode,
        on_change: Rc::new(on_change),
    }
}

impl RenderOnce for MarkdownModeToggle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let tab = |label: &'static str, mode: MarkdownViewMode| {
            let selected = self.mode == mode;
            let on_change = self.on_change.clone();
            let hover = theme.content(0.80);
            div()
                .id(label)
                .debug_selector(move || format!("markdown-mode-{label}"))
                .rounded(u(theme.radius.sm))
                .px(u(8.))
                .py(u(2.))
                .font_family(theme.fonts.sans.clone())
                .text_px(theme.text.caption)
                .map(|tab| {
                    if selected {
                        tab.bg(theme.colors.selection_strong)
                            .text_color(theme.colors.content)
                    } else {
                        tab.text_color(theme.content(0.45))
                            .hover(move |style| style.text_color(hover))
                    }
                })
                .on_click(move |_, window, cx| on_change(mode, window, cx))
                .child(label)
        };
        div()
            .flex()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.10))
            .p(u(2.))
            .child(tab("Preview", MarkdownViewMode::Preview))
            .child(tab("Source", MarkdownViewMode::Source))
    }
}

/// `MarkdownViewShell`: the active side, with the toggle and any actions
/// floating at the top right. `find_open` moves them below a find bar.
#[derive(IntoElement)]
pub struct MarkdownViewShell {
    mode: MarkdownViewMode,
    on_change: ModeHandler,
    preview: AnyElement,
    source: AnyElement,
    actions: Option<AnyElement>,
    find_open: bool,
}

pub fn markdown_view_shell(
    mode: MarkdownViewMode,
    on_change: impl Fn(MarkdownViewMode, &mut Window, &mut App) + 'static,
    preview: impl IntoElement,
    source: impl IntoElement,
) -> MarkdownViewShell {
    MarkdownViewShell {
        mode,
        on_change: Rc::new(on_change),
        preview: preview.into_any_element(),
        source: source.into_any_element(),
        actions: None,
        find_open: false,
    }
}

impl MarkdownViewShell {
    pub fn actions(mut self, actions: impl IntoElement) -> Self {
        self.actions = Some(actions.into_any_element());
        self
    }

    /// A find bar is open in the active side (35 px tall).
    pub fn find_open(mut self, open: bool) -> Self {
        self.find_open = open;
        self
    }
}

impl RenderOnce for MarkdownViewShell {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let active = match self.mode {
            MarkdownViewMode::Preview => self.preview,
            MarkdownViewMode::Source => self.source,
        };
        let on_change = self.on_change.clone();
        let top = if self.find_open { 35. + 8. } else { 8. };
        div()
            .relative()
            .min_h_0()
            .min_w_0()
            .flex_1()
            .child(div().absolute().top_0().left_0().size_full().child(active))
            .child(
                div().absolute().right(u(8.)).top(u(top)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(6.))
                        .children(self.actions)
                        .child(markdown_mode_toggle(self.mode, move |mode, window, cx| {
                            on_change(mode, window, cx)
                        })),
                ),
            )
    }
}

/// `MarkdownDocumentParts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDocumentParts {
    pub metadata: Option<String>,
    pub body: String,
}

/// `splitMarkdownFrontmatter`: split a leading YAML block without reading it.
pub fn split_markdown_frontmatter(text: &str) -> MarkdownDocumentParts {
    static OPENING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\u{FEFF}?---[ \t]*\r?\n").expect("opening fence"));
    static CLOSING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)").expect("closing fence")
    });
    let whole = MarkdownDocumentParts {
        metadata: None,
        body: text.to_string(),
    };
    let Some(opening) = OPENING.find(text) else {
        return whole;
    };
    let remaining = &text[opening.end()..];
    // An unfinished header stays visible as ordinary text.
    let Some(closing) = CLOSING.find(remaining) else {
        return whole;
    };
    let metadata = &remaining[..closing.start()];
    let metadata = metadata
        .strip_suffix("\r\n")
        .or_else(|| metadata.strip_suffix('\n'))
        .unwrap_or(metadata);
    MarkdownDocumentParts {
        metadata: Some(metadata.to_string()),
        body: remaining[closing.end()..].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_leading_frontmatter_block() {
        assert_eq!(
            split_markdown_frontmatter("---\ntitle: A\n---\n# Body\n"),
            MarkdownDocumentParts {
                metadata: Some("title: A".into()),
                body: "# Body\n".into(),
            }
        );
        assert_eq!(
            split_markdown_frontmatter("---\ntitle: A\n...\nrest"),
            MarkdownDocumentParts {
                metadata: Some("title: A".into()),
                body: "rest".into(),
            }
        );
        // No closing fence: the text stays as it is.
        assert_eq!(split_markdown_frontmatter("---\nopen").metadata, None);
        assert_eq!(split_markdown_frontmatter("# Plain").body, "# Plain");
    }
}
