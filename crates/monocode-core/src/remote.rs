//! Remote host connections. The renderer receives machine metadata, never the
//! saved bearer credential. Workspace requests never fall back to local calls.
pub mod attachments;
pub mod protocol;
mod ssh;
pub mod ssh_askpass;

use ssh::{Job, Tunnel, Tunnels};
pub use ssh::{JobView, Prompt, SshTarget};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub struct Connections {
    data_dir: PathBuf,
    store: Mutex<()>,
    tunnels: Tunnels,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

impl Connections {
    /// Uses the same connection store as the Tauri desktop.
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_owned(),
            store: Mutex::new(()),
            tunnels: Tunnels::default(),
            jobs: Mutex::new(HashMap::new()),
        }
    }

    fn store_path(&self) -> PathBuf {
        self.data_dir.join("remote-machines.json")
    }

    pub fn machine_for_environment(&self, environment_id: &str) -> Result<Machine, String> {
        self.list()?
            .into_iter()
            .find(|machine| machine.environment_id == environment_id)
            .ok_or_else(|| "Machine is no longer connected".into())
    }

    pub fn shutdown(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.cancel();
            }
        }
        self.tunnels.clear();
    }
    /// Records a machine reached directly at `endpoint`, as `connect` would
    /// after pairing, for tests elsewhere in the crate.
    #[cfg(test)]
    pub(crate) fn add_test_machine(&self, id: &str, environment_id: &str, endpoint: &str) {
        let mut machines = read(&self.store_path()).unwrap();
        machines.push(StoredMachine {
            id: id.into(),
            name: id.into(),
            endpoint: endpoint.into(),
            environment_id: environment_id.into(),
            token: "t".into(),
            ssh: None,
        });
        write(&self.store_path(), &machines).unwrap();
    }
    fn job(&self, id: &str) -> Result<Arc<Job>, String> {
        self.jobs
            .lock()
            .map_err(|_| "Connection setup is unavailable")?
            .get(id)
            .cloned()
            .ok_or_else(|| "Connection setup has expired".into())
    }
}

impl std::fmt::Debug for Connections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connections")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        self.shutdown();
    }
}

static DEFAULT_CONNECTIONS: OnceLock<RwLock<Option<Arc<Connections>>>> = OnceLock::new();

/// Installs the shared transport used by path-based workspace services.
pub fn set_default_connections(connections: Arc<Connections>) {
    let slot = DEFAULT_CONNECTIONS.get_or_init(|| RwLock::new(None));
    *slot
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(connections);
}

pub fn default_connections() -> Result<Arc<Connections>, String> {
    DEFAULT_CONNECTIONS
        .get_or_init(|| RwLock::new(None))
        .read()
        .map_err(|_| "Remote connections are unavailable")?
        .clone()
        .ok_or_else(|| "Remote connections are unavailable".into())
}

pub const REMOTE_PATH_PREFIX: &str = "remote://";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemotePath {
    pub environment_id: String,
    pub host_path: String,
}

/// Qualifies a host path without losing Windows drive letters or UNC roots.
pub fn remote_path(environment_id: &str, host_path: &str) -> String {
    let path = host_path.replace('\\', "/");
    let path = if path.starts_with("//") {
        &path[1..]
    } else {
        path.trim_start_matches('/')
    };
    format!("{REMOTE_PATH_PREFIX}{environment_id}/{path}")
}

pub fn parse_remote_path(path: &str) -> Option<RemotePath> {
    let path = path.strip_prefix(REMOTE_PATH_PREFIX)?.replace('\\', "/");
    let (environment_id, host_path) = path.split_once('/')?;
    if environment_id.is_empty() {
        return None;
    }
    let bytes = host_path.as_bytes();
    let drive = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/');
    Some(RemotePath {
        environment_id: environment_id.into(),
        host_path: if drive {
            host_path.into()
        } else {
            format!("/{host_path}")
        },
    })
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredMachine {
    id: String,
    name: String,
    endpoint: String,
    environment_id: String,
    token: String,
    #[serde(default)]
    ssh: Option<SshTarget>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub environment_id: String,
    pub ssh: Option<SshTarget>,
}

impl StoredMachine {
    fn public(&self) -> Machine {
        Machine {
            id: self.id.clone(),
            name: self.name.clone(),
            endpoint: self.endpoint.clone(),
            environment_id: self.environment_id.clone(),
            ssh: self.ssh.clone(),
        }
    }
}

fn endpoint(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value.trim()).map_err(|_| "Enter a valid host URL")?;
    let host = url.host_str().ok_or("Host URL has no hostname")?;
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err("Use HTTPS, or an HTTP loopback address forwarded through SSH".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Host URL must contain only the scheme, hostname, and port".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn read(path: &Path) -> Result<Vec<StoredMachine>, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Remote connection store is invalid".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

fn write(path: &Path, machines: &[StoredMachine]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing connection directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!("remote-machines-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(machines).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

const INCOMPLETE: &str = "The host request did not complete. Retry to confirm its result.";

/// Methods that change nothing on the host, so a request that never reached
/// it and one that did look the same afterwards.
fn read_only_method(method: &str) -> bool {
    matches!(
        method,
        "environment.describe"
            | "projects.list"
            | "projects.browse"
            | "models.list"
            | "sessions.list"
            | "sessions.sync"
            | "sessions.syncChunk"
            | "attachments.read"
            | "git.diff"
            | "git.branches"
            | "git.worktrees"
            | "git.index"
            | "git.fileDiff"
            | "files.read"
            | "files.list"
            | "files.index"
            | "files.search"
            | "files.searchContent"
    )
}

/// Explains a request through an SSH tunnel that was reset before any
/// response, using the forward OpenSSH refused meanwhile (`refusal`).
///
/// The refusal log is shared by every request on the tunnel, so it may belong
/// to a concurrent request. A read-only request is rewritten to name the cause.
/// A request that may change something keeps its "retry to confirm" advice,
/// because it may have reached the host before another request was refused;
/// the cause is only added as a likely reason.
fn explain_incomplete(
    error: String,
    machine: &StoredMachine,
    method: &str,
    refusal: impl FnOnce() -> Option<ssh::ForwardRefusal>,
) -> String {
    use ssh::ForwardRefusal;
    let Some(target) = machine.ssh.as_ref() else {
        return error;
    };
    if error != INCOMPLETE {
        return error;
    }
    let Some(refusal) = refusal() else {
        return error;
    };
    let (name, port) = (&machine.name, target.remote_port);
    if !read_only_method(method) {
        let cause = match refusal {
            ForwardRefusal::NotListening => {
                format!("nothing answers on port {port}, so MonoCode Host may not be running")
            }
            ForwardRefusal::Prohibited => {
                format!("its SSH server does not allow forwarding to port {port}")
            }
            ForwardRefusal::Other(reason) => {
                format!("its SSH server could not open port {port} ({reason})")
            }
        };
        return format!("{INCOMPLETE} OpenSSH reported that on {name} {cause}.");
    }
    match refusal {
        ForwardRefusal::NotListening => format!(
            "MonoCode Host is not running on {name}. SSH connects, but nothing answers on the host's port {port}. \
             Start it on the machine with `monocode-host service install --port {port}`, then retry."
        ),
        ForwardRefusal::Prohibited => format!(
            "The SSH server on {name} does not allow forwarding to the host's port {port} \
             (OpenSSH: administratively prohibited). Allow TCP forwarding for this account \
             (`AllowTcpForwarding`, and `PermitOpen` must include 127.0.0.1:{port}), then reconnect."
        ),
        ForwardRefusal::Other(reason) => format!(
            "The SSH server on {name} could not open the host's port {port} (OpenSSH: {reason}). \
             Retry, or reconnect from Settings → Connections."
        ),
    }
}

/// Hosts split large session syncs into pieces below this cap
/// (`host/sync-transfer.ts`), so it bounds memory without limiting transcripts.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

fn rpc(
    endpoint: &str,
    token: &str,
    environment_id: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let payload = json!({ "version": 1, "environmentId": environment_id, "method": method, "params": params });
    let response = agent
        .post(&format!("{endpoint}/rpc"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string());
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(error)) if connection_refused(&error) => {
            return Err(
                "Machine is unreachable. Check the host and SSH tunnel, then reconnect.".into(),
            )
        }
        Err(_) => return Err(INCOMPLETE.into()),
    };
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("Host response is too large".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid host response")?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("Host rejected request: {error}"));
    }
    if status != 200 {
        return Err(format!("Host returned HTTP {status}"));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| "Invalid host response".into())
}

// Only a refused connection proves the local forwarding listener is gone.
// A slow request or an interrupted response says nothing about SSH health.
fn connection_refused(error: &ureq::Transport) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused)
        {
            return true;
        }
        source = error.source();
    }
    false
}

impl Connections {
    pub fn list(&self) -> Result<Vec<Machine>, String> {
        let _guard = self
            .store
            .lock()
            .map_err(|_| "Connection store is locked")?;
        Ok(read(&self.store_path())?
            .iter()
            .map(StoredMachine::public)
            .collect())
    }

    pub fn connect(&self, name: String, url: String, token: String) -> Result<Machine, String> {
        let endpoint = endpoint(&url)?;
        let token = token.trim().to_string();
        if token.len() != 43
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("Enter the device token issued by monocode-host pair".into());
        }
        let descriptor = rpc(
            &endpoint,
            &token,
            None,
            "environment.describe",
            json!({"supportedProviders":protocol::supported_providers()}),
        )?;
        if descriptor.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
            return Err("This host uses an incompatible protocol version".into());
        }
        let environment_id = descriptor
            .get("environmentId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("Host identity is missing")?
            .to_string();
        let name = if name.trim().is_empty() {
            descriptor
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Remote machine")
                .to_string()
        } else {
            name.trim().chars().take(100).collect()
        };
        let _guard = self
            .store
            .lock()
            .map_err(|_| "Connection store is locked")?;
        let path = self.store_path();
        let mut machines = read(&path)?;
        let existing = machines.iter().find(|m| m.environment_id == environment_id);
        let id = existing
            .map(|m| m.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let machine = StoredMachine {
            id: id.clone(),
            name,
            endpoint,
            environment_id,
            token,
            ssh: existing.and_then(|m| m.ssh.clone()),
        };
        machines.retain(|m| m.id != id);
        machines.push(machine.clone());
        write(&path, &machines)?;
        Ok(machine.public())
    }

    pub fn disconnect(&self, machine_id: &str) -> Result<(), String> {
        let _guard = self
            .store
            .lock()
            .map_err(|_| "Connection store is locked")?;
        let path = self.store_path();
        let mut machines = read(&path)?;
        machines.retain(|m| m.id != machine_id);
        write(&path, &machines)?;
        self.tunnels.remove(machine_id);
        Ok(())
    }

    pub fn request(
        &self,
        machine_id: &str,
        method: &str,
        mut params: Value,
    ) -> Result<Value, String> {
        if method == "environment.describe" {
            let values = params
                .as_object_mut()
                .ok_or("Invalid host descriptor parameters")?;
            values
                .entry("supportedProviders")
                .or_insert_with(|| json!(protocol::supported_providers()));
        }
        if !supported_remote_method(method) {
            return Err("Unsupported remote operation".into());
        }
        let machine = {
            let _guard = self
                .store
                .lock()
                .map_err(|_| "Connection store is locked")?;
            read(&self.store_path())?
                .into_iter()
                .find(|m| m.id == machine_id)
                .ok_or("Machine is no longer connected")?
        };
        let tunnel_lease = if let Some(target) = &machine.ssh {
            Some(self.tunnels.endpoint(&machine.id, target)?)
        } else {
            None
        };
        let endpoint = tunnel_lease
            .as_ref()
            .map(|lease| lease.endpoint.as_str())
            .unwrap_or(&machine.endpoint);
        let before = tunnel_lease
            .as_ref()
            .map(ssh::TunnelLease::forward_failures);
        let response = rpc(
            endpoint,
            &machine.token,
            Some(&machine.environment_id),
            method,
            params,
        )
        .map_err(|error| {
            explain_incomplete(error, &machine, method, || {
                tunnel_lease
                    .as_ref()
                    .zip(before)
                    .and_then(|(lease, before)| lease.forward_refused_since(before))
            })
        });
        if response
            .as_ref()
            .err()
            .is_some_and(|error| error.starts_with("Machine is unreachable"))
        {
            if let Some(lease) = &tunnel_lease {
                self.tunnels.invalidate(&machine.id, lease);
            }
        }
        let result = response?;
        if method == "environment.describe"
            && result.get("environmentId").and_then(Value::as_str)
                != Some(machine.environment_id.as_str())
        {
            return Err("Host identity changed. Add this machine again before continuing.".into());
        }
        Ok(result)
    }
}

fn supported_remote_method(method: &str) -> bool {
    matches!(
        method,
        "environment.describe"
            | "projects.list"
            | "projects.browse"
            | "projects.open"
            | "models.list"
            | "sessions.list"
            | "sessions.update"
            | "sessions.delete"
            | "sessions.sync"
            | "sessions.syncChunk"
            | "commands.dispatch"
            | "attachments.upload"
            | "attachments.read"
            | "devices.revokeSelf"
            | "git.diff"
            | "git.branches"
            | "git.switch"
            | "git.createBranch"
            | "git.worktrees"
            | "git.worktreeCreate"
            | "files.read"
            | "files.list"
            | "files.index"
            | "workspace.run"
            | "files.search"
            | "files.searchContent"
            | "files.create"
            | "files.write"
            | "git.index"
            | "git.fileDiff"
            | "git.action"
    )
}

impl Connections {
    fn start_ssh_job(
        self: &Arc<Self>,
        target: SshTarget,
        name: String,
        existing: Option<StoredMachine>,
        upgrade: bool,
    ) -> Result<String, String> {
        let job = Job::new();
        let id = job.view().id;
        {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| "Connection setup is unavailable")?;
            if jobs.values().any(|job| !job.view().done) {
                return Err(
                    "Another SSH connection is being set up. Finish or cancel it first.".into(),
                );
            }
            jobs.retain(|_, job| !job.view().done);
            jobs.insert(id.clone(), job.clone());
        }
        let state = self.clone();
        std::thread::spawn(move || {
            let prepared = (|| -> Result<(StoredMachine, Tunnel), String> {
                let askpass = job.askpass()?;
                let mut target = target;
                let mut machine = if let Some(mut existing) = existing {
                    if upgrade {
                        let platform = ssh::detect_platform(&target, &job, &askpass)?;
                        job.message("Updating MonoCode Host on the machine…");
                        let output = ssh::run_script(
                            &target,
                            platform,
                            ssh::upgrade_script(platform, target.remote_port),
                            &job,
                            &askpass,
                        )?;
                        let info: Value = serde_json::from_str(output.lines().last().unwrap_or(""))
                            .map_err(|_| "Host update returned an invalid response")?;
                        target.remote_port = info
                            .get("port")
                            .and_then(Value::as_u64)
                            .and_then(|v| u16::try_from(v).ok())
                            .filter(|p| *p > 0)
                            .ok_or("Host did not report a valid port")?;
                        existing.ssh = Some(target.clone());
                    } else {
                        job.message("Reconnecting to the machine…");
                    }
                    existing
                } else {
                    let platform = ssh::detect_platform(&target, &job, &askpass)?;
                    job.message("Installing or starting MonoCode Host…");
                    let output = ssh::run_script(
                        &target,
                        platform,
                        ssh::bootstrap_script(platform),
                        &job,
                        &askpass,
                    )?;
                    let info: Value = serde_json::from_str(output.lines().last().unwrap_or(""))
                        .map_err(|_| "Host setup returned an invalid response")?;
                    target.remote_port = info
                        .get("port")
                        .and_then(Value::as_u64)
                        .and_then(|v| u16::try_from(v).ok())
                        .filter(|p| *p > 0)
                        .ok_or("Host did not report a valid port")?;
                    job.message("Pairing this desktop with the host…");
                    let pair = ssh::run_script(
                        &target,
                        platform,
                        ssh::pairing_script(platform, &ssh::device_name()),
                        &job,
                        &askpass,
                    )?;
                    let pair: Value = serde_json::from_str(pair.lines().last().unwrap_or(""))
                        .map_err(|_| "Host pairing returned an invalid response")?;
                    let token = pair
                        .get("token")
                        .and_then(Value::as_str)
                        .filter(|s| {
                            s.len() == 43
                                && s.bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                        })
                        .ok_or("Host returned an invalid device credential")?
                        .to_string();
                    let environment_id = pair
                        .get("environmentId")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or("Host identity is missing")?
                        .to_string();
                    StoredMachine {
                        id: uuid::Uuid::new_v4().to_string(),
                        name,
                        endpoint: format!("ssh://{}", target.target),
                        environment_id,
                        token,
                        ssh: Some(target.clone()),
                    }
                };
                job.message("Opening the secure connection…");
                let tunnel = Tunnel::start(&target, Some(&job), Some(&askpass))?;
                let descriptor = rpc(
                    &format!("http://127.0.0.1:{}", tunnel.port),
                    &machine.token,
                    Some(&machine.environment_id),
                    "environment.describe",
                    json!({"supportedProviders":protocol::supported_providers()}),
                )?;
                if descriptor.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
                    return Err("This host uses an incompatible protocol version".into());
                }
                if descriptor.get("environmentId").and_then(Value::as_str)
                    != Some(&machine.environment_id)
                {
                    return Err(
                        "Host identity changed. Remove this connection and add the machine again."
                            .into(),
                    );
                }
                if upgrade {
                    let capabilities = descriptor.get("capabilities").and_then(Value::as_array);
                    let supports = |name: &str| {
                        capabilities.is_some_and(|entries| {
                            entries.iter().any(|entry| entry.as_str() == Some(name))
                        })
                    };
                    if !supports("git.index")
                        || !supports("files.list")
                        || !supports("files.create")
                        || !supports("files.searchContent")
                    {
                        return Err("The installed host package still lacks Explorer and Changes. Install a newer MonoCode release with updated host packages.".into());
                    }
                }
                if machine.name.trim().is_empty() {
                    machine.name = descriptor
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&target.target)
                        .chars()
                        .take(100)
                        .collect();
                }
                Ok((machine, tunnel))
            })();
            job.complete(|| {
                let (mut machine, tunnel) = prepared?;
                let _guard = state
                    .store
                    .lock()
                    .map_err(|_| "Connection store is locked")?;
                let path = state.store_path();
                let mut machines = read(&path)?;
                if let Some(old) = machines
                    .iter()
                    .find(|m| m.environment_id == machine.environment_id)
                {
                    machine.id = old.id.clone();
                }
                machines.retain(|m| m.id != machine.id);
                machines.push(machine.clone());
                write(&path, &machines)?;
                state.tunnels.insert(machine.id.clone(), tunnel);
                Ok(machine.public())
            });
        });
        Ok(id)
    }

    pub fn ssh_begin(
        self: &Arc<Self>,
        target: String,
        name: String,
        port: Option<u16>,
    ) -> Result<String, String> {
        let target = ssh::validate_target(&target, port)?;
        self.start_ssh_job(
            SshTarget {
                target,
                port,
                remote_port: 3774,
            },
            name.trim().chars().take(100).collect(),
            None,
            false,
        )
    }

    pub fn ssh_reconnect(
        self: &Arc<Self>,
        machine_id: &str,
        upgrade: bool,
    ) -> Result<String, String> {
        let machine = {
            let _guard = self
                .store
                .lock()
                .map_err(|_| "Connection store is locked")?;
            read(&self.store_path())?
                .into_iter()
                .find(|m| m.id == machine_id)
                .ok_or("Machine is no longer connected")?
        };
        let target = machine
            .ssh
            .clone()
            .ok_or("This connection does not use SSH")?;
        self.start_ssh_job(target, machine.name.clone(), Some(machine), upgrade)
    }

    pub fn ssh_poll(&self, job_id: &str) -> Result<JobView, String> {
        Ok(self.job(job_id)?.view())
    }

    pub fn ssh_answer(&self, job_id: &str, prompt_id: &str, answer: String) -> Result<(), String> {
        self.job(job_id)?.answer(prompt_id, answer)
    }

    pub fn ssh_cancel(&self, job_id: &str) -> Result<(), String> {
        self.job(job_id)?.cancel();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ReceivedRequest {
        headers: String,
        payload: Value,
    }

    fn host_fixture(
        responses: Vec<Value>,
    ) -> (
        String,
        std::sync::mpsc::Receiver<ReceivedRequest>,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (sender, receiver) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut headers = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        if key.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap();
                        }
                    }
                    headers.push_str(&line);
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let _ = sender.send(ReceivedRequest {
                    headers,
                    payload: serde_json::from_slice(&body).unwrap(),
                });
                let body = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        (endpoint, receiver, server)
    }

    #[test]
    fn existing_store_remains_compatible_and_credentials_are_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote-machines.json");
        std::fs::write(&path, r#"[{"id":"old","name":"server","endpoint":"https://host.example","environmentId":"env","token":"private"}]"#).unwrap();
        let connections = Connections::new(dir.path());
        let machine = connections.machine_for_environment("env").unwrap();
        assert_eq!(machine.id, "old");
        assert_eq!(machine.ssh, None);
        assert!(!serde_json::to_string(&machine).unwrap().contains("private"));
        assert!(!format!("{connections:?}").contains("private"));
        connections.disconnect("old").unwrap();
        assert!(connections.list().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn connecting_same_host_keeps_identity_and_scopes_authenticated_requests() {
        let descriptor =
            json!({ "result": { "protocolVersion": 1, "environmentId": "env", "name": "Server" }});
        let (url, requests, server) = host_fixture(vec![
            descriptor.clone(),
            descriptor,
            json!({ "result": { "projects": [] } }),
            json!({ "result": { "protocolVersion": 1, "environmentId": "replacement" } }),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let connections = Connections::new(dir.path());
        let token = "a".repeat(43);
        let first = connections
            .connect("".into(), url.clone(), token.clone())
            .unwrap();
        assert_eq!(first.name, "Server");
        let second = connections
            .connect(" New name ".into(), url, token.clone())
            .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.name, "New name");
        assert_eq!(connections.list().unwrap().len(), 1);
        assert_eq!(
            connections
                .request(&first.id, "projects.list", json!({}))
                .unwrap(),
            json!({ "projects": [] })
        );
        let error = connections
            .request(&first.id, "environment.describe", json!({}))
            .unwrap_err();
        assert!(error.starts_with("Host identity changed"));
        server.join().unwrap();
        let requests: Vec<_> = requests.iter().collect();
        assert_eq!(requests.len(), 4);
        for request in &requests {
            assert!(request.headers.starts_with("POST /rpc HTTP/1.1"));
            assert!(request
                .headers
                .contains(&format!("Authorization: Bearer {token}")));
            assert_eq!(request.payload["version"], 1);
        }
        assert_eq!(requests[0].payload["environmentId"], Value::Null);
        assert_eq!(requests[2].payload["environmentId"], "env");
        assert_eq!(requests[2].payload["method"], "projects.list");
        assert!(requests[3].payload["params"]["supportedProviders"]
            .as_array()
            .unwrap()
            .contains(&json!("devin")));
    }

    #[test]
    fn incompatible_pairing_does_not_create_a_machine() {
        let (url, _, server) = host_fixture(vec![
            json!({ "result": { "protocolVersion": 2, "environmentId": "env" } }),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let connections = Connections::new(dir.path());
        assert!(connections
            .connect("".into(), url, "a".repeat(43))
            .unwrap_err()
            .contains("incompatible"));
        server.join().unwrap();
        assert!(connections.list().unwrap().is_empty());
        assert!(!dir.path().join("remote-machines.json").exists());
    }

    #[test]
    fn invalid_store_and_unknown_remote_operations_never_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let connections = Connections::new(dir.path());
        assert_eq!(
            connections
                .request("missing", "git.arbitrary", json!({}))
                .unwrap_err(),
            "Unsupported remote operation"
        );
        assert_eq!(
            connections
                .request("missing", "files.read", json!({ "path": "/tmp/private" }))
                .unwrap_err(),
            "Machine is no longer connected"
        );
        std::fs::write(connections.store_path(), "invalid").unwrap();
        assert!(connections.list().is_err());
        assert!(connections.disconnect("missing").is_err());
        assert_eq!(
            std::fs::read_to_string(connections.store_path()).unwrap(),
            "invalid"
        );
    }

    #[test]
    fn remote_paths_round_trip_unix_drives_and_unc_roots() {
        for (host, normalized, qualified) in [
            (
                "/home/user/repo",
                "/home/user/repo",
                "remote://env/home/user/repo",
            ),
            (
                r"C:\Users\Nick\repo",
                "C:/Users/Nick/repo",
                "remote://env/C:/Users/Nick/repo",
            ),
            (
                r"\\server\share\repo",
                "//server/share/repo",
                "remote://env//server/share/repo",
            ),
            ("/", "/", "remote://env/"),
            ("C:", "C:", "remote://env/C:"),
        ] {
            assert_eq!(remote_path("env", host), qualified);
            let parsed = parse_remote_path(qualified).unwrap();
            assert_eq!(parsed.environment_id, "env");
            assert_eq!(parsed.host_path, normalized);
        }
        assert!(parse_remote_path("/local/repo").is_none());
        assert!(parse_remote_path("remote://env").is_none());
        assert!(parse_remote_path("remote:///repo").is_none());
    }

    #[test]
    fn a_refused_ssh_forward_says_the_host_is_not_running() {
        use std::{net::TcpListener, thread};
        // OpenSSH accepts the local connection, then resets it when the host
        // port refuses. The client sees an incomplete request, not a refusal.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            // Closing with unread request bytes makes the OS send a reset.
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0; 1]);
            drop(stream);
        });
        let error = rpc(&url, "t", Some("env"), "environment.describe", json!({})).unwrap_err();
        server.join().unwrap();
        assert_eq!(error, INCOMPLETE);
        let machine = |ssh| StoredMachine {
            id: "m".into(),
            name: "VPS".into(),
            endpoint: "ssh://vps".into(),
            environment_id: "env".into(),
            token: String::new(),
            ssh,
        };
        let tunneled = machine(Some(SshTarget {
            target: "vps".into(),
            port: None,
            remote_port: 3774,
        }));
        use ssh::ForwardRefusal::{NotListening, Other, Prohibited};
        let explain = |error: &str, machine: &StoredMachine, method, refusal| {
            explain_incomplete(error.into(), machine, method, || refusal)
        };
        let explained = explain(
            &error,
            &tunneled,
            "environment.describe",
            Some(NotListening),
        );
        assert!(
            explained.starts_with("MonoCode Host is not running on VPS.")
                && explained.contains("port 3774")
                && explained.contains("service install --port 3774")
                && !explained.contains("Retry to confirm"),
            "{explained}"
        );
        // A server that forbids the forward says nothing about the host.
        let explained = explain(&error, &tunneled, "sessions.list", Some(Prohibited));
        assert!(
            explained.contains("does not allow forwarding to the host's port 3774")
                && explained.contains("AllowTcpForwarding")
                && !explained.contains("not running"),
            "{explained}"
        );
        let explained = explain(
            &error,
            &tunneled,
            "models.list",
            Some(Other("resource shortage".into())),
        );
        assert!(
            explained.contains("resource shortage") && !explained.contains("not running"),
            "{explained}"
        );
        // A request that may have changed something keeps its retry advice:
        // the refusal may belong to another request on the same tunnel.
        for method in ["commands.dispatch", "sessions.update", "sessions.delete"] {
            let explained = explain(&error, &tunneled, method, Some(NotListening));
            assert!(
                explained.starts_with(INCOMPLETE)
                    && explained.contains("MonoCode Host may not be running"),
                "{method}: {explained}"
            );
        }
        // Without OpenSSH's proof the result stays unknown, so keep the retry advice.
        assert_eq!(
            explain(&error, &tunneled, "environment.describe", None),
            INCOMPLETE
        );
        assert_eq!(
            explain(
                &error,
                &machine(None),
                "environment.describe",
                Some(NotListening)
            ),
            INCOMPLETE
        );
        assert_eq!(
            explain(
                "Host returned HTTP 500",
                &tunneled,
                "environment.describe",
                Some(NotListening)
            ),
            "Host returned HTTP 500"
        );
    }

    /// `Connections::request` through a tunnel: OpenSSH's refusal line
    /// arriving after the reset is attributed to the request, by reason and
    /// method, and a refusal from before the request is not.
    #[cfg(unix)]
    #[test]
    fn a_tunneled_request_reads_the_refusal_openssh_logs_for_it() {
        use std::{io::Write as _, net::TcpListener, thread};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stderr, mut openssh) = std::io::pipe().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let connections = Connections::new(dir.path());
        write(
            &connections.store_path(),
            &[StoredMachine {
                id: "m".into(),
                name: "VPS".into(),
                endpoint: "ssh://vps".into(),
                environment_id: "env".into(),
                token: "t".into(),
                ssh: Some(SshTarget {
                    target: "vps".into(),
                    port: None,
                    remote_port: 3774,
                }),
            }],
        )
        .unwrap();
        connections
            .tunnels
            .insert("m".into(), Tunnel::for_test(port, stderr));
        // A refusal logged before the request belongs to an earlier one.
        writeln!(
            openssh,
            "channel 1: open failed: connect failed: Connection refused"
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        // Each accepted connection is reset, then OpenSSH logs `lines[i]`.
        let lines = [
            None,
            Some("channel 2: open failed: connect failed: Connection refused"),
            Some("channel 3: open failed: administratively prohibited: open failed"),
            Some("channel 4: open failed: connect failed: Connection refused"),
        ];
        let server = thread::spawn(move || {
            for line in lines {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.read(&mut [0; 1]);
                drop(stream);
                if let Some(line) = line {
                    writeln!(openssh, "{line}").unwrap();
                }
            }
        });
        let request = |method| connections.request("m", method, json!({})).unwrap_err();
        assert_eq!(request("sessions.list"), INCOMPLETE);
        let error = request("sessions.list");
        assert!(
            error.starts_with("MonoCode Host is not running on VPS."),
            "{error}"
        );
        let error = request("environment.describe");
        assert!(
            error.contains("does not allow forwarding") && !error.contains("not running"),
            "{error}"
        );
        let error = request("commands.dispatch");
        assert!(
            error.starts_with(INCOMPLETE) && error.contains("may not be running"),
            "{error}"
        );
        server.join().unwrap();
    }

    #[test]
    fn a_slow_host_does_not_look_like_a_dead_tunnel() {
        use std::{net::TcpListener, thread, time::Duration};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(200));
        });
        let error = ureq::get(&url)
            .timeout(Duration::from_millis(50))
            .call()
            .unwrap_err();
        let ureq::Error::Transport(error) = error else {
            panic!("Expected a timeout")
        };
        assert!(!connection_refused(&error));
        server.join().unwrap();
        // A freed ephemeral port can be claimed by another parallel fixture.
        // Exercise ureq's actual wrapped I/O source without that port race.
        let error = ureq::Error::from(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        let ureq::Error::Transport(error) = error else {
            panic!("Expected a refused connection")
        };
        assert!(connection_refused(&error));
    }

    #[test]
    fn desktop_forwards_supported_host_operations() {
        for method in [
            "git.branches",
            "git.switch",
            "git.createBranch",
            "git.worktrees",
            "git.worktreeCreate",
            "attachments.upload",
            "attachments.read",
        ] {
            assert!(supported_remote_method(method), "{method}");
        }
        assert!(!supported_remote_method("git.arbitrary"));
    }
    #[test]
    fn endpoints_require_an_encrypted_route_and_no_embedded_secrets() {
        assert_eq!(
            endpoint("http://127.0.0.1:3774/").unwrap(),
            "http://127.0.0.1:3774"
        );
        assert!(endpoint("http://[::1]:3774").is_ok());
        assert!(endpoint("https://host.example").is_ok());
        for url in [
            "http://192.168.1.5:3774",
            "file:///tmp/socket",
            "https://user:secret@host.example",
            "https://host.example/rpc",
            "https://host.example?token=secret",
            "https://host.example#secret",
        ] {
            assert!(endpoint(url).is_err(), "{url}");
        }
    }
    #[test]
    fn public_machine_metadata_never_contains_the_token() {
        let stored = StoredMachine {
            id: "id".into(),
            name: "server".into(),
            endpoint: "https://host.example".into(),
            environment_id: "env".into(),
            token: "secret".into(),
            ssh: None,
        };
        let value = serde_json::to_string(&stored.public()).unwrap();
        assert!(!value.contains("secret"));
        assert!(!value.contains("token"));
    }
}
