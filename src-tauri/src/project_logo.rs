//! Tauri commands over `monocode_git::project_logo`.
use tauri::AppHandle;

#[tauri::command]
pub async fn save_project_logo(
    app: AppHandle,
    project: String,
    source_path: String,
) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::project_logo::save_project_logo(&data_dir, project, source_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn forget_logo_file(app: AppHandle, path: String) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::project_logo::forget_logo_file(&data_dir, path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_project_logo(app: AppHandle, project: String) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        monocode_git::project_logo::remove_project_logo(&data_dir, project)
    })
    .await
    .map_err(|e| e.to_string())?
}
