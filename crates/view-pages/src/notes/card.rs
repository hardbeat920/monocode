//! NotesView.tsx `NoteCard`: one row of the notes list.

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_layout::paths::project_name;
use monocode_ui::{Theme, UiStyled as _, u};

use super::data::Note;
use super::model::{note_preview, note_source_project};
use crate::data::ProjectMark;
use crate::format::format_relative_time;
use crate::widgets::project_mark;

/// The card. Wrap it with a click handler.
#[derive(IntoElement)]
pub struct NoteCard {
    note: Note,
    active: bool,
    mark: Option<ProjectMark>,
    now: i64,
}

/// `mark` is the appearance of the note's project, when it has one.
pub fn note_card(note: Note, active: bool, mark: Option<ProjectMark>, now: i64) -> NoteCard {
    NoteCard {
        note,
        active,
        mark,
        now,
    }
}

/// `NoteProjectMark`: the mark and the folder name.
pub fn note_project_mark(cwd: &str, mark: &ProjectMark, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(u(6.))
        .child(project_mark(mark, 14., 12., theme.content(0.50)))
        .child(div().min_w_0().truncate().child(project_name(cwd)))
}

impl RenderOnce for NoteCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let note = self.note;
        let preview = note_preview(&note.body, &note.title);
        let project = note_source_project(note.source_cwd.as_deref());
        let time = format_relative_time(note.updated_at, self.now);
        let hint: SharedString = [Some(note.title.clone()), project.clone()]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
            .into();
        let ink = theme.colors.content;
        let hover = theme.content(0.05);
        let title = note.title.clone();
        let mut top = div().flex().items_center().gap(u(8.));
        top = match (&project, &note.source_cwd, &self.mark) {
            (Some(_), Some(cwd), Some(mark)) => top.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.50))
                    .child(note_project_mark(cwd, mark, theme)),
            ),
            _ => top.child(div().flex_1().min_w_0()),
        };
        if !time.is_empty() {
            top = top.child(
                div()
                    .flex_none()
                    .text_px(theme.text.caption)
                    .tabular()
                    .text_color(theme.content(0.45))
                    .child(time),
            );
        }
        let mut card = div()
            .id(ElementId::Name(SharedString::from(format!(
                "note-card-{}",
                note.id
            ))))
            .debug_selector(move || format!("note-card {title}"))
            .flex()
            .flex_col()
            .w_full()
            .px(u(10.))
            .py(u(8.))
            .rounded(u(theme.radius.md))
            .tooltip(monocode_ui::widgets::tooltip(hint))
            .map(|card| {
                if self.active {
                    card.bg(theme.colors.selection).text_color(ink)
                } else {
                    card.text_color(theme.content(0.80))
                        .hover(move |s| s.bg(hover).text_color(ink))
                }
            })
            .child(top)
            .child(
                div()
                    .mt(u(4.))
                    .truncate()
                    .text_px(theme.text.body)
                    .semibold()
                    .leading(theme.leading.snug)
                    .text_color(ink)
                    .child(note.title.clone()),
            );
        if !preview.is_empty() {
            card = card.child(
                div()
                    .mt(u(4.))
                    .truncate()
                    .text_px(theme.text.label)
                    .leading(theme.leading.snug)
                    .text_color(theme.content(0.45))
                    .child(preview),
            );
        }
        if !note.tags.is_empty() {
            let extra = note.tags.len().saturating_sub(3);
            card = card.child(
                div()
                    .mt(u(6.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(4.))
                    .overflow_hidden()
                    .children(note.tags.iter().take(3).map(|tag| {
                        div()
                            .max_w(u(96.))
                            .truncate()
                            .rounded(u(theme.radius.sm))
                            .bg(theme.content(0.08))
                            .px(u(6.))
                            .py(u(2.))
                            .text_px(theme.text.micro)
                            .leading(theme.leading.none)
                            .text_color(theme.content(0.55))
                            .child(format!("#{tag}"))
                    }))
                    .when(extra > 0, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_px(theme.text.micro)
                                .text_color(theme.content(0.40))
                                .child(format!("+{extra}")),
                        )
                    }),
            );
        }
        card
    }
}
