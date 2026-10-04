//! The searchable project picker shared by the sidebar and compact rail.

use super::{
    Shell,
    project_rail::{Project, project_mark, rail_projects},
};
use gpui::{
    Anchor, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Point, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window,
    div,
};
use gpui_component::input::{Enter, Escape, InputEvent, InputState, MoveDown, MoveUp};
use monocode_engine::runtime::util::project_path::same_project_path;
use monocode_engine::{projects::ProjectsGlobal, runtime::Engine};
use monocode_ui::widgets::{popover_at, popover_frame, text_field, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

pub(super) enum ProjectPickerEvent {
    Select(String),
    OpenFolder,
    Menu {
        path: String,
        position: Point<Pixels>,
    },
    Close,
}

pub(super) struct ProjectPicker {
    shell: WeakEntity<Shell>,
    cwd: String,
    position: Point<Pixels>,
    query: Entity<InputState>,
    active: usize,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProjectPickerEvent> for ProjectPicker {}

fn filtered_projects(mut projects: Vec<Project>, cwd: &str, query: &str) -> Vec<Project> {
    if let Some(index) = projects
        .iter()
        .position(|project| same_project_path(&project.path, cwd))
    {
        let current = projects.remove(index);
        projects.insert(0, current);
    }
    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        projects.retain(|project| {
            format!("{}\n{}", project.name, project.path)
                .to_lowercase()
                .contains(&query)
        });
    }
    projects
}

impl ProjectPicker {
    fn new(
        shell: WeakEntity<Shell>,
        cwd: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let mut subscriptions = vec![cx.subscribe(&query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.active = 0;
                this.scroll.scroll_to_item(0);
                cx.notify();
            }
        })];
        if let Some(projects) = ProjectsGlobal::try_global(cx).map(|global| global.projects.clone())
        {
            subscriptions.push(cx.observe(&projects, |_, _, cx| cx.notify()));
        }
        if let Some(sessions) = Engine::try_global(cx).map(|engine| engine.sessions.clone()) {
            subscriptions.push(cx.observe(&sessions, |_, _, cx| cx.notify()));
        }
        Self {
            shell,
            cwd,
            position,
            query,
            active: 0,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn results(&self, cx: &mut App) -> Vec<Project> {
        filtered_projects(
            rail_projects(&self.cwd, cx).0,
            &self.cwd,
            self.query.read(cx).value().as_ref(),
        )
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        self.active = self
            .active
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = self.results(cx).get(self.active) {
            cx.emit(ProjectPickerEvent::Select(project.path.clone()));
        }
    }

    fn panel(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let focus = self.query.read(cx).focus_handle(cx);
        div()
            .id("project-picker")
            .debug_selector(|| "project-picker".into())
            .key_context("ProjectPicker")
            .track_focus(&focus)
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Enter, _, cx| {
                this.confirm(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.emit(ProjectPickerEvent::Close);
                cx.stop_propagation();
            }))
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                let modifiers = event.keystroke.modifiers;
                if key == "contextmenu"
                    || key == "menu"
                    || (key == "f10"
                        && modifiers.shift
                        && !modifiers.control
                        && !modifiers.platform
                        && !modifiers.alt)
                {
                    if let Some(project) = this.results(cx).get(this.active) {
                        cx.emit(ProjectPickerEvent::Menu {
                            path: project.path.clone(),
                            position: this.position,
                        });
                    }
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if !this.menu_active(cx) {
                    cx.emit(ProjectPickerEvent::Close);
                }
            }))
    }

    fn menu_active(&self, cx: &App) -> bool {
        self.shell.upgrade().is_some_and(|shell| {
            let shell = shell.read(cx);
            shell.project_menu.is_some() || shell.project_dialog.is_some()
        })
    }
}

impl Render for ProjectPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let projects = self.results(cx);
        self.active = self.active.min(projects.len().saturating_sub(1));
        let fill = theme.content(0.08);
        let mut rows = div()
            .id("project-picker-list")
            .flex()
            .flex_col()
            .gap(gpui::px(1.))
            .max_h(window.viewport_size().height * 0.5)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(u(4.));
        if projects.is_empty() {
            rows = rows.child(
                div()
                    .p(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child("No matching projects"),
            );
        }
        for (index, project) in projects.iter().enumerate() {
            let selected = index == self.active;
            let row = div()
                .id(("project-picker-row", index))
                .debug_selector(move || format!("project-picker-row:{index}"))
                .flex()
                .items_center()
                .gap(u(8.))
                .h(u(32.))
                .px(u(8.))
                .rounded(u(theme.radius.md))
                .text_color(theme.colors.content)
                .bg(if selected {
                    theme.colors.selection
                } else {
                    gpui::transparent_black()
                })
                .hover(move |style| style.bg(fill))
                .child(project_mark(Some(project), 12., &theme))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_px(theme.text.ui)
                        .child(project.name.clone()),
                )
                .children(
                    same_project_path(&project.path, &self.cwd)
                        .then(|| icon(IconName::Check).size(u(14.))),
                )
                .tooltip(tooltip(project.path.clone()))
                .on_mouse_down(MouseButton::Right, {
                    let path = project.path.clone();
                    cx.listener(move |_, event: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(ProjectPickerEvent::Menu {
                            path: path.clone(),
                            position: event.position,
                        });
                    })
                })
                .on_click({
                    let path = project.path.clone();
                    cx.listener(move |_, _, _, cx| {
                        cx.emit(ProjectPickerEvent::Select(path.clone()))
                    })
                });
            rows = rows.child(row);
        }
        let panel = self.panel(cx).child(
            popover_frame("project-picker-frame")
                .width(280.)
                .child(
                    div()
                        .p(u(8.))
                        .border_b_1()
                        .border_color(theme.colors.stroke)
                        .child(text_field(&self.query).icon(IconName::Search)),
                )
                .child(rows)
                .child(
                    div()
                        .id("project-picker-open-folder")
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .h(u(32.))
                        .px(u(12.))
                        .border_t_1()
                        .border_color(theme.colors.stroke)
                        .text_px(theme.text.ui)
                        .text_color(theme.content(0.70))
                        .hover(move |style| style.bg(fill))
                        .child(icon(IconName::Plus).size(u(14.)))
                        .child("Open project")
                        .on_click(
                            cx.listener(|_, _, _, cx| cx.emit(ProjectPickerEvent::OpenFolder)),
                        ),
                ),
        );
        popover_at(self.position, Anchor::TopLeft, panel, cx)
    }
}

impl Shell {
    pub(super) fn show_project_picker(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_picker.is_some() {
            self.project_menu = None;
            self.project_menu_subscription = None;
            self.project_menu_return_focus = None;
            self.close_project_picker(window, cx);
            return;
        }
        self.project_menu = None;
        self.project_menu_subscription = None;
        self.project_picker_return_focus = self
            .project_menu_return_focus
            .take()
            .or_else(|| window.focused(cx));
        let cwd = self.sidebar_cwd(cx);
        let shell = cx.weak_entity();
        let picker = cx.new(|cx| ProjectPicker::new(shell, cwd, position, window, cx));
        self.project_picker_subscription =
            Some(
                cx.subscribe_in(&picker, window, |shell, _, event, window, cx| match event {
                    ProjectPickerEvent::Select(path) => {
                        shell.close_project_picker(window, cx);
                        if !same_project_path(path, &shell.sidebar_cwd(cx)) {
                            shell.select_project(path, cx);
                        }
                    }
                    ProjectPickerEvent::OpenFolder => {
                        shell.close_project_picker(window, cx);
                        shell.open_project_folder(cx);
                    }
                    ProjectPickerEvent::Menu { path, position } => {
                        shell.show_project_menu(path, *position, window, cx)
                    }
                    ProjectPickerEvent::Close => shell.close_project_picker(window, cx),
                }),
            );
        picker
            .read(cx)
            .query
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        self.project_picker = Some(picker);
        cx.notify();
    }

    fn close_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_picker = None;
        self.project_picker_subscription = None;
        if let Some(focus) = self.project_picker_return_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellOptions;
    use gpui::{FocusHandle, TestAppContext};
    use monocode_engine::{
        history::History,
        projects::{ProjectsConfig, testing::FakeBackend},
        runtime::testing::init_test_engine,
        workspace::WorkspaceConfig,
    };
    use monocode_settings::Kv;
    use std::sync::Arc;

    struct PickerRoot {
        shell: Entity<Shell>,
        focus: FocusHandle,
    }

    impl Render for PickerRoot {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().track_focus(&self.focus).children(
                self.shell.read(cx).project_picker.clone().map(|picker| {
                    picker.update(cx, |picker, cx| {
                        // The styled input synchronizes macOS autofill through a native
                        // window handle. Test windows render the same editing state
                        // directly and retain the production picker action handlers.
                        picker.panel(cx).child(picker.query.clone())
                    })
                }),
            )
        }
    }

    #[gpui::test]
    fn picker_search_and_keyboard_selection_switch_the_workspace_and_escape_restores_focus(
        cx: &mut TestAppContext,
    ) {
        init_test_engine(cx);
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            ProjectsGlobal::init(
                ProjectsConfig {
                    kv: Kv::in_memory(),
                    backend: FakeBackend::new(),
                    clock: Arc::new(|| 1_000_000),
                },
                cx,
            );
            ProjectsGlobal::projects(cx).update(cx, |projects, cx| {
                projects.remember_project("/work/current", cx);
                projects.remember_project("/work/other", cx);
            });
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(ShellOptions::full(), window, cx);
                let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
                shell.attach(
                    WorkspaceConfig::fresh(Some("/work/current")),
                    history,
                    window,
                    cx,
                );
                shell
            });
            cx.observe(&shell, |_, _, cx| cx.notify()).detach();
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            shell.update(cx, |shell, cx| {
                shell.show_project_picker(gpui::point(gpui::px(20.), gpui::px(20.)), window, cx)
            });
            PickerRoot { shell, focus }
        });
        cx.run_until_parked();
        cx.simulate_input("other");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let picker = root.read(cx).shell.read(cx).project_picker.clone().unwrap();
            assert_eq!(
                picker
                    .update(cx, |picker, cx| picker.results(cx))
                    .iter()
                    .map(|project| project.path.as_str())
                    .collect::<Vec<_>>(),
                ["/work/other"]
            );
        });
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let shell = root.read(cx).shell.clone();
            assert!(shell.read(cx).project_picker.is_none());
            assert_eq!(
                shell
                    .read(cx)
                    .workspace
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .project_cwd(),
                "/work/other"
            );
            shell.update(cx, |shell, cx| {
                shell.show_project_picker(gpui::point(gpui::px(20.), gpui::px(20.)), window, cx)
            });
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let root = root.read(cx);
            assert!(root.shell.read(cx).project_picker.is_none());
            assert!(root.focus.is_focused(window));
        });
    }

    #[test]
    fn picker_searches_saved_labels_and_remote_paths_and_prioritizes_the_current_project() {
        let project = |name: &str, path: &str| Project {
            name: name.into(),
            path: path.into(),
            additions: 0,
            deletions: 0,
            busy: false,
            color: 0x7dd3fc,
            logo: None,
            mascot: None,
        };
        let projects = vec![
            project("Remote backend", "remote://host/srv/api"),
            project("Website", "/work/site"),
        ];
        assert_eq!(
            filtered_projects(projects.clone(), "/work/site", "")
                .iter()
                .map(|project| project.path.as_str())
                .collect::<Vec<_>>(),
            ["/work/site", "remote://host/srv/api"]
        );
        assert_eq!(
            filtered_projects(projects.clone(), "/work/site", "BACKEND")[0].path,
            "remote://host/srv/api"
        );
        assert_eq!(
            filtered_projects(projects, "/work/site", " /SRV/API ")[0].path,
            "remote://host/srv/api"
        );
    }
}
