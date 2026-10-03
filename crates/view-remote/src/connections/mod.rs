//! Port of src/features/connections/ui/ConnectionsSettings.tsx: the
//! Connections page in Settings. It lists the paired machines with their
//! route and status, pairs a machine from a `monocode://pair` link, runs the
//! SSH setup with its host trust and password prompts, and retries, updates,
//! reconnects, and removes machines.
//!
//! The page drives its own checks the way the React effects did: every
//! machine is described now and every 10 seconds while no setup runs, and a
//! running SSH job is polled every 350 ms. Everything native goes through
//! [`RemoteHost`].

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_remote::host::protocol::{RemoteMachine, SshSetup, host_connect_command};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::host::{RemoteHost, SshBegin};
use crate::machines::{
    MachineStatus, connected_notice, describe_params, machine_status, route_label,
};
use crate::style::{
    ButtonKind, FieldStyle, action_button, code, code_text, field_label, help, spinning_loader,
    text, text_input,
};

#[cfg(test)]
mod tests;

/// How often each machine is described while the page is open.
const CHECK_INTERVAL: Duration = Duration::from_secs(10);
/// How often a running SSH setup job is read.
const POLL_INTERVAL: Duration = Duration::from_millis(350);
/// How long Copy reads "Copied".
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// The two ways to add a machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddMode {
    #[default]
    Link,
    Ssh,
}

/// The page's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Link,
    Name,
    Target,
    Port,
    /// The SSH password or passphrase prompt.
    Answer,
}

/// Notifications for the owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionsEvent {
    /// `refreshRemoteMachines`: the saved machine list changed.
    MachinesChanged,
}

/// What the status effect last ran with, standing in for its React deps.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CheckKey {
    machines: u64,
    busy: bool,
    version: Option<String>,
    checks: u64,
}

pub struct ConnectionsSettings {
    host: Rc<dyn RemoteHost>,
    machines: Vec<RemoteMachine>,
    loaded: bool,
    /// Bumped on every machine list read, like a new React array.
    machines_revision: u64,
    adding: bool,
    mode: AddMode,
    link: Entity<InputState>,
    name: Entity<InputState>,
    target: Entity<InputState>,
    port: Entity<InputState>,
    answer: Entity<InputState>,
    advanced_open: bool,
    version: Option<String>,
    copied: bool,
    job_id: Option<String>,
    job: Option<SshSetup>,
    busy: bool,
    error: String,
    notice: String,
    answering: bool,
    status: HashMap<String, MachineStatus>,
    updating_machine: Option<String>,
    removing: Option<String>,
    revoking: bool,
    checks: u64,
    /// The job to cancel when the page closes.
    current_job: Option<String>,
    submitting: bool,
    check_key: Option<CheckKey>,
    poll_key: Option<(Option<String>, Option<String>)>,
    prompt_key: Option<String>,
    /// Fields to empty and to focus on the next frame, which has a window.
    clear: Vec<Field>,
    focus: Option<Field>,
    check_task: Option<Task<()>>,
    poll_task: Option<Task<()>>,
    copied_task: Option<Task<()>>,
    machines_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ConnectionsEvent> for ConnectionsSettings {}

impl ConnectionsSettings {
    pub fn new(host: Rc<dyn RemoteHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let link = input("monocode://pair?...", window, cx);
        let name = input("Optional, e.g. Home Mac mini", window, cx);
        let target = input("user@my-mac-mini or an SSH alias", window, cx);
        let port = input("From SSH config", window, cx);
        let answer = cx.new(|cx| InputState::new(window, cx).masked(true));
        let mut subscriptions = Vec::new();
        for (state, field) in [
            (&link, Field::Link),
            (&name, Field::Name),
            (&target, Field::Target),
            (&port, Field::Port),
            (&answer, Field::Answer),
        ] {
            subscriptions.push(cx.subscribe(state, move |this, _, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.press_enter(field, cx);
                } else if let InputEvent::Change = event {
                    cx.notify();
                }
            }));
        }
        cx.on_release(|this: &mut Self, cx| {
            if let Some(job) = this.current_job.take() {
                this.host.ssh_cancel(&job, cx).detach();
            }
        })
        .detach();
        let mut this = Self {
            host,
            machines: Vec::new(),
            loaded: false,
            machines_revision: 0,
            adding: false,
            mode: AddMode::Link,
            link,
            name,
            target,
            port,
            answer,
            advanced_open: false,
            version: None,
            copied: false,
            job_id: None,
            job: None,
            busy: false,
            error: String::new(),
            notice: String::new(),
            answering: false,
            status: HashMap::new(),
            updating_machine: None,
            removing: None,
            revoking: false,
            checks: 0,
            current_job: None,
            submitting: false,
            check_key: None,
            poll_key: None,
            prompt_key: None,
            clear: Vec::new(),
            focus: None,
            check_task: None,
            poll_task: None,
            copied_task: None,
            machines_task: None,
            _subscriptions: subscriptions,
        };
        let version = this.host.app_version(cx);
        cx.spawn(async move |this, cx| {
            if let Ok(version) = version.await {
                this.update(cx, |this, cx| {
                    this.version = Some(version);
                    this.effects(cx);
                })
                .ok();
            }
        })
        .detach();
        this.refresh_machines(cx);
        this.effects(cx);
        this
    }

    // Reading state, for the owner and the tests.

    pub fn machines(&self) -> &[RemoteMachine] {
        &self.machines
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn is_adding(&self) -> bool {
        self.adding
    }

    pub fn mode(&self) -> AddMode {
        self.mode
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn notice(&self) -> &str {
        &self.notice
    }

    pub fn job(&self) -> Option<&SshSetup> {
        self.job.as_ref()
    }

    pub fn job_id(&self) -> Option<&str> {
        self.job_id.as_deref()
    }

    /// The machine whose removal is being confirmed.
    pub fn removing(&self) -> Option<&str> {
        self.removing.as_deref()
    }

    pub fn status(&self, machine_id: &str) -> Option<&MachineStatus> {
        self.status.get(machine_id)
    }

    /// The status line under a machine.
    pub fn status_label(&self, machine_id: &str) -> String {
        self.status
            .get(machine_id)
            .map(|status| status.label.clone())
            .unwrap_or_else(|| MachineStatus::checking().label)
    }

    /// The command that installs or updates a host for this desktop.
    pub fn command(&self) -> String {
        host_connect_command(self.version.as_deref())
    }

    pub fn value(&self, field: Field, cx: &App) -> String {
        self.input(field).read(cx).value().to_string()
    }

    /// `upgradeBlocked`: setup stopped at an older host with running turns
    /// until the user agrees to restart it.
    pub fn upgrade_blocked(&self) -> bool {
        self.error.contains("--yes")
    }

    /// Fills a field, for example the pairing link from a `monocode://pair`
    /// deep link.
    pub fn set_value(
        &mut self,
        field: Field,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.to_string();
        self.input(field)
            .clone()
            .update(cx, |state, cx| state.set_value(value, window, cx));
        cx.notify();
    }

    fn input(&self, field: Field) -> &Entity<InputState> {
        match field {
            Field::Link => &self.link,
            Field::Name => &self.name,
            Field::Target => &self.target,
            Field::Port => &self.port,
            Field::Answer => &self.answer,
        }
    }

    /// Reads the machine list again. The owner calls this when the engine's
    /// list changes; the page calls it after pairing, setup, and removal.
    pub fn refresh_machines(&mut self, cx: &mut Context<Self>) {
        let task = self.host.machines(cx);
        self.machines_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                // A temporary failure keeps the last list instead of
                // blanking the page.
                if let Ok(machines) = result {
                    this.machines = machines;
                }
                this.loaded = true;
                this.machines_revision += 1;
                this.effects(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Restarts the checks and the job poll when what they depend on
    /// changed, as the React effects did.
    fn effects(&mut self, cx: &mut Context<Self>) {
        let key = CheckKey {
            machines: self.machines_revision,
            busy: self.busy,
            version: self.version.clone(),
            checks: self.checks,
        };
        if self.check_key.as_ref() != Some(&key) {
            self.check_key = Some(key);
            self.restart_checks(cx);
        }
        let poll = (self.job_id.clone(), self.updating_machine.clone());
        if self.poll_key.as_ref() != Some(&poll) {
            self.poll_key = Some(poll);
            self.restart_poll(cx);
        }
        let prompt = self
            .job
            .as_ref()
            .and_then(|job| job.prompt.as_ref())
            .map(|prompt| prompt.id.clone());
        if prompt != self.prompt_key {
            let shows_field = self
                .job
                .as_ref()
                .and_then(|job| job.prompt.as_ref())
                .is_some_and(|prompt| !prompt.confirm);
            self.prompt_key = prompt;
            self.clear.push(Field::Answer);
            self.answering = false;
            if shows_field {
                self.focus = Some(Field::Answer);
            }
        }
        cx.notify();
    }

    fn restart_checks(&mut self, cx: &mut Context<Self>) {
        let busy = self.busy;
        let version = self.version.clone();
        let host = self.host.clone();
        let machines = self.machines.clone();
        self.check_task = Some(cx.spawn(async move |this, cx| {
            loop {
                if !busy {
                    // Start every request before awaiting any, so slow
                    // machines are checked together.
                    let requests: Vec<_> = cx.update(|cx| {
                        machines
                            .iter()
                            .map(|machine| {
                                host.request(
                                    &machine.id,
                                    "environment.describe",
                                    describe_params(),
                                    cx,
                                )
                            })
                            .collect()
                    });
                    for (machine, request) in machines.iter().zip(requests) {
                        let next = machine_status(machine, request.await, version.as_deref());
                        let alive = this
                            .update(cx, |this, cx| {
                                this.status.insert(machine.id.clone(), next);
                                cx.notify();
                            })
                            .is_ok();
                        if !alive {
                            return;
                        }
                    }
                }
                cx.background_executor().timer(CHECK_INTERVAL).await;
            }
        }));
    }

    fn restart_poll(&mut self, cx: &mut Context<Self>) {
        let Some(job_id) = self.job_id.clone() else {
            self.poll_task = None;
            return;
        };
        let host = self.host.clone();
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let poll = cx.update(|cx| host.ssh_poll(&job_id, cx));
                let result = poll.await;
                let more = this
                    .update(cx, |this, cx| this.polled(&job_id, result, cx))
                    .unwrap_or(false);
                if !more {
                    return;
                }
                cx.background_executor().timer(POLL_INTERVAL).await;
            }
        }));
    }

    /// One poll answer. Returns whether to keep polling.
    fn polled(
        &mut self,
        job_id: &str,
        result: Result<SshSetup, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        match result {
            Ok(next) => {
                let done = next.done;
                let error = next.error.clone();
                let machine = next.machine.clone();
                self.job = Some(next);
                if done {
                    self.current_job = None;
                    self.submitting = false;
                    self.busy = false;
                    self.job_id = None;
                    self.clear.push(Field::Answer);
                    if let Some(error) = error {
                        self.error = error;
                    } else if let Some(machine) = machine {
                        self.adding = false;
                        self.clear.extend([Field::Target, Field::Name, Field::Port]);
                        self.notice = if self.updating_machine.is_some() {
                            format!("{} was updated and reconnected.", machine.name)
                        } else {
                            connected_notice(&machine)
                        };
                        self.updating_machine = None;
                        self.checks += 1;
                        self.machines_changed(cx);
                    }
                }
                self.effects(cx);
                !done
            }
            Err(reason) => {
                self.error = reason;
                self.host.ssh_cancel(job_id, cx).detach();
                self.current_job = None;
                self.submitting = false;
                self.busy = false;
                self.job_id = None;
                self.effects(cx);
                false
            }
        }
    }

    /// `refreshRemoteMachines`.
    fn machines_changed(&mut self, cx: &mut Context<Self>) {
        self.host.machines_changed(cx);
        cx.emit(ConnectionsEvent::MachinesChanged);
        self.refresh_machines(cx);
    }

    fn press_enter(&mut self, field: Field, cx: &mut Context<Self>) {
        match field {
            Field::Answer => {
                let confirm = self
                    .job
                    .as_ref()
                    .and_then(|job| job.prompt.as_ref())
                    .is_some_and(|prompt| prompt.confirm);
                let value = if confirm {
                    "yes".to_string()
                } else {
                    self.value(Field::Answer, cx)
                };
                self.respond(value, cx);
            }
            Field::Link => self.pair(cx),
            Field::Name if self.mode == AddMode::Link => self.pair(cx),
            Field::Name | Field::Target | Field::Port => {
                // The address is a required field.
                if !self.value(Field::Target, cx).is_empty() && !self.busy {
                    self.begin(None, false, cx);
                }
            }
        }
    }

    /// Opens the add form.
    pub fn start_adding(&mut self, cx: &mut Context<Self>) {
        self.adding = true;
        self.error.clear();
        self.notice.clear();
        self.focus = Some(match self.mode {
            AddMode::Link => Field::Link,
            AddMode::Ssh => Field::Target,
        });
        cx.notify();
    }

    pub fn set_mode(&mut self, mode: AddMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.error.clear();
        self.focus = Some(match mode {
            AddMode::Link => Field::Link,
            AddMode::Ssh => Field::Target,
        });
        cx.notify();
    }

    pub fn cancel_adding(&mut self, cx: &mut Context<Self>) {
        self.adding = false;
        cx.notify();
    }

    /// `begin`: SSH setup for a new machine, or `connect` again on a machine
    /// set up over SSH. `upgrade` installs this desktop's host version.
    pub fn begin(&mut self, machine: Option<RemoteMachine>, upgrade: bool, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        self.submitting = true;
        self.busy = true;
        self.error.clear();
        self.notice.clear();
        self.job = None;
        self.updating_machine = if upgrade {
            machine.as_ref().map(|machine| machine.id.clone())
        } else {
            None
        };
        let task = match &machine {
            Some(machine) => self.host.ssh_reconnect(&machine.id, upgrade, cx),
            None => {
                let port = self.value(Field::Port, cx);
                let port = port.trim();
                let port = if port.is_empty() {
                    Ok(None)
                } else {
                    port.parse::<u16>().map(Some).map_err(|_| {
                        "Enter an SSH hostname or alias, such as user@my-mac-mini, and a valid port."
                            .to_string()
                    })
                };
                match port {
                    Ok(port) => self.host.ssh_begin(
                        SshBegin {
                            target: self.value(Field::Target, cx).trim().to_string(),
                            name: self.value(Field::Name, cx).trim().to_string(),
                            port,
                            upgrade,
                        },
                        cx,
                    ),
                    Err(message) => Task::ready(Err(message)),
                }
            }
        };
        self.effects(cx);
        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let job = result.as_ref().ok().cloned();
            let alive = this
                .update(cx, |this, cx| {
                    match result {
                        Ok(id) => {
                            this.current_job = Some(id.clone());
                            this.job_id = Some(id);
                        }
                        Err(reason) => {
                            this.submitting = false;
                            this.error = reason;
                            this.busy = false;
                        }
                    }
                    this.effects(cx);
                })
                .is_ok();
            // The page closed while the job started: stop it.
            if !alive && let Some(job) = job {
                cx.update(|cx| host.ssh_cancel(&job, cx)).await.ok();
            }
        })
        .detach();
    }

    /// `pair`: pairs the machine in the pasted link.
    pub fn pair(&mut self, cx: &mut Context<Self>) {
        let link = self.value(Field::Link, cx);
        if self.busy || link.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.error.clear();
        self.notice.clear();
        let name = self.value(Field::Name, cx);
        let task = self.host.pair(link.trim(), name.trim(), cx);
        self.effects(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(machine) => {
                        this.clear.extend([Field::Link, Field::Name]);
                        this.adding = false;
                        this.notice = connected_notice(&machine);
                        this.checks += 1;
                        this.machines_changed(cx);
                    }
                    Err(reason) => this.error = reason,
                }
                this.busy = false;
                this.effects(cx);
            })
            .ok();
        })
        .detach();
    }

    /// `retry`: tries every route to the machine again, then checks it.
    pub fn retry(&mut self, machine: &RemoteMachine, cx: &mut Context<Self>) {
        self.error.clear();
        self.status
            .insert(machine.id.clone(), MachineStatus::checking());
        let task = self.host.retry(&machine.id, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            task.await.ok();
            this.update(cx, |this, cx| {
                this.checks += 1;
                this.effects(cx);
            })
            .ok();
        })
        .detach();
    }

    /// `respond`: answers the SSH job's current prompt.
    pub fn respond(&mut self, value: String, cx: &mut Context<Self>) {
        let (Some(job_id), Some(prompt)) = (
            self.job_id.clone(),
            self.job.as_ref().and_then(|job| job.prompt.clone()),
        ) else {
            return;
        };
        if self.answering {
            return;
        }
        self.answering = true;
        self.error.clear();
        let task = self.host.ssh_answer(&job_id, &prompt.id, value, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.clear.push(Field::Answer),
                    Err(reason) => {
                        this.error = reason;
                        this.answering = false;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `remove`: removes the saved connection, after revoking this
    /// desktop's credential on the host when `revoke` is set.
    pub fn remove(&mut self, machine: RemoteMachine, revoke: bool, cx: &mut Context<Self>) {
        self.error.clear();
        self.notice.clear();
        self.revoking = true;
        cx.notify();
        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                if revoke {
                    let request = cx.update(|cx| {
                        host.request(&machine.id, "devices.revokeSelf", serde_json::json!({}), cx)
                    });
                    if let Err(reason) = request.await {
                        return Err(format!(
                            "Could not revoke access, so {} was not removed: {reason}. Reconnect and try again, or remove it from this desktop only and revoke it on the host with monocode-host devices and monocode-host revoke <device-id>.",
                            machine.name
                        ));
                    }
                }
                cx.update(|cx| host.disconnect(&machine.id, cx)).await
            }
            .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.machines.retain(|entry| entry.id != machine.id);
                        this.removing = None;
                        this.notice = if revoke {
                            format!(
                                "{} was removed and this desktop's access was revoked. The host and its sessions keep running.",
                                machine.name
                            )
                        } else {
                            format!(
                                "{} was removed from this desktop. The host and its sessions keep running, and it still accepts this desktop's credential.",
                                machine.name
                            )
                        };
                        this.machines_changed(cx);
                    }
                    Err(reason) => this.error = reason,
                }
                this.revoking = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows the removal confirmation under a machine.
    pub fn confirm_remove(&mut self, machine_id: &str, cx: &mut Context<Self>) {
        self.error.clear();
        self.removing = Some(machine_id.to_string());
        cx.notify();
    }

    /// `copyCommand`.
    pub fn copy_command(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.command()));
        self.copied = true;
        cx.notify();
        self.copied_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Cancel connection.
    pub fn cancel_job(&mut self, cx: &mut Context<Self>) {
        let Some(job_id) = self.job_id.clone() else {
            return;
        };
        let task = self.host.ssh_cancel(&job_id, cx);
        cx.spawn(async move |this, cx| {
            if let Err(reason) = task.await {
                this.update(cx, |this, cx| {
                    this.error = reason;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Applies the field changes queued by async work, which has no window.
    fn apply_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for field in std::mem::take(&mut self.clear) {
            self.input(field)
                .clone()
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        if let Some(field) = self.focus.take() {
            self.input(field)
                .clone()
                .update(cx, |state, cx| state.focus(window, cx));
        }
        let busy = self.busy;
        let answering = self.answering;
        for (field, disabled) in [
            (Field::Link, busy),
            (Field::Name, busy),
            (Field::Target, busy),
            (Field::Port, busy),
            (Field::Answer, answering),
        ] {
            self.input(field)
                .clone()
                .update(cx, |state, cx| state.set_disabled(disabled, cx));
        }
    }
}

impl Render for ConnectionsSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_fields(window, cx);
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("remote-machines")
            .flex()
            .flex_col()
            .gap(u(20.))
            .w_full()
            .min_w_0()
            .leading(theme.leading.normal)
            .text_color(theme.colors.content)
            .child(self.render_header(cx));
        if !self.machines.is_empty() {
            root = root.child(self.render_machines(cx));
        } else if self.loaded && !self.adding {
            root = root.child(
                div()
                    .rounded(u(theme.radius.xl))
                    .border_1()
                    .border_dashed()
                    .border_color(theme.content(0.15))
                    .px(u(20.))
                    .py(u(32.))
                    .flex()
                    .justify_center()
                    .text_px(theme.text.body)
                    .text_color(theme.content(0.45))
                    .debug_selector(|| "machines-empty".into())
                    .child("Add your always-on Windows, Mac, or Linux machine to get started."),
            );
        }
        if self.adding {
            root = root.child(self.render_add(window, cx));
        }
        if self.busy && self.job_id.is_some() {
            root = root.child(self.render_progress(window, cx));
        }
        if !self.error.is_empty() {
            root = root.child(self.render_error(cx));
        }
        if !self.notice.is_empty() {
            root = root.child(
                div()
                    .text_px(theme.text.body)
                    .text_color(theme.colors.success)
                    .debug_selector(|| "connections-notice".into())
                    .child(self.notice.clone()),
            );
        }
        root
    }
}

impl ConnectionsSettings {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(u(16.))
            .child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .text_px(theme.text.body)
                            .semibold()
                            .text_color(theme.colors.content)
                            .child("Your machines"),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .text_px(theme.text.label)
                            .leading(theme.leading.relaxed)
                            .text_color(theme.content(0.45))
                            .child(
                                "Run agents on another computer and return to them from your laptop. The host keeps working when you close MonoCode here.",
                            ),
                    ),
            )
            .when(!self.adding, |header| {
                header.child(
                    action_button("add-machine", "Add machine")
                        .icon(IconName::Plus)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.start_adding(cx))),
                )
            })
    }

    fn render_machines(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut list = div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.colors.stroke);
        for (index, machine) in self.machines.iter().enumerate() {
            let mut entry = div().flex().flex_col();
            if index > 0 {
                entry = entry.border_t_1().border_color(theme.colors.stroke);
            }
            entry = entry.child(self.render_machine(machine, cx));
            if self.removing.as_deref() == Some(&machine.id) {
                entry = entry.child(self.render_removal(machine, cx));
            }
            list = list.child(entry);
        }
        list
    }

    fn render_machine(&self, machine: &RemoteMachine, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let state = self.status.get(&machine.id);
        let update = state.is_some_and(|state| state.needs_update(self.version.as_deref()));
        let label: SharedString = state
            .map(|state| state.label.clone())
            .unwrap_or_else(|| MachineStatus::checking().label)
            .into();
        let id = machine.id.clone();
        let mut details = div()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .truncate()
                    .text_px(theme.text.body)
                    .medium()
                    .child(machine.name.clone()),
            )
            .child(
                div()
                    .mt(u(4.))
                    .truncate()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("route:{id}")
                    })
                    .child(route_label(machine)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("status-{id}")))
                    .mt(u(4.))
                    .line_clamp(3)
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .tooltip(tooltip(label.clone()))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("status:{id}")
                    })
                    .child(label),
            );
        if update {
            let note: AnyElement = if machine.ssh.is_some() {
                "Updating restarts the host and interrupts active agent turns.".into_any_element()
            } else {
                code_text([
                    text("To update, run "),
                    code(self.command()),
                    text(
                        " on the machine. It restarts the host and interrupts active agent turns.",
                    ),
                ])
                .into_any_element()
            };
            details = details.child(
                div()
                    .mt(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .child(note),
            );
        }
        let mut actions = div().flex().flex_none().items_center().gap(u(8.));
        if machine.ssh.is_some() && update {
            let target = machine.clone();
            actions = actions.child(
                action_button(SharedString::from(format!("update-{id}")), "Update Host")
                    .disabled(self.busy)
                    .tooltip("Installs this desktop's host version over SSH and restarts the host; active agent turns will be interrupted")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.begin(Some(target.clone()), true, cx)
                    })),
            );
        }
        if machine.ssh.is_some() {
            let target = machine.clone();
            actions = actions.child(
                action_button(SharedString::from(format!("reconnect-{id}")), "Reconnect")
                    .disabled(self.busy)
                    .tooltip("Starts the host over SSH if it stopped, then reconnects")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.begin(Some(target.clone()), false, cx)
                    })),
            );
        } else if state.is_some_and(|state| state.offline) {
            let target = machine.clone();
            actions = actions.child(
                action_button(SharedString::from(format!("retry-{id}")), "Retry")
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, _, cx| this.retry(&target, cx))),
            );
        }
        let remove_disabled = self.busy || self.revoking;
        let ink = theme.content(0.40);
        let hover_ink = theme.colors.content;
        let hover_fill = theme.colors.selection;
        let name = machine.name.clone();
        let mut remove = div()
            .id(SharedString::from(format!("remove-{id}")))
            .flex_none()
            .p(u(8.))
            .rounded(u(theme.radius.sm))
            .group("remove-machine")
            .tooltip(tooltip("Remove connection…"))
            .debug_selector(move || format!("Remove {name}"))
            .child(
                icon(IconName::Trash2)
                    .size(u(16.))
                    .text_color(ink)
                    .when(!remove_disabled, |glyph| {
                        glyph.group_hover("remove-machine", move |s| s.text_color(hover_ink))
                    }),
            );
        if remove_disabled {
            remove = remove.opacity(0.4);
        } else {
            let target = id.clone();
            remove = remove
                .hover(move |s| s.bg(hover_fill))
                .on_click(cx.listener(move |this, _, _, cx| this.confirm_remove(&target, cx)));
        }
        div()
            .flex()
            .items_center()
            .gap(u(12.))
            .p(u(16.))
            .child(
                icon(IconName::Internet)
                    .size(u(20.))
                    .text_color(theme.content(0.45)),
            )
            .child(details)
            .child(actions)
            .child(remove)
    }

    fn render_removal(&self, machine: &RemoteMachine, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let name = machine.name.clone();
        let revoke_target = machine.clone();
        let remove_target = machine.clone();
        div()
            .flex()
            .flex_col()
            .gap(u(12.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .bg(theme.content(0.03))
            .p(u(16.))
            .text_px(theme.text.label)
            .leading(theme.leading.relaxed)
            .text_color(theme.content(0.60))
            .debug_selector(move || format!("Confirm removing {name}"))
            .child(
                div()
                    .text_px(theme.text.body)
                    .medium()
                    .text_color(theme.colors.content)
                    .child(format!("Remove {} from this desktop?", machine.name)),
            )
            .child(
                "This closes this desktop’s connection to the machine. It does not stop the host, and its sessions keep running and stay on that machine. You can pair it again later.",
            )
            .child(
                "Removing alone leaves this desktop’s credential valid on the host. Revoke access to invalidate it first; the machine must be reachable.",
            )
            .child(code_text([
                text("To stop the host and turn off its background service, run "),
                code("~/.monocode-host/bin/monocode-host service uninstall"),
                text(" on that machine ("),
                code("%USERPROFILE%\\.monocode-host\\bin\\monocode-host.cmd service uninstall"),
                text(" on Windows). Its sessions and history are kept."),
            ]))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(u(8.))
                    .child(
                        action_button("revoke-and-remove", "Revoke access and remove")
                            .disabled(self.revoking)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove(revoke_target.clone(), true, cx)
                            })),
                    )
                    .child(
                        action_button("remove-only", "Remove from this desktop only")
                            .disabled(self.revoking)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove(remove_target.clone(), false, cx)
                            })),
                    )
                    .child(
                        action_button("cancel-remove", "Cancel")
                            .kind(ButtonKind::Quiet)
                            .selector("button:Cancel remove")
                            .disabled(self.revoking)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.removing = None;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_add(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let mut tabs = div()
            .flex()
            .flex_none()
            .rounded(u(theme.radius.lg))
            .bg(theme.content(0.05))
            .p(u(2.))
            .text_px(theme.text.label);
        for (mode, label) in [(AddMode::Link, "Pairing link"), (AddMode::Ssh, "SSH")] {
            let selected = self.mode == mode;
            let mut tab = div()
                .id(SharedString::from(format!("tab-{label}")))
                .rounded(u(theme.radius.md))
                .px(u(12.))
                .py(u(4.))
                .debug_selector(move || format!("tab:{label}"))
                .child(label);
            tab = if selected {
                tab.bg(c.selection).medium().text_color(c.content)
            } else {
                tab.text_color(theme.content(0.55))
            };
            if self.busy {
                tab = tab.opacity(0.4);
            } else {
                tab = tab.on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)));
            }
            tabs = tabs.child(tab);
        }
        let form = match self.mode {
            AddMode::Link => self.render_link_form(window, cx).into_any_element(),
            AddMode::Ssh => self.render_ssh_form(window, cx).into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(c.stroke)
            .p(u(20.))
            .debug_selector(|| "add-machine-form".into())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .child(div().text_px(theme.text.ui).medium().child("Add a machine"))
                    .child(tabs),
            )
            .child(form)
    }

    fn name_field(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        field_label(
            "Name",
            text_input("input:Name", &self.name, FieldStyle::settings(), window, cx),
            cx,
        )
    }

    fn form_buttons(&self, submit: AnyElement, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .justify_end()
            .gap(u(8.))
            .child(
                action_button("cancel-add", "Cancel")
                    .kind(ButtonKind::Quiet)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_adding(cx))),
            )
            .child(submit)
    }

    fn render_link_form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let command = self.command();
        let link_empty = self.value(Field::Link, cx).trim().is_empty();
        let step_one = div()
            .flex()
            .flex_col()
            .gap(u(6.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.65))
            .child("1. On the machine, run")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .rounded(u(theme.radius.lg))
                            .bg(theme.content(0.05))
                            .px(u(12.))
                            .py(u(8.))
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.label)
                            .text_color(theme.colors.content)
                            .debug_selector(|| "connect-command".into())
                            .child(command),
                    )
                    .child(
                        action_button("copy-command", if self.copied { "Copied" } else { "Copy" })
                            .selector("button:Copy")
                            .on_click(cx.listener(|this, _, _, cx| this.copy_command(cx))),
                    ),
            )
            .child(div().text_color(theme.content(0.45)).child(
                "It installs MonoCode Host as a background service, listens on port 3774 over TLS, and prints a one-time pairing link. It needs Node.js 22.13 or newer.",
            ));
        let submit = action_button("pair", if self.busy { "Pairing…" } else { "Pair" })
            .selector("button:Pair")
            .disabled(self.busy || link_empty)
            .on_click(cx.listener(|this, _, _, cx| this.pair(cx)))
            .into_any_element();
        div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .child(step_one)
            .child(field_label(
                "2. Paste the pairing link",
                text_input(
                    "input:Pairing link",
                    &self.link,
                    FieldStyle::settings().mono(),
                    window,
                    cx,
                ),
                cx,
            ))
            .child(self.name_field(window, cx))
            .child(help(
                "The link works once, for 15 minutes, and pins the host’s certificate. This computer must reach one of the machine’s addresses, such as on the same network or tailnet. Otherwise, use SSH.",
                cx,
            ))
            .child(requirements(cx))
            .child(self.form_buttons(submit, cx))
    }

    fn render_ssh_form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let target_empty = self.value(Field::Target, cx).trim().is_empty();
        let chevron = if self.advanced_open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let mut advanced = div()
            .flex()
            .flex_col()
            .text_px(theme.text.label)
            .text_color(theme.content(0.50))
            .child(
                div()
                    .id("ssh-advanced")
                    .flex()
                    .items_center()
                    .gap(u(4.))
                    .debug_selector(|| "Advanced".into())
                    .child(icon(chevron).size(u(12.)).text_color(theme.content(0.50)))
                    .child("Advanced")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced_open = !this.advanced_open;
                        cx.notify();
                    })),
            );
        if self.advanced_open {
            advanced = advanced.child(div().mt(u(12.)).max_w(u(160.)).child(field_label(
                "SSH port",
                text_input(
                    "input:SSH port",
                    &self.port,
                    FieldStyle::settings(),
                    window,
                    cx,
                ),
                cx,
            )));
        }
        let submit = action_button(
            "set-up-ssh",
            if self.busy {
                "Setting up…"
            } else {
                "Set up over SSH"
            },
        )
        .selector("button:Set up over SSH")
        .disabled(self.busy || target_empty)
        .on_click(cx.listener(|this, _, _, cx| this.begin(None, false, cx)))
        .into_any_element();
        div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .child(field_label(
                "SSH address",
                text_input(
                    "input:SSH address",
                    &self.target,
                    FieldStyle::settings(),
                    window,
                    cx,
                ),
                cx,
            ))
            .child(self.name_field(window, cx))
            .child(advanced)
            .child(help(
                code_text([
                    text("MonoCode runs "),
                    code(self.command()),
                    text(" on the machine over SSH and pairs this desktop. Your SSH keys and config are used automatically. When this computer can’t reach the machine’s network addresses, MonoCode connects through an SSH forward instead."),
                ]),
                cx,
            ))
            .child(requirements(cx))
            .child(self.form_buttons(submit, cx))
    }

    fn render_progress(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let message = self
            .job
            .as_ref()
            .map(|job| job.message.clone())
            .unwrap_or_else(|| "Starting connection…".into());
        let mut card = div()
            .flex()
            .flex_col()
            .gap(u(12.))
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.colors.stroke)
            .p(u(20.))
            .debug_selector(|| "ssh-progress".into())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .text_px(theme.text.body)
                    .child(spinning_loader(
                        "ssh-progress-spinner",
                        16.,
                        theme.colors.content,
                    ))
                    .child(message),
            );
        if let Some(prompt) = self.job.as_ref().and_then(|job| job.prompt.clone()) {
            let mut form = div().flex().flex_col().gap(u(12.)).child(
                div()
                    .text_px(theme.text.label)
                    .leading(theme.leading.relaxed)
                    .text_color(theme.content(0.70))
                    .child(prompt.message.clone()),
            );
            if !prompt.confirm {
                form = form.child(text_input(
                    "input:SSH password or passphrase",
                    &self.answer,
                    FieldStyle::settings(),
                    window,
                    cx,
                ));
            }
            let confirm = prompt.confirm;
            let mut buttons = div().flex().gap(u(8.)).child(
                action_button(
                    "ssh-continue",
                    if confirm {
                        "Trust host and continue"
                    } else {
                        "Continue"
                    },
                )
                .disabled(self.answering)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let value = if confirm {
                        "yes".to_string()
                    } else {
                        this.value(Field::Answer, cx)
                    };
                    this.respond(value, cx)
                })),
            );
            if confirm {
                buttons = buttons.child(
                    action_button("ssh-reject", "Reject")
                        .disabled(self.answering)
                        .on_click(cx.listener(|this, _, _, cx| this.respond("no".into(), cx))),
                );
            }
            card = card.child(form.child(buttons));
        }
        let quiet = theme.content(0.50);
        let ink = theme.colors.content;
        card.child(
            div().flex().child(
                div()
                    .id("cancel-connection")
                    .text_px(theme.text.label)
                    .text_color(quiet)
                    .hover(move |s| s.text_color(ink))
                    .debug_selector(|| "button:Cancel connection".into())
                    .child("Cancel connection")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_job(cx))),
            ),
        )
    }

    fn render_error(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let mut alert = div()
            .flex()
            .flex_col()
            .gap(u(8.))
            .rounded(u(theme.radius.lg))
            .bg(monocode_ui::color::with_alpha(c.danger_fill, 0.05))
            .p(u(12.))
            .text_px(theme.text.label)
            .leading(theme.leading.relaxed)
            .text_color(c.danger)
            .debug_selector(|| "connections-error".into())
            .child(self.error.clone());
        if self.upgrade_blocked() && self.adding && self.mode == AddMode::Ssh {
            alert = alert.child(
                div().flex().child(
                    action_button("upgrade-host", "Update and restart the host")
                        .ink(c.content)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.begin(None, true, cx))),
                ),
            );
        }
        alert
    }
}

/// `Requirements`.
fn requirements(cx: &App) -> AnyElement {
    help(
        code_text([
            text(
                "Sign in to Codex or Claude Code on the machine as the same user. On Linux, the host runs as a systemd user service and setup turns on lingering for your account (",
            ),
            code("loginctl enable-linger"),
            text(
                "), so agents keep running after you log out. On Windows and Mac, keep the machine’s desktop account signed in and the machine awake. Locking the desktop is fine.",
            ),
        ]),
        cx,
    )
}
