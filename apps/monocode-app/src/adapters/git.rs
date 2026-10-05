//! Git calls run on the connected host when their paths use remote://.
use gpui::App;
use monocode_engine::remote::{RemoteGlobal, client::RemoteClient};
use monocode_view_scm::git::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub struct AppGit {
    local: LocalGit,
    remote: Option<RemoteClient>,
}
impl AppGit {
    pub fn new(cx: &App) -> Self {
        Self {
            local: LocalGit::new(),
            remote: RemoteGlobal::try_global(cx).map(|remote| remote.client.clone()),
        }
    }
    fn run<T: DeserializeOwned>(
        &self,
        command: &str,
        args: Value,
        local: impl FnOnce(&LocalGit) -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(run) = self.remote.as_ref().and_then(|client| {
            client.invoke_workspace(command, &args.as_object().cloned().unwrap_or_default())
        }) {
            return smol::block_on(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            });
        }
        local(&self.local)
    }
}
impl GitBackend for AppGit {
    fn git_diff_files(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        self.run("git_diff_files", json!({ "cwd": cwd }), |local| {
            local.git_diff_files(cwd)
        })
    }
    fn git_file_diff(
        &self,
        cwd: &str,
        relative: &str,
        kind: GitFileDiffKind,
    ) -> Result<GitFileDiff, String> {
        self.run(
            "git_file_diff",
            json!({ "cwd": cwd, "relative": relative, "staged": kind == GitFileDiffKind::Staged }),
            |local| local.git_file_diff(cwd, relative, kind),
        )
    }
    fn git_history(&self, cwd: &str) -> Result<GitHistory, String> {
        self.run("git_history", json!({ "cwd": cwd }), |local| {
            local.git_history(cwd)
        })
    }
    fn git_commit_files(&self, cwd: &str, sha: &str) -> Result<Vec<GitChangedFile>, String> {
        self.run(
            "git_commit_files",
            json!({ "cwd": cwd, "sha": sha }),
            |local| local.git_commit_files(cwd, sha),
        )
    }
    fn git_commit_file_diff(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
    ) -> Result<GitFileDiff, String> {
        self.run(
            "git_commit_file_diff",
            json!({ "cwd": cwd, "sha": sha, "relative": relative }),
            |local| local.git_commit_file_diff(cwd, sha, relative),
        )
    }
    fn git_stage_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        self.run(
            "git_stage_file",
            json!({ "cwd": cwd, "relative": relative }),
            |local| local.git_stage_file(cwd, relative),
        )
    }
    fn git_stage_contents(&self, cwd: &str, relative: &str, contents: &str) -> Result<(), String> {
        self.run(
            "git_stage_contents",
            json!({ "cwd": cwd, "relative": relative, "contents": contents }),
            |local| local.git_stage_contents(cwd, relative, contents),
        )
    }
    fn git_unstage_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        self.run(
            "git_unstage_file",
            json!({ "cwd": cwd, "relative": relative }),
            |local| local.git_unstage_file(cwd, relative),
        )
    }
    fn git_discard_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        self.run(
            "git_discard_file",
            json!({ "cwd": cwd, "relative": relative }),
            |local| local.git_discard_file(cwd, relative),
        )
    }
    fn git_stage_all(&self, cwd: &str) -> Result<(), String> {
        self.run("git_stage_all", json!({ "cwd": cwd }), |local| {
            local.git_stage_all(cwd)
        })
    }
    fn git_unstage_all(&self, cwd: &str) -> Result<(), String> {
        self.run("git_unstage_all", json!({ "cwd": cwd }), |local| {
            local.git_unstage_all(cwd)
        })
    }
    fn git_discard_all(&self, cwd: &str) -> Result<(), String> {
        self.run("git_discard_all", json!({ "cwd": cwd }), |local| {
            local.git_discard_all(cwd)
        })
    }
    fn git_commit(&self, cwd: &str, message: &str, amend: bool) -> Result<(), String> {
        self.run(
            "git_commit",
            json!({ "cwd": cwd, "message": message, "amend": amend }),
            |local| local.git_commit(cwd, message, amend),
        )
    }
    fn git_head_message(&self, cwd: &str) -> Result<String, String> {
        self.run("git_head_message", json!({ "cwd": cwd }), |local| {
            local.git_head_message(cwd)
        })
    }
    fn git_push(&self, cwd: &str) -> Result<(), String> {
        self.run("git_push", json!({ "cwd": cwd }), |local| {
            local.git_push(cwd)
        })
    }
    fn git_pull(&self, cwd: &str) -> Result<(), String> {
        self.run("git_pull", json!({ "cwd": cwd }), |local| {
            local.git_pull(cwd)
        })
    }
    fn git_sync(&self, cwd: &str) -> Result<(), String> {
        self.run("git_sync", json!({ "cwd": cwd }), |local| {
            local.git_sync(cwd)
        })
    }
    fn git_range_context(&self, cwd: &str) -> Result<GitRangeContext, String> {
        self.run("git_range_context", json!({ "cwd": cwd }), |local| {
            local.git_range_context(cwd)
        })
    }
    fn git_pr_status(&self, cwd: &str) -> Result<Option<GitPr>, String> {
        self.run("git_pr_status", json!({ "cwd": cwd }), |local| {
            local.git_pr_status(cwd)
        })
    }
    fn git_pr_create(
        &self,
        cwd: &str,
        title: &str,
        body: &str,
        base: &str,
        head: &str,
    ) -> Result<String, String> {
        self.run(
            "git_pr_create",
            json!({ "cwd": cwd, "title": title, "body": body, "base": base, "head": head }),
            |local| local.git_pr_create(cwd, title, body, base, head),
        )
    }
    fn git_checkout(
        &self,
        cwd: &str,
        name: &str,
        remote: Option<&str>,
        force: bool,
    ) -> Result<String, String> {
        self.run(
            "git_checkout",
            json!({ "cwd": cwd, "name": name, "remote": remote, "force": force }),
            |local| local.git_checkout(cwd, name, remote, force),
        )
    }
    fn git_create_branch(&self, cwd: &str, name: &str, force: bool) -> Result<String, String> {
        self.run(
            "git_create_branch",
            json!({ "cwd": cwd, "name": name, "force": force }),
            |local| local.git_create_branch(cwd, name, force),
        )
    }
    fn git_stash(&self, cwd: &str, message: Option<&str>) -> Result<(), String> {
        self.run(
            "git_stash",
            json!({ "cwd": cwd, "message": message }),
            |local| local.git_stash(cwd, message),
        )
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
            json!({ "cwd": cwd, "branch": branch, "base": base, "existing": existing }),
            |local| local.git_worktree_create(cwd, branch, base, existing),
        )
    }
    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        self.run(
            "git_worktree_check_remove",
            json!({ "cwd": cwd, "path": path, "force": force }),
            |local| local.git_worktree_check_remove(cwd, path, force),
        )
    }
    fn git_github_pr_action(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
        action: &str,
    ) -> Result<GitHubWorkItem, String> {
        self.run(
            "git_github_pr_action",
            json!({ "cwd": cwd, "repo": repo, "number": number, "action": action }),
            |local| local.git_github_pr_action(cwd, repo, number, action),
        )
    }
    fn reveal_path(&self, path: &str) -> Result<(), String> {
        self.run("reveal_path", json!({ "path": path }), |local| {
            local.reveal_path(path)
        })
    }
}
