use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const DEFAULT_LIMIT: u32 = 40;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
const USER_AGENT: &str = "MonoCode";
const API_VERSION: &str = "7.1";
const CONNECTION_DATA_API_VERSION: &str = "7.1-preview.1";
const WIT_COMMENTS_API_VERSION: &str = "7.1-preview.4";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsStatus {
    pub connected: bool,
    pub url: String,
    pub organization: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct AzureDevOpsConfig {
    url: String,
    token: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsLabel {
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsAssignee {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsWorkItem {
    pub kind: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub updated_at: String,
    pub labels: Vec<AzureDevOpsLabel>,
    pub assignees: Vec<AzureDevOpsAssignee>,
    pub draft: bool,
    pub repo: String,
    pub attention_reason: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsWorkItemDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
    pub review_decision: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsWorkItemComment {
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
    pub replies: Vec<AzureDevOpsWorkItemComment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsWorkItemThread {
    pub comments: Vec<AzureDevOpsWorkItemComment>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsMrFile {
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsMrDiff {
    pub additions: i64,
    pub deletions: i64,
    pub files: Vec<AzureDevOpsMrFile>,
    pub patch: String,
    pub truncated: bool,
}

#[tauri::command(async)]
pub fn azure_devops_status(app: AppHandle) -> Result<AzureDevOpsStatus, String> {
    let config = read_config(&app)?;
    Ok(AzureDevOpsStatus {
        connected: config.is_some(),
        url: config
            .as_ref()
            .map(|config| config.url.clone())
            .unwrap_or_default(),
        organization: config
            .as_ref()
            .map(|config| organization_from_url(&config.url))
            .unwrap_or_default(),
    })
}

#[tauri::command]
pub async fn azure_devops_set_config(
    app: AppHandle,
    url: String,
    token: String,
) -> Result<AzureDevOpsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = normalize_azure_devops_url(&url)?;
        let token = token.trim().to_string();
        if token.is_empty() {
            delete_config(&app)?;
            let organization = organization_from_url(&url);
            return Ok(AzureDevOpsStatus {
                connected: false,
                url,
                organization,
            });
        }
        let config = AzureDevOpsConfig { url, token };
        // Validates the PAT and fails fast on 401/203 (non-visualstudio passthrough).
        let user_id = azure_current_user_id(&config)?;
        if user_id.trim().is_empty() {
            return Err("Azure DevOps did not return the current user".into());
        }
        write_config(&app, &config)?;
        Ok(AzureDevOpsStatus {
            connected: true,
            organization: organization_from_url(&config.url),
            url: config.url,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_repo(app: AppHandle, cwd: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        azure_devops_repo_for(&expand_home(&cwd), &config.url)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_list_work_items(
    app: AppHandle,
    cwd: String,
    kind: String,
    assigned_to_me: bool,
    state: String,
    limit: Option<u32>,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = azure_devops_repo_for(&expand_home(&cwd), &config.url)?;
        azure_devops_list_work_items_for(
            &config,
            &repo,
            &kind,
            assigned_to_me,
            &state,
            limit.unwrap_or(DEFAULT_LIMIT),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_list_todos(
    app: AppHandle,
    kind: String,
    limit: Option<u32>,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        azure_devops_list_todos_for(&config, &kind, limit.unwrap_or(DEFAULT_LIMIT))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_work_item_details(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
) -> Result<AzureDevOpsWorkItemDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        azure_devops_work_item_details_for(&config, &repo, &kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_work_item_thread(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
) -> Result<AzureDevOpsWorkItemThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        azure_devops_work_item_thread_for(&config, &repo, &kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_work_item_comment(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
    body: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        azure_devops_work_item_comment_for(&config, &repo, &kind, number, &body)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn azure_devops_mr_diff(
    app: AppHandle,
    repo: String,
    number: i64,
) -> Result<AzureDevOpsMrDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        azure_devops_mr_diff_for(&config, &repo, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn azure_devops_list_work_items_for(
    config: &AzureDevOpsConfig,
    repo: &str,
    kind: &str,
    assigned_to_me: bool,
    state: &str,
    limit: u32,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    validate_kind(kind)?;
    let (project, repo_name) = split_repo(repo)?;
    let limit = limit.clamp(1, 100);
    if kind == "pr" {
        let me = if assigned_to_me {
            Some(azure_current_user_id(config)?)
        } else {
            None
        };
        return azure_list_prs_for(config, &project, &repo_name, me.as_deref(), state, limit);
    }
    azure_list_wit_for(config, Some(&project), assigned_to_me, state, limit)
}

fn azure_devops_list_todos_for(
    config: &AzureDevOpsConfig,
    kind: &str,
    limit: u32,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    validate_kind(kind)?;
    let limit = limit.clamp(1, 100);
    if kind == "pr" {
        return azure_list_pr_reviews_for(config, limit);
    }
    let mut items = azure_list_wit_for(config, None, true, "open", limit)?;
    for item in &mut items {
        if item.attention_reason.is_empty() {
            item.attention_reason = "assigned".into();
        }
    }
    Ok(items)
}

fn azure_devops_work_item_details_for(
    config: &AzureDevOpsConfig,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<AzureDevOpsWorkItemDetails, String> {
    validate_item(kind, number)?;
    let (project, repo_name) = split_repo(repo)?;
    if kind == "pr" {
        let path = format!(
            "/{}/_apis/git/repositories/{}/pullrequests/{}?api-version={}",
            encode_segment(&project),
            encode_segment(&repo_name),
            number,
            API_VERSION
        );
        let response = azure_get(config, &path)?;
        return parse_pr_details(&response.value);
    }
    let path = format!(
        "/_apis/wit/workitems/{}?$expand=relations&api-version={}",
        number, API_VERSION
    );
    let response = azure_get(config, &path)?;
    parse_wit_details(&response.value)
}

fn azure_devops_work_item_thread_for(
    config: &AzureDevOpsConfig,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<AzureDevOpsWorkItemThread, String> {
    validate_item(kind, number)?;
    let (project, repo_name) = split_repo(repo)?;
    if kind == "pr" {
        let path = format!(
            "/{}/_apis/git/repositories/{}/pullrequests/{}/threads?api-version={}",
            encode_segment(&project),
            encode_segment(&repo_name),
            number,
            API_VERSION
        );
        let response = azure_get(config, &path)?;
        let pr_url = pr_web_url(config, &project, &repo_name, number);
        return parse_pr_thread(&response.value, &pr_url, response.truncated);
    }
    let path = format!(
        "/_apis/wit/workitems/{}/comments?api-version={}",
        number, WIT_COMMENTS_API_VERSION
    );
    let response = azure_get(config, &path)?;
    parse_wit_thread(&response.value, response.truncated)
}

fn azure_devops_work_item_comment_for(
    config: &AzureDevOpsConfig,
    repo: &str,
    kind: &str,
    number: i64,
    body: &str,
) -> Result<String, String> {
    validate_item(kind, number)?;
    let body = body.trim();
    if body.is_empty() {
        return Err("Comment cannot be empty".into());
    }
    let (project, repo_name) = split_repo(repo)?;
    if kind == "pr" {
        let path = format!(
            "/{}/_apis/git/repositories/{}/pullrequests/{}/threads?api-version={}",
            encode_segment(&project),
            encode_segment(&repo_name),
            number,
            API_VERSION
        );
        let payload = json!({
            "comments": [{ "parentCommentId": 0, "content": body, "commentType": 1 }],
            "status": 1,
        });
        azure_post_json(config, &path, payload)?;
        return Ok(pr_web_url(config, &project, &repo_name, number));
    }
    let path = format!(
        "/_apis/wit/workitems/{}/comments?api-version={}",
        number, WIT_COMMENTS_API_VERSION
    );
    let response = azure_post_json(config, &path, json!({ "text": body }))?;
    Ok(string_field(&response.value, "url").unwrap_or_default())
}

fn azure_devops_mr_diff_for(
    config: &AzureDevOpsConfig,
    repo: &str,
    number: i64,
) -> Result<AzureDevOpsMrDiff, String> {
    validate_item("pr", number)?;
    let (project, repo_name) = split_repo(repo)?;
    let iterations_path = format!(
        "/{}/_apis/git/repositories/{}/pullrequests/{}/iterations?api-version={}",
        encode_segment(&project),
        encode_segment(&repo_name),
        number,
        API_VERSION
    );
    let iterations = azure_get(config, &iterations_path)?;
    let iteration_id = latest_iteration_id(&iterations.value).unwrap_or(0);
    if iteration_id <= 0 {
        return Ok(AzureDevOpsMrDiff {
            additions: 0,
            deletions: 0,
            files: Vec::new(),
            patch: String::new(),
            truncated: false,
        });
    }
    let changes_path = format!(
        "/{}/_apis/git/repositories/{}/pullrequests/{}/iterations/{}/changes?$top=100&api-version={}",
        encode_segment(&project),
        encode_segment(&repo_name),
        number,
        iteration_id,
        API_VERSION
    );
    let response = azure_get(config, &changes_path)?;
    Ok(parse_iteration_changes(&response.value, response.truncated))
}

// ---- Pull requests ----

fn azure_list_prs_for(
    config: &AzureDevOpsConfig,
    project: &str,
    repo_name: &str,
    assigned_user_id: Option<&str>,
    state: &str,
    limit: u32,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    let repo = format!("{project}/{repo_name}");
    let reviewer = assigned_user_id.map(str::trim).filter(|id| !id.is_empty());
    if state.trim().eq_ignore_ascii_case("all") {
        let mut active = azure_fetch_prs(
            config,
            project,
            repo_name,
            &repo,
            "active",
            limit * 2,
            reviewer,
        )?;
        let mut completed = azure_fetch_prs(
            config,
            project,
            repo_name,
            &repo,
            "completed",
            limit * 2,
            reviewer,
        )?;
        // Abandoned PRs surface through the "all" view too, like closed GitHub PRs.
        let mut abandoned = azure_fetch_prs(
            config,
            project,
            repo_name,
            &repo,
            "abandoned",
            limit * 2,
            reviewer,
        )?;
        let mut items = Vec::new();
        items.append(&mut active);
        items.append(&mut completed);
        items.append(&mut abandoned);
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        items.truncate(limit as usize);
        return Ok(items);
    }
    azure_fetch_prs(config, project, repo_name, &repo, "active", limit, reviewer)
}

fn azure_fetch_prs(
    config: &AzureDevOpsConfig,
    project: &str,
    repo_name: &str,
    repo: &str,
    status: &str,
    limit: u32,
    reviewer_id: Option<&str>,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    let reviewer = reviewer_id
        .map(|id| format!("&searchCriteria.reviewerId={}", encode_segment(id)))
        .unwrap_or_default();
    let path = format!(
        "/{}/_apis/git/repositories/{}/pullrequests?searchCriteria.status={}{}&$top={}&api-version={}",
        encode_segment(project),
        encode_segment(repo_name),
        status,
        reviewer,
        limit.clamp(1, 100),
        API_VERSION
    );
    let response = azure_get(config, &path)?;
    parse_pr_list(&response.value, repo, &config.url)
}

fn azure_list_pr_reviews_for(
    config: &AzureDevOpsConfig,
    limit: u32,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    let user_id = azure_current_user_id(config)?;
    let path = format!(
        "/_apis/git/pullrequests?searchCriteria.status=active&searchCriteria.reviewerId={}&$top={}&api-version={}",
        encode_segment(&user_id),
        limit.clamp(1, 100),
        API_VERSION
    );
    let response = azure_get(config, &path)?;
    let rows = response
        .value
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "Azure DevOps did not return pull requests".to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let (project, repo_name) = pr_repo_parts(row);
        let repo = match (project, repo_name) {
            (Some(project), Some(repo_name)) => format!("{project}/{repo_name}"),
            _ => continue,
        };
        if let Some(mut item) = parse_pr(row, &repo, &config.url) {
            if item.attention_reason.is_empty() {
                item.attention_reason = "review_requested".into();
            }
            items.push(item);
        }
    }
    Ok(items)
}

// ---- Work items (Boards) ----

fn azure_list_wit_for(
    config: &AzureDevOpsConfig,
    project: Option<&str>,
    assigned_to_me: bool,
    state: &str,
    limit: u32,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    let limit = limit.clamp(1, 100);
    let wiql = wit_wiql(project, assigned_to_me, state);
    let ids = azure_query_wit_ids(config, &wiql, limit)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    azure_fetch_wit_batch(config, &ids)
}

fn wit_wiql(project: Option<&str>, assigned_to_me: bool, state: &str) -> String {
    let mut query = String::from("SELECT [System.Id] FROM WorkItems WHERE ");
    let mut clauses: Vec<String> = Vec::new();
    if let Some(project) = project.map(str::trim).filter(|p| !p.is_empty()) {
        clauses.push(format!(
            "[System.TeamProject] = '{}'",
            project.replace('\'', "''")
        ));
    }
    if assigned_to_me {
        clauses.push("[System.AssignedTo] = @me".into());
    }
    if !state.trim().eq_ignore_ascii_case("all") {
        clauses.push("[System.State] NOT IN ('Closed', 'Done', 'Removed')".into());
    }
    if clauses.is_empty() {
        clauses.push("[System.Id] > 0".into());
    }
    query.push_str(&clauses.join(" AND "));
    query.push_str(" ORDER BY [System.ChangedDate] DESC");
    query
}

fn azure_query_wit_ids(
    config: &AzureDevOpsConfig,
    wiql: &str,
    limit: u32,
) -> Result<Vec<i64>, String> {
    let path = format!("/_apis/wit/wiql?$top={limit}&api-version={API_VERSION}");
    let response = azure_post_json(config, &path, json!({ "query": wiql }))?;
    let relations = response
        .value
        .get("workItems")
        .and_then(Value::as_array)
        .ok_or_else(|| "Azure DevOps did not return work items".to_string())?;
    Ok(relations
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_i64))
        .filter(|id| *id > 0)
        .take(limit as usize)
        .collect())
}

fn azure_fetch_wit_batch(
    config: &AzureDevOpsConfig,
    ids: &[i64],
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    let joined = ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fields = [
        "System.Id",
        "System.Title",
        "System.State",
        "System.ChangedDate",
        "System.CreatedDate",
        "System.AssignedTo",
        "System.Tags",
        "System.TeamProject",
    ]
    .join(",");
    let path = format!(
        "/_apis/wit/workitems?ids={joined}&fields={fields}&$expand=relations&api-version={API_VERSION}"
    );
    let response = azure_get(config, &path)?;
    let rows = response
        .value
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "Azure DevOps did not return work items".to_string())?;
    let mut items: Vec<AzureDevOpsWorkItem> = rows.iter().filter_map(parse_wit).collect();
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(items)
}

// ---- Parsing ----

fn parse_pr_list(
    value: &Value,
    repo: &str,
    base_url: &str,
) -> Result<Vec<AzureDevOpsWorkItem>, String> {
    let rows = value
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "Azure DevOps did not return pull requests".to_string())?;
    Ok(rows
        .iter()
        .filter_map(|row| parse_pr(row, repo, base_url))
        .collect())
}

fn parse_pr(row: &Value, repo: &str, base_url: &str) -> Option<AzureDevOpsWorkItem> {
    let number = row.get("pullRequestId").and_then(Value::as_i64)?;
    if number <= 0 {
        return None;
    }
    let status = string_field(row, "status").unwrap_or_default();
    Some(AzureDevOpsWorkItem {
        kind: "pr".into(),
        number,
        title: string_field(row, "title").unwrap_or_default(),
        // List responses often omit `_links.web` and even `remoteUrl`, so the
        // last resort builds the standard web URL from request-side data.
        url: pr_link(row)
            .or_else(|| pr_url_from_repository(row, number))
            .or_else(|| pr_url_from_parts(repo, base_url, number))
            .unwrap_or_default(),
        state: normalize_pr_state(&status),
        updated_at: string_field(row, "creationDate")
            .or_else(|| string_field(row, "closedDate"))
            .unwrap_or_default(),
        labels: parse_pr_labels(row),
        assignees: parse_pr_assignees(row),
        draft: row.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        repo: repo.into(),
        attention_reason: String::new(),
    })
}

fn parse_pr_details(value: &Value) -> Result<AzureDevOpsWorkItemDetails, String> {
    if !value.is_object() {
        return Err("Azure DevOps did not return that pull request".into());
    }
    let creator = value.get("createdBy");
    Ok(AzureDevOpsWorkItemDetails {
        body: string_field(value, "description").unwrap_or_default(),
        author: creator
            .and_then(|user| string_field(user, "displayName"))
            .unwrap_or_default(),
        author_avatar_url: creator
            .and_then(|user| string_field(user, "imageUrl"))
            .or_else(|| {
                creator.and_then(|user| {
                    user.get("_links")
                        .and_then(|links| links.get("avatar"))
                        .and_then(|avatar| string_field(avatar, "href"))
                })
            })
            .unwrap_or_default(),
        base_ref_name: short_ref(&string_field(value, "targetRefName").unwrap_or_default()),
        head_ref_name: short_ref(&string_field(value, "sourceRefName").unwrap_or_default()),
        review_decision: String::new(),
    })
}

fn parse_pr_thread(
    value: &Value,
    pr_url: &str,
    truncated: bool,
) -> Result<AzureDevOpsWorkItemThread, String> {
    let rows = value
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "Azure DevOps did not return comments".to_string())?;
    let mut comments: Vec<AzureDevOpsWorkItemComment> = Vec::new();
    for thread in rows {
        let thread_id = thread
            .get("id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string())
            .unwrap_or_default();
        let status = string_field(thread, "status").unwrap_or_default();
        let resolved = matches!(
            status.to_ascii_lowercase().as_str(),
            "fixed" | "closed" | "resolved"
        );
        let Some(thread_comments) = thread.get("comments").and_then(Value::as_array) else {
            continue;
        };
        for comment in thread_comments {
            if comment
                .get("isDeleted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let body = string_field(comment, "content").unwrap_or_default();
            if body.is_empty() {
                continue;
            }
            let author = comment.get("author");
            let comment_id = comment
                .get("id")
                .and_then(Value::as_i64)
                .map(|id| id.to_string())
                .unwrap_or_default();
            comments.push(AzureDevOpsWorkItemComment {
                id: if thread_id.is_empty() {
                    comment_id.clone()
                } else {
                    format!("{thread_id}-{comment_id}")
                },
                kind: "comment".into(),
                author: author
                    .and_then(|user| string_field(user, "displayName"))
                    .unwrap_or_default(),
                author_avatar_url: author
                    .and_then(|user| string_field(user, "imageUrl"))
                    .unwrap_or_default(),
                body,
                created_at: string_field(comment, "publishedDate")
                    .or_else(|| string_field(comment, "lastUpdatedDate"))
                    .unwrap_or_default(),
                url: pr_url.into(),
                state: String::new(),
                path: String::new(),
                line: None,
                resolved,
                thread_id: thread_id.clone(),
                replies: Vec::new(),
            });
        }
    }
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(AzureDevOpsWorkItemThread {
        comments,
        truncated,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    })
}

fn parse_wit(row: &Value) -> Option<AzureDevOpsWorkItem> {
    let number = row.get("id").and_then(Value::as_i64)?;
    if number <= 0 {
        return None;
    }
    let fields = row.get("fields")?;
    let project = string_field(fields, "System.TeamProject").unwrap_or_default();
    Some(AzureDevOpsWorkItem {
        kind: "issue".into(),
        number,
        title: string_field(fields, "System.Title").unwrap_or_default(),
        url: row
            .get("_links")
            .and_then(|links| links.get("html"))
            .and_then(|html| string_field(html, "href"))
            .unwrap_or_default(),
        state: normalize_wit_state(&string_field(fields, "System.State").unwrap_or_default()),
        updated_at: string_field(fields, "System.ChangedDate")
            .or_else(|| string_field(fields, "System.CreatedDate"))
            .unwrap_or_default(),
        labels: parse_wit_labels(fields),
        assignees: parse_wit_assignees(fields),
        draft: false,
        repo: project,
        attention_reason: String::new(),
    })
}

fn parse_wit_details(value: &Value) -> Result<AzureDevOpsWorkItemDetails, String> {
    if !value.is_object() {
        return Err("Azure DevOps did not return that work item".into());
    }
    let fields = value
        .get("fields")
        .ok_or_else(|| "Azure DevOps did not return that work item".to_string())?;
    let creator = fields.get("System.CreatedBy");
    Ok(AzureDevOpsWorkItemDetails {
        body: string_field(fields, "System.Description").unwrap_or_default(),
        author: creator
            .and_then(|user| string_field(user, "displayName"))
            .unwrap_or_default(),
        author_avatar_url: creator
            .and_then(|user| string_field(user, "imageUrl"))
            .unwrap_or_default(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
        review_decision: String::new(),
    })
}

fn parse_wit_thread(value: &Value, truncated: bool) -> Result<AzureDevOpsWorkItemThread, String> {
    let rows = value
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| "Azure DevOps did not return comments".to_string())?;
    let mut comments: Vec<AzureDevOpsWorkItemComment> = rows
        .iter()
        .filter_map(|row| {
            let id = row.get("id").and_then(Value::as_i64)?;
            let body = string_field(row, "text").unwrap_or_default();
            if body.is_empty() {
                return None;
            }
            let author = row.get("createdBy");
            Some(AzureDevOpsWorkItemComment {
                id: id.to_string(),
                kind: "comment".into(),
                author: author
                    .and_then(|user| string_field(user, "displayName"))
                    .unwrap_or_default(),
                author_avatar_url: author
                    .and_then(|user| string_field(user, "imageUrl"))
                    .unwrap_or_default(),
                body,
                created_at: string_field(row, "createdDate").unwrap_or_default(),
                url: string_field(row, "url").unwrap_or_default(),
                state: String::new(),
                path: String::new(),
                line: None,
                resolved: false,
                thread_id: String::new(),
                replies: Vec::new(),
            })
        })
        .collect();
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let total = value
        .get("totalCount")
        .and_then(Value::as_i64)
        .unwrap_or(comments.len() as i64);
    Ok(AzureDevOpsWorkItemThread {
        truncated: truncated || total > comments.len() as i64,
        comments,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    })
}

fn parse_iteration_changes(value: &Value, truncated: bool) -> AzureDevOpsMrDiff {
    let entries = value
        .get("changeEntries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut files = Vec::new();
    let mut patch = String::new();
    let mut truncated = truncated;
    for entry in &entries {
        let item = entry.get("item");
        let path = item
            .and_then(|item| string_field(item, "path"))
            .unwrap_or_default();
        let is_folder = item
            .and_then(|item| item.get("isFolder"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if path.is_empty() || is_folder || path == "/" {
            continue;
        }
        let clean = path.trim_start_matches('/').to_string();
        if clean.is_empty() {
            continue;
        }
        files.push(AzureDevOpsMrFile {
            path: clean.clone(),
            additions: 0,
            deletions: 0,
        });
        let block = format!("diff --git a/{clean} b/{clean}\n--- a/{clean}\n+++ b/{clean}\n");
        if patch.len() + block.len() > MAX_DIFF_BYTES {
            truncated = true;
            continue;
        }
        patch.push_str(&block);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    AzureDevOpsMrDiff {
        additions: 0,
        deletions: 0,
        files,
        patch,
        truncated,
    }
}

fn parse_pr_labels(row: &Value) -> Vec<AzureDevOpsLabel> {
    row.get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| {
                    let name = string_field(label, "name")?;
                    if name.is_empty() {
                        return None;
                    }
                    Some(AzureDevOpsLabel {
                        name,
                        color: String::new(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_pr_assignees(row: &Value) -> Vec<AzureDevOpsAssignee> {
    let mut people: Vec<AzureDevOpsAssignee> = row
        .get("reviewers")
        .and_then(Value::as_array)
        .map(|reviewers| {
            reviewers
                .iter()
                .filter_map(|reviewer| {
                    let login = string_field(reviewer, "displayName")?;
                    if login.is_empty() {
                        return None;
                    }
                    Some(AzureDevOpsAssignee {
                        login,
                        avatar_url: string_field(reviewer, "imageUrl").unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(creator) = row.get("createdBy") {
        if let Some(login) = string_field(creator, "displayName").filter(|name| !name.is_empty()) {
            if !people.iter().any(|person| person.login == login) {
                people.push(AzureDevOpsAssignee {
                    login,
                    avatar_url: string_field(creator, "imageUrl").unwrap_or_default(),
                });
            }
        }
    }
    people
}

fn parse_wit_labels(fields: &Value) -> Vec<AzureDevOpsLabel> {
    string_field(fields, "System.Tags")
        .map(|tags| {
            tags.split(';')
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(|tag| AzureDevOpsLabel {
                    name: tag.to_string(),
                    color: String::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_wit_assignees(fields: &Value) -> Vec<AzureDevOpsAssignee> {
    let assigned = fields.get("System.AssignedTo");
    assigned
        .and_then(|user| {
            let login = string_field(user, "displayName")?;
            if login.is_empty() {
                return None;
            }
            Some(vec![AzureDevOpsAssignee {
                login,
                avatar_url: string_field(user, "imageUrl").unwrap_or_default(),
            }])
        })
        .unwrap_or_default()
}

fn normalize_pr_state(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "active" => "open".into(),
        "completed" => "merged".into(),
        "abandoned" => "closed".into(),
        other => other.into(),
    }
}

fn normalize_wit_state(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "new" | "active" | "to do" | "doing" | "in progress" | "resolved" | "proposed"
        | "approved" | "committed" => "open".into(),
        "closed" | "done" | "removed" | "abandoned" | "cut" => "closed".into(),
        other => other.into(),
    }
}

fn pr_link(row: &Value) -> Option<String> {
    row.get("_links")
        .and_then(|links| links.get("web"))
        .and_then(|web| string_field(web, "href"))
        .filter(|href| !href.is_empty())
}

fn pr_url_from_repository(row: &Value, number: i64) -> Option<String> {
    let remote = row
        .get("repository")
        .and_then(|repo| string_field(repo, "remoteUrl"))
        .filter(|url| !url.is_empty())?;
    Some(format!(
        "{}/pullrequest/{number}",
        remote.trim_end_matches('/')
    ))
}

fn pr_url_from_parts(repo: &str, base_url: &str, number: i64) -> Option<String> {
    let (project, repo_name) = split_repo(repo).ok()?;
    let base = base_url.trim_end_matches('/').trim();
    if base.is_empty() {
        return None;
    }
    Some(format!(
        "{base}/{}/_git/{}/pullrequest/{number}",
        project.trim(),
        repo_name.trim()
    ))
}

fn pr_repo_parts(row: &Value) -> (Option<String>, Option<String>) {
    let repository = row.get("repository");
    let project = repository
        .and_then(|repo| repo.get("project"))
        .and_then(|project| string_field(project, "name"));
    let repo_name = repository.and_then(|repo| string_field(repo, "name"));
    (project, repo_name)
}

fn pr_web_url(config: &AzureDevOpsConfig, project: &str, repo: &str, number: i64) -> String {
    format!(
        "{}/{}/_git/{}/pullrequest/{}",
        config.url.trim_end_matches('/'),
        project.trim(),
        repo.trim(),
        number
    )
}

fn short_ref(value: &str) -> String {
    value
        .trim()
        .strip_prefix("refs/heads/")
        .unwrap_or(value.trim())
        .to_string()
}

fn latest_iteration_id(value: &Value) -> Option<i64> {
    value
        .get("value")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(Value::as_i64))
                .max()
        })
        .unwrap_or_else(|| value.get("id").and_then(Value::as_i64))
}

// ---- Validation ----

fn validate_kind(kind: &str) -> Result<(), String> {
    if kind == "issue" || kind == "pr" {
        Ok(())
    } else {
        Err("Unknown Azure DevOps task kind".into())
    }
}

fn validate_item(kind: &str, number: i64) -> Result<(), String> {
    validate_kind(kind)?;
    if number <= 0 {
        return Err("Invalid Azure DevOps item number".into());
    }
    Ok(())
}

fn split_repo(repo: &str) -> Result<(String, String), String> {
    let repo = validate_repo(repo)?;
    let (project, name) = repo
        .split_once('/')
        .ok_or_else(|| "Invalid Azure DevOps repository".to_string())?;
    Ok((project.to_string(), name.to_string()))
}

fn validate_repo(repo: &str) -> Result<String, String> {
    let repo = repo.trim();
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.trim().is_empty()
                || *part == "."
                || *part == ".."
                || part.contains(['?', '#', '\\'])
                || part.chars().any(char::is_control)
        })
    {
        return Err("Invalid Azure DevOps repository".into());
    }
    Ok(repo.to_string())
}

// ---- HTTP ----

struct AzureResponse {
    value: Value,
    truncated: bool,
}

fn basic_auth(token: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!(":{token}"));
    format!("Basic {encoded}")
}

fn azure_get(config: &AzureDevOpsConfig, path: &str) -> Result<AzureResponse, String> {
    let url = format!("{}{}", config.url.trim_end_matches('/'), path);
    let agent = azure_agent();
    read_azure_response(
        agent
            .get(&url)
            .set("Authorization", &basic_auth(&config.token))
            .set("Accept", "application/json")
            .set("User-Agent", USER_AGENT)
            .call(),
    )
}

fn azure_post_json(
    config: &AzureDevOpsConfig,
    path: &str,
    body: Value,
) -> Result<AzureResponse, String> {
    let url = format!("{}{}", config.url.trim_end_matches('/'), path);
    let payload = serde_json::to_string(&body).map_err(|error| error.to_string())?;
    let agent = azure_agent();
    read_azure_response(
        agent
            .post(&url)
            .set("Authorization", &basic_auth(&config.token))
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .set("User-Agent", USER_AGENT)
            .send_string(&payload),
    )
}

fn azure_current_user_id(config: &AzureDevOpsConfig) -> Result<String, String> {
    // connectionData is still under preview and rejects stable api-versions.
    let response = azure_get(
        config,
        &format!("/_apis/connectionData?api-version={CONNECTION_DATA_API_VERSION}"),
    )?;
    let id = response
        .value
        .get("authenticatedUser")
        .and_then(|user| string_field(user, "id"))
        .or_else(|| {
            response
                .value
                .get("authorizedUser")
                .and_then(|user| string_field(user, "id"))
        })
        .unwrap_or_default();
    if id.trim().is_empty() {
        return Err("Azure DevOps did not return the current user".into());
    }
    Ok(id)
}

fn azure_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .redirects(0)
        .build()
}

fn read_azure_response(
    result: Result<ureq::Response, ureq::Error>,
) -> Result<AzureResponse, String> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
            return Err("Azure DevOps personal access token is invalid or lacks permission".into());
        }
        Err(ureq::Error::Status(404, _)) => {
            return Err("Azure DevOps did not find that item".into());
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(azure_http_error(status, &body));
        }
        Err(_) => return Err("Could not reach Azure DevOps".into()),
    };
    let status = response.status();
    let truncated = response
        .header("x-ms-continuationtoken")
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let body = response
        .into_string()
        .map_err(|_| "Azure DevOps returned an unreadable response".to_string())?;
    if !(200..300).contains(&status) {
        return Err(azure_http_error(status, &body));
    }
    let value: Value = serde_json::from_str(&body)
        .map_err(|_| "Azure DevOps returned invalid JSON".to_string())?;
    Ok(AzureResponse { value, truncated })
}

fn azure_http_error(status: u16, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|value| {
        value
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    });
    message.unwrap_or_else(|| format!("Azure DevOps request failed ({status})"))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
}

// ---- URLs / remotes ----

fn normalize_azure_devops_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(
            "Enter your Azure DevOps organization URL (e.g. https://dev.azure.com/myorg)".into(),
        );
    }
    if raw.chars().any(char::is_whitespace) || raw.contains(['?', '#', '\\']) {
        return Err("Azure DevOps URL is invalid".into());
    }
    // Bare `myorg` becomes the hosted organization URL.
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else if raw.contains('.') || raw.contains('/') {
        format!("https://{raw}")
    } else {
        format!("https://dev.azure.com/{raw}")
    };
    if !with_scheme.starts_with("https://") && !with_scheme.starts_with("http://") {
        return Err("Azure DevOps URL must use HTTP or HTTPS".into());
    }
    let (_, rest) = with_scheme
        .split_once("://")
        .ok_or_else(|| "Azure DevOps URL is invalid".to_string())?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty()
        || !authority.contains('.')
        || authority.contains('@')
        || authority.contains(':')
    {
        return Err("Azure DevOps URL is invalid".into());
    }
    let mut segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    // Drop API/git suffixes pasted from the browser address bar.
    if let Some(index) = segments.iter().position(|segment| {
        segment.eq_ignore_ascii_case("_apis") || segment.eq_ignore_ascii_case("_git")
    }) {
        segments.truncate(index);
    }
    let authority_lower = authority.to_ascii_lowercase();
    if authority_lower == "dev.azure.com" || authority_lower == "ssh.dev.azure.com" {
        let org = segments.first().ok_or_else(|| {
            "Azure DevOps URL must include your organization (e.g. https://dev.azure.com/myorg)"
                .to_string()
        })?;
        return Ok(format!("https://dev.azure.com/{}", org.trim()));
    }
    if let Some(org) = authority_lower.strip_suffix(".visualstudio.com") {
        let org = org.trim();
        if org.is_empty() || org.contains('.') {
            return Err("Azure DevOps URL is invalid".into());
        }
        return Ok(format!("https://dev.azure.com/{org}"));
    }
    if segments.is_empty() {
        return Err("Azure DevOps URL must include your organization or collection".into());
    }
    // On-premises (e.g. https://host/tfs/collection): keep host plus the
    // collection path so remotes match the same base.
    let kept = segments
        .iter()
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join("/");
    let scheme = with_scheme
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .unwrap_or("https");
    Ok(format!("{}://{}/{}", scheme, authority, kept))
}

fn organization_from_url(url: &str) -> String {
    canonical_org_key(url).unwrap_or_default()
}

fn canonical_org_key(url: &str) -> Option<String> {
    let normalized = normalize_azure_devops_url(url).ok()?;
    Some(normalized.to_ascii_lowercase())
}

fn encode_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn azure_devops_repo_for(root: &Path, organization_url: &str) -> Result<String, String> {
    let mut cmd = Command::new("git");
    crate::hide_window_console(&mut cmd);
    let output = cmd
        .args(["config", "--get-regexp", r"^remote\..*\.url$"])
        .current_dir(root)
        .output()
        .map_err(|_| "Could not run git".to_string())?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err("Could not read git remotes".into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut matches = Vec::new();
    for line in stdout.lines() {
        let Some((name, remote)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if let Some(repo) = project_repo_from_remote(remote.trim(), organization_url) {
            matches.push((name == "remote.origin.url", repo));
        }
    }
    matches
        .iter()
        .find(|(origin, _)| *origin)
        .or_else(|| matches.first())
        .map(|(_, repo)| repo.clone())
        .ok_or_else(|| "No Azure DevOps remote matches the configured organization".to_string())
}

fn project_repo_from_remote(remote: &str, organization_url: &str) -> Option<String> {
    let expected = canonical_org_key(organization_url)?;
    let (org_key, project, repo) = parse_azure_remote(remote)?;
    if org_key.to_ascii_lowercase() != expected {
        return None;
    }
    if !valid_repo_parts(&project, &repo) {
        return None;
    }
    Some(format!("{project}/{repo}"))
}

fn parse_azure_remote(remote: &str) -> Option<(String, String, String)> {
    let remote = remote.trim();
    if remote.is_empty() {
        return None;
    }
    // SSH: git@ssh.dev.azure.com:v3/{org}/{project}/{repo} (no scheme).
    if !remote.contains("://") && remote.contains('@') {
        if let Some((_, path)) = remote.split_once(':') {
            let path = path.strip_prefix("v3/").unwrap_or(path);
            let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
            if parts.len() >= 3 && remote.contains("dev.azure.com") {
                let org = parts[0].to_string();
                let project = percent_decode(parts[1]);
                let repo = percent_decode(parts[2..].join("/").trim_end_matches(".git"));
                // Repository names never contain `/`; extra segments belong to
                // on-prem collection paths, which HTTPS remotes already cover.
                let repo = repo.split('/').next_back().unwrap_or("").to_string();
                return Some((
                    format!("https://dev.azure.com/{}", org.to_ascii_lowercase()),
                    project,
                    repo,
                ));
            }
        }
    }
    // HTTPS (credentials stripped first).
    let without_scheme = remote.split_once("://")?.1;
    let after_auth = without_scheme.rsplit('@').next().unwrap_or(without_scheme);
    let (authority, path) = after_auth.split_once('/')?;
    let authority_lower = authority.to_ascii_lowercase();
    let segments: Vec<String> = path
        .split('/')
        .filter(|part| !part.is_empty())
        .map(percent_decode)
        .collect();
    if authority_lower == "dev.azure.com" {
        // {org}/{project}/_git/{repo}
        if segments.len() >= 4 && segments[2].eq_ignore_ascii_case("_git") {
            let org_key = format!("https://dev.azure.com/{}", segments[0].to_ascii_lowercase());
            let repo = segments[3..]
                .join("/")
                .trim_end_matches(".git")
                .trim()
                .to_string();
            return Some((org_key, segments[1].clone(), repo));
        }
        return None;
    }
    if let Some(org) = authority_lower.strip_suffix(".visualstudio.com") {
        // {project}/_git/{repo}
        if segments.len() >= 3 && segments[1].eq_ignore_ascii_case("_git") {
            let org_key = format!("https://dev.azure.com/{}", org.to_ascii_lowercase());
            let repo = segments[2..]
                .join("/")
                .trim_end_matches(".git")
                .trim()
                .to_string();
            return Some((org_key, segments[0].clone(), repo));
        }
        return None;
    }
    // On-premises or custom host: {collection...}/{project}/_git/{repo}
    let git_index = segments
        .iter()
        .position(|segment| segment.eq_ignore_ascii_case("_git"))?;
    if git_index < 2 || git_index + 1 >= segments.len() {
        return None;
    }
    let project = segments[git_index - 1].clone();
    let repo = segments[git_index + 1..]
        .join("/")
        .trim_end_matches(".git")
        .trim()
        .to_string();
    let org_path = segments[..git_index - 1].join("/");
    Some((
        format!("https://{authority_lower}/{org_path}").to_ascii_lowercase(),
        project,
        repo,
    ))
}

fn percent_decode(value: &str) -> String {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                out.push((high * 16 + low) as char);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out.trim().trim_end_matches(".git").to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn valid_repo_parts(project: &str, repo: &str) -> bool {
    for part in [project, repo] {
        if part.trim().is_empty()
            || part == "."
            || part == ".."
            || part.contains(['?', '#', '\\'])
            || part.chars().any(char::is_control)
        {
            return false;
        }
    }
    !project.contains('/') && !repo.contains('/')
}

// ---- Config storage ----

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("azure-devops-config.json"))
}

fn read_config(app: &AppHandle) -> Result<Option<AzureDevOpsConfig>, String> {
    let path = config_path(app)?;
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut config: AzureDevOpsConfig = serde_json::from_str(&raw)
                .map_err(|_| "Azure DevOps settings are invalid".to_string())?;
            config.url = normalize_azure_devops_url(&config.url)?;
            config.token = config.token.trim().to_string();
            if config.token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(config))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn require_config(app: &AppHandle) -> Result<AzureDevOpsConfig, String> {
    read_config(app)?.ok_or_else(|| "Connect Azure DevOps in Settings".to_string())
}

fn write_config(app: &AppHandle, config: &AzureDevOpsConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let value = serde_json::to_string(config).map_err(|error| error.to_string())?;
    write_secret_file(&path, &value)
}

fn delete_config(app: &AppHandle) -> Result<(), String> {
    let path = config_path(app)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_secret_file(path: &Path, value: &str) -> Result<(), String> {
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
        file.write_all(value.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, value).map_err(|error| error.to_string())
    }
}

fn expand_home(input: &str) -> PathBuf {
    if input == "~" {
        return crate::dirs_home()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(input));
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return crate::dirs_home()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest);
    }
    PathBuf::from(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_organization_urls() {
        assert_eq!(
            normalize_azure_devops_url("myorg").unwrap(),
            "https://dev.azure.com/myorg"
        );
        assert_eq!(
            normalize_azure_devops_url("https://dev.azure.com/myorg/").unwrap(),
            "https://dev.azure.com/myorg"
        );
        assert_eq!(
            normalize_azure_devops_url("https://myorg.visualstudio.com").unwrap(),
            "https://dev.azure.com/myorg"
        );
        assert_eq!(
            normalize_azure_devops_url("https://dev.azure.com/myorg/_git/repo").unwrap(),
            "https://dev.azure.com/myorg"
        );
        assert!(normalize_azure_devops_url("").is_err());
        assert!(normalize_azure_devops_url("https://dev.azure.com").is_err());
        assert!(normalize_azure_devops_url("ftp://dev.azure.com/myorg").is_err());
    }

    #[test]
    fn reads_hosted_remotes() {
        let org = "https://dev.azure.com/acme";
        assert_eq!(
            project_repo_from_remote("https://dev.azure.com/acme/platform/_git/web.git", org)
                .as_deref(),
            Some("platform/web")
        );
        assert_eq!(
            project_repo_from_remote("https://user@dev.azure.com/acme/platform/_git/web", org)
                .as_deref(),
            Some("platform/web")
        );
        assert_eq!(
            project_repo_from_remote("https://acme.visualstudio.com/platform/_git/web.git", org)
                .as_deref(),
            Some("platform/web")
        );
        assert_eq!(
            project_repo_from_remote("git@ssh.dev.azure.com:v3/acme/platform/web", org).as_deref(),
            Some("platform/web")
        );
        assert!(project_repo_from_remote("git@github.com:acme/web.git", org).is_none());
        assert!(
            project_repo_from_remote("https://dev.azure.com/other/platform/_git/web", org)
                .is_none()
        );
    }

    #[test]
    fn parses_pull_request_rows() {
        let rows = json!({ "value": [{
            "pullRequestId": 12,
            "title": "Draft login",
            "status": "active",
            "isDraft": true,
            "creationDate": "2026-09-09T10:00:00Z",
            "createdBy": { "displayName": "Maya", "imageUrl": "https://example.com/maya.png" },
            "reviewers": [{ "displayName": "Ada", "imageUrl": "https://example.com/ada.png" }],
            "_links": { "web": { "href": "https://dev.azure.com/acme/p/_git/r/pullrequest/12" } }
        }]});
        let items = parse_pr_list(&rows, "platform/web", "https://dev.azure.com/acme").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "pr");
        assert_eq!(items[0].state, "open");
        assert!(items[0].draft);
        assert_eq!(items[0].assignees.len(), 2);
        assert!(items[0].attention_reason.is_empty());
    }

    #[test]
    fn falls_back_to_repository_url_without_links() {
        let rows = json!({ "value": [{
            "pullRequestId": 12,
            "title": "Login",
            "status": "active",
            "creationDate": "2026-09-09T10:00:00Z",
            "repository": {
                "name": "web",
                "remoteUrl": "https://dev.azure.com/acme/platform/_git/web"
            }
        }]});
        let items = parse_pr_list(&rows, "platform/web", "https://dev.azure.com/acme").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].url,
            "https://dev.azure.com/acme/platform/_git/web/pullrequest/12"
        );
    }

    #[test]
    fn builds_pr_url_from_request_parts_without_row_links() {
        let rows = json!({ "value": [{
            "pullRequestId": 7,
            "title": "Login",
            "status": "active",
            "creationDate": "2026-09-09T10:00:00Z"
        }]});
        let items = parse_pr_list(&rows, "platform/web", "https://dev.azure.com/acme").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].url,
            "https://dev.azure.com/acme/platform/_git/web/pullrequest/7"
        );
    }

    #[test]
    fn maps_pull_request_states() {
        assert_eq!(normalize_pr_state("active"), "open");
        assert_eq!(normalize_pr_state("completed"), "merged");
        assert_eq!(normalize_pr_state("abandoned"), "closed");
    }

    #[test]
    fn builds_work_item_queries() {
        let open = wit_wiql(Some("platform"), false, "open");
        assert!(open.contains("[System.TeamProject] = 'platform'"));
        assert!(open.contains("NOT IN"));
        assert!(!open.contains("@me"));
        let assigned = wit_wiql(None, true, "open");
        assert!(assigned.contains("[System.AssignedTo] = @me"));
        let all = wit_wiql(Some("platform"), false, "all");
        assert!(!all.contains("NOT IN"));
    }

    #[test]
    fn parses_work_item_rows() {
        let row = json!({
            "id": 193,
            "fields": {
                "System.Title": "Global inbox",
                "System.State": "Active",
                "System.ChangedDate": "2026-09-12T17:28:00Z",
                "System.Tags": "feature; bug",
                "System.AssignedTo": { "displayName": "Maya" },
                "System.TeamProject": "platform"
            },
            "_links": { "html": { "href": "https://dev.azure.com/acme/_workitems/edit/193" } }
        });
        let item = parse_wit(&row).unwrap();
        assert_eq!(item.kind, "issue");
        assert_eq!(item.number, 193);
        assert_eq!(item.state, "open");
        assert_eq!(item.repo, "platform");
        assert_eq!(item.labels.len(), 2);
        assert_eq!(item.assignees[0].login, "Maya");
    }

    #[test]
    fn parses_pull_request_threads() {
        let value = json!({ "value": [{
            "id": 3,
            "status": "fixed",
            "comments": [
                { "id": 1, "content": "", "author": { "displayName": "Ada" } },
                { "id": 2, "content": "Looks good", "publishedDate": "2026-09-09T10:00:00Z",
                  "author": { "displayName": "Ada" }, "isDeleted": false }
            ]
        }]});
        let thread = parse_pr_thread(&value, "https://example.com/pr/9", false).unwrap();
        assert_eq!(thread.comments.len(), 1);
        assert_eq!(thread.comments[0].author, "Ada");
        assert!(thread.comments[0].resolved);
        assert_eq!(thread.comments[0].id, "3-2");
    }

    #[test]
    fn builds_iteration_diff_headers() {
        let value = json!({ "changeEntries": [
            { "item": { "path": "/src/new.ts", "isFolder": false }, "changeType": "add" },
            { "item": { "path": "/", "isFolder": true }, "changeType": "edit" }
        ]});
        let diff = parse_iteration_changes(&value, false);
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "src/new.ts");
        assert!(diff.patch.contains("diff --git a/src/new.ts b/src/new.ts"));
    }

    #[test]
    fn validates_explicit_project_repos() {
        assert_eq!(validate_repo(" platform/web ").unwrap(), "platform/web");
        assert!(validate_repo("platform").is_err());
        assert!(validate_repo("platform/web/extra").is_err());
    }
}
