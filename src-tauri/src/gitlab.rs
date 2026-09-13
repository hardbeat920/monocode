use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

const DEFAULT_GITLAB_URL: &str = "https://gitlab.com";
const DEFAULT_LIMIT: u32 = 40;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
const USER_AGENT: &str = "MonoCode";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitlabStatus {
    pub connected: bool,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct GitlabConfig {
    url: String,
    token: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabLabel {
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabAssignee {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabWorkItem {
    pub kind: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub updated_at: String,
    pub labels: Vec<GitlabLabel>,
    pub assignees: Vec<GitlabAssignee>,
    pub draft: bool,
    pub repo: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabWorkItemDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
    pub review_decision: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabWorkItemComment {
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
    pub replies: Vec<GitlabWorkItemComment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabWorkItemThread {
    pub comments: Vec<GitlabWorkItemComment>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabMrFile {
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitlabMrDiff {
    pub additions: i64,
    pub deletions: i64,
    pub files: Vec<GitlabMrFile>,
    pub patch: String,
    pub truncated: bool,
}

#[tauri::command(async)]
pub fn gitlab_status(app: AppHandle) -> Result<GitlabStatus, String> {
    let config = read_config(&app)?;
    Ok(GitlabStatus {
        connected: config.is_some(),
        url: config
            .map(|config| config.url)
            .unwrap_or_else(|| DEFAULT_GITLAB_URL.into()),
    })
}

#[tauri::command]
pub async fn gitlab_set_config(
    app: AppHandle,
    url: String,
    token: String,
) -> Result<GitlabStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = normalize_gitlab_url(&url)?;
        let token = token.trim().to_string();
        if token.is_empty() {
            delete_config(&app)?;
            return Ok(GitlabStatus {
                connected: false,
                url,
            });
        }
        let config = GitlabConfig { url, token };
        let response = gitlab_get(&config, "/user")?;
        if response.value.get("id").and_then(Value::as_i64).is_none() {
            return Err("GitLab did not return the current user".into());
        }
        write_config(&app, &config)?;
        Ok(GitlabStatus {
            connected: true,
            url: config.url,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_repo(app: AppHandle, cwd: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        gitlab_repo_for(&expand_home(&cwd), &config.url)
    })
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
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = gitlab_repo_for(&expand_home(&cwd), &config.url)?;
        gitlab_list_work_items_for(
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
pub async fn gitlab_work_item_details(
    app: AppHandle,
    cwd: String,
    kind: String,
    number: i64,
) -> Result<GitlabWorkItemDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = gitlab_repo_for(&expand_home(&cwd), &config.url)?;
        gitlab_work_item_details_for(&config, &repo, &kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_work_item_thread(
    app: AppHandle,
    cwd: String,
    kind: String,
    number: i64,
) -> Result<GitlabWorkItemThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = gitlab_repo_for(&expand_home(&cwd), &config.url)?;
        gitlab_work_item_thread_for(&config, &repo, &kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_work_item_comment(
    app: AppHandle,
    cwd: String,
    kind: String,
    number: i64,
    body: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = gitlab_repo_for(&expand_home(&cwd), &config.url)?;
        gitlab_work_item_comment_for(&config, &repo, &kind, number, &body)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn gitlab_mr_diff(
    app: AppHandle,
    cwd: String,
    number: i64,
) -> Result<GitlabMrDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = gitlab_repo_for(&expand_home(&cwd), &config.url)?;
        gitlab_mr_diff_for(&config, &repo, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn gitlab_list_work_items_for(
    config: &GitlabConfig,
    repo: &str,
    kind: &str,
    assigned_to_me: bool,
    state: &str,
    limit: u32,
) -> Result<Vec<GitlabWorkItem>, String> {
    validate_kind(kind)?;
    let resource = resource_for_kind(kind);
    let state = if state.trim().eq_ignore_ascii_case("all") {
        "all"
    } else {
        "opened"
    };
    let limit = limit.clamp(1, 100);
    let assigned = if assigned_to_me {
        "&scope=assigned_to_me"
    } else {
        "&scope=all"
    };
    let path = format!(
        "/projects/{}/{resource}?state={state}&order_by=updated_at&sort=desc&per_page={limit}&with_labels_details=true{assigned}",
        encode_path_component(repo)
    );
    let response = gitlab_get(config, &path)?;
    parse_work_items(&response.value, kind, repo)
}

fn gitlab_work_item_details_for(
    config: &GitlabConfig,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<GitlabWorkItemDetails, String> {
    validate_item(kind, number)?;
    let path = item_path(repo, kind, number);
    let response = gitlab_get(config, &path)?;
    parse_work_item_details(&response.value, kind)
}

fn gitlab_work_item_thread_for(
    config: &GitlabConfig,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<GitlabWorkItemThread, String> {
    validate_item(kind, number)?;
    let path = format!(
        "{}/notes?order_by=created_at&sort=desc&per_page=100",
        item_path(repo, kind, number)
    );
    let response = gitlab_get(config, &path)?;
    parse_work_item_thread(
        &response.value,
        &config.url,
        repo,
        kind,
        number,
        response.has_next_page,
    )
}

fn gitlab_work_item_comment_for(
    config: &GitlabConfig,
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
    let path = format!("{}/notes", item_path(repo, kind, number));
    let response = gitlab_post_form(config, &path, &[("body", body)])?;
    let id = response
        .value
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| "GitLab did not return a comment".to_string())?;
    Ok(note_url(&config.url, repo, kind, number, id))
}

fn gitlab_mr_diff_for(
    config: &GitlabConfig,
    repo: &str,
    number: i64,
) -> Result<GitlabMrDiff, String> {
    validate_item("pr", number)?;
    let path = format!(
        "/projects/{}/merge_requests/{number}/diffs?per_page=100",
        encode_path_component(repo)
    );
    let response = gitlab_get(config, &path)?;
    parse_mr_diff(&response.value, response.has_next_page)
}

fn validate_kind(kind: &str) -> Result<(), String> {
    if kind == "issue" || kind == "pr" {
        Ok(())
    } else {
        Err("Unknown GitLab task kind".into())
    }
}

fn validate_item(kind: &str, number: i64) -> Result<(), String> {
    validate_kind(kind)?;
    if number <= 0 {
        return Err("Invalid GitLab item number".into());
    }
    Ok(())
}

fn resource_for_kind(kind: &str) -> &'static str {
    if kind == "pr" {
        "merge_requests"
    } else {
        "issues"
    }
}

fn item_path(repo: &str, kind: &str, number: i64) -> String {
    format!(
        "/projects/{}/{}/{number}",
        encode_path_component(repo),
        resource_for_kind(kind)
    )
}

fn parse_work_items(value: &Value, kind: &str, repo: &str) -> Result<Vec<GitlabWorkItem>, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "GitLab did not return work items".to_string())?;
    Ok(rows
        .iter()
        .filter_map(|row| parse_work_item(row, kind, repo))
        .collect())
}

fn parse_work_item(row: &Value, kind: &str, repo: &str) -> Option<GitlabWorkItem> {
    let number = row.get("iid").and_then(Value::as_i64)?;
    if number <= 0 {
        return None;
    }
    let title = string_field(row, "title").unwrap_or_default();
    let title_lower = title.to_ascii_lowercase();
    let draft = kind == "pr"
        && (row.get("draft").and_then(Value::as_bool).unwrap_or(false)
            || row
                .get("work_in_progress")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            || title_lower.starts_with("draft:")
            || title_lower.starts_with("wip:"));
    Some(GitlabWorkItem {
        kind: kind.into(),
        number,
        title,
        url: string_field(row, "web_url").unwrap_or_default(),
        state: normalize_state(&string_field(row, "state").unwrap_or_default()),
        updated_at: string_field(row, "updated_at").unwrap_or_default(),
        labels: parse_labels(row),
        assignees: parse_assignees(row),
        draft,
        repo: repo.into(),
    })
}

fn parse_work_item_details(value: &Value, kind: &str) -> Result<GitlabWorkItemDetails, String> {
    if !value.is_object() {
        return Err("GitLab did not return that item".into());
    }
    let author = value.get("author");
    Ok(GitlabWorkItemDetails {
        body: string_field(value, "description").unwrap_or_default(),
        author: author
            .and_then(|author| string_field(author, "username"))
            .or_else(|| author.and_then(|author| string_field(author, "name")))
            .unwrap_or_default(),
        author_avatar_url: author
            .and_then(|author| string_field(author, "avatar_url"))
            .unwrap_or_default(),
        base_ref_name: if kind == "pr" {
            string_field(value, "target_branch").unwrap_or_default()
        } else {
            String::new()
        },
        head_ref_name: if kind == "pr" {
            string_field(value, "source_branch").unwrap_or_default()
        } else {
            String::new()
        },
        review_decision: String::new(),
    })
}

fn parse_work_item_thread(
    value: &Value,
    base_url: &str,
    repo: &str,
    kind: &str,
    number: i64,
    has_next_page: bool,
) -> Result<GitlabWorkItemThread, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "GitLab did not return comments".to_string())?;
    let mut comments: Vec<GitlabWorkItemComment> = rows
        .iter()
        .filter(|row| !row.get("system").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|row| {
            let id = row.get("id").and_then(Value::as_i64)?;
            let author = row.get("author");
            Some(GitlabWorkItemComment {
                id: id.to_string(),
                kind: "comment".into(),
                author: author
                    .and_then(|author| string_field(author, "username"))
                    .or_else(|| author.and_then(|author| string_field(author, "name")))
                    .unwrap_or_default(),
                author_avatar_url: author
                    .and_then(|author| string_field(author, "avatar_url"))
                    .unwrap_or_default(),
                body: string_field(row, "body").unwrap_or_default(),
                created_at: string_field(row, "created_at").unwrap_or_default(),
                url: note_url(base_url, repo, kind, number, id),
                state: String::new(),
                path: String::new(),
                line: None,
                resolved: row
                    .get("resolved")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                thread_id: String::new(),
                replies: Vec::new(),
            })
        })
        .collect();
    comments.reverse();
    Ok(GitlabWorkItemThread {
        comments,
        truncated: has_next_page,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    })
}

fn parse_mr_diff(value: &Value, has_next_page: bool) -> Result<GitlabMrDiff, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "GitLab did not return merge request diffs".to_string())?;
    let mut files = Vec::new();
    let mut patch = String::new();
    let mut total_additions = 0;
    let mut total_deletions = 0;
    let mut truncated = has_next_page;

    for row in rows {
        let old_path = string_field(row, "old_path").unwrap_or_default();
        let new_path = string_field(row, "new_path").unwrap_or_else(|| old_path.clone());
        if new_path.is_empty() && old_path.is_empty() {
            continue;
        }
        let diff = string_field_preserve(row, "diff").unwrap_or_default();
        let (additions, deletions) = diff_counts(&diff);
        total_additions += additions;
        total_deletions += deletions;
        files.push(GitlabMrFile {
            path: if new_path.is_empty() {
                old_path.clone()
            } else {
                new_path.clone()
            },
            additions,
            deletions,
        });
        truncated |= row
            .get("too_large")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || row
                .get("collapsed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        if diff.is_empty() || patch.len() >= MAX_DIFF_BYTES {
            continue;
        }
        let block = gitlab_diff_block(row, &old_path, &new_path, &diff);
        if patch.len() + block.len() > MAX_DIFF_BYTES {
            truncated = true;
            continue;
        }
        patch.push_str(&block);
    }

    Ok(GitlabMrDiff {
        additions: total_additions,
        deletions: total_deletions,
        files,
        patch,
        truncated,
    })
}

fn gitlab_diff_block(row: &Value, old_path: &str, new_path: &str, diff: &str) -> String {
    let old = if old_path.is_empty() {
        new_path
    } else {
        old_path
    };
    let new = if new_path.is_empty() {
        old_path
    } else {
        new_path
    };
    let mut block = format!("diff --git a/{old} b/{new}\n");
    if row
        .get("new_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        block.push_str("new file mode 100644\n");
    }
    if row
        .get("deleted_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        block.push_str("deleted file mode 100644\n");
    }
    if row
        .get("renamed_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        block.push_str(&format!("rename from {old}\nrename to {new}\n"));
    }
    let old_header = if row
        .get("new_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "/dev/null".to_string()
    } else {
        format!("a/{old}")
    };
    let new_header = if row
        .get("deleted_file")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "/dev/null".to_string()
    } else {
        format!("b/{new}")
    };
    block.push_str(&format!("--- {old_header}\n+++ {new_header}\n"));
    block.push_str(diff);
    if !diff.ends_with('\n') {
        block.push('\n');
    }
    block
}

fn diff_counts(diff: &str) -> (i64, i64) {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    (additions, deletions)
}

fn parse_labels(row: &Value) -> Vec<GitlabLabel> {
    row.get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| {
                    if let Some(name) = label.as_str() {
                        return Some(GitlabLabel {
                            name: name.trim().to_string(),
                            color: String::new(),
                        });
                    }
                    let name = string_field(label, "name")?;
                    Some(GitlabLabel {
                        name,
                        color: string_field(label, "color")
                            .unwrap_or_default()
                            .trim_start_matches('#')
                            .to_string(),
                    })
                })
                .filter(|label| !label.name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn parse_assignees(row: &Value) -> Vec<GitlabAssignee> {
    let mut people: Vec<&Value> = row
        .get("assignees")
        .and_then(Value::as_array)
        .map(|people| people.iter().collect())
        .unwrap_or_default();
    if people.is_empty() {
        if let Some(assignee) = row.get("assignee").filter(|value| value.is_object()) {
            people.push(assignee);
        }
    }
    people
        .into_iter()
        .filter_map(|person| {
            let login =
                string_field(person, "username").or_else(|| string_field(person, "name"))?;
            if login.is_empty() {
                return None;
            }
            Some(GitlabAssignee {
                login,
                avatar_url: string_field(person, "avatar_url").unwrap_or_default(),
            })
        })
        .collect()
}

fn normalize_state(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "opened" | "reopened" => "open".into(),
        other => other.into(),
    }
}

fn note_url(base_url: &str, repo: &str, kind: &str, number: i64, id: i64) -> String {
    let item = if kind == "pr" {
        "merge_requests"
    } else {
        "issues"
    };
    format!(
        "{}/{repo}/-/{item}/{number}#note_{id}",
        base_url.trim_end_matches('/')
    )
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
}

fn string_field_preserve(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

struct GitlabResponse {
    value: Value,
    has_next_page: bool,
}

fn gitlab_get(config: &GitlabConfig, path: &str) -> Result<GitlabResponse, String> {
    let url = format!("{}/api/v4{}", config.url.trim_end_matches('/'), path);
    let agent = gitlab_agent();
    read_gitlab_response(
        agent
            .get(&url)
            .set("PRIVATE-TOKEN", &config.token)
            .set("Accept", "application/json")
            .set("User-Agent", USER_AGENT)
            .call(),
    )
}

fn gitlab_post_form(
    config: &GitlabConfig,
    path: &str,
    fields: &[(&str, &str)],
) -> Result<GitlabResponse, String> {
    let url = format!("{}/api/v4{}", config.url.trim_end_matches('/'), path);
    let agent = gitlab_agent();
    read_gitlab_response(
        agent
            .post(&url)
            .set("PRIVATE-TOKEN", &config.token)
            .set("Accept", "application/json")
            .set("User-Agent", USER_AGENT)
            .send_form(fields),
    )
}

fn gitlab_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .redirects(0)
        .build()
}

fn read_gitlab_response(
    result: Result<ureq::Response, ureq::Error>,
) -> Result<GitlabResponse, String> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            return Err("GitLab access token is invalid or lacks permission".into());
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(gitlab_http_error(status, &body));
        }
        Err(_) => return Err("Could not reach GitLab".into()),
    };
    let status = response.status();
    let has_next_page = response
        .header("X-Next-Page")
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let body = response
        .into_string()
        .map_err(|_| "GitLab returned an unreadable response".to_string())?;
    if !(200..300).contains(&status) {
        return Err(gitlab_http_error(status, &body));
    }
    let value =
        serde_json::from_str(&body).map_err(|_| "GitLab returned invalid JSON".to_string())?;
    Ok(GitlabResponse {
        value,
        has_next_page,
    })
}

fn gitlab_http_error(status: u16, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|value| {
        value
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| value.get("error").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    });
    message.unwrap_or_else(|| format!("GitLab request failed ({status})"))
}

fn normalize_gitlab_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let raw = if raw.is_empty() {
        DEFAULT_GITLAB_URL
    } else {
        raw
    };
    if raw.contains("://") && !raw.starts_with("https://") && !raw.starts_with("http://") {
        return Err("GitLab URL must use HTTP or HTTPS".into());
    }
    let value = if raw.starts_with("https://") || raw.starts_with("http://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let (_, rest) = value
        .split_once("://")
        .ok_or_else(|| "GitLab URL must use HTTP or HTTPS".to_string())?;
    let path = rest.split_once('/').map(|(_, path)| path).unwrap_or("");
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.contains('@')
        || rest.contains('?')
        || rest.contains('#')
        || rest.contains('\\')
        || rest.chars().any(char::is_whitespace)
        || path.split('/').any(|segment| {
            matches!(
                segment.to_ascii_lowercase().as_str(),
                "." | ".." | "%2e" | "%2e%2e" | "%2e." | ".%2e"
            )
        })
    {
        return Err("GitLab URL is invalid".into());
    }
    let normalized = value.trim_end_matches('/');
    let normalized = normalized.strip_suffix("/api/v4").unwrap_or(normalized);
    Ok(normalized.trim_end_matches('/').to_string())
}

fn encode_path_component(value: &str) -> String {
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

fn gitlab_repo_for(root: &Path, gitlab_url: &str) -> Result<String, String> {
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
        if let Some(repo) = project_from_remote(remote.trim(), gitlab_url) {
            matches.push((name == "remote.origin.url", repo));
        }
    }
    matches
        .iter()
        .find(|(origin, _)| *origin)
        .or_else(|| matches.first())
        .map(|(_, repo)| repo.clone())
        .ok_or_else(|| "No GitLab remote matches the configured host".to_string())
}

fn project_from_remote(remote: &str, gitlab_url: &str) -> Option<String> {
    let configured = configured_remote(gitlab_url)?;
    let (authority, mut path) = remote_authority_path(remote)?;
    if host_without_port(&authority) != host_without_port(&configured.authority) {
        return None;
    }
    path = path.trim_matches('/').to_string();
    let prefix = configured.path.trim_matches('/');
    if !prefix.is_empty() {
        path = path.strip_prefix(&format!("{prefix}/"))?.to_string();
    }
    if let Some(stripped) = path.strip_suffix(".git") {
        path = stripped.to_string();
    }
    if !valid_project_path(&path) {
        return None;
    }
    Some(path)
}

struct ConfiguredRemote {
    authority: String,
    path: String,
}

fn configured_remote(url: &str) -> Option<ConfiguredRemote> {
    let normalized = normalize_gitlab_url(url).ok()?;
    let (_, rest) = normalized.split_once("://")?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    Some(ConfiguredRemote {
        authority: authority.to_ascii_lowercase(),
        path: path.to_string(),
    })
}

fn remote_authority_path(remote: &str) -> Option<(String, String)> {
    let remote = remote.trim();
    if let Some((_, rest)) = remote.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let authority = authority.rsplit('@').next()?.to_ascii_lowercase();
        return Some((authority, path.to_string()));
    }
    let (user_host, path) = remote.split_once(':')?;
    let authority = user_host.rsplit('@').next()?.to_ascii_lowercase();
    Some((authority, path.to_string()))
}

fn host_without_port(authority: &str) -> String {
    let authority = authority.trim().to_ascii_lowercase();
    if authority.starts_with('[') {
        return authority
            .split(']')
            .next()
            .map(|host| format!("{host}]"))
            .unwrap_or(authority);
    }
    authority
        .split(':')
        .next()
        .unwrap_or(&authority)
        .to_string()
}

fn valid_project_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && !part.chars().any(char::is_whitespace)
                && !part.contains(['?', '#', '\\'])
        })
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("gitlab-config.json"))
}

fn read_config(app: &AppHandle) -> Result<Option<GitlabConfig>, String> {
    let path = config_path(app)?;
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut config: GitlabConfig = serde_json::from_str(&raw)
                .map_err(|_| "GitLab settings are invalid".to_string())?;
            config.url = normalize_gitlab_url(&config.url)?;
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

fn require_config(app: &AppHandle) -> Result<GitlabConfig, String> {
    read_config(app)?.ok_or_else(|| "Connect GitLab in Settings".to_string())
}

fn write_config(app: &AppHandle, config: &GitlabConfig) -> Result<(), String> {
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
    fn normalizes_host_and_api_suffix() {
        assert_eq!(normalize_gitlab_url("").unwrap(), "https://gitlab.com");
        assert_eq!(
            normalize_gitlab_url("gitlab.example.com/").unwrap(),
            "https://gitlab.example.com"
        );
        assert_eq!(
            normalize_gitlab_url("https://gitlab.example.com/api/v4").unwrap(),
            "https://gitlab.example.com"
        );
        assert!(normalize_gitlab_url("ftp://gitlab.example.com").is_err());
        assert!(normalize_gitlab_url("https://user@host").is_err());
        assert!(normalize_gitlab_url("https://host/../admin").is_err());
    }

    #[test]
    fn reads_https_and_ssh_project_remotes() {
        assert_eq!(
            project_from_remote(
                "https://gitlab.example.com/acme/platform/web.git",
                "https://gitlab.example.com"
            )
            .as_deref(),
            Some("acme/platform/web")
        );
        assert_eq!(
            project_from_remote(
                "git@gitlab.example.com:acme/web.git",
                "https://gitlab.example.com"
            )
            .as_deref(),
            Some("acme/web")
        );
        assert_eq!(
            project_from_remote(
                "ssh://git@gitlab.example.com/acme/web.git",
                "https://gitlab.example.com"
            )
            .as_deref(),
            Some("acme/web")
        );
        assert!(
            project_from_remote("git@github.com:acme/web.git", "https://gitlab.example.com")
                .is_none()
        );
    }

    #[test]
    fn reads_relative_url_root_remotes() {
        assert_eq!(
            project_from_remote(
                "https://code.example.com/gitlab/acme/web.git",
                "https://code.example.com/gitlab"
            )
            .as_deref(),
            Some("acme/web")
        );
        assert!(project_from_remote(
            "https://code.example.com/gitlab-old/acme/web.git",
            "https://code.example.com/gitlab"
        )
        .is_none());
    }

    #[test]
    fn encodes_project_path_for_api() {
        assert_eq!(
            encode_path_component("acme/platform web"),
            "acme%2Fplatform%20web"
        );
    }

    #[test]
    fn parses_issue_and_merge_request_fields() {
        let rows = json!([{
            "iid": 9,
            "title": "Draft: Improve login",
            "web_url": "https://gitlab.example.com/acme/web/-/merge_requests/9",
            "state": "opened",
            "updated_at": "2026-09-09T10:00:00Z",
            "labels": [{ "name": "bug", "color": "#ff0000" }],
            "assignees": [{ "username": "maya", "avatar_url": "https://gitlab.example.com/uploads/maya.png" }]
        }]);
        let items = parse_work_items(&rows, "pr", "acme/web").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "pr");
        assert_eq!(items[0].state, "open");
        assert!(items[0].draft);
        assert_eq!(items[0].labels[0].color, "ff0000");
        assert_eq!(items[0].assignees[0].login, "maya");
    }

    #[test]
    fn parses_details_and_comments() {
        let details = parse_work_item_details(
            &json!({
                "description": "Body",
                "author": { "username": "maya", "avatar_url": "https://example.com/maya.png" },
                "target_branch": "main",
                "source_branch": "feature"
            }),
            "pr",
        )
        .unwrap();
        assert_eq!(details.author, "maya");
        assert_eq!(details.base_ref_name, "main");
        assert_eq!(details.head_ref_name, "feature");

        let thread = parse_work_item_thread(
            &json!([
                { "id": 1, "body": "Started", "system": true },
                {
                    "id": 2,
                    "body": "Looks good",
                    "created_at": "2026-09-09T10:00:00Z",
                    "author": { "username": "ada" },
                    "system": false
                }
            ]),
            "https://gitlab.example.com",
            "acme/web",
            "pr",
            9,
            true,
        )
        .unwrap();
        assert!(thread.truncated);
        assert_eq!(thread.comments.len(), 1);
        assert_eq!(thread.comments[0].author, "ada");
        assert_eq!(
            thread.comments[0].url,
            "https://gitlab.example.com/acme/web/-/merge_requests/9#note_2"
        );
    }

    #[test]
    fn builds_merge_request_diff() {
        let diff = parse_mr_diff(
            &json!([{
                "old_path": "src/old.ts",
                "new_path": "src/new.ts",
                "renamed_file": true,
                "diff": "@@ -1 +1 @@\n-old\n+new\n"
            }]),
            false,
        )
        .unwrap();
        assert_eq!(diff.additions, 1);
        assert_eq!(diff.deletions, 1);
        assert_eq!(diff.files[0].path, "src/new.ts");
        assert!(diff.patch.contains("rename from src/old.ts"));
        assert!(diff.patch.contains("@@ -1 +1 @@"));
    }
}
