//! Tauri commands over `monocode_remote::remote`.
use serde_json::Value;
use tauri::State;

pub(crate) use monocode_remote::remote::Remote;
use monocode_remote::remote::{self, Machine};
use monocode_remote::remote_ssh::JobView;

#[tauri::command(async)]
pub fn remote_machines(remote: State<'_, Remote>) -> Result<Vec<Machine>, String> {
    remote::remote_machines(&remote)
}

/// Pairs a machine from the link `monocode-host connect` printed.
#[tauri::command]
pub async fn remote_pair(
    remote: State<'_, Remote>,
    link: String,
    name: String,
) -> Result<Machine, String> {
    let remote = remote.inner().clone();
    tauri::async_runtime::spawn_blocking(move || remote::remote_pair(&remote, link, name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn remote_disconnect(remote: State<'_, Remote>, machine_id: String) -> Result<(), String> {
    remote::remote_disconnect(&remote, machine_id)
}

/// Forgets the machine's route and recent failure, so the next request
/// probes every address again.
#[tauri::command]
pub fn remote_retry(remote: State<'_, Remote>, machine_id: String) {
    remote::remote_retry(&remote, machine_id)
}

#[tauri::command]
pub async fn remote_request(
    remote: State<'_, Remote>,
    machine_id: String,
    method: String,
    params: Value,
) -> Result<Value, String> {
    // Requests block on the network, and `changes.wait` holds its request
    // for up to 25 seconds. Keep both off the async runtime's threads.
    let remote = remote.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        remote::remote_request(&remote, machine_id, method, params)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn remote_ssh_begin(
    remote: State<'_, Remote>,
    target: String,
    name: String,
    port: Option<u16>,
    upgrade: Option<bool>,
) -> Result<String, String> {
    remote::remote_ssh_begin(&remote, target, name, port, upgrade)
}

#[tauri::command(async)]
pub fn remote_ssh_reconnect(
    remote: State<'_, Remote>,
    machine_id: String,
    upgrade: Option<bool>,
) -> Result<String, String> {
    remote::remote_ssh_reconnect(&remote, machine_id, upgrade)
}

#[tauri::command]
pub fn remote_ssh_poll(remote: State<'_, Remote>, job_id: String) -> Result<JobView, String> {
    remote::remote_ssh_poll(&remote, job_id)
}

#[tauri::command]
pub fn remote_ssh_answer(
    remote: State<'_, Remote>,
    job_id: String,
    prompt_id: String,
    answer: String,
) -> Result<(), String> {
    remote::remote_ssh_answer(&remote, job_id, prompt_id, answer)
}

#[tauri::command]
pub fn remote_ssh_cancel(remote: State<'_, Remote>, job_id: String) -> Result<(), String> {
    remote::remote_ssh_cancel(&remote, job_id)
}
