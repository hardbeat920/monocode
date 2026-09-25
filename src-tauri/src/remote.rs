//! Remote host connections. The renderer receives machine metadata, never the
//! saved bearer credential. Workspace requests never fall back to local calls.
use crate::remote_ssh::{self, Job, JobView, SshTarget, Tunnel, Tunnels};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct RemoteConnections {
    store: Mutex<()>,
    tunnels: Tunnels,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    id: String,
    name: String,
    endpoint: String,
    environment_id: String,
    ssh: Option<SshTarget>,
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
        Err(_) => {
            return Err(
                "Machine is unreachable. Check the host and SSH tunnel, then reconnect.".into(),
            )
        }
    };
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
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

fn store_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("remote-machines.json"))
}

#[tauri::command(async)]
pub fn remote_machines(
    app: AppHandle,
    state: State<'_, RemoteConnections>,
) -> Result<Vec<Machine>, String> {
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    Ok(read(&store_path(&app)?)?
        .iter()
        .map(StoredMachine::public)
        .collect())
}

#[tauri::command(async)]
pub fn remote_connect(
    app: AppHandle,
    state: State<'_, RemoteConnections>,
    name: String,
    url: String,
    token: String,
) -> Result<Machine, String> {
    let endpoint = endpoint(&url)?;
    let token = token.trim().to_string();
    if token.len() != 43
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("Enter the device token issued by monocode-host pair".into());
    }
    let descriptor = rpc(&endpoint, &token, None, "environment.describe", json!({}))?;
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
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    let path = store_path(&app)?;
    let mut machines = read(&path)?;
    let id = machines
        .iter()
        .find(|m| m.environment_id == environment_id)
        .map(|m| m.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let machine = StoredMachine {
        id: id.clone(),
        name,
        endpoint,
        environment_id,
        token,
        ssh: None,
    };
    machines.retain(|m| m.id != id);
    machines.push(machine.clone());
    write(&path, &machines)?;
    Ok(machine.public())
}

#[tauri::command(async)]
pub fn remote_disconnect(
    app: AppHandle,
    state: State<'_, RemoteConnections>,
    machine_id: String,
) -> Result<(), String> {
    let _guard = state
        .store
        .lock()
        .map_err(|_| "Connection store is locked")?;
    let path = store_path(&app)?;
    let mut machines = read(&path)?;
    machines.retain(|m| m.id != machine_id);
    write(&path, &machines)?;
    state.tunnels.remove(&machine_id);
    Ok(())
}

#[tauri::command(async)]
pub fn remote_request(
    app: AppHandle,
    state: State<'_, RemoteConnections>,
    machine_id: String,
    method: String,
    params: Value,
) -> Result<Value, String> {
    if !matches!(
        method.as_str(),
        "environment.describe"
            | "projects.list"
            | "projects.open"
            | "sessions.list"
            | "sessions.get"
            | "sessions.sync"
            | "commands.dispatch"
            | "events.read"
            | "git.diff"
            | "files.read"
    ) {
        return Err("Unsupported remote operation".into());
    }
    let machine = {
        let _guard = state
            .store
            .lock()
            .map_err(|_| "Connection store is locked")?;
        read(&store_path(&app)?)?
            .into_iter()
            .find(|m| m.id == machine_id)
            .ok_or("Machine is no longer connected")?
    };
    let endpoint = if let Some(target) = &machine.ssh {
        state.tunnels.endpoint(&machine.id, target)?
    } else {
        machine.endpoint.clone()
    };
    let response = rpc(
        &endpoint,
        &machine.token,
        Some(&machine.environment_id),
        &method,
        params,
    );
    if machine.ssh.is_some()
        && response
            .as_ref()
            .err()
            .is_some_and(|error| error.starts_with("Machine is unreachable"))
    {
        state.tunnels.remove(&machine.id);
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

fn start_ssh_job(
    app: AppHandle,
    target: SshTarget,
    name: String,
    existing: Option<StoredMachine>,
) -> Result<String, String> {
    let job = Job::new();
    let id = job.view().id;
    {
        let state = app.state::<RemoteConnections>();
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
        let prepared = (|| -> Result<(StoredMachine, Tunnel), String> {
            let askpass = job.askpass()?;
            let mut target = target;
            let mut machine = if let Some(existing) = existing {
                job.message("Reconnecting to the machine…");
                existing
            } else {
                let platform = remote_ssh::detect_platform(&target, &job, &askpass)?;
                job.message("Installing or starting MonoCode Host…");
                let output = remote_ssh::run_script(
                    &target,
                    platform,
                    remote_ssh::bootstrap_script(platform),
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
                let pair = remote_ssh::run_script(
                    &target,
                    platform,
                    remote_ssh::pairing_script(platform, &remote_ssh::device_name()),
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
                json!({}),
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
            let state = app.state::<RemoteConnections>();
            let _guard = state
                .store
                .lock()
                .map_err(|_| "Connection store is locked")?;
            let path = store_path(&app)?;
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

#[tauri::command(async)]
pub fn remote_ssh_begin(
    app: AppHandle,
    target: String,
    name: String,
    port: Option<u16>,
) -> Result<String, String> {
    let target = remote_ssh::validate_target(&target, port)?;
    start_ssh_job(
        app,
        SshTarget {
            target,
            port,
            remote_port: 3774,
        },
        name.trim().chars().take(100).collect(),
        None,
    )
}

#[tauri::command(async)]
pub fn remote_ssh_reconnect(
    app: AppHandle,
    state: State<'_, RemoteConnections>,
    machine_id: String,
) -> Result<String, String> {
    let machine = {
        let _guard = state
            .store
            .lock()
            .map_err(|_| "Connection store is locked")?;
        read(&store_path(&app)?)?
            .into_iter()
            .find(|m| m.id == machine_id)
            .ok_or("Machine is no longer connected")?
    };
    let target = machine
        .ssh
        .clone()
        .ok_or("This connection does not use SSH")?;
    start_ssh_job(app, target, machine.name.clone(), Some(machine))
}

#[tauri::command]
pub fn remote_ssh_poll(
    state: State<'_, RemoteConnections>,
    job_id: String,
) -> Result<JobView, String> {
    Ok(state.job(&job_id)?.view())
}

#[tauri::command]
pub fn remote_ssh_answer(
    state: State<'_, RemoteConnections>,
    job_id: String,
    prompt_id: String,
    answer: String,
) -> Result<(), String> {
    state.job(&job_id)?.answer(&prompt_id, answer)
}

#[tauri::command]
pub fn remote_ssh_cancel(
    state: State<'_, RemoteConnections>,
    job_id: String,
) -> Result<(), String> {
    state.job(&job_id)?.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
