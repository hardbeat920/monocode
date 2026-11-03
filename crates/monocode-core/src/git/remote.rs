//! Branch and worktree RPCs shared with the original remote project pickers.
use super::{Branch, Snapshot, Worktree};
use crate::remote::{default_connections, parse_remote_path, remote_path, Connections};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

#[derive(Deserialize)]
struct Project {
    id: String,
    cwd: String,
}
#[derive(Deserialize)]
struct Trees {
    worktrees: Vec<Worktree>,
}
#[derive(Deserialize)]
struct Branches {
    current: Option<String>,
    branches: Vec<String>,
    remotes: Vec<RemoteBranch>,
}
#[derive(Deserialize)]
struct RemoteBranch {
    name: String,
    remote: String,
}

struct Context {
    connections: Arc<Connections>,
    machine: String,
    project: String,
    environment: String,
    cwd: String,
}
impl Context {
    fn request(&self, method: &str, mut params: Value) -> Result<Value, String> {
        params["projectId"] = json!(self.project);
        params["cwd"] = json!(self.cwd);
        self.connections.request(&self.machine, method, params)
    }
    fn trees(&self) -> Result<Vec<Worktree>, String> {
        let listed: Trees = decode(self.request("git.worktrees", json!({}))?)?;
        Ok(listed
            .worktrees
            .into_iter()
            .map(|mut tree| {
                tree.path = remote_path(&self.environment, &tree.path);
                tree
            })
            .collect())
    }
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid remote Git response: {error}"))
}
fn context(cwd: &Path) -> Result<Context, String> {
    let remote = parse_remote_path(&cwd.to_string_lossy()).ok_or("Invalid remote working copy")?;
    let connections = default_connections()?;
    let machine = connections
        .machine_for_environment(&remote.environment_id)?
        .id;
    let projects: Vec<Project> =
        decode(connections.request(&machine, "projects.list", json!({}))?)?;
    let mut project = projects
        .iter()
        .find(|project| key(&project.cwd) == key(&remote.host_path))
        .map(|project| project.id.clone());
    // Worktrees commonly live beside, rather than under, their main checkout.
    if project.is_none() {
        for candidate in &projects {
            let listed =
                connections.request(&machine, "git.worktrees", json!({"projectId":candidate.id}));
            if let Ok(listed) = listed.and_then(decode::<Trees>) {
                if listed
                    .worktrees
                    .iter()
                    .any(|tree| !tree.missing && key(&tree.path) == key(&remote.host_path))
                {
                    project = Some(candidate.id.clone());
                    break;
                }
            }
        }
    }
    Ok(Context {
        connections,
        machine,
        project: project.ok_or("Choose an available worktree of this project")?,
        environment: remote.environment_id,
        cwd: remote.host_path,
    })
}
fn key(path: &str) -> String {
    crate::files::path_key(path)
}

pub(super) fn snapshot(cwd: &Path) -> Result<Snapshot, String> {
    let context = context(cwd)?;
    let branches: Branches = decode(context.request("git.branches", json!({}))?)?;
    let trees = context.trees()?;
    Ok(Snapshot {
        detached: branches.current.is_none(),
        current: branches.current,
        linked: trees
            .iter()
            .any(|tree| !tree.main && key(&tree.path) == key(&cwd.to_string_lossy())),
        branches: branches
            .branches
            .into_iter()
            .map(|name| Branch { name, remote: None })
            .chain(branches.remotes.into_iter().map(|branch| Branch {
                name: branch.name,
                remote: Some(branch.remote),
            }))
            .collect(),
        worktrees: trees,
    })
}
pub(super) fn worktrees(cwd: &Path) -> Result<Vec<Worktree>, String> {
    context(cwd)?.trees()
}
pub(super) fn checkout(cwd: &Path, branch: &Branch) -> Result<(), String> {
    context(cwd)?.request(
        "git.switch",
        json!({"branch":branch.name,"remote":branch.remote}),
    )?;
    Ok(())
}
pub(super) fn create_branch(cwd: &Path, branch: &str) -> Result<(), String> {
    context(cwd)?.request("git.createBranch", json!({"branch":branch}))?;
    Ok(())
}
pub(super) fn create_worktree(
    cwd: &Path,
    branch: &str,
    base: Option<&str>,
    existing: bool,
) -> Result<Worktree, String> {
    let context = context(cwd)?;
    let mut tree: Worktree = decode(context.request(
        "git.worktreeCreate",
        json!({"branch":branch,"base":base.unwrap_or("HEAD"),"existing":existing}),
    )?)?;
    tree.path = remote_path(&context.environment, &tree.path);
    Ok(tree)
}
