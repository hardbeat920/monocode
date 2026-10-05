//! Port of src/features/sessions/ui/MarkdownModeToggle.tsx: the remembered
//! preview or source choice per document, and the small two-tab toggle.

use std::collections::HashMap;

use gpui::{
    App, ElementId, Global, InteractiveElement as _, ParentElement as _, SharedString, Styled as _,
    div, prelude::FluentBuilder as _,
};
use monocode_ui::{Theme, UiStyled as _, u};

/// `MarkdownViewMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MarkdownMode {
    #[default]
    Preview,
    Source,
}

/// `remembered`: the mode each key last used, for this run of the app.
#[derive(Default)]
pub struct MarkdownModes(HashMap<String, MarkdownMode>);

impl Global for MarkdownModes {}

impl MarkdownModes {
    /// `useMarkdownMode(key)`: the remembered mode, preview by default.
    pub fn get(key: &str, cx: &App) -> MarkdownMode {
        cx.try_global::<MarkdownModes>()
            .and_then(|modes| modes.0.get(key).copied())
            .unwrap_or_default()
    }

    /// The setter `useMarkdownMode` returns.
    pub fn set(key: &str, mode: MarkdownMode, cx: &mut App) {
        cx.default_global::<MarkdownModes>()
            .0
            .insert(key.to_string(), mode);
    }
}

/// `MarkdownModeToggle`: `rounded-md border border-content/10 bg-content/10
/// p-0.5`, the picked tab on `bg-selection-strong`. Returns the two tabs'
/// ids as `<prefix>-preview` and `<prefix>-source` for click handlers.
pub fn markdown_mode_toggle(
    prefix: &str,
    mode: MarkdownMode,
    theme: &Theme,
) -> (gpui::Div, [gpui::Stateful<gpui::Div>; 2]) {
    let tab = |label: &'static str, selected: bool| {
        let hover = theme.content(0.80);
        div()
            .id(ElementId::Name(SharedString::from(format!(
                "{prefix}-{}",
                label.to_lowercase()
            ))))
            .debug_selector(move || format!("markdown-mode-{label}"))
            .px(u(8.))
            .py(u(2.))
            .rounded(u(theme.radius.sm))
            .text_px(theme.text.caption)
            .leading(theme.leading.normal)
            .map(|el| {
                if selected {
                    el.bg(theme.colors.selection_strong)
                        .text_color(theme.colors.content)
                } else {
                    el.text_color(theme.content(0.45))
                        .hover(move |s| s.text_color(hover))
                }
            })
            .child(label)
    };
    let frame = div()
        .flex()
        .p(u(2.))
        .rounded(u(theme.radius.md))
        .border_1()
        .border_color(theme.content(0.10))
        .bg(theme.content(0.10));
    (
        frame,
        [
            tab("Preview", mode == MarkdownMode::Preview),
            tab("Source", mode == MarkdownMode::Source),
        ],
    )
}
