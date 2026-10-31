//! Dev servers per checkout live in `monocode_browser::devservers`, shared
//! with Monocode Native; this is the Tauri command the thread's browser menu
//! calls.

pub use monocode_browser::devservers::*;

/// Dev servers of the checkout holding `cwd` (a thread's working directory),
/// for the thread's browser menu.
#[tauri::command]
pub async fn dev_servers(cwd: String) -> Result<Vec<DevServer>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = crate::fs::expand_home(&cwd);
        for_dir(&dir)
    })
    .await
    .map_err(|e| e.to_string())
}
