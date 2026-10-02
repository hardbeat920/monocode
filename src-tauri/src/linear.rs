//! Tauri commands over `monocode_integrations::linear`.
use tauri::AppHandle;

use monocode_integrations::linear::{
    self, LinearIssue, LinearIssueDetails, LinearIssueThread, LinearStatus, LinearTeam,
};

#[tauri::command(async)]
pub fn linear_status(app: AppHandle) -> Result<LinearStatus, String> {
    let data_dir = crate::app_data_dir(&app)?;
    linear::linear_status(&data_dir)
}

#[tauri::command]
pub async fn linear_set_token(app: AppHandle, token: String) -> Result<LinearStatus, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || linear::linear_set_token(&data_dir, token))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_list_teams(app: AppHandle) -> Result<Vec<LinearTeam>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || linear::linear_list_teams(&data_dir))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_list_issues(
    app: AppHandle,
    assigned_to_me: bool,
    state: String,
    team_ids: Vec<String>,
    limit: Option<u32>,
) -> Result<Vec<LinearIssue>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        linear::linear_list_issues(&data_dir, assigned_to_me, state, team_ids, limit)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_issue_details(
    app: AppHandle,
    id: String,
) -> Result<LinearIssueDetails, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || linear::linear_issue_details(&data_dir, id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_issue_thread(app: AppHandle, id: String) -> Result<LinearIssueThread, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || linear::linear_issue_thread(&data_dir, id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_issue_comment(
    app: AppHandle,
    id: String,
    body: String,
    parent_id: String,
) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        linear::linear_issue_comment(&data_dir, id, body, parent_id)
    })
    .await
    .map_err(|error| error.to_string())?
}
