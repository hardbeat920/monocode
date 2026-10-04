//! Port of src/features/sessions/ui/MarkdownModeToggle.tsx: the Preview and
//! Source tabs over a Markdown file, the per-key memory of which one was
//! picked (`useMarkdownMode`), and the shell that stacks the two views with
//! the tabs in the top right corner.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ElementId, Global, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{Theme, u};

/// `MarkdownViewMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MarkdownViewMode {
    #[default]
    Preview,
    Source,
}

/// `remembered`: the mode last picked per file path or note id, for this run.
#[derive(Default)]
pub struct MarkdownModes(HashMap<String, MarkdownViewMode>);

impl Global for MarkdownModes {}

/// `useMarkdownMode(key)[0]`: the remembered mode, Preview by default.
pub fn markdown_mode(key: &str, cx: &App) -> MarkdownViewMode {
    cx.try_global::<MarkdownModes>()
        .and_then(|modes| modes.0.get(key).copied())
        .unwrap_or_default()
}

/// `useMarkdownMode(key)[1]`: remember `mode` for `key`.
pub fn set_markdown_mode(key: &str, mode: MarkdownViewMode, cx: &mut App) {
    cx.default_global::<MarkdownModes>()
        .0
        .insert(key.to_string(), mode);
}

type ModeHandler = Rc<dyn Fn(MarkdownViewMode, &mut Window, &mut App)>;

/// `<MarkdownModeToggle mode onChange />`: a two-tab segmented control.
#[derive(IntoElement)]
pub struct MarkdownModeToggle {
    id: ElementId,
    mode: MarkdownViewMode,
    on_change: Option<ModeHandler>,
}

pub fn markdown_mode_toggle(
    id: impl Into<ElementId>,
    mode: MarkdownViewMode,
) -> MarkdownModeToggle {
    MarkdownModeToggle {
        id: id.into(),
        mode,
        on_change: None,
    }
}

impl MarkdownModeToggle {
    pub fn on_change(
        mut self,
        handler: impl Fn(MarkdownViewMode, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MarkdownModeToggle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let tab = |label: &'static str, mode: MarkdownViewMode| {
            let selected = self.mode == mode;
            let handler = self.on_change.clone();
            div()
                .id(ElementId::NamedChild(
                    std::sync::Arc::new(self.id.clone()),
                    label.into(),
                ))
                .rounded(u(4.))
                .px(u(8.))
                .py(u(2.))
                .font_family(theme.fonts.sans.clone())
                .text_px(11.)
                .line_height(u(16.))
                .cursor_pointer()
                .map(|el| {
                    if selected {
                        el.bg(theme.colors.selection_strong)
                            .text_color(theme.colors.content)
                    } else {
                        el.text_color(theme.content(0.45))
                            .hover(|s| s.text_color(theme.content(0.8)))
                    }
                })
                .when_some(handler, |el, handler| {
                    el.on_click(move |_, window, cx| handler(mode, window, cx))
                })
                .child(label)
        };
        div()
            .flex()
            .rounded(u(6.))
            .border_1()
            .border_color(theme.content(0.1))
            .bg(theme.content(0.1))
            .p(u(2.))
            .child(tab("Preview", MarkdownViewMode::Preview))
            .child(tab("Source", MarkdownViewMode::Source))
    }
}

/// `<MarkdownViewShell />`: the active view fills the shell and the toggle
/// floats in its top right corner, after any extra actions. The inactive
/// view is not drawn; its entity keeps its own state.
#[derive(IntoElement)]
pub struct MarkdownViewShell {
    toggle: MarkdownModeToggle,
    preview: Option<AnyElement>,
    source: Option<AnyElement>,
    actions: Vec<AnyElement>,
    /// `.markdown-view-actions` moves down under an open find panel.
    find_open: bool,
}

pub fn markdown_view_shell(
    id: impl Into<ElementId>,
    mode: MarkdownViewMode,
    preview: impl IntoElement,
    source: impl IntoElement,
) -> MarkdownViewShell {
    MarkdownViewShell {
        toggle: markdown_mode_toggle(id, mode),
        preview: Some(preview.into_any_element()),
        source: Some(source.into_any_element()),
        actions: Vec::new(),
        find_open: false,
    }
}

impl MarkdownViewShell {
    pub fn on_mode_change(
        mut self,
        handler: impl Fn(MarkdownViewMode, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.toggle = self.toggle.on_change(handler);
        self
    }

    /// Buttons shown left of the toggle.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// A find panel is open over the active view (35px tall).
    pub fn find_open(mut self, open: bool) -> Self {
        self.find_open = open;
        self
    }
}

impl RenderOnce for MarkdownViewShell {
    fn render(mut self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let active = match self.toggle.mode {
            MarkdownViewMode::Preview => self.preview.take(),
            MarkdownViewMode::Source => self.source.take(),
        };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .size_full()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .children(active),
            )
            .child(
                div()
                    .absolute()
                    .right(u(8.))
                    .top(u(if self.find_open { 35. + 8. } else { 8. }))
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .children(self.actions)
                    .child(self.toggle),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn remembers_the_mode_per_key(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            assert_eq!(markdown_mode("a.md", cx), MarkdownViewMode::Preview);
            set_markdown_mode("a.md", MarkdownViewMode::Source, cx);
            assert_eq!(markdown_mode("a.md", cx), MarkdownViewMode::Source);
            assert_eq!(markdown_mode("b.md", cx), MarkdownViewMode::Preview);
        });
    }
}
