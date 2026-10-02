//! Tauri commands over `monocode_integrations::gitlab`.
use tauri::AppHandle;

use monocode_integrations::gitlab::{
    self, GitlabMrDiff, GitlabStatus, GitlabWorkItem, GitlabWorkItemDetails, GitlabWorkItemThread,
};

#[tauri::command(async)]
pub fn gitlab_status(app: AppHandle) -> Result<GitlabStatus, String> {
    let data_dir = crate::app_data_dir(&app)?;
    gitlab::gitlab_status(&data_dir)
}

#[tauri::command]
pub async fn gitlab_set_config(
    app: AppHandle,
    url: String,
    token: String,
) -> Result<GitlabStatus, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || gitlab::gitlab_set_config(&data_dir, url, token))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_repo(app: AppHandle, cwd: String) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || gitlab::gitlab_repo(&data_dir, cwd))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_list_work_items(
    app: AppHandle,
    cwd: String,
    kind: String,
    assigned_to_me: bool,
    state: String,
    limit: Option<u32>,
) -> Result<Vec<GitlabWorkItem>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        gitlab::gitlab_list_work_items(&data_dir, cwd, kind, assigned_to_me, state, limit)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_list_todos(
    app: AppHandle,
    kind: String,
    limit: Option<u32>,
) -> Result<Vec<GitlabWorkItem>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || gitlab::gitlab_list_todos(&data_dir, kind, limit))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_work_item_details(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
) -> Result<GitlabWorkItemDetails, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        gitlab::gitlab_work_item_details(&data_dir, repo, kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_work_item_thread(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
) -> Result<GitlabWorkItemThread, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        gitlab::gitlab_work_item_thread(&data_dir, repo, kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_work_item_comment(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
    body: String,
) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        gitlab::gitlab_work_item_comment(&data_dir, repo, kind, number, body)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_mr_diff(
    app: AppHandle,
    repo: String,
    number: i64,
) -> Result<GitlabMrDiff, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || gitlab::gitlab_mr_diff(&data_dir, repo, number))
        .await
        .map_err(|error| error.to_string())?
}
