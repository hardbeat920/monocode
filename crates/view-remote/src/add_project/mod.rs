//! Port of src/features/connections/ui/AddRemoteProjectDialog.tsx: adds a
//! project whose folder is on a connected machine. Sessions in it run on
//! that machine; the project otherwise behaves like any other in the rail.
//!
//! The owner renders the dialog while it is open and closes it on
//! [`AddRemoteProjectEvent::Cancel`] and [`AddRemoteProjectEvent::Open`].

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, deferred, div,
    relative,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_remote::host::protocol::{HostDirectory, HostProject, RemoteMachine};
use monocode_ui::color::with_alpha;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::{SearchableSelect, SearchableSelectOption};
use serde_json::{Value, json};

use crate::host::RemoteHost;
use crate::machines::strip_error;
use crate::style::{ButtonKind, FieldStyle, action_button, text_input, tinted_text};

#[cfg(test)]
mod tests;

/// What the dialog asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddRemoteProjectEvent {
    /// `onCancel`: close the dialog.
    Cancel,
    /// `onOpen`: the folder was added; this is its rail key. Close the
    /// dialog and select the project.
    Open(String),
    /// `OPEN_CONNECTIONS_EVENT`: show Settings → Connections. A
    /// [`AddRemoteProjectEvent::Cancel`] follows.
    OpenConnections,
}

pub struct AddRemoteProjectDialog {
    host: Rc<dyn RemoteHost>,
    machines: Vec<RemoteMachine>,
    loaded: bool,
    machine_id: Option<String>,
    path: Entity<InputState>,
    /// A path to put in the field on the next frame, which has a window.
    pending_path: Option<String>,
    directory: Option<HostDirectory>,
    loading: bool,
    opening: bool,
    error: String,
    /// False once the dialog was cancelled: late answers are dropped.
    alive: bool,
    request_version: u64,
    /// The machine the folder list was last read for.
    browsed: Option<Option<String>>,
    select: Option<Entity<SearchableSelect>>,
    focus: FocusHandle,
    focused_once: bool,
    request: Option<Task<()>>,
    machines_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AddRemoteProjectEvent> for AddRemoteProjectDialog {}

impl Focusable for AddRemoteProjectDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AddRemoteProjectDialog {
    pub fn new(host: Rc<dyn RemoteHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("/home/me/code/my-app"));
        let subscriptions =
            vec![
                cx.subscribe(&path, |this, _, event: &InputEvent, cx| match event {
                    InputEvent::PressEnter { .. } => this.open(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                }),
            ];
        let mut this = Self {
            host,
            machines: Vec::new(),
            loaded: false,
            machine_id: None,
            path,
            pending_path: None,
            directory: None,
            loading: false,
            opening: false,
            error: String::new(),
            alive: true,
            request_version: 0,
            browsed: None,
            select: None,
            focus: cx.focus_handle(),
            focused_once: false,
            request: None,
            machines_task: None,
            _subscriptions: subscriptions,
        };
        this.refresh_machines(cx);
        this
    }

    /// Reads the machine list again.
    pub fn refresh_machines(&mut self, cx: &mut Context<Self>) {
        let task = self.host.machines(cx);
        self.machines_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if let Ok(machines) = result {
                    this.machines = machines;
                    this.select = None;
                }
                this.loaded = true;
                this.effects(cx);
            })
            .ok();
        }));
    }

    /// The chosen machine, or the first one.
    pub fn machine(&self) -> Option<&RemoteMachine> {
        self.machine_id
            .as_ref()
            .and_then(|id| self.machines.iter().find(|entry| &entry.id == id))
            .or_else(|| self.machines.first())
    }

    pub fn machines(&self) -> &[RemoteMachine] {
        &self.machines
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn directory(&self) -> Option<&HostDirectory> {
        self.directory.as_ref()
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn is_opening(&self) -> bool {
        self.opening
    }

    /// The folder path as typed or browsed.
    pub fn path(&self, cx: &App) -> String {
        self.pending_path
            .clone()
            .unwrap_or_else(|| self.path.read(cx).value().to_string())
    }

    pub fn set_machine(&mut self, machine_id: &str, cx: &mut Context<Self>) {
        self.machine_id = Some(machine_id.to_string());
        self.effects(cx);
    }

    /// Reads the folder list again when the machine changed.
    fn effects(&mut self, cx: &mut Context<Self>) {
        let current = self.machine().map(|machine| machine.id.clone());
        if self.browsed.as_ref() != Some(&current) {
            self.browsed = Some(current.clone());
            self.directory = None;
            self.pending_path = Some(String::new());
            if current.is_some() {
                self.browse(None, cx);
            }
        }
        cx.notify();
    }

    /// `browse`: lists `next`, or the host's home folder.
    pub fn browse(&mut self, next: Option<String>, cx: &mut Context<Self>) {
        let Some(machine) = self.machine().cloned() else {
            return;
        };
        self.request_version += 1;
        let version = self.request_version;
        self.loading = true;
        self.opening = false;
        self.error.clear();
        let params = match next {
            Some(path) => json!({ "path": path }),
            None => json!({}),
        };
        let task = self
            .host
            .request(&machine.id, "projects.browse", params, cx);
        cx.notify();
        self.request = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if !this.alive || version != this.request_version {
                    return;
                }
                match result.and_then(decode::<HostDirectory>) {
                    Ok(directory) => {
                        this.pending_path = Some(directory.path.clone());
                        this.directory = Some(directory);
                    }
                    Err(reason) => this.error = strip_error(&reason),
                }
                this.loading = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// `open`: asks the host to open the folder as a project.
    pub fn open(&mut self, cx: &mut Context<Self>) {
        let path = self.path(cx);
        let Some(machine) = self.machine().cloned() else {
            return;
        };
        if path.trim().is_empty() || self.opening {
            return;
        }
        self.request_version += 1;
        let version = self.request_version;
        self.opening = true;
        self.loading = false;
        self.error.clear();
        let task = self.host.request(
            &machine.id,
            "projects.open",
            json!({ "cwd": path.trim() }),
            cx,
        );
        cx.notify();
        self.request = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if !this.alive || version != this.request_version {
                    return;
                }
                this.opening = false;
                match result.and_then(decode::<HostProject>) {
                    Ok(project) => {
                        let key = this
                            .host
                            .remember_project(&machine.environment_id, &project, cx);
                        cx.emit(AddRemoteProjectEvent::Open(key));
                    }
                    Err(reason) => this.error = strip_error(&reason),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// `cancel`: answers that arrive later are ignored.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.alive = false;
        self.request_version += 1;
        cx.emit(AddRemoteProjectEvent::Cancel);
    }

    fn open_connections(&mut self, cx: &mut Context<Self>) {
        self.cancel(cx);
        cx.emit(AddRemoteProjectEvent::OpenConnections);
    }

    fn sync_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.machines.len() < 2 {
            self.select = None;
            return;
        }
        let value = self
            .machine()
            .map(|machine| machine.id.clone())
            .unwrap_or_default();
        if let Some(select) = &self.select {
            select.update(cx, |select, cx| select.set_value(value, cx));
            return;
        }
        let options = self
            .machines
            .iter()
            .map(|entry| {
                SearchableSelectOption::new(entry.id.clone(), entry.name.clone()).keywords(
                    entry
                        .ssh
                        .as_ref()
                        .map(|ssh| ssh.target.clone())
                        .unwrap_or_else(|| entry.endpoint.clone()),
                )
            })
            .collect();
        let layer = Theme::of(cx).layer.dialog_popover;
        let this = cx.weak_entity();
        self.select = Some(cx.new(|cx| {
            SearchableSelect::new("Machine", value, options, window, cx)
                .searchable(false)
                .layer(layer)
                .on_change(move |id, _, cx| {
                    let id = id.to_string();
                    this.update(cx, |this, cx| this.set_machine(&id, cx)).ok();
                })
        }));
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

impl Render for AddRemoteProjectDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(path) = self.pending_path.take() {
            self.path
                .update(cx, |state, cx| state.set_value(path, window, cx));
        }
        if !self.focused_once {
            self.focused_once = true;
            window.focus(&self.focus, cx);
        }
        self.sync_select(window, cx);
        let theme = Theme::of(cx).clone();
        let c = theme.colors;

        let mut panel = div()
            .id("add-remote-project")
            .relative()
            .flex()
            .flex_col()
            .gap(u(12.))
            .w(u(480.))
            .max_w_full()
            .max_h(relative(0.7))
            .p(u(16.))
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .shadow_xl()
            .leading(theme.leading.normal)
            .text_color(c.content)
            .debug_selector(|| "dialog:Open folder on a machine".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(u(4.))
                    .child(
                        div()
                            .text_px(theme.text.body)
                            .medium()
                            .leading(theme.leading.tight)
                            .child("Open folder on a machine"),
                    )
                    .child(
                        div()
                            .text_px(theme.text.label)
                            .leading(theme.leading.snug)
                            .text_color(theme.content(0.55))
                            .child(
                                "Sessions in this project run on that machine, using its checkout and its Codex or Claude Code sign-in. They keep running when you close MonoCode here.",
                            ),
                    ),
            );
        if self.loaded {
            panel = match self.machine().cloned() {
                None => panel.child(self.render_no_machine(cx)),
                Some(machine) => panel.child(self.render_browser(&machine, window, cx)),
            };
        }

        let overlay = with_alpha(c.modal_overlay, 0.3);
        deferred(
            div()
                .id("add-remote-project-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .track_focus(&self.focus)
                .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.cancel(cx);
                    }
                }))
                .child(
                    div()
                        .id("add-remote-project-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(overlay)
                        .debug_selector(|| "dialog-backdrop".into())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.cancel(cx)),
                        ),
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
                        .child(div().flex_none().h(relative(0.16)))
                        .child(panel),
                ),
        )
        .with_priority(theme.layer.dialog)
    }
}

impl AddRemoteProjectDialog {
    fn render_no_machine(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        div()
            .relative()
            .flex()
            .flex_col()
            .gap(u(12.))
            .child(
                div()
                    .text_px(theme.text.label)
                    .leading(theme.leading.snug)
                    .text_color(theme.content(0.55))
                    .debug_selector(|| "no-machines".into())
                    .child("No machines are connected yet. Add one in Settings, then open a folder on it here."),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(u(8.))
                    .child(
                        action_button("dialog-cancel", "Cancel")
                            .kind(ButtonKind::Ghost)
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                    )
                    .child(
                        action_button("dialog-add-machine", "Add a machine")
                            .small()
                            .radius(theme.radius.md)
                            .on_click(cx.listener(|this, _, _, cx| this.open_connections(cx))),
                    ),
            )
    }

    fn render_browser(
        &self,
        machine: &RemoteMachine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let chooser = match &self.select {
            Some(select) => div().child(select.clone()),
            None => div()
                .text_px(theme.text.label)
                .text_color(theme.content(0.55))
                .child(tinted_text([
                    (SharedString::from("On "), None),
                    (
                        SharedString::from(machine.name.clone()),
                        Some(theme.content(0.80)),
                    ),
                ])),
        };

        let mut rows = div().flex().flex_col().p(u(4.));
        if let Some(directory) = &self.directory {
            if let Some(parent) = directory.parent.clone() {
                rows = rows.child(self.folder_row("..", parent, cx));
            }
            for entry in &directory.entries {
                rows = rows.child(self.folder_row(&entry.name, entry.path.clone(), cx));
            }
            if directory.entries.is_empty() {
                rows = rows.child(self.folder_note("No subfolders", cx));
            }
        } else if self.loading {
            rows = rows.child(self.folder_note("Loading folders…", cx));
        }
        let folders = div()
            .id("folders")
            .min_h(u(96.))
            .flex_1()
            .overflow_y_scroll()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .debug_selector(|| "Folders".into())
            .child(rows);

        let path_empty = self.path(cx).trim().is_empty();
        let mut body = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(u(12.))
            .child(chooser)
            .child(text_input(
                "input:Folder path on the machine",
                &self.path,
                FieldStyle::path(),
                window,
                cx,
            ))
            .child(folders);
        if !self.error.is_empty() {
            body = body.child(
                div()
                    .text_px(theme.text.label)
                    .leading(theme.leading.snug)
                    .text_color(with_alpha(theme.colors.danger, 0.9))
                    .debug_selector(|| "dialog-error".into())
                    .child(self.error.clone()),
            );
        }
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(
                    action_button("dialog-cancel", "Cancel")
                        .kind(ButtonKind::Ghost)
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                )
                .child(
                    action_button(
                        "dialog-open",
                        if self.opening { "Opening…" } else { "Open" },
                    )
                    .selector("button:Open")
                    .small()
                    .radius(theme.radius.md)
                    .disabled(self.opening || path_empty)
                    .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                ),
        )
    }

    /// `FolderRow`.
    fn folder_row(&self, name: &str, path: String, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let hover_fill = theme.content(0.08);
        let hover_ink = theme.colors.content;
        let selector = format!("folder:{name}");
        div()
            .id(SharedString::from(format!("folder-{path}")))
            .flex()
            .w_full()
            .items_center()
            .gap(u(8.))
            .rounded(u(theme.radius.md))
            .px(u(8.))
            .py(u(6.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.75))
            .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
            .debug_selector(move || selector)
            .on_click(cx.listener(move |this, _, _, cx| this.browse(Some(path.clone()), cx)))
            .child(
                icon(IconName::Folder)
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
            )
            .child(div().min_w_0().flex_1().truncate().child(name.to_string()))
            .child(
                icon(IconName::ChevronRight)
                    .size(u(12.))
                    .text_color(theme.content(0.30)),
            )
    }

    fn folder_note(&self, text: &'static str, cx: &App) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .px(u(8.))
            .py(u(6.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.45))
            .child(text)
    }
}
