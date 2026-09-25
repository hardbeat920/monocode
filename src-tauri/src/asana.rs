use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const ASANA_API: &str = "https://app.asana.com/api/1.0";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const OPEN_LIMIT: u32 = 40;
const ALL_LIMIT: u32 = 100;
// My Tasks are split by linked project on the client, so one fetch must hold them all.
const MAX_LIMIT: u32 = 500;
const COMMENT_LIMIT: usize = 50;
const PAGE_SIZE: &str = "100";
const MAX_PAGES: usize = 50;
// Task lists come back unordered, so this only bounds requests on huge projects.
const MAX_TASK_PAGES: usize = 10;
const FALLBACK_PROJECT_LIMIT: usize = 25;
const COMPLETED_WINDOW_DAYS: u64 = 365;
const NEUTRAL_TAG_COLOR: &str = "c7c4c4";
const TASK_FIELDS: &str = "name,permalink_url,completed,completed_at,modified_at,due_on,due_at,\
memberships.project.name,memberships.section.name,tags.name,tags.color,\
assignee.name,assignee.photo.image_60x60";
const DETAILS_FIELDS: &str = "notes,created_by.name,created_by.photo.image_60x60";
const STORY_FIELDS: &str =
    "resource_subtype,text,created_at,created_by.name,created_by.photo.image_60x60";

// Status is polled with every inbox refresh; the profile lookup runs once per token.
static PROFILE_CACHE: Mutex<Option<(String, AsanaStatus)>> = Mutex::new(None);

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaStatus {
    pub connected: bool,
    pub name: String,
    pub email: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaProject {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaLabel {
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaAssignee {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaIssue {
    pub provider: String,
    pub kind: String,
    pub id: String,
    pub identifier: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub state_type: String,
    pub updated_at: String,
    /// `YYYY-MM-DD`, empty when the task has no due date.
    pub due_on: String,
    pub labels: Vec<AsanaLabel>,
    pub assignees: Vec<AsanaAssignee>,
    pub draft: bool,
    pub repo: String,
    pub team_id: String,
    pub team_name: String,
    pub project_path: String,
    /// Every project the task belongs to, so the client can resolve local project links.
    pub projects: Vec<AsanaProjectRef>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaProjectRef {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaIssueDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaIssueComment {
    pub id: String,
    pub kind: String,
    pub author: String,
    pub author_avatar_url: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
    pub state: String,
    pub path: String,
    pub line: Option<i64>,
    pub resolved: bool,
    pub thread_id: String,
    pub replies: Vec<AsanaIssueComment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsanaIssueThread {
    pub comments: Vec<AsanaIssueComment>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AsanaWorkspace {
    gid: String,
    name: String,
}

#[derive(Debug)]
struct AsanaError {
    status: Option<u16>,
    message: String,
}

impl From<AsanaError> for String {
    fn from(error: AsanaError) -> Self {
        error.message
    }
}

#[tauri::command]
pub async fn asana_status(app: AppHandle) -> Result<AsanaStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(token) = read_token(&app)? else {
            return Ok(AsanaStatus::default());
        };
        if let Some(status) = cached_status(&token) {
            return Ok(status);
        }
        // A stored token stays connected offline; list calls surface the real error.
        match fetch_profile(&token) {
            Ok(status) => {
                set_cached_status(Some((token.as_str(), &status)));
                Ok(status)
            }
            Err(_) => Ok(AsanaStatus {
                connected: true,
                ..AsanaStatus::default()
            }),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn asana_set_token(app: AppHandle, token: String) -> Result<AsanaStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = token.trim().to_string();
        if token.is_empty() {
            delete_token(&app)?;
            set_cached_status(None);
            return Ok(AsanaStatus::default());
        }
        let status = fetch_profile(&token)?;
        write_token(&app, &token)?;
        set_cached_status(Some((token.as_str(), &status)));
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn asana_list_projects(app: AppHandle) -> Result<Vec<AsanaProject>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let mut projects = Vec::new();
        for workspace in fetch_workspaces(&token)? {
            projects.extend(fetch_projects(&token, &workspace, MAX_PAGES)?);
        }
        let mut seen = HashSet::new();
        projects.retain(|project| seen.insert(project.id.clone()));
        Ok(projects)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn asana_list_issues(
    app: AppHandle,
    assigned_to_me: bool,
    state: String,
    project_ids: Vec<String>,
    limit: Option<u32>,
) -> Result<Vec<AsanaIssue>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(token) = read_token(&app)? else {
            return Ok(Vec::new());
        };
        let include_all = state.trim().eq_ignore_ascii_case("all");
        let default_limit = if include_all { ALL_LIMIT } else { OPEN_LIMIT };
        let limit = limit.unwrap_or(default_limit).clamp(1, MAX_LIMIT) as usize;
        let cutoff = include_all.then(|| iso_days_before(unix_now(), COMPLETED_WINDOW_DAYS));
        let project_ids = project_filter(&project_ids);
        let tasks = fetch_issue_tasks(&token, assigned_to_me, cutoff.as_deref(), &project_ids)?;
        Ok(collect_issues(
            &tasks,
            &project_ids,
            cutoff.as_deref(),
            limit,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn asana_issue_details(app: AppHandle, id: String) -> Result<AsanaIssueDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let id = require_task_gid(&id)?;
        let data = asana_get(
            &token,
            &format!("/tasks/{id}"),
            &[("opt_fields", DETAILS_FIELDS)],
        )?;
        parse_asana_issue_details(&data)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn asana_issue_thread(app: AppHandle, id: String) -> Result<AsanaIssueThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let id = require_task_gid(&id)?;
        let task = asana_get(
            &token,
            &format!("/tasks/{id}"),
            &[("opt_fields", "permalink_url")],
        )?;
        let url = task
            .pointer("/data/permalink_url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let stories = asana_get_all(
            &token,
            &format!("/tasks/{id}/stories"),
            &[("opt_fields", STORY_FIELDS)],
            MAX_PAGES,
        )?;
        Ok(parse_asana_issue_thread(&stories, url))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn asana_issue_comment(
    app: AppHandle,
    id: String,
    body: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let id = require_task_gid(&id)?;
        let body = body.trim();
        if body.is_empty() {
            return Err("Comment cannot be empty".into());
        }
        let data = asana_post(
            &token,
            &format!("/tasks/{id}/stories"),
            &[("opt_fields", "gid")],
            &json!({ "data": { "text": body } }),
        )?;
        data.pointer("/data/gid")
            .and_then(Value::as_str)
            .filter(|gid| valid_gid(gid))
            .map(str::to_string)
            .ok_or_else(|| "Could not post Asana comment".to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn cached_status(token: &str) -> Option<AsanaStatus> {
    let cache = PROFILE_CACHE.lock().ok()?;
    cache
        .as_ref()
        .filter(|(cached, _)| cached == token)
        .map(|(_, status)| status.clone())
}

fn set_cached_status(entry: Option<(&str, &AsanaStatus)>) {
    if let Ok(mut cache) = PROFILE_CACHE.lock() {
        *cache = entry.map(|(token, status)| (token.to_string(), status.clone()));
    }
}

fn fetch_profile(token: &str) -> Result<AsanaStatus, String> {
    let me = asana_get(token, "/users/me", &[("opt_fields", "name,email")])?;
    parse_profile(&me)
}

fn fetch_workspaces(token: &str) -> Result<Vec<AsanaWorkspace>, String> {
    let me = asana_get(token, "/users/me", &[("opt_fields", "workspaces.name")])?;
    Ok(parse_workspaces(&me))
}

fn fetch_projects(
    token: &str,
    workspace: &AsanaWorkspace,
    max_pages: usize,
) -> Result<Vec<AsanaProject>, String> {
    let pages = asana_get_all(
        token,
        "/projects",
        &[
            ("workspace", workspace.gid.as_str()),
            ("archived", "false"),
            ("opt_fields", "name"),
        ],
        max_pages,
    )?;
    Ok(parse_asana_projects(&pages, &workspace.name))
}

fn fetch_issue_tasks(
    token: &str,
    assigned_to_me: bool,
    cutoff: Option<&str>,
    project_ids: &[String],
) -> Result<Vec<Value>, String> {
    let completed_since = cutoff.unwrap_or("now");
    let workspaces = fetch_workspaces(token)?;
    let mut tasks = Vec::new();
    if assigned_to_me {
        for workspace in &workspaces {
            tasks.extend(asana_get_all(
                token,
                "/tasks",
                &[
                    ("assignee", "me"),
                    ("workspace", workspace.gid.as_str()),
                    ("completed_since", completed_since),
                    ("opt_fields", TASK_FIELDS),
                ],
                MAX_TASK_PAGES,
            )?);
        }
        return Ok(tasks);
    }
    let mut fallback = Vec::new();
    for workspace in &workspaces {
        let result = search_workspace_tasks(token, &workspace.gid, cutoff.is_some(), project_ids);
        match search_or_fallback(result)? {
            Some(found) => tasks.extend(found),
            None => fallback.push(workspace),
        }
    }
    if fallback.is_empty() {
        return Ok(tasks);
    }
    let fallback_projects = if project_ids.is_empty() {
        let mut ids = Vec::new();
        for workspace in fallback {
            if ids.len() >= FALLBACK_PROJECT_LIMIT {
                break;
            }
            let projects = fetch_projects(token, workspace, 1)?;
            ids.extend(projects.into_iter().map(|project| project.id));
        }
        ids
    } else {
        project_ids.to_vec()
    };
    for project in fallback_projects.iter().take(FALLBACK_PROJECT_LIMIT) {
        tasks.extend(asana_get_all(
            token,
            &format!("/projects/{project}/tasks"),
            &[
                ("completed_since", completed_since),
                ("opt_fields", TASK_FIELDS),
            ],
            MAX_TASK_PAGES,
        )?);
    }
    Ok(tasks)
}

fn search_workspace_tasks(
    token: &str,
    workspace: &str,
    include_all: bool,
    project_ids: &[String],
) -> Result<Value, AsanaError> {
    let projects = project_ids.join(",");
    let mut query = vec![
        ("opt_fields", TASK_FIELDS),
        ("sort_by", "modified_at"),
        ("sort_ascending", "false"),
        ("limit", PAGE_SIZE),
    ];
    if !include_all {
        query.push(("completed", "false"));
    }
    if !projects.is_empty() {
        query.push(("projects.any", projects.as_str()));
    }
    asana_get(
        token,
        &format!("/workspaces/{workspace}/tasks/search"),
        &query,
    )
}

/// Workspace search is Premium-only; free workspaces answer 402 and fall back to project lists.
fn search_or_fallback(result: Result<Value, AsanaError>) -> Result<Option<Vec<Value>>, String> {
    match result {
        Ok(data) => data
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .map(Some)
            .ok_or_else(|| "Asana did not return tasks".to_string()),
        Err(error) if error.status == Some(402) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn project_filter(project_ids: &[String]) -> Vec<String> {
    project_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| valid_gid(id))
        .map(str::to_string)
        .collect()
}

fn asana_authorization(token: &str) -> String {
    let trimmed = token.trim();
    let raw = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim();
    format!("Bearer {raw}")
}

fn asana_get(token: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, AsanaError> {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let mut request = agent
        .get(&format!("{ASANA_API}{path}"))
        .set("Authorization", &asana_authorization(token))
        .set("Accept", "application/json");
    for (key, value) in query {
        request = request.query(key, value);
    }
    read_asana_response(request.call())
}

fn asana_post(
    token: &str,
    path: &str,
    query: &[(&str, &str)],
    body: &Value,
) -> Result<Value, AsanaError> {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let payload = serde_json::to_string(body).map_err(|error| AsanaError {
        status: None,
        message: error.to_string(),
    })?;
    let mut request = agent
        .post(&format!("{ASANA_API}{path}"))
        .set("Authorization", &asana_authorization(token))
        .set("Accept", "application/json")
        .set("Content-Type", "application/json");
    for (key, value) in query {
        request = request.query(key, value);
    }
    read_asana_response(request.send_string(&payload))
}

fn asana_get_all(
    token: &str,
    path: &str,
    query: &[(&str, &str)],
    max_pages: usize,
) -> Result<Vec<Value>, String> {
    fetch_pages(max_pages, |offset| {
        let mut query = query.to_vec();
        query.push(("limit", PAGE_SIZE));
        if let Some(offset) = offset {
            query.push(("offset", offset));
        }
        Ok(asana_get(token, path, &query)?)
    })
}

fn fetch_pages(
    max_pages: usize,
    mut fetch: impl FnMut(Option<&str>) -> Result<Value, String>,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let mut offset: Option<String> = None;
    for _ in 0..max_pages {
        let page = fetch(offset.as_deref())?;
        let data = page
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "Asana returned an unexpected response".to_string())?;
        items.extend(data.iter().cloned());
        let Some(next) = page
            .pointer("/next_page/offset")
            .and_then(Value::as_str)
            .filter(|next| !next.is_empty())
        else {
            break;
        };
        if offset.as_deref() == Some(next) {
            return Err("Asana pagination did not advance".into());
        }
        offset = Some(next.to_string());
    }
    Ok(items)
}

fn read_asana_response(result: Result<ureq::Response, ureq::Error>) -> Result<Value, AsanaError> {
    let failure = |status: Option<u16>, message: String| AsanaError { status, message };
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => {
            return Err(failure(Some(401), asana_http_error(401, "")));
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(failure(Some(status), asana_http_error(status, &body)));
        }
        Err(_) => return Err(failure(None, "Could not reach Asana".into())),
    };
    let status = response.status();
    let body = response
        .into_string()
        .map_err(|_| failure(None, "Asana returned an unreadable response".into()))?;
    if !(200..300).contains(&status) {
        return Err(failure(Some(status), asana_http_error(status, &body)));
    }
    serde_json::from_str(&body).map_err(|_| failure(None, "Asana returned invalid JSON".into()))
}

fn asana_http_error(status: u16, body: &str) -> String {
    if status == 401 {
        return "Asana personal access token is invalid".into();
    }
    if let Some(message) = asana_error_message(body) {
        return message;
    }
    match status {
        403 => "Asana denied access. Check the token's permissions".into(),
        404 => "Asana could not find that task".into(),
        429 => "Asana rate limit reached. Try again shortly".into(),
        _ => format!("Asana request failed ({status})"),
    }
}

fn asana_error_message(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    parsed
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| string_field(error, "message"))
        .filter(|message| !message.is_empty())
}

fn parse_profile(data: &Value) -> Result<AsanaStatus, String> {
    let user = data
        .get("data")
        .filter(|user| string_field(user, "gid").is_some_and(|gid| !gid.is_empty()))
        .ok_or_else(|| "Asana did not return the current user".to_string())?;
    Ok(AsanaStatus {
        connected: true,
        name: string_field(user, "name").unwrap_or_default(),
        email: string_field(user, "email").unwrap_or_default(),
    })
}

fn parse_workspaces(data: &Value) -> Vec<AsanaWorkspace> {
    data.pointer("/data/workspaces")
        .and_then(Value::as_array)
        .map(|workspaces| {
            workspaces
                .iter()
                .filter_map(|workspace| {
                    let gid = string_field(workspace, "gid").filter(|gid| valid_gid(gid))?;
                    let name = string_field(workspace, "name").unwrap_or_default();
                    Some(AsanaWorkspace { gid, name })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_asana_projects(values: &[Value], workspace_name: &str) -> Vec<AsanaProject> {
    values
        .iter()
        .filter_map(|value| {
            let id = string_field(value, "gid").filter(|gid| valid_gid(gid))?;
            let name = string_field(value, "name")
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| id.clone());
            Some(AsanaProject {
                id,
                key: workspace_name.to_string(),
                name,
            })
        })
        .collect()
}

fn collect_issues(
    tasks: &[Value],
    project_ids: &[String],
    completed_cutoff: Option<&str>,
    limit: usize,
) -> Vec<AsanaIssue> {
    let mut issues: Vec<AsanaIssue> = tasks
        .iter()
        .filter(|task| task_in_window(task, completed_cutoff))
        .filter(|task| project_ids.is_empty() || primary_membership(task, project_ids).is_some())
        .filter_map(|task| parse_asana_task(task, project_ids))
        .collect();
    let mut seen = HashSet::new();
    issues.retain(|issue| seen.insert(issue.id.clone()));
    issues.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    issues.truncate(limit);
    issues
}

/// `None` keeps incomplete tasks only; a cutoff also keeps tasks completed since then.
fn task_in_window(task: &Value, completed_cutoff: Option<&str>) -> bool {
    if !task["completed"].as_bool().unwrap_or(false) {
        return true;
    }
    let completed_at = task["completed_at"].as_str().unwrap_or_default();
    completed_cutoff.is_some_and(|cutoff| completed_at >= cutoff)
}

// With a project filter, report a visible project so the client's hidden-project check agrees.
fn primary_membership<'a>(task: &'a Value, project_ids: &[String]) -> Option<&'a Value> {
    task.get("memberships")
        .and_then(Value::as_array)?
        .iter()
        .find(|membership| {
            let gid = membership
                .pointer("/project/gid")
                .and_then(Value::as_str)
                .unwrap_or_default();
            !gid.is_empty() && (project_ids.is_empty() || project_ids.iter().any(|id| id == gid))
        })
}

fn parse_asana_task(node: &Value, project_ids: &[String]) -> Option<AsanaIssue> {
    let id = string_field(node, "gid").filter(|gid| valid_gid(gid))?;
    let (fallback_state, state_type) = if node["completed"].as_bool().unwrap_or(false) {
        ("Completed", "done")
    } else {
        ("Open", "new")
    };
    let membership = primary_membership(node, project_ids);
    let project = membership.and_then(|value| value.get("project"));
    let project_name = project
        .and_then(|value| string_field(value, "name"))
        .unwrap_or_default();
    let (assignee, avatar_url) = person_fields(node.get("assignee"));
    Some(AsanaIssue {
        provider: "asana".into(),
        kind: "asana".into(),
        number: 0,
        title: string_field(node, "name").unwrap_or_default(),
        url: string_field(node, "permalink_url").unwrap_or_default(),
        state: membership
            .and_then(|value| value.get("section"))
            .and_then(|section| string_field(section, "name"))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| fallback_state.into()),
        state_type: state_type.into(),
        updated_at: string_field(node, "modified_at").unwrap_or_default(),
        due_on: due_date(node),
        labels: parse_tags(node),
        assignees: assignee
            .map(|login| vec![AsanaAssignee { login, avatar_url }])
            .unwrap_or_default(),
        draft: false,
        repo: project_name.clone(),
        team_id: project
            .and_then(|value| string_field(value, "gid"))
            .unwrap_or_default(),
        team_name: project_name,
        project_path: String::new(),
        projects: membership_projects(node),
        identifier: id.clone(),
        id,
    })
}

// `due_at` tasks also carry `due_on`; fall back to its date part in case one is missing.
fn due_date(node: &Value) -> String {
    string_field(node, "due_on")
        .filter(|date| !date.is_empty())
        .or_else(|| string_field(node, "due_at").and_then(|at| at.get(..10).map(str::to_string)))
        .unwrap_or_default()
}

fn membership_projects(node: &Value) -> Vec<AsanaProjectRef> {
    let mut seen = HashSet::new();
    node.get("memberships")
        .and_then(Value::as_array)
        .map(|memberships| {
            memberships
                .iter()
                .filter_map(|membership| {
                    let project = membership.get("project")?;
                    let id = string_field(project, "gid").filter(|gid| valid_gid(gid))?;
                    let name = string_field(project, "name").unwrap_or_default();
                    Some(AsanaProjectRef { id, name })
                })
                .filter(|project| seen.insert(project.id.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_tags(node: &Value) -> Vec<AsanaLabel> {
    node.get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| {
                    let name = string_field(tag, "name").filter(|name| !name.is_empty())?;
                    let color = tag_color(tag.get("color").and_then(Value::as_str));
                    Some(AsanaLabel {
                        name,
                        color: color.into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Asana names its palette; legacy `dark-*`/`light-*` names alias the current colors.
fn tag_color(name: Option<&str>) -> &'static str {
    match name.unwrap_or_default() {
        "red" | "dark-red" => "e8384f",
        "orange" | "dark-orange" => "fd612c",
        "yellow-orange" | "light-orange" => "fd9a00",
        "yellow" | "dark-brown" | "light-yellow" => "eec300",
        "yellow-green" | "light-green" => "a4cf30",
        "green" | "dark-green" => "62d26f",
        "blue-green" | "light-teal" => "37c5ab",
        "aqua" | "dark-teal" => "20aaea",
        "blue" | "light-blue" | "dark-blue" => "4186e0",
        "indigo" | "dark-purple" => "7a6ff0",
        "purple" | "light-purple" => "aa62e3",
        "magenta" | "light-pink" => "e362e3",
        "hot-pink" | "dark-pink" => "ea4e9d",
        "pink" | "light-red" => "fc91ad",
        "cool-gray" | "light-warm-gray" | "dark-warm-gray" => "8da3a6",
        _ => NEUTRAL_TAG_COLOR,
    }
}

fn parse_asana_issue_details(data: &Value) -> Result<AsanaIssueDetails, String> {
    let task = data
        .get("data")
        .filter(|task| task.is_object())
        .ok_or_else(|| "Asana did not return that task".to_string())?;
    let (author, author_avatar_url) = person_fields(task.get("created_by"));
    Ok(AsanaIssueDetails {
        body: string_field(task, "notes").unwrap_or_default(),
        author: author.unwrap_or_default(),
        author_avatar_url,
    })
}

fn parse_asana_issue_thread(stories: &[Value], task_url: &str) -> AsanaIssueThread {
    let mut comments: Vec<AsanaIssueComment> = stories
        .iter()
        .filter(|story| story["resource_subtype"].as_str() == Some("comment_added"))
        .filter_map(|story| parse_asana_comment(story, task_url))
        .collect();
    comments.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    let overflow = comments.len().saturating_sub(COMMENT_LIMIT);
    comments.drain(..overflow);
    AsanaIssueThread {
        truncated: overflow > 0,
        comments,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    }
}

fn parse_asana_comment(node: &Value, task_url: &str) -> Option<AsanaIssueComment> {
    let id = string_field(node, "gid").filter(|gid| valid_gid(gid))?;
    let (author, author_avatar_url) = person_fields(node.get("created_by"));
    Some(AsanaIssueComment {
        id,
        kind: "comment".into(),
        author: author.unwrap_or_default(),
        author_avatar_url,
        body: string_field(node, "text").unwrap_or_default(),
        created_at: string_field(node, "created_at").unwrap_or_default(),
        url: task_url.to_string(),
        state: String::new(),
        path: String::new(),
        line: None,
        resolved: false,
        thread_id: String::new(),
        replies: Vec::new(),
    })
}

fn valid_gid(gid: &str) -> bool {
    !gid.is_empty() && gid.len() < 64 && gid.bytes().all(|byte| byte.is_ascii_digit())
}

fn require_task_gid(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if valid_gid(id) {
        Ok(id)
    } else {
        Err("Missing Asana task".into())
    }
}

fn person_fields(node: Option<&Value>) -> (Option<String>, String) {
    let Some(node) = node.filter(|node| node.is_object()) else {
        return (None, String::new());
    };
    (
        string_field(node, "name").filter(|name| !name.is_empty()),
        node.pointer("/photo/image_60x60")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    )
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// UTC midnight `days` before `now_secs`, in the ISO form `completed_since` accepts.
fn iso_days_before(now_secs: u64, days: u64) -> String {
    let (year, month, day) = civil_from_days((now_secs / 86_400).saturating_sub(days) as i64);
    format!("{year:04}-{month:02}-{day:02}T00:00:00.000Z")
}

// Howard Hinnant's days-since-epoch to proleptic Gregorian date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month as u32, day)
}

// ---- Token storage ----

fn token_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("asana-token"))
}

fn read_token(app: &AppHandle) -> Result<Option<String>, String> {
    let path = token_path(app)?;
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let token = raw.trim().to_string();
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn require_token(app: &AppHandle) -> Result<String, String> {
    read_token(app)?.ok_or_else(|| "Connect Asana in Settings".to_string())
}

fn write_token(app: &AppHandle, token: &str) -> Result<(), String> {
    let path = token_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    write_secret_file(&path, token)
}

fn delete_token(app: &AppHandle) -> Result<(), String> {
    let path = token_path(app)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_secret_file(path: &std::path::Path, token: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| error.to_string())?;
        file.write_all(token.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, token).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(gid: &str, modified_at: &str) -> Value {
        json!({ "gid": gid, "name": gid, "completed": false, "modified_at": modified_at })
    }

    fn comment(gid: u32, created_at: &str) -> Value {
        json!({
            "gid": gid.to_string(),
            "resource_subtype": "comment_added",
            "text": format!("Comment {gid}"),
            "created_at": created_at,
            "created_by": { "name": "Ada" }
        })
    }

    #[test]
    fn authorization_is_bearer_token() {
        assert_eq!(asana_authorization(" 2/123/abc "), "Bearer 2/123/abc");
        assert_eq!(asana_authorization("Bearer 2/123/abc"), "Bearer 2/123/abc");
    }

    #[test]
    fn gids_are_validated() {
        assert!(valid_gid("1204567890"));
        assert!(!valid_gid(""));
        assert!(!valid_gid("12a4"));
        assert!(!valid_gid("12/../34"));
        assert!(!valid_gid("\u{661}\u{662}"));
        assert_eq!(require_task_gid(" 1204567890 ").unwrap(), "1204567890");
        assert!(require_task_gid("1204567890?opt_fields=x").is_err());
        assert_eq!(
            project_filter(&[" 111 ".into(), "".into(), "x,1".into(), "222".into()]),
            vec!["111".to_string(), "222".to_string()]
        );
    }

    #[test]
    fn search_falls_back_only_on_payment_required() {
        let tasks = search_or_fallback(Ok(json!({ "data": [{ "gid": "1" }] }))).unwrap();
        assert_eq!(tasks.map(|tasks| tasks.len()), Some(1));
        let premium_only = AsanaError {
            status: Some(402),
            message: "Payment Required".into(),
        };
        assert_eq!(search_or_fallback(Err(premium_only)).unwrap(), None);
        let denied = AsanaError {
            status: Some(403),
            message: "Forbidden".into(),
        };
        assert_eq!(search_or_fallback(Err(denied)).unwrap_err(), "Forbidden");
        let offline = AsanaError {
            status: None,
            message: "Could not reach Asana".into(),
        };
        assert!(search_or_fallback(Err(offline)).is_err());
    }

    #[test]
    fn maps_tag_colors_to_hex() {
        assert_eq!(tag_color(Some("dark-red")), "e8384f");
        assert_eq!(tag_color(Some("red")), "e8384f");
        assert_eq!(tag_color(Some("light-blue")), "4186e0");
        assert_eq!(tag_color(Some("hot-pink")), "ea4e9d");
        assert_eq!(tag_color(Some("none")), NEUTRAL_TAG_COLOR);
        assert_eq!(tag_color(Some("chartreuse")), NEUTRAL_TAG_COLOR);
        assert_eq!(tag_color(None), NEUTRAL_TAG_COLOR);
        for color in [tag_color(Some("aqua")), NEUTRAL_TAG_COLOR] {
            assert!(color.len() == 6 && color.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn maps_task_fields() {
        let node = json!({
            "gid": "1204567890",
            "name": "Fix auth",
            "permalink_url": "https://app.asana.com/0/111/1204567890",
            "completed": false,
            "modified_at": "2026-08-27T10:00:00.000Z",
            "due_on": "2026-09-30",
            "memberships": [{
                "project": { "gid": "111", "name": "Engineering" },
                "section": { "gid": "900", "name": "In Progress" }
            }],
            "tags": [
                { "gid": "5", "name": "bug", "color": "dark-red" },
                { "gid": "6", "name": "backend", "color": null }
            ],
            "assignee": {
                "gid": "7",
                "name": "Maya",
                "photo": { "image_60x60": "https://s3.asana.com/maya.png" }
            }
        });
        let issue = parse_asana_task(&node, &[]).unwrap();
        assert_eq!(
            issue,
            AsanaIssue {
                provider: "asana".into(),
                kind: "asana".into(),
                id: "1204567890".into(),
                identifier: "1204567890".into(),
                number: 0,
                title: "Fix auth".into(),
                url: "https://app.asana.com/0/111/1204567890".into(),
                state: "In Progress".into(),
                state_type: "new".into(),
                updated_at: "2026-08-27T10:00:00.000Z".into(),
                due_on: "2026-09-30".into(),
                labels: vec![
                    AsanaLabel {
                        name: "bug".into(),
                        color: "e8384f".into(),
                    },
                    AsanaLabel {
                        name: "backend".into(),
                        color: NEUTRAL_TAG_COLOR.into(),
                    },
                ],
                assignees: vec![AsanaAssignee {
                    login: "Maya".into(),
                    avatar_url: "https://s3.asana.com/maya.png".into(),
                }],
                draft: false,
                repo: "Engineering".into(),
                team_id: "111".into(),
                team_name: "Engineering".into(),
                project_path: String::new(),
                projects: vec![AsanaProjectRef {
                    id: "111".into(),
                    name: "Engineering".into(),
                }],
            }
        );
    }

    #[test]
    fn maps_completed_task_without_project() {
        let node = json!({
            "gid": "42",
            "name": "Ship it",
            "completed": true,
            "memberships": [],
            "assignee": null
        });
        let issue = parse_asana_task(&node, &[]).unwrap();
        assert_eq!(issue.state, "Completed");
        assert_eq!(issue.state_type, "done");
        assert!(issue.assignees.is_empty());
        assert!(issue.team_id.is_empty());
        assert!(parse_asana_task(&json!({ "gid": "abc" }), &[]).is_none());
        let open = parse_asana_task(&json!({ "gid": "43" }), &[]).unwrap();
        assert_eq!(open.state, "Open");
        assert_eq!(open.state_type, "new");
        assert!(open.due_on.is_empty());
        let timed = json!({ "gid": "44", "due_at": "2026-10-02T15:00:00.000Z" });
        assert_eq!(parse_asana_task(&timed, &[]).unwrap().due_on, "2026-10-02");
    }

    #[test]
    fn issue_serializes_jira_field_set() {
        let issue = parse_asana_task(&task("1", "2026-08-27T10:00:00.000Z"), &[]).unwrap();
        let value = serde_json::to_value(&issue).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "assignees",
                "draft",
                "dueOn",
                "id",
                "identifier",
                "kind",
                "labels",
                "number",
                "projectPath",
                "projects",
                "provider",
                "repo",
                "state",
                "stateType",
                "teamId",
                "teamName",
                "title",
                "updatedAt",
                "url",
            ]
        );
    }

    #[test]
    fn collect_issues_filters_dedupes_and_sorts() {
        let mut in_both = task("3", "2026-08-03T00:00:00.000Z");
        in_both["memberships"] = json!([
            { "project": { "gid": "111", "name": "Hidden" }, "section": { "name": "Todo" } },
            { "project": { "gid": "222", "name": "Visible" }, "section": { "name": "Doing" } }
        ]);
        let mut hidden_only = task("4", "2026-08-04T00:00:00.000Z");
        hidden_only["memberships"] = json!([{ "project": { "gid": "111", "name": "Hidden" } }]);
        let mut old_done = task("5", "2026-08-05T00:00:00.000Z");
        old_done["completed"] = json!(true);
        old_done["completed_at"] = json!("2024-01-01T00:00:00.000Z");
        let mut recent_done = task("6", "2026-08-06T00:00:00.000Z");
        recent_done["completed"] = json!(true);
        recent_done["completed_at"] = json!("2026-08-06T00:00:00.000Z");
        let tasks = [
            task("1", "2026-08-01T00:00:00.000Z"),
            task("2", "2026-08-02T00:00:00.000Z"),
            in_both.clone(),
            in_both,
            hidden_only,
            old_done,
            recent_done,
        ];

        let open = collect_issues(&tasks, &[], None, 40);
        let ids: Vec<&str> = open.iter().map(|issue| issue.id.as_str()).collect();
        assert_eq!(ids, vec!["4", "3", "2", "1"]);
        let project_ids: Vec<&str> = open[1]
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect();
        assert_eq!(project_ids, vec!["111", "222"]);

        let all = collect_issues(&tasks, &[], Some("2025-09-26T00:00:00.000Z"), 2);
        let ids: Vec<&str> = all.iter().map(|issue| issue.id.as_str()).collect();
        assert_eq!(ids, vec!["6", "4"]);

        let visible = collect_issues(&tasks, &["222".into()], None, 40);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].team_id, "222");
        assert_eq!(visible[0].team_name, "Visible");
        assert_eq!(visible[0].state, "Doing");
    }

    #[test]
    fn thread_keeps_latest_comments_oldest_first() {
        let mut stories: Vec<Value> = (1..=55)
            .rev()
            .map(|gid| comment(gid, &format!("2026-08-01T00:00:{gid:02}.000Z")))
            .collect();
        stories.push(json!({
            "gid": "900",
            "resource_subtype": "assigned",
            "text": "assigned to you",
            "created_at": "2026-08-02T00:00:00.000Z"
        }));
        let url = "https://app.asana.com/0/111/1204567890";
        let thread = parse_asana_issue_thread(&stories, url);
        assert!(thread.truncated);
        assert_eq!(thread.comments.len(), COMMENT_LIMIT);
        assert_eq!(thread.comments[0].id, "6");
        assert_eq!(thread.comments[49].id, "55");
        assert_eq!(thread.comments[0].kind, "comment");
        assert_eq!(thread.comments[0].author, "Ada");
        assert_eq!(thread.comments[0].body, "Comment 6");
        assert_eq!(thread.comments[0].url, url);
        assert!(thread.comments.iter().all(|comment| comment.id != "900"));

        let short = parse_asana_issue_thread(&stories[..3], url);
        assert!(!short.truncated);
        assert_eq!(short.comments.len(), 3);
        assert_eq!(short.comments[0].id, "53");
    }

    #[test]
    fn parse_asana_issue_details_reads_notes_and_creator() {
        let data = json!({
            "data": {
                "gid": "1",
                "notes": "Steps to reproduce\n",
                "created_by": {
                    "gid": "7",
                    "name": "Ada",
                    "photo": { "image_60x60": "https://s3.asana.com/ada.png" }
                }
            }
        });
        let details = parse_asana_issue_details(&data).unwrap();
        assert_eq!(details.body, "Steps to reproduce");
        assert_eq!(details.author, "Ada");
        assert_eq!(details.author_avatar_url, "https://s3.asana.com/ada.png");

        let orphan = parse_asana_issue_details(&json!({ "data": { "notes": "" } })).unwrap();
        assert!(orphan.author.is_empty());
        assert!(orphan.author_avatar_url.is_empty());
    }

    #[test]
    fn fetches_every_page() {
        let mut offsets = Vec::new();
        let items = fetch_pages(MAX_PAGES, |offset| {
            offsets.push(offset.map(str::to_string));
            Ok(match offset {
                None => json!({ "data": [{ "gid": "1" }], "next_page": { "offset": "a+b/c=" } }),
                Some(_) => json!({ "data": [{ "gid": "2" }], "next_page": null }),
            })
        })
        .unwrap();
        assert_eq!(offsets, vec![None, Some("a+b/c=".to_string())]);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn rejects_stalled_pagination() {
        let stalled = json!({ "data": [], "next_page": { "offset": "same" } });
        assert!(fetch_pages(MAX_PAGES, |_| Ok(stalled.clone())).is_err());
    }

    #[test]
    fn parses_workspaces_and_projects() {
        let me = json!({ "data": { "gid": "7", "workspaces": [
            { "gid": "100", "name": "Acme" },
            { "gid": "../x", "name": "Bad" }
        ] } });
        assert_eq!(
            parse_workspaces(&me),
            vec![AsanaWorkspace {
                gid: "100".into(),
                name: "Acme".into(),
            }]
        );
        let projects = parse_asana_projects(
            &[
                json!({ "gid": "111", "name": "Engineering" }),
                json!({ "gid": "", "name": "Skip" }),
            ],
            "Acme",
        );
        assert_eq!(
            projects,
            vec![AsanaProject {
                id: "111".into(),
                key: "Acme".into(),
                name: "Engineering".into(),
            }]
        );
    }

    #[test]
    fn parse_profile_requires_a_user() {
        let me = json!({ "data": { "gid": "7", "name": "Ada", "email": "ada@acme.com" } });
        let status = parse_profile(&me).unwrap();
        assert_eq!(
            status,
            AsanaStatus {
                connected: true,
                name: "Ada".into(),
                email: "ada@acme.com".into(),
            }
        );
        assert!(parse_profile(&json!({ "data": {} })).is_err());
    }

    #[test]
    fn http_errors_are_readable() {
        assert_eq!(
            asana_http_error(401, r#"{"errors":[{"message":"Not Authorized"}]}"#),
            "Asana personal access token is invalid"
        );
        assert_eq!(
            asana_http_error(404, r#"{"errors":[{"message":"Unknown object"}]}"#),
            "Unknown object"
        );
        assert_eq!(asana_http_error(500, ""), "Asana request failed (500)");
    }

    #[test]
    fn iso_days_before_formats_utc_dates() {
        assert_eq!(iso_days_before(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_days_before(19_783 * 86_400 + 5_000, 1),
            "2024-02-29T00:00:00.000Z"
        );
        assert_eq!(
            iso_days_before(20_000 * 86_400, 365),
            "2023-10-05T00:00:00.000Z"
        );
    }
}
