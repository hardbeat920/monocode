//! Port of src/features/notes/ui/NoteMiniCard.tsx: the note a user turn
//! carried. Inside a prompt bubble it is `embedded`: the label reads "Note"
//! and the project line is left out. In the composer it shows the note's
//! slug and project, and a remove button.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div, img,
};
use monocode_core::notes::NoteCardMeta;
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, u};

/// The project a note came from, resolved by the host
/// (`noteSourceProject`, the tab group logo, mascot, and color).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteProject {
    pub name: String,
    /// A custom project logo image.
    pub logo: Option<PathBuf>,
    /// The mascot color (`resolveTabGroupColor`), when there is no logo.
    pub color: Option<Hsla>,
}

/// `<NoteMiniCard card embedded onDismiss />`.
#[derive(IntoElement)]
pub struct NoteCard {
    id: ElementId,
    card: NoteCardMeta,
    embedded: bool,
    project: Option<NoteProject>,
    on_dismiss: Option<ClickHandler>,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub fn note_card(id: impl Into<ElementId>, card: NoteCardMeta) -> NoteCard {
    NoteCard {
        id: id.into(),
        card,
        embedded: false,
        project: None,
        on_dismiss: None,
    }
}

/// The note's title, or "Untitled".
pub fn note_title(card: &NoteCardMeta) -> String {
    if card.title.is_empty() {
        "Untitled".into()
    } else {
        card.title.clone()
    }
}

/// The small label over the title: "Note", with the slug outside a bubble.
pub fn note_label(card: &NoteCardMeta, embedded: bool) -> String {
    if !embedded && !card.slug.is_empty() {
        format!("Note \u{b7} {}", card.slug)
    } else {
        "Note".into()
    }
}

impl NoteCard {
    /// Inside a prompt bubble: no slug, no project, no outer padding.
    pub fn embedded(mut self, embedded: bool) -> Self {
        self.embedded = embedded;
        self
    }

    pub fn project(mut self, project: Option<NoteProject>) -> Self {
        self.project = project;
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for NoteCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let title = note_title(&self.card);
        let project = if self.embedded {
            None
        } else {
            self.project.clone()
        };
        let project_line = project.map(|project| {
            let mark: AnyElement = match &project.logo {
                Some(logo) => img(logo.clone())
                    .flex_none()
                    .size(u(14.))
                    .rounded(u(2.))
                    .into_any_element(),
                None => crate::transcript::view::mascot::mascot(
                    ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "mascot".into()),
                    &project.name,
                    project.color.unwrap_or(theme.content(0.45)),
                    false,
                )
                .into_any_element(),
            };
            div()
                .mt(u(4.))
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .text_px(11.)
                .text_color(theme.content(0.45))
                .child(mark)
                .child(div().min_w_0().truncate().child(project.name.clone()))
        });
        let dismiss = self.on_dismiss.clone().map(|dismiss| {
            div()
                .id(ElementId::NamedChild(
                    std::sync::Arc::new(self.id.clone()),
                    "dismiss".into(),
                ))
                .absolute()
                .right(u(6.))
                .top(u(6.))
                .flex()
                .items_center()
                .justify_center()
                .size(u(20.))
                .rounded(u(4.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.content(0.1)))
                .tooltip(tooltip("Remove"))
                .on_click(move |event, window, cx| dismiss(event, window, cx))
                .child(
                    icon(IconName::X)
                        .size(u(12.))
                        .text_color(theme.content(0.4)),
                )
        });
        let has_dismiss = dismiss.is_some();
        let inner = div()
            .id(self.id.clone())
            .relative()
            .rounded(u(6.))
            .border_1()
            .border_color(theme.content(0.1))
            .bg(theme.content(0.06))
            .px(u(10.))
            .py(u(8.))
            .when(has_dismiss, |el| el.pr(u(32.)))
            .font_family(theme.fonts.sans.clone())
            .child(
                div()
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
                                    .flex_none()
                                    .text_color(theme.content(0.45)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_px(11.)
                                    .line_height(u(16.))
                                    .text_color(theme.content(0.5))
                                    .child(note_label(&self.card, self.embedded)),
                            ),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .line_clamp(1)
                            .text_px(13.)
                            .leading(1.375)
                            .semibold()
                            .text_color(theme.colors.content)
                            .child(title),
                    )
                    .children(project_line),
            )
            .children(dismiss);
        if self.embedded {
            inner.into_any_element()
        } else {
            div().px(u(12.)).pt(u(8.)).child(inner).into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(title: &str, slug: &str) -> NoteCardMeta {
        NoteCardMeta {
            id: "n1".into(),
            slug: slug.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn titles_an_empty_note_untitled() {
        assert_eq!(note_title(&meta("", "")), "Untitled");
        assert_eq!(note_title(&meta("Release plan", "")), "Release plan");
    }

    #[test]
    fn shows_the_slug_only_outside_a_bubble() {
        let card = meta("Release plan", "release-plan");
        assert_eq!(note_label(&card, true), "Note");
        assert_eq!(note_label(&card, false), "Note \u{b7} release-plan");
        assert_eq!(note_label(&meta("x", ""), false), "Note");
    }
}
