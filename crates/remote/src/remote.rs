//! Moved from src-tauri/src/remote.rs.
//!
//! Remote host connections. The renderer receives machine metadata, never the
//! saved bearer credential. Workspace requests never fall back to local calls.
//!
//! A machine is reached by the first route that answers: its direct TLS
//! addresses, whose certificate the pairing link pinned, then an SSH forward
//! to the host's loopback port when the machine was set up over SSH.
use crate::remote_ssh::{self, Job, JobView, SshTarget, Tunnel, TunnelLease, Tunnels};
use crate::remote_tls;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Saved machines and live connections. Clones share state. The machine list
/// lives in `<data_dir>/remote-machines.json`.
#[derive(Clone)]
pub struct Remote {
    connections: Arc<RemoteConnections>,
    data_dir: PathBuf,
}

impl Remote {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            connections: Arc::new(RemoteConnections::default()),
            data_dir,
        }
    }

    pub fn connections(&self) -> &RemoteConnections {
        &self.connections
    }

    /// Cancel SSH setup jobs and close every tunnel.
    pub fn shutdown(&self) {
        self.connections.shutdown();
    }
}

#[derive(Default)]
pub struct RemoteConnections {
    store: Mutex<()>,
    tunnels: Tunnels,
    routes: Mutex<HashMap<String, Arc<Mutex<RouteSlot>>>>,
    agents: Mutex<HashMap<String, ureq::Agent>>,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

impl RemoteConnections {
    pub fn shutdown(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.cancel();
            }
        }
        self.tunnels.clear();
    }
    fn job(&self, id: &str) -> Result<Arc<Job>, String> {
        self.jobs
            .lock()
            .map_err(|_| "Connection setup is unavailable")?
            .get(id)
            .cloned()
            .ok_or_else(|| "Connection setup has expired".into())
    }
    /// One pooled agent per pinned certificate, so requests reuse TLS
    /// connections. `None` is plain HTTP through a loopback SSH forward.
    fn agent(&self, fingerprint: Option<&str>) -> Result<ureq::Agent, String> {
        let key = fingerprint.unwrap_or("").to_string();
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(agent) = agents.get(&key) {
            return Ok(agent.clone());
        }
        // Long enough for a `changes.wait` long poll (25 s).
        let agent = remote_tls::agent(fingerprint, CONNECT_TIMEOUT, Duration::from_secs(40))?;
        agents.insert(key, agent.clone());
        Ok(agent)
    }
    fn slot(&self, machine_id: &str) -> Arc<Mutex<RouteSlot>> {
        self.routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(machine_id.into())
            .or_default()
            .clone()
    }
    fn set_route(&self, machine_id: &str, route: Route) {
        let slot = self.slot(machine_id);
        let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
        slot.route = Some(route);
        slot.failure = None;
    }
    fn forget(&self, machine_id: &str) {
        self.routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(machine_id);
    }
}

/// A direct address that does not accept a connection this quickly is
/// treated as unreachable from this network, and the next route is tried.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// After every route failed, requests report that failure for this long
/// instead of probing each address again.
const FAILURE_RETRY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq)]
enum Route {
    Direct(String),
    Ssh,
}

#[derive(Default)]
struct RouteSlot {
    route: Option<Route>,
    failure: Option<(Instant, String)>,
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
    /// Direct `https://` addresses, most recently working first.
    #[serde(default)]
    endpoints: Vec<String>,
    /// The pinned SHA-256 of the host certificate, base64url.
    #[serde(default)]
    fingerprint: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    id: String,
    name: String,
    endpoint: String,
    endpoints: Vec<String>,
    environment_id: String,
    ssh: Option<SshTarget>,
}

impl StoredMachine {
    fn public(&self) -> Machine {
        Machine {
            id: self.id.clone(),
            name: self.name.clone(),
            endpoint: self.endpoint.clone(),
            endpoints: self.endpoints.clone(),
            environment_id: self.environment_id.clone(),
            ssh: self.ssh.clone(),
        }
    }
    fn candidates(&self) -> Vec<Route> {
        let mut routes: Vec<Route> = if self.fingerprint.is_some() {
            self.endpoints.iter().cloned().map(Route::Direct).collect()
        } else {
            Vec::new()
        };
        if self.ssh.is_some() {
            routes.push(Route::Ssh);
        }
        routes
    }
}

fn display(endpoints: &[String], ssh: Option<&SshTarget>) -> String {
    if let Some(first) = endpoints.first() {
        return first.trim_start_matches("https://").to_string();
    }
    ssh.map(|target| format!("ssh://{}", target.target))
        .unwrap_or_default()
}

/// A direct host address from a pairing link or a host descriptor: HTTPS
/// with only a host and port. Certificates are pinned, so any hostname or IP
/// works.
fn direct_endpoint(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value.trim()).map_err(|_| "Invalid host address")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Host addresses must be https:// with only a hostname and port".into());
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

/// Hosts split large session syncs into pieces below this cap
/// (`host/sync-transfer.ts`), so it bounds memory without limiting transcripts.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

/// Why a request failed, which decides whether another route may be tried.
#[derive(Debug)]
enum Failure {
    /// No request bytes reached the host: the connection or TLS handshake
    /// failed. Safe to retry on another route.
    Unreachable(String),
    /// The request may have reached the host. Commands must not be resent;
    /// the renderer confirms them by command ID.
    Uncertain(String),
    /// The host answered and refused.
    Rejected(String),
    /// The host turned the request away before acting on it (HTTP 429), as
    /// its pairing throttle does. Another route may still succeed.
    Throttled(String),
}

impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Unreachable(e)
            | Failure::Uncertain(e)
            | Failure::Rejected(e)
            | Failure::Throttled(e) => e,
        }
    }
}

fn call(
    agent: &ureq::Agent,
    base: &str,
    token: Option<&str>,
    environment_id: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, Failure> {
    let payload = json!({ "version": 1, "environmentId": environment_id, "method": method, "params": params });
    let mut request = agent
        .post(&format!("{base}/rpc"))
        .set("Content-Type", "application/json");
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    let response = match request.send_string(&payload.to_string()) {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(error)) => {
            let place = base
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            if let Some(mismatch) = remote_tls::handshake_failure(&error) {
                return Err(Failure::Unreachable(if mismatch {
                    format!(
                        "{place} presented a different certificate than the one this desktop paired with"
                    )
                } else {
                    format!("{place} failed the TLS handshake")
                }));
            }
            return Err(match error.kind() {
                ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Dns => {
                    Failure::Unreachable(format!(
                        "{place} {}",
                        if connection_refused(&error) {
                            "refused the connection"
                        } else {
                            "did not answer"
                        }
                    ))
                }
                _ => Failure::Uncertain(
                    "The host request did not complete. Retry to confirm its result.".into(),
                ),
            });
        }
    };
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            Failure::Uncertain(
                "The host request did not complete. Retry to confirm its result.".into(),
            )
        })?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(Failure::Rejected("Host response is too large".into()));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::Rejected("Invalid host response".into()))?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        let error = format!("Host rejected request: {error}");
        return Err(if status == 429 {
            Failure::Throttled(error)
        } else {
            Failure::Rejected(error)
        });
    }
    if status != 200 {
        return Err(Failure::Rejected(format!("Host returned HTTP {status}")));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| Failure::Rejected("Invalid host response".into()))
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

fn store_path(remote: &Remote) -> Result<std::path::PathBuf, String> {
    Ok(remote.data_dir.join("remote-machines.json"))
}

fn load_machine(remote: &Remote, machine_id: &str) -> Result<StoredMachine, String> {
    let state = remote.connections();
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    read(&store_path(remote)?)?
        .into_iter()
        .find(|m| m.id == machine_id)
        .ok_or_else(|| "Machine is no longer connected".into())
}

/// Saves a machine, reusing the entry for the same host environment.
fn save_machine(remote: &Remote, mut machine: StoredMachine) -> Result<StoredMachine, String> {
    let state = remote.connections();
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    let path = store_path(remote)?;
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
    Ok(machine)
}

/// A request on one route. Direct routes pin the certificate; the SSH route
/// uses the forward's loopback port.
fn call_route(
    state: &RemoteConnections,
    machine: &StoredMachine,
    route: &Route,
    method: &str,
    params: Value,
) -> Result<Value, Failure> {
    match route {
        Route::Direct(base) => {
            let agent = state
                .agent(machine.fingerprint.as_deref())
                .map_err(Failure::Rejected)?;
            call(
                &agent,
                base,
                Some(&machine.token),
                Some(&machine.environment_id),
                method,
                params,
            )
        }
        Route::Ssh => {
            let target = machine
                .ssh
                .as_ref()
                .ok_or_else(|| Failure::Rejected("This connection does not use SSH".into()))?;
            let lease: TunnelLease = state
                .tunnels
                .endpoint(&machine.id, target)
                .map_err(Failure::Unreachable)?;
            let agent = state.agent(None).map_err(Failure::Rejected)?;
            let result = call(
                &agent,
                &lease.endpoint,
                Some(&machine.token),
                Some(&machine.environment_id),
                method,
                params,
            );
            if matches!(result, Err(Failure::Unreachable(_))) {
                state.tunnels.invalidate(&machine.id, &lease);
            }
            result.map_err(|failure| match failure {
                Failure::Unreachable(_) => Failure::Unreachable(
                    "Machine is unreachable. Check the host and SSH tunnel, then reconnect.".into(),
                ),
                other => other,
            })
        }
    }
}

/// The machine's working route, probing each candidate with
/// `environment.describe` when none is known. Probing also learns the
/// host's current addresses, such as after its IP address changes.
fn resolve_route(remote: &Remote, machine: &StoredMachine) -> Result<Route, String> {
    let state = remote.connections();
    let slot = state.slot(&machine.id);
    let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(route) = &slot.route {
        return Ok(route.clone());
    }
    if let Some((when, error)) = &slot.failure
        && when.elapsed() < FAILURE_RETRY
    {
        return Err(error.clone());
    }
    let candidates = machine.candidates();
    if candidates.is_empty() {
        return Err("This machine has no saved address. Pair it again.".into());
    }
    let mut failures = Vec::new();
    for route in candidates {
        match call_route(state, machine, &route, "environment.describe", json!({})) {
            Ok(descriptor) => {
                if descriptor.get("environmentId").and_then(Value::as_str)
                    != Some(machine.environment_id.as_str())
                {
                    return Err(
                        "Host identity changed. Add this machine again before continuing.".into(),
                    );
                }
                learn_endpoints(remote, machine, &route, &descriptor);
                slot.route = Some(route.clone());
                slot.failure = None;
                return Ok(route);
            }
            Err(Failure::Rejected(error)) | Err(Failure::Throttled(error)) => return Err(error),
            Err(Failure::Unreachable(error)) | Err(Failure::Uncertain(error)) => {
                failures.push(error)
            }
        }
    }
    let error = format!(
        "Machine is unreachable. {}. Check that the host is running (monocode-host connect status) and reachable from this computer.",
        failures.join("; ")
    );
    slot.failure = Some((Instant::now(), error.clone()));
    Err(error)
}

/// Saves the addresses a host reports, keeping the one that answered first.
fn learn_endpoints(remote: &Remote, machine: &StoredMachine, route: &Route, descriptor: &Value) {
    if machine.fingerprint.is_none() {
        return;
    }
    let mut endpoints = Vec::new();
    if let Route::Direct(base) = route {
        endpoints.push(base.clone());
    }
    for value in descriptor
        .get("endpoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(endpoint) = value.as_str().and_then(|v| direct_endpoint(v).ok())
            && !endpoints.contains(&endpoint)
        {
            endpoints.push(endpoint);
        }
    }
    endpoints.truncate(8);
    if endpoints == machine.endpoints || endpoints.is_empty() {
        return;
    }
    let state = remote.connections();
    let Ok(_guard) = state.store.lock() else {
        return;
    };
    let Ok(path) = store_path(remote) else { return };
    let Ok(mut machines) = read(&path) else {
        return;
    };
    if let Some(saved) = machines.iter_mut().find(|m| m.id == machine.id) {
        saved.endpoint = display(&endpoints, saved.ssh.as_ref());
        saved.endpoints = endpoints;
        let _ = write(&path, &machines);
    }
}

fn request(
    remote: &Remote,
    machine_id: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    if !supported_remote_method(method) {
        return Err("Unsupported remote operation".into());
    }
    let state = remote.connections();
    let machine = load_machine(remote, machine_id)?;
    let mut retried = false;
    loop {
        let route = resolve_route(remote, &machine)?;
        match call_route(state, &machine, &route, method, params.clone()) {
            Ok(result) => {
                if method == "environment.describe"
                    && result.get("environmentId").and_then(Value::as_str)
                        != Some(machine.environment_id.as_str())
                {
                    return Err(
                        "Host identity changed. Add this machine again before continuing.".into(),
                    );
                }
                return Ok(result);
            }
            // Nothing reached the host, so the request may go to the next
            // route once, whatever it does.
            Err(Failure::Unreachable(error)) => {
                state.forget(&machine.id);
                if retried {
                    return Err(error);
                }
                retried = true;
            }
            Err(failure) => return Err(failure.into()),
        }
    }
}

pub fn remote_machines(remote: &Remote) -> Result<Vec<Machine>, String> {
    let state = remote.connections();
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    Ok(read(&store_path(remote)?)?
        .iter()
        .map(StoredMachine::public)
        .collect())
}

/// A `monocode://pair` link printed by `monocode-host connect`.
#[derive(Debug, PartialEq)]
struct PairingLink {
    name: String,
    environment_id: String,
    fingerprint: String,
    code: String,
    endpoints: Vec<String>,
}

fn parse_pairing_link(value: &str) -> Result<PairingLink, String> {
    let invalid = || {
        "Paste the whole pairing link that monocode-host connect printed. It starts with monocode://pair".to_string()
    };
    let url = url::Url::parse(value.trim()).map_err(|_| invalid())?;
    if url.scheme() != "monocode" || url.host_str() != Some("pair") {
        return Err(invalid());
    }
    let mut query: HashMap<String, String> = HashMap::new();
    let mut endpoints = Vec::new();
    for (key, value) in url.query_pairs() {
        if key == "url" {
            endpoints.push(direct_endpoint(&value)?);
        } else {
            query.insert(key.into_owned(), value.into_owned());
        }
    }
    if query.get("v").map(String::as_str) != Some("1") {
        return Err("This pairing link is from a newer MonoCode Host. Update MonoCode.".into());
    }
    let token = |key: &str| {
        query
            .get(key)
            .filter(|value| {
                value.len() == 43
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            .cloned()
            .ok_or_else(invalid)
    };
    let fingerprint = token("fp")?;
    remote_tls::decode_fingerprint(&fingerprint)?;
    let environment_id = query
        .get("id")
        .filter(|id| !id.is_empty() && id.len() <= 100)
        .cloned()
        .ok_or_else(invalid)?;
    Ok(PairingLink {
        name: query
            .get("name")
            .map(|name| {
                name.trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(100)
                    .collect()
            })
            .unwrap_or_default(),
        environment_id,
        fingerprint,
        code: token("code")?,
        endpoints,
    })
}

struct Paired {
    token: String,
    route: Route,
    tunnel: Option<Tunnel>,
}

/// Exchanges the link's one-time code for a device credential over the first
/// direct address that answers, or through `tunnel` when given.
fn exchange(
    state: &RemoteConnections,
    link: &PairingLink,
    tunnel: Option<Tunnel>,
) -> Result<Paired, Failure> {
    let params = json!({ "code": link.code, "name": remote_ssh::device_name() });
    let accept = |result: Value, route: Route, tunnel: Option<Tunnel>| -> Result<Paired, Failure> {
        if result.get("environmentId").and_then(Value::as_str) != Some(&link.environment_id) {
            return Err(Failure::Rejected(
                "The machine that answered is not the one in the pairing link".into(),
            ));
        }
        let token = result
            .get("token")
            .and_then(Value::as_str)
            .filter(|s| {
                s.len() == 43
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            .ok_or_else(|| {
                Failure::Rejected("Host returned an invalid device credential".into())
            })?;
        Ok(Paired {
            token: token.into(),
            route,
            tunnel,
        })
    };
    if let Some(tunnel) = tunnel {
        let base = format!("http://127.0.0.1:{}", tunnel.port);
        let result = call(
            &state.agent(None).map_err(Failure::Rejected)?,
            &base,
            None,
            None,
            "pair.exchange",
            params,
        )?;
        return accept(result, Route::Ssh, Some(tunnel));
    }
    let agent = state
        .agent(Some(&link.fingerprint))
        .map_err(Failure::Rejected)?;
    let mut failures = Vec::new();
    for base in &link.endpoints {
        match call(&agent, base, None, None, "pair.exchange", params.clone()) {
            Ok(result) => return accept(result, Route::Direct(base.clone()), None),
            // The host redeems a code once, so resending one that may have
            // arrived gets a clear "already used" answer at worst.
            Err(Failure::Unreachable(error)) | Err(Failure::Uncertain(error)) => {
                failures.push(error)
            }
            // Every address reaches the same throttle.
            Err(failure) => return Err(failure),
        }
    }
    Err(Failure::Unreachable(if link.endpoints.is_empty() {
        "This host listens only on loopback. Use Set up over SSH instead.".into()
    } else {
        format!(
            "None of the machine's addresses answered from this computer: {}. Both computers need to be on the same network or tailnet, or use Set up over SSH.",
            failures.join("; ")
        )
    }))
}

/// Stores a newly paired machine. Pairing the same host again replaces this
/// desktop's older credential, which is then revoked on the host.
fn store_paired(
    remote: &Remote,
    link: &PairingLink,
    paired: Paired,
    name: String,
    ssh: Option<SshTarget>,
) -> Result<Machine, String> {
    let state = remote.connections();
    let previous = {
        let _guard = state
            .store
            .lock()
            .map_err(|_| "Connection store is locked")?;
        read(&store_path(remote)?)?
            .into_iter()
            .find(|m| m.environment_id == link.environment_id)
    };
    let mut endpoints = link.endpoints.clone();
    if let Route::Direct(base) = &paired.route {
        endpoints.retain(|e| e != base);
        endpoints.insert(0, base.clone());
    }
    let ssh = ssh.or_else(|| previous.as_ref().and_then(|m| m.ssh.clone()));
    let machine = save_machine(
        remote,
        StoredMachine {
            id: uuid::Uuid::new_v4().to_string(),
            name: if name.trim().is_empty() {
                link.name.clone()
            } else {
                name.trim().chars().take(100).collect()
            },
            endpoint: display(&endpoints, ssh.as_ref()),
            environment_id: link.environment_id.clone(),
            token: paired.token,
            ssh,
            endpoints,
            fingerprint: Some(link.fingerprint.clone()),
        },
    )?;
    state.set_route(&machine.id, paired.route.clone());
    if let Some(tunnel) = paired.tunnel {
        state.tunnels.insert(machine.id.clone(), tunnel);
    }
    if let Some(old) = previous.filter(|old| old.token != machine.token) {
        let mut stale = machine.clone();
        stale.token = old.token;
        let _ = call_route(
            state,
            &stale,
            &paired.route,
            "devices.revokeSelf",
            json!({}),
        );
    }
    Ok(machine.public())
}

/// Pairs a machine from the link `monocode-host connect` printed.
pub fn remote_pair(remote: &Remote, link: String, name: String) -> Result<Machine, String> {
    let link = parse_pairing_link(&link)?;
    let state = remote.connections();
    let paired = exchange(state, &link, None).map_err(String::from)?;
    store_paired(remote, &link, paired, name, None)
}

pub fn remote_disconnect(remote: &Remote, machine_id: String) -> Result<(), String> {
    let state = remote.connections();
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    let path = store_path(remote)?;
    let mut machines = read(&path)?;
    machines.retain(|m| m.id != machine_id);
    write(&path, &machines)?;
    state.tunnels.remove(&machine_id);
    state.forget(&machine_id);
    Ok(())
}

/// Forgets the machine's route and recent failure, so the next request
/// probes every address again.
pub fn remote_retry(remote: &Remote, machine_id: String) {
    let state = remote.connections();
    state.forget(&machine_id);
}

/// Requests block on the network, and `changes.wait` holds its request for
/// up to 25 seconds. Callers keep both off async runtime threads.
pub fn remote_request(
    remote: &Remote,
    machine_id: String,
    method: String,
    params: Value,
) -> Result<Value, String> {
    request(remote, &machine_id, &method, params)
}

fn supported_remote_method(method: &str) -> bool {
    matches!(
        method,
        "environment.describe"
            | "changes.wait"
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

/// What `monocode-host connect --json` prints on its last line.
#[derive(Deserialize)]
struct ConnectOutput {
    link: String,
    port: u16,
}

/// Sets up or updates a host over SSH: runs `monocode-host connect` there
/// through npx, then pairs over the host's direct addresses, falling back to
/// an SSH forward when this computer cannot reach them.
fn start_ssh_job(
    remote: Remote,
    target: SshTarget,
    name: String,
    existing: Option<StoredMachine>,
    upgrade: bool,
) -> Result<String, String> {
    let package = remote_ssh::host_package()?;
    let job = Job::new();
    let id = job.view().id;
    {
        let state = remote.connections();
        let mut jobs = state
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
    std::thread::spawn(move || {
        let state = remote.connections();
        let prepared = (|| -> Result<Machine, String> {
            let askpass = job.askpass()?;
            let mut target = target;
            let platform = remote_ssh::detect_platform(&target, &job, &askpass)?;
            job.message(if upgrade {
                "Updating MonoCode Host on the machine…"
            } else {
                "Starting MonoCode Host with npx. The first run downloads it…"
            });
            let output = remote_ssh::run_script(
                &target,
                platform,
                remote_ssh::connect_script(platform, &package, upgrade),
                &job,
                &askpass,
            )?;
            let info: ConnectOutput = output
                .lines()
                .rev()
                .find(|line| line.trim_start().starts_with('{'))
                .and_then(|line| serde_json::from_str(line.trim()).ok())
                .ok_or("MonoCode Host setup returned an invalid response")?;
            if info.port == 0 {
                return Err("Host did not report a valid port".into());
            }
            target.remote_port = info.port;
            let link = parse_pairing_link(&info.link)?;
            if let Some(mut machine) = existing {
                if machine.environment_id != link.environment_id {
                    return Err(
                        "Host identity changed. Remove this connection and add the machine again."
                            .into(),
                    );
                }
                // The connect output arrived over the authenticated SSH
                // channel, so its certificate and addresses can be saved.
                machine.ssh = Some(target.clone());
                machine.fingerprint = Some(link.fingerprint.clone());
                machine.endpoints = link.endpoints.clone();
                machine.endpoint = display(&machine.endpoints, machine.ssh.as_ref());
                job.message("Reconnecting…");
                state.forget(&machine.id);
                let tunnel = Tunnel::start(&target, Some(&job), Some(&askpass))?;
                state.tunnels.insert(machine.id.clone(), tunnel);
                let machine = save_machine(&remote, machine)?;
                resolve_route(&remote, &machine)?;
                return Ok(machine.public());
            }
            job.message("Pairing this desktop with the host…");
            // The SSH forward reaches the host over loopback, which its
            // pairing throttle exempts.
            let paired = match exchange(state, &link, None) {
                Ok(paired) => paired,
                Err(Failure::Unreachable(_)) | Err(Failure::Throttled(_)) => {
                    job.message("Opening an SSH forward to the host…");
                    let tunnel = Tunnel::start(&target, Some(&job), Some(&askpass))?;
                    exchange(state, &link, Some(tunnel)).map_err(String::from)?
                }
                Err(failure) => return Err(failure.into()),
            };
            store_paired(&remote, &link, paired, name, Some(target))
        })();
        job.complete(|| prepared);
    });
    Ok(id)
}

pub fn remote_ssh_begin(
    remote: &Remote,
    target: String,
    name: String,
    port: Option<u16>,
    upgrade: Option<bool>,
) -> Result<String, String> {
    let target = remote_ssh::validate_target(&target, port)?;
    start_ssh_job(
        remote.clone(),
        SshTarget {
            target,
            port,
            remote_port: 3774,
        },
        name.trim().chars().take(100).collect(),
        None,
        upgrade.unwrap_or(false),
    )
}

pub fn remote_ssh_reconnect(
    remote: &Remote,
    machine_id: String,
    upgrade: Option<bool>,
) -> Result<String, String> {
    let machine = load_machine(remote, &machine_id)?;
    let target = machine
        .ssh
        .clone()
        .ok_or("This connection does not use SSH")?;
    start_ssh_job(
        remote.clone(),
        target,
        machine.name.clone(),
        Some(machine),
        upgrade.unwrap_or(false),
    )
}

pub fn remote_ssh_poll(remote: &Remote, job_id: String) -> Result<JobView, String> {
    let state = remote.connections();
    Ok(state.job(&job_id)?.view())
}

pub fn remote_ssh_answer(
    remote: &Remote,
    job_id: String,
    prompt_id: String,
    answer: String,
) -> Result<(), String> {
    let state = remote.connections();
    state.job(&job_id)?.answer(&prompt_id, answer)
}

pub fn remote_ssh_cancel(remote: &Remote, job_id: String) -> Result<(), String> {
    let state = remote.connections();
    state.job(&job_id)?.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::net::TcpListener;

    // Generated by host/tls.ts, like a real host's certificate. Inline
    // because the repository ignores .pem and .key files. Test only.
    const CERT: &[u8] = br#"-----BEGIN CERTIFICATE-----
MIIBXjCCAQWgAwIBAgIQcT7HxNtLrgCn81joEqO4JjAKBggqhkjOPQQDAjAdMRsw
GQYDVQQDDBJNb25vQ29kZSB0ZXN0IGhvc3QwHhcNMjUxMjMxMDAwMDAwWhcNNDUx
MjI3MDAwMDAwWjAdMRswGQYDVQQDDBJNb25vQ29kZSB0ZXN0IGhvc3QwWTATBgcq
hkjOPQIBBggqhkjOPQMBBwNCAASLCXUeQOtw5x9bmnqffwefGVFH2NWOCgAQCZOp
9ryb4fYlBO4HfeeJrWwh6XNlAg5VL5wWSUBdgwZut41c+ND7oycwJTAJBgNVHRME
AjAAMBgGA1UdEQQRMA+CDW1vbm9jb2RlLWhvc3QwCgYIKoZIzj0EAwIDRwAwRAIg
aKsltyMKcOisqD8EiaVH2+9jbNaov2BGuizMOUVxRfMCIBJKDYslwBN2x5t/39C0
2CdA6i4dNG1LEY4kAJb88byy
-----END CERTIFICATE-----
"#;
    const KEY: &[u8] = br#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgLYVZ/ljgngB0PYx7
xupDHdtkkbtP1uPFMs0c1xkn57OhRANCAASLCXUeQOtw5x9bmnqffwefGVFH2NWO
CgAQCZOp9ryb4fYlBO4HfeeJrWwh6XNlAg5VL5wWSUBdgwZut41c+ND7
-----END PRIVATE KEY-----
"#;
    const FINGERPRINT: &str = "-EcUWBSssurVvQMouW-dKgMaHMfKn1kMzWGr6lKqIIc";

    /// A one-request-per-connection TLS host that answers every RPC with
    /// `reply` and records the request bodies it received.
    fn tls_host(
        connections: usize,
        reply: &'static str,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        tls_host_with_status(connections, "200 OK", reply)
    }

    fn tls_host_with_status(
        connections: usize,
        status: &'static str,
        reply: &'static str,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from_pem_slice(CERT).unwrap()],
                    PrivateKeyDer::from_pem_slice(KEY).unwrap(),
                )
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!(
            "https://127.0.0.1:{}",
            listener.local_addr().unwrap().port()
        );
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for _ in 0..connections {
                let (socket, _) = listener.accept().unwrap();
                let connection = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut stream = rustls::StreamOwned::new(connection, socket);
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match stream.read(&mut byte) {
                        Ok(1) => head.push(byte[0]),
                        _ => break,
                    }
                }
                // A client that rejected the certificate sends nothing.
                if !head.ends_with(b"\r\n\r\n") {
                    continue;
                }
                let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map(|value| value.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                bodies.push(String::from_utf8(body).unwrap());
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
                stream.conn.send_close_notify();
                let _ = stream.flush();
            }
            bodies
        });
        (base, server)
    }

    #[test]
    fn pinned_tls_reaches_the_paired_host_and_rejects_any_other_certificate() {
        let (base, server) = tls_host(2, r#"{"result":{"environmentId":"env"}}"#);
        let pinned =
            remote_tls::agent(Some(FINGERPRINT), CONNECT_TIMEOUT, Duration::from_secs(10)).unwrap();
        let result = call(
            &pinned,
            &base,
            Some("token"),
            Some("env"),
            "environment.describe",
            json!({}),
        )
        .unwrap();
        assert_eq!(result["environmentId"], "env");
        let other = remote_tls::agent(
            Some(&"A".repeat(43)),
            CONNECT_TIMEOUT,
            Duration::from_secs(10),
        )
        .unwrap();
        match call(
            &other,
            &base,
            Some("token"),
            Some("env"),
            "commands.dispatch",
            json!({}),
        ) {
            // Nothing was sent, so the request may safely go to another route.
            Err(Failure::Unreachable(error)) => {
                assert!(error.contains("different certificate"), "{error}")
            }
            other => panic!("expected a certificate failure, got {other:?}"),
        }
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("environment.describe"));
    }

    #[test]
    fn host_errors_are_rejections_and_closed_ports_are_unreachable() {
        let (base, server) = tls_host(1, r#"{"error":"Session not found"}"#);
        let agent =
            remote_tls::agent(Some(FINGERPRINT), CONNECT_TIMEOUT, Duration::from_secs(10)).unwrap();
        match call(
            &agent,
            &base,
            Some("token"),
            None,
            "sessions.sync",
            json!({}),
        ) {
            Err(Failure::Rejected(error)) => {
                assert_eq!(error, "Host rejected request: Session not found")
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
        server.join().unwrap();
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        assert!(matches!(
            call(
                &agent,
                &format!("https://127.0.0.1:{port}"),
                None,
                None,
                "environment.describe",
                json!({})
            ),
            Err(Failure::Unreachable(_))
        ));
    }

    #[test]
    fn a_throttled_pairing_attempt_can_retry_over_ssh() {
        let (base, server) = tls_host_with_status(
            1,
            "429 Too Many Requests",
            r#"{"error":"Too many pairing attempts. Wait a minute and try again."}"#,
        );
        let link = PairingLink {
            name: "host".into(),
            environment_id: "env".into(),
            fingerprint: FINGERPRINT.into(),
            code: "code".into(),
            endpoints: vec![base],
        };
        // SSH setup falls back to its loopback forward for this failure,
        // which the host's pairing throttle exempts.
        match exchange(&RemoteConnections::default(), &link, None) {
            Err(Failure::Throttled(error)) => {
                assert!(error.contains("Too many pairing attempts"), "{error}")
            }
            other => panic!("expected a throttled pairing, got {:?}", other.err()),
        }
        assert!(server.join().unwrap()[0].contains("pair.exchange"));
    }

    #[test]
    fn pairing_links_carry_a_pin_a_code_and_https_addresses() {
        let code = "c".repeat(43);
        let link = format!(
            "  monocode://pair?v=1&name=Studio%20Mac&id=env-1&fp={FINGERPRINT}&code={code}&url=https%3A%2F%2F10.0.0.2%3A3774&url=https%3A%2F%2Fbox.ts.net%3A3774\n"
        );
        assert_eq!(
            parse_pairing_link(&link).unwrap(),
            PairingLink {
                name: "Studio Mac".into(),
                environment_id: "env-1".into(),
                fingerprint: FINGERPRINT.into(),
                code: code.clone(),
                endpoints: vec![
                    "https://10.0.0.2:3774".into(),
                    "https://box.ts.net:3774".into()
                ],
            }
        );
        for bad in [
            "https://example.com/pair".to_string(),
            format!("monocode://pair?v=2&id=e&fp={FINGERPRINT}&code={code}"),
            format!("monocode://pair?v=1&id=e&fp=short&code={code}"),
            format!("monocode://pair?v=1&id=e&fp={FINGERPRINT}&code=short"),
            format!("monocode://pair?v=1&fp={FINGERPRINT}&code={code}"),
            format!(
                "monocode://pair?v=1&id=e&fp={FINGERPRINT}&code={code}&url=http%3A%2F%2F10.0.0.2%3A3774"
            ),
        ] {
            assert!(parse_pairing_link(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn direct_addresses_are_bare_https_origins() {
        assert_eq!(
            direct_endpoint("https://10.0.0.2:3774/").unwrap(),
            "https://10.0.0.2:3774"
        );
        assert!(direct_endpoint("https://[fd7a::1]:3774").is_ok());
        for url in [
            "http://10.0.0.2:3774",
            "https://user:secret@host.example",
            "https://host.example/rpc",
            "https://host.example?token=secret",
            "file:///tmp/socket",
        ] {
            assert!(direct_endpoint(url).is_err(), "{url}");
        }
    }

    #[test]
    fn routes_try_pinned_addresses_before_ssh_and_legacy_machines_use_ssh() {
        let ssh = SshTarget {
            target: "me@box".into(),
            port: None,
            remote_port: 3774,
        };
        let mut machine = StoredMachine {
            id: "id".into(),
            name: "box".into(),
            endpoint: String::new(),
            environment_id: "env".into(),
            token: "secret".into(),
            ssh: Some(ssh.clone()),
            endpoints: vec!["https://10.0.0.2:3774".into()],
            fingerprint: Some(FINGERPRINT.into()),
        };
        assert_eq!(
            machine.candidates(),
            vec![Route::Direct("https://10.0.0.2:3774".into()), Route::Ssh]
        );
        assert_eq!(display(&machine.endpoints, Some(&ssh)), "10.0.0.2:3774");
        // Without a pinned certificate, direct addresses are never used.
        machine.fingerprint = None;
        assert_eq!(machine.candidates(), vec![Route::Ssh]);
        assert_eq!(display(&[], Some(&ssh)), "ssh://me@box");
        let public = serde_json::to_string(&machine.public()).unwrap();
        assert!(!public.contains("secret"));
        assert!(!public.contains("token"));
        // Machines saved by older versions have neither field.
        let legacy: StoredMachine = serde_json::from_str(
            r#"{"id":"a","name":"b","endpoint":"ssh://me@box","environmentId":"e","token":"t","ssh":{"target":"me@box","port":null,"remotePort":3774}}"#,
        )
        .unwrap();
        assert_eq!(legacy.candidates(), vec![Route::Ssh]);
    }

    #[test]
    fn connect_output_is_read_from_the_last_json_line() {
        let output = "npm warn exec The following package was not found\n{\"link\":\"monocode://pair?v=1\",\"port\":3774,\"service\":\"linux\"}\n";
        let info: ConnectOutput = output
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with('{'))
            .and_then(|line| serde_json::from_str(line.trim()).ok())
            .unwrap();
        assert_eq!(info.port, 3774);
    }

    // Pairs with a real Node host. Start one and pass its link:
    //   node build/host/monocode-host.mjs connect --no-service --json --bind 127.0.0.1 --data-dir <tmp>
    //   MONOCODE_TEST_PAIRING_LINK=<link> cargo test --lib real_host -- --ignored
    #[test]
    #[ignore = "requires a running host and MONOCODE_TEST_PAIRING_LINK"]
    fn pairs_with_a_real_host_over_pinned_tls() {
        let link =
            parse_pairing_link(&std::env::var("MONOCODE_TEST_PAIRING_LINK").unwrap()).unwrap();
        let state = RemoteConnections::default();
        let paired = exchange(&state, &link, None).unwrap();
        let Route::Direct(base) = paired.route.clone() else {
            panic!("expected a direct route")
        };
        let agent = state.agent(Some(&link.fingerprint)).unwrap();
        let ask = |method: &str, params: Value| {
            call(
                &agent,
                &base,
                Some(&paired.token),
                Some(&link.environment_id),
                method,
                params,
            )
        };
        let described = ask("environment.describe", json!({})).unwrap();
        assert_eq!(described["environmentId"], link.environment_id.as_str());
        assert!(
            described["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry == "changes.wait")
        );
        let first = ask("changes.wait", json!({})).unwrap();
        assert_eq!(first["reset"], true);
        let started = Instant::now();
        let idle = ask(
            "changes.wait",
            json!({ "boot": first["boot"], "after": first["cursor"], "timeoutMs": 300 }),
        )
        .unwrap();
        assert_eq!(idle["sessions"], json!([]));
        assert!(started.elapsed() >= Duration::from_millis(250));
        match exchange(&state, &link, None).map_err(String::from) {
            Err(error) => assert!(error.contains("already used"), "{error}"),
            Ok(_) => panic!("a pairing code must work once"),
        }
        let _ = ask("devices.revokeSelf", json!({}));
    }

    #[test]
    fn a_slow_host_does_not_look_like_a_dead_tunnel() {
        use std::thread;
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
        let error = ureq::Error::from(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        let ureq::Error::Transport(error) = error else {
            panic!("Expected a refused connection")
        };
        assert!(connection_refused(&error));
    }

    #[test]
    fn desktop_forwards_supported_host_operations() {
        for method in [
            "changes.wait",
            "git.branches",
            "git.worktreeCreate",
            "attachments.upload",
        ] {
            assert!(supported_remote_method(method), "{method}");
        }
        assert!(!supported_remote_method("git.arbitrary"));
        assert!(!supported_remote_method("pair.exchange"));
    }
}
