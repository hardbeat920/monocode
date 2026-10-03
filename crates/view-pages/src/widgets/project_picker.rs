//! Port of src/features/projects/ui/SearchableProjectPicker.tsx: a trigger
//! with the project's mark and label that opens a searchable list of the
//! rail's projects, the current one first and checked.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, InputEvent, InputState, MoveDown, MoveUp};
use monocode_core::paths::basename;
use monocode_layout::paths::same_project_path;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::anchor::{
    BoundsCell, Side, anchored_popover, popover_layer, popover_surface,
};

use super::{plain_input, project_mark};
use crate::data::{ProjectMark, ProjectsData};
use crate::format::{looks_like_project, pretty_parent};

/// `mode`: what the trigger's accessible name says the pick does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectPickerMode {
    #[default]
    Switch,
    Move,
}

/// The trigger's look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectPickerAppearance {
    /// `text-content/50 hover:bg-content/5`.
    #[default]
    Ghost,
    /// `bg-content/10 hover:bg-content/14`.
    Filled,
    /// McpSettings: `h-7.5 gap-2 bg-content/5 px-2.5 text-[13px]
    /// hover:bg-content/12`.
    Settings,
}

type SelectFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// One row of the open list.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRow {
    pub path: String,
    pub mark: ProjectMark,
    pub current: bool,
}

pub struct ProjectPicker {
    cwd: String,
    rail_cwd: Option<String>,
    projects: Rc<dyn ProjectsData>,
    mode: ProjectPickerMode,
    appearance: ProjectPickerAppearance,
    layer: Option<usize>,
    open: bool,
    query: String,
    active: usize,
    search: Entity<InputState>,
    focus: FocusHandle,
    root_bounds: BoundsCell,
    scroll: ScrollHandle,
    on_select: Option<SelectFn>,
    _subscriptions: Vec<Subscription>,
}

impl ProjectPicker {
    pub fn new(
        cwd: &str,
        projects: Rc<dyn ProjectsData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects..."));
        let events = cx.subscribe_in(&search, window, |this, search, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = search.read(cx).value().to_string();
                this.active = 0;
                cx.notify();
            }
        });
        let weak = cx.weak_entity();
        let changes = projects.subscribe(
            Box::new(move |cx| {
                weak.update(cx, |_, cx| cx.notify()).ok();
            }),
            cx,
        );
        Self {
            cwd: cwd.to_string(),
            rail_cwd: None,
            projects,
            mode: ProjectPickerMode::Switch,
            appearance: ProjectPickerAppearance::Ghost,
            layer: None,
            open: false,
            query: String::new(),
            active: 0,
            search,
            focus: cx.focus_handle(),
            root_bounds: BoundsCell::default(),
            scroll: ScrollHandle::new(),
            on_select: None,
            _subscriptions: vec![events, changes],
        }
    }

    pub fn mode(mut self, mode: ProjectPickerMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn appearance(mut self, appearance: ProjectPickerAppearance) -> Self {
        self.appearance = appearance;
        self
    }

    /// The paint layer, for a picker inside a modal.
    pub fn layer(mut self, layer: usize) -> Self {
        self.layer = Some(layer);
        self
    }

    /// Called with the picked path. Picking the current project only closes.
    pub fn on_select(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// `railCwd`: the project whose rail supplies the choices.
    pub fn set_rail_cwd(&mut self, rail_cwd: Option<&str>, cx: &mut Context<Self>) {
        self.rail_cwd = rail_cwd.map(str::to_string);
        cx.notify();
    }

    pub fn set_cwd(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if self.cwd != cwd {
            self.cwd = cwd.to_string();
            cx.notify();
        }
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The trigger's label: the group label, else the folder name, or
    /// "Choose project" outside a project.
    pub fn label(&self, cx: &App) -> String {
        if looks_like_project(&self.cwd) {
            self.projects.mark(&self.cwd, cx).label
        } else {
            "Choose project".into()
        }
    }

    /// The trigger's accessible name.
    pub fn trigger_label(&self, cx: &App) -> String {
        if !looks_like_project(&self.cwd) {
            return "Choose project for note".into();
        }
        let action = match self.mode {
            ProjectPickerMode::Move => "Move note to project",
            ProjectPickerMode::Switch => "Switch project",
        };
        format!("{action}, current project {}", self.label(cx))
    }

    /// The listed projects: the rail's, with the current project added when
    /// missing, the current one first, filtered by label and path.
    pub fn rows(&self, cx: &App) -> Vec<ProjectRow> {
        let in_project = looks_like_project(&self.cwd);
        let rail = self
            .projects
            .rail_projects(self.rail_cwd.as_deref().unwrap_or(&self.cwd), cx);
        let mut projects = rail.clone();
        if in_project && !rail.iter().any(|path| same_project_path(path, &self.cwd)) {
            projects.insert(0, self.cwd.clone());
        }
        let (current, rest): (Vec<String>, Vec<String>) = projects
            .into_iter()
            .partition(|path| same_project_path(path, &self.cwd));
        let needle = monocode_core::js::trim(&self.query).to_lowercase();
        current
            .into_iter()
            .chain(rest)
            .filter_map(|path| {
                let mut mark = self.projects.mark(&path, cx);
                if mark.label.is_empty() {
                    mark.label = basename(&path);
                }
                if !needle.is_empty()
                    && !format!("{}\n{}", mark.label, path)
                        .to_lowercase()
                        .contains(&needle)
                {
                    return None;
                }
                Some(ProjectRow {
                    current: same_project_path(&path, &self.cwd),
                    path,
                    mark,
                })
            })
            .collect()
    }

    pub fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.query.clear();
        self.active = 0;
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
        cx.notify();
    }

    pub fn close_picker(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.query.clear();
        self.active = 0;
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        if restore {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// `pickProject`.
    pub fn pick(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(true, window, cx);
        if same_project_path(path, &self.cwd) {
            return;
        }
        if let Some(f) = self.on_select.clone() {
            let path = path.to_string();
            window.defer(cx, move |window, cx| f(&path, window, cx));
        }
    }

    fn list_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        match key {
            "down" => {
                if !rows.is_empty() {
                    self.active = (self.active + 1).min(rows.len() - 1);
                }
            }
            "up" => self.active = self.active.saturating_sub(1),
            "enter" => {
                if let Some(row) = rows.get(self.active) {
                    let path = row.path.clone();
                    self.pick(&path, window, cx);
                }
                return;
            }
            _ => return,
        }
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if !self.open {
            if key == "down" {
                self.open_picker(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if key == "escape" {
            self.close_picker(true, window, cx);
            cx.stop_propagation();
        }
    }

    fn render_trigger(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let in_project = looks_like_project(&self.cwd);
        let mark = self.projects.mark(&self.cwd, cx);
        let label = self.label(cx);
        let trigger_label = self.trigger_label(cx);
        let content = theme.colors.content;
        let mut trigger = div()
            .id("project-picker-trigger")
            .debug_selector(move || format!("project-picker-trigger {trigger_label}"))
            .flex()
            .min_w_0()
            .items_center()
            .rounded(u(theme.radius.md))
            .leading(theme.leading.none);
        trigger = match self.appearance {
            ProjectPickerAppearance::Settings => trigger
                .h(u(30.))
                .gap(u(8.))
                .px(u(10.))
                .text_px(theme.text.body),
            _ => trigger
                .h(u(26.))
                .gap(u(6.))
                .px(u(8.))
                .text_px(theme.text.label),
        };
        trigger = if self.open {
            trigger.bg(theme.colors.selection).text_color(content)
        } else {
            match self.appearance {
                ProjectPickerAppearance::Ghost => {
                    let hover = theme.content(0.05);
                    trigger
                        .text_color(theme.content(0.50))
                        .hover(move |s| s.bg(hover).text_color(content))
                }
                ProjectPickerAppearance::Filled => {
                    let hover = theme.content(0.14);
                    trigger
                        .bg(theme.content(0.10))
                        .text_color(content)
                        .hover(move |s| s.bg(hover))
                }
                ProjectPickerAppearance::Settings => {
                    let hover = theme.content(0.12);
                    trigger
                        .bg(theme.content(0.05))
                        .text_color(content)
                        .hover(move |s| s.bg(hover))
                }
            }
        };
        if in_project {
            trigger = trigger.child(project_mark(&mark, 14., 12., theme.content(0.50)));
        }
        let title_hint: Option<SharedString> = in_project.then(|| self.cwd.clone().into());
        if let Some(hint) = title_hint {
            trigger = trigger.tooltip(monocode_ui::widgets::tooltip(hint));
        }
        trigger
            .on_click(cx.listener(|this, _, window, cx| {
                if this.open {
                    this.close_picker(false, window, cx);
                } else {
                    this.open_picker(window, cx);
                }
            }))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .medium()
                    .text_color(theme.content(0.90))
                    .child(label),
            )
            .child(
                icon(if self.open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .size(u(12.))
                .text_color(theme.content(0.45)),
            )
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.rows(cx);
        let search_row = div()
            .flex()
            .flex_none()
            .h(u(44.))
            .items_center()
            .gap(u(10.))
            .px(u(12.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                icon(IconName::Search)
                    .size(u(16.))
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h(u(20.))
                    .items_center()
                    .text_px(theme.text.body)
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveDown, window, cx| {
                        this.list_key("down", window, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveUp, window, cx| {
                        this.list_key("up", window, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &Enter, window, cx| {
                        cx.stop_propagation();
                        this.list_key("enter", window, cx);
                    }))
                    .child(plain_input(&self.search, Some(theme.content(0.35)), cx)),
            );
        let mut list = div()
            .id("project-picker-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(u(6.));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(u(10.))
                    .py(u(20.))
                    .text_center()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("No projects found"),
            );
        }
        let hover = theme.content(0.05);
        let ink = theme.colors.content;
        for (index, row) in rows.into_iter().enumerate() {
            let highlighted = index == self.active;
            let path = row.path.clone();
            let selector_path = row.path.clone();
            let glyph = if row.current {
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(theme.colors.content)
                    .into_any_element()
            } else {
                project_mark(&row.mark, 16., 14., theme.content(0.75)).into_any_element()
            };
            list = list.child(
                div()
                    .id(("project-picker-row", index))
                    .debug_selector(move || format!("project-picker-row {selector_path}"))
                    .flex()
                    .flex_none()
                    .w_full()
                    .h(u(36.))
                    .items_center()
                    .gap(u(10.))
                    .px(u(10.))
                    .rounded(u(theme.radius.lg))
                    .map(|el| {
                        if highlighted {
                            el.bg(theme.colors.selection).text_color(ink)
                        } else {
                            el.text_color(theme.content(0.75))
                                .hover(move |s| s.bg(hover).text_color(ink))
                        }
                    })
                    .tooltip(monocode_ui::widgets::tooltip(row.path.clone()))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.active != index {
                            this.active = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.pick(&path, window, cx)))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .size(u(16.))
                            .items_center()
                            .justify_center()
                            .child(glyph),
                    )
                    .child(
                        div()
                            .flex_none()
                            .min_w_0()
                            .max_w(gpui::relative(0.7))
                            .truncate()
                            .text_px(theme.text.body)
                            .medium()
                            .child(row.mark.label.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(u(112.))
                            .truncate()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.40))
                            .child(pretty_parent(&row.path)),
                    ),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.root_bounds.contains(event.position) {
                this.close_picker(false, window, cx);
            }
        });
        popover_surface(
            "project-picker",
            Some(286.),
            Some(380.),
            outside,
            div()
                .flex()
                .flex_col()
                .max_h(u(380.))
                .font_family(theme.fonts.sans.clone())
                .debug_selector(|| "project-picker".into())
                .child(search_row)
                .child(list),
        )
    }
}

impl Focusable for ProjectPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ProjectPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("project-picker-root")
            .relative()
            .flex()
            .min_w_0()
            .items_center()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.root_bounds.probe())
            .child(self.render_trigger(&theme, cx));
        if self.open {
            let layer = self.layer.unwrap_or_else(|| popover_layer(cx));
            let menu = self.render_menu(&theme, cx).into_any_element();
            root = root.child(anchored_popover(Side::Bottom, 4., layer, window, menu));
        }
        root
    }
}
