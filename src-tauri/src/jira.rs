//! Tauri commands over `monocode_integrations::jira`.
use tauri::AppHandle;

use monocode_integrations::jira::{
    self, JiraIssue, JiraIssueDetails, JiraIssueThread, JiraProject, JiraStatus,
};

#[tauri::command(async)]
pub fn jira_status(app: AppHandle) -> Result<JiraStatus, String> {
    let data_dir = crate::app_data_dir(&app)?;
    jira::jira_status(&data_dir)
}

#[tauri::command]
pub async fn jira_set_config(
    app: AppHandle,
    site: String,
    email: String,
    token: String,
) -> Result<JiraStatus, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        jira::jira_set_config(&data_dir, site, email, token)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_list_projects(app: AppHandle) -> Result<Vec<JiraProject>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || jira::jira_list_projects(&data_dir))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_list_issues(
    app: AppHandle,
    assigned_to_me: bool,
    state: String,
    project_ids: Vec<String>,
    limit: Option<u32>,
) -> Result<Vec<JiraIssue>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        jira::jira_list_issues(&data_dir, assigned_to_me, state, project_ids, limit)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_issue_details(app: AppHandle, key: String) -> Result<JiraIssueDetails, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || jira::jira_issue_details(&data_dir, key))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_issue_thread(app: AppHandle, key: String) -> Result<JiraIssueThread, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || jira::jira_issue_thread(&data_dir, key))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_issue_comment(
    app: AppHandle,
    key: String,
    body: String,
) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || jira::jira_issue_comment(&data_dir, key, body))
        .await
        .map_err(|error| error.to_string())?
}
