//! Port of `ArchivePage`, `formatDate`, and `archivedProjectLabel` in
//! SettingsView.tsx, and of src/features/projects/ui/RemoveProjectDialog.tsx.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, deferred, div, relative,
};
use monocode_core::session::session_display_title;
use monocode_layout::paths::{pretty_cwd, project_name};
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{Theme, UiStyled as _, provider_logo, u};

use super::chrome::{card_note, group, row};
use super::controls::{secondary_button, toggle};
use super::host::{ArchivedProject, SessionSummary, SettingsCallbacks};
use super::providers::{harness_logo, looks_like_project};
use super::section::SectionContext;
use super::store;

/// The archived item's month and day in the system locale.
pub fn format_date(value: i64) -> String {
    if value <= 0 {
        return String::new();
    }
    monocode_platform::date_time::format_local(
        value,
        monocode_platform::date_time::DateTimeStyle::MonthDay,
    )
}

type DialogHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// `RemoveProjectDialog`: confirm deleting an archived project and its chats.
pub struct RemoveProjectDialog {
    pub name: SharedString,
    pub path: SharedString,
    pub sessions: Option<i64>,
}

impl RemoveProjectDialog {
    fn render(
        &self,
        on_cancel: DialogHandler,
        on_confirm: DialogHandler,
        focus: &FocusHandle,
        cx: &App,
    ) -> AnyElement {
        let theme = Theme::of(cx);
        let mut text = div()
            .flex()
            .flex_col()
            .gap(u(4.))
            .child(
                div()
                    .text_px(theme.text.body)
                    .medium()
                    .leading(theme.leading.tight)
                    .text_color(theme.colors.content)
                    .child(format!("Delete “{}”?", self.name)),
            )
            .child(
                div()
                    .text_px(theme.text.label)
                    .leading(theme.leading.snug)
                    .text_color(theme.content(0.55))
                    .child("All conversations for this project will be deleted. It also leaves the sidebar. The folder on disk stays put, and opening it again brings the project back empty."),
            );
        if let Some(count) = self.sessions.filter(|count| *count > 0) {
            text = text.child(
                div()
                    .text_px(theme.text.label)
                    .leading(theme.leading.snug)
                    .text_color(theme.content(0.45))
                    .child(if count == 1 {
                        "1 saved conversation will be removed.".to_string()
                    } else {
                        format!("{count} saved conversations will be removed.")
                    }),
            );
        }
        text = text.child(
            div()
                .truncate()
                .text_px(theme.text.caption)
                .leading(theme.leading.tight)
                .text_color(theme.content(0.40))
                .child(pretty_cwd(&self.path)),
        );
        let cancel = on_cancel.clone();
        let backdrop_cancel = on_cancel.clone();
        let hover = theme.content(0.08);
        let ink = theme.colors.content;
        let danger_fill = gpui::Hsla {
            a: 0.2,
            ..theme.colors.danger_fill
        };
        let danger_hover = gpui::Hsla {
            a: 0.3,
            ..theme.colors.danger_fill
        };
        let panel = div()
            .id("remove-project-dialog")
            .relative()
            .flex()
            .flex_col()
            .gap(u(12.))
            .w(u(420.))
            .max_w_full()
            .p(u(16.))
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .shadow_xl()
            .overflow_hidden()
            .track_focus(focus)
            .debug_selector(|| "remove-project-dialog".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)))
            .child(text)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(u(8.))
                    .child(
                        div()
                            .id("remove-project-cancel")
                            .px(u(12.))
                            .py(u(6.))
                            .rounded(u(theme.radius.md))
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.70))
                            .hover(move |s| s.bg(hover).text_color(ink))
                            .debug_selector(|| "button:remove-project-cancel".into())
                            .on_click(move |_, window, cx| cancel(window, cx))
                            .child("Cancel"),
                    )
                    .child(
                        div()
                            .id("remove-project-confirm")
                            .px(u(12.))
                            .py(u(6.))
                            .rounded(u(theme.radius.md))
                            .bg(danger_fill)
                            .text_px(theme.text.label)
                            .medium()
                            .text_color(theme.colors.danger_soft)
                            .hover(move |s| s.bg(danger_hover))
                            .debug_selector(|| "button:remove-project-confirm".into())
                            .on_click(move |_, window, cx| on_confirm(window, cx))
                            .child("Delete"),
                    ),
            );
        deferred(
            div()
                .id("remove-project-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(gpui::Hsla {
                            a: 0.3,
                            ..gpui::black()
                        })
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            backdrop_cancel(window, cx)
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .px(u(12.))
                        .child(div().flex_none().h(relative(0.22)))
                        .child(panel),
                ),
        )
        .with_priority(theme.layer.dialog)
        .into_any_element()
    }
}

pub struct ArchiveSection {
    ctx: SectionContext,
    cwd: String,
    sessions: Vec<SessionSummary>,
    callbacks: SettingsCallbacks,
    show_archived: bool,
    deleting: Option<RemoveProjectDialog>,
    dialog_focus: FocusHandle,
    count_job: Option<Task<()>>,
}

impl ArchiveSection {
    pub fn new(
        ctx: SectionContext,
        cwd: String,
        sessions: Vec<SessionSummary>,
        callbacks: SettingsCallbacks,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            show_archived: store::load_session_sidebar_filters(&ctx.kv).show_archived,
            ctx,
            cwd,
            sessions,
            callbacks,
            deleting: None,
            dialog_focus: cx.focus_handle(),
            count_job: None,
        }
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionSummary>, cx: &mut Context<Self>) {
        self.sessions = sessions;
        cx.notify();
    }

    /// The archived conversations, newest first.
    pub fn archived(&self) -> Vec<SessionSummary> {
        let mut archived: Vec<SessionSummary> = self
            .sessions
            .iter()
            .filter(|session| session.archived)
            .cloned()
            .collect();
        archived.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        archived
    }

    pub fn deleting(&self) -> Option<&RemoveProjectDialog> {
        self.deleting.as_ref()
    }

    pub fn on_show_archived(&mut self, show_archived: bool, cx: &mut Context<Self>) {
        let mut filters = store::load_session_sidebar_filters(&self.ctx.kv);
        filters.show_archived = show_archived;
        store::save_session_sidebar_filters(&self.ctx.kv, &filters);
        self.show_archived = show_archived;
        cx.notify();
    }

    pub fn start_delete(
        &mut self,
        project: &ArchivedProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.deleting = Some(RemoveProjectDialog {
            name: project.label.clone().into(),
            path: project.path.clone().into(),
            sessions: None,
        });
        let count = self
            .ctx
            .hosts
            .archive
            .project_session_count(&project.path, cx);
        self.count_job = Some(cx.spawn(async move |this, cx| {
            let sessions = count.await;
            this.update(cx, |this, cx| {
                if let Some(dialog) = &mut this.deleting {
                    dialog.sessions = sessions;
                    cx.notify();
                }
            })
            .ok();
        }));
        self.dialog_focus.focus(window, cx);
        cx.notify();
    }

    pub fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.deleting = None;
        self.count_job = None;
        cx.notify();
    }

    pub fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.deleting.take()
            && let Some(delete) = self.callbacks.on_delete_project.clone()
        {
            delete(dialog.path.to_string(), window, cx);
        }
        cx.notify();
    }

    fn project_row(&self, project: &ArchivedProject, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mut el = div()
            .flex()
            .items_center()
            .gap(u(12.))
            .px(u(16.))
            .py(u(10.))
            .border_b_1()
            .border_color(theme.content(0.05))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_px(theme.text.body)
                            .child(project.label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.40))
                            .child(pretty_cwd(&project.path)),
                    ),
            );
        if let Some(restore) = self.callbacks.on_restore_project.clone() {
            let path = project.path.clone();
            el = el.child(
                secondary_button(format!("restore-{}", project.path), "Restore")
                    .on_click(move |_, window, cx| restore(path.clone(), window, cx)),
            );
        }
        if self.callbacks.on_delete_project.is_some() {
            let project = project.clone();
            el = el.child(
                secondary_button(format!("delete-project-{}", project.path), "Delete")
                    .danger(true)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_delete(&project, window, cx)
                    })),
            );
        }
        el.into_any_element()
    }

    fn session_row(&self, session: &SessionSummary, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let id = session.id.clone();
        let open = self.callbacks.on_open_session.clone();
        let archive = self.callbacks.on_archive_session.clone();
        let delete = self.callbacks.on_delete_session.clone();
        let hover = theme.colors.content;
        let (open_id, archive_id, delete_id) = (id.clone(), id.clone(), id.clone());
        div()
            .flex()
            .items_center()
            .gap(u(12.))
            .px(u(16.))
            .py(u(10.))
            .border_b_1()
            .border_color(theme.content(0.05))
            .child(provider_logo(harness_logo(session.harness)).size(14.))
            .child(
                div()
                    .id(gpui::ElementId::from(SharedString::from(format!(
                        "open-{id}"
                    ))))
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_px(theme.text.body)
                    .hover(move |s| s.text_color(hover))
                    .debug_selector(move || format!("archived-session:{open_id}"))
                    .on_click(move |_, window, cx| {
                        if let Some(open) = open.clone() {
                            open(id.clone(), window, cx);
                        }
                    })
                    .child(session_display_title(&session.title, session.harness)),
            )
            .child(
                div()
                    .flex_none()
                    .text_px(theme.text.caption)
                    .tabular()
                    .text_color(theme.content(0.35))
                    .child(format_date(session.updated_at)),
            )
            .child(
                secondary_button(format!("unarchive-{archive_id}"), "Unarchive").on_click(
                    move |_, window, cx| {
                        if let Some(archive) = archive.clone() {
                            archive((archive_id.clone(), false), window, cx);
                        }
                    },
                ),
            )
            .child(
                secondary_button(format!("delete-session-{delete_id}"), "Delete")
                    .danger(true)
                    .on_click(move |_, window, cx| {
                        if let Some(delete) = delete.clone() {
                            delete(delete_id.clone(), window, cx);
                        }
                    }),
            )
            .into_any_element()
    }
}

impl Render for ArchiveSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = self.ctx.reveal(cx);
        let projects = self.ctx.hosts.archive.archived_projects(cx);
        let mut archived_projects = group(&reveal, "Archived projects").first(true).description(
            "Archive a project from the rail to keep its chats without listing it in the sidebar.",
        );
        if projects.is_empty() {
            archived_projects = archived_projects.child(card_note("No archived projects.", cx));
        }
        for project in &projects {
            archived_projects = archived_projects.child(self.project_row(project, cx));
        }

        let is_project = looks_like_project(&self.cwd);
        let title = if is_project {
            format!("Archived in {}", project_name(&self.cwd))
        } else {
            "Archived conversations".to_string()
        };
        let mut conversations = group(&reveal, title).child(
            row(&reveal, "Show archived in the sidebar")
                .id("show-archived")
                .description("Keep archived conversations listed alongside the active ones.")
                .switch_only()
                .child(
                    toggle("Show archived in the sidebar", self.show_archived).on_change(
                        cx.listener(|this, next: &bool, _, cx| this.on_show_archived(*next, cx)),
                    ),
                ),
        );
        let archived = self.archived();
        if !is_project {
            conversations = conversations.child(card_note(
                "Open a project to see its archived conversations.",
                cx,
            ));
        } else if archived.is_empty() {
            conversations =
                conversations.child(card_note("No archived conversations in this project.", cx));
        } else {
            for session in &archived {
                conversations = conversations.child(self.session_row(session, cx));
            }
        }

        let dialog = self.deleting.as_ref().map(|dialog| {
            let this = cx.entity().downgrade();
            let cancel_this = this.clone();
            dialog.render(
                Rc::new(move |_, cx| {
                    cancel_this
                        .update(cx, |this, cx| this.cancel_delete(cx))
                        .ok();
                }),
                Rc::new(move |window, cx| {
                    this.update(cx, |this, cx| this.confirm_delete(window, cx))
                        .ok();
                }),
                &self.dialog_focus,
                cx,
            )
        });

        div()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.deleting.is_some() && event.keystroke.key == "escape" {
                    this.cancel_delete(cx);
                    cx.stop_propagation();
                }
            }))
            .child(archived_projects)
            .child(conversations)
            .children(dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dates_as_month_and_day() {
        assert_eq!(format_date(0), "");
        assert_eq!(format_date(-5), "");
        let october_2 = 1_790_942_400_000;
        assert_eq!(
            format_date(october_2),
            monocode_platform::date_time::format_local(
                october_2,
                monocode_platform::date_time::DateTimeStyle::MonthDay,
            )
        );
        assert!(!format_date(october_2).is_empty());
    }
}
