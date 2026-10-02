//! Tauri commands over `monocode_process::external_editor`.
use monocode_process::external_editor::{self, ExternalEditor};

#[tauri::command(async)]
pub async fn list_external_editors() -> Result<Vec<ExternalEditor>, String> {
    tauri::async_runtime::spawn_blocking(external_editor::list_external_editors)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub async fn open_in_external_editor(editor_id: String, cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        external_editor::open_in_external_editor(editor_id, cwd)
    })
    .await
    .map_err(|error| error.to_string())?
}
