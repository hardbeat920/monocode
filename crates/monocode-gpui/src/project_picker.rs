//! A searchable list of the open projects, shown as a modal. Its caller sets
//! the wording and which rows cannot be picked; duplicating a Mono uses it.
use crate::{input, project_settings::project_icon, theme::*};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, Role, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use monocode_core::{composer, project_appearance::ProjectAppearance};
use std::path::{Path, PathBuf};

pub(crate) struct Project {
    pub path: String,
    pub label: String,
    pub appearance: ProjectAppearance,
    pub current: bool,
    /// Why the row cannot be picked, shown in place of "Current".
    pub unavailable: Option<&'static str>,
}
pub(crate) struct Labels {
    pub title: SharedString,
    pub action: &'static str,
    pub empty: &'static str,
}

pub(crate) enum Picked {
    Project(String),
    Dismiss,
}
pub(crate) struct ProjectPicker {
    pub(crate) search: Entity<input::Input>,
    labels: Labels,
    projects: Vec<Project>,
    /// Indices into `projects`, best match first.
    rows: Vec<usize>,
    active: usize,
    scroll: gpui::ScrollHandle,
    _changed: Subscription,
}
impl EventEmitter<Picked> for ProjectPicker {}
impl Focusable for ProjectPicker {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}
impl ProjectPicker {
    pub(crate) fn new(projects: Vec<Project>, labels: Labels, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            input::Input::field(labels.title.clone(), "Search projects…", content(0.35), cx)
        });
        let changed = cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
            this.active = 0;
            this.rank(cx);
            cx.notify();
        });
        let mut this = Self {
            search,
            labels,
            projects,
            rows: Vec::new(),
            active: 0,
            scroll: gpui::ScrollHandle::new(),
            _changed: changed,
        };
        this.rank(cx);
        // Picking is for reaching another project, so start past the current
        // one when it leads the list.
        if this.rows.len() > 1 && this.projects[this.rows[0]].current {
            this.active = 1;
        }
        this.skip_unavailable(1);
        this
    }
    fn available(&self, row: usize) -> bool {
        self.rows
            .get(row)
            .is_some_and(|ix| self.projects[*ix].unavailable.is_none())
    }
    /// Moves `active` off unavailable rows in `direction`, if any row is free.
    fn skip_unavailable(&mut self, direction: isize) {
        let len = self.rows.len() as isize;
        for step in 0..len {
            let row = (self.active as isize + step * direction).rem_euclid(len) as usize;
            if self.available(row) {
                self.active = row;
                break;
            }
        }
        self.scroll.scroll_to_item(self.active);
    }
    fn rank(&mut self, cx: &gpui::App) {
        let query = self.search.read(cx).content.trim().to_owned();
        let mut ranked: Vec<_> = self
            .projects
            .iter()
            .enumerate()
            .filter_map(|(ix, project)| {
                let by_label = composer::score(&query, &project.label);
                let by_path = composer::score(&query, &project.path).map(|s| s / 2);
                by_label.max(by_path).map(|score| (score, ix))
            })
            .collect();
        // Stable, so an empty query keeps the rail's order.
        ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        self.rows = ranked.into_iter().map(|(_, ix)| ix).collect();
        self.active = self.active.min(self.rows.len().saturating_sub(1));
        self.skip_unavailable(1);
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.rows.is_empty() {
            self.active =
                (self.active as isize + delta).rem_euclid(self.rows.len() as isize) as usize;
            self.skip_unavailable(delta.signum());
            cx.notify();
        }
    }
    fn pick(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.search.read(cx).is_composing() || !self.available(row) {
            return;
        }
        if let Some(project) = self.rows.get(row).map(|ix| &self.projects[*ix]) {
            cx.emit(Picked::Project(project.path.clone()));
        }
    }
}
fn pretty_path(path: &str) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = Path::new(path).strip_prefix(home)
    {
        return format!("~/{}", rest.display());
    }
    path.into()
}
impl Render for ProjectPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(row, ix)| {
                let project = &self.projects[*ix];
                let note = project.unavailable.or(project.current.then_some("Current"));
                div()
                    .id(("project-picker-row", row))
                    .role(Role::ListItem)
                    .aria_label(project.label.clone())
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .flex()
                    .gap_2p5()
                    .items_center()
                    .bg(if self.active == row {
                        content(0.1)
                    } else {
                        content(0.)
                    })
                    .when_some(project.unavailable, |d, why| {
                        d.opacity(0.45).aria_description(why)
                    })
                    .when(project.unavailable.is_none(), |d| {
                        d.hover(|s| s.bg(content(0.07)))
                    })
                    .child(
                        div()
                            .size(px(22.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(project_icon(&project.appearance, &project.path, 18.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .child(div().truncate().child(project.label.clone()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .truncate()
                                    .child(pretty_path(&project.path)),
                            ),
                    )
                    .when_some(note, |d, note| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child(note),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(row, cx)))
                    .into_any_element()
            })
            .collect();
        div()
            .id("project-picker")
            .role(Role::Dialog)
            .aria_label(self.labels.title.clone())
            .w_full()
            .flex()
            .flex_col()
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.active, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|_, _: &crate::shell::DismissMenu, _, cx| {
                cx.emit(Picked::Dismiss);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(content(0.08))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .child(icon("search", 16.).text_color(content(0.5)))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("project-picker-results")
                    .role(Role::ListBox)
                    .aria_label("Projects")
                    .p_1p5()
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows)
                    .when(self.rows.is_empty(), |d| {
                        d.child(
                            div()
                                .px_3()
                                .py_6()
                                .text_size(px(13.))
                                .text_color(content(0.45))
                                .child(if self.projects.is_empty() {
                                    self.labels.empty
                                } else {
                                    "No matching projects"
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(content(0.07))
                    .text_size(px(10.))
                    .text_color(content(0.35))
                    .child(format!(
                        "↑ ↓ navigate    ↵ {}    Esc close",
                        self.labels.action
                    )),
            )
    }
}
