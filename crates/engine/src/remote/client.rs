//! The thread-safe half of the connections model: requests, the machine
//! list cache (`cachedMachines` and `machinesLoaded` in connections.ts),
//! session sync (`syncRemoteSession`, `loadRemoteSession`), attachment
//! uploads, and workspace command routing (`runRemoteCommand`,
//! `invokeWorkspace`).
//!
//! `RemoteClient` is `Send + Sync`, so file backends and background tasks
//! can route `remote://` paths without the GPUI entities.

use std::sync::Arc;

use monocode_core::Attachment;
use monocode_remote::host::protocol::{
    HostSession, RemoteAttachment, RemoteMachine, SessionSync, SessionSyncChunk,
    SessionSyncResponse, SshSetup, apply_session_sync,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::remote_attachment_previews::{ChunkReader, Downloads, with_remote_attachment_previews};
use super::remote_attachments::upload_remote_attachments;
use super::remote_commands::{
    HostCall, NOT_CONNECTED, OUTDATED, host_call, is_remote_workspace_call, is_unsupported,
    remote_result,
};
use super::transport::{RemoteFuture, RemoteTransport};

#[derive(Default)]
struct MachineCache {
    machines: Vec<RemoteMachine>,
    loaded: bool,
}

struct Inner {
    transport: Arc<dyn RemoteTransport>,
    machines: Mutex<MachineCache>,
    downloads: Downloads,
}

/// Requests to connected machines. Clones share the machine cache and the
/// image downloads in progress.
#[derive(Clone)]
pub struct RemoteClient(Arc<Inner>);

/// Read a typed answer from a host result.
pub fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| "Invalid host response".to_string())
}

impl RemoteClient {
    pub fn new(transport: Arc<dyn RemoteTransport>) -> Self {
        Self(Arc::new(Inner {
            transport,
            machines: Mutex::new(MachineCache::default()),
            downloads: Downloads::default(),
        }))
    }

    pub fn transport(&self) -> &Arc<dyn RemoteTransport> {
        &self.0.transport
    }

    /// `remoteRequest`.
    pub fn request(&self, machine_id: &str, method: &str, params: Value) -> RemoteFuture<Value> {
        self.0
            .transport
            .request(machine_id.to_string(), method.to_string(), params)
    }

    /// `remoteRequest<T>`: the answer read as `T`.
    pub fn request_as<T: DeserializeOwned + Send + 'static>(
        &self,
        machine_id: &str,
        method: &str,
        params: Value,
    ) -> RemoteFuture<T> {
        let request = self.request(machine_id, method, params);
        Box::pin(async move { decode(request.await?) })
    }

    /// `read_file_base64` for an attachment on this computer.
    pub fn read_file_base64(&self, path: String) -> RemoteFuture<String> {
        self.0.transport.read_file_base64(path)
    }

    // The machine list.

    /// The machines from the last list read.
    pub fn cached_machines(&self) -> Vec<RemoteMachine> {
        self.0.machines.lock().machines.clone()
    }

    /// `machinesLoaded`.
    pub fn machines_loaded(&self) -> bool {
        self.0.machines.lock().loaded
    }

    /// Replace the cached list after a successful read.
    pub fn set_machines(&self, machines: Vec<RemoteMachine>) {
        let mut cache = self.0.machines.lock();
        cache.machines = machines;
        cache.loaded = true;
    }

    /// `remote_machines`, saving the answer in the cache.
    pub fn load_machines(&self) -> RemoteFuture<Vec<RemoteMachine>> {
        let client = self.clone();
        let read = self.0.transport.machines();
        Box::pin(async move {
            let machines = read.await?;
            client.set_machines(machines.clone());
            Ok(machines)
        })
    }

    /// `knownRemoteMachine`: the connected machine for an environment, from
    /// the last machine list read.
    pub fn known_remote_machine(&self, environment_id: &str) -> Option<RemoteMachine> {
        self.0
            .machines
            .lock()
            .machines
            .iter()
            .find(|machine| machine.environment_id == environment_id)
            .cloned()
    }

    /// `remoteMachineFor`: the connected machine for an environment, reading
    /// the list when it was never read.
    pub fn remote_machine_for(&self, environment_id: &str) -> RemoteFuture<Option<RemoteMachine>> {
        let known = self.known_remote_machine(environment_id);
        if known.is_some() || self.machines_loaded() {
            return Box::pin(async move { Ok(known) });
        }
        let client = self.clone();
        let environment_id = environment_id.to_string();
        Box::pin(async move {
            client.load_machines().await?;
            Ok(client.known_remote_machine(&environment_id))
        })
    }

    /// `pairMachine`: pair, then add the machine to the cache.
    pub fn pair(&self, link: String, name: String) -> RemoteFuture<RemoteMachine> {
        let client = self.clone();
        let pair = self.0.transport.pair(link, name);
        Box::pin(async move {
            let machine = pair.await?;
            let mut cache = client.0.machines.lock();
            cache.machines.retain(|entry| entry.id != machine.id);
            cache.machines.push(machine.clone());
            cache.loaded = true;
            Ok(machine)
        })
    }

    /// `retryMachine`: try every route again on the next request.
    pub fn retry(&self, machine_id: &str) -> RemoteFuture<()> {
        self.0.transport.retry(machine_id.to_string())
    }

    /// `disconnectMachine`: delete the connection and drop it from the cache.
    pub fn disconnect(&self, machine_id: &str) -> RemoteFuture<()> {
        let client = self.clone();
        let id = machine_id.to_string();
        let disconnect = self.0.transport.disconnect(id.clone());
        Box::pin(async move {
            disconnect.await?;
            client
                .0
                .machines
                .lock()
                .machines
                .retain(|entry| entry.id != id);
            Ok(())
        })
    }

    // SSH setup, as the connections settings call it.

    pub fn ssh_begin(
        &self,
        target: String,
        name: String,
        port: Option<u16>,
        upgrade: bool,
    ) -> RemoteFuture<String> {
        self.0.transport.ssh_begin(target, name, port, upgrade)
    }

    pub fn ssh_reconnect(&self, machine_id: &str, upgrade: bool) -> RemoteFuture<String> {
        self.0
            .transport
            .ssh_reconnect(machine_id.to_string(), upgrade)
    }

    pub fn ssh_poll(&self, job_id: &str) -> RemoteFuture<SshSetup> {
        self.0.transport.ssh_poll(job_id.to_string())
    }

    pub fn ssh_answer(&self, job_id: &str, prompt_id: &str, answer: String) -> RemoteFuture<()> {
        self.0
            .transport
            .ssh_answer(job_id.to_string(), prompt_id.to_string(), answer)
    }

    pub fn ssh_cancel(&self, job_id: &str) -> RemoteFuture<()> {
        self.0.transport.ssh_cancel(job_id.to_string())
    }

    // Sessions.

    /// `syncRemoteSession`: one sync, assembled from bounded pieces when the
    /// host chunks it.
    pub fn sync_remote_session(
        &self,
        machine_id: &str,
        session_id: &str,
        revision: Option<i64>,
    ) -> RemoteFuture<SessionSync> {
        let client = self.clone();
        let machine_id = machine_id.to_string();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let mut params = json!({ "sessionId": session_id });
            if let Some(revision) = revision {
                params["revision"] = json!(revision);
            }
            let response: SessionSyncResponse = client
                .request_as(&machine_id, "sessions.sync", params)
                .await?;
            let transfer = match response {
                SessionSyncResponse::Sync(sync) => return Ok(sync),
                SessionSyncResponse::Chunked(transfer) => transfer,
            };
            // Offsets and lengths count UTF-16 code units, as the hosts do.
            // Hosts never split a surrogate pair between pieces.
            let mut pieces = String::new();
            let mut offset: i64 = 0;
            while offset < transfer.length {
                let chunk: SessionSyncChunk = client
                    .request_as(
                        &machine_id,
                        "sessions.syncChunk",
                        json!({
                            "sessionId": session_id,
                            "transfer": transfer.transfer,
                            "offset": offset,
                        }),
                    )
                    .await?;
                if chunk.data.is_empty() {
                    return Err("Session transfer ended early".into());
                }
                offset += chunk.data.encode_utf16().count() as i64;
                pieces.push_str(&chunk.data);
            }
            if offset != transfer.length {
                return Err("Session transfer has an unexpected length".into());
            }
            serde_json::from_str(&pieces).map_err(|error| error.to_string())
        })
    }

    /// `loadRemoteSession`: fetch only what changed since `known`, falling
    /// back to a full snapshot. Returns `known` itself when nothing changed,
    /// so callers can tell an unchanged poll by pointer.
    pub fn load_remote_session(
        &self,
        machine_id: &str,
        session_id: &str,
        known: Option<Arc<HostSession>>,
    ) -> RemoteFuture<Arc<HostSession>> {
        let client = self.clone();
        let machine_id = machine_id.to_string();
        let session_id = session_id.to_string();
        Box::pin(async move {
            let update = client
                .sync_remote_session(
                    &machine_id,
                    &session_id,
                    known.as_ref().map(|known| known.revision),
                )
                .await?;
            let unchanged = matches!(update, SessionSync::Unchanged { .. });
            let applied = apply_session_sync(known.as_deref(), update);
            let snapshot = match (applied, &known) {
                (Ok(_), Some(known)) if unchanged => known.clone(),
                (Ok(snapshot), _) => Arc::new(snapshot),
                (Err(_), _) => {
                    let full = client
                        .sync_remote_session(&machine_id, &session_id, None)
                        .await?;
                    Arc::new(apply_session_sync(None, full)?)
                }
            };
            let reader = client.clone();
            let read_machine = machine_id.clone();
            let read: ChunkReader =
                Arc::new(move |params| reader.request(&read_machine, "attachments.read", params));
            Ok(with_remote_attachment_previews(
                &machine_id,
                snapshot,
                known.as_deref(),
                read,
                &client.0.downloads,
            )
            .await)
        })
    }

    /// `uploadRemoteAttachments`.
    pub fn upload_attachments(
        &self,
        machine_id: &str,
        attachments: Vec<Attachment>,
    ) -> RemoteFuture<Vec<RemoteAttachment>> {
        let client = self.clone();
        let machine_id = machine_id.to_string();
        Box::pin(async move { upload_remote_attachments(&client, &machine_id, &attachments).await })
    }

    // Workspace commands.

    /// `runRemoteCommand`: run a file or Git command whose paths are
    /// `remote://` paths on the machine that owns them.
    pub fn run_remote_command(
        &self,
        command: &str,
        args: Map<String, Value>,
    ) -> RemoteFuture<Value> {
        let call: HostCall = match host_call(command, &args) {
            Ok(call) => call,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let client = self.clone();
        let command = command.to_string();
        Box::pin(async move {
            let machine = client
                .remote_machine_for(&call.environment_id)
                .await?
                .ok_or_else(|| NOT_CONNECTED.to_string())?;
            let result = client
                .request(
                    &machine.id,
                    "workspace.run",
                    json!({ "command": command, "args": Value::Object(call.args.clone()) }),
                )
                .await
                .map_err(|reason| {
                    if is_unsupported(&reason) {
                        OUTDATED.to_string()
                    } else {
                        reason
                    }
                })?;
            Ok(remote_result(&command, &call, result))
        })
    }

    /// `invokeWorkspace`: `Some` with the host's answer when the command's
    /// paths are on a connected machine, `None` when the caller should run
    /// it on this computer.
    pub fn invoke_workspace(
        &self,
        command: &str,
        args: &Map<String, Value>,
    ) -> Option<RemoteFuture<Value>> {
        is_remote_workspace_call(args).then(|| self.run_remote_command(command, args.clone()))
    }

    /// `statFiles` for paths that may span machines: one call per machine
    /// (`""` is this computer, answered by `local`), merged back in the order
    /// asked, with `mtimeMs: null` for paths no machine reported.
    pub fn stat_files(
        &self,
        paths: Vec<String>,
        local: impl FnOnce(Vec<String>) -> RemoteFuture<Value> + Send + 'static,
    ) -> RemoteFuture<Vec<Value>> {
        use super::remote_commands::path_machine;
        if paths.is_empty() {
            return Box::pin(async { Ok(Vec::new()) });
        }
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for path in &paths {
            let machine = path_machine(path).to_string();
            match groups.iter_mut().find(|(key, _)| *key == machine) {
                Some((_, group)) => group.push(path.clone()),
                None => groups.push((machine, vec![path.clone()])),
            }
        }
        let mut local = Some(local);
        let calls: Vec<RemoteFuture<Value>> = groups
            .into_iter()
            .map(|(machine, group)| {
                if machine.is_empty() {
                    let local = local.take().expect("one local group");
                    local(group)
                } else {
                    let mut args = Map::new();
                    args.insert("paths".into(), json!(group));
                    self.run_remote_command("stat_files", args)
                }
            })
            .collect();
        Box::pin(async move {
            let results = futures::future::try_join_all(calls).await?;
            let mut by_path: Map<String, Value> = Map::new();
            for entry in results.into_iter().flat_map(|result| match result {
                Value::Array(entries) => entries,
                _ => Vec::new(),
            }) {
                if let Some(path) = entry.get("path").and_then(Value::as_str) {
                    by_path.insert(path.to_string(), entry.clone());
                }
            }
            Ok(paths
                .into_iter()
                .map(|path| {
                    by_path
                        .get(&path)
                        .cloned()
                        .unwrap_or_else(|| json!({ "path": path, "mtimeMs": null }))
                })
                .collect())
        })
    }
}
