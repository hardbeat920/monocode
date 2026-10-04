//! The Connect page's native calls over the remote package.

use gpui::{App, Entity, Task};
use monocode_engine::remote::{RemoteConnections, RemoteGlobal};
use monocode_remote::host::protocol::{HostProject, RemoteMachine, SshSetup};
use monocode_view_remote::host::{HostTask, RemoteHost, SshBegin};
use serde_json::Value;

pub struct RemoteAdapter {
    connections: Entity<RemoteConnections>,
}
impl RemoteAdapter {
    pub fn new(cx: &App) -> Self {
        Self {
            connections: RemoteGlobal::global(cx).connections.clone(),
        }
    }
}
impl RemoteHost for RemoteAdapter {
    fn machines(&self, cx: &mut App) -> HostTask<Vec<RemoteMachine>> {
        let client = self.connections.read(cx).client().clone();
        cx.foreground_executor().spawn(client.load_machines())
    }
    fn request(&self, machine: &str, method: &str, params: Value, cx: &mut App) -> HostTask<Value> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).request(machine, method, params))
    }
    fn app_version(&self, _: &mut App) -> HostTask<String> {
        Task::ready(Ok(env!("CARGO_PKG_VERSION").into()))
    }
    fn pair(&self, link: &str, name: &str, cx: &mut App) -> HostTask<RemoteMachine> {
        self.connections.update(cx, |connections, cx| {
            connections.pair(link.into(), name.into(), cx)
        })
    }
    fn retry(&self, machine: &str, cx: &mut App) -> HostTask<()> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).retry(machine))
    }
    fn disconnect(&self, machine: &str, cx: &mut App) -> HostTask<()> {
        self.connections
            .update(cx, |connections, cx| connections.disconnect(machine, cx))
    }
    fn ssh_begin(&self, request: SshBegin, cx: &mut App) -> HostTask<String> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).ssh_begin(
                request.target,
                request.name,
                request.port,
                request.upgrade,
            ))
    }
    fn ssh_reconnect(&self, machine: &str, upgrade: bool, cx: &mut App) -> HostTask<String> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).ssh_reconnect(machine, upgrade))
    }
    fn ssh_poll(&self, job: &str, cx: &mut App) -> HostTask<SshSetup> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).ssh_poll(job))
    }
    fn ssh_answer(&self, job: &str, prompt: &str, answer: String, cx: &mut App) -> HostTask<()> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).ssh_answer(job, prompt, answer))
    }
    fn ssh_cancel(&self, job: &str, cx: &mut App) -> HostTask<()> {
        cx.foreground_executor()
            .spawn(self.connections.read(cx).ssh_cancel(job))
    }
    fn remember_project(&self, environment: &str, project: &HostProject, cx: &mut App) -> String {
        self.connections.update(cx, |connections, cx| {
            connections
                .remember_remote_project(environment, project, cx)
                .key
        })
    }
    fn machines_changed(&self, cx: &mut App) {
        self.connections
            .update(cx, |connections, cx| connections.refresh_machines(cx));
    }
}

/// Confirms the machine offered by a pairing link and pairs it through the engine.
pub fn open_pairing_link(link: &str, cx: &mut App) {
    let offer = match monocode_remote::host::network::parse_pairing_link(link) {
        Ok(offer) => offer,
        Err(error) => {
            monocode_app::bridge::dialogs::alert(&error, true, cx);
            return;
        }
    };
    let name = if offer.name.trim().is_empty() {
        "Remote machine".into()
    } else {
        offer.name.clone()
    };
    let confirm = monocode_app::bridge::dialogs::confirm(
        &format!(
            "Connect to {name}?\n\n{}\n\nFingerprint: {}",
            offer.endpoints.join("\n"),
            offer.fingerprint
        ),
        "Connect",
        cx,
    );
    let link = link.to_string();
    let connections = RemoteGlobal::global(cx).connections.clone();
    cx.spawn(async move |cx| {
        if !confirm.await {
            return;
        }
        let task = cx.update(|cx| {
            connections.update(cx, |connections, cx| connections.pair(link, name, cx))
        });
        if let Err(error) = task.await {
            cx.update(|cx| monocode_app::bridge::dialogs::alert(&error, true, cx));
        }
    })
    .detach();
}
