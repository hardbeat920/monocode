use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const LINEAR_API: &str = "https://api.linear.app/graphql";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_LIMIT: u32 = 40;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearStatus {
    pub connected: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearTeam {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearLabel {
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearAssignee {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssue {
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
    pub labels: Vec<LinearLabel>,
    pub assignees: Vec<LinearAssignee>,
    pub draft: bool,
    pub repo: String,
    pub team_id: String,
    pub team_name: String,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssueDetails {
    pub body: String,
    pub author: String,
    pub author_avatar_url: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssueComment {
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
    pub replies: Vec<LinearIssueComment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinearIssueThread {
    pub comments: Vec<LinearIssueComment>,
    pub truncated: bool,
    pub review_decision: String,
    pub base_ref_name: String,
    pub head_ref_name: String,
}

#[tauri::command(async)]
pub fn linear_status(app: AppHandle) -> Result<LinearStatus, String> {
    Ok(LinearStatus {
        connected: read_token(&app)?.is_some(),
    })
}

#[tauri::command]
pub async fn linear_set_token(app: AppHandle, token: String) -> Result<LinearStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = token.trim().to_string();
        if trimmed.is_empty() {
            delete_token(&app)?;
            return Ok(LinearStatus { connected: false });
        }
        graphql_with_token(&trimmed, VIEWER_QUERY, json!({}))?;
        write_token(&app, &trimmed)?;
        Ok(LinearStatus { connected: true })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_list_teams(app: AppHandle) -> Result<Vec<LinearTeam>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let data = graphql_with_token(&token, TEAMS_QUERY, json!({}))?;
        parse_linear_teams(&data)
    })
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
    tauri::async_runtime::spawn_blocking(move || {
        let Some(token) = read_token(&app)? else {
            return Ok(Vec::new());
        };
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100);
        let filter = issue_filter(assigned_to_me, &state, &team_ids);
        let data = graphql_with_token(
            &token,
            ISSUES_QUERY,
            json!({ "first": limit, "filter": filter }),
        )?;
        parse_linear_issues(&data)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_issue_details(
    app: AppHandle,
    id: String,
) -> Result<LinearIssueDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let id = id.trim();
        if id.is_empty() {
            return Err("Missing Linear issue".into());
        }
        let data = graphql_with_token(&token, ISSUE_QUERY, json!({ "id": id }))?;
        parse_linear_issue_details(&data)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn linear_issue_thread(app: AppHandle, id: String) -> Result<LinearIssueThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let id = id.trim();
        if !valid_linear_id(id) {
            return Err("Missing Linear issue".into());
        }
        let data = graphql_with_token(&token, ISSUE_COMMENTS_QUERY, json!({ "id": id }))?;
        parse_linear_issue_thread(&data)
    })
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
    tauri::async_runtime::spawn_blocking(move || {
        let token = require_token(&app)?;
        let id = id.trim();
        if !valid_linear_id(id) {
            return Err("Missing Linear issue".into());
        }
        let body = body.trim();
        if body.is_empty() {
            return Err("Comment cannot be empty".into());
        }
        let parent = parent_id.trim();
        if !parent.is_empty() && !valid_linear_id(parent) {
            return Err("Invalid Linear comment".into());
        }
        let mut input = serde_json::Map::new();
        input.insert("issueId".into(), json!(id));
        input.insert("body".into(), json!(body));
        if !parent.is_empty() {
            input.insert("parentId".into(), json!(parent));
        }
        let data = graphql_with_token(
            &token,
            COMMENT_CREATE_MUTATION,
            json!({ "input": Value::Object(input) }),
        )?;
        parse_linear_comment_create(&data)
    })
    .await
    .map_err(|error| error.to_string())?
}

const VIEWER_QUERY: &str = "query { viewer { id } }";
const TEAMS_QUERY: &str = r#"
query {
  teams(first: 50) {
    nodes { id key name }
  }
}
"#;
const ISSUES_QUERY: &str = r#"
query InboxIssues($first: Int!, $filter: IssueFilter) {
  issues(first: $first, filter: $filter, orderBy: updatedAt) {
    nodes {
      id
      identifier
      number
      title
      url
      updatedAt
      state { name type }
      team { id key name }
      project { id name }
      labels { nodes { name color } }
      assignee { name displayName avatarUrl }
    }
  }
}
"#;
const ISSUE_QUERY: &str = r#"
query InboxIssue($id: String!) {
  issue(id: $id) {
    description
    creator { name displayName avatarUrl }
    assignee { name displayName avatarUrl }
  }
}
"#;
const ISSUE_COMMENTS_QUERY: &str = r#"
query InboxIssueComments($id: String!) {
  issue(id: $id) {
    comments(first: 50) {
      pageInfo { hasNextPage }
      nodes {
        id
        body
        createdAt
        url
        user { name displayName avatarUrl }
        parent { id }
      }
    }
  }
}
"#;
const COMMENT_CREATE_MUTATION: &str = r#"
mutation InboxCommentCreate($input: CommentCreateInput!) {
  commentCreate(input: $input) {
    success
    comment { id url }
  }
}
"#;

fn issue_filter(assigned_to_me: bool, state: &str, team_ids: &[String]) -> Value {
    let mut filter = serde_json::Map::new();
    if assigned_to_me {
        filter.insert("assignee".into(), json!({ "isMe": { "eq": true } }));
    }
    let ids: Vec<String> = team_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    if !ids.is_empty() {
        filter.insert("team".into(), json!({ "id": { "in": ids } }));
    }
    if !state.trim().eq_ignore_ascii_case("all") {
        filter.insert(
            "state".into(),
            json!({ "type": { "nin": ["completed", "canceled"] } }),
        );
    }
    Value::Object(filter)
}

fn linear_authorization(token: &str) -> String {
    let trimmed = token.trim();
    trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn graphql_with_token(token: &str, query: &str, variables: Value) -> Result<Value, String> {
    let authorization = linear_authorization(token);
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let payload = serde_json::to_string(&json!({ "query": query, "variables": variables }))
        .map_err(|error| error.to_string())?;
    let result = agent
        .post(LINEAR_API)
        .set("Authorization", &authorization)
        .set("Content-Type", "application/json")
        .send_string(&payload);
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            return Err("Linear API key is invalid".into());
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(linear_http_error(status, &body));
        }
        Err(_) => return Err("Could not reach Linear".into()),
    };
    let status = response.status();
    let body = response
        .into_string()
        .map_err(|_| "Linear returned an unreadable response".to_string())?;
    if status == 401 || status == 403 {
        return Err("Linear API key is invalid".into());
    }
    if !(200..300).contains(&status) {
        return Err(linear_http_error(status, &body));
    }
    parse_graphql_data(&body)
}

fn linear_http_error(status: u16, body: &str) -> String {
    graphql_error_message(body).unwrap_or_else(|| format!("Linear request failed ({status})"))
}

fn parse_graphql_data(body: &str) -> Result<Value, String> {
    let parsed: Value =
        serde_json::from_str(body).map_err(|_| "Linear returned invalid JSON".to_string())?;
    if let Some(message) = graphql_error_message_from_value(&parsed) {
        if message.to_ascii_lowercase().contains("auth")
            || message.to_ascii_lowercase().contains("unauthor")
        {
            return Err("Linear API key is invalid".into());
        }
        return Err(message);
    }
    parsed
        .get("data")
        .cloned()
        .ok_or_else(|| "Linear returned no data".to_string())
}

fn graphql_error_message(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    graphql_error_message_from_value(&parsed)
}

fn graphql_error_message_from_value(parsed: &Value) -> Option<String> {
    parsed
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(|message| message.trim().to_string())
        .filter(|message| !message.is_empty())
}

fn parse_linear_teams(data: &Value) -> Result<Vec<LinearTeam>, String> {
    let nodes = data
        .pointer("/teams/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "Linear did not return teams".to_string())?;
    Ok(nodes
        .iter()
        .filter_map(|node| {
            let id = string_field(node, "id")?;
            let key = string_field(node, "key").unwrap_or_default();
            let name = string_field(node, "name").unwrap_or_else(|| key.clone());
            if id.is_empty() {
                return None;
            }
            Some(LinearTeam { id, key, name })
        })
        .collect())
}

fn parse_linear_issues(data: &Value) -> Result<Vec<LinearIssue>, String> {
    let nodes = data
        .pointer("/issues/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "Linear did not return issues".to_string())?;
    Ok(nodes.iter().filter_map(parse_linear_issue).collect())
}

fn parse_linear_issue(node: &Value) -> Option<LinearIssue> {
    let id = string_field(node, "id")?;
    if id.is_empty() {
        return None;
    }
    let identifier = string_field(node, "identifier").unwrap_or_default();
    let number = node.get("number").and_then(Value::as_i64).unwrap_or(0);
    let team = node.get("team");
    let team_id = team
        .and_then(|value| string_field(value, "id"))
        .unwrap_or_default();
    let team_key = team
        .and_then(|value| string_field(value, "key"))
        .unwrap_or_default();
    let team_name = team
        .and_then(|value| string_field(value, "name"))
        .unwrap_or_else(|| team_key.clone());
    let project = node.get("project");
    let project_id = project
        .and_then(|value| string_field(value, "id"))
        .unwrap_or_default();
    let project_name = project
        .and_then(|value| string_field(value, "name"))
        .unwrap_or_default();
    let state = node.get("state");
    Some(LinearIssue {
        provider: "linear".into(),
        kind: "linear".into(),
        id,
        identifier,
        number,
        title: string_field(node, "title").unwrap_or_default(),
        url: string_field(node, "url").unwrap_or_default(),
        state: state
            .and_then(|value| string_field(value, "name"))
            .unwrap_or_else(|| "Open".into()),
        state_type: state
            .and_then(|value| string_field(value, "type"))
            .unwrap_or_default(),
        updated_at: string_field(node, "updatedAt").unwrap_or_default(),
        labels: parse_labels(node),
        assignees: parse_assignees(node),
        draft: false,
        repo: team_key,
        team_id,
        team_name,
        project_id,
        project_name,
        project_path: String::new(),
    })
}

fn parse_linear_issue_details(data: &Value) -> Result<LinearIssueDetails, String> {
    let issue = data
        .get("issue")
        .ok_or_else(|| "Linear did not return that issue".to_string())?;
    let creator = person_fields(issue.get("creator"));
    let assignee = person_fields(issue.get("assignee"));
    let (author, author_avatar_url) = if creator.0.is_some() {
        creator
    } else {
        assignee
    };
    Ok(LinearIssueDetails {
        body: string_field(issue, "description").unwrap_or_default(),
        author: author.unwrap_or_default(),
        author_avatar_url,
    })
}

fn parse_linear_issue_thread(data: &Value) -> Result<LinearIssueThread, String> {
    let issue = data
        .get("issue")
        .ok_or_else(|| "Linear did not return that issue".to_string())?;
    let comments_field = issue
        .get("comments")
        .ok_or_else(|| "Linear did not return comments".to_string())?;
    let truncated = comments_field
        .pointer("/pageInfo/hasNextPage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let nodes = comments_field
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "Linear did not return comments".to_string())?;
    let rows: Vec<ParsedLinearComment> = nodes.iter().filter_map(parse_linear_comment).collect();
    Ok(LinearIssueThread {
        comments: nest_linear_comments(rows),
        truncated,
        review_decision: String::new(),
        base_ref_name: String::new(),
        head_ref_name: String::new(),
    })
}

struct ParsedLinearComment {
    id: String,
    parent_id: String,
    comment: LinearIssueComment,
}

fn parse_linear_comment(node: &Value) -> Option<ParsedLinearComment> {
    let id = string_field(node, "id").filter(|value| !value.is_empty())?;
    let (author, author_avatar_url) = person_fields(node.get("user"));
    let parent_id = node
        .get("parent")
        .and_then(|parent| string_field(parent, "id"))
        .unwrap_or_default();
    Some(ParsedLinearComment {
        id: id.clone(),
        parent_id,
        comment: LinearIssueComment {
            id,
            kind: "comment".into(),
            author: author.unwrap_or_default(),
            author_avatar_url,
            body: string_field(node, "body").unwrap_or_default(),
            created_at: string_field(node, "createdAt").unwrap_or_default(),
            url: string_field(node, "url").unwrap_or_default(),
            state: String::new(),
            path: String::new(),
            line: None,
            resolved: false,
            thread_id: String::new(),
            replies: Vec::new(),
        },
    })
}

fn nest_linear_comments(mut rows: Vec<ParsedLinearComment>) -> Vec<LinearIssueComment> {
    rows.sort_by(|left, right| left.comment.created_at.cmp(&right.comment.created_at));
    let ids: HashSet<String> = rows.iter().map(|row| row.id.clone()).collect();
    let mut parent_of = HashMap::new();
    for row in &rows {
        if !row.parent_id.is_empty() && ids.contains(&row.parent_id) {
            parent_of.insert(row.id.clone(), row.parent_id.clone());
        }
    }
    let mut top = Vec::new();
    let mut replies: HashMap<String, Vec<LinearIssueComment>> = HashMap::new();
    for row in rows {
        let root = linear_comment_root(&row.id, &parent_of);
        if root == row.id {
            top.push(row.comment);
        } else {
            replies.entry(root).or_default().push(row.comment);
        }
    }
    for comment in &mut top {
        if let Some(children) = replies.remove(&comment.id) {
            comment.replies = children;
        }
    }
    top
}

fn linear_comment_root(id: &str, parent_of: &HashMap<String, String>) -> String {
    let mut current = id.to_string();
    let mut seen = HashSet::new();
    while let Some(parent) = parent_of.get(&current) {
        if !seen.insert(current.clone()) {
            break;
        }
        current = parent.clone();
    }
    current
}

fn parse_linear_comment_create(data: &Value) -> Result<String, String> {
    let payload = data
        .get("commentCreate")
        .ok_or_else(|| "Linear did not return a comment".to_string())?;
    let success = payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !success {
        return Err("Could not post Linear comment".into());
    }
    Ok(payload
        .get("comment")
        .and_then(|comment| string_field(comment, "url"))
        .unwrap_or_default())
}

fn valid_linear_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id.len() < 128
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn parse_labels(node: &Value) -> Vec<LinearLabel> {
    node.pointer("/labels/nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|label| {
                    let name = string_field(label, "name")?;
                    let color = string_field(label, "color")
                        .unwrap_or_default()
                        .trim_start_matches('#')
                        .to_string();
                    Some(LinearLabel { name, color })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_assignees(node: &Value) -> Vec<LinearAssignee> {
    let (name, avatar_url) = person_fields(node.get("assignee"));
    name.map(|login| vec![LinearAssignee { login, avatar_url }])
        .unwrap_or_default()
}

fn person_fields(node: Option<&Value>) -> (Option<String>, String) {
    let Some(node) = node else {
        return (None, String::new());
    };
    (
        display_name(Some(node)),
        string_field(node, "avatarUrl").unwrap_or_default(),
    )
}

fn display_name(node: Option<&Value>) -> Option<String> {
    let node = node?;
    string_field(node, "displayName")
        .filter(|value| !value.is_empty())
        .or_else(|| string_field(node, "name"))
        .filter(|value| !value.is_empty())
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
}

fn token_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("linear-token"))
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
    read_token(app)?.ok_or_else(|| "Connect Linear in Settings".to_string())
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

    #[test]
    fn linear_authorization_sends_the_raw_api_key() {
        assert_eq!(linear_authorization(" lin_api_abc "), "lin_api_abc");
        assert_eq!(linear_authorization("Bearer lin_api_abc"), "lin_api_abc");
    }

    #[test]
    fn issue_filter_assigned_open_teams() {
        let filter = issue_filter(
            true,
            "open",
            &[" team-1 ".into(), "".into(), "team-2".into()],
        );
        assert_eq!(
            filter,
            json!({
                "assignee": { "isMe": { "eq": true } },
                "team": { "id": { "in": ["team-1", "team-2"] } },
                "state": { "type": { "nin": ["completed", "canceled"] } }
            })
        );
    }

    #[test]
    fn issue_filter_all_states_no_team() {
        let filter = issue_filter(false, "all", &[]);
        assert_eq!(filter, json!({}));
    }

    #[test]
    fn parse_linear_teams_reads_nodes() {
        let data = json!({
            "teams": {
                "nodes": [
                    { "id": "t1", "key": "ENG", "name": "Engineering" },
                    { "id": "", "key": "SKIP", "name": "Skip" }
                ]
            }
        });
        let teams = parse_linear_teams(&data).unwrap();
        assert_eq!(
            teams,
            vec![LinearTeam {
                id: "t1".into(),
                key: "ENG".into(),
                name: "Engineering".into(),
            }]
        );
    }

    #[test]
    fn parse_linear_issues_maps_fields() {
        let data = json!({
            "issues": {
                "nodes": [{
                    "id": "issue-1",
                    "identifier": "ENG-9",
                    "number": 9,
                    "title": "Fix auth",
                    "url": "https://linear.app/acme/issue/ENG-9",
                    "updatedAt": "2026-08-27T10:00:00.000Z",
                    "state": { "name": "In Progress", "type": "started" },
                    "team": { "id": "t1", "key": "ENG", "name": "Engineering" },
                    "project": { "id": "p1", "name": "Billing" },
                    "labels": { "nodes": [{ "name": "bug", "color": "#eb5757" }] },
                    "assignee": { "displayName": "Maya", "name": "maya", "avatarUrl": "https://uploads.linear.app/maya.png" }
                }]
            }
        });
        let items = parse_linear_issues(&data).unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.provider, "linear");
        assert_eq!(item.kind, "linear");
        assert_eq!(item.identifier, "ENG-9");
        assert_eq!(item.number, 9);
        assert_eq!(item.state, "In Progress");
        assert_eq!(item.state_type, "started");
        assert_eq!(item.repo, "ENG");
        assert_eq!(item.team_id, "t1");
        assert_eq!(item.team_name, "Engineering");
        assert_eq!(item.project_id, "p1");
        assert_eq!(item.project_name, "Billing");
        assert_eq!(item.labels[0].color, "eb5757");
        assert_eq!(item.assignees[0].login, "Maya");
        assert_eq!(
            item.assignees[0].avatar_url,
            "https://uploads.linear.app/maya.png"
        );
        assert!(item.project_path.is_empty());
    }

    #[test]
    fn parse_linear_issues_leaves_project_empty_when_unassigned() {
        let data = json!({
            "issues": {
                "nodes": [{
                    "id": "issue-2",
                    "identifier": "ENG-10",
                    "number": 10,
                    "title": "Loose issue",
                    "team": { "id": "t1", "key": "ENG", "name": "Engineering" },
                    "project": Value::Null
                }]
            }
        });
        let items = parse_linear_issues(&data).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].project_id.is_empty());
        assert!(items[0].project_name.is_empty());
    }

    #[test]
    fn parse_linear_issue_details_prefers_creator() {
        let data = json!({
            "issue": {
                "description": "Steps to reproduce",
                "creator": { "name": "Ada", "avatarUrl": "https://uploads.linear.app/ada.png" },
                "assignee": { "displayName": "Maya" }
            }
        });
        let details = parse_linear_issue_details(&data).unwrap();
        assert_eq!(details.body, "Steps to reproduce");
        assert_eq!(details.author, "Ada");
        assert_eq!(
            details.author_avatar_url,
            "https://uploads.linear.app/ada.png"
        );
    }

    #[test]
    fn parse_linear_issue_thread_nests_replies() {
        let data = json!({
            "issue": {
                "comments": {
                    "pageInfo": { "hasNextPage": true },
                    "nodes": [
                        {
                            "id": "c2",
                            "body": "Reply",
                            "createdAt": "2026-08-31T11:00:00.000Z",
                            "url": "https://linear.app/acme/issue/ENG-9#comment-c2",
                            "user": { "displayName": "Maya" },
                            "parent": { "id": "c1" }
                        },
                        {
                            "id": "c1",
                            "body": "Root",
                            "createdAt": "2026-08-31T10:00:00.000Z",
                            "url": "https://linear.app/acme/issue/ENG-9#comment-c1",
                            "user": { "name": "Ada", "avatarUrl": "https://uploads.linear.app/ada.png" }
                        },
                        {
                            "id": "c3",
                            "body": "Nested reply",
                            "createdAt": "2026-08-31T11:30:00.000Z",
                            "user": { "displayName": "Lin" },
                            "parent": { "id": "c2" }
                        }
                    ]
                }
            }
        });
        let thread = parse_linear_issue_thread(&data).unwrap();
        assert!(thread.truncated);
        assert_eq!(thread.comments.len(), 1);
        assert_eq!(thread.comments[0].author, "Ada");
        assert_eq!(thread.comments[0].body, "Root");
        assert_eq!(thread.comments[0].replies.len(), 2);
        assert_eq!(thread.comments[0].replies[0].body, "Reply");
        assert_eq!(thread.comments[0].replies[1].body, "Nested reply");
    }

    #[test]
    fn parse_linear_comment_create_reads_url() {
        let data = json!({
            "commentCreate": {
                "success": true,
                "comment": { "id": "c9", "url": "https://linear.app/acme/issue/ENG-9#comment-c9" }
            }
        });
        assert_eq!(
            parse_linear_comment_create(&data).unwrap(),
            "https://linear.app/acme/issue/ENG-9#comment-c9"
        );
        assert!(
            parse_linear_comment_create(&json!({ "commentCreate": { "success": false } })).is_err()
        );
    }

    #[test]
    fn valid_linear_id_allows_uuids() {
        assert!(valid_linear_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(!valid_linear_id(""));
        assert!(!valid_linear_id("id with space"));
    }

    #[test]
    fn graphql_error_message_reads_first_error() {
        let body = r#"{"errors":[{"message":"Invalid token"}]}"#;
        assert_eq!(
            graphql_error_message(body).as_deref(),
            Some("Invalid token")
        );
    }
}
