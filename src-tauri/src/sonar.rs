use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct SonarConfig {
    site: String,
    token: String,
    /// Empty on SonarQube Server, which has no organizations.
    #[serde(default)]
    organization: String,
    #[serde(default)]
    projects: Vec<SonarProject>,
    #[serde(default)]
    selected_project: String,
}

impl SonarConfig {
    /// The inbox reads one project at a time. Fall back to the first when the choice was unwatched.
    fn selected(&self) -> Option<&SonarProject> {
        self.projects
            .iter()
            .find(|project| project.key == self.selected_project)
            .or_else(|| self.projects.first())
    }

    fn watches(&self, key: &str) -> bool {
        self.projects.iter().any(|project| project.key == key)
    }

    /// Projects belong to one organization, so switching clears them.
    fn set_organization(&mut self, organization: &str) {
        let organization = organization.trim();
        if self.organization != organization {
            self.organization = organization.into();
            self.projects.clear();
            self.selected_project.clear();
        }
    }

    fn set_projects(&mut self, projects: Vec<SonarProject>) {
        self.projects.clear();
        for project in projects {
            let key = project.key.trim();
            if !key.is_empty() && !self.watches(key) {
                let name = project.name.trim();
                self.projects.push(SonarProject {
                    key: key.into(),
                    name: if name.is_empty() {
                        key.into()
                    } else {
                        name.into()
                    },
                });
            }
        }
    }

    fn select(&mut self, key: String) -> Result<(), String> {
        if !self.watches(&key) {
            return Err("This Sonar project is not connected".into());
        }
        self.selected_project = key;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SonarProject {
    key: String,
    name: String,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SonarOrganization {
    key: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SonarStatus {
    connected: bool,
    site: String,
    organization: String,
    projects: Vec<SonarProject>,
    selected_project: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SonarBranch {
    project_key: String,
    repository_name: String,
    name: String,
    is_main: bool,
    analysis_date: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SonarIssue {
    key: String,
    repository_name: String,
    project_key: String,
    branch: String,
    message: String,
    severity: String,
    rule: String,
    issue_type: String,
    status: String,
    resolved: bool,
    file: String,
    line: Option<u64>,
    updated_at: String,
    assignee: String,
    url: String,
}

#[derive(Serialize, Default)]
pub struct SonarSnapshot {
    issues: Vec<SonarIssue>,
    branches: Vec<SonarBranch>,
    errors: Vec<String>,
}

#[tauri::command(async)]
pub fn sonar_status(app: AppHandle) -> Result<SonarStatus, String> {
    Ok(status_for(read_config(&app)?.as_ref()))
}

#[tauri::command]
pub async fn sonar_connect(
    app: AppHandle,
    site: String,
    token: String,
) -> Result<SonarStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err("Enter a Sonar personal access token".into());
        }
        if token.starts_with("sqco_") {
            return Err("Use a personal access token with Browse permission. Scoped organization tokens only grant Execute analysis".into());
        }
        let config = SonarConfig { site: normalize_site(&site)?, token, ..Default::default() };
        let validation = sonar_get(&config, "/api/authentication/validate", &[])?;
        if validation.get("valid").and_then(Value::as_bool) != Some(true) {
            return Err("Sonar token is invalid or expired".into());
        }
        let _lock = config_lock();
        write_config(&app, &config)?;
        Ok(status_for(Some(&config)))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn sonar_disconnect(app: AppHandle) -> Result<SonarStatus, String> {
    let _lock = config_lock();
    delete_config(&app)?;
    Ok(status_for(None))
}

/// Empty on SonarQube Server, which has no organizations.
#[tauri::command]
pub async fn sonar_list_organizations(app: AppHandle) -> Result<Vec<SonarOrganization>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = read_config(&app)?.ok_or("Connect Sonar in Settings")?;
        let raw = match sonar_text(
            &config,
            "/api/organizations/search",
            &[("member", "true"), ("ps", "500")],
        ) {
            Ok(raw) => raw,
            Err(SonarTextError::NotFound) => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let data: Value = serde_json::from_str(&raw).map_err(|_| "Sonar returned invalid JSON")?;
        Ok(parse_organizations(&data))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Every project the token can browse in the saved organization.
#[tauri::command]
pub async fn sonar_list_projects(app: AppHandle) -> Result<Vec<SonarProject>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = read_config(&app)?.ok_or("Connect Sonar in Settings")?;
        fetch_projects(&config.organization, |query| {
            sonar_get(&config, "/api/components/search_projects", query)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn sonar_set_organization(app: AppHandle, organization: String) -> Result<SonarStatus, String> {
    update_config(&app, |config| {
        config.set_organization(&organization);
        Ok(())
    })
}

#[tauri::command(async)]
pub fn sonar_set_projects(
    app: AppHandle,
    projects: Vec<SonarProject>,
) -> Result<SonarStatus, String> {
    update_config(&app, |config| {
        config.set_projects(projects);
        Ok(())
    })
}

#[tauri::command(async)]
pub fn sonar_select_project(app: AppHandle, project_key: String) -> Result<SonarStatus, String> {
    update_config(&app, |config| config.select(project_key))
}

#[tauri::command]
pub async fn sonar_list_issues(
    app: AppHandle,
    assigned_to_me: bool,
    state: String,
) -> Result<SonarSnapshot, String> {
    // Include time waiting for the blocking worker in the refresh budget.
    let deadline = Instant::now() + Duration::from_secs(60);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(config) = read_config(&app)? else {
            return Ok(SonarSnapshot::default());
        };
        let Some(project) = config.selected() else {
            return Ok(SonarSnapshot::default());
        };
        fetch_snapshot(
            &config,
            &project.key,
            assigned_to_me,
            &state,
            deadline,
            |path, query| sonar_get(&config, path, query),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn status_for(config: Option<&SonarConfig>) -> SonarStatus {
    SonarStatus {
        connected: config.is_some(),
        site: config.map(|c| c.site.clone()).unwrap_or_default(),
        organization: config.map(|c| c.organization.clone()).unwrap_or_default(),
        projects: config.map(|c| c.projects.clone()).unwrap_or_default(),
        selected_project: config
            .and_then(SonarConfig::selected)
            .map(|project| project.key.clone())
            .unwrap_or_default(),
    }
}

fn parse_organizations(data: &Value) -> Vec<SonarOrganization> {
    data.get("organizations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|node| {
            let key = field(node, "key");
            if key.is_empty() {
                return None;
            }
            let name = field(node, "name");
            Some(SonarOrganization {
                name: if name.is_empty() { key.clone() } else { name },
                key,
            })
        })
        .collect()
}

fn fetch_projects(
    organization: &str,
    mut get: impl FnMut(&[(&str, &str)]) -> Result<Value, String>,
) -> Result<Vec<SonarProject>, String> {
    let mut projects = Vec::new();
    for page in 1..=20 {
        let p = page.to_string();
        let mut query = vec![("ps", "500"), ("p", p.as_str())];
        if !organization.is_empty() {
            query.push(("organization", organization));
        }
        let data = get(&query)?;
        let nodes = data
            .get("components")
            .and_then(Value::as_array)
            .ok_or("Sonar did not return projects")?;
        projects.extend(nodes.iter().filter_map(|node| {
            let key = field(node, "key");
            if key.is_empty() {
                return None;
            }
            let name = field(node, "name");
            Some(SonarProject {
                name: if name.is_empty() { key.clone() } else { name },
                key,
            })
        }));
        let total = data
            .pointer("/paging/total")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if nodes.is_empty() || page * 500 >= total {
            break;
        }
    }
    projects.sort_by_key(|project| project.name.to_lowercase());
    Ok(projects)
}

fn fetch_snapshot(
    config: &SonarConfig,
    key: &str,
    assigned_to_me: bool,
    state: &str,
    deadline: Instant,
    get: impl Fn(&str, &[(&str, &str)]) -> Result<Value, String> + Sync,
) -> Result<SonarSnapshot, String> {
    // Stop starting requests after the shared refresh deadline.
    let bounded_get = |path: &str, query: &[(&str, &str)]| {
        if Instant::now() >= deadline {
            return Err("Sonar refresh time budget reached. Results may be incomplete".to_string());
        }
        get(path, query)
    };
    let mut snapshot = SonarSnapshot::default();
    let mut branches = match bounded_get("/api/project_branches/list", &[("project", key)])
        .and_then(|data| parse_branches(&data, key))
    {
        Ok(branches) => branches,
        Err(e) => {
            snapshot.errors.push(format!("{key}: {e}"));
            return Ok(snapshot);
        }
    };
    let repository_name = bounded_get("/api/project_links/search", &[("projectKey", key)])
        .ok()
        .and_then(|data| repository_name_from_links(&data))
        .or_else(|| {
            bounded_get("/api/components/show", &[("component", key)])
                .ok()
                .and_then(|data| {
                    data.pointer("/component/name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                })
        })
        .unwrap_or_else(|| key.to_string());
    for branch in &mut branches {
        branch.repository_name = repository_name.clone();
    }
    for batch in branches.chunks(4) {
        if Instant::now() >= deadline {
            snapshot.errors.push(format!(
                "{key}: Sonar refresh time budget reached. Results may be incomplete"
            ));
            break;
        }
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|branch| {
                    let get = &bounded_get;
                    scope.spawn(move || {
                        fetch_branch_issues(
                            config,
                            branch,
                            assigned_to_me,
                            state,
                            &mut |path, query| get(path, query),
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        Err("Sonar branch worker panicked. Results may be incomplete".into())
                    })
                })
                .collect::<Vec<_>>()
        });
        for (branch, result) in batch.iter().zip(results) {
            match result {
                Ok(issues) => snapshot.issues.extend(issues),
                Err(e) => snapshot
                    .errors
                    .push(format!("{key} / {}: {e}", branch.name)),
            }
        }
    }
    // The roster comes from scans, independently of issue counts and inbox filters.
    snapshot.branches = branches;
    Ok(snapshot)
}

fn repository_name_from_links(data: &Value) -> Option<String> {
    data.get("links")?.as_array()?.iter().find_map(|link| {
        if field(link, "type") != "scm" {
            return None;
        }
        let href = field(link, "url");
        let href = if href.is_empty() {
            field(link, "href")
        } else {
            href
        };
        let url = url::Url::parse(&href).ok()?;
        let name = url
            .path_segments()?
            .rfind(|segment| !segment.is_empty())?
            .trim_end_matches(".git");
        (!name.is_empty()).then(|| name.to_string())
    })
}

fn parse_branches(data: &Value, key: &str) -> Result<Vec<SonarBranch>, String> {
    let nodes = data
        .get("branches")
        .and_then(Value::as_array)
        .ok_or("Sonar did not return branches")?;
    let mut branches: Vec<_> = nodes
        .iter()
        .filter_map(|node| {
            let name = field(node, "name");
            let date = field(node, "analysisDate");
            if name.is_empty() || date.is_empty() {
                return None;
            }
            Some(SonarBranch {
                project_key: key.into(),
                repository_name: key.into(),
                name,
                is_main: node.get("isMain").and_then(Value::as_bool).unwrap_or(false),
                analysis_date: date,
            })
        })
        .collect();
    branches.sort_by(|a, b| b.is_main.cmp(&a.is_main).then_with(|| a.name.cmp(&b.name)));
    Ok(branches)
}

fn fetch_branch_issues(
    config: &SonarConfig,
    branch: &SonarBranch,
    assigned_to_me: bool,
    state: &str,
    get: &mut impl FnMut(&str, &[(&str, &str)]) -> Result<Value, String>,
) -> Result<Vec<SonarIssue>, String> {
    let mut issues = Vec::new();
    for page in 1..=20 {
        let p = page.to_string();
        let mut query = vec![
            ("componentKeys", branch.project_key.as_str()),
            ("branch", branch.name.as_str()),
            ("ps", "500"),
            ("p", &p),
        ];
        if state == "open" {
            query.push(("resolved", "false"));
        }
        if assigned_to_me {
            query.push(("assignees", "__me__"));
        }
        let data = get("/api/issues/search", &query)?;
        let nodes = data
            .get("issues")
            .and_then(Value::as_array)
            .ok_or("Sonar did not return issues")?;
        for node in nodes {
            if let Some(issue) = parse_issue(node, config, branch) {
                issues.push(issue);
            }
        }
        let total = data
            .pointer("/paging/total")
            .or_else(|| data.get("total"))
            .and_then(Value::as_u64)
            .ok_or("Sonar did not return issue pagination")?;
        if page * 500 >= total {
            return Ok(issues);
        }
        if nodes.is_empty() {
            return Err("Sonar returned an incomplete issue page".into());
        }
    }
    Err(
        "Sonar's 10,000 issue search limit was reached for this branch. Narrow the scope in Sonar"
            .into(),
    )
}

fn parse_issue(node: &Value, config: &SonarConfig, branch: &SonarBranch) -> Option<SonarIssue> {
    let key = field(node, "key");
    if key.is_empty() {
        return None;
    }
    let component = field(node, "component");
    let prefix = format!("{}:", branch.project_key);
    let file = component
        .strip_prefix(&prefix)
        .unwrap_or(&component)
        .to_string();
    let status = field(node, "issueStatus");
    let status = if status.is_empty() {
        field(node, "status")
    } else {
        status
    };
    let resolved = !field(node, "resolution").is_empty()
        || matches!(
            status.as_str(),
            "CLOSED" | "RESOLVED" | "FIXED" | "ACCEPTED" | "FALSE_POSITIVE"
        );
    let mut url = url::Url::parse(&format!("{}/project/issues", config.site)).ok()?;
    url.query_pairs_mut()
        .append_pair("id", &branch.project_key)
        .append_pair("branch", &branch.name)
        .append_pair("issues", &key)
        .append_pair("open", &key);
    Some(SonarIssue {
        key,
        repository_name: branch.repository_name.clone(),
        project_key: branch.project_key.clone(),
        branch: branch.name.clone(),
        message: field(node, "message"),
        severity: field(node, "severity"),
        rule: field(node, "rule"),
        issue_type: field(node, "type"),
        status,
        resolved,
        file,
        line: node
            .get("line")
            .or_else(|| node.pointer("/textRange/startLine"))
            .and_then(Value::as_u64),
        updated_at: field(node, "updateDate"),
        assignee: field(node, "assignee"),
        url: url.into(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SonarIssueContent {
    repository_name: String,
    issue: Value,
    rule: Option<Value>,
    rule_html: String,
    rule_url: String,
    reference_documentation: bool,
    source: Vec<SonarSourceLine>,
    comparison: Option<SonarSourceComparison>,
    errors: Vec<String>,
}

#[derive(Serialize)]
pub struct SonarSourceLine {
    line: u64,
    code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SonarSourceComparison {
    base_branch: String,
    original: String,
    current: String,
}

#[tauri::command]
pub async fn sonar_issue_content(
    app: AppHandle,
    project_key: String,
    branch: String,
    issue_key: String,
) -> Result<SonarIssueContent, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = read_config(&app)?.ok_or("Connect Sonar in Settings")?;
        if !config.watches(&project_key) {
            return Err("This Sonar project is not connected".into());
        }
        fetch_issue_content(
            &config,
            &project_key,
            &branch,
            &issue_key,
            |path, query| sonar_get(&config, path, query),
            |path, query| sonar_text(&config, path, query),
            |path| {
                // Documentation is public. Never send the user's token to the documentation host.
                let public = SonarConfig {
                    site: "https://raw.githubusercontent.com".into(),
                    ..Default::default()
                };
                sonar_text(&public, path, &[]).map_err(|e| e.to_string())
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn fetch_issue_content(
    config: &SonarConfig,
    project: &str,
    branch: &str,
    key: &str,
    mut get: impl FnMut(&str, &[(&str, &str)]) -> Result<Value, String>,
    mut text: impl FnMut(&str, &[(&str, &str)]) -> Result<String, SonarTextError>,
    mut documentation: impl FnMut(&str) -> Result<String, String>,
) -> Result<SonarIssueContent, String> {
    let data = get(
        "/api/issues/search",
        &[
            ("componentKeys", project),
            ("branch", branch),
            ("issues", key),
            ("additionalFields", "_all"),
            ("ps", "1"),
        ],
    )?;
    let issue = data
        .get("issues")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.iter().find(|node| field(node, "key") == key))
        .cloned()
        .ok_or("Sonar could not find this issue on the selected branch")?;
    let metadata = get("/api/components/show", &[("component", project)]).unwrap_or(Value::Null);
    let display_name = metadata
        .pointer("/component/name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(project)
        .to_string();
    let repository_name = get("/api/project_links/search", &[("projectKey", project)])
        .ok()
        .and_then(|v| repository_name_from_links(&v))
        .unwrap_or(display_name);
    let organization = metadata
        .pointer("/component/organization")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let rule_key = field(&issue, "rule");
    let mut rule_query = vec![("key", rule_key.as_str())];
    if !organization.is_empty() {
        rule_query.push(("organization", organization));
    }
    let mut errors = Vec::new();
    let rule = match get("/api/rules/show", &rule_query) {
        Ok(data) => data.get("rule").cloned(),
        Err(e) => {
            errors.push(format!("Rule guidance: {e}"));
            None
        }
    };
    let mut rule_html = rule
        .as_ref()
        .map(|r| field(r, "htmlDesc"))
        .unwrap_or_default();
    let has_sections = rule
        .as_ref()
        .and_then(|r| r.get("descriptionSections"))
        .and_then(Value::as_array)
        .is_some_and(|sections| sections.iter().any(|s| !field(s, "content").is_empty()));
    let mut rule_url = url::Url::parse(&format!("{}/coding_rules", config.site))
        .map_err(|_| "Invalid Sonar URL")?;
    rule_url
        .query_pairs_mut()
        .append_pair("open", &rule_key)
        .append_pair("rule_key", &rule_key);
    let mut rule_url = rule_url.to_string();
    let mut reference_documentation = false;
    if rule_html.is_empty() && !has_sections {
        if let Some(path) = reference_documentation_path(&rule_key) {
            match documentation(&path) {
                Ok(html) if html.contains("<p>") || html.contains("<h2>") => {
                    rule_html = html;
                    rule_url = format!(
                        "https://github.com{}",
                        path.replace("/master/", "/blob/master/")
                    );
                    reference_documentation = true;
                }
                Ok(_) => errors
                    .push("Rule guidance: Sonar's reference documentation is unavailable".into()),
                Err(e) => errors.push(format!("Reference documentation: {e}")),
            }
        }
    }
    let component = field(&issue, "component");
    let mut source = Vec::new();
    let mut comparison = None;
    if !component.is_empty() {
        match text(
            "/api/sources/raw",
            &[("key", component.as_str()), ("branch", branch)],
        ) {
            Ok(raw) => {
                source = source_context(&raw, &issue);
                let main = get("/api/project_branches/list", &[("project", project)])
                    .ok()
                    .and_then(|data| parse_branches(&data, project).ok())
                    .and_then(|branches| {
                        branches.into_iter().find(|b| b.is_main && b.name != branch)
                    });
                if let Some(main) = main {
                    match text(
                        "/api/sources/raw",
                        &[("key", component.as_str()), ("branch", &main.name)],
                    ) {
                        Ok(original) if original != raw => {
                            if original.len() + raw.len() <= 250_000 {
                                comparison = Some(SonarSourceComparison {
                                    base_branch: main.name,
                                    original,
                                    current: raw,
                                });
                            } else {
                                errors.push("Analyzed branch diff is too large to display. Source context is shown above".into());
                            }
                        }
                        Ok(_) => {}
                        Err(SonarTextError::NotFound) => {}
                        Err(e) => errors.push(format!("Main branch comparison: {e}")),
                    }
                }
            }
            Err(e) => errors.push(format!(
                "Source context: {e}. Source access requires See Source Code permission"
            )),
        }
    }
    Ok(SonarIssueContent {
        repository_name,
        issue,
        rule,
        rule_html,
        rule_url,
        reference_documentation,
        source,
        comparison,
        errors,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SonarTransitionRequest {
    project_key: String,
    branch: String,
    issue_key: String,
    transition: String,
    comment: String,
    share_feedback: bool,
}

#[tauri::command]
pub async fn sonar_transition_issue(
    app: AppHandle,
    request: SonarTransitionRequest,
) -> Result<SonarIssue, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = read_config(&app)?.ok_or("Connect Sonar in Settings")?;
        transition_issue_with(
            &config,
            &request,
            |path, query| sonar_get(&config, path, query),
            |fields| sonar_transition_post(&config, fields),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn transition_issue_with(
    config: &SonarConfig,
    request: &SonarTransitionRequest,
    mut get: impl FnMut(&str, &[(&str, &str)]) -> Result<Value, String>,
    mut post: impl FnMut(&[(&str, &str)]) -> Result<Value, String>,
) -> Result<SonarIssue, String> {
    if !config.watches(&request.project_key) {
        return Err("This Sonar project is not connected".into());
    }
    let data = get(
        "/api/issues/search",
        &[
            ("componentKeys", &request.project_key),
            ("branch", &request.branch),
            ("issues", &request.issue_key),
            ("additionalFields", "transitions"),
            ("ps", "1"),
        ],
    )?;
    let issue = data
        .get("issues")
        .and_then(Value::as_array)
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|issue| field(issue, "key") == request.issue_key)
        })
        .ok_or("Sonar could not find this issue on the selected branch")?;
    let allowed = issue
        .get("transitions")
        .and_then(Value::as_array)
        .is_some_and(|transitions| {
            transitions
                .iter()
                .any(|value| value.as_str() == Some(request.transition.as_str()))
        });
    if !allowed {
        return Err("This status change is no longer available. Refresh the issue and check your permissions".into());
    }
    let mut fields = vec![
        ("issue", request.issue_key.as_str()),
        ("transition", request.transition.as_str()),
    ];
    let comment = request.comment.trim();
    if !comment.is_empty() {
        fields.push(("comment", comment));
        fields.push((
            "isFeedback",
            if request.share_feedback {
                "true"
            } else {
                "false"
            },
        ));
    }
    let response = post(&fields)?;
    // The mutation has succeeded. Keep the original fields if the response omits context.
    let mut updated = issue.clone();
    if let Some(fields) = response.get("issue").and_then(Value::as_object) {
        if let Some(target) = updated.as_object_mut() {
            if fields.contains_key("status") && !fields.contains_key("issueStatus") {
                target.remove("issueStatus");
            }
            let new_status = fields
                .get("issueStatus")
                .or_else(|| fields.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !fields.contains_key("resolution")
                && ["OPEN", "CONFIRMED", "REOPENED"].contains(&new_status)
            {
                target.remove("resolution");
            }
            target.extend(fields.clone());
        }
    }
    let branch = SonarBranch {
        project_key: request.project_key.clone(),
        repository_name: request.project_key.clone(),
        name: request.branch.clone(),
        is_main: false,
        analysis_date: String::new(),
    };
    parse_issue(&updated, config, &branch).ok_or_else(|| {
        "Sonar changed the status but returned an unreadable issue. Refresh the inbox".to_string()
    })
}

fn sonar_transition_post(config: &SonarConfig, fields: &[(&str, &str)]) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .redirects(0)
        .build();
    let response = match agent.post(&format!("{}/api/issues/do_transition", config.site))
        .set("Authorization", &format!("Bearer {}", config.token))
        .set("Accept", "application/json").send_form(fields) {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => return Err("Sonar token is invalid or expired".into()),
        Err(ureq::Error::Status(403, _)) => return Err("Sonar denied this status change. Accept and False positive require Administer Issues permission".into()),
        Err(ureq::Error::Status(400, _)) => return Err("Sonar rejected the status change. Refresh the available transitions and try again".into()),
        Err(ureq::Error::Status(429, _)) => return Err("Sonar rate limit reached. Wait before trying again".into()),
        Err(ureq::Error::Status(status, _)) => return Err(format!("Sonar status change failed ({status})")),
        Err(_) => return Err("Could not confirm the status change with Sonar. Refresh the issue before trying again".into()),
    };
    if !(200..300).contains(&response.status()) {
        return Err("Sonar redirected the status change. Check the server URL".into());
    }
    let body = response.into_string().map_err(|_| {
        "Sonar changed the status but the response could not be read. Refresh the issue"
    })?;
    serde_json::from_str(&body)
        .map_err(|_| "Sonar changed the status but returned invalid JSON. Refresh the issue".into())
}

fn reference_documentation_path(rule: &str) -> Option<String> {
    let (repo, id) = rule.split_once(':')?;
    if !id.starts_with('S') || id.len() < 2 || !id[1..].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let language = match repo {
        "csharpsquid" => "cs",
        "vbnet" => "vbnet",
        _ => return None,
    };
    Some(format!(
        "/SonarSource/sonar-dotnet/master/analyzers/rspec/{language}/{id}.html"
    ))
}

fn source_context(raw: &str, issue: &Value) -> Vec<SonarSourceLine> {
    let start = issue
        .pointer("/textRange/startLine")
        .or_else(|| issue.get("line"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1);
    let end = issue
        .pointer("/textRange/endLine")
        .and_then(Value::as_u64)
        .unwrap_or(start)
        .max(start)
        .min(start.saturating_add(60));
    let first = start.saturating_sub(8).max(1);
    raw.lines()
        .enumerate()
        .skip((first - 1) as usize)
        .take((end.saturating_add(8) - first + 1) as usize)
        .map(|(index, code)| SonarSourceLine {
            line: index as u64 + 1,
            code: code.to_string(),
        })
        .collect()
}

fn field(node: &Value, key: &str) -> String {
    node.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn normalize_site(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    let raw = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let mut parsed = url::Url::parse(&raw).map_err(|_| "Enter a valid Sonar URL")?;
    if parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Enter a Sonar server URL without credentials, query or fragment".into());
    }
    if parsed.scheme() != "https"
        && !(parsed.scheme() == "http"
            && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        return Err("Use HTTPS for Sonar, or HTTP for a local server".into());
    }
    if parsed.path() == "/" {
        parsed.set_path("");
    }
    Ok(parsed.to_string().trim_end_matches('/').to_string())
}

fn sonar_get(config: &SonarConfig, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    serde_json::from_str(&sonar_text(config, path, query).map_err(|e| e.to_string())?)
        .map_err(|_| "Sonar returned invalid JSON".into())
}

#[derive(Debug)]
enum SonarTextError {
    NotFound,
    Other(String),
}

impl std::fmt::Display for SonarTextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str(
                "Sonar project or API endpoint was not found. Check the server URL and project key",
            ),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl From<String> for SonarTextError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for SonarTextError {
    fn from(message: &str) -> Self {
        Self::Other(message.into())
    }
}

fn sonar_text(
    config: &SonarConfig,
    path: &str,
    query: &[(&str, &str)],
) -> Result<String, SonarTextError> {
    // Do not follow redirects with credentials to another host.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .redirects(0)
        .build();
    let mut request = agent
        .get(&format!("{}{path}", config.site))
        .set("Accept", "application/json, text/plain, text/html");
    if !config.token.is_empty() {
        request = request.set("Authorization", &format!("Bearer {}", config.token));
    }
    for (name, value) in query {
        request = request.query(name, value);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => return Err("Sonar token is invalid or expired".into()),
        Err(ureq::Error::Status(403, _)) => return Err("Sonar denied access. Use a personal access token with Browse permission on the project".into()),
        Err(ureq::Error::Status(404, _)) => return Err(SonarTextError::NotFound),
        Err(ureq::Error::Status(429, _)) => return Err("Sonar rate limit reached. Wait a few minutes before refreshing".into()),
        Err(ureq::Error::Status(status, _)) => return Err(format!("Sonar request failed ({status})").into()),
        Err(_) => return Err("Could not reach Sonar".into()),
    };
    if !(200..300).contains(&response.status()) {
        return Err("Sonar redirected the request. Enter the server's final URL".into());
    }
    use std::io::Read;
    let mut body = String::new();
    response
        .into_reader()
        .take(2_000_001)
        .read_to_string(&mut body)
        .map_err(|_| "Sonar returned an unreadable response")?;
    if body.len() > 2_000_000 {
        return Err("Sonar response is too large to display".into());
    }
    Ok(body)
}

fn config_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("sonar-config.json"))
}
fn read_config(app: &AppHandle) -> Result<Option<SonarConfig>, String> {
    match fs::read_to_string(config_path(app)?) {
        Ok(raw) => {
            let mut config: SonarConfig =
                serde_json::from_str(&raw).map_err(|_| "Sonar settings are invalid")?;
            config.site = normalize_site(&config.site)?;
            Ok((!config.token.is_empty()).then_some(config))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
fn write_config(app: &AppHandle, config: &SonarConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Readers do not take the lock, so replace the file whole rather than truncating it in place.
    let temp = path.with_extension("json.tmp");
    write_secret_file(
        &temp,
        &serde_json::to_string(config).map_err(|e| e.to_string())?,
    )?;
    fs::rename(&temp, &path).map_err(|e| e.to_string())
}
fn update_config(
    app: &AppHandle,
    change: impl FnOnce(&mut SonarConfig) -> Result<(), String>,
) -> Result<SonarStatus, String> {
    // Commands run concurrently. Serialize read-modify-write so one change cannot drop another.
    let _lock = config_lock();
    let mut config = read_config(app)?.ok_or("Connect Sonar in Settings")?;
    change(&mut config)?;
    write_config(app, &config)?;
    Ok(status_for(Some(&config)))
}
fn delete_config(app: &AppHandle) -> Result<(), String> {
    match fs::remove_file(config_path(app)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
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
            .map_err(|e| e.to_string())?;
        file.write_all(value.as_bytes()).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, value).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> SonarConfig {
        SonarConfig {
            site: "https://sonarcloud.io".into(),
            token: "test-token".into(),
            organization: "org".into(),
            projects: vec![SonarProject {
                key: "org_app".into(),
                name: "App".into(),
            }],
            selected_project: String::new(),
        }
    }

    #[test]
    fn scanned_branches_include_zero_issue_scans_but_exclude_unanalyzed_branches() {
        let data = json!({"branches": [
            {"name":"unscanned","isMain":false},
            {"name":"feature/empty","analysisDate":"2026-10-14T10:00:00Z","status":{"bugs":0}},
            {"name":"main","isMain":true,"analysisDate":"2026-10-14T10:00:00Z"}
        ]});
        let branches = parse_branches(&data, "org_app").unwrap();
        assert_eq!(
            branches.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            vec!["main", "feature/empty"]
        );
        assert!(branches.iter().all(|b| b.project_key == "org_app"));
    }

    #[test]
    fn a_panicked_branch_keeps_other_branch_results_and_the_roster() {
        let result = fetch_snapshot(
            &config(),
            "org_app",
            false,
            "all",
            Instant::now() + Duration::from_secs(60),
            |path, query| match path {
                "/api/project_branches/list" => Ok(json!({"branches":[
                    {"name":"broken","analysisDate":"date"},
                    {"name":"healthy","analysisDate":"date"}
                ]})),
                "/api/project_links/search" => {
                    Ok(json!({"links":[{"type":"scm","url":"https://github.com/org/app"}]}))
                }
                "/api/issues/search" => {
                    if query.contains(&("branch", "broken")) {
                        panic!("simulated branch worker failure");
                    }
                    Ok(json!({"issues":[{"key":"healthy-issue"}],"paging":{"total":1}}))
                }
                _ => panic!("unexpected request: {path}"),
            },
        )
        .unwrap();
        assert_eq!(result.branches.len(), 2);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].key, "healthy-issue");
        assert_eq!(result.issues[0].branch, "healthy");
        assert_eq!(
            result.errors,
            vec!["org_app / broken: Sonar branch worker panicked. Results may be incomplete"]
        );
    }

    #[test]
    fn deadline_reached_keeps_the_scanned_roster_and_skips_issue_reads() {
        let config = config();
        let deadline = Instant::now() + Duration::from_secs(1);
        let result = fetch_snapshot(&config, "org_app", false, "all", deadline, |path, _| {
            match path {
                "/api/project_branches/list" => Ok(json!({"branches":[
                    {"name":"main","analysisDate":"date"},
                    {"name":"feature","analysisDate":"date"}
                ]})),
                "/api/project_links/search" => {
                    // Let this already-started metadata request consume the remaining budget.
                    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                    Ok(json!({"links":[{"type":"scm","url":"https://github.com/org/app"}]}))
                }
                _ => panic!("no further requests should start after the deadline: {path}"),
            }
        })
        .unwrap();
        assert_eq!(result.branches.len(), 2);
        assert!(result
            .branches
            .iter()
            .all(|branch| branch.repository_name == "app"));
        assert!(result.issues.is_empty());
        assert_eq!(
            result.errors,
            vec!["org_app: Sonar refresh time budget reached. Results may be incomplete"]
        );
    }

    #[test]
    fn branch_reads_overlap_with_at_most_four_workers() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let reads = AtomicUsize::new(0);
        let result = fetch_snapshot(&config(), "org_app", false, "all", Instant::now() + Duration::from_secs(60), |path, _| {
            if path.ends_with("/list") {
                return Ok(json!({"branches": (0..9).map(|i| json!({"name": format!("branch-{i}"), "analysisDate": "date"})).collect::<Vec<_>>()}));
            }
            if path == "/api/project_links/search" {
                return Ok(json!({"links": [{"type":"scm","url":"https://github.com/org/app"}]}));
            }
            assert_eq!(path, "/api/issues/search");
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(count, Ordering::SeqCst);
            reads.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(10));
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(json!({"issues":[],"paging":{"total":0}}))
        }).unwrap();
        assert_eq!(result.branches.len(), 9);
        assert_eq!(reads.load(Ordering::SeqCst), 9);
        assert!((2..=4).contains(&peak.load(Ordering::SeqCst)));
        assert!(result.errors.is_empty());
    }

    #[test]
    fn roster_survives_empty_assignee_results_and_failed_issue_reads() {
        let result = fetch_snapshot(
            &config(),
            "org_app",
            true,
            "open",
            Instant::now() + Duration::from_secs(60),
            |path, query| {
                if path.ends_with("/list") {
                    return Ok(json!({"branches":[
                        {"name":"main","analysisDate":"2026-10-14T10:00:00Z"},
                        {"name":"feature/empty","analysisDate":"2026-10-14T10:00:00Z"}
                    ]}));
                }
                if path == "/api/project_links/search" {
                    return Ok(json!({"links":[]}));
                }
                if path == "/api/components/show" {
                    return Ok(json!({"component":{"name":"App repository"}}));
                }
                assert!(query.contains(&("assignees", "__me__")));
                assert!(query.contains(&("resolved", "false")));
                if query.contains(&("branch", "main")) {
                    return Err("Browse access denied".into());
                }
                Ok(json!({"issues":[],"paging":{"total":0}}))
            },
        )
        .unwrap();
        assert_eq!(result.branches.len(), 2);
        assert!(result
            .branches
            .iter()
            .all(|branch| branch.repository_name == "App repository"));
        assert!(result.issues.is_empty());
        assert_eq!(result.errors, vec!["org_app / main: Browse access denied"]);
    }

    #[test]
    fn issues_are_paginated_and_keep_branch_file_and_rule_context() {
        let branch = SonarBranch {
            project_key: "org_app".into(),
            repository_name: "app".into(),
            name: "feature/a & b".into(),
            is_main: false,
            analysis_date: "date".into(),
        };
        let mut pages = Vec::new();
        let issues = fetch_branch_issues(&config(), &branch, false, "all", &mut |_, query| {
            assert!(query.contains(&("branch", "feature/a & b")));
            assert!(!query.iter().any(|(key, _)| *key == "resolved"));
            let page = query.iter().find(|(key, _)| *key == "p").unwrap().1;
            pages.push(page.to_string());
            let count = if page == "1" { 500 } else { 1 };
            let nodes: Vec<_> = (0..count).map(|i| json!({"key":format!("{page}-{i}"),"message":"Fix this","component":"org_app:src/a.ts","rule":"ts:S1","severity":"MAJOR","issueStatus":"OPEN","textRange":{"startLine":12},"updateDate":"2026-10-14T10:00:00+0000"})).collect();
            Ok(json!({"issues":nodes,"paging":{"total":501}}))
        }).unwrap();
        assert_eq!(pages, vec!["1", "2"]);
        assert_eq!(issues.len(), 501);
        assert_eq!(issues[0].file, "src/a.ts");
        assert_eq!(issues[0].line, Some(12));
        assert!(!issues[0].resolved);
        let parsed = url::Url::parse(&issues[0].url).unwrap();
        assert!(parsed
            .query_pairs()
            .any(|(key, value)| key == "branch" && value == "feature/a & b"));
    }

    #[test]
    fn issue_statuses_support_cloud_and_server() {
        let branch = SonarBranch {
            project_key: "org_app".into(),
            repository_name: "app".into(),
            name: "main".into(),
            is_main: true,
            analysis_date: "date".into(),
        };
        for status in ["FIXED", "ACCEPTED", "FALSE_POSITIVE", "RESOLVED", "CLOSED"] {
            assert!(
                parse_issue(
                    &json!({"key":"i", "issueStatus":status}),
                    &config(),
                    &branch
                )
                .unwrap()
                .resolved
            );
        }
        assert!(
            parse_issue(
                &json!({"key":"i", "status":"RESOLVED", "resolution":"FIXED"}),
                &config(),
                &branch
            )
            .unwrap()
            .resolved
        );
    }

    #[test]
    fn issue_content_uses_the_analyzed_branch_and_falls_back_to_official_rule_docs() {
        let mut documentation_path = String::new();
        let result = fetch_issue_content(&config(), "org_app", "develop", "issue-1", |path, query| {
            match path {
                "/api/issues/search" => {
                    assert!(query.contains(&("branch", "develop")));
                    assert!(query.contains(&("issues", "issue-1")));
                    Ok(json!({"issues":[{"key":"issue-1","component":"org_app:File.cs","rule":"csharpsquid:S6967","line":10,"textRange":{"startLine":10,"endLine":11},"flows":[{"locations":[{"msg":"Related call"}]}]}]}))
                }
                "/api/components/show" => Ok(json!({"component":{"name":"App","organization":"my-org"}})),
                "/api/project_links/search" => Ok(json!({"links":[{"type":"scm","url":"https://github.com/acme/app-repo.git"}]})),
                "/api/project_branches/list" => Ok(json!({"branches":[{"name":"main","isMain":true,"analysisDate":"2026-10-14"}]})),
                "/api/rules/show" => {
                    assert!(query.contains(&("organization", "my-org")));
                    Ok(json!({"rule":{"name":"Validate models","lang":"cs"}}))
                }
                _ => panic!("Unexpected path {path}"),
            }
        }, |path, query| {
            assert_eq!(path, "/api/sources/raw");
            assert!(query.contains(&("key", "org_app:File.cs")));
            if query.contains(&("branch", "main")) { return Ok("base file\n".into()); }
            assert!(query.contains(&("branch", "develop")));
            Ok((1..=50).map(|n| format!("line {n}\n")).collect())
        }, |path| { documentation_path = path.into(); Ok("<h2>Why is this an issue?</h2><p>Validate input</p>".into()) }).unwrap();
        assert_eq!(result.repository_name, "app-repo");
        assert!(result.reference_documentation);
        assert!(documentation_path.ends_with("/cs/S6967.html"));
        assert!(result.rule_url.contains("/blob/master/"));
        assert_eq!(result.comparison.as_ref().unwrap().base_branch, "main");
        assert_eq!(result.comparison.as_ref().unwrap().original, "base file\n");
        assert_eq!(result.source.first().unwrap().line, 2);
        assert_eq!(result.source.last().unwrap().line, 19);
        assert!(result.issue.get("flows").is_some());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn main_branch_comparison_skips_only_not_found() {
        for (error, expected_errors) in [
            (SonarTextError::NotFound, vec![]),
            (
                SonarTextError::Other("Access denied".into()),
                vec!["Main branch comparison: Access denied"],
            ),
            (
                SonarTextError::Other(SonarTextError::NotFound.to_string()),
                vec!["Main branch comparison: Sonar project or API endpoint was not found. Check the server URL and project key"],
            ),
        ] {
            let mut error = Some(error);
            let result = fetch_issue_content(
                &config(), "org_app", "develop", "i",
                |path, _| match path {
                    "/api/issues/search" => Ok(json!({"issues":[{"key":"i","component":"org_app:new.ts","rule":"ts:S1","line":1}]})),
                    "/api/rules/show" => Ok(json!({"rule":{"htmlDesc":"<p>Guidance</p>"}})),
                    "/api/project_branches/list" => Ok(json!({"branches":[{"name":"main","isMain":true,"analysisDate":"2026-10-14"}]})),
                    _ => Err("Metadata unavailable".into()),
                },
                |_, query| {
                    if query.contains(&("branch", "main")) {
                        Err(error.take().unwrap())
                    } else {
                        Ok("new source\n".into())
                    }
                },
                |_| panic!("Instance rule guidance is available"),
            ).unwrap();
            assert!(error.is_none(), "Main branch source must be requested");
            assert!(!result.source.is_empty());
            assert!(result.comparison.is_none());
            assert_eq!(result.errors, expected_errors);
        }
    }

    #[test]
    fn sonar_text_distinguishes_http_not_found_from_other_failures() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        for status in [404, 403, 500] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut config = config();
            config.site = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                write!(
                    stream,
                    "HTTP/1.1 {status} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });
            let error = sonar_text(&config, "/api/sources/raw", &[]).unwrap_err();
            server.join().unwrap();
            assert_eq!(matches!(error, SonarTextError::NotFound), status == 404);
            if status == 403 {
                assert!(error.to_string().contains("Sonar denied access"));
            } else if status == 500 {
                assert_eq!(error.to_string(), "Sonar request failed (500)");
            }
        }
    }

    #[test]
    fn rule_guidance_survives_source_permissions_and_does_not_fetch_reference_docs_when_available()
    {
        let result = fetch_issue_content(&config(), "org_app", "main", "i", |path, _| {
            match path {
                "/api/issues/search" => Ok(json!({"issues":[{"key":"i","component":"org_app:File.cs","rule":"csharpsquid:S6967"}]})),
                "/api/rules/show" => Ok(json!({"rule":{"descriptionSections":[{"key":"root_cause","content":"<p>Why</p>"}]}})),
                _ => Err("Metadata unavailable".into()),
            }
        }, |_, _| Err("Access denied".into()), |_| panic!("Do not fetch latest docs when instance docs exist")).unwrap();
        assert!(!result.reference_documentation);
        assert!(result.rule.is_some());
        assert!(result.source.is_empty());
        assert!(result.errors[0].contains("See Source Code"));
        assert_eq!(result.repository_name, "org_app");
        assert!(reference_documentation_path("custom:../../secret").is_none());
    }

    fn transition_request(transition: &str) -> SonarTransitionRequest {
        SonarTransitionRequest {
            project_key: "org_app".into(),
            branch: "develop".into(),
            issue_key: "i".into(),
            transition: transition.into(),
            comment: "Explanation & evidence".into(),
            share_feedback: false,
        }
    }

    #[test]
    fn status_change_rechecks_available_transitions_and_sends_comment_with_opt_in_feedback() {
        let mut request = transition_request("falsepositive");
        request.share_feedback = true;
        let result = transition_issue_with(&config(), &request, |_, query| {
            assert!(query.contains(&("branch", "develop")));
            assert!(query.contains(&("additionalFields", "transitions")));
            Ok(json!({"issues":[{"key":"i","component":"org_app:File.cs","rule":"csharpsquid:S1","status":"OPEN","transitions":["falsepositive"]}]}))
        }, |fields| {
            assert!(fields.contains(&("transition", "falsepositive")));
            assert!(fields.contains(&("comment", "Explanation & evidence")));
            assert!(fields.contains(&("isFeedback", "true")));
            Ok(json!({"issue":{"key":"i","status":"RESOLVED","issueStatus":"FALSE_POSITIVE","resolution":"FALSE-POSITIVE"}}))
        }).unwrap();
        assert_eq!(result.status, "FALSE_POSITIVE");
        assert!(result.resolved);
        assert_eq!(result.file, "File.cs");
    }

    #[test]
    fn unavailable_status_changes_are_not_posted() {
        let result = transition_issue_with(
            &config(),
            &transition_request("accept"),
            |_, _| Ok(json!({"issues":[{"key":"i","transitions":["confirm"]}]})),
            |_| panic!("Do not post a disallowed transition"),
        );
        assert!(result.unwrap_err().contains("no longer available"));
    }

    #[test]
    fn reopening_drops_stale_resolution_and_blank_comments_are_not_shared() {
        let mut request = transition_request("reopen");
        request.comment = " ".into();
        request.share_feedback = true;
        let result = transition_issue_with(&config(), &request, |_, _| Ok(json!({"issues":[{"key":"i","issueStatus":"ACCEPTED","status":"RESOLVED","resolution":"WONTFIX","transitions":["reopen"]}]})), |fields| {
            assert!(!fields.iter().any(|(key, _)| *key == "comment" || *key == "isFeedback"));
            Ok(json!({"issue":{"key":"i","status":"OPEN"}}))
        }).unwrap();
        assert_eq!(result.status, "OPEN");
        assert!(!result.resolved);
    }

    #[test]
    fn repository_names_come_from_scm_links() {
        assert_eq!(
            repository_name_from_links(&json!({"links":[
                {"type":"homepage","href":"https://example.com/docs"},
                {"type":"scm","url":"https://github.com/acme/monocode.git/"}
            ]}))
            .as_deref(),
            Some("monocode")
        );
        assert!(repository_name_from_links(&json!({"links":[]})).is_none());
    }

    #[test]
    fn base_urls_allow_server_context_paths_and_reject_credential_urls() {
        assert_eq!(
            normalize_site("sonarcloud.io/").unwrap(),
            "https://sonarcloud.io"
        );
        assert_eq!(
            normalize_site("https://example.com/sonar/").unwrap(),
            "https://example.com/sonar"
        );
        assert!(normalize_site("https://user:pass@example.com").is_err());
        assert!(normalize_site("http://example.com").is_err());
        assert!(normalize_site("https://example.com?token=secret").is_err());
        assert!(normalize_site("http://localhost:9000").is_ok());
    }

    #[test]
    fn projects_are_paged_scoped_to_the_organization_and_sorted_by_name() {
        let mut pages = Vec::new();
        let projects = fetch_projects("org", |query| {
            assert!(query.contains(&("organization", "org")));
            let page = query
                .iter()
                .find(|(key, _)| *key == "p")
                .unwrap()
                .1
                .to_string();
            let nodes: Vec<_> = if page == "1" {
                (0..500)
                    .map(|i| json!({"key": format!("org_p{i:03}"), "name": format!("P{i:03}")}))
                    .collect()
            } else {
                vec![
                    json!({"key":"org_alpha","name":"alpha"}),
                    json!({"key":"org_unnamed"}),
                    json!({"name":"no key"}),
                ]
            };
            pages.push(page);
            Ok(json!({"components": nodes, "paging": {"total": 503}}))
        })
        .unwrap();
        assert_eq!(pages, vec!["1", "2"]);
        assert_eq!(projects.len(), 502);
        assert_eq!(
            projects[0],
            SonarProject {
                key: "org_alpha".into(),
                name: "alpha".into()
            }
        );
        assert!(projects.contains(&SonarProject {
            key: "org_unnamed".into(),
            name: "org_unnamed".into()
        }));
    }

    #[test]
    fn server_projects_are_listed_without_an_organization() {
        let projects = fetch_projects("", |query| {
            assert!(!query.iter().any(|(key, _)| *key == "organization"));
            Ok(json!({"components": [{"key":"app","name":"App"}], "paging": {"total": 1}}))
        })
        .unwrap();
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn organizations_fall_back_to_their_key_for_a_name() {
        let organizations = parse_organizations(&json!({"organizations": [
            {"key":"org","name":"Org"},
            {"key":"bare"},
            {"name":"no key"}
        ]}));
        assert_eq!(
            organizations,
            vec![
                SonarOrganization {
                    key: "org".into(),
                    name: "Org".into()
                },
                SonarOrganization {
                    key: "bare".into(),
                    name: "bare".into()
                },
            ]
        );
    }

    #[test]
    fn an_unwatched_selection_falls_back_to_the_first_project() {
        let mut config = config();
        config.projects.push(SonarProject {
            key: "org_api".into(),
            name: "API".into(),
        });
        config.selected_project = "org_api".into();
        assert_eq!(config.selected().unwrap().key, "org_api");
        config.selected_project = "org_gone".into();
        assert_eq!(config.selected().unwrap().key, "org_app");
        assert_eq!(status_for(Some(&config)).selected_project, "org_app");
        config.projects.clear();
        assert!(config.selected().is_none());
    }

    #[test]
    fn switching_organization_clears_its_projects_but_reselecting_it_keeps_them() {
        let mut config = config();
        config.selected_project = "org_app".into();
        config.set_organization(" org ");
        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.selected_project, "org_app");
        config.set_organization("other");
        assert_eq!(config.organization, "other");
        assert!(config.projects.is_empty());
        assert!(config.selected_project.is_empty());
    }

    #[test]
    fn watched_projects_are_trimmed_deduplicated_and_named() {
        let mut config = config();
        config.set_projects(vec![
            SonarProject {
                key: " org_api ".into(),
                name: " API ".into(),
            },
            SonarProject {
                key: "org_api".into(),
                name: "Duplicate".into(),
            },
            SonarProject {
                key: "org_bare".into(),
                name: " ".into(),
            },
            SonarProject {
                key: " ".into(),
                name: "No key".into(),
            },
        ]);
        assert_eq!(
            config.projects,
            vec![
                SonarProject {
                    key: "org_api".into(),
                    name: "API".into()
                },
                SonarProject {
                    key: "org_bare".into(),
                    name: "org_bare".into()
                },
            ]
        );
    }

    #[test]
    fn only_a_watched_project_can_be_selected() {
        let mut config = config();
        assert!(config.select("org_other".into()).is_err());
        assert!(config.selected_project.is_empty());
        config.select("org_app".into()).unwrap();
        assert_eq!(config.selected_project, "org_app");
    }
}
