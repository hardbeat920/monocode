//! Tauri commands over `monocode_integrations::harness_updates`.
use monocode_integrations::harness_updates;

/// True for the first caller per app process, so a window opened later in the
/// same run does not repeat the launch check.
#[tauri::command]
pub fn harness_update_check_claim() -> bool {
    harness_updates::harness_update_check_claim()
}

#[tauri::command]
pub async fn harness_latest_version(provider: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || harness_updates::harness_latest_version(provider))
        .await
        .map_err(|e| e.to_string())?
}

/// Runs the harness's self-update against the binary MonoCode resolved for
/// it. stdin is closed, so an updater that stops to ask fails instead of
/// hanging.
#[tauri::command]
pub async fn harness_update(
    command: String,
    binary_provider: String,
    binary_path: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        harness_updates::harness_update(command, binary_provider, binary_path)
    })
    .await
    .map_err(|e| e.to_string())?
}
