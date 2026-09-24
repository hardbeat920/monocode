use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_LIMIT: u32 = 40;
const COMMENT_LIMIT: u32 = 50;
const ISSUE_FIELDS: [&str; 6] = [
    "summary", "status", "updated", "labels", "assignee", "project",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraStatus {
    pub connected: bool,
    pub site: String,
    pub email: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct JiraConfig {
    site: String,
    email: String,
    token: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraProject {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraLabel {
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraAssignee {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssue {
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
    pub labels: Vec<JiraLabel>,
    pub assignees: Vec<JiraAssignee>,
    pub draft: bool,
    pub repo: String,
    pub team_id: String,
    pub team_name: String,
    pub project_path: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueComment {
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
    pub replies: Vec<JiraIssueComment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueThread {
    pub comments: Vec<JiraIssueComment>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}

#[tauri::command(async)]
pub fn jira_status(app: AppHandle) -> Result<JiraStatus, String> {
    let config = read_config(&app)?;
    Ok(status_for(config.as_ref()))
}

#[tauri::command]
pub async fn jira_set_config(
    app: AppHandle,
    site: String,
    email: String,
    token: String,
) -> Result<JiraStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = token.trim().to_string();
        if token.is_empty() {
            delete_config(&app)?;
            return Ok(status_for(None));
        }
        let email = email.trim().to_string();
        if email.is_empty() || !email.contains('@') {
            return Err("Enter the email address of your Atlassian account".into());
        }
        let config = JiraConfig {
            site: normalize_jira_site(&site)?,
            email,
            token,
        };
        let myself = jira_get(&config, "/rest/api/3/myself")?;
        if string_field(&myself, "accountId")
            .unwrap_or_default()
            .is_empty()
        {
            return Err("Jira did not return the current user".into());
        }
        write_config(&app, &config)?;
        Ok(status_for(Some(&config)))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_list_projects(app: AppHandle) -> Result<Vec<JiraProject>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        fetch_jira_projects(|start| {
            jira_get(
                &config,
                &format!("/rest/api/3/project/search?maxResults=100&orderBy=name&startAt={start}"),
            )
        })
    })
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
    tauri::async_runtime::spawn_blocking(move || {
        let Some(config) = read_config(&app)? else {
            return Ok(Vec::new());
        };
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100);
        let jql = issue_jql(assigned_to_me, &state, &project_ids);
        let data = jira_post(
            &config,
            "/rest/api/3/search/jql",
            &json!({ "jql": jql, "maxResults": limit, "fields": ISSUE_FIELDS }),
        )?;
        parse_jira_issues(&data, &config.site)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_issue_details(app: AppHandle, key: String) -> Result<JiraIssueDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let key = require_issue_key(&key)?;
        let data = jira_get(
            &config,
            &format!("/rest/api/3/issue/{key}?fields=description,reporter,creator,assignee"),
        )?;
        parse_jira_issue_details(&data)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_issue_thread(app: AppHandle, key: String) -> Result<JiraIssueThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let key = require_issue_key(&key)?;
        // Newest first so a long thread keeps its latest comments; re-sorted below.
        let data = jira_get(
            &config,
            &format!("/rest/api/3/issue/{key}/comment?maxResults={COMMENT_LIMIT}&orderBy=-created"),
        )?;
        parse_jira_issue_thread(&data, &config.site, key)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn jira_issue_comment(
    app: AppHandle,
    key: String,
    body: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&app)?;
        let key = require_issue_key(&key)?;
        let body = body.trim();
        if body.is_empty() {
            return Err("Comment cannot be empty".into());
        }
        let data = jira_post(
            &config,
            &format!("/rest/api/3/issue/{key}/comment"),
            &json!({ "body": text_to_adf(body) }),
        )?;
        let id = string_field(&data, "id").unwrap_or_default();
        if id.is_empty() {
            return Err("Could not post Jira comment".into());
        }
        Ok(comment_url(&config.site, key, &id))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn status_for(config: Option<&JiraConfig>) -> JiraStatus {
    JiraStatus {
        connected: config.is_some(),
        site: config.map(|config| config.site.clone()).unwrap_or_default(),
        email: config
            .map(|config| config.email.clone())
            .unwrap_or_default(),
    }
}

fn issue_jql(assigned_to_me: bool, state: &str, project_ids: &[String]) -> String {
    let mut clauses = Vec::new();
    if assigned_to_me {
        clauses.push("assignee = currentUser()".to_string());
    }
    let ids: Vec<&str> = project_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit()))
        .collect();
    if !ids.is_empty() {
        clauses.push(format!("project in ({})", ids.join(", ")));
    }
    if !state.trim().eq_ignore_ascii_case("all") {
        clauses.push("statusCategory != Done".to_string());
    }
    // The search endpoint rejects unbounded JQL, so an unfiltered query is
    // limited to the last year of activity.
    if clauses.is_empty() {
        clauses.push("updated >= -365d".to_string());
    }
    format!("{} ORDER BY updated DESC", clauses.join(" AND "))
}

fn jira_authorization(config: &JiraConfig) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!(
        "{}:{}",
        config.email.trim(),
        config.token.trim()
    ));
    format!("Basic {encoded}")
}

fn jira_get(config: &JiraConfig, path: &str) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(&format!("{}{path}", config.site))
        .set("Authorization", &jira_authorization(config))
        .set("Accept", "application/json")
        .call();
    read_jira_response(result)
}

fn jira_post(config: &JiraConfig, path: &str, body: &Value) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let payload = serde_json::to_string(body).map_err(|error| error.to_string())?;
    let result = agent
        .post(&format!("{}{path}", config.site))
        .set("Authorization", &jira_authorization(config))
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_string(&payload);
    read_jira_response(result)
}

fn read_jira_response(result: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => {
            return Err("Jira email or API token is invalid".into());
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(jira_http_error(status, &body));
        }
        Err(_) => return Err("Could not reach Jira".into()),
    };
    let status = response.status();
    let body = response
        .into_string()
        .map_err(|_| "Jira returned an unreadable response".to_string())?;
    if !(200..300).contains(&status) {
        return Err(jira_http_error(status, &body));
    }
    serde_json::from_str(&body).map_err(|_| "Jira returned invalid JSON".to_string())
}

fn jira_http_error(status: u16, body: &str) -> String {
    if let Some(message) = jira_error_message(body) {
        return message;
    }
    match status {
        403 => "Jira denied access. Check the API token's permissions".into(),
        404 => "Jira could not find that issue".into(),
        _ => format!("Jira request failed ({status})"),
    }
}

fn jira_error_message(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    let from_list = parsed
        .get("errorMessages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.iter().filter_map(Value::as_str).next())
        .map(str::to_string);
    let from_map = || {
        parsed
            .get("errors")
            .and_then(Value::as_object)
            .and_then(|errors| errors.values().filter_map(Value::as_str).next())
            .map(str::to_string)
    };
    from_list
        .or_else(from_map)
        .map(|message| message.trim().to_string())
        .filter(|message| !message.is_empty())
}

fn parse_jira_projects(data: &Value) -> Result<Vec<JiraProject>, String> {
    let values = data
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "Jira did not return projects".to_string())?;
    Ok(values
        .iter()
        .filter_map(|value| {
            let id = string_field(value, "id").filter(|id| !id.is_empty())?;
            let key = string_field(value, "key").unwrap_or_default();
            let name = string_field(value, "name")
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| key.clone());
            Some(JiraProject { id, key, name })
        })
        .collect())
}

// Follow every page: a partial project list would silently exclude visible
// projects when converting hidden projects into the JQL allowlist.
fn fetch_jira_projects(
    mut fetch: impl FnMut(u64) -> Result<Value, String>,
) -> Result<Vec<JiraProject>, String> {
    let mut start = 0;
    let mut projects = Vec::new();
    loop {
        let data = fetch(start)?;
        let page = parse_jira_projects(&data)?;
        let count = data["values"].as_array().map_or(0, Vec::len) as u64;
        projects.extend(page);
        if data["isLast"].as_bool() == Some(true) {
            break;
        }
        let next = data["startAt"].as_u64().unwrap_or(start) + count;
        if data["total"].as_u64().is_some_and(|total| next >= total) {
            break;
        }
        if count == 0 || next <= start {
            if data["isLast"].as_bool() == Some(false) {
                return Err("Jira project pagination did not advance".into());
            }
            break;
        }
        start = next;
    }
    let mut seen = std::collections::HashSet::new();
    projects.retain(|project| seen.insert(project.id.clone()));
    Ok(projects)
}

fn parse_jira_issues(data: &Value, site: &str) -> Result<Vec<JiraIssue>, String> {
    let issues = data
        .get("issues")
        .and_then(Value::as_array)
        .ok_or_else(|| "Jira did not return issues".to_string())?;
    Ok(issues
        .iter()
        .filter_map(|issue| parse_jira_issue(issue, site))
        .collect())
}

fn parse_jira_issue(node: &Value, site: &str) -> Option<JiraIssue> {
    let id = string_field(node, "id").filter(|id| !id.is_empty())?;
    let key = string_field(node, "key").filter(|key| !key.is_empty())?;
    let fields = node.get("fields").cloned().unwrap_or(Value::Null);
    let project = fields.get("project");
    let status = fields.get("status");
    let labels = fields
        .get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(Value::as_str)
                .map(|name| JiraLabel {
                    name: name.to_string(),
                    color: String::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    let (assignee, avatar_url) = person_fields(fields.get("assignee"));
    Some(JiraIssue {
        provider: "jira".into(),
        kind: "jira".into(),
        number: issue_number(&key),
        url: format!("{site}/browse/{key}"),
        title: string_field(&fields, "summary").unwrap_or_default(),
        state: status
            .and_then(|value| string_field(value, "name"))
            .unwrap_or_else(|| "Open".into()),
        state_type: status
            .and_then(|value| value.pointer("/statusCategory/key"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        updated_at: normalize_jira_time(&string_field(&fields, "updated").unwrap_or_default()),
        labels,
        assignees: assignee
            .map(|login| vec![JiraAssignee { login, avatar_url }])
            .unwrap_or_default(),
        draft: false,
        repo: project
            .and_then(|value| string_field(value, "key"))
            .unwrap_or_default(),
        team_id: project
            .and_then(|value| string_field(value, "id"))
            .unwrap_or_default(),
        team_name: project
            .and_then(|value| string_field(value, "name"))
            .unwrap_or_default(),
        project_path: String::new(),
        id,
        identifier: key,
    })
}

fn parse_jira_issue_details(data: &Value) -> Result<JiraIssueDetails, String> {
    let fields = data
        .get("fields")
        .ok_or_else(|| "Jira did not return that issue".to_string())?;
    let (author, author_avatar_url) = [fields.get("reporter"), fields.get("creator")]
        .into_iter()
        .map(person_fields)
        .find(|(name, _)| name.is_some())
        .unwrap_or_else(|| person_fields(fields.get("assignee")));
    Ok(JiraIssueDetails {
        body: rich_text(fields.get("description")),
        author: author.unwrap_or_default(),
        author_avatar_url,
    })
}

fn parse_jira_issue_thread(data: &Value, site: &str, key: &str) -> Result<JiraIssueThread, String> {
    let comments = data
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| "Jira did not return comments".to_string())?;
    let total = data
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(comments.len() as u64);
    let mut parsed: Vec<JiraIssueComment> = comments
        .iter()
        .filter_map(|comment| parse_jira_comment(comment, site, key))
        .collect();
    parsed.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    Ok(JiraIssueThread {
        truncated: total > comments.len() as u64,
        comments: parsed,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    })
}

fn parse_jira_comment(node: &Value, site: &str, key: &str) -> Option<JiraIssueComment> {
    let id = string_field(node, "id").filter(|id| !id.is_empty())?;
    let (author, author_avatar_url) = person_fields(node.get("author"));
    Some(JiraIssueComment {
        url: comment_url(site, key, &id),
        kind: "comment".into(),
        author: author.unwrap_or_default(),
        author_avatar_url,
        body: rich_text(node.get("body")),
        created_at: normalize_jira_time(&string_field(node, "created").unwrap_or_default()),
        state: String::new(),
        path: String::new(),
        line: None,
        resolved: false,
        thread_id: String::new(),
        replies: Vec::new(),
        id,
    })
}

fn comment_url(site: &str, key: &str, id: &str) -> String {
    format!("{site}/browse/{key}?focusedCommentId={id}")
}

fn issue_number(key: &str) -> i64 {
    key.rsplit('-')
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or(0)
}

/// Jira sends `+0000` offsets, which WebKit's `Date.parse` rejects.
fn normalize_jira_time(value: &str) -> String {
    let bytes = value.as_bytes();
    let len = bytes.len();
    if len > 5
        && matches!(bytes[len - 5], b'+' | b'-')
        && bytes[len - 4..].iter().all(u8::is_ascii_digit)
    {
        return format!("{}:{}", &value[..len - 2], &value[len - 2..]);
    }
    value.to_string()
}

fn require_issue_key(key: &str) -> Result<&str, String> {
    let key = key.trim();
    let valid = !key.is_empty()
        && key.len() < 64
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
    if valid {
        Ok(key)
    } else {
        Err("Missing Jira issue".into())
    }
}

fn person_fields(node: Option<&Value>) -> (Option<String>, String) {
    let Some(node) = node.filter(|node| node.is_object()) else {
        return (None, String::new());
    };
    (
        string_field(node, "displayName").filter(|name| !name.is_empty()),
        node.pointer("/avatarUrls/48x48")
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

// ---- Atlassian Document Format ----

/// Descriptions and comments arrive as ADF (v3) or plain text; the Inbox renders markdown.
fn rich_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(doc @ Value::Object(_)) => adf_to_markdown(doc),
        _ => String::new(),
    }
}

fn adf_to_markdown(doc: &Value) -> String {
    let mut blocks = Vec::new();
    for node in children(doc) {
        let block = adf_block(node, "");
        if !block.trim().is_empty() {
            blocks.push(block);
        }
    }
    blocks.join("\n\n").trim().to_string()
}

fn children(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn node_type(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or_default()
}

fn attr<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    node.pointer(&format!("/attrs/{name}"))
}

fn attr_str<'a>(node: &'a Value, name: &str) -> &'a str {
    attr(node, name).and_then(Value::as_str).unwrap_or_default()
}

fn adf_block(node: &Value, indent: &str) -> String {
    match node_type(node) {
        "paragraph" => adf_inline(children(node)),
        "heading" => {
            let level = attr(node, "level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            format!("{} {}", "#".repeat(level), adf_inline(children(node)))
        }
        "bulletList" => adf_list(node, indent, None),
        "orderedList" => {
            let start = attr(node, "order").and_then(Value::as_u64).unwrap_or(1);
            adf_list(node, indent, Some(start))
        }
        "codeBlock" => {
            let code: String = children(node)
                .iter()
                .filter_map(|child| child.get("text").and_then(Value::as_str))
                .collect();
            format!("```{}\n{}\n```", attr_str(node, "language"), code)
        }
        "blockquote" | "panel" => adf_blocks(children(node), indent)
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "rule" => "---".into(),
        "table" => adf_table(node),
        "mediaSingle" | "mediaGroup" | "media" => String::new(),
        _ => {
            if children(node).is_empty() {
                adf_inline(std::slice::from_ref(node))
            } else {
                adf_blocks(children(node), indent)
            }
        }
    }
}

fn adf_blocks(nodes: &[Value], indent: &str) -> String {
    nodes
        .iter()
        .map(|node| adf_block(node, indent))
        .filter(|block| !block.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn adf_list(node: &Value, indent: &str, start: Option<u64>) -> String {
    let mut lines = Vec::new();
    for (index, item) in children(node).iter().enumerate() {
        let marker = match start {
            Some(start) => format!("{}. ", start + index as u64),
            None => "- ".into(),
        };
        let nested = format!("{indent}{}", " ".repeat(marker.len()));
        let mut first = true;
        for child in children(item) {
            let text = adf_block(child, &nested);
            if text.trim().is_empty() {
                continue;
            }
            if matches!(node_type(child), "bulletList" | "orderedList") {
                lines.push(text);
            } else if first {
                lines.push(format!(
                    "{indent}{marker}{}",
                    text.replace('\n', &format!("\n{nested}"))
                ));
                first = false;
            } else {
                lines.push(format!(
                    "{nested}{}",
                    text.replace('\n', &format!("\n{nested}"))
                ));
            }
        }
    }
    lines.join("\n")
}

fn adf_table(node: &Value) -> String {
    let rows: Vec<Vec<String>> = children(node)
        .iter()
        .map(|row| {
            children(row)
                .iter()
                .map(|cell| {
                    adf_blocks(children(cell), "")
                        .replace('\n', " ")
                        .replace('|', "\\|")
                })
                .collect()
        })
        .filter(|row: &Vec<String>| !row.is_empty())
        .collect();
    let Some(width) = rows.iter().map(Vec::len).max() else {
        return String::new();
    };
    let line = |cells: &[String]| {
        let mut padded = cells.to_vec();
        padded.resize(width, String::new());
        format!("| {} |", padded.join(" | "))
    };
    let mut lines = vec![line(&rows[0]), format!("|{}", " --- |".repeat(width))];
    lines.extend(rows[1..].iter().map(|row| line(row)));
    lines.join("\n")
}

fn adf_inline(nodes: &[Value]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node_type(node) {
            "text" => out.push_str(&adf_marked_text(node)),
            "hardBreak" => out.push('\n'),
            "mention" => {
                let text = attr_str(node, "text");
                if text.starts_with('@') {
                    out.push_str(text);
                } else if !text.is_empty() {
                    out.push('@');
                    out.push_str(text);
                }
            }
            "emoji" => {
                let text = attr_str(node, "text");
                out.push_str(if text.is_empty() {
                    attr_str(node, "shortName")
                } else {
                    text
                });
            }
            "inlineCard" | "blockCard" | "embedCard" => {
                let url = attr_str(node, "url");
                if !url.is_empty() {
                    out.push_str(&format!("<{url}>"));
                }
            }
            "status" => out.push_str(&format!("`{}`", attr_str(node, "text"))),
            "date" => {}
            _ => out.push_str(&adf_inline(children(node))),
        }
    }
    out
}

fn adf_marked_text(node: &Value) -> String {
    let mut text = node
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if text.is_empty() {
        return text;
    }
    let marks = node
        .get("marks")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if marks.iter().any(|mark| node_type(mark) == "code") {
        return format!("`{text}`");
    }
    for mark in marks {
        text = match node_type(mark) {
            "strong" => format!("**{text}**"),
            "em" => format!("_{text}_"),
            "strike" => format!("~~{text}~~"),
            "link" => {
                let href = attr_str(mark, "href");
                if href.is_empty() {
                    text
                } else {
                    format!("[{text}]({href})")
                }
            }
            _ => text,
        };
    }
    text
}

/// Posts plain text as ADF: blank lines split paragraphs, single newlines become hard breaks.
fn text_to_adf(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .replace("\r\n", "\n")
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| {
            let mut content = Vec::new();
            for (index, line) in paragraph.lines().enumerate() {
                if index > 0 {
                    content.push(json!({ "type": "hardBreak" }));
                }
                if !line.is_empty() {
                    content.push(json!({ "type": "text", "text": line }));
                }
            }
            json!({ "type": "paragraph", "content": content })
        })
        .collect();
    json!({ "type": "doc", "version": 1, "content": paragraphs })
}

// ---- Config storage ----

fn normalize_jira_site(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("Enter your Jira site (e.g. yourteam.atlassian.net)".into());
    }
    // Bare `yourteam` becomes the Atlassian Cloud site.
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else if raw.contains('.') {
        format!("https://{raw}")
    } else {
        format!("https://{raw}.atlassian.net")
    };
    let url = url::Url::parse(&with_scheme).map_err(|_| "Jira site is invalid".to_string())?;
    // The API token travels on every request as Basic auth.
    if url.scheme() != "https" {
        return Err("Jira site must use HTTPS".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Jira site is invalid".into());
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| "Jira site is invalid".to_string())?;
    Ok(match url.port() {
        Some(port) => format!("https://{}:{port}", host.to_ascii_lowercase()),
        None => format!("https://{}", host.to_ascii_lowercase()),
    })
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("jira-config.json"))
}

fn read_config(app: &AppHandle) -> Result<Option<JiraConfig>, String> {
    let path = config_path(app)?;
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut config: JiraConfig =
                serde_json::from_str(&raw).map_err(|_| "Jira settings are invalid".to_string())?;
            config.site = normalize_jira_site(&config.site)?;
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

fn require_config(app: &AppHandle) -> Result<JiraConfig, String> {
    read_config(app)?.ok_or_else(|| "Connect Jira in Settings".to_string())
}

fn write_config(app: &AppHandle, config: &JiraConfig) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://acme.atlassian.net";

    #[test]
    fn fetches_every_project_page() {
        let mut offsets = Vec::new();
        let projects = fetch_jira_projects(|start| {
            offsets.push(start);
            Ok(if start == 0 {
                json!({ "startAt": 0, "isLast": false, "values": [
                    { "id": "10000", "key": "ENG", "name": "Engineering" }
                ] })
            } else {
                json!({ "startAt": 1, "isLast": true, "values": [
                    { "id": "10001", "key": "OPS", "name": "Operations" }
                ] })
            })
        })
        .unwrap();
        assert_eq!(offsets, vec![0, 1]);
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[1].key, "OPS");
    }

    #[test]
    fn rejects_stalled_project_pagination() {
        assert!(fetch_jira_projects(|_| Ok(json!({
            "isLast": false, "values": []
        })))
        .is_err());
    }

    #[test]
    fn normalizes_jira_sites() {
        assert_eq!(normalize_jira_site("acme").unwrap(), SITE);
        assert_eq!(normalize_jira_site("acme.atlassian.net").unwrap(), SITE);
        assert_eq!(
            normalize_jira_site(" https://Acme.atlassian.net/jira/software/projects ").unwrap(),
            SITE
        );
        assert_eq!(
            normalize_jira_site("jira.example.com:8443").unwrap(),
            "https://jira.example.com:8443"
        );
        assert!(normalize_jira_site("http://acme.atlassian.net").is_err());
        assert!(normalize_jira_site("https://user@acme.atlassian.net").is_err());
        assert!(normalize_jira_site("").is_err());
    }

    #[test]
    fn authorization_is_basic_email_and_token() {
        let config = JiraConfig {
            site: SITE.into(),
            email: "ada@acme.com".into(),
            token: "secret".into(),
        };
        assert_eq!(
            jira_authorization(&config),
            "Basic YWRhQGFjbWUuY29tOnNlY3JldA=="
        );
    }

    #[test]
    fn issue_jql_combines_filters() {
        assert_eq!(
            issue_jql(true, "open", &[" 10000 ".into(), "".into(), "10001".into(), "x) OR (1".into()]),
            "assignee = currentUser() AND project in (10000, 10001) AND statusCategory != Done ORDER BY updated DESC"
        );
        assert_eq!(
            issue_jql(false, "all", &[]),
            "updated >= -365d ORDER BY updated DESC"
        );
    }

    #[test]
    fn parse_jira_projects_reads_values() {
        let data = json!({
            "values": [
                { "id": "10000", "key": "ENG", "name": "Engineering" },
                { "id": "", "key": "SKIP", "name": "Skip" }
            ]
        });
        assert_eq!(
            parse_jira_projects(&data).unwrap(),
            vec![JiraProject {
                id: "10000".into(),
                key: "ENG".into(),
                name: "Engineering".into(),
            }]
        );
    }

    #[test]
    fn parse_jira_issues_maps_fields() {
        let data = json!({
            "issues": [{
                "id": "10042",
                "key": "ENG-42",
                "fields": {
                    "summary": "Fix auth",
                    "updated": "2026-08-27T10:00:00.000+0000",
                    "status": { "name": "In Progress", "statusCategory": { "key": "indeterminate" } },
                    "labels": ["bug", "backend"],
                    "assignee": {
                        "displayName": "Maya",
                        "avatarUrls": { "48x48": "https://avatar.example/maya.png" }
                    },
                    "project": { "id": "10000", "key": "ENG", "name": "Engineering" }
                }
            }, { "id": "10043" }]
        });
        let items = parse_jira_issues(&data, SITE).unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.provider, "jira");
        assert_eq!(item.kind, "jira");
        assert_eq!(item.id, "10042");
        assert_eq!(item.identifier, "ENG-42");
        assert_eq!(item.number, 42);
        assert_eq!(item.url, "https://acme.atlassian.net/browse/ENG-42");
        assert_eq!(item.state, "In Progress");
        assert_eq!(item.state_type, "indeterminate");
        assert_eq!(item.updated_at, "2026-08-27T10:00:00.000+00:00");
        assert_eq!(item.labels[1].name, "backend");
        assert_eq!(item.assignees[0].login, "Maya");
        assert_eq!(
            item.assignees[0].avatar_url,
            "https://avatar.example/maya.png"
        );
        assert_eq!(item.repo, "ENG");
        assert_eq!(item.team_id, "10000");
        assert_eq!(item.team_name, "Engineering");
    }

    #[test]
    fn parse_jira_issue_details_prefers_reporter() {
        let data = json!({
            "fields": {
                "description": {
                    "type": "doc",
                    "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "Steps" }] }]
                },
                "reporter": { "displayName": "Ada", "avatarUrls": { "48x48": "https://avatar.example/ada.png" } },
                "creator": { "displayName": "Lin" },
                "assignee": null
            }
        });
        let details = parse_jira_issue_details(&data).unwrap();
        assert_eq!(details.body, "Steps");
        assert_eq!(details.author, "Ada");
        assert_eq!(details.author_avatar_url, "https://avatar.example/ada.png");
    }

    #[test]
    fn parse_jira_issue_thread_sorts_oldest_first() {
        let data = json!({
            "total": 3,
            "comments": [
                {
                    "id": "2",
                    "author": { "displayName": "Maya" },
                    "body": "Plain reply",
                    "created": "2026-08-31T11:00:00.000+0000"
                },
                {
                    "id": "1",
                    "author": { "displayName": "Ada" },
                    "body": { "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "Root" }] }] },
                    "created": "2026-08-31T10:00:00.000+0000"
                }
            ]
        });
        let thread = parse_jira_issue_thread(&data, SITE, "ENG-42").unwrap();
        assert!(thread.truncated);
        assert_eq!(thread.comments.len(), 2);
        assert_eq!(thread.comments[0].body, "Root");
        assert_eq!(
            thread.comments[0].url,
            "https://acme.atlassian.net/browse/ENG-42?focusedCommentId=1"
        );
        assert_eq!(thread.comments[1].author, "Maya");
        assert_eq!(thread.comments[1].body, "Plain reply");
    }

    #[test]
    fn adf_converts_to_markdown() {
        let doc = json!({
            "type": "doc",
            "version": 1,
            "content": [
                { "type": "heading", "attrs": { "level": 2 }, "content": [{ "type": "text", "text": "Repro" }] },
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "Ping " },
                    { "type": "mention", "attrs": { "text": "@Maya" } },
                    { "type": "text", "text": " see " },
                    { "type": "text", "text": "docs", "marks": [{ "type": "link", "attrs": { "href": "https://example.com" } }] },
                    { "type": "hardBreak" },
                    { "type": "text", "text": "now", "marks": [{ "type": "strong" }] },
                    { "type": "text", "text": " run " },
                    { "type": "text", "text": "make", "marks": [{ "type": "code" }, { "type": "strong" }] }
                ]},
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "one" }] },
                        { "type": "orderedList", "content": [
                            { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "nested" }] }] }
                        ]}
                    ]},
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "two" }] }] }
                ]},
                { "type": "codeBlock", "attrs": { "language": "sh" }, "content": [{ "type": "text", "text": "npm test" }] },
                { "type": "blockquote", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "quoted" }] }] },
                { "type": "mediaSingle", "content": [{ "type": "media", "attrs": { "id": "x" } }] }
            ]
        });
        assert_eq!(
            adf_to_markdown(&doc),
            "## Repro\n\nPing @Maya see [docs](https://example.com)\n**now** run `make`\n\n- one\n  1. nested\n- two\n\n```sh\nnpm test\n```\n\n> quoted"
        );
    }

    #[test]
    fn adf_renders_tables() {
        let cell = |text: &str| json!({ "type": "tableCell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] });
        let doc = json!({
            "type": "doc",
            "content": [{ "type": "table", "content": [
                { "type": "tableRow", "content": [cell("a"), cell("b")] },
                { "type": "tableRow", "content": [cell("1"), cell("2|3")] }
            ]}]
        });
        assert_eq!(
            adf_to_markdown(&doc),
            "| a | b |\n| --- | --- |\n| 1 | 2\\|3 |"
        );
    }

    #[test]
    fn text_to_adf_splits_paragraphs_and_lines() {
        assert_eq!(
            text_to_adf("First line\nsecond\n\nNext"),
            json!({
                "type": "doc",
                "version": 1,
                "content": [
                    { "type": "paragraph", "content": [
                        { "type": "text", "text": "First line" },
                        { "type": "hardBreak" },
                        { "type": "text", "text": "second" }
                    ]},
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Next" }] }
                ]
            })
        );
    }

    #[test]
    fn normalizes_jira_offsets() {
        assert_eq!(
            normalize_jira_time("2026-08-27T10:00:00.000+0200"),
            "2026-08-27T10:00:00.000+02:00"
        );
        assert_eq!(
            normalize_jira_time("2026-08-27T10:00:00.000Z"),
            "2026-08-27T10:00:00.000Z"
        );
    }

    #[test]
    fn issue_keys_are_validated() {
        assert_eq!(require_issue_key(" ENG-42 ").unwrap(), "ENG-42");
        assert!(require_issue_key("").is_err());
        assert!(require_issue_key("ENG-42/../x").is_err());
    }

    #[test]
    fn jira_error_message_reads_both_shapes() {
        assert_eq!(
            jira_error_message(r#"{"errorMessages":["Issue does not exist"],"errors":{}}"#)
                .as_deref(),
            Some("Issue does not exist")
        );
        assert_eq!(
            jira_error_message(r#"{"errorMessages":[],"errors":{"jql":"Bad JQL"}}"#).as_deref(),
            Some("Bad JQL")
        );
    }
}
