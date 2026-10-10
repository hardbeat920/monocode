use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const API_BASE: &str = "https://api.bitbucket.org/2.0";
const BITBUCKET_HOST: &str = "bitbucket.org";
const DEFAULT_LIMIT: u32 = 40;
/// Largest `pagelen` Bitbucket accepts on the pull request list; larger
/// result limits are fetched across pages.
const MAX_PR_PAGE: u32 = 50;
/// Cap on rows gathered across pages for statuses, steps and pipeline lookups.
const MAX_LISTED_ROWS: usize = 500;
/// Bounds pagination even when pages are empty and `next` loops back.
const MAX_PAGES: usize = 50;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
const USER_AGENT: &str = "MonoCode";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketStatus {
    pub connected: bool,
    pub email: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct BitbucketConfig {
    email: String,
    token: String,
    #[serde(default)]
    account_id: String,
    /// Overrides the API origin in tests; never persisted.
    #[serde(skip)]
    api_base: String,
}

impl BitbucketConfig {
    fn api_base(&self) -> &str {
        if self.api_base.is_empty() {
            API_BASE
        } else {
            self.api_base.trim_end_matches('/')
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketLabel {
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketAssignee {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketWorkItem {
    pub kind: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub updated_at: String,
    pub labels: Vec<BitbucketLabel>,
    pub assignees: Vec<BitbucketAssignee>,
    pub draft: bool,
    pub repo: String,
    pub attention_reason: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketWorkItemDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
    pub review_decision: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketWorkItemComment {
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
    pub replies: Vec<BitbucketWorkItemComment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketWorkItemThread {
    pub comments: Vec<BitbucketWorkItemComment>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketPrFile {
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketPrDiff {
    pub additions: i64,
    pub deletions: i64,
    pub files: Vec<BitbucketPrFile>,
    pub patch: String,
    pub truncated: bool,
}

/// Same shape as the GitHub checks payload so the Checks tab is shared.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketPrCheck {
    pub name: String,
    pub workflow: String,
    pub state: String,
    pub url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketPrChecks {
    pub head_oid: String,
    pub checks: Vec<BitbucketPrCheck>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketBuildStep {
    pub name: String,
    pub state: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Same shape as the GitHub check details so the expanded row is shared.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BitbucketBuildDetails {
    pub steps: Vec<BitbucketBuildStep>,
    pub annotations: Vec<Value>,
    pub notice: Option<String>,
}

#[tauri::command(async)]
pub fn bitbucket_status(app: AppHandle) -> Result<BitbucketStatus, String> {
    let config = read_config(&app)?;
    Ok(BitbucketStatus {
        connected: config.is_some(),
        email: config.map(|config| config.email).unwrap_or_default(),
    })
}

#[tauri::command]
pub async fn bitbucket_set_config(
    app: AppHandle,
    email: String,
    token: String,
) -> Result<BitbucketStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let email = email.trim().to_string();
        let token = token.trim().to_string();
        if token.is_empty() {
            delete_config(&app)?;
            return Ok(BitbucketStatus {
                connected: false,
                email,
            });
        }
        if email.is_empty() || email.contains(char::is_whitespace) {
            return Err("Enter your Atlassian account email".into());
        }
        let mut config = BitbucketConfig {
            email,
            token,
            account_id: String::new(),
            api_base: String::new(),
        };
        let response = bitbucket_get(&config, "/user")?;
        config.account_id = string_field(&response.value, "account_id")
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "Bitbucket did not return the current user".to_string())?;
        write_config(&app, &config)?;
        Ok(BitbucketStatus {
            connected: true,
            email: config.email,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_repo(app: AppHandle, cwd: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        require_config(&app)?;
        bitbucket_repo_for(&expand_home(&cwd))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_list_work_items(
    app: AppHandle,
    cwd: String,
    kind: String,
    assigned_to_me: bool,
    state: String,
    limit: Option<u32>,
) -> Result<Vec<BitbucketWorkItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = bitbucket_repo_for(&expand_home(&cwd))?;
        bitbucket_list_work_items_for(
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
pub async fn bitbucket_work_item_details(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
) -> Result<BitbucketWorkItemDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        bitbucket_work_item_details_for(&config, &repo, &kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_work_item_thread(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
) -> Result<BitbucketWorkItemThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        bitbucket_work_item_thread_for(&config, &repo, &kind, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_work_item_comment(
    app: AppHandle,
    repo: String,
    kind: String,
    number: i64,
    body: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        bitbucket_work_item_comment_for(&config, &repo, &kind, number, &body)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_pr_diff(
    app: AppHandle,
    repo: String,
    number: i64,
) -> Result<BitbucketPrDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        bitbucket_pr_diff_for(&config, &repo, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_pr_checks(
    app: AppHandle,
    repo: String,
    number: i64,
) -> Result<BitbucketPrChecks, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        bitbucket_pr_checks_for(&config, &repo, number)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bitbucket_build_details(
    app: AppHandle,
    repo: String,
    build: String,
) -> Result<BitbucketBuildDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let repo = validate_repo(&repo)?;
        bitbucket_build_details_for(&config, &repo, &build)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn bitbucket_list_work_items_for(
    config: &BitbucketConfig,
    repo: &str,
    kind: &str,
    assigned_to_me: bool,
    state: &str,
    limit: u32,
) -> Result<Vec<BitbucketWorkItem>, String> {
    validate_kind(kind)?;
    let all = state.trim().eq_ignore_ascii_case("all");
    let limit = limit.max(1);
    let page_size = limit.min(MAX_PR_PAGE);
    let account_id = config.account_id.as_str();
    if assigned_to_me && account_id.is_empty() {
        return Err("Reconnect Bitbucket in Settings".into());
    }
    let repo_path = repo_path(repo);
    let states = if all {
        "&state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED"
    } else {
        "&state=OPEN"
    };
    let mut path = format!(
        "/repositories/{repo_path}/pullrequests?sort=-updated_on&pagelen={page_size}{states}"
    );
    if assigned_to_me {
        let query = format!(
            "(reviewers.account_id=\"{account_id}\" OR author.account_id=\"{account_id}\")"
        );
        path.push_str(&format!("&q={}", encode_component(&query)));
    }
    let (rows, _) = bitbucket_get_values(config, &path, limit as usize)
        .map_err(|error| format!("Bitbucket pull requests for {repo}: {error}"))?;
    parse_pr_list(
        &json!({ "values": rows }),
        repo,
        assigned_to_me.then_some(account_id),
    )
}

fn bitbucket_work_item_details_for(
    config: &BitbucketConfig,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<BitbucketWorkItemDetails, String> {
    validate_item(kind, number)?;
    let response = bitbucket_get(config, &item_path(repo, number))?;
    parse_work_item_details(&response.value)
}

fn bitbucket_work_item_thread_for(
    config: &BitbucketConfig,
    repo: &str,
    kind: &str,
    number: i64,
) -> Result<BitbucketWorkItemThread, String> {
    validate_item(kind, number)?;
    let path = format!(
        "{}/comments?sort=-created_on&pagelen=100",
        item_path(repo, number)
    );
    let response = bitbucket_get(config, &path)?;
    let mut thread = parse_work_item_thread(&response.value, response.has_next_page)?;
    let details = bitbucket_get(config, &item_path(repo, number))
        .and_then(|response| parse_work_item_details(&response.value))?;
    thread.review_decision = details.review_decision;
    thread.base_ref_name = details.base_ref_name;
    thread.head_ref_name = details.head_ref_name;
    Ok(thread)
}

fn bitbucket_work_item_comment_for(
    config: &BitbucketConfig,
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
    let path = format!("{}/comments", item_path(repo, number));
    let response = bitbucket_post_json(config, &path, json!({ "content": { "raw": body } }))?;
    Ok(response
        .value
        .pointer("/links/html/href")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| web_url(repo, number)))
}

fn bitbucket_pr_diff_for(
    config: &BitbucketConfig,
    repo: &str,
    number: i64,
) -> Result<BitbucketPrDiff, String> {
    validate_item("pr", number)?;
    let base = item_path(repo, number);
    let stat = bitbucket_get(config, &format!("{base}/diffstat?pagelen=100"))?;
    let (patch, patch_truncated) = bitbucket_get_text(config, &format!("{base}/diff"))?;
    parse_pr_diff(&stat.value, &patch, stat.has_next_page || patch_truncated)
}

/// Build and CI results Bitbucket attaches to the pull request's head commit:
/// Pipelines and third-party builds such as SonarCloud.
fn bitbucket_pr_checks_for(
    config: &BitbucketConfig,
    repo: &str,
    number: i64,
) -> Result<BitbucketPrChecks, String> {
    validate_item("pr", number)?;
    let base = item_path(repo, number);
    let pr = bitbucket_get(config, &base)?;
    let head_oid = pr
        .value
        .pointer("/source/commit/hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if head_oid.is_empty() {
        return Err("Bitbucket did not return the pull request head commit".into());
    }
    // Read the statuses of the captured commit rather than the PR, so a push
    // between the two requests cannot pair newer results with this head.
    let (rows, truncated) = bitbucket_get_values(
        config,
        &format!(
            "/repositories/{}/commit/{}/statuses?pagelen=100&sort=-updated_on",
            repo_path(repo),
            encode_component(&head_oid)
        ),
        MAX_LISTED_ROWS,
    )?;
    if truncated {
        return Err("Bitbucket returned too many build statuses to summarize".into());
    }
    parse_pr_checks(&json!({ "values": rows }), head_oid)
}

const PIPELINES_SCOPE: &str = "read:pipeline:bitbucket";

fn missing_pipelines_scope_notice() -> String {
    format!(
        "Build steps need the {PIPELINES_SCOPE} scope. Atlassian API tokens cannot be changed after they are created, so create a new token that includes it, then reconnect in Settings."
    )
}

/// Both 403 shapes from `read_response` mean the token cannot do this.
fn is_scope_error(message: &str) -> bool {
    message.starts_with("Bitbucket API token is missing a required scope")
        || message.starts_with("Bitbucket denied access")
}

/// Steps of one Pipelines build, found from the build number in its status
/// URL. A token without the Pipelines scope is not an error: the row still
/// expands and says what to add.
fn bitbucket_build_details_for(
    config: &BitbucketConfig,
    repo: &str,
    build: &str,
) -> Result<BitbucketBuildDetails, String> {
    let number: i64 = build
        .parse()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| "Invalid Bitbucket build number".to_string())?;
    let notice = |text: String| BitbucketBuildDetails {
        steps: Vec::new(),
        annotations: Vec::new(),
        notice: Some(text),
    };
    let uuid = match find_pipeline_uuid(config, repo, number) {
        Ok((Some(uuid), _)) => uuid,
        Ok((None, true)) => {
            return Ok(notice(
                "Couldn't find this build among the most recent pipelines.".into(),
            ))
        }
        Ok((None, false)) => return Ok(notice("Bitbucket has no steps for this build.".into())),
        Err(error) if is_scope_error(&error) => {
            return Ok(notice(missing_pipelines_scope_notice()))
        }
        Err(error) => return Err(error),
    };
    let path = format!(
        "/repositories/{}/pipelines/{}/steps/?pagelen=100",
        repo_path(repo),
        encode_component(&uuid)
    );
    match bitbucket_get_values(config, &path, MAX_LISTED_ROWS) {
        Ok((rows, truncated)) => {
            let mut details = parse_build_steps(&json!({ "values": rows }))?;
            if truncated {
                details.notice = Some("Showing the first steps of a very large build.".into());
            }
            Ok(details)
        }
        Err(error) if is_scope_error(&error) => Ok(notice(missing_pipelines_scope_notice())),
        Err(error) => Err(error),
    }
}

/// Bitbucket identifies pipelines by UUID, but status URLs only carry the
/// build number. Try the number directly, then look through recent pipelines.
/// The flag reports that the search was cut short before finding a match.
fn find_pipeline_uuid(
    config: &BitbucketConfig,
    repo: &str,
    number: i64,
) -> Result<(Option<String>, bool), String> {
    let base = format!("/repositories/{}/pipelines", repo_path(repo));
    match bitbucket_get(config, &format!("{base}/{number}")) {
        Ok(response) => {
            if let Some(uuid) = pipeline_uuid_for(&response.value, number) {
                return Ok((Some(uuid), false));
            }
        }
        Err(error) if is_scope_error(&error) => return Err(error),
        // Not addressable by number here; fall through to the list.
        Err(_) => {}
    }
    let (rows, truncated) = bitbucket_get_values(
        config,
        &format!("{base}/?sort=-created_on&pagelen=100"),
        MAX_LISTED_ROWS,
    )?;
    let uuid = rows.iter().find_map(|row| pipeline_uuid_for(row, number));
    let truncated = uuid.is_none() && truncated;
    Ok((uuid, truncated))
}

fn pipeline_uuid_for(row: &Value, number: i64) -> Option<String> {
    if row.get("build_number").and_then(Value::as_i64) != Some(number) {
        return None;
    }
    string_field(row, "uuid").filter(|uuid| !uuid.is_empty())
}

fn parse_build_steps(value: &Value) -> Result<BitbucketBuildDetails, String> {
    let rows = value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "Bitbucket did not return build steps".to_string())?;
    let steps = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let name = string_field(row, "name")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("Step {}", index + 1));
            let phase = row
                .pointer("/state/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let result = row
                .pointer("/state/result/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let state = match (phase, result) {
                (_, "SUCCESSFUL") => "pass",
                (_, "FAILED" | "ERROR") => "fail",
                (_, "STOPPED" | "EXPIRED") => "cancel",
                (_, "NOT_RUN") => "skipping",
                ("PENDING" | "IN_PROGRESS" | "PAUSED", _) => "pending",
                _ => "unknown",
            };
            BitbucketBuildStep {
                name,
                state: state.into(),
                started_at: string_field(row, "started_on"),
                completed_at: string_field(row, "completed_on"),
            }
        })
        .collect();
    Ok(BitbucketBuildDetails {
        steps,
        annotations: Vec::new(),
        notice: None,
    })
}

fn parse_pr_checks(value: &Value, head_oid: String) -> Result<BitbucketPrChecks, String> {
    let rows = value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "Bitbucket did not return build statuses".to_string())?;
    let mut seen = std::collections::HashSet::new();
    let mut checks = Vec::new();
    // Newest first, so the first row per key is the current result.
    for row in rows {
        let name = string_field(row, "name")
            .filter(|name| !name.is_empty())
            .or_else(|| string_field(row, "key"))
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let key = string_field(row, "key").unwrap_or_else(|| name.clone());
        if !seen.insert(key) {
            continue;
        }
        let state = match string_field(row, "state").unwrap_or_default().as_str() {
            "SUCCESSFUL" => "pass",
            "FAILED" => "fail",
            "INPROGRESS" => "pending",
            "STOPPED" => "cancel",
            _ => "unknown",
        };
        checks.push(BitbucketPrCheck {
            name,
            workflow: String::new(),
            state: state.into(),
            url: string_field(row, "url").filter(|url| !url.is_empty()),
            started_at: string_field(row, "created_on"),
            completed_at: if state == "pending" {
                None
            } else {
                string_field(row, "updated_on")
            },
        });
    }
    Ok(BitbucketPrChecks { head_oid, checks })
}

fn validate_kind(kind: &str) -> Result<(), String> {
    if kind == "pr" {
        Ok(())
    } else {
        Err("Unsupported Bitbucket task kind".into())
    }
}

fn validate_item(kind: &str, number: i64) -> Result<(), String> {
    validate_kind(kind)?;
    if number <= 0 {
        return Err("Invalid Bitbucket item number".into());
    }
    Ok(())
}

fn repo_path(repo: &str) -> String {
    repo.split('/')
        .map(encode_component)
        .collect::<Vec<_>>()
        .join("/")
}

fn item_path(repo: &str, number: i64) -> String {
    format!("/repositories/{}/pullrequests/{number}", repo_path(repo))
}

fn web_url(repo: &str, number: i64) -> String {
    format!("https://{BITBUCKET_HOST}/{repo}/pull-requests/{number}")
}

fn validate_repo(repo: &str) -> Result<String, String> {
    let repo = repo.trim();
    if valid_repo_path(repo) {
        Ok(repo.to_string())
    } else {
        Err("Invalid Bitbucket repository".into())
    }
}

fn valid_repo_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

fn parse_pr_list(
    value: &Value,
    repo: &str,
    attention_for: Option<&str>,
) -> Result<Vec<BitbucketWorkItem>, String> {
    let rows = value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "Bitbucket did not return pull requests".to_string())?;
    Ok(rows
        .iter()
        .filter_map(|row| parse_pr(row, repo, attention_for))
        .collect())
}

fn parse_pr(row: &Value, repo: &str, attention_for: Option<&str>) -> Option<BitbucketWorkItem> {
    let number = row.get("id").and_then(Value::as_i64)?;
    if number <= 0 {
        return None;
    }
    let reviewers = people(row.get("reviewers"));
    let attention_reason = match attention_for {
        Some(account_id)
            if row
                .get("reviewers")
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter()
                        .any(|r| string_field(r, "account_id").as_deref() == Some(account_id))
                }) =>
        {
            "review_requested"
        }
        Some(account_id)
            if row
                .get("author")
                .and_then(|author| string_field(author, "account_id"))
                .as_deref()
                == Some(account_id) =>
        {
            "authored"
        }
        _ => "",
    };
    Some(BitbucketWorkItem {
        kind: "pr".into(),
        number,
        title: string_field(row, "title").unwrap_or_default(),
        url: row
            .pointer("/links/html/href")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| web_url(repo, number)),
        state: normalize_state(&string_field(row, "state").unwrap_or_default()),
        updated_at: string_field(row, "updated_on").unwrap_or_default(),
        labels: Vec::new(),
        assignees: reviewers,
        draft: row.get("draft").and_then(Value::as_bool).unwrap_or(false),
        repo: repo.into(),
        attention_reason: attention_reason.into(),
    })
}

fn parse_work_item_details(value: &Value) -> Result<BitbucketWorkItemDetails, String> {
    if !value.is_object() {
        return Err("Bitbucket did not return that item".into());
    }
    let author = person(value.get("author"));
    let branch = |side: &str| {
        value
            .pointer(&format!("/{side}/branch/name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(BitbucketWorkItemDetails {
        body: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        author: author.as_ref().map(|a| a.login.clone()).unwrap_or_default(),
        author_avatar_url: author.map(|a| a.avatar_url).unwrap_or_default(),
        base_ref_name: branch("destination"),
        head_ref_name: branch("source"),
        review_decision: review_decision(value),
    })
}

fn review_decision(pr: &Value) -> String {
    let participants = pr
        .get("participants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let reviewers = || {
        participants
            .iter()
            .filter(|p| string_field(p, "role").as_deref() == Some("REVIEWER"))
    };
    if reviewers().any(|p| string_field(p, "state").as_deref() == Some("changes_requested")) {
        "CHANGES_REQUESTED".into()
    } else if reviewers().any(|p| {
        p.get("approved").and_then(Value::as_bool).unwrap_or(false)
            || string_field(p, "state").as_deref() == Some("approved")
    }) {
        "APPROVED".into()
    } else {
        String::new()
    }
}

fn parse_work_item_thread(
    value: &Value,
    has_next_page: bool,
) -> Result<BitbucketWorkItemThread, String> {
    let rows = value
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "Bitbucket did not return comments".to_string())?;
    // Replies point at their parent, possibly through several levels; fold
    // them under the top-level comment so the UI shows one flat reply list.
    let parent_of = |row: &Value| row.pointer("/parent/id").and_then(Value::as_i64);
    let by_id: std::collections::HashMap<i64, &Value> = rows
        .iter()
        .filter_map(|row| Some((row.get("id").and_then(Value::as_i64)?, row)))
        .collect();
    let root_of = |row: &Value| {
        let mut current = row;
        for _ in 0..32 {
            match parent_of(current).and_then(|id| by_id.get(&id)) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        current.get("id").and_then(Value::as_i64)
    };

    let mut roots: Vec<BitbucketWorkItemComment> = Vec::new();
    let mut replies: Vec<(i64, BitbucketWorkItemComment)> = Vec::new();
    // The API returns newest first; walk oldest first.
    for row in rows.iter().rev() {
        if row.get("deleted").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(id) = row.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let body = row
            .pointer("/content/raw")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if body.is_empty() {
            continue;
        }
        let author = person(row.get("user"));
        let root = root_of(row).unwrap_or(id);
        let inline = row.get("inline").filter(|value| value.is_object());
        let comment = BitbucketWorkItemComment {
            id: id.to_string(),
            kind: "comment".into(),
            author: author.as_ref().map(|a| a.login.clone()).unwrap_or_default(),
            author_avatar_url: author.map(|a| a.avatar_url).unwrap_or_default(),
            body,
            created_at: string_field(row, "created_on").unwrap_or_default(),
            url: row
                .pointer("/links/html/href")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            state: String::new(),
            path: inline
                .and_then(|inline| string_field(inline, "path"))
                .unwrap_or_default(),
            line: inline.and_then(|inline| {
                inline
                    .get("to")
                    .and_then(Value::as_i64)
                    .or_else(|| inline.get("from").and_then(Value::as_i64))
            }),
            resolved: row.get("resolution").is_some_and(|value| !value.is_null()),
            thread_id: root.to_string(),
            replies: Vec::new(),
        };
        if root == id {
            roots.push(comment);
        } else {
            replies.push((root, comment));
        }
    }
    for (root, reply) in replies {
        match roots
            .iter_mut()
            .find(|comment| comment.id == root.to_string())
        {
            Some(parent) => parent.replies.push(reply),
            // The parent fell outside this page; keep the reply visible.
            None => roots.push(reply),
        }
    }
    roots.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(BitbucketWorkItemThread {
        comments: roots,
        truncated: has_next_page,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    })
}

fn parse_pr_diff(stat: &Value, patch: &str, truncated: bool) -> Result<BitbucketPrDiff, String> {
    let rows = stat
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "Bitbucket did not return pull request diffs".to_string())?;
    let mut files = Vec::new();
    let mut additions = 0;
    let mut deletions = 0;
    for row in rows {
        let path = row
            .pointer("/new/path")
            .and_then(Value::as_str)
            .or_else(|| row.pointer("/old/path").and_then(Value::as_str))
            .unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        let added = row.get("lines_added").and_then(Value::as_i64).unwrap_or(0);
        let removed = row
            .get("lines_removed")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        additions += added;
        deletions += removed;
        files.push(BitbucketPrFile {
            path: path.to_string(),
            additions: added,
            deletions: removed,
        });
    }
    let (patch, cut) = cap_patch(patch);
    Ok(BitbucketPrDiff {
        additions,
        deletions,
        files,
        patch,
        truncated: truncated || cut,
    })
}

/// Trims an oversized patch back to the last whole file block.
fn cap_patch(patch: &str) -> (String, bool) {
    if patch.len() <= MAX_DIFF_BYTES {
        return (patch.to_string(), false);
    }
    let mut end = MAX_DIFF_BYTES;
    while !patch.is_char_boundary(end) {
        end -= 1;
    }
    let head = &patch[..end];
    let keep = head.rfind("\ndiff --git ").map(|at| at + 1).unwrap_or(0);
    (head[..keep].to_string(), true)
}

fn person(value: Option<&Value>) -> Option<BitbucketAssignee> {
    let value = value.filter(|value| value.is_object())?;
    let login = string_field(value, "nickname").or_else(|| string_field(value, "display_name"))?;
    if login.is_empty() {
        return None;
    }
    Some(BitbucketAssignee {
        login,
        avatar_url: value
            .pointer("/links/avatar/href")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn people(value: Option<&Value>) -> Vec<BitbucketAssignee> {
    value
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(|row| person(Some(row))).collect())
        .unwrap_or_default()
}

fn normalize_state(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "new" | "open" | "on hold" | "reopened" => "open".into(),
        "declined" | "superseded" | "resolved" | "invalid" | "duplicate" | "wontfix" => {
            "closed".into()
        }
        other => other.into(),
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
}

struct BitbucketResponse {
    value: Value,
    has_next_page: bool,
}

fn basic_auth(config: &BitbucketConfig) -> String {
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", config.email, config.token));
    format!("Basic {encoded}")
}

fn bitbucket_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .redirects(0)
        .build()
}

fn bitbucket_get(config: &BitbucketConfig, path: &str) -> Result<BitbucketResponse, String> {
    read_response(get_following_redirects(config, path, "application/json"))
}

/// Collects `values` across pages by following `next`, stopping at `max_items`.
/// The flag reports that more rows existed than were returned.
fn bitbucket_get_values(
    config: &BitbucketConfig,
    path: &str,
    max_items: usize,
) -> Result<(Vec<Value>, bool), String> {
    let mut path = path.to_string();
    let mut values: Vec<Value> = Vec::new();
    for _ in 0..MAX_PAGES {
        let response = bitbucket_get(config, &path)?;
        let rows = response
            .value
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| "Bitbucket returned an unexpected response".to_string())?;
        values.extend(rows.iter().cloned());
        let next = response
            .value
            .get("next")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|next| !next.is_empty());
        // The final page can overflow the limit too, so check before `next`.
        let over = values.len() > max_items;
        if over {
            values.truncate(max_items);
        }
        let Some(next) = next else {
            return Ok((values, over));
        };
        if values.len() >= max_items {
            return Ok((values, true));
        }
        // Only follow pagination links on the configured API, so credentials
        // never leave it.
        match next.strip_prefix(config.api_base()) {
            Some(rest) => path = rest.to_string(),
            None => return Err("Bitbucket returned an unexpected pagination link".into()),
        }
    }
    Err("Bitbucket returned too many pages".into())
}

/// `scheme://host[:port]` of a URL.
fn origin(url: &str) -> &str {
    let after_scheme = url.find("://").map(|at| at + 3).unwrap_or(0);
    match url[after_scheme..].find('/') {
        Some(end) => &url[..after_scheme + end],
        None => url,
    }
}

/// Bitbucket answers some endpoints (diff, diffstat) with a redirect to the
/// canonical URL. Follow it by hand so the credentials only ever go to the
/// API origin they were configured for.
// `ureq::Error` is large, but `read_response` consumes it directly.
#[allow(clippy::result_large_err)]
fn get_following_redirects(
    config: &BitbucketConfig,
    path: &str,
    accept: &str,
) -> Result<ureq::Response, ureq::Error> {
    let api_origin = origin(config.api_base()).to_string();
    let mut url = format!("{}{path}", config.api_base());
    let mut hops = 0;
    loop {
        let response = bitbucket_agent()
            .get(&url)
            .set("Authorization", &basic_auth(config))
            .set("Accept", accept)
            .set("User-Agent", USER_AGENT)
            .call()?;
        if !matches!(response.status(), 301 | 302 | 303 | 307 | 308) || hops >= 4 {
            return Ok(response);
        }
        let Some(location) = response.header("Location").map(str::trim) else {
            return Ok(response);
        };
        let next = if location.starts_with('/') {
            format!("{api_origin}{location}")
        } else {
            location.to_string()
        };
        if origin(&next) != api_origin {
            return Ok(response);
        }
        url = next;
        hops += 1;
    }
}

fn bitbucket_post_json(
    config: &BitbucketConfig,
    path: &str,
    body: Value,
) -> Result<BitbucketResponse, String> {
    let url = format!("{}{path}", config.api_base());
    read_response(
        bitbucket_agent()
            .post(&url)
            .set("Authorization", &basic_auth(config))
            .set("Accept", "application/json")
            .set("User-Agent", USER_AGENT)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
    )
}

/// Fetches a plain-text body such as a raw diff.
fn bitbucket_get_text(config: &BitbucketConfig, path: &str) -> Result<(String, bool), String> {
    let result = get_following_redirects(config, path, "text/plain");
    let response = match result {
        Ok(response) => response,
        Err(error) => return read_response(Err(error)).map(|_| (String::new(), false)),
    };
    // A blocked or exhausted redirect hands back the 3xx itself.
    if !(200..300).contains(&response.status()) {
        let status = response.status();
        let body = response.into_string().unwrap_or_default();
        return Err(bitbucket_http_error(status, &body));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((MAX_DIFF_BYTES * 2 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Bitbucket returned an unreadable response".to_string())?;
    let capped = bytes.len() > MAX_DIFF_BYTES * 2;
    if capped {
        bytes.truncate(MAX_DIFF_BYTES * 2);
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), capped))
}

fn read_response(result: Result<ureq::Response, ureq::Error>) -> Result<BitbucketResponse, String> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => {
            return Err("Bitbucket email or API token is invalid".into());
        }
        Err(ureq::Error::Status(403, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(forbidden_error(&body));
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(bitbucket_http_error(status, &body));
        }
        Err(_) => return Err("Could not reach Bitbucket".into()),
    };
    let status = response.status();
    let body = response
        .into_string()
        .map_err(|_| "Bitbucket returned an unreadable response".to_string())?;
    if !(200..300).contains(&status) {
        return Err(bitbucket_http_error(status, &body));
    }
    let value: Value =
        serde_json::from_str(&body).map_err(|_| "Bitbucket returned invalid JSON".to_string())?;
    let has_next_page = value
        .get("next")
        .and_then(Value::as_str)
        .is_some_and(|next| !next.trim().is_empty());
    Ok(BitbucketResponse {
        value,
        has_next_page,
    })
}

/// Names the missing scopes when Bitbucket reports them, since a bare 403 is
/// otherwise indistinguishable from a bad token.
fn forbidden_error(body: &str) -> String {
    let required: Vec<String> = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/detail/required")
                .and_then(Value::as_array)
                .map(|scopes| {
                    scopes
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|scope| match scope {
                            "account" => "read:user:bitbucket".to_string(),
                            other => other.to_string(),
                        })
                        .collect()
                })
        })
        .unwrap_or_default();
    if required.is_empty() {
        "Bitbucket denied access; check the API token's scopes".into()
    } else {
        format!(
            "Bitbucket API token is missing a required scope: {}",
            required.join(", ")
        )
    }
}

fn bitbucket_http_error(status: u16, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body).ok().and_then(|value| {
        value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    });
    message.unwrap_or_else(|| format!("Bitbucket request failed ({status})"))
}

fn encode_component(value: &str) -> String {
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

fn bitbucket_repo_for(root: &Path) -> Result<String, String> {
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
        if let Some(repo) = repo_from_remote(remote.trim()) {
            matches.push((name == "remote.origin.url", repo));
        }
    }
    matches
        .iter()
        .find(|(origin, _)| *origin)
        .or_else(|| matches.first())
        .map(|(_, repo)| repo.clone())
        .ok_or_else(|| "No Bitbucket remote found".to_string())
}

/// Accepts `git@bitbucket.org:ws/repo.git`, `https://user@bitbucket.org/ws/repo.git`
/// and `ssh://git@bitbucket.org/ws/repo.git`.
fn repo_from_remote(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let (authority, path) = if let Some((_, rest)) = remote.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?, path)
    } else {
        let (user_host, path) = remote.split_once(':')?;
        (user_host.rsplit('@').next()?, path)
    };
    let host = authority.split(':').next()?.to_ascii_lowercase();
    if host != BITBUCKET_HOST {
        return None;
    }
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    valid_repo_path(path).then(|| path.to_string())
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("bitbucket-config.json"))
}

fn read_config(app: &AppHandle) -> Result<Option<BitbucketConfig>, String> {
    let path = config_path(app)?;
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut config: BitbucketConfig = serde_json::from_str(&raw)
                .map_err(|_| "Bitbucket settings are invalid".to_string())?;
            config.email = config.email.trim().to_string();
            config.token = config.token.trim().to_string();
            if config.token.is_empty() || config.email.is_empty() {
                Ok(None)
            } else {
                Ok(Some(config))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn require_config(app: &AppHandle) -> Result<BitbucketConfig, String> {
    read_config(app)?.ok_or_else(|| "Connect Bitbucket in Settings".to_string())
}

fn write_config(app: &AppHandle, config: &BitbucketConfig) -> Result<(), String> {
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

    #[test]
    fn parses_bitbucket_remotes() {
        for remote in [
            "git@bitbucket.org:acme/app.git",
            "https://nate@bitbucket.org/acme/app.git",
            "ssh://git@bitbucket.org/acme/app",
            "https://bitbucket.org/acme/app/",
        ] {
            assert_eq!(
                repo_from_remote(remote).as_deref(),
                Some("acme/app"),
                "{remote}"
            );
        }
        for remote in [
            "git@github.com:acme/app.git",
            "https://bitbucket.example.com/acme/app.git",
            "https://bitbucket.org/acme",
            "https://bitbucket.org/acme/app/extra",
            "https://bitbucket.org/acme/..",
        ] {
            assert_eq!(repo_from_remote(remote), None, "{remote}");
        }
    }

    #[test]
    fn parses_pull_requests_with_attention_reason() {
        let value = json!({ "values": [
            {
                "id": 7, "title": "Add thing", "state": "OPEN", "draft": true,
                "updated_on": "2026-01-02T03:04:05Z",
                "links": { "html": { "href": "https://bitbucket.org/acme/app/pull-requests/7" } },
                "author": { "account_id": "a1", "nickname": "ann" },
                "reviewers": [{ "account_id": "me", "nickname": "nate",
                    "links": { "avatar": { "href": "https://a/avatar.png" } } }]
            },
            { "id": 8, "title": "Mine", "state": "DECLINED",
              "author": { "account_id": "me", "nickname": "nate" }, "reviewers": [] },
            { "id": 9, "title": "Other", "state": "MERGED",
              "author": { "account_id": "a1" }, "reviewers": [] }
        ]});
        let items = parse_pr_list(&value, "acme/app", Some("me")).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].attention_reason, "review_requested");
        assert!(items[0].draft);
        assert_eq!(items[0].state, "open");
        assert_eq!(items[0].assignees[0].login, "nate");
        assert_eq!(items[1].attention_reason, "authored");
        assert_eq!(items[1].state, "closed");
        assert_eq!(
            items[1].url,
            "https://bitbucket.org/acme/app/pull-requests/8"
        );
        assert_eq!(items[2].attention_reason, "");
        assert_eq!(items[2].state, "merged");
        let plain = parse_pr_list(&value, "acme/app", None).unwrap();
        assert!(plain.iter().all(|item| item.attention_reason.is_empty()));
    }

    #[test]
    fn derives_review_decision_from_participants() {
        let pr = |participants: Value| {
            json!({ "author": { "nickname": "ann" },
                "source": { "branch": { "name": "feat" } },
                "destination": { "branch": { "name": "main" } },
                "participants": participants })
        };
        let approved = parse_work_item_details(&pr(
            json!([{ "role": "REVIEWER", "approved": true, "state": "approved" }]),
        ))
        .unwrap();
        assert_eq!(approved.review_decision, "APPROVED");
        assert_eq!(approved.head_ref_name, "feat");
        assert_eq!(approved.base_ref_name, "main");
        let changes = parse_work_item_details(&pr(json!([
            { "role": "REVIEWER", "approved": true },
            { "role": "REVIEWER", "approved": false, "state": "changes_requested" }
        ])))
        .unwrap();
        assert_eq!(changes.review_decision, "CHANGES_REQUESTED");
        let author_only =
            parse_work_item_details(&pr(json!([{ "role": "PARTICIPANT", "approved": true }])))
                .unwrap();
        assert_eq!(author_only.review_decision, "");
    }

    #[test]
    fn folds_replies_under_top_level_comments() {
        let value = json!({ "values": [
            { "id": 3, "parent": { "id": 2 }, "created_on": "2026-01-01T00:00:03Z",
              "content": { "raw": "deep reply" }, "user": { "nickname": "c" } },
            { "id": 2, "parent": { "id": 1 }, "created_on": "2026-01-01T00:00:02Z",
              "content": { "raw": "reply" }, "user": { "nickname": "b" } },
            { "id": 4, "deleted": true, "created_on": "2026-01-01T00:00:04Z",
              "content": { "raw": "gone" } },
            { "id": 1, "created_on": "2026-01-01T00:00:01Z",
              "content": { "raw": "top" }, "user": { "nickname": "a" },
              "inline": { "path": "src/lib.rs", "to": 12, "from": null },
              "resolution": { "type": "resolved" } }
        ]});
        let thread = parse_work_item_thread(&value, true).unwrap();
        assert!(thread.truncated);
        assert_eq!(thread.comments.len(), 1);
        let top = &thread.comments[0];
        assert_eq!(top.body, "top");
        assert_eq!(top.path, "src/lib.rs");
        assert_eq!(top.line, Some(12));
        assert!(top.resolved);
        assert_eq!(
            top.replies
                .iter()
                .map(|r| r.body.as_str())
                .collect::<Vec<_>>(),
            ["reply", "deep reply"]
        );
        assert!(top.replies.iter().all(|r| r.thread_id == "1"));
    }

    #[test]
    fn builds_diff_summary_and_caps_patch_at_file_boundary() {
        let stat = json!({ "values": [
            { "lines_added": 3, "lines_removed": 1, "new": { "path": "a.rs" } },
            { "lines_added": 0, "lines_removed": 5, "old": { "path": "b.rs" }, "new": null }
        ]});
        let diff = parse_pr_diff(&stat, "diff --git a/a.rs b/a.rs\n+x\n", false).unwrap();
        assert_eq!((diff.additions, diff.deletions), (3, 6));
        assert_eq!(diff.files[1].path, "b.rs");
        assert!(!diff.truncated);

        let block = format!("diff --git a/f b/f\n{}\n", "+x".repeat(1024));
        let patch = block.repeat(MAX_DIFF_BYTES / block.len() + 2);
        let (capped, cut) = cap_patch(&patch);
        assert!(cut);
        assert!(capped.len() <= MAX_DIFF_BYTES);
        assert!(capped.ends_with('\n'));
        assert!(capped.len() % block.len() == 0);
    }

    #[test]
    fn redirects_stay_on_the_api_origin() {
        assert_eq!(
            origin("https://api.bitbucket.org/2.0/x"),
            "https://api.bitbucket.org"
        );
        assert_eq!(origin("http://127.0.0.1:8080/a/b"), "http://127.0.0.1:8080");
        assert_eq!(
            origin("https://api.bitbucket.org"),
            "https://api.bitbucket.org"
        );
        assert_ne!(origin("https://evil.example/2.0/x"), origin(API_BASE));
    }

    #[test]
    fn forbidden_error_names_missing_scopes() {
        let body = json!({ "type": "error", "error": {
            "message": "Your credentials lack one or more required privilege scopes.",
            "detail": { "granted": ["pullrequest"], "required": ["account"] } } });
        assert_eq!(
            forbidden_error(&body.to_string()),
            "Bitbucket API token is missing a required scope: read:user:bitbucket"
        );
        assert_eq!(
            forbidden_error("not json"),
            "Bitbucket denied access; check the API token's scopes"
        );
    }

    /// One canned response: status, extra headers, body.
    type Canned = (u16, Vec<(&'static str, String)>, String);

    /// Serves `responses` in order on a local port and returns the raw request
    /// heads (request line plus headers) it saw.
    fn serve(responses: Vec<Canned>) -> (BitbucketConfig, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let config = BitbucketConfig {
            email: "me@example.com".into(),
            token: "secret-token".into(),
            account_id: "acct-1".into(),
            api_base: format!("http://{}", listener.local_addr().unwrap()),
        };
        let base = config.api_base.clone();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, headers, body) in responses {
                // Lets a canned `next` link point back at this server.
                let body = body.replace("{BASE}", &base);
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return requests;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut head = String::new();
                {
                    let mut reader = BufReader::new(&mut stream);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                            break;
                        }
                        head.push_str(&line);
                    }
                }
                requests.push(head);
                let extra: String = headers
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}\r\n"))
                    .collect();
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            requests
        });
        (config, handle)
    }

    fn ok(body: Value) -> Canned {
        (200, Vec::new(), body.to_string())
    }

    #[test]
    fn pull_request_list_caps_page_size_and_scopes_the_query() {
        let (config, server) = serve(vec![ok(json!({ "values": [] }))]);

        let items =
            bitbucket_list_work_items_for(&config, "acme/app", "pr", true, "all", 100).unwrap();

        assert!(items.is_empty());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        let head = &requests[0];
        let line = head.lines().next().unwrap();
        assert!(
            line.starts_with("GET /repositories/acme/app/pullrequests?"),
            "{line}"
        );
        assert!(line.contains("pagelen=50"), "{line}");
        assert!(!line.contains("pagelen=100"), "{line}");
        assert!(line.contains("state=OPEN&state=MERGED"), "{line}");
        assert!(
            line.contains(&format!(
                "q={}",
                encode_component(
                    "(reviewers.account_id=\"acct-1\" OR author.account_id=\"acct-1\")"
                )
            )),
            "{line}"
        );
        assert!(
            head.to_ascii_lowercase().contains("authorization: basic "),
            "{head}"
        );
    }

    #[test]
    fn open_list_omits_the_user_query_and_merged_states() {
        let (config, server) = serve(vec![ok(json!({ "values": [] }))]);

        bitbucket_list_work_items_for(&config, "acme/app", "pr", false, "open", 10).unwrap();

        let line = server.join().unwrap()[0]
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert!(line.contains("pagelen=10"), "{line}");
        assert!(line.contains("state=OPEN"), "{line}");
        assert!(!line.contains("MERGED"), "{line}");
        assert!(!line.contains("q="), "{line}");
    }

    #[test]
    fn issue_kind_is_rejected_since_bitbucket_removed_issues() {
        let config = BitbucketConfig {
            email: "me@example.com".into(),
            token: "t".into(),
            account_id: "a".into(),
            api_base: "http://127.0.0.1:1".into(),
        };
        let error = bitbucket_list_work_items_for(&config, "acme/app", "issue", false, "open", 10)
            .unwrap_err();
        assert_eq!(error, "Unsupported Bitbucket task kind");
    }

    #[test]
    fn list_errors_name_the_repository() {
        let body = json!({ "type": "error", "error": { "message": "Invalid pagelen" } });
        let (config, server) = serve(vec![(400, Vec::new(), body.to_string())]);

        let error = bitbucket_list_work_items_for(&config, "acme/app", "pr", false, "open", 10)
            .unwrap_err();

        server.join().unwrap();
        assert_eq!(
            error,
            "Bitbucket pull requests for acme/app: Invalid pagelen"
        );
    }

    #[test]
    fn diff_follows_same_origin_redirects_and_keeps_credentials() {
        let redirect = |to: &str| (302, vec![("Location", to.to_string())], String::new());
        let stat = json!({ "values": [
            { "lines_added": 2, "lines_removed": 1, "new": { "path": "a.rs" } }
        ]});
        let (config, server) = serve(vec![
            redirect("/repositories/acme/app/diffstat/abc..def"),
            ok(stat),
            redirect("/repositories/acme/app/diff/abc..def"),
            (
                200,
                Vec::new(),
                "diff --git a/a.rs b/a.rs\n+x\n".to_string(),
            ),
        ]);

        let diff = bitbucket_pr_diff_for(&config, "acme/app", 7).unwrap();

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[1].starts_with("GET /repositories/acme/app/diffstat/abc..def"));
        assert!(requests[3].starts_with("GET /repositories/acme/app/diff/abc..def"));
        for head in &requests {
            assert!(
                head.to_ascii_lowercase().contains("authorization: basic "),
                "credentials dropped on a hop: {head}"
            );
        }
        assert_eq!((diff.additions, diff.deletions), (2, 1));
        assert_eq!(diff.files[0].path, "a.rs");
        assert!(diff.patch.starts_with("diff --git"));
    }

    #[test]
    fn redirects_to_another_origin_are_not_followed() {
        let (config, server) = serve(vec![(
            302,
            vec![("Location", "http://other.invalid/steal".to_string())],
            String::new(),
        )]);

        let error = bitbucket_get(&config, "/user").err().unwrap();

        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(error, "Bitbucket request failed (302)");
    }

    #[test]
    fn maps_build_statuses_to_checks_keeping_the_newest_per_key() {
        let value = json!({ "values": [
            { "key": "pipeline", "name": "Pipeline - pullrequests: **", "state": "SUCCESSFUL",
              "url": "https://bitbucket.org/acme/app/pipelines/results/9",
              "created_on": "2026-10-07T22:40:00Z", "updated_on": "2026-10-07T22:48:29Z" },
            { "key": "sonar", "name": "", "state": "INPROGRESS",
              "created_on": "2026-10-07T22:41:00Z", "updated_on": "2026-10-07T22:42:00Z" },
            { "key": "pipeline", "name": "Pipeline - pullrequests: **", "state": "FAILED",
              "created_on": "2026-10-07T20:00:00Z", "updated_on": "2026-10-07T20:05:00Z" },
            { "key": "lint", "name": "Lint", "state": "STOPPED" },
            { "key": "odd", "name": "Odd", "state": "SOMETHING_NEW", "url": "" },
            { "key": "", "name": "" }
        ]});

        let checks = parse_pr_checks(&value, "abc123".into()).unwrap();

        assert_eq!(checks.head_oid, "abc123");
        let summary: Vec<_> = checks
            .checks
            .iter()
            .map(|c| (c.name.as_str(), c.state.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("Pipeline - pullrequests: **", "pass"),
                ("sonar", "pending"),
                ("Lint", "cancel"),
                ("Odd", "unknown"),
            ]
        );
        let pipeline = &checks.checks[0];
        assert_eq!(
            pipeline.url.as_deref(),
            Some("https://bitbucket.org/acme/app/pipelines/results/9")
        );
        assert_eq!(
            pipeline.completed_at.as_deref(),
            Some("2026-10-07T22:48:29Z")
        );
        // A running build has not completed, even though it has an update time.
        assert_eq!(checks.checks[1].completed_at, None);
        assert_eq!(checks.checks[3].url, None);
    }

    #[test]
    fn pr_checks_read_the_statuses_of_the_captured_head_commit() {
        let (config, server) = serve(vec![
            ok(json!({ "source": { "commit": { "hash": "deadbeef" } } })),
            ok(json!({ "values": [
                { "key": "ci", "name": "CI", "state": "FAILED", "updated_on": "2026-10-07T00:00:00Z" }
            ]})),
        ]);

        let checks = bitbucket_pr_checks_for(&config, "acme/app", 7).unwrap();

        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /repositories/acme/app/pullrequests/7 "));
        assert!(
            requests[1].starts_with("GET /repositories/acme/app/commit/deadbeef/statuses?"),
            "{}",
            requests[1]
        );
        assert!(requests[1].contains("pagelen=100"));
        assert_eq!(checks.head_oid, "deadbeef");
        assert_eq!(checks.checks[0].state, "fail");
    }

    #[test]
    fn list_follows_pagination_up_to_the_requested_limit() {
        let page = |ids: &[i64], next: bool| {
            let values: Vec<Value> = ids
                .iter()
                .map(|id| {
                    json!({ "id": id, "title": "t", "state": "OPEN",
                            "author": { "display_name": "A" } })
                })
                .collect();
            let mut body = json!({ "values": values });
            if next {
                body["next"] = json!("{BASE}/repositories/acme/app/pullrequests?page=2");
            }
            ok(body)
        };
        let (config, server) = serve(vec![page(&[1, 2], true), page(&[3, 4], true)]);

        let items =
            bitbucket_list_work_items_for(&config, "acme/app", "pr", false, "all", 3).unwrap();

        assert_eq!(items.len(), 3);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("pagelen=3"), "{}", requests[0]);
        assert!(requests[1].starts_with("GET /repositories/acme/app/pullrequests?page=2"));
    }

    #[test]
    fn the_final_page_cannot_exceed_the_limit() {
        let rows = |n: usize| json!({ "values": vec![json!({}); n] });
        let (config, server) = serve(vec![ok(rows(4))]);
        let (values, truncated) = bitbucket_get_values(&config, "/x", 3).unwrap();
        server.join().unwrap();
        assert_eq!((values.len(), truncated), (3, true));

        let (config, server) = serve(vec![ok(rows(3))]);
        let (values, truncated) = bitbucket_get_values(&config, "/x", 3).unwrap();
        server.join().unwrap();
        assert_eq!((values.len(), truncated), (3, false));
    }

    #[test]
    fn empty_pages_that_link_to_themselves_stop() {
        let looping = || ok(json!({ "values": [], "next": "{BASE}/x" }));
        let (config, server) = serve((0..MAX_PAGES).map(|_| looping()).collect());

        let error = bitbucket_get_values(&config, "/x", 100).unwrap_err();

        server.join().unwrap();
        assert_eq!(error, "Bitbucket returned too many pages");
    }

    #[test]
    fn pagination_links_to_another_origin_are_not_followed() {
        let (config, server) = serve(vec![ok(json!({
            "values": [{}],
            "next": "http://other.invalid/page2"
        }))]);

        let error = bitbucket_get_values(&config, "/x", 100).unwrap_err();

        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(error, "Bitbucket returned an unexpected pagination link");
    }

    #[test]
    fn pr_checks_gather_every_status_page_for_the_captured_commit() {
        let (config, server) = serve(vec![
            ok(json!({ "source": { "commit": { "hash": "deadbeef" } } })),
            ok(json!({
                "values": [{ "key": "a", "name": "A", "state": "SUCCESSFUL" }],
                "next": "{BASE}/repositories/acme/app/commit/deadbeef/statuses?page=2"
            })),
            ok(json!({ "values": [{ "key": "b", "name": "B", "state": "FAILED" }] })),
        ]);

        let checks = bitbucket_pr_checks_for(&config, "acme/app", 7).unwrap();

        assert_eq!(server.join().unwrap().len(), 3);
        assert_eq!(checks.checks.len(), 2);
    }

    #[test]
    fn diff_text_rejects_a_redirect_it_cannot_follow() {
        let (config, server) = serve(vec![(
            302,
            vec![("Location", "http://other.invalid/steal".to_string())],
            String::new(),
        )]);

        let error = bitbucket_get_text(&config, "/x/diff").unwrap_err();

        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(error, "Bitbucket request failed (302)");
    }

    #[test]
    fn pr_checks_reject_a_malformed_status_payload() {
        let error = parse_pr_checks(&json!({ "oops": [] }), String::new()).unwrap_err();
        assert_eq!(error, "Bitbucket did not return build statuses");
    }

    #[test]
    fn maps_pipeline_steps_to_check_states() {
        let value = json!({ "values": [
            { "name": "Build", "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } },
              "started_on": "2026-10-07T10:00:00Z", "completed_on": "2026-10-07T10:02:00Z" },
            { "name": "Test", "state": { "name": "COMPLETED", "result": { "name": "FAILED" } } },
            { "name": "Lint", "state": { "name": "COMPLETED", "result": { "name": "ERROR" } } },
            { "name": "Deploy", "state": { "name": "COMPLETED", "result": { "name": "STOPPED" } } },
            { "name": "Docs", "state": { "name": "COMPLETED", "result": { "name": "NOT_RUN" } } },
            { "name": "Smoke", "state": { "name": "IN_PROGRESS" } },
            { "name": "Queue", "state": { "name": "PENDING" } },
            { "name": "Mystery", "state": { "name": "SOMETHING" } },
            { "name": "", "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } } },
            { "state": { "name": "COMPLETED", "result": { "name": "FAILED" } } }
        ]});

        let details = parse_build_steps(&value).unwrap();

        let states: Vec<_> = details
            .steps
            .iter()
            .map(|s| (s.name.as_str(), s.state.as_str()))
            .collect();
        assert_eq!(
            states,
            [
                ("Build", "pass"),
                ("Test", "fail"),
                ("Lint", "fail"),
                ("Deploy", "cancel"),
                ("Docs", "skipping"),
                ("Smoke", "pending"),
                ("Queue", "pending"),
                ("Mystery", "unknown"),
                ("Step 9", "pass"),
                ("Step 10", "fail"),
            ]
        );
        assert_eq!(
            details.steps[0].completed_at.as_deref(),
            Some("2026-10-07T10:02:00Z")
        );
        assert_eq!(details.notice, None);
        assert!(details.annotations.is_empty());
    }

    #[test]
    fn build_details_resolve_a_pipeline_by_number_then_list_steps() {
        let (config, server) = serve(vec![
            ok(json!({ "build_number": 42, "uuid": "{abc-123}" })),
            ok(json!({ "values": [
                { "name": "Build", "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } } }
            ]})),
        ]);

        let details = bitbucket_build_details_for(&config, "acme/app", "42").unwrap();

        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /repositories/acme/app/pipelines/42 "));
        assert!(
            requests[1].starts_with("GET /repositories/acme/app/pipelines/%7Babc-123%7D/steps/?"),
            "{}",
            requests[1]
        );
        assert_eq!(details.steps.len(), 1);
        assert_eq!(details.notice, None);
    }

    #[test]
    fn build_details_fall_back_to_the_pipeline_list_when_the_number_is_not_addressable() {
        let (config, server) = serve(vec![
            (
                404,
                Vec::new(),
                json!({ "error": { "message": "nope" } }).to_string(),
            ),
            ok(json!({ "values": [
                { "build_number": 43, "uuid": "{other}" },
                { "build_number": 42, "uuid": "{abc-123}" }
            ]})),
            ok(json!({ "values": [] })),
        ]);

        let details = bitbucket_build_details_for(&config, "acme/app", "42").unwrap();

        let requests = server.join().unwrap();
        assert!(requests[1].contains("/pipelines/?sort=-created_on&pagelen=100"));
        assert!(requests[2].contains("/pipelines/%7Babc-123%7D/steps/"));
        assert!(details.steps.is_empty());
        assert_eq!(details.notice, None);
    }

    #[test]
    fn build_details_say_what_scope_is_missing_instead_of_failing() {
        let body = json!({ "type": "error", "error": {
            "message": "Your credentials lack one or more required privilege scopes.",
            "detail": { "granted": ["pullrequest"], "required": ["pipeline"] } } });
        let (config, server) = serve(vec![(403, Vec::new(), body.to_string())]);

        let details = bitbucket_build_details_for(&config, "acme/app", "42").unwrap();

        server.join().unwrap();
        assert!(details.steps.is_empty());
        let notice = details.notice.unwrap();
        assert!(notice.contains("read:pipeline:bitbucket"), "{notice}");
        assert!(notice.contains("Settings"), "{notice}");
        // A token's scopes are fixed at creation, so "add it" would mislead.
        assert!(notice.contains("create a new token"), "{notice}");
        assert!(!notice.contains("Add it"), "{notice}");
    }

    #[test]
    fn build_details_report_a_missing_pipeline_without_erroring() {
        let (config, server) = serve(vec![
            (404, Vec::new(), "{}".to_string()),
            ok(json!({ "values": [ { "build_number": 7, "uuid": "{x}" } ] })),
        ]);

        let details = bitbucket_build_details_for(&config, "acme/app", "42").unwrap();

        server.join().unwrap();
        assert_eq!(
            details.notice.as_deref(),
            Some("Bitbucket has no steps for this build.")
        );
    }

    #[test]
    fn build_details_reject_a_bad_build_number_and_surface_real_errors() {
        let config = BitbucketConfig {
            email: "e".into(),
            token: "t".into(),
            account_id: "a".into(),
            api_base: "http://127.0.0.1:1".into(),
        };
        for bad in ["", "0", "-3", "abc", "1; DROP"] {
            assert_eq!(
                bitbucket_build_details_for(&config, "acme/app", bad).unwrap_err(),
                "Invalid Bitbucket build number",
                "{bad:?}"
            );
        }
        let (config, server) = serve(vec![
            ok(json!({ "build_number": 42, "uuid": "{u}" })),
            (
                500,
                Vec::new(),
                json!({ "error": { "message": "boom" } }).to_string(),
            ),
        ]);
        let error = bitbucket_build_details_for(&config, "acme/app", "42").unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "boom");
    }
}
