//! Tauri commands over `monocode_integrations::rate_limits`.
use tauri::AppHandle;

use monocode_integrations::rate_limits::{
    self, ClaudeUsageFetch, DroidUsageFetch, OpencodeGoUsageFetch,
};

/// Fetch OpenCode Go 5h / weekly / monthly usage via the local Go API key.
/// Runs in the host process so the webview CORS policy does not apply.
/// The key never leaves the host process.
#[tauri::command]
pub async fn fetch_opencode_go_usage() -> Result<OpencodeGoUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(rate_limits::fetch_opencode_go_usage)
        .await
        .map_err(|e| e.to_string())?
}

/// Fetch Factory Droid 5-hour / weekly / monthly usage via the token the
/// Droid CLI stores in `~/.factory`. The token never leaves the host process.
#[tauri::command]
pub async fn fetch_droid_usage() -> Result<DroidUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(rate_limits::fetch_droid_usage)
        .await
        .map_err(|e| e.to_string())?
}

/// Fetch Claude Code 5-hour / weekly usage via the local OAuth token.
/// The token never leaves the host process.
#[tauri::command]
pub async fn fetch_claude_usage(
    app: AppHandle,
    account_id: Option<String>,
) -> Result<ClaudeUsageFetch, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        rate_limits::fetch_claude_usage(&data_dir, account_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
