//! Project operations use the machine that owns the working directory.

use std::sync::Arc;

use monocode_engine::projects::backend::{
    ProjectLocation, ProjectsBackend, Worktree, WorktreeRemoval, Worktrees,
};
use monocode_engine::remote::client::RemoteClient;
use monocode_git::fs::{GitBranches, GitDiffIndex, GitDiffStats};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub struct AppProjectsBackend {
    local: Arc<dyn ProjectsBackend>,
    remote: RemoteClient,
}

impl AppProjectsBackend {
    pub fn new(local: Arc<dyn ProjectsBackend>, remote: RemoteClient) -> Self {
        Self { local, remote }
    }

    fn run<T: DeserializeOwned>(
        &self,
        command: &str,
        args: Value,
        local: impl FnOnce(&dyn ProjectsBackend) -> Result<T, String>,
    ) -> Result<T, String> {
        let args = args.as_object().expect("project command arguments");
        if let Some(run) = self.remote.invoke_workspace(command, args) {
            let result = smol::block_on(run)?;
            return serde_json::from_value(result).map_err(|error| error.to_string());
        }
        local(self.local.as_ref())
    }
}

impl ProjectsBackend for AppProjectsBackend {
    fn git_diff_index(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        self.run("git_diff_index", json!({"cwd": cwd}), |local| {
            local.git_diff_index(cwd)
        })
    }

    fn git_diff_stats(&self, cwd: &str) -> Result<GitDiffStats, String> {
        self.run("git_diff_stats", json!({"cwd": cwd}), |local| {
            local.git_diff_stats(cwd)
        })
    }

    fn git_branches(&self, cwd: &str) -> Result<GitBranches, String> {
        self.run("git_branches", json!({"cwd": cwd}), |local| {
            local.git_branches(cwd)
        })
    }

    fn git_worktrees(&self, cwd: &str) -> Result<Worktrees, String> {
        self.run("git_worktrees", json!({"cwd": cwd}), |local| {
            local.git_worktrees(cwd)
        })
    }

    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String> {
        self.run(
            "git_worktree_create",
            json!({"cwd": cwd, "branch": branch, "base": base, "existing": existing}),
            |local| local.git_worktree_create(cwd, branch, base, existing),
        )
    }

    fn git_orchestration_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        self.run(
            "git_orchestration_worktree_create",
            json!({"cwd": cwd, "branch": branch}),
            |local| local.git_orchestration_worktree_create(cwd, branch),
        )
    }

    fn git_worktree_rename_branch(
        &self,
        cwd: &str,
        path: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        self.run(
            "git_worktree_rename_branch",
            json!({"cwd": cwd, "path": path, "branch": branch}),
            |local| local.git_worktree_rename_branch(cwd, path, branch),
        )
    }

    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        self.run(
            "git_worktree_check_remove",
            json!({"cwd": cwd, "path": path, "force": force}),
            |local| local.git_worktree_check_remove(cwd, path, force),
        )
    }

    fn git_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
        force: bool,
        keep_sessions: bool,
    ) -> Result<WorktreeRemoval, String> {
        self.run(
            "git_worktree_remove",
            json!({"cwd": cwd, "path": path, "force": force, "keepSessions": keep_sessions}),
            |local| local.git_worktree_remove(cwd, path, force, keep_sessions),
        )
    }

    fn git_orchestration_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
    ) -> Result<WorktreeRemoval, String> {
        self.run(
            "git_orchestration_worktree_remove",
            json!({"cwd": cwd, "path": path}),
            |local| local.git_orchestration_worktree_remove(cwd, path),
        )
    }

    fn git_orchestration_branch_remove(&self, cwd: &str, branch: &str) -> Result<(), String> {
        self.run(
            "git_orchestration_branch_remove",
            json!({"cwd": cwd, "branch": branch}),
            |local| local.git_orchestration_branch_remove(cwd, branch),
        )
    }

    fn resolve_project_location(
        &self,
        path: &str,
        identity: Option<&str>,
    ) -> Result<Option<ProjectLocation>, String> {
        if monocode_layout::paths::is_remote_project_path(path) {
            return Ok(Some(ProjectLocation {
                path: path.to_string(),
                identity: String::new(),
            }));
        }
        self.local.resolve_project_location(path, identity)
    }

    fn save_project_logo(&self, project: &str, source_path: &str) -> Result<String, String> {
        self.local.save_project_logo(project, source_path)
    }

    fn forget_logo_file(&self, path: &str) -> Result<(), String> {
        self.local.forget_logo_file(path)
    }

    fn remove_project_logo(&self, project: &str) -> Result<(), String> {
        self.local.remove_project_logo(project)
    }

    fn save_chat_background(&self, source_path: &str) -> Result<String, String> {
        self.local.save_chat_background(source_path)
    }

    fn remove_chat_background(&self) -> Result<(), String> {
        self.local.remove_chat_background()
    }

    fn save_project_chat_background(
        &self,
        project: &str,
        source_path: &str,
    ) -> Result<String, String> {
        self.local
            .save_project_chat_background(project, source_path)
    }

    fn remove_project_chat_background(&self, project: &str) -> Result<(), String> {
        self.local.remove_project_chat_background(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_engine::projects::testing::FakeBackend;
    use monocode_engine::remote::{
        remote_commands,
        testing::{FakeTransport, machine},
    };

    #[test]
    fn disconnected_remote_operations_never_run_against_local_paths() {
        let local = FakeBackend::new();
        let remote = RemoteClient::new(Arc::new(FakeTransport::new()));
        remote.set_machines(Vec::new());
        let backend = AppProjectsBackend::new(local.clone(), remote);
        assert_eq!(
            backend.git_branches("remote://home/tmp/repo").unwrap_err(),
            remote_commands::NOT_CONNECTED
        );
        assert_eq!(
            backend
                .git_worktree_create("remote://home/tmp/repo", "topic", "main", false)
                .unwrap_err(),
            remote_commands::UNAVAILABLE
        );
        assert!(local.commands().is_empty());
    }

    #[test]
    fn remote_worktree_listing_translates_the_host_reply() {
        let local = FakeBackend::new();
        let transport = FakeTransport::new();
        transport.respond("workspace.run", json!({
            "worktrees": [{"path":"/tmp/repo", "branch":"main", "head":"abc", "isMain":true,
                "locked":false, "prunable":false, "missing":false, "dirty":false, "unpushed":0, "sessionIds":[]}],
            "defaultRoot":"/tmp"
        }));
        let remote = RemoteClient::new(Arc::new(transport));
        remote.set_machines(vec![machine("machine", "home")]);
        let backend = AppProjectsBackend::new(local.clone(), remote);
        let listed = backend.git_worktrees("remote://home/tmp/repo").unwrap();
        assert_eq!(listed.worktrees[0].path, "remote://home/tmp/repo");
        assert!(local.commands().is_empty());
        backend.git_branches("/tmp/local").unwrap();
        assert_eq!(local.commands(), vec!["git_branches"]);
    }
}
