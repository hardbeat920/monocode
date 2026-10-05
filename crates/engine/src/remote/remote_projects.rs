//! Port of src/features/connections/model/remoteProjects.ts: rail projects
//! whose folder lives on a connected machine, saved under
//! `monocode.remote-projects.v2`.
//!
//! `remote_path` and `parse_remote_path` already live in
//! `monocode_layout::paths`; this module re-exports them.

use monocode_core::Extra;
use monocode_layout::paths::is_remote_project_path;
pub use monocode_layout::paths::{RemotePathParts, parse_remote_path, remote_path};
use monocode_remote::host::protocol::HostProject;
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A rail project whose folder lives on another machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProject {
    pub key: String,
    pub environment_id: String,
    /// The host's ID for this folder.
    pub project_id: String,
    /// The folder's path on the host.
    pub cwd: String,
    #[serde(flatten)]
    pub extra: Extra,
}

pub const KEY: &str = "monocode.remote-projects.v2";
/// `REMOTE_PROJECTS_CHANGED`.
pub const REMOTE_PROJECTS_CHANGED: &str = "monocode:remote-projects-changed";

/// `slashed`: every backslash becomes a slash.
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// `.replace(/\/+$/, "")`.
fn trim_trailing_slashes(path: &str) -> &str {
    path.trim_end_matches('/')
}

/// `remoteProjectKey`.
pub fn remote_project_key(environment_id: &str, cwd: &str) -> String {
    remote_path(environment_id, trim_trailing_slashes(&slashed(cwd)))
}

/// `readAll`: the saved projects as raw JSON, so entries this build does not
/// understand survive a write.
fn read_all(kv: &Kv) -> Map<String, Value> {
    let raw = kv.get_item(KEY).unwrap_or_else(|| "{}".into());
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(projects)) => projects,
        _ => Map::new(),
    }
}

fn project(value: &Value) -> Option<RemoteProject> {
    serde_json::from_value(value.clone()).ok()
}

/// `remoteProjectFor`: the remote project a rail path names.
pub fn remote_project_for(kv: &Kv, path: &str) -> Option<RemoteProject> {
    if !is_remote_project_path(path) {
        return None;
    }
    let slashed_path = slashed(path);
    let key = trim_trailing_slashes(&slashed_path);
    let projects = read_all(kv);
    if let Some(found) = projects.get(key).and_then(project) {
        return Some(found);
    }
    projects
        .values()
        .filter_map(project)
        .find(|project| remote_project_key(&project.environment_id, &project.cwd) == key)
}

/// `rememberRemoteProject`. The caller announces `REMOTE_PROJECTS_CHANGED`
/// (`RemoteConnections::remember_remote_project` does).
pub fn remember_remote_project(kv: &Kv, environment_id: &str, host: &HostProject) -> RemoteProject {
    let remote = RemoteProject {
        key: remote_project_key(environment_id, &host.cwd),
        environment_id: environment_id.to_string(),
        project_id: host.id.clone(),
        cwd: host.cwd.clone(),
        extra: Extra::new(),
    };
    let mut all = read_all(kv);
    if let Ok(value) = serde_json::to_value(&remote) {
        all.insert(remote.key.clone(), value);
        kv.set_item(KEY, &Value::Object(all).to_string());
    }
    remote
}

/// `remoteProjectsOn`: every saved project on one machine.
pub fn remote_projects_on(kv: &Kv, environment_id: &str) -> Vec<RemoteProject> {
    read_all(kv)
        .values()
        .filter_map(project)
        .filter(|project| project.environment_id == environment_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // remoteProjects.test.ts
    #[test]
    fn finds_a_saved_unc_project_through_its_corrected_remote_path() {
        let kv = Kv::in_memory();
        let legacy_key = "remote://env/server/share/repo";
        let saved = json!({
            "key": legacy_key,
            "environmentId": "env",
            "projectId": "project",
            "cwd": "\\\\server\\share\\repo",
        });
        kv.set_item(KEY, &json!({ legacy_key: saved }).to_string());
        let found =
            remote_project_for(&kv, &remote_path("env", "\\\\server\\share\\repo")).unwrap();
        assert_eq!(serde_json::to_value(found).unwrap(), saved);
        kv.remove_item(KEY);
    }

    #[test]
    fn remembers_projects_per_machine_and_keeps_other_entries() {
        let kv = Kv::in_memory();
        kv.set_item(KEY, &json!({ "other": { "future": true } }).to_string());
        let remote = remember_remote_project(
            &kv,
            "env",
            &HostProject {
                id: "p1".into(),
                cwd: "/home/me/repo/".into(),
                name: "repo".into(),
            },
        );
        assert_eq!(remote.key, "remote://env/home/me/repo");
        assert_eq!(
            remote_project_for(&kv, "remote://env/home/me/repo/"),
            Some(remote.clone())
        );
        assert_eq!(remote_projects_on(&kv, "env"), vec![remote]);
        assert!(remote_projects_on(&kv, "elsewhere").is_empty());
        assert_eq!(read_all(&kv)["other"], json!({ "future": true }));
        assert_eq!(remote_project_for(&kv, "/home/me/repo"), None);
    }
}
