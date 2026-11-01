//! Registering `monocode browser-mcp` with the providers, the Tauri app's
//! side: the logic is `monocode_browser::agent::register`; this gives it the
//! app's MCP file writers, provider CLIs and account directories, and wraps
//! it in Tauri commands.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use monocode_browser::agent::register::{self, BrowserMcpRegistration, ProviderConfig};

/// The Tauri app's MCP writers, provider CLIs and account directories.
struct TauriProviders {
    app: AppHandle,
}

impl ProviderConfig for TauriProviders {
    fn home_dir(&self) -> Option<PathBuf> {
        crate::dirs_home().map(PathBuf::from)
    }

    fn opencode_config_path(&self, home: &Path, override_path: Option<&Path>) -> PathBuf {
        crate::mcp::opencode_config_path(home, home, "user", override_path)
    }

    fn strip_jsonc(&self, raw: &str) -> String {
        crate::mcp::strip_jsonc(raw)
    }

    fn put_json_server(&self, path: &Path, name: &str, server: Value) -> Result<(), String> {
        crate::mcp::put_json_server(path, name, server, true)
    }

    fn put_opencode_server(
        &self,
        path: &Path,
        name: &str,
        server: Value,
        major: u32,
    ) -> Result<(), String> {
        crate::mcp::put_opencode_server(path, name, server, major, true)
    }

    fn add_mcp_via_cli(
        &self,
        provider: &str,
        cwd: &str,
        name: &str,
        server: &Value,
        binary_path: Option<&str>,
    ) -> Result<(), String> {
        crate::harness::add_mcp_via_cli(provider, "user", cwd, name, server, binary_path)
    }

    fn remove_mcp_via_cli(
        &self,
        provider: &str,
        cwd: &str,
        name: &str,
        binary_path: Option<&str>,
    ) -> Result<(), String> {
        crate::harness::remove_mcp_via_cli(provider, cwd, name, binary_path)
    }

    fn opencode_major_version(&self, cwd: &str, binary_path: Option<&str>) -> Result<u32, String> {
        crate::harness::opencode_major_version(cwd, binary_path)
    }

    fn account_dirs(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Vec<PathBuf>, String> {
        let mut dirs = Vec::new();
        // The parent of any account's path is where all of them live.
        let probe = crate::harness::provider_account_path(&self.app, provider, "probe")?;
        if let Some(Ok(entries)) = probe.parent().map(std::fs::read_dir) {
            for entry in entries.flatten() {
                let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if let Ok(dir) = crate::harness::provider_account_path(&self.app, provider, &id) {
                    if dir.is_dir() {
                        dirs.push(dir);
                    }
                }
            }
        }
        if let Some(dir) = crate::harness::provider_account_dir(&self.app, provider, account_id)? {
            dirs.push(dir);
        }
        Ok(dirs)
    }

    fn copies_dir(&self) -> Result<PathBuf, String> {
        Ok(self
            .app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("bin"))
    }
}

/// Registration state of the browser MCP server for every provider.
#[tauri::command]
pub async fn browser_mcp_status(
    app: AppHandle,
    host: State<'_, crate::harness::HarnessHost>,
) -> Result<Vec<BrowserMcpRegistration>, String> {
    let opencode_binary = host.runtime_binary_path("opencode");
    tauri::async_runtime::spawn_blocking(move || {
        register::mcp_status(&TauriProviders { app }, opencode_binary.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Make sure `provider` starts `monocode browser-mcp`; see
/// [`register::mcp_register`]. Called by the UI when a session of that
/// provider is first granted `/browser`.
#[tauri::command]
pub async fn browser_mcp_register(
    app: AppHandle,
    host: State<'_, crate::harness::HarnessHost>,
    provider: String,
    cwd: String,
    account_id: Option<String>,
) -> Result<BrowserMcpRegistration, String> {
    let binary_path = host.runtime_binary_path(&provider);
    tauri::async_runtime::spawn_blocking(move || {
        register::mcp_register(
            &TauriProviders { app },
            &provider,
            &cwd,
            account_id.as_deref(),
            binary_path.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Text the UI appends to a turn while the session has browser access.
#[tauri::command]
pub async fn browser_agent_instructions(
    app: AppHandle,
    host: State<'_, crate::harness::HarnessHost>,
    provider: String,
    cwd: Option<String>,
) -> Result<String, String> {
    let binary_path = host.runtime_binary_path(&provider);
    tauri::async_runtime::spawn_blocking(move || {
        let checkout = cwd
            .filter(|cwd| !cwd.trim().is_empty())
            .map(|cwd| crate::fs::expand_home(&cwd));
        register::agent_instructions(
            &TauriProviders { app },
            &provider,
            checkout.as_deref(),
            binary_path.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())
}
