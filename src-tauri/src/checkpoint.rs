//! Tauri commands over `monocode_store::checkpoint`.
use tauri::{AppHandle, Manager, State};

use monocode_store::checkpoint::{
    self, CheckpointApplyResult, CheckpointFileDiff, CheckpointStatus, CheckpointStore,
};

pub fn init(app: &AppHandle) -> Result<(), String> {
    app.manage(checkpoint::init(&crate::app_data_dir(app)?)?);
    Ok(())
}

#[tauri::command]
pub async fn session_checkpoint_ensure(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_ensure(&store, session_id, cwd)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_prepare(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    paths: Vec<String>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_prepare(&store, session_id, cwd, paths)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_capture(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    paths: Vec<String>,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_capture(&store, session_id, cwd, paths)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_status(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<CheckpointStatus, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_status(&store, session_id, cwd)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_apply(
    store: State<'_, CheckpointStore>,
    session_id: String,
    from_cwd: String,
    to_cwd: String,
) -> Result<CheckpointApplyResult, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_apply(&store, session_id, from_cwd, to_cwd)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_cleanup_safe(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
) -> Result<bool, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_cleanup_safe(&store, session_id, cwd)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_forget(
    store: State<'_, CheckpointStore>,
    session_id: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_forget(&store, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_file_diff(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: String,
) -> Result<CheckpointFileDiff, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_file_diff(&store, session_id, cwd, relative)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_undo(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: Option<String>,
) -> Result<CheckpointStatus, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_undo(&store, session_id, cwd, relative)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn session_checkpoint_keep(
    store: State<'_, CheckpointStore>,
    session_id: String,
    cwd: String,
    relative: Option<String>,
) -> Result<CheckpointStatus, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        checkpoint::session_checkpoint_keep(&store, session_id, cwd, relative)
    })
    .await
    .map_err(|e| e.to_string())?
}
