//! The blocking calls this package makes: git status, branches, worktrees,
//! project folder identity, and the images MonoCode copies into its data
//! directory. They were Tauri commands; here they sit behind
//! `ProjectsBackend`, and callers run them on the background executor.
//!
//! `LocalProjectsBackend` calls `monocode_git` directly. Tests use the fake
//! in `projects::testing`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use monocode_git::fs::{GitBranches, GitDiffIndex, GitDiffStats};
use monocode_store::session_store::SessionStore;
use serde::{Deserialize, Serialize};

/// `ProjectLocation` from src/platform/tauri/fs.ts: a folder and its
/// filesystem identity, which survives a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLocation {
    pub path: String,
    pub identity: String,
}

/// `Worktree` from src/features/source-control/model/worktrees.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub locked: bool,
    pub prunable: bool,
    pub missing: bool,
    pub dirty: Option<bool>,
    pub unpushed: Option<i64>,
    pub session_ids: Vec<String>,
}

impl Worktree {
    /// A linked worktree with a branch and nothing else set.
    pub fn new(path: impl Into<String>, branch: Option<&str>) -> Self {
        Self {
            path: path.into(),
            branch: branch.map(str::to_string),
            head: String::new(),
            is_main: false,
            locked: false,
            prunable: false,
            missing: false,
            dirty: None,
            unpushed: None,
            session_ids: Vec::new(),
        }
    }
}

impl From<monocode_git::worktrees::Worktree> for Worktree {
    fn from(tree: monocode_git::worktrees::Worktree) -> Self {
        Self {
            path: tree.path,
            branch: tree.branch,
            head: tree.head,
            is_main: tree.is_main,
            locked: tree.locked,
            prunable: tree.prunable,
            missing: tree.missing,
            dirty: tree.dirty,
            unpushed: tree.unpushed.map(|count| count as i64),
            session_ids: tree.session_ids,
        }
    }
}

/// `Worktrees`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktrees {
    pub worktrees: Vec<Worktree>,
    pub default_root: String,
}

/// What `git_worktree_remove` returns: the sessions that used the removed
/// working copy and the repository they now belong to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemoval {
    pub session_ids: Vec<String>,
    pub project_cwd: String,
}

/// The git, filesystem, and app-data calls the projects package makes. Every
/// method blocks; run them with `cx.background_spawn`.
pub trait ProjectsBackend: Send + Sync + 'static {
    /// `git_diff_index`: changed files with branch and upstream state.
    fn git_diff_index(&self, cwd: &str) -> Result<GitDiffIndex, String>;
    /// `git_diff_stats`: uncommitted line counts.
    fn git_diff_stats(&self, cwd: &str) -> Result<GitDiffStats, String>;
    /// `git_branches`.
    fn git_branches(&self, cwd: &str) -> Result<GitBranches, String>;
    /// `git_worktrees`, with the sessions that use each working copy.
    fn git_worktrees(&self, cwd: &str) -> Result<Worktrees, String>;
    /// `git_worktree_create`.
    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String>;
    /// `git_orchestration_worktree_create`.
    fn git_orchestration_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<Worktree, String>;
    /// `git_worktree_rename_branch`.
    fn git_worktree_rename_branch(
        &self,
        cwd: &str,
        path: &str,
        branch: &str,
    ) -> Result<Worktree, String>;
    /// `git_worktree_check_remove`: the read-only preflight.
    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String>;
    /// `git_worktree_remove`.
    fn git_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
        force: bool,
        keep_sessions: bool,
    ) -> Result<WorktreeRemoval, String>;
    /// `git_orchestration_worktree_remove`.
    fn git_orchestration_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
    ) -> Result<WorktreeRemoval, String>;
    /// `git_orchestration_branch_remove`.
    fn git_orchestration_branch_remove(&self, cwd: &str, branch: &str) -> Result<(), String>;
    /// `resolve_project_location`: the folder now, following a rename among
    /// its siblings when `identity` is known.
    fn resolve_project_location(
        &self,
        path: &str,
        identity: Option<&str>,
    ) -> Result<Option<ProjectLocation>, String>;
    /// `save_project_logo`: copy an image into app data; returns its path.
    fn save_project_logo(&self, project: &str, source_path: &str) -> Result<String, String>;
    /// `forget_logo_file`.
    fn forget_logo_file(&self, path: &str) -> Result<(), String>;
    /// `remove_project_logo`.
    fn remove_project_logo(&self, project: &str) -> Result<(), String>;
    /// `save_chat_background`.
    fn save_chat_background(&self, source_path: &str) -> Result<String, String>;
    /// `remove_chat_background`.
    fn remove_chat_background(&self) -> Result<(), String>;
    /// `save_project_chat_background`.
    fn save_project_chat_background(
        &self,
        project: &str,
        source_path: &str,
    ) -> Result<String, String>;
    /// `remove_project_chat_background`.
    fn remove_project_chat_background(&self, project: &str) -> Result<(), String>;
}

/// Is a working directory in use by a terminal or an agent process.
pub type InUse = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// `ProjectsBackend` on this machine.
pub struct LocalProjectsBackend {
    data_dir: PathBuf,
    store: Arc<SessionStore>,
    /// `terminals.has_working_dir`: blocks the removal preflight.
    terminals_in_use: InUse,
    /// `terminals.has_working_dir || agents.has_working_dir`: blocks removal.
    in_use: InUse,
}

impl LocalProjectsBackend {
    /// `data_dir` is the app data directory; `store` records the sessions
    /// each working copy holds. `terminals_in_use` and `in_use` answer for
    /// the PTY host and the agent processes.
    pub fn new(
        data_dir: PathBuf,
        store: Arc<SessionStore>,
        terminals_in_use: InUse,
        in_use: InUse,
    ) -> Self {
        Self {
            data_dir,
            store,
            terminals_in_use,
            in_use,
        }
    }
}

/// The private-field structs in `monocode_git` only serialize; read them
/// back through their JSON.
fn from_json<T: serde::de::DeserializeOwned>(value: impl Serialize) -> Result<T, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value::<T>)
        .map_err(|error| error.to_string())
}

impl ProjectsBackend for LocalProjectsBackend {
    fn git_diff_index(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        Ok(monocode_git::fs::git_diff_index(cwd.to_string()))
    }

    fn git_diff_stats(&self, cwd: &str) -> Result<GitDiffStats, String> {
        Ok(monocode_git::fs::git_diff_stats(cwd.to_string()))
    }

    fn git_branches(&self, cwd: &str) -> Result<GitBranches, String> {
        monocode_git::fs::git_branches(cwd.to_string())
    }

    fn git_worktrees(&self, cwd: &str) -> Result<Worktrees, String> {
        let listed =
            monocode_git::worktrees::git_worktrees(cwd.to_string(), || self.store.lock_conn())?;
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
        monocode_git::worktrees::git_worktree_create(
            cwd.to_string(),
            branch.to_string(),
            base.to_string(),
            existing,
        )
        .map(Worktree::from)
    }

    fn git_orchestration_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        monocode_git::worktrees::git_orchestration_worktree_create(
            cwd.to_string(),
            branch.to_string(),
        )
        .map(Worktree::from)
    }

    fn git_worktree_rename_branch(
        &self,
        cwd: &str,
        path: &str,
        branch: &str,
    ) -> Result<Worktree, String> {
        monocode_git::worktrees::git_worktree_rename_branch(
            cwd.to_string(),
            path.to_string(),
            branch.to_string(),
        )
        .map(Worktree::from)
    }

    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        let terminals = self.terminals_in_use.clone();
        monocode_git::worktrees::git_worktree_check_remove(
            cwd.to_string(),
            path.to_string(),
            force,
            |path| terminals(path),
        )
    }

    fn git_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
        force: bool,
        keep_sessions: bool,
    ) -> Result<WorktreeRemoval, String> {
        let in_use = self.in_use.clone();
        let removed = monocode_git::worktrees::git_worktree_remove(
            cwd.to_string(),
            path.to_string(),
            force,
            Some(keep_sessions),
            |path| in_use(path),
            || self.store.lock_conn(),
        )?;
        from_json(removed)
    }

    fn git_orchestration_worktree_remove(
        &self,
        cwd: &str,
        path: &str,
    ) -> Result<WorktreeRemoval, String> {
        let in_use = self.in_use.clone();
        let removed = monocode_git::worktrees::git_orchestration_worktree_remove(
            cwd.to_string(),
            path.to_string(),
            |path| in_use(path),
            || self.store.lock_conn(),
        )?;
        from_json(removed)
    }

    fn git_orchestration_branch_remove(&self, cwd: &str, branch: &str) -> Result<(), String> {
        monocode_git::worktrees::git_orchestration_branch_remove(
            cwd.to_string(),
            branch.to_string(),
        )
    }

    fn resolve_project_location(
        &self,
        path: &str,
        identity: Option<&str>,
    ) -> Result<Option<ProjectLocation>, String> {
        let location = monocode_git::fs::resolve_project_location(
            path.to_string(),
            identity.map(str::to_string),
        )?;
        location.map(from_json::<ProjectLocation>).transpose()
    }

    fn save_project_logo(&self, project: &str, source_path: &str) -> Result<String, String> {
        monocode_git::project_logo::save_project_logo(
            &self.data_dir,
            project.to_string(),
            source_path.to_string(),
        )
    }

    fn forget_logo_file(&self, path: &str) -> Result<(), String> {
        monocode_git::project_logo::forget_logo_file(&self.data_dir, path.to_string())
    }

    fn remove_project_logo(&self, project: &str) -> Result<(), String> {
        monocode_git::project_logo::remove_project_logo(&self.data_dir, project.to_string())
    }

    fn save_chat_background(&self, source_path: &str) -> Result<String, String> {
        monocode_git::chat_background::save_chat_background(&self.data_dir, source_path.to_string())
    }

    fn remove_chat_background(&self) -> Result<(), String> {
        monocode_git::chat_background::remove_chat_background(&self.data_dir)
    }

    fn save_project_chat_background(
        &self,
        project: &str,
        source_path: &str,
    ) -> Result<String, String> {
        monocode_git::chat_background::save_project_chat_background(
            &self.data_dir,
            project.to_string(),
            source_path.to_string(),
        )
    }

    fn remove_project_chat_background(&self, project: &str) -> Result<(), String> {
        monocode_git::chat_background::remove_project_chat_background(
            &self.data_dir,
            project.to_string(),
        )
    }
}
