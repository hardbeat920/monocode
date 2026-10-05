//! Tauri commands over `monocode_git::chat_background`.
use tauri::AppHandle;

#[tauri::command]
pub async fn save_chat_background(app: AppHandle, source_path: String) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::chat_background::save_chat_background(&data_dir, source_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_chat_background(app: AppHandle) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::chat_background::remove_chat_background(&data_dir)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_project_chat_background(
    app: AppHandle,
    project: String,
    source_path: String,
) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::chat_background::save_project_chat_background(&data_dir, project, source_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_project_chat_background(app: AppHandle, project: String) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::chat_background::remove_project_chat_background(&data_dir, project)
    })
    .await
    .map_err(|e| e.to_string())?
}
