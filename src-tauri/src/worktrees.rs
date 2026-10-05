//! Tauri commands over `monocode_git::worktrees`.
use tauri::State;

use crate::harness::HarnessHost;
use crate::pty::PtyHost;
use crate::session_store::SessionStore;
use monocode_git::worktrees::{self, Worktree, WorktreeRemoval, Worktrees};

#[tauri::command(async)]
pub fn git_worktrees(cwd: String, store: State<'_, SessionStore>) -> Result<Worktrees, String> {
    worktrees::git_worktrees(cwd, || store.lock_conn())
}

#[tauri::command(async)]
pub fn git_worktree_check_remove(
    cwd: String,
    path: String,
    force: bool,
    terminals: State<'_, PtyHost>,
) -> Result<(), String> {
    worktrees::git_worktree_check_remove(cwd, path, force, |path| terminals.has_working_dir(path))
}

#[tauri::command(async)]
pub fn git_worktree_remove(
    cwd: String,
    path: String,
    force: bool,
    keep_sessions: Option<bool>,
    store: State<'_, SessionStore>,
    terminals: State<'_, PtyHost>,
    agents: State<'_, HarnessHost>,
) -> Result<WorktreeRemoval, String> {
    worktrees::git_worktree_remove(
        cwd,
        path,
        force,
        keep_sessions,
        |path| terminals.has_working_dir(path) || agents.has_working_dir(path),
        || store.lock_conn(),
    )
}

#[tauri::command(async)]
pub fn git_orchestration_worktree_remove(
    cwd: String,
    path: String,
    store: State<'_, SessionStore>,
    terminals: State<'_, PtyHost>,
    agents: State<'_, HarnessHost>,
) -> Result<WorktreeRemoval, String> {
    worktrees::git_orchestration_worktree_remove(
        cwd,
        path,
        |path| terminals.has_working_dir(path) || agents.has_working_dir(path),
        || store.lock_conn(),
    )
}

#[tauri::command(async)]
pub async fn git_worktree_create(
    cwd: String,
    branch: String,
    base: String,
    existing: bool,
) -> Result<Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        worktrees::git_worktree_create(cwd, branch, base, existing)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Create an isolated worker checkout with the lead checkout's current file
/// contents as its baseline. Reusing the deterministic branch makes a crash
/// between Git creation and run-state persistence recoverable.
#[tauri::command(async)]
pub async fn git_orchestration_worktree_create(
    cwd: String,
    branch: String,
) -> Result<Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        worktrees::git_orchestration_worktree_create(cwd, branch)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(async)]
pub async fn git_worktree_rename_branch(
    cwd: String,
    path: String,
    branch: String,
) -> Result<Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        worktrees::git_worktree_rename_branch(cwd, path, branch)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command(async)]
pub async fn git_orchestration_branch_remove(cwd: String, branch: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        worktrees::git_orchestration_branch_remove(cwd, branch)
    })
    .await
    .map_err(|error| error.to_string())?
}
