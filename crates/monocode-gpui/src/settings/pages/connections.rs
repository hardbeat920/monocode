//! ConnectionsSettings.tsx: saved hosts, SSH setup, reconnect and credential revocation.
use crate::{
    input::{self, Input},
    popover::AnchorSlot,
    settings::{
        Section, SettingsView,
        widgets::{red, secondary_button},
    },
    theme::*,
};
use gpui::{
    AnyElement, Context, Div, Focusable, FontWeight, Role, SharedString, Task, Window, div,
    prelude::*, px, rgb,
};
use monocode_core::remote::{Connections, JobView, Machine};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

const TARGET: &str = "SSH target";
const NAME: &str = "Machine name";
const PORT: &str = "SSH port";
const URL: &str = "Host URL";
const TOKEN: &str = "Host token";
const ANSWER: &str = "SSH password or passphrase";

#[derive(Default)]
pub(in crate::settings) struct ConnectionsState {
    adding: bool,
    url: bool,
    advanced: bool,
    initialized: bool,
    loaded: bool,
    machines: Vec<Machine>,
    status: HashMap<String, String>,
    needs_update: HashSet<String>,
    busy: bool,
    error: String,
    notice: String,
    updating: Option<String>,
    removing: Option<String>,
    revoking: bool,
    job: Option<JobView>,
    lease: Option<JobLease>,
    answering: bool,
    generation: u64,
    poll: Option<Task<()>>,
    timer: Option<Task<()>>,
    refresh: Option<Task<()>>,
    progress: AnchorSlot,
    reveal_prompt: bool,
}

/// Dropping Settings while an SSH job is waiting must terminate its askpass wait.
struct JobLease {
    remote: Arc<Connections>,
    id: Option<String>,
}
impl Drop for JobLease {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let remote = self.remote.clone();
            std::thread::spawn(move || {
                let _ = remote.ssh_cancel(&id);
            });
        }
    }
}

#[derive(Debug, PartialEq)]
struct Health {
    label: String,
    needs_update: bool,
}

fn describe_health(environment: &str, host: &Value) -> Result<Health, String> {
    if host.get("environmentId").and_then(Value::as_str) != Some(environment) {
        return Err("Host identity changed".into());
    }
    let providers = host
        .get("providers")
        .and_then(Value::as_array)
        .ok_or_else(|| "Host returned an invalid provider list".to_owned())?;
    let supports = |capability| {
        host.get("capabilities")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(capability)))
    };
    let needs_update = !supports("workspace.run") || !supports("git.worktreeCreate");
    Ok(Health {
        label: if needs_update {
            "Connected · host update needed for Explorer and Changes"
        } else if providers.is_empty() {
            "Connected · install a supported provider on the host"
        } else {
            "Connected"
        }
        .into(),
        needs_update,
    })
}

fn parse_port(value: &str) -> Result<Option<u16>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    value
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .map(Some)
        .ok_or_else(|| "SSH port must be between 1 and 65535.".into())
}

fn paragraph(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(19.5))
        .text_color(content(0.45))
        .child(text.into())
}

fn remove_machine(remote: &Connections, machine: Machine, revoke: bool) -> Result<Machine, String> {
    if revoke {
        remote.request(&machine.id, "devices.revokeSelf", json!({})).map_err(|reason| format!(
            "Could not revoke access, so {} was not removed: {reason}. Reconnect and try again, or remove it from this desktop only and revoke it on the host with monocode-host devices and monocode-host revoke <device-id>.", machine.name
        ))?;
    }
    remote.disconnect(&machine.id)?;
    Ok(machine)
}

impl SettingsView {
    fn initialize_connection_fields(&mut self, cx: &mut Context<Self>) {
        if self.connections.initialized {
            return;
        }
        self.connections.initialized = true;
        for (key, placeholder, secret) in [
            (TARGET, "user@my-mac-mini or an SSH alias", false),
            (NAME, "Optional, e.g. Home Mac mini", false),
            (PORT, "From SSH config", false),
            (URL, "http://127.0.0.1:3774", false),
            (TOKEN, "Device token", true),
            (ANSWER, "Password or passphrase", true),
        ] {
            let input = cx.new(|cx| {
                let mut input = Input::field(key, placeholder, content(0.35), cx);
                input.set_font_size(13.);
                input.set_secret(secret);
                if key == URL {
                    input.set_text("http://127.0.0.1:3774".into(), cx);
                }
                input
            });
            self._subscriptions
                .push(cx.subscribe(&input, |_, _, _: &input::Changed, cx| cx.notify()));
            self._subscriptions.push(cx.subscribe(
                &input,
                move |this, _, _: &input::Submitted, cx| match key {
                    TARGET | NAME | PORT => this.begin_connection(None, false, cx),
                    URL | TOKEN => this.connect_url(cx),
                    ANSWER => this.answer_connection(None, cx),
                    _ => {}
                },
            ));
            self.fields.insert(key, input);
        }
    }

    fn connection_value(&self, key: &'static str, cx: &Context<Self>) -> String {
        self.fields
            .get(key)
            .map(|input| input.read(cx).content.to_string())
            .unwrap_or_default()
    }

    fn clear_connection_field(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if let Some(input) = self.fields.get(key) {
            input.update(cx, |input, cx| input.set_text(String::new(), cx));
        }
    }

    fn connection_field(
        &mut self,
        key: &'static str,
        label: &'static str,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let input = self.fields[key].clone();
        input.update(cx, |input, _| input.set_read_only(disabled));
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(content(0.65))
            .child(label)
            .child(
                div()
                    .h(px(36.))
                    .min_w_0()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.15))
                    .bg(content(0.03))
                    .px_3()
                    .flex()
                    .items_center()
                    .when(disabled, |d| d.opacity(0.4))
                    .child(input),
            )
    }

    pub(in crate::settings) fn watch_connections(&mut self, cx: &mut Context<Self>) {
        if self.section != Section::Connections || self.ctx.remote.is_none() {
            return;
        }
        self.initialize_connection_fields(cx);
        self.refresh_connections(cx);
        self.connections.timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(10))
                    .await;
                if this
                    .update(cx, |this, cx| this.refresh_connections(cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(in crate::settings) fn leave_connections(&mut self, cx: &mut Context<Self>) {
        self.connections.generation += 1;
        self.connections.poll = None;
        self.connections.timer = None;
        self.connections.refresh = None;
        self.connections.lease = None;
        self.connections.job = None;
        self.connections.busy = false;
        self.connections.revoking = false;
        self.connections.answering = false;
        self.connections.removing = None;
        self.clear_connection_field(ANSWER, cx);
        self.clear_connection_field(TOKEN, cx);
    }

    pub(in crate::settings) fn dismiss_connections(&mut self, cx: &mut Context<Self>) -> bool {
        if self.section != Section::Connections
            || self.connections.busy
            || self.connections.revoking
        {
            return false;
        }
        if self.connections.removing.take().is_some() {
            cx.notify();
            return true;
        }
        if self.connections.adding {
            self.connections.adding = false;
            cx.notify();
            return true;
        }
        false
    }

    pub(in crate::settings) fn connections_window_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.connections.reveal_prompt {
            return;
        }
        self.connections.reveal_prompt = false;
        let weak = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = weak.update(cx, |this, cx| {
                if this.section != Section::Connections || !this.connections.busy {
                    return;
                }
                if let Some(target) = this.connections.progress.get() {
                    let viewport = this.scroll.bounds();
                    if target.top() < viewport.top() || target.bottom() > viewport.bottom() {
                        let offset = this.scroll.offset();
                        let delta = target.center().y - viewport.center().y;
                        let y = (offset.y - delta).clamp(-this.scroll.max_offset().y, px(0.));
                        this.scroll.set_offset(gpui::point(offset.x, y));
                    }
                }
                if this
                    .connections
                    .job
                    .as_ref()
                    .and_then(|job| job.prompt.as_ref())
                    .is_some_and(|prompt| !prompt.confirm)
                    && let Some(input) = this.fields.get(ANSWER)
                {
                    window.focus(&input.focus_handle(cx), cx);
                }
                cx.notify();
            });
        });
    }

    fn refresh_connections(&mut self, cx: &mut Context<Self>) {
        if self.section != Section::Connections
            || self.connections.busy
            || self.connections.refresh.is_some()
        {
            return;
        }
        let Some(remote) = self.ctx.remote.clone() else {
            return;
        };
        let generation = self.connections.generation;
        let work = cx.background_executor().spawn(async move {
            let machines = remote.list()?;
            let providers: Vec<_> = monocode_core::provider::Kind::ALL
                .into_iter()
                .map(|p| p.id())
                .collect();
            let statuses = machines
                .iter()
                .map(|machine| {
                    let health = remote
                        .request(
                            &machine.id,
                            "environment.describe",
                            json!({"supportedProviders": providers}),
                        )
                        .and_then(|host| describe_health(&machine.environment_id, &host));
                    (machine.id.clone(), health)
                })
                .collect::<Vec<_>>();
            Ok::<_, String>((machines, statuses))
        });
        self.connections.refresh = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.connections.generation {
                    return;
                }
                this.connections.refresh = None;
                this.connections.loaded = true;
                match result {
                    Ok((machines, statuses)) => {
                        this.connections.machines = machines;
                        for (id, health) in statuses {
                            match health {
                                Ok(health) => {
                                    this.connections.status.insert(id.clone(), health.label);
                                    if health.needs_update {
                                        this.connections.needs_update.insert(id);
                                    } else {
                                        this.connections.needs_update.remove(&id);
                                    }
                                }
                                Err(_) => {
                                    this.connections
                                        .status
                                        .insert(id, "Offline · reconnect to check access".into());
                                }
                            }
                        }
                    }
                    Err(error) => this.connections.error = error,
                }
                cx.notify();
            });
        }));
    }

    fn begin_connection(&mut self, machine: Option<String>, upgrade: bool, cx: &mut Context<Self>) {
        if self.connections.busy || self.connections.revoking {
            return;
        }
        let Some(remote) = self.ctx.remote.clone() else {
            return;
        };
        let target = self.connection_value(TARGET, cx).trim().to_owned();
        if machine.is_none() && target.is_empty() {
            return;
        }
        let port = match if machine.is_some() {
            Ok(None)
        } else {
            parse_port(&self.connection_value(PORT, cx))
        } {
            Ok(port) => port,
            Err(error) => {
                self.connections.error = error;
                cx.notify();
                return;
            }
        };
        let name = self.connection_value(NAME, cx).trim().to_owned();
        self.connections.busy = true;
        self.connections.error.clear();
        self.connections.notice.clear();
        self.connections.job = None;
        self.connections.updating = machine.clone().filter(|_| upgrade);
        self.connections.generation += 1;
        self.connections.refresh = None;
        let generation = self.connections.generation;
        let begin_remote = remote.clone();
        let work = cx.background_executor().spawn(async move {
            if let Some(id) = machine {
                begin_remote.ssh_reconnect(&id, upgrade)
            } else {
                begin_remote.ssh_begin(target, name, port)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let id = result.as_ref().ok().cloned();
            let accepted = this
                .update(cx, |this, cx| {
                    if generation != this.connections.generation {
                        return false;
                    }
                    match result {
                        Ok(id) => {
                            this.connections.lease = Some(JobLease {
                                remote: remote.clone(),
                                id: Some(id.clone()),
                            });
                            this.connections.reveal_prompt = true;
                            this.poll_connection(id, generation, cx);
                        }
                        Err(error) => {
                            this.connections.error = error;
                            this.connections.busy = false;
                        }
                    }
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !accepted && let Some(id) = id {
                cx.background_executor()
                    .spawn(async move {
                        let _ = remote.ssh_cancel(&id);
                    })
                    .detach();
            }
        })
        .detach();
        cx.notify();
    }

    fn poll_connection(&mut self, id: String, generation: u64, cx: &mut Context<Self>) {
        let Some(remote) = self.ctx.remote.clone() else {
            return;
        };
        self.connections.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                let poll_remote = remote.clone();
                let poll_id = id.clone();
                let next = cx.background_executor().spawn(async move { poll_remote.ssh_poll(&poll_id) }).await;
                let keep_polling = this.update(cx, |this, cx| {
                    if generation != this.connections.generation { return false; }
                    match next {
                        Ok(job) => {
                            let old_prompt = this.connections.job.as_ref().and_then(|j| j.prompt.as_ref()).map(|p| p.id.as_str());
                            let next_prompt = job.prompt.as_ref().map(|p| p.id.as_str());
                            if old_prompt != next_prompt {
                                this.connections.reveal_prompt = next_prompt.is_some();
                                this.connections.answering = false;
                                this.clear_connection_field(ANSWER, cx);
                            }
                            let done = job.done;
                            if done {
                                if let Some(lease) = this.connections.lease.as_mut() { lease.id = None; }
                                this.connections.lease = None;
                                this.connections.busy = false;
                                this.clear_connection_field(ANSWER, cx);
                                if let Some(error) = &job.error { this.connections.error = error.clone(); }
                                else if let Some(machine) = &job.machine {
                                    this.connections.adding = false;
                                    this.connections.notice = if this.connections.updating.take().is_some() {
                                        format!("{} was updated and reconnected.", machine.name)
                                    } else {
                                        format!("{} is connected. To work on it, click + next to Projects in the project rail and choose Open folder on a machine.", machine.name)
                                    };
                                    this.connections.status.insert(machine.id.clone(), "Connected".into());
                                    for key in [TARGET, NAME, PORT] { this.clear_connection_field(key, cx); }
                                }
                            }
                            this.connections.job = Some(job);
                            if done { this.refresh_connections(cx); }
                            cx.notify();
                            !done
                        }
                        Err(error) => {
                            this.connections.error = error;
                            this.connections.busy = false;
                            this.connections.lease = None;
                            cx.notify();
                            false
                        }
                    }
                }).unwrap_or(false);
                if !keep_polling { break; }
                cx.background_executor().timer(Duration::from_millis(350)).await;
            }
        }));
    }

    fn answer_connection(&mut self, value: Option<&str>, cx: &mut Context<Self>) {
        if self.connections.answering {
            return;
        }
        let Some(id) = self
            .connections
            .lease
            .as_ref()
            .and_then(|lease| lease.id.clone())
        else {
            return;
        };
        let Some(prompt) = self
            .connections
            .job
            .as_ref()
            .and_then(|job| job.prompt.clone())
        else {
            return;
        };
        let Some(remote) = self.ctx.remote.clone() else {
            return;
        };
        let answer = value.map(str::to_owned).unwrap_or_else(|| {
            if prompt.confirm {
                "yes".into()
            } else {
                self.connection_value(ANSWER, cx)
            }
        });
        self.connections.answering = true;
        self.connections.error.clear();
        let generation = self.connections.generation;
        let work = cx
            .background_executor()
            .spawn(async move { remote.ssh_answer(&id, &prompt.id, answer) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.connections.generation {
                    return;
                }
                match result {
                    Ok(()) => this.clear_connection_field(ANSWER, cx),
                    Err(error) => {
                        this.connections.error = error;
                        this.connections.answering = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn cancel_connection(&mut self, cx: &mut Context<Self>) {
        let Some(lease) = self.connections.lease.as_ref() else {
            return;
        };
        let Some(id) = lease.id.clone() else {
            return;
        };
        let remote = lease.remote.clone();
        let generation = self.connections.generation;
        let work = cx
            .background_executor()
            .spawn(async move { remote.ssh_cancel(&id) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = work.await {
                let _ = this.update(cx, |this, cx| {
                    if generation == this.connections.generation {
                        this.connections.error = error;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn connect_url(&mut self, cx: &mut Context<Self>) {
        if self.connections.busy || self.connections.revoking {
            return;
        }
        let Some(remote) = self.ctx.remote.clone() else {
            return;
        };
        let url = self.connection_value(URL, cx);
        let token = self.connection_value(TOKEN, cx);
        if url.trim().is_empty() || token.trim().is_empty() {
            self.connections.error = "Enter a host URL and device token.".into();
            cx.notify();
            return;
        }
        self.connections.busy = true;
        self.connections.error.clear();
        self.connections.notice.clear();
        self.connections.generation += 1;
        self.connections.refresh = None;
        let generation = self.connections.generation;
        let work = cx
            .background_executor()
            .spawn(async move { remote.connect("".into(), url, token) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.connections.generation {
                    return;
                }
                this.connections.busy = false;
                match result {
                    Ok(machine) => {
                        this.clear_connection_field(TOKEN, cx);
                        this.connections.notice = format!("{} is connected.", machine.name);
                        this.refresh_connections(cx);
                    }
                    Err(error) => this.connections.error = error,
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_connection(&mut self, machine: Machine, revoke: bool, cx: &mut Context<Self>) {
        if self.connections.busy || self.connections.revoking {
            return;
        }
        let Some(remote) = self.ctx.remote.clone() else {
            return;
        };
        self.connections.error.clear();
        self.connections.notice.clear();
        self.connections.revoking = true;
        self.connections.generation += 1;
        self.connections.refresh = None;
        let generation = self.connections.generation;
        let work = cx
            .background_executor()
            .spawn(async move { remove_machine(&remote, machine, revoke) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.connections.generation { return; }
                this.connections.revoking = false;
                match result {
                    Ok(machine) => {
                        this.connections.removing = None;
                        this.connections.machines.retain(|m| m.id != machine.id);
                        this.connections.notice = if revoke {
                            format!("{} was removed and this desktop's access was revoked. The host and its sessions keep running.", machine.name)
                        } else {
                            format!("{} was removed from this desktop. The host and its sessions keep running, and it still accepts this desktop's credential.", machine.name)
                        };
                        this.refresh_connections(cx);
                    }
                    Err(error) => this.connections.error = error,
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    pub(super) fn connections_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.initialize_connection_fields(cx);
        let busy = self.connections.busy;
        let unavailable = self.ctx.remote.is_none();
        let disabled = busy || unavailable || self.connections.revoking;
        let mut page = vec![div().relative().child(self.reveal_probe("remote-machines"))
            .flex().items_end().justify_between().gap_4()
            .child(div().min_w_0().flex_1()
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Your machines"))
                .child(paragraph("Run agents on another computer and return to them from your laptop. The host keeps working when you close MonoCode here.").mt_1()))
            .when(!self.connections.adding, |d| d.child(
                secondary_button("add-machine", "Add machine", false, disabled).child(icon("plus", 16.)).child("Add machine")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.connections.busy || this.connections.revoking || this.ctx.remote.is_none() { return; }
                        this.connections.adding = true;
                        if let Some(input) = this.fields.get(TARGET) { window.focus(&input.focus_handle(cx), cx); }
                        this.connections.error.clear(); this.connections.notice.clear(); cx.notify();
                    })))).into_any_element()];
        if !self.connections.machines.is_empty() {
            let mut list = div()
                .mt_5()
                .rounded_xl()
                .border_1()
                .border_color(content(0.1))
                .overflow_hidden();
            for machine in self.connections.machines.clone() {
                let id = machine.id.clone();
                let endpoint = machine
                    .ssh
                    .as_ref()
                    .map(|ssh| {
                        format!(
                            "SSH · {}{}",
                            ssh.target,
                            ssh.port
                                .map(|port| format!(" · port {port}"))
                                .unwrap_or_default()
                        )
                    })
                    .unwrap_or_else(|| machine.endpoint.clone());
                let update = self.connections.needs_update.contains(&id);
                let reconnect_id = id.clone();
                let update_id = id.clone();
                let remove_id = id.clone();
                list = list.child(div().border_b_1().border_color(content(0.1))
                    .child(div().flex().items_center().gap_3().p_4()
                        .child(icon("globe", 20.).text_color(content(0.45)))
                        .child(div().min_w_0().flex_1()
                            .child(div().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(machine.name.clone()))
                            .child(paragraph(endpoint).mt_1())
                            .child(paragraph(self.connections.status.get(&id).cloned().unwrap_or_else(|| "Checking connection…".into())).mt_1().text_color(content(0.5)))
                            .when(machine.ssh.is_some() && update, |d| d.child(paragraph("Updating restarts the host and interrupts active agent turns.").mt_1().text_size(px(11.)))))
                        .when(machine.ssh.is_some(), |d| d.child(div().flex().items_center().gap_2()
                            .when(update, |d| d.child(secondary_button(SharedString::from(format!("update-host-{id}")), "Update Host", false, disabled)
                                .child("Update Host").on_click(cx.listener(move |this, _, _, cx| this.begin_connection(Some(update_id.clone()), true, cx)))))
                            .child(secondary_button(SharedString::from(format!("reconnect-{id}")), "Reconnect", false, disabled)
                                .child("Reconnect").on_click(cx.listener(move |this, _, _, cx| this.begin_connection(Some(reconnect_id.clone()), false, cx))))))
                        .child(secondary_button(SharedString::from(format!("remove-machine-{id}")), format!("Remove {}", machine.name), true, disabled)
                            .child(icon("trash", 16.)).on_click(cx.listener(move |this, _, _, cx| {
                                if this.connections.busy || this.connections.revoking { return; }
                                this.connections.error.clear(); this.connections.removing = Some(remove_id.clone()); cx.notify();
                            }))))
                    .when(self.connections.removing.as_ref() == Some(&id), |d| d.child(self.remove_connection_confirmation(machine.clone(), cx))));
            }
            page.push(list.into_any_element());
        } else if self.connections.loaded && !self.connections.adding {
            page.push(
                div()
                    .mt_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(content(0.15))
                    .px_5()
                    .py_8()
                    .text_center()
                    .text_size(px(13.))
                    .text_color(content(0.45))
                    .child("Add your always-on Windows, Mac, or Linux machine to get started.")
                    .into_any_element(),
            );
        }
        if self.connections.adding {
            let target = self.connection_field(TARGET, "SSH address", disabled, cx);
            let name = self.connection_field(NAME, "Name (optional)", disabled, cx);
            let port = self.connection_field(PORT, "SSH port", disabled, cx);
            let connect_disabled = disabled || self.connection_value(TARGET, cx).trim().is_empty();
            page.push(div().mt_5().p_5().rounded_xl().border_1().border_color(content(0.1)).flex().flex_col().gap_4()
                .child(div().flex().justify_between().child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child("Connect through SSH"))
                    .child(div().rounded_sm().bg(content(0.1)).px_2().py_1().text_size(px(11.)).text_color(content(0.6)).child("SSH")))
                .child(target).child(name)
                .child(div().id("ssh-advanced").role(Role::Button).aria_label("Advanced SSH settings").text_size(px(12.)).text_color(content(0.5)).cursor_pointer()
                    .child(if self.connections.advanced { "▾ Advanced" } else { "▸ Advanced" })
                    .on_click(cx.listener(|this, _, _, cx| { this.connections.advanced = !this.connections.advanced; cx.notify(); })))
                .when(self.connections.advanced, |d| d.child(div().max_w(px(160.)).child(port)))
                .child(paragraph("MonoCode installs and starts its background host, then connects securely. Your SSH keys and config are used automatically. Enable SSH on the host and sign in to Codex or Claude Code there. On Windows and Mac, keep the host’s desktop account signed in and the machine awake. Locking the desktop is fine."))
                .child(paragraph("On Linux, setup installs a systemd user service and turns on lingering for your account (loginctl enable-linger), so the host and your other user services keep running after you log out. Without systemd, as in many containers and microVMs, the host runs as a background process instead and is started again when you reconnect. The host keeps running until you stop it on that machine; removing it here only disconnects this desktop."))
                .child(div().flex().justify_end().gap_2()
                    .child(secondary_button("cancel-machine", "Cancel", false, busy).child("Cancel").on_click(cx.listener(|this, _, _, cx| {
                        if !this.connections.busy { this.connections.adding = false; cx.notify(); }
                    })))
                    .child(secondary_button("connect-machine", "Connect", false, connect_disabled).child(if busy { "Connecting…" } else { "Connect" })
                        .on_click(cx.listener(|this, _, _, cx| this.begin_connection(None, false, cx)))))
                .into_any_element());
        }
        if busy && self.connections.lease.is_some() {
            let job = self.connections.job.clone();
            let mut progress = div()
                .id("ssh-connection-progress")
                .role(Role::Status)
                .relative()
                .child(self.connections.progress.probe())
                .mt_5()
                .p_5()
                .rounded_xl()
                .border_1()
                .border_color(content(0.1))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(13.))
                        .child(icon("loader", 16.))
                        .child(
                            job.as_ref()
                                .map(|job| job.message.clone())
                                .unwrap_or_else(|| "Starting connection…".into()),
                        ),
                );
            if let Some(prompt) = job.and_then(|job| job.prompt) {
                let answering = self.connections.answering;
                progress = progress.child(paragraph(prompt.message).text_color(content(0.7)));
                if !prompt.confirm {
                    progress = progress.child(self.connection_field(
                        ANSWER,
                        "SSH password or passphrase",
                        answering,
                        cx,
                    ));
                }
                progress = progress.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            secondary_button("answer-ssh-prompt", "Continue", false, answering)
                                .child(if prompt.confirm {
                                    "Trust host and continue"
                                } else {
                                    "Continue"
                                })
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.answer_connection(None, cx)),
                                ),
                        )
                        .when(prompt.confirm, |d| {
                            d.child(
                                secondary_button("reject-ssh-host", "Reject", false, answering)
                                    .child("Reject")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.answer_connection(Some("no"), cx)
                                    })),
                            )
                        }),
                );
            }
            page.push(
                progress
                    .child(
                        secondary_button(
                            "cancel-ssh-connection",
                            "Cancel connection",
                            false,
                            false,
                        )
                        .child("Cancel connection")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_connection(cx))),
                    )
                    .into_any_element(),
            );
        }
        if !self.connections.error.is_empty() {
            page.push(
                div()
                    .id("remote-connection-error")
                    .role(Role::Alert)
                    .mt_4()
                    .rounded_lg()
                    .bg(red(0.05))
                    .p_3()
                    .text_size(px(12.))
                    .line_height(px(19.5))
                    .text_color(red(1.))
                    .child(self.connections.error.clone())
                    .into_any_element(),
            );
        }
        if !self.connections.notice.is_empty() {
            page.push(
                div()
                    .id("remote-connection-notice")
                    .role(Role::Status)
                    .mt_4()
                    .text_size(px(13.))
                    .text_color(rgb(0x10b981))
                    .child(self.connections.notice.clone())
                    .into_any_element(),
            );
        }
        page.push(
            div()
                .id("existing-host-url")
                .role(Role::Button)
                .aria_label("Connect to an existing host by URL")
                .mt_5()
                .text_size(px(12.))
                .text_color(content(0.45))
                .cursor_pointer()
                .child(if self.connections.url {
                    "▾ Connect to an existing host by URL"
                } else {
                    "▸ Connect to an existing host by URL"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.connections.url = !this.connections.url;
                    cx.notify();
                }))
                .into_any_element(),
        );
        if self.connections.url {
            let url = self.connection_field(URL, "Host URL", disabled, cx);
            let token = self.connection_field(TOKEN, "Device token", disabled, cx);
            page.push(
                div()
                    .mt_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(url)
                    .child(token)
                    .child(
                        secondary_button("connect-host-url", "Connect by URL", false, disabled)
                            .child("Connect by URL")
                            .on_click(cx.listener(|this, _, _, cx| this.connect_url(cx))),
                    )
                    .into_any_element(),
            );
        }
        page
    }

    fn remove_connection_confirmation(
        &mut self,
        machine: Machine,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let revoke_machine = machine.clone();
        let disabled = self.connections.revoking;
        div().id("remote-remove-confirmation").role(Role::Group).aria_label(format!("Confirm removing {}", machine.name)).border_t_1().border_color(content(0.1)).bg(content(0.03)).p_4().flex().flex_col().gap_3()
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(format!("Remove {} from this desktop?", machine.name)))
            .child(paragraph("This closes this desktop’s connection to the machine. It does not stop the host, and its sessions keep running and stay on that machine. You can add it again later."))
            .child(paragraph("Removing alone leaves this desktop’s credential valid on the host. Revoke access to invalidate it first; the machine must be reachable."))
            .child(paragraph("To stop the host and turn off its background service, run ~/.monocode-host/bin/monocode-host service uninstall on that machine (%USERPROFILE%\\.monocode-host\\bin\\monocode-host.cmd service uninstall on Windows). Its sessions and history are kept."))
            .child(div().flex().flex_wrap().gap_2()
                .child(secondary_button("revoke-remove-machine", "Revoke access and remove", true, disabled).child("Revoke access and remove")
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_connection(revoke_machine.clone(), true, cx))))
                .child(secondary_button("remove-machine-only", "Remove from this desktop only", false, disabled).child("Remove from this desktop only")
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_connection(machine.clone(), false, cx))))
                .child(secondary_button("cancel-remove-machine", "Cancel", false, disabled).child("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| { if !this.connections.revoking { this.connections.removing = None; cx.notify(); } })))).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Read, Write};

    #[test]
    fn failed_revocation_preserves_connection_until_local_only_removal() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut methods = vec![];
            for response in [
                json!({"result": {"protocolVersion": 1, "environmentId": "env", "name": "Server"}}),
                json!({"error": "device revocation failed"}),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                methods.push(request["method"].as_str().unwrap().to_owned());
                let body = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            methods
        });
        let directory = std::env::temp_dir().join(format!(
            "monocode-revocation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let remote = Connections::new(&directory);
        let machine = remote.connect(String::new(), url, "a".repeat(43)).unwrap();
        let error = remove_machine(&remote, machine.clone(), true).unwrap_err();
        assert!(error.contains("Server was not removed"));
        assert_eq!(remote.list().unwrap(), vec![machine.clone()]);
        remove_machine(&remote, machine, false).unwrap();
        assert!(remote.list().unwrap().is_empty());
        assert_eq!(
            server.join().unwrap(),
            ["environment.describe", "devices.revokeSelf"]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn identity_change_never_reports_a_healthy_connection() {
        assert_eq!(
            describe_health(
                "known-host",
                &json!({"environmentId": "other-host", "providers": ["codex"], "capabilities": ["workspace.run", "git.worktreeCreate"]})
            ),
            Err("Host identity changed".into())
        );
    }

    #[test]
    fn health_checks_explorer_and_worktree_support_before_provider_installation() {
        assert_eq!(
            describe_health("host", &json!({"environmentId": "host", "providers": []})).unwrap(),
            Health {
                label: "Connected · host update needed for Explorer and Changes".into(),
                needs_update: true
            }
        );
        assert_eq!(describe_health("host", &json!({"environmentId": "host", "providers": [], "capabilities": ["workspace.run", "git.worktreeCreate"]})).unwrap(), Health {
            label: "Connected · install a supported provider on the host".into(), needs_update: false
        });
    }

    #[test]
    fn ssh_port_uses_config_by_default_and_rejects_invalid_ports() {
        assert_eq!(parse_port(""), Ok(None));
        assert_eq!(parse_port(" 2222 "), Ok(Some(2222)));
        for value in ["0", "65536", "-1", "22.5", "ssh"] {
            assert!(parse_port(value).is_err());
        }
    }
}
