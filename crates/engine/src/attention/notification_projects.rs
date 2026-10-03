//! Port of src/features/notifications/model/notificationProjects.ts: the
//! project identity notification preferences attach to, for local folders,
//! repositories, Linear projects, and Jira projects.
//!
//! localStorage becomes `Kv` with the same key and JSON. The in-memory parse
//! cache is gone; the catalog is small. `subscribeNotificationProjects`
//! becomes a `Kv` subscription in the `Notifier` entity.

use std::sync::LazyLock;

use monocode_core::inbox::InboxProvider;
use monocode_core::paths::{path_key, slash};
use monocode_layout::paths::{pretty_cwd, project_name};
use monocode_settings::Kv;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The catalog key. v1 held Git-derived path mappings and is ignored.
pub const NOTIFICATION_PROJECTS_KEY: &str = "monocode.notificationProjects.v2";

/// `NotificationProject["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationProjectKind {
    #[serde(rename = "repository")]
    Repository,
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "jira")]
    Jira,
}

/// `NotificationProject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationProject {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub kind: NotificationProjectKind,
    pub paths: Vec<String>,
}

/// `looksLikeProject` from src/features/projects/model/recents.ts: a folder
/// worth treating as a project. Home, roots, and app bundles are not.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    let normalized = if trimmed.is_empty() { "/" } else { trimmed };
    let bytes = normalized.as_bytes();
    if (bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':') || normalized == "/"
    {
        return false;
    }
    // Home itself arrives expanded (`/Users/me`), so the `~` check above
    // misses it. Indexing it walks `~/Library`, which trips the OS consent
    // prompt.
    if pretty_cwd(path) == "~" {
        return false;
    }
    if path.contains(".app/") || path.contains(".app\\") {
        return false;
    }
    true
}

fn local_notification_project(path: &str) -> NotificationProject {
    NotificationProject {
        id: format!("local:{}", path_key(path)),
        name: project_name(path),
        detail: path.to_string(),
        kind: NotificationProjectKind::Local,
        paths: vec![path.to_string()],
    }
}

/// `knownNotificationProject`: the catalog entry that lists this path, or a
/// local project derived from the path.
pub fn known_notification_project(kv: &Kv, path: &str) -> Option<NotificationProject> {
    known_in(&load_notification_projects(kv), path)
}

fn known_in(catalog: &[NotificationProject], path: &str) -> Option<NotificationProject> {
    if !looks_like_project(path) {
        return None;
    }
    let key = path_key(path);
    Some(
        catalog
            .iter()
            .find(|project| project.paths.iter().any(|entry| path_key(entry) == key))
            .cloned()
            .unwrap_or_else(|| local_notification_project(path)),
    )
}

/// A `Map` keyed by id: replacing a value keeps its first position.
fn insert_ordered(projects: &mut Vec<NotificationProject>, project: NotificationProject) {
    match projects.iter_mut().find(|entry| entry.id == project.id) {
        Some(entry) => *entry = project,
        None => projects.push(project),
    }
}

/// `knownNotificationProjectSelection`: the requested paths' projects plus
/// the provider-only catalog projects.
pub fn known_notification_project_selection(kv: &Kv, paths: &[&str]) -> Vec<NotificationProject> {
    let mut requested: Vec<(String, String)> = Vec::new();
    for path in paths.iter().filter(|path| looks_like_project(path)) {
        let key = path_key(path);
        match requested.iter_mut().find(|(entry, _)| *entry == key) {
            Some(entry) => entry.1 = path.to_string(),
            None => requested.push((key, path.to_string())),
        }
    }
    let stored = load_notification_projects(kv);
    let mut projects = Vec::new();
    for project in &stored {
        if project.paths.is_empty()
            || project
                .paths
                .iter()
                .any(|path| requested.iter().any(|(key, _)| *key == path_key(path)))
        {
            insert_ordered(&mut projects, project.clone());
        }
    }
    for (_, path) in &requested {
        if let Some(project) = known_in(&stored, path) {
            insert_ordered(&mut projects, project);
        }
    }
    projects
}

fn valid_project(value: &Value) -> Option<NotificationProject> {
    let rec = value.as_object()?;
    rec.get("id")?.as_str()?;
    rec.get("name")?.as_str()?;
    rec.get("detail")?.as_str()?;
    let paths = rec.get("paths")?.as_array()?;
    if !paths.iter().all(Value::is_string) {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

/// `loadNotificationProjects`.
pub fn load_notification_projects(kv: &Kv) -> Vec<NotificationProject> {
    let raw = kv.get_item(NOTIFICATION_PROJECTS_KEY);
    match serde_json::from_str::<Value>(raw.as_deref().unwrap_or("[]")) {
        Ok(Value::Array(items)) => items.iter().filter_map(valid_project).collect(),
        _ => Vec::new(),
    }
}

/// `rememberNotificationProjects`: merge into the catalog by id, keeping
/// every path seen for a project. Returns whether the catalog changed.
pub fn remember_notification_projects(kv: &Kv, projects: &[NotificationProject]) -> bool {
    let current = load_notification_projects(kv);
    let mut next = current.clone();
    for project in projects {
        let previous = next.iter().find(|entry| entry.id == project.id);
        let mut paths: Vec<String> = previous
            .map(|previous| previous.paths.clone())
            .unwrap_or_default();
        for path in &project.paths {
            if !paths.contains(path) {
                paths.push(path.clone());
            }
        }
        insert_ordered(
            &mut next,
            NotificationProject {
                paths,
                ..project.clone()
            },
        );
    }
    if next == current {
        return false;
    }
    if let Ok(raw) = serde_json::to_string(&next) {
        kv.set_item(NOTIFICATION_PROJECTS_KEY, &raw);
    }
    true
}

/// `notificationProjectsSnapshot`.
pub fn notification_projects_snapshot(kv: &Kv) -> String {
    serde_json::to_string(&load_notification_projects(kv)).unwrap_or_else(|_| "[]".into())
}

/// The fields of an Inbox item `inboxNotificationProject` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationWorkItem {
    pub provider: InboxProvider,
    pub repo: String,
    pub url: String,
    pub project_path: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
}

impl NotificationWorkItem {
    pub fn new(provider: InboxProvider, repo: &str, url: &str) -> Self {
        Self {
            provider,
            repo: repo.into(),
            url: url.into(),
            project_path: None,
            project_id: None,
            project_name: None,
            team_id: None,
            team_name: None,
        }
    }
}

/// `value || fallback` for an optional string.
fn or_else(value: Option<&str>, fallback: impl FnOnce() -> String) -> String {
    value
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(fallback)
}

/// The WHATWG `URL.host`: the host name plus a non-default port.
fn url_host(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("");
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// `inboxNotificationProject`.
pub fn inbox_notification_project(item: &NotificationWorkItem) -> NotificationProject {
    if item.provider == InboxProvider::Jira {
        // Keep a usable project identity even if a provider omits the URL.
        let site = url::Url::parse(&item.url)
            .map(|url| url_host(&url).to_lowercase())
            .unwrap_or_else(|_| "unknown".into());
        let team = or_else(item.team_id.as_deref(), || {
            let repo = item.repo.to_lowercase();
            if repo.is_empty() {
                "unknown".into()
            } else {
                repo
            }
        });
        return NotificationProject {
            id: format!("jira:{site}:project:{team}"),
            name: or_else(item.team_name.as_deref(), || {
                if item.repo.is_empty() {
                    "Jira project".into()
                } else {
                    item.repo.clone()
                }
            }),
            detail: format!("Jira · {site}"),
            kind: NotificationProjectKind::Jira,
            paths: Vec::new(),
        };
    }
    if item.provider == InboxProvider::Linear {
        let project = item
            .project_id
            .as_deref()
            .map(monocode_core::js::trim)
            .filter(|project| !project.is_empty());
        let team_id = or_else(item.team_id.as_deref(), || "unknown".into());
        return NotificationProject {
            id: match project {
                Some(project) => format!("linear:project:{project}"),
                None => format!("linear:team:{team_id}:unassigned"),
            },
            name: match project {
                Some(project) => or_else(item.project_name.as_deref(), || project.to_string()),
                None => "No project".into(),
            },
            detail: format!(
                "Linear · {}",
                or_else(item.team_name.as_deref(), || or_else(
                    item.team_id.as_deref(),
                    || "Unknown team".into()
                ))
            ),
            kind: NotificationProjectKind::Linear,
            paths: Vec::new(),
        };
    }
    if let Some(path) = item.project_path.as_deref().filter(|path| !path.is_empty())
        && looks_like_project(path)
    {
        return NotificationProject {
            name: or_else(Some(&item.repo), || project_name(path)),
            kind: NotificationProjectKind::Repository,
            ..local_notification_project(path)
        };
    }
    if let Some(remote) = repository_project(&item.url, Some(&item.repo)) {
        return remote;
    }
    NotificationProject {
        id: format!(
            "repository:{}:{}",
            serde_json::to_value(item.provider)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default(),
            item.repo.to_lowercase()
        ),
        name: item.repo.clone(),
        detail: serde_json::to_value(item.provider)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default(),
        kind: NotificationProjectKind::Repository,
        paths: Vec::new(),
    }
}

/// `/^(?:[^@/]+@)?([^/:]+):(.+)$/`: an scp-style Git remote.
static SCP_REMOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[^@/]+@)?([^/:]+):(.+)$").expect("valid regex"));

fn scp_remote(remote: &str) -> Option<(&str, &str)> {
    let captures = SCP_REMOTE.captures(remote)?;
    Some((captures.get(1)?.as_str(), captures.get(2)?.as_str()))
}

fn repository_project(remote: &str, repo: Option<&str>) -> Option<NotificationProject> {
    let source = if remote.contains("://") {
        remote.to_string()
    } else if let Some((host, path)) = scp_remote(remote) {
        format!("ssh://{host}/{path}")
    } else {
        remote.to_string()
    };
    let url = url::Url::parse(&source).ok()?;
    if !["http", "https", "ssh", "git"].contains(&url.scheme()) {
        return None;
    }
    let name = repo.unwrap_or(url.path());
    let name = name.trim_matches('/');
    let name = name.strip_suffix(".git").unwrap_or(name);
    if url.host_str().is_none_or(str::is_empty) || !name.contains('/') {
        return None;
    }
    let host = url_host(&url).to_lowercase();
    Some(NotificationProject {
        id: format!("repository:{host}/{}", name.to_lowercase()),
        name: name.to_string(),
        detail: host,
        kind: NotificationProjectKind::Repository,
        paths: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jira() -> NotificationWorkItem {
        NotificationWorkItem {
            team_id: Some("10000".into()),
            team_name: Some("Engineering".into()),
            ..NotificationWorkItem::new(
                InboxProvider::Jira,
                "ENG",
                "https://acme.atlassian.net/browse/ENG-42",
            )
        }
    }

    #[test]
    fn groups_jira_preferences_by_site_and_stable_project_id() {
        let kv = Kv::in_memory();
        let project = inbox_notification_project(&jira());
        assert_eq!(
            project,
            NotificationProject {
                id: "jira:acme.atlassian.net:project:10000".into(),
                name: "Engineering".into(),
                detail: "Jira · acme.atlassian.net".into(),
                kind: NotificationProjectKind::Jira,
                paths: vec![],
            }
        );
        let renamed = NotificationWorkItem {
            repo: "RENAMED".into(),
            url: "https://acme.atlassian.net/browse/RENAMED-1".into(),
            ..jira()
        };
        assert_eq!(inbox_notification_project(&renamed).id, project.id);
        let other = NotificationWorkItem {
            url: "https://other.atlassian.net/browse/ENG-42".into(),
            ..jira()
        };
        assert_ne!(inbox_notification_project(&other).id, project.id);
        remember_notification_projects(&kv, std::slice::from_ref(&project));
        assert!(load_notification_projects(&kv).contains(&project));
    }

    #[test]
    fn derives_local_identity_immediately_from_the_normalized_path() {
        let kv = Kv::in_memory();
        assert_eq!(
            known_notification_project(&kv, "C:/Work/App"),
            Some(NotificationProject {
                id: "local:c:/work/app".into(),
                name: "App".into(),
                detail: "C:/Work/App".into(),
                kind: NotificationProjectKind::Local,
                paths: vec!["C:/Work/App".into()],
            })
        );
        assert_eq!(
            known_notification_project(&kv, "c:\\work\\app").map(|project| project.id),
            Some("local:c:/work/app".into())
        );
    }

    #[test]
    fn keeps_separate_checkout_and_worktree_paths_as_separate_projects() {
        let kv = Kv::in_memory();
        let ids: Vec<String> =
            known_notification_project_selection(&kv, &["/work/app", "/work/app-review"])
                .into_iter()
                .map(|project| project.id)
                .collect();
        assert_eq!(ids, ["local:/work/app", "local:/work/app-review"]);
    }

    #[test]
    fn includes_requested_paths_and_provider_only_catalog_projects() {
        let kv = Kv::in_memory();
        remember_notification_projects(
            &kv,
            &[
                NotificationProject {
                    id: "linear:project:roadmap".into(),
                    name: "Roadmap".into(),
                    detail: "Linear".into(),
                    kind: NotificationProjectKind::Linear,
                    paths: vec![],
                },
                NotificationProject {
                    id: "local:/old/path".into(),
                    name: "old".into(),
                    detail: "/old/path".into(),
                    kind: NotificationProjectKind::Local,
                    paths: vec!["/old/path".into()],
                },
            ],
        );
        let ids: Vec<String> = known_notification_project_selection(&kv, &["/work/app"])
            .into_iter()
            .map(|project| project.id)
            .collect();
        assert_eq!(ids, ["linear:project:roadmap", "local:/work/app"]);
    }

    #[test]
    fn uses_the_same_local_id_when_inbox_enriches_a_path_with_repository_metadata() {
        let kv = Kv::in_memory();
        let project = inbox_notification_project(&NotificationWorkItem {
            project_path: Some("/work/app".into()),
            ..NotificationWorkItem::new(
                InboxProvider::Github,
                "acme/app",
                "https://github.com/acme/app/pull/5",
            )
        });
        assert_eq!(project.id, "local:/work/app");
        assert_eq!(project.name, "acme/app");
        assert_eq!(project.kind, NotificationProjectKind::Repository);
        remember_notification_projects(&kv, std::slice::from_ref(&project));
        assert_eq!(known_notification_project(&kv, "/work/app"), Some(project));
    }

    #[test]
    fn keeps_provider_only_repository_and_linear_identities() {
        assert_eq!(
            inbox_notification_project(&NotificationWorkItem::new(
                InboxProvider::Github,
                "Acme/App",
                "git@github.com:Acme/App.git"
            ))
            .id,
            "repository:github.com/acme/app"
        );
        assert_eq!(
            inbox_notification_project(&NotificationWorkItem {
                project_id: Some("launch".into()),
                project_name: Some("Launch".into()),
                team_id: Some("product".into()),
                team_name: Some("Product".into()),
                ..NotificationWorkItem::new(
                    InboxProvider::Linear,
                    "PROD",
                    "https://linear.app/acme/issue/PROD-1"
                )
            })
            .id,
            "linear:project:launch"
        );
    }

    #[test]
    fn uses_a_new_catalog_version_so_old_git_derived_path_mappings_are_ignored() {
        let kv = Kv::in_memory();
        kv.set_item(
            "monocode.notificationProjects.v1",
            &serde_json::json!([{
                "id": "repository:github.com/old/app",
                "name": "old/app",
                "detail": "github.com",
                "kind": "repository",
                "paths": ["/work/app"],
            }])
            .to_string(),
        );
        assert_eq!(load_notification_projects(&kv), vec![]);
        assert_eq!(
            known_notification_project(&kv, "/work/app").map(|project| project.id),
            Some("local:/work/app".into())
        );
    }

    #[test]
    fn non_project_paths_have_no_identity() {
        for path in [
            "",
            "/",
            "~",
            "C:",
            "/Users/me",
            "/Applications/Foo.app/Contents",
        ] {
            assert!(!looks_like_project(path), "{path}");
        }
        assert!(looks_like_project("/Users/me/src/app"));
    }
}
