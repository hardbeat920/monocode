//! Tauri commands over `monocode_process::mcp`.
use tauri::State;

use monocode_process::mcp::{self, McpConnection};

#[tauri::command]
pub async fn mcp_add(
    host: State<'_, crate::harness::HarnessHost>,
    cwd: String,
    provider: String,
    scope: String,
    name: String,
    config: String,
) -> Result<(), String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        mcp::mcp_add(&host, cwd, provider, scope, name, config)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mcp_discover(cwd: String) -> Result<Vec<McpConnection>, String> {
    tauri::async_runtime::spawn_blocking(move || mcp::mcp_discover(cwd))
        .await
        .map_err(|e| e.to_string())?
}
