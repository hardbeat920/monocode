//! Port of src/features/notes/ui/NoteMiniCard.tsx: the note chip in the
//! composer and on the user turn that carried it.

use std::rc::Rc;

use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_core::notes::NoteCardMeta;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::model::note_source_project;
use crate::data::ProjectMark;
use crate::widgets::project_mark;

type DismissFn = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct NoteMiniCard {
    card: NoteCardMeta,
    mark: Option<ProjectMark>,
    embedded: bool,
    on_dismiss: Option<DismissFn>,
}

/// `mark` is the appearance of `card.source_cwd`, when it is a project.
pub fn note_mini_card(card: NoteCardMeta, mark: Option<ProjectMark>) -> NoteMiniCard {
    NoteMiniCard {
        card,
        mark,
        embedded: false,
        on_dismiss: None,
    }
}

impl NoteMiniCard {
    /// `embedded`: no slug, no project line, and no outer padding.
    pub fn embedded(mut self, embedded: bool) -> Self {
        self.embedded = embedded;
        self
    }

    /// Shows the remove button.
    pub fn on_dismiss(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for NoteMiniCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let card = self.card;
        let title = if card.title.is_empty() {
            "Untitled".to_string()
        } else {
            card.title.clone()
        };
        let project = note_source_project(card.source_cwd.as_deref());
        let caption = if !self.embedded && !card.slug.is_empty() {
            format!("Note · {}", card.slug)
        } else {
            "Note".into()
        };
        let mut body = div()
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .child(
                        icon(IconName::File)
                            .size(u(14.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.50))
                            .child(caption),
                    ),
            )
            .child(
                div()
                    .mt(u(4.))
                    .truncate()
                    .text_px(theme.text.body)
                    .semibold()
                    .leading(theme.leading.snug)
                    .text_color(theme.colors.content)
                    .child(title.clone()),
            );
        if !self.embedded
            && let Some(project) = project
        {
            let mark = self.mark.clone().unwrap_or_else(|| ProjectMark {
                label: project.clone(),
                ..ProjectMark::plain(card.source_cwd.as_deref().unwrap_or(""))
            });
            body = body.child(
                div()
                    .mt(u(4.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .child(project_mark(&mark, 14., 12., theme.content(0.45)))
                    .child(div().min_w_0().truncate().child(project)),
            );
        }
        let mut inner = div()
            .relative()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.06))
            .px(u(10.))
            .py(u(8.))
            .child(body);
        if let Some(dismiss) = self.on_dismiss {
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            inner = inner.pr(u(32.)).child(
                div()
                    .id("note-mini-card-remove")
                    .debug_selector(move || format!("remove note {title}"))
                    .absolute()
                    .right(u(6.))
                    .top(u(6.))
                    .flex()
                    .size(u(20.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.sm))
                    .group("note-mini-card-remove")
                    .hover(move |s| s.bg(hover))
                    .tooltip(monocode_ui::widgets::tooltip("Remove"))
                    .on_click(move |_, window, cx| dismiss(window, cx))
                    .child(
                        icon(IconName::X)
                            .size(u(12.))
                            .text_color(theme.content(0.40))
                            .group_hover("note-mini-card-remove", move |s| s.text_color(ink)),
                    ),
            );
        }
        if self.embedded {
            return inner;
        }
        div().px(u(12.)).pt(u(8.)).child(inner)
    }
}
