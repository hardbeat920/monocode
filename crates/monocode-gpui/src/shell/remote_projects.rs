//! AddRemoteProjectDialog.tsx: browse host directories and remember their remote identity.
use super::*;
use crate::settings::widgets::secondary_button;
use gpui::EventEmitter;
use monocode_core::remote::{Connections, Machine};
use serde::Deserialize;
use serde_json::json;

pub(super) struct Open {
    picker: Entity<RemoteProjectPicker>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

#[derive(Clone, Debug, Deserialize)]
struct Directory {
    path: String,
    parent: Option<String>,
    entries: Vec<DirectoryEntry>,
}
#[derive(Clone, Debug, Deserialize)]
struct DirectoryEntry {
    name: String,
    path: String,
}

enum Picked {
    Open(String),
    Connections,
    Cancel,
}

fn browse_arguments(path: Option<String>) -> serde_json::Value {
    // The original host distinguishes an omitted path (home) from null,
    // which is an invalid directory argument.
    path.map_or_else(|| json!({}), |path| json!({"path":path}))
}

struct RemoteProjectPicker {
    remote: Arc<Connections>,
    store: Database,
    machines: Vec<Machine>,
    loaded: bool,
    machine: Option<String>,
    path: Entity<input::Input>,
    directory: Option<Directory>,
    loading: bool,
    opening: bool,
    error: String,
    version: u64,
    focus: FocusHandle,
    _events: Vec<Subscription>,
}
impl EventEmitter<Picked> for RemoteProjectPicker {}
impl Focusable for RemoteProjectPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl RemoteProjectPicker {
    fn new(remote: Arc<Connections>, store: Database, cx: &mut Context<Self>) -> Self {
        let path = cx.new(|cx| {
            let mut input = input::Input::field(
                "Folder path on the machine",
                "/home/me/code/my-app",
                content(0.35),
                cx,
            );
            input.set_font_size(12.);
            input
        });
        let events = vec![
            cx.subscribe(&path, |_, _, _: &input::Changed, cx| cx.notify()),
            cx.subscribe(&path, |this, _, _: &input::Submitted, cx| this.open(cx)),
        ];
        let mut this = Self {
            focus: cx.focus_handle(),
            remote,
            store,
            machines: vec![],
            loaded: false,
            machine: None,
            path,
            directory: None,
            loading: false,
            opening: false,
            error: String::new(),
            version: 0,
            _events: events,
        };
        this.load_machines(cx);
        this
    }

    fn machine(&self) -> Option<&Machine> {
        self.machine
            .as_ref()
            .and_then(|id| self.machines.iter().find(|m| &m.id == id))
            .or_else(|| self.machines.first())
    }

    fn load_machines(&mut self, cx: &mut Context<Self>) {
        let remote = self.remote.clone();
        let work = cx.background_executor().spawn(async move { remote.list() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.loaded = true;
                match result {
                    Ok(machines) => {
                        this.machines = machines;
                        this.browse(None, cx);
                        if !this.machines.is_empty() {
                            window.focus(&this.path.focus_handle(cx), cx);
                        }
                    }
                    Err(error) => this.error = error,
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_machine(&mut self, id: String, cx: &mut Context<Self>) {
        if self.machine().is_some_and(|machine| machine.id == id) {
            return;
        }
        self.machine = Some(id);
        self.directory = None;
        self.path
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.browse(None, cx);
    }

    fn browse(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        let Some(machine) = self.machine() else {
            return;
        };
        let machine_id = machine.id.clone();
        let remote = self.remote.clone();
        self.version += 1;
        let version = self.version;
        self.loading = true;
        self.opening = false;
        self.error.clear();
        let work = cx.background_executor().spawn(async move {
            let value = remote.request(&machine_id, "projects.browse", browse_arguments(path))?;
            serde_json::from_value::<Directory>(value)
                .map_err(|e| format!("Invalid host directory: {e}"))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.version != version {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(directory) => {
                        this.path
                            .update(cx, |input, cx| input.set_text(directory.path.clone(), cx));
                        this.directory = Some(directory);
                    }
                    Err(error) => this.error = error,
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if self.opening {
            return;
        }
        let Some(machine) = self.machine().cloned() else {
            return;
        };
        let path = self.path.read(cx).content.trim().to_owned();
        if path.is_empty() {
            return;
        }
        self.version += 1;
        let version = self.version;
        self.opening = true;
        self.loading = false;
        self.error.clear();
        let remote = self.remote.clone();
        let environment = machine.environment_id.clone();
        let work = cx.background_executor().spawn(async move {
            remote.request(&machine.id, "projects.open", json!({"cwd": path}))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.version != version {
                    return;
                }
                match result {
                    Ok(project) => this.remember_project(environment, project, version, cx),
                    Err(error) => {
                        this.opening = false;
                        this.error = error;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remember_project(
        &mut self,
        environment: String,
        project: serde_json::Value,
        version: u64,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let work = cx
            .background_executor()
            .spawn(async move { locked(&store)?.remember_remote_project(&environment, &project) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.version != version {
                    return;
                }
                this.opening = false;
                match result {
                    Ok(project) => cx.emit(Picked::Open(project.key)),
                    Err(error) => this.error = error,
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn folder_row(
        &self,
        id: usize,
        name: String,
        path: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("remote-folder", id))
            .role(Role::Button)
            .aria_label(format!("Browse {name}"))
            .w_full()
            .rounded_md()
            .px_2()
            .py(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .text_color(content(0.75))
            .cursor_pointer()
            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
            .child(icon("folder", 14.).text_color(content(0.45)))
            .child(div().min_w_0().flex_1().truncate().child(name))
            .child(icon("chevron-right", 12.).text_color(content(0.3)))
            .on_click(cx.listener(move |this, _, _, cx| this.browse(Some(path.clone()), cx)))
            .into_any_element()
    }
}
impl Render for RemoteProjectPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let machine = self.machine().cloned();
        let mut view = div().id("remote-project-picker").debug_selector(|| "remote-project-picker".into()).role(Role::Dialog).aria_label("Open folder on a machine")
            .track_focus(&self.focus)
            .p_4().flex().flex_col().gap_3().min_h_0().text_color(content(1.))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" { cx.stop_propagation(); cx.emit(Picked::Cancel); }
            }))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("Open folder on a machine"))
            .child(div().text_size(px(12.)).line_height(px(17.)).text_color(content(0.55))
                .child("Sessions in this project run on that machine, using its checkout and its Codex or Claude Code sign-in. They keep running when you close MonoCode here."));
        if !self.loaded {
            view = view.child(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.45))
                    .child("Loading machines…"),
            );
        } else if let Some(machine) = machine {
            if self.machines.len() > 1 {
                view = view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.55))
                                .child("Machine"),
                        )
                        .child(div().flex().flex_wrap().gap_2().children(
                            self.machines.iter().map(|entry| {
                                let id = entry.id.clone();
                                secondary_button(
                                    SharedString::from(format!("remote-project-machine-{id}")),
                                    entry.name.clone(),
                                    false,
                                    false,
                                )
                                .when(entry.id == machine.id, |d| d.bg(content(0.12)))
                                .child(icon("globe", 14.))
                                .child(entry.name.clone())
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.select_machine(id.clone(), cx),
                                ))
                            }),
                        )),
                );
            } else {
                view = view.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(content(0.55))
                        .child(icon("globe", 14.))
                        .child(format!("On {}", machine.name)),
                );
            }
            let path = self.path.read(cx).content.trim().to_owned();
            view = view.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .h_8()
                            .min_w_0()
                            .flex_1()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(content(0.03))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .font_family("Menlo")
                            .child(self.path.clone()),
                    )
                    .child(
                        secondary_button(
                            "browse-remote-path",
                            "Browse path",
                            false,
                            path.is_empty(),
                        )
                        .child("Browse")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let path = this.path.read(cx).content.trim().to_owned();
                            if !path.is_empty() {
                                this.browse(Some(path), cx);
                            }
                        })),
                    ),
            );
            let mut folders = div()
                .id("remote-project-folders")
                .role(Role::List)
                .aria_label("Folders")
                .min_h(px(96.))
                .max_h(px(280.))
                .overflow_y_scroll()
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .p_1()
                .flex()
                .flex_col();
            if let Some(directory) = &self.directory {
                if let Some(parent) = &directory.parent {
                    folders = folders.child(self.folder_row(0, "..".into(), parent.clone(), cx));
                }
                folders =
                    folders.children(directory.entries.iter().enumerate().map(|(ix, entry)| {
                        self.folder_row(ix + 1, entry.name.clone(), entry.path.clone(), cx)
                    }));
                if directory.entries.is_empty() {
                    folders = folders.child(
                        div()
                            .px_2()
                            .py(px(6.))
                            .text_size(px(12.))
                            .text_color(content(0.45))
                            .child("No subfolders"),
                    );
                }
            } else if self.loading {
                folders = folders.child(
                    div()
                        .px_2()
                        .py(px(6.))
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child("Loading folders…"),
                );
            }
            view = view.child(folders);
        } else {
            view = view.child(div().text_size(px(12.)).line_height(px(17.)).text_color(content(0.55))
                .child("No machines are connected yet. Add one in Settings, then open a folder on it here."));
        }
        if !self.error.is_empty() {
            view = view.child(
                div()
                    .id("remote-project-error")
                    .role(Role::Alert)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(rgb(0xf87171))
                    .child(self.error.clone()),
            );
        }
        view.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    crate::modal::ghost_button("cancel-remote-project", "Cancel")
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Picked::Cancel))),
                )
                .when(self.loaded && self.machines.is_empty(), |d| {
                    d.child(
                        crate::modal::primary_button("add-project-machine", "Add a machine")
                            .cursor_pointer()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Picked::Connections))),
                    )
                })
                .when(!self.machines.is_empty(), |d| {
                    d.child(
                        crate::modal::disabled(
                            crate::modal::primary_button(
                                "open-remote-project",
                                if self.opening { "Opening…" } else { "Open" },
                            ),
                            self.opening || self.path.read(cx).content.trim().is_empty(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                    )
                }),
        )
    }
}

impl Shell {
    pub(super) fn open_remote_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self
            .remote_project_picker
            .as_ref()
            .and_then(|o| o.restore.clone())
            .or_else(|| window.focused(cx));
        self.close_menu(cx);
        self.close_preview(cx);
        let picker =
            cx.new(|cx| RemoteProjectPicker::new(self.remote.clone(), self.store.clone(), cx));
        let events = cx.subscribe_in(&picker, window, |this, _, event: &Picked, window, cx| {
            this.close_remote_project(window, cx);
            match event {
                Picked::Open(key) => {
                    this.workspace.add_project(key);
                    this.select_project(key.clone(), window, cx);
                }
                Picked::Connections => {
                    this.open_settings(crate::settings::Section::Connections, window, cx)
                }
                Picked::Cancel => {}
            }
            this.invalidate_chrome(cx);
            cx.notify();
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.remote_project_picker = Some(Open {
            picker,
            restore,
            _events: events,
        });
        cx.notify();
    }

    pub(super) fn close_remote_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.remote_project_picker.take() {
            if let Some(restore) = open.restore {
                window.focus(&restore, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn remote_project_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.remote_project_picker.as_ref()?;
        let viewport = window.viewport_size();
        let width = 480f32.min(viewport.width.as_f32() - 24.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px((viewport.height.as_f32() * 0.16).max(16.)),
        ));
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("remote-project-scrim"))
                .child(
                    popover("remote-project-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .max_height(viewport.height.as_f32() * 0.7)
                        .gap(0.)
                        .padding(0.)
                        .priority(80)
                        .on_dismiss(move |window, cx| {
                            let _ =
                                weak.update(cx, |this, cx| this.close_remote_project(window, cx));
                        })
                        .child(open.picker.clone()),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_home_omits_the_optional_path_instead_of_sending_null() {
        let home = browse_arguments(None);
        assert!(home.get("path").is_none());
        assert_eq!(home, json!({}));
        assert_eq!(
            browse_arguments(Some("C:\\Users\\me".into())),
            json!({"path":"C:\\Users\\me"})
        );
    }

    #[test]
    fn remote_directory_keeps_host_paths_unchanged() {
        let directory: Directory = serde_json::from_value(json!({
            "path": "C:\\Users\\me\\code",
            "parent": "C:\\Users\\me",
            "entries": [{"name": "my-app", "path": "C:\\Users\\me\\code\\my-app"}],
        }))
        .unwrap();
        assert_eq!(directory.path, "C:\\Users\\me\\code");
        assert_eq!(directory.entries[0].path, "C:\\Users\\me\\code\\my-app");
        assert!(
            serde_json::from_value::<Directory>(
                json!({"path": "/home", "entries": [{"name": "missing-path"}]})
            )
            .is_err()
        );
    }

    #[gpui::test]
    fn escape_closes_remote_folder_browser_without_switching_projects(
        cx: &mut gpui::TestAppContext,
    ) {
        let (window, _, _) = crate::shell::tests::window(cx);
        let previous = window
            .update(cx, |shell, window, cx| {
                let previous = shell.workspace.project.clone();
                shell.open_remote_project(window, cx);
                assert!(shell.remote_project_picker.is_some());
                previous
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("remote-project-picker").is_some());
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert!(shell.remote_project_picker.is_none());
                assert_eq!(shell.workspace.project, previous);
            })
            .unwrap();
    }
}
