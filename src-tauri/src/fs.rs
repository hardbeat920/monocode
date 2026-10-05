//! Tauri commands over `monocode_git::fs`.
use std::collections::HashMap;

use tauri::AppHandle;

use monocode_git::fs as git_fs;
use monocode_git::fs::*;

/// Recover Bash commands that older UI builds saved as a bare "Shell" row.
/// Claude's own transcript retains the complete tool input by tool-use id.
#[tauri::command(async)]
pub fn claude_shell_commands(
    app: AppHandle,
    provider_session_id: String,
    provider_account_id: Option<String>,
    tool_ids: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    git_fs::claude_shell_commands(
        &crate::app_data_dir(&app)?,
        provider_session_id,
        provider_account_id,
        tool_ids,
    )
}

/// Returns an `ipc::Response`, which reaches the webview as an ArrayBuffer, so
/// previews skip the 33% base64 inflation that inline attachments pay. The
/// caller decides what the bytes are by sniffing them; this only guards size.
#[tauri::command]
pub async fn read_binary_file(path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || git_fs::read_binary_file(path))
        .await
        .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn save_generated_image(
    app: AppHandle,
    data: String,
    name: String,
) -> Result<GeneratedImageAsset, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::save_generated_image(&data_dir, data, name)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_generated_images(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || git_fs::delete_generated_images(&data_dir, paths))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_path_with_default_app(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::open_path_with_default_app(path))
        .await
        .map_err(|error| error.to_string())?
}

/// Resolve a project by filesystem identity when its directory was renamed.
///
/// A rename preserves the directory identity. We only inspect direct siblings
/// of the missing path, which keeps this bounded and avoids a filesystem watch
/// or a broad disk search.
#[tauri::command(async)]
pub fn resolve_project_location(
    path: String,
    identity: Option<String>,
) -> Result<Option<ProjectLocation>, String> {
    git_fs::resolve_project_location(path, identity)
}

/// Recover displayed OMP custom messages that older MonoCode builds omitted
/// from their persisted transcript. The provider id is already stored with the
/// session; matching the original JSONL keeps the repair deterministic instead
/// of guessing from neighbouring reasoning text.
#[tauri::command(async)]
pub fn omp_session_interjections(
    provider_session_id: String,
) -> Result<Vec<OmpInterjectionAnchor>, String> {
    git_fs::omp_session_interjections(provider_session_id)
}

#[tauri::command(async)]
pub fn omp_active_assistant_texts(
    provider_session_id: String,
) -> Result<Vec<OmpAssistantText>, String> {
    git_fs::omp_active_assistant_texts(provider_session_id)
}

/// Immediate children of `path` (project tree). Folders first, then files.
#[tauri::command(async)]
pub fn list_dir(path: String) -> Result<Vec<DirEntry>, String> {
    git_fs::list_dir(path)
}

/// Workspace files for Quick Open. Prefer `git ls-files` (gitignore-aware,
/// index-backed); otherwise a bounded walk that never descends into vendor dirs.
#[tauri::command]
pub async fn list_project_files(cwd: String) -> Result<Vec<ProjectFile>, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::list_project_files(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Uncommitted line counts for the opened folder: staged + unstaged vs HEAD,
/// plus untracked (gitignore-aware) files counted as additions.
#[tauri::command]
pub async fn git_diff_stats(cwd: String) -> Result<GitDiffStats, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_diff_stats(cwd))
        .await
        .map_err(|e| e.to_string())
}

/// Changed files in the opened folder, with per-file line counts and status.
#[tauri::command]
pub async fn git_diff_index(cwd: String) -> Result<GitDiffIndex, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_diff_index(cwd))
        .await
        .map_err(|e| e.to_string())
}

/// Changed files and counts without branch/upstream synchronization metadata.
#[tauri::command]
pub async fn git_diff_files(cwd: String) -> Result<GitDiffIndex, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_diff_files(cwd))
        .await
        .map_err(|e| e.to_string())
}

/// Contents for one changed file. Staged diffs compare HEAD to the index;
/// unstaged diffs compare the index to the working tree.
#[tauri::command]
pub async fn git_file_diff(
    cwd: String,
    relative: String,
    staged: bool,
) -> Result<GitFileDiff, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_file_diff(cwd, relative, staged))
        .await
        .map_err(|e| e.to_string())?
}

/// Recent commits for the Graph view: HEAD, upstream, and the default
/// branch. Newest first, with parent SHAs for the graph.
#[tauri::command]
pub async fn git_history(cwd: String, limit: Option<u32>) -> Result<GitHistory, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_history(cwd, limit))
        .await
        .map_err(|e| e.to_string())?
}

/// Files changed in one commit (first parent / root).
#[tauri::command]
pub async fn git_commit_files(cwd: String, sha: String) -> Result<Vec<GitChangedFile>, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_commit_files(cwd, sha))
        .await
        .map_err(|e| e.to_string())?
}

/// Parent vs commit contents for one path in a historical commit.
#[tauri::command]
pub async fn git_commit_file_diff(
    cwd: String,
    sha: String,
    relative: String,
) -> Result<GitFileDiff, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_commit_file_diff(cwd, sha, relative))
        .await
        .map_err(|e| e.to_string())?
}

/// Stage a changed file (`git add`).
#[tauri::command]
pub async fn git_stage_file(cwd: String, relative: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_stage_file(cwd, relative))
        .await
        .map_err(|e| e.to_string())?
}

/// Write `contents` into the index for one path, leaving the working tree alone.
#[tauri::command]
pub async fn git_stage_contents(
    cwd: String,
    relative: String,
    contents: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_stage_contents(cwd, relative, contents)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Unstage a file (`git restore --staged`).
#[tauri::command]
pub async fn git_unstage_file(cwd: String, relative: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_unstage_file(cwd, relative))
        .await
        .map_err(|e| e.to_string())?
}

/// Discard uncommitted changes so the file matches HEAD (or delete if untracked).
#[tauri::command]
pub async fn git_discard_file(cwd: String, relative: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_discard_file(cwd, relative))
        .await
        .map_err(|e| e.to_string())?
}

/// Discard every unstaged change (restore tracked files; delete untracked).
#[tauri::command]
pub async fn git_discard_all(cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_discard_all(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Stage every changed file in the repo.
#[tauri::command]
pub async fn git_stage_all(cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_stage_all(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Unstage every staged file.
#[tauri::command]
pub async fn git_unstage_all(cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_unstage_all(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Staged diff (or unstaged vs HEAD if nothing is staged) for commit text generation.
#[tauri::command]
pub async fn git_staged_context(cwd: String) -> Result<GitStagedContext, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_staged_context(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Create a commit from the current index, or rewrite HEAD with it when `amend` is set.
#[tauri::command]
pub async fn git_commit(cwd: String, message: String, amend: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_commit(cwd, message, amend))
        .await
        .map_err(|e| e.to_string())?
}

/// Full message (subject and body) of the commit at HEAD.
#[tauri::command]
pub async fn git_head_message(cwd: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_head_message(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Push the current branch to its upstream, or set upstream on first push.
#[tauri::command]
pub async fn git_push(cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_push(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Fast-forward the current branch from its upstream.
#[tauri::command]
pub async fn git_pull(cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_pull(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Pull incoming commits, then push local commits.
#[tauri::command]
pub async fn git_sync(cwd: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_sync(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Commits and diff between the default branch and HEAD, for PR text generation.
#[tauri::command]
pub async fn git_range_context(cwd: String) -> Result<GitRangeContext, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_range_context(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Latest pull request for the current branch, if `gh` can see one.
#[tauri::command]
pub async fn git_pr_status(cwd: String) -> Result<Option<GitPr>, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_pr_status(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Create a GitHub pull request with `gh` and return its URL.
#[tauri::command]
pub async fn git_pr_create(
    cwd: String,
    title: String,
    body: String,
    base: String,
    head: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_pr_create(cwd, title, body, base, head)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether the GitHub CLI is installed and has an active authenticated account.
#[tauri::command]
pub async fn git_github_status() -> Result<GitHubStatus, String> {
    tauri::async_runtime::spawn_blocking(git_fs::git_github_status)
        .await
        .map_err(|error| error.to_string())
}

/// Whether the active GitHub CLI account has starred the MonoCode repository.
#[tauri::command]
pub async fn github_monocode_star_status() -> Result<GitHubStarStatus, String> {
    tauri::async_runtime::spawn_blocking(git_fs::github_monocode_star_status)
        .await
        .map_err(|error| error.to_string())
}

/// Star the MonoCode repository for the active GitHub CLI account.
#[tauri::command]
pub async fn github_star_monocode() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(git_fs::github_star_monocode)
        .await
        .map_err(|error| error.to_string())?
}

/// `owner/repo` for the GitHub remote of this working copy, via `gh`.
#[tauri::command]
pub async fn git_github_repo(cwd: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_github_repo(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// The GitHub remote of this working copy and, when it is a fork, its parent.
#[tauri::command]
pub async fn git_github_repositories(cwd: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_github_repositories(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Open issues or pull requests for one GitHub repository, via `gh`.
#[tauri::command]
pub async fn git_github_work_items(
    cwd: String,
    repo: String,
    kind: String,
    assigned_to_me: bool,
    state: String,
    search: String,
    limit: Option<u32>,
) -> Result<Vec<GitHubWorkItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_work_items(cwd, repo, kind, assigned_to_me, state, search, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One issue or pull request by number, used when session navigation misses
/// the existing Inbox cache.
#[tauri::command]
pub async fn git_github_work_item(
    cwd: String,
    repo: String,
    kind: String,
    number: i64,
) -> Result<GitHubWorkItem, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_work_item(cwd, repo, kind, number)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Issue or pull request body for the inbox detail pane.
#[tauri::command]
pub async fn git_github_work_item_details(
    cwd: String,
    repo: String,
    kind: String,
    number: i64,
) -> Result<GitHubWorkItemDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_work_item_details(cwd, repo, kind, number)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Conversation for the inbox detail pane: comments, reviews, and review threads.
#[tauri::command]
pub async fn git_github_work_item_thread(
    cwd: String,
    repo: String,
    kind: String,
    number: i64,
) -> Result<GitHubWorkItemThread, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_work_item_thread(cwd, repo, kind, number)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Post a conversation comment, or a reply on a review thread.
#[tauri::command]
pub async fn git_github_work_item_comment(
    cwd: String,
    repo: String,
    kind: String,
    number: i64,
    body: String,
    in_reply_to: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_work_item_comment(cwd, repo, kind, number, body, in_reply_to)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Merge or change the lifecycle state of a GitHub pull request via `gh`.
#[tauri::command]
pub async fn git_github_pr_action(
    cwd: String,
    repo: String,
    number: i64,
    action: String,
) -> Result<GitHubWorkItem, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_pr_action(cwd, repo, number, action)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Unified diff and file stats for a pull request, via `gh`.
/// When `full_context` is true, prefer a large-context `git diff` between the PR OIDs.
#[tauri::command]
pub async fn git_github_pr_diff(
    cwd: String,
    repo: String,
    number: i64,
    full_context: Option<bool>,
) -> Result<GitHubPrDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_pr_diff(cwd, repo, number, full_context)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// CI checks for one pull request, targeted explicitly by `repo` and `number` via `gh`.
#[tauri::command]
pub async fn git_github_pr_checks(
    cwd: String,
    repo: String,
    number: i64,
) -> Result<GitHubPrChecks, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_github_pr_checks(cwd, repo, number))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_github_check_details(
    cwd: String,
    repo: String,
    job_id: String,
) -> Result<GitHubCheckDetails, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_fs::git_github_check_details(cwd, repo, job_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Local branches, plus remote-only branches that can be checked out.
#[tauri::command]
pub async fn git_branches(cwd: String) -> Result<GitBranches, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_branches(cwd))
        .await
        .map_err(|e| e.to_string())?
}

/// Switch to an existing local branch, or create a local tracking branch from a remote.
#[tauri::command]
pub async fn git_checkout(
    cwd: String,
    name: String,
    remote: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_checkout(cwd, name, remote))
        .await
        .map_err(|e| e.to_string())?
}

/// Create a branch from HEAD and switch to it.
#[tauri::command]
pub async fn git_create_branch(cwd: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_create_branch(cwd, name))
        .await
        .map_err(|e| e.to_string())?
}

/// Stash tracked and untracked local changes so a checkout can proceed.
#[tauri::command]
pub async fn git_stash(cwd: String, message: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::git_stash(cwd, message))
        .await
        .map_err(|e| e.to_string())?
}

/// Create a file or folder under `parent`. `name` may contain `/` or `\` to
/// nest. Returns the created path.
#[tauri::command(async)]
pub fn create_path(parent: String, name: String, is_dir: bool) -> Result<String, String> {
    git_fs::create_path(parent, name, is_dir)
}

/// Clone `url` into `parent`/`<repo-name>` and return the new directory.
#[tauri::command]
pub async fn clone_repo(url: String, parent: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::clone_repo(url, parent))
        .await
        .map_err(|e| e.to_string())?
}

/// First few lines of a text file for tool previews.
#[tauri::command(async)]
pub fn read_file_preview(
    path: String,
    max_lines: usize,
    start_line: Option<usize>,
) -> Result<Vec<String>, String> {
    git_fs::read_file_preview(path, max_lines, start_line)
}

/// Metadata only — used to notice disk changes on currently open editors.
#[tauri::command(async)]
pub fn stat_files(paths: Vec<String>) -> Result<Vec<FileMtime>, String> {
    git_fs::stat_files(paths)
}

/// Metadata for files the composer is attaching (picker, drop, paste).
#[tauri::command(async)]
pub fn inspect_paths(paths: Vec<String>) -> Vec<PathInfo> {
    git_fs::inspect_paths(paths)
}

/// Base64-encode a file so vision images can be sent inline over ACP.
#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::read_file_base64(path))
        .await
        .map_err(|e| e.to_string())?
}

/// Persist a pasted blob so non-image attachments have a real path.
#[tauri::command]
pub async fn write_attachment(name: String, data: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::write_attachment(name, data))
        .await
        .map_err(|e| e.to_string())?
}

/// Read a reasonably sized UTF-8 file for the editor.
#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::read_text_file(path))
        .await
        .map_err(|e| e.to_string())?
}

/// Atomically replace a text file from a temporary file in the same directory.
#[tauri::command]
pub async fn write_text_file(path: String, content: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::write_text_file(path, content))
        .await
        .map_err(|e| e.to_string())?
}

/// Rename `path` to `name` (relative to the current parent; `/` nests).
#[tauri::command]
pub async fn rename_path(path: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::rename_path(path, name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::delete_path(path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn copy_path(from: String, dest_parent: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::copy_path(from, dest_parent))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn move_path(from: String, dest_parent: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_fs::move_path(from, dest_parent))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    git_fs::reveal_path(path)
}
