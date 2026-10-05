//! The git calls the source control views make, behind [`GitBackend`].
//!
//! The React views called Tauri commands through src/platform/tauri/fs.ts.
//! Here every call blocks and runs on the background executor. Status that
//! several views share (the diff index, branches, the worktree list) comes
//! from the engine's `projects::GitStatus` entities instead, which own the
//! polling; those reads go through the engine's `ProjectsBackend`.
//!
//! [`LocalGit`] implements both traits over monocode-git.

use std::path::{Path, PathBuf};

use monocode_engine::projects::{ProjectLocation, ProjectsBackend};
use monocode_git::fs as gitfs;
use rusqlite::Connection;

pub use monocode_engine::projects::{Worktree, WorktreeRemoval, Worktrees};
pub use monocode_git::fs::{
    GitBranchEntry, GitBranches, GitChangedFile, GitDiffIndex, GitDiffStats, GitFileDiff,
    GitHistory, GitHistoryCommit, GitHistoryRef, GitHubWorkItem, GitPr, GitRangeContext,
};

/// `GitFileDiffKind`: which comparison a changed file opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GitFileDiffKind {
    /// HEAD against the index.
    Staged,
    /// The index against the working tree.
    Unstaged,
}

impl GitFileDiffKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
        }
    }
}

/// The blocking git calls the views make. Names and types follow the Tauri
/// commands and the engine's `ProjectsBackend`. Run them with
/// `cx.background_spawn`.
pub trait GitBackend: Send + Sync + 'static {
    /// `git_diff_files`: changed files and counts, without upstream state.
    fn git_diff_files(&self, cwd: &str) -> Result<GitDiffIndex, String>;
    /// `git_file_diff`: both sides of one changed file.
    fn git_file_diff(
        &self,
        cwd: &str,
        relative: &str,
        kind: GitFileDiffKind,
    ) -> Result<GitFileDiff, String>;
    /// `git_history`: recent commits for the graph, newest first.
    fn git_history(&self, cwd: &str) -> Result<GitHistory, String>;
    /// `git_commit_files`.
    fn git_commit_files(&self, cwd: &str, sha: &str) -> Result<Vec<GitChangedFile>, String>;
    /// `git_commit_file_diff`.
    fn git_commit_file_diff(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
    ) -> Result<GitFileDiff, String>;
    fn git_stage_file(&self, cwd: &str, relative: &str) -> Result<(), String>;
    /// `git_stage_contents`: write `contents` into the index for one path.
    fn git_stage_contents(&self, cwd: &str, relative: &str, contents: &str) -> Result<(), String>;
    fn git_unstage_file(&self, cwd: &str, relative: &str) -> Result<(), String>;
    fn git_discard_file(&self, cwd: &str, relative: &str) -> Result<(), String>;
    fn git_stage_all(&self, cwd: &str) -> Result<(), String>;
    fn git_unstage_all(&self, cwd: &str) -> Result<(), String>;
    fn git_discard_all(&self, cwd: &str) -> Result<(), String>;
    fn git_commit(&self, cwd: &str, message: &str, amend: bool) -> Result<(), String>;
    fn git_head_message(&self, cwd: &str) -> Result<String, String>;
    fn git_push(&self, cwd: &str) -> Result<(), String>;
    fn git_pull(&self, cwd: &str) -> Result<(), String>;
    fn git_sync(&self, cwd: &str) -> Result<(), String>;
    fn git_range_context(&self, cwd: &str) -> Result<GitRangeContext, String>;
    fn git_pr_status(&self, cwd: &str) -> Result<Option<GitPr>, String>;
    /// `git_pr_create`: returns the new pull request's URL.
    fn git_pr_create(
        &self,
        cwd: &str,
        title: &str,
        body: &str,
        base: &str,
        head: &str,
    ) -> Result<String, String>;
    /// `git_checkout`. `force` switches a connected machine's branch under
    /// running sessions; a local checkout ignores it.
    fn git_checkout(
        &self,
        cwd: &str,
        name: &str,
        remote: Option<&str>,
        force: bool,
    ) -> Result<String, String>;
    /// `git_create_branch`, with `force` as in [`Self::git_checkout`].
    fn git_create_branch(&self, cwd: &str, name: &str, force: bool) -> Result<String, String>;
    fn git_stash(&self, cwd: &str, message: Option<&str>) -> Result<(), String>;
    /// `git_worktree_create`, as in `ProjectsBackend`.
    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String>;
    /// `git_worktree_check_remove`, as in `ProjectsBackend`.
    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String>;
    /// `git_github_pr_action`: merge, squash, rebase, draft, ready, close,
    /// or reopen. Returns GitHub's fresh state.
    fn git_github_pr_action(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
        action: &str,
    ) -> Result<GitHubWorkItem, String>;
    /// `reveal_path`: show a folder in the file manager.
    fn reveal_path(&self, path: &str) -> Result<(), String>;
}

/// [`GitBackend`] and the git half of `ProjectsBackend` over monocode-git.
///
/// Worktree listings report the sessions that use each working copy when a
/// session database is set ([`LocalGit::with_session_db`]); otherwise they
/// report none.
#[derive(Clone, Debug, Default)]
pub struct LocalGit {
    session_db: Option<PathBuf>,
}

impl LocalGit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read worktree sessions from this `monocode.db`.
    pub fn with_session_db(path: impl Into<PathBuf>) -> Self {
        Self {
            session_db: Some(path.into()),
        }
    }

    fn sessions(&self) -> Result<Connection, String> {
        match &self.session_db {
            Some(path) => Connection::open(path).map_err(|e| e.to_string()),
            None => {
                let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
                conn.execute_batch(
                    "CREATE TABLE sessions (id TEXT, cwd TEXT, worktree_cwd TEXT, worktree_removed INTEGER DEFAULT 0)",
                )
                .map_err(|e| e.to_string())?;
                Ok(conn)
            }
        }
    }
}

const NOT_SUPPORTED: &str = "LocalGit only implements the git calls";

impl GitBackend for LocalGit {
    fn git_diff_files(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        Ok(gitfs::git_diff_files(cwd.into()))
    }

    fn git_file_diff(
        &self,
        cwd: &str,
        relative: &str,
        kind: GitFileDiffKind,
    ) -> Result<GitFileDiff, String> {
        gitfs::git_file_diff(cwd.into(), relative.into(), kind == GitFileDiffKind::Staged)
    }

    fn git_history(&self, cwd: &str) -> Result<GitHistory, String> {
        gitfs::git_history(cwd.into(), None)
    }

    fn git_commit_files(&self, cwd: &str, sha: &str) -> Result<Vec<GitChangedFile>, String> {
        gitfs::git_commit_files(cwd.into(), sha.into())
    }

    fn git_commit_file_diff(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
    ) -> Result<GitFileDiff, String> {
        gitfs::git_commit_file_diff(cwd.into(), sha.into(), relative.into())
    }

    fn git_stage_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        gitfs::git_stage_file(cwd.into(), relative.into())
    }

    fn git_stage_contents(&self, cwd: &str, relative: &str, contents: &str) -> Result<(), String> {
        gitfs::git_stage_contents(cwd.into(), relative.into(), contents.into())
    }

    fn git_unstage_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        gitfs::git_unstage_file(cwd.into(), relative.into())
    }

    fn git_discard_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        gitfs::git_discard_file(cwd.into(), relative.into())
    }

    fn git_stage_all(&self, cwd: &str) -> Result<(), String> {
        gitfs::git_stage_all(cwd.into())
    }

    fn git_unstage_all(&self, cwd: &str) -> Result<(), String> {
        gitfs::git_unstage_all(cwd.into())
    }

    fn git_discard_all(&self, cwd: &str) -> Result<(), String> {
        gitfs::git_discard_all(cwd.into())
    }

    fn git_commit(&self, cwd: &str, message: &str, amend: bool) -> Result<(), String> {
        gitfs::git_commit(cwd.into(), message.into(), amend)
    }

    fn git_head_message(&self, cwd: &str) -> Result<String, String> {
        gitfs::git_head_message(cwd.into())
    }

    fn git_push(&self, cwd: &str) -> Result<(), String> {
        gitfs::git_push(cwd.into())
    }

    fn git_pull(&self, cwd: &str) -> Result<(), String> {
        gitfs::git_pull(cwd.into())
    }

    fn git_sync(&self, cwd: &str) -> Result<(), String> {
        gitfs::git_sync(cwd.into())
    }

    fn git_range_context(&self, cwd: &str) -> Result<GitRangeContext, String> {
        gitfs::git_range_context(cwd.into())
    }

    fn git_pr_status(&self, cwd: &str) -> Result<Option<GitPr>, String> {
        gitfs::git_pr_status(cwd.into())
    }

    fn git_pr_create(
        &self,
        cwd: &str,
        title: &str,
        body: &str,
        base: &str,
        head: &str,
    ) -> Result<String, String> {
        gitfs::git_pr_create(
            cwd.into(),
            title.into(),
            body.into(),
            base.into(),
            head.into(),
        )
    }

    fn git_checkout(
        &self,
        cwd: &str,
        name: &str,
        remote: Option<&str>,
        _force: bool,
    ) -> Result<String, String> {
        gitfs::git_checkout(cwd.into(), name.into(), remote.map(str::to_string))
    }

    fn git_create_branch(&self, cwd: &str, name: &str, _force: bool) -> Result<String, String> {
        gitfs::git_create_branch(cwd.into(), name.into())
    }

    fn git_stash(&self, cwd: &str, message: Option<&str>) -> Result<(), String> {
        gitfs::git_stash(cwd.into(), message.map(str::to_string))
    }

    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String> {
        monocode_git::worktrees::git_worktree_create(
            cwd.into(),
            branch.into(),
            base.into(),
            existing,
        )
        .map(Worktree::from)
    }

    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        monocode_git::worktrees::git_worktree_check_remove(cwd.into(), path.into(), force, |_| {
            false
        })
    }

    fn git_github_pr_action(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
        action: &str,
    ) -> Result<GitHubWorkItem, String> {
        gitfs::git_github_pr_action(cwd.into(), repo.into(), number, action.into())
    }

    fn reveal_path(&self, path: &str) -> Result<(), String> {
        gitfs::reveal_path(path.into())
    }
}

impl ProjectsBackend for LocalGit {
    fn git_diff_index(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        Ok(gitfs::git_diff_index(cwd.into()))
    }

    fn git_diff_stats(&self, cwd: &str) -> Result<GitDiffStats, String> {
        Ok(gitfs::git_diff_stats(cwd.into()))
    }

    fn git_branches(&self, cwd: &str) -> Result<GitBranches, String> {
        gitfs::git_branches(cwd.into())
    }

    fn git_worktrees(&self, cwd: &str) -> Result<Worktrees, String> {
        let listed =
            monocode_git::worktrees::git_worktrees(cwd.into(), || self.sessions().map(Box::new))?;
        Ok(Worktrees {
            worktrees: listed.worktrees.into_iter().map(Worktree::from).collect(),
            default_root: listed.default_root,
        })
    }

    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String> {
        GitBackend::git_worktree_create(self, cwd, branch, base, existing)
    }

    fn git_orchestration_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        monocode_git::worktrees::git_orchestration_worktree_create(cwd.into(), branch.into())
            .map(Worktree::from)
    }

    fn git_worktree_rename_branch(
        &self,
        cwd: &str,
        path: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        monocode_git::worktrees::git_worktree_rename_branch(cwd.into(), path.into(), branch.into())
            .map(Worktree::from)
    }

    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        GitBackend::git_worktree_check_remove(self, cwd, path, force)
    }

    fn git_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
        force: bool,
        keep_sessions: bool,
    ) -> Result<WorktreeRemoval, String> {
        let removal = monocode_git::worktrees::git_worktree_remove(
            cwd.into(),
            path.into(),
            force,
            Some(keep_sessions),
            |_: &Path| false,
            || self.sessions().map(Box::new),
        )?;
        let value = serde_json::to_value(removal).map_err(|e| e.to_string())?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    fn git_orchestration_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
    ) -> Result<WorktreeRemoval, String> {
        let removal = monocode_git::worktrees::git_orchestration_worktree_remove(
            cwd.into(),
            path.into(),
            |_: &Path| false,
            || self.sessions().map(Box::new),
        )?;
        let value = serde_json::to_value(removal).map_err(|e| e.to_string())?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    fn git_orchestration_branch_remove(&self, cwd: &str, branch: &str) -> Result<(), String> {
        monocode_git::worktrees::git_orchestration_branch_remove(cwd.into(), branch.into())
    }

    fn resolve_project_location(
        &self,
        _path: &str,
        _identity: Option<&str>,
    ) -> Result<Option<ProjectLocation>, String> {
        Err(NOT_SUPPORTED.into())
    }

    fn save_project_logo(&self, _project: &str, _source_path: &str) -> Result<String, String> {
        Err(NOT_SUPPORTED.into())
    }

    fn forget_logo_file(&self, _path: &str) -> Result<(), String> {
        Err(NOT_SUPPORTED.into())
    }

    fn remove_project_logo(&self, _project: &str) -> Result<(), String> {
        Err(NOT_SUPPORTED.into())
    }

    fn save_chat_background(&self, _source_path: &str) -> Result<String, String> {
        Err(NOT_SUPPORTED.into())
    }

    fn remove_chat_background(&self) -> Result<(), String> {
        Err(NOT_SUPPORTED.into())
    }

    fn save_project_chat_background(
        &self,
        _project: &str,
        _source_path: &str,
    ) -> Result<String, String> {
        Err(NOT_SUPPORTED.into())
    }

    fn remove_project_chat_background(&self, _project: &str) -> Result<(), String> {
        Err(NOT_SUPPORTED.into())
    }
}

/// `isSwitchBlockedByRunningSessions`: a connected machine refused a branch
/// change because sessions are running there.
pub fn is_switch_blocked_by_running_sessions(message: &str) -> bool {
    message
        .to_lowercase()
        .contains("switching branches changes the files")
}

/// `isCheckoutBlockedByChanges`: git refused a checkout because the working
/// tree would be overwritten.
pub fn is_checkout_blocked_by_changes(message: &str) -> bool {
    let text = message.to_lowercase();
    text.contains("would be overwritten")
        || text.contains("commit your changes or stash")
        || text.contains("please move or remove them before")
}
