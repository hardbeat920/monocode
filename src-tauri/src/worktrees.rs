use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::fs::{expand_home, git_checked, git_diff_files_for, path_to_js, resolve_repo_path};
use crate::session_store::SessionStore;

#[derive(Clone, Debug, Default, Serialize)]
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
    pub unpushed: Option<u64>,
    pub session_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktrees {
    pub worktrees: Vec<Worktree>,
    pub default_root: String,
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    crate::hide_window_console(&mut command);
    let output = command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8(output.stdout).map_err(|_| "Git returned a non-UTF-8 path".into())
}

fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut result = Vec::new();
    let mut current = Worktree::default();
    for field in text.split('\0') {
        if field.is_empty() {
            if !current.path.is_empty() {
                current.is_main = result.is_empty();
                result.push(std::mem::take(&mut current));
            }
        } else if let Some(path) = field.strip_prefix("worktree ") {
            current.path = path_to_js(Path::new(path));
        } else if let Some(head) = field.strip_prefix("HEAD ") {
            current.head = head.into();
        } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
            current.branch = Some(branch.into());
        } else if field == "locked" || field.starts_with("locked ") {
            current.locked = true;
        } else if field == "prunable" || field.starts_with("prunable ") {
            current.prunable = true;
        }
    }
    result
}

fn list(root: &Path) -> Result<Vec<Worktree>, String> {
    Ok(parse_worktrees(&git(
        root,
        &["worktree", "list", "--porcelain", "-z"],
    )?))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

pub(crate) fn contains_working_dir(root: &Path, cwd: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if cfg!(windows) {
        let root = path_to_js(&root).to_lowercase();
        let cwd = path_to_js(&cwd).to_lowercase();
        cwd == root || cwd.starts_with(&format!("{}/", root.trim_end_matches('/')))
    } else {
        cwd.starts_with(root)
    }
}

fn session_ids(conn: &rusqlite::Connection, path: &Path) -> Result<Vec<String>, String> {
    let mut query = conn
        .prepare("SELECT id, COALESCE(NULLIF(worktree_cwd, ''), cwd) FROM sessions WHERE worktree_removed = 0")
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for row in rows {
        let (id, cwd) = row.map_err(|e| e.to_string())?;
        let cwd = expand_home(&cwd);
        if contains_working_dir(path, &cwd) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn default_root(main: &Path) -> PathBuf {
    let name = main.file_name().unwrap_or_default().to_string_lossy();
    main.with_file_name(format!("{name}-worktrees"))
}

#[tauri::command(async)]
pub fn git_worktrees(cwd: String, store: State<'_, SessionStore>) -> Result<Worktrees, String> {
    let mut worktrees = list(&expand_home(&cwd))?;
    let main = worktrees.first().ok_or("No working copies found")?;
    let default_root = path_to_js(&default_root(Path::new(&main.path)));
    {
        let conn = store.lock_conn()?;
        for tree in &mut worktrees {
            tree.session_ids = session_ids(&conn, Path::new(&tree.path))?;
        }
    }
    for tree in &mut worktrees {
        let path = Path::new(&tree.path);
        tree.missing = !path.is_dir();
        if !tree.missing {
            tree.dirty = git(path, &["status", "--porcelain", "--untracked-files=normal"])
                .ok()
                .map(|status| !status.is_empty());
            tree.unpushed = git(path, &["rev-list", "--count", "HEAD", "--not", "--remotes"])
                .ok()
                .and_then(|count| count.trim().parse().ok());
        }
    }
    Ok(Worktrees {
        worktrees,
        default_root,
    })
}

fn create(root: &Path, branch: &str, base: &str, existing: bool) -> Result<Worktree, String> {
    let branch = branch.trim();
    if branch.starts_with('-') || branch.starts_with('@') || branch.is_empty() {
        return Err("Enter a valid branch name".into());
    }
    git(root, &["check-ref-format", "--branch", branch])?;
    let trees = list(root)?;
    if trees
        .iter()
        .any(|tree| tree.branch.as_deref() == Some(branch))
    {
        return Err("This branch already has a working copy. Select it from the picker.".into());
    }
    let main = trees.first().ok_or("No working copies found")?;
    let parent = default_root(Path::new(&main.path));
    let slug: String = branch
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = parent.join(slug);
    if path.exists() {
        return Err(format!(
            "{} already exists. Choose another branch name.",
            path.display()
        ));
    }
    // Resolve user-supplied refs before passing them to worktree add. Never
    // interpret a ref as an option, and only accept existing local branches.
    let source = if existing {
        format!("refs/heads/{branch}")
    } else {
        base.trim().to_owned()
    };
    let commit = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{source}^{{commit}}"),
        ],
    )?;
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let path_str = path_to_js(&path);
    if existing {
        git_checked(root, &["worktree", "add", "--", &path_str, branch])?;
    } else {
        git_checked(
            root,
            &[
                "worktree",
                "add",
                "--no-track",
                "-b",
                branch,
                "--",
                &path_str,
                commit.trim(),
            ],
        )?;
    }
    list(root)?
        .into_iter()
        .find(|tree| same_path(Path::new(&tree.path), &path))
        .ok_or_else(|| {
            "Worktree created, but could not be found. Refresh the working copies.".into()
        })
}

#[tauri::command(async)]
pub async fn git_worktree_create(
    cwd: String,
    branch: String,
    base: String,
    existing: bool,
) -> Result<Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        create(&expand_home(&cwd), &branch, &base, existing)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn copy_checkout_state(source: &Path, target: &Path) -> Result<(), String> {
    for file in git_diff_files_for(source).files {
        let relative = resolve_repo_path(source, &file.relative)?;
        let from = source.join(&relative);
        let to = target.join(&relative);
        if path_contains_symlink(source, &relative) || path_contains_symlink(target, &relative) {
            return Err(format!(
                "Cannot seed orchestration worktree: {relative} contains a symbolic link"
            ));
        }
        match std::fs::symlink_metadata(&from) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                return Err(format!(
                    "Cannot seed orchestration worktree: {relative} is not a regular file"
                ));
            }
            Ok(meta) => {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
                std::fs::set_permissions(&to, meta.permissions()).map_err(|e| e.to_string())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if to.is_file() || to.is_symlink() {
                    std::fs::remove_file(&to).map_err(|e| e.to_string())?;
                } else if to.exists() {
                    return Err(format!(
                        "Cannot seed orchestration worktree: {relative} is a directory"
                    ));
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn path_contains_symlink(root: &Path, relative: &str) -> bool {
    let mut current = root.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            Err(_) => return true,
        }
    }
    false
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|meta| meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> Option<u32> {
    None
}

fn checkout_state_matches(source: &Path, target: &Path) -> bool {
    let source_files = git_diff_files_for(source).files;
    let target_files = git_diff_files_for(target).files;
    let source_paths: Vec<&str> = source_files
        .iter()
        .map(|file| file.relative.as_str())
        .collect();
    let target_paths: Vec<&str> = target_files
        .iter()
        .map(|file| file.relative.as_str())
        .collect();
    if source_paths != target_paths {
        return false;
    }
    source_paths.into_iter().all(|relative| {
        if path_contains_symlink(source, relative) || path_contains_symlink(target, relative) {
            return false;
        }
        let from = source.join(relative);
        let to = target.join(relative);
        match (
            std::fs::symlink_metadata(&from),
            std::fs::symlink_metadata(&to),
        ) {
            (Err(left), Err(right))
                if left.kind() == std::io::ErrorKind::NotFound
                    && right.kind() == std::io::ErrorKind::NotFound =>
            {
                true
            }
            (Ok(left), Ok(right)) if left.is_file() && right.is_file() => {
                file_mode(&from) == file_mode(&to)
                    && matches!(
                        (std::fs::read(from), std::fs::read(to)),
                        (Ok(left), Ok(right)) if left == right
                    )
            }
            _ => false,
        }
    })
}

fn create_seeded(root: &Path, branch: &str) -> Result<Worktree, String> {
    if let Some(tree) = list(root)?
        .into_iter()
        .find(|tree| tree.branch.as_deref() == Some(branch))
    {
        let source_head = git(root, &["rev-parse", "HEAD"])?;
        if tree.head != source_head.trim() || !checkout_state_matches(root, Path::new(&tree.path)) {
            return Err(format!(
                "The recovered orchestration worktree for {branch} is incomplete or has unexpected changes. It was kept for manual review."
            ));
        }
        return Ok(tree);
    }
    let branch_ref = format!("refs/heads/{branch}");
    let branch_exists = git(root, &["rev-parse", "--verify", &branch_ref]).is_ok();
    if branch_exists {
        let head = git(root, &["rev-parse", "HEAD"])?;
        let branch_head = git(root, &["rev-parse", &branch_ref])?;
        if head.trim() != branch_head.trim() {
            return Err(format!(
                "The recovery branch {branch} no longer starts at this checkout. Remove or rename it before retrying."
            ));
        }
    }
    let tree = create(root, branch, "HEAD", branch_exists)?;
    if let Err(error) = copy_checkout_state(root, Path::new(&tree.path)) {
        let _ = remove(root, Path::new(&tree.path), true);
        if !branch_exists {
            let _ = git(root, &["branch", "-D", branch]);
        }
        return Err(error);
    }
    Ok(tree)
}

/// Create an isolated worker checkout with the lead checkout's current file
/// contents as its baseline. Reusing the deterministic branch makes a crash
/// between Git creation and run-state persistence recoverable.
#[tauri::command(async)]
pub async fn git_orchestration_worktree_create(
    cwd: String,
    branch: String,
) -> Result<Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || create_seeded(&expand_home(&cwd), branch.trim()))
        .await
        .map_err(|error| error.to_string())?
}

fn rename_branch(root: &Path, path: &Path, branch: &str) -> Result<Worktree, String> {
    let branch = branch.trim();
    if branch.starts_with('-') || branch.starts_with('@') || branch.is_empty() {
        return Err("Enter a valid branch name".into());
    }
    git(root, &["check-ref-format", "--branch", branch])?;
    let tree = list(root)?
        .into_iter()
        .find(|tree| same_path(Path::new(&tree.path), path))
        .ok_or("This path is not a registered worktree of this repository")?;
    if tree.is_main {
        return Err("The main working copy cannot be renamed here".into());
    }
    let current = tree.branch.as_deref().ok_or("The worktree is detached")?;
    if !current.starts_with("mc/") && !current.starts_with("monocode/") {
        return Err("Only automatically created worktree branches can be renamed".into());
    }
    if current == branch {
        return Ok(tree);
    }
    git(Path::new(&tree.path), &["branch", "-m", branch])?;
    list(root)?
        .into_iter()
        .find(|entry| same_path(Path::new(&entry.path), path))
        .ok_or_else(|| "Branch renamed, but its worktree could not be found".into())
}

#[tauri::command(async)]
pub async fn git_worktree_rename_branch(
    cwd: String,
    path: String,
    branch: String,
) -> Result<Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        rename_branch(&expand_home(&cwd), &expand_home(&path), &branch)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn removal_target(root: &Path, path: &Path) -> Result<Worktree, String> {
    let tree = list(root)?
        .into_iter()
        .find(|tree| same_path(Path::new(&tree.path), path))
        .ok_or("This path is not a registered worktree of this repository")?;
    if tree.is_main {
        return Err("The main working copy cannot be deleted".into());
    }
    if tree.locked {
        return Err("This worktree is locked. Unlock it in Git before deleting it.".into());
    }
    if tree.branch.is_none() {
        // There is no branch retaining detached commits after removal.
        return Err("Create a branch for this detached worktree before deleting it.".into());
    }
    Ok(tree)
}

#[tauri::command(async)]
pub fn git_worktree_check_remove(
    cwd: String,
    path: String,
    force: bool,
    terminals: State<'_, crate::pty::PtyHost>,
) -> Result<(), String> {
    let path = expand_home(&path);
    check_removal(
        &expand_home(&cwd),
        &path,
        force,
        terminals.has_working_dir(&path),
    )
}

fn check_removal(root: &Path, path: &Path, force: bool, has_terminals: bool) -> Result<(), String> {
    let tree = removal_target(root, path)?;
    if has_terminals {
        return Err("Close the terminals using this worktree first.".into());
    }
    // Sessions and their agents still exist during preflight. The actual
    // removal below checks them again after the session deletion lifecycle.
    if !force
        && !git(
            Path::new(&tree.path),
            &["status", "--porcelain", "--untracked-files=normal"],
        )?
        .is_empty()
    {
        return Err("This worktree has uncommitted or untracked changes.".into());
    }
    Ok(())
}

fn remove(root: &Path, path: &Path, force: bool) -> Result<(), String> {
    let tree = removal_target(root, path)?;
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", tree.path.as_str()]);
    git_checked(root, &args)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemoval {
    session_ids: Vec<String>,
    project_cwd: String,
}

#[derive(Serialize, Deserialize)]
struct SessionBeforeRemoval {
    id: String,
    cwd: String,
    worktree_cwd: Option<String>,
    branch: Option<String>,
    provider_session_id: Option<String>,
    context_used: Option<i64>,
    context_window: Option<i64>,
    in_flight_cwd: Option<String>,
    in_flight_sort_index: Option<i64>,
    detached_cwd: String,
}

/// Make sessions safe to reopen *before* touching Git. A small metadata journal
/// lets a failed/interrupted deletion restore their original working context.
fn prepare_removal(
    conn: &rusqlite::Connection,
    path: &Path,
    project_cwd: &str,
    ids: &[String],
) -> Result<Vec<SessionBeforeRemoval>, String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut saved = Vec::new();
    for id in ids {
        let mut session = tx
            .query_row(
                "SELECT s.cwd, s.worktree_cwd, s.branch, s.provider_session_id,
                    s.context_used, s.context_window, f.cwd, f.sort_index
             FROM sessions s LEFT JOIN in_flight_sessions f ON f.session_id = s.id
             WHERE s.id = ?1",
                [id],
                |row| {
                    Ok(SessionBeforeRemoval {
                        id: id.clone(),
                        cwd: row.get(0)?,
                        worktree_cwd: row.get(1)?,
                        branch: row.get(2)?,
                        provider_session_id: row.get(3)?,
                        context_used: row.get(4)?,
                        context_window: row.get(5)?,
                        in_flight_cwd: row.get(6)?,
                        in_flight_sort_index: row.get(7)?,
                        detached_cwd: String::new(),
                    })
                },
            )
            .map_err(|e| e.to_string())?;
        session.detached_cwd = if contains_working_dir(path, &expand_home(&session.cwd)) {
            project_cwd.to_owned()
        } else {
            session.cwd.clone()
        };
        tx.execute(
            "UPDATE sessions SET worktree_removed = 1,
               worktree_cwd = COALESCE(NULLIF(worktree_cwd, ''), cwd),
               cwd = ?2, branch = NULL, provider_session_id = NULL,
               context_used = NULL, context_window = NULL WHERE id = ?1",
            rusqlite::params![id, session.detached_cwd],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM in_flight_sessions WHERE session_id = ?1", [id])
            .map_err(|e| e.to_string())?;
        saved.push(session);
    }
    tx.execute(
        "INSERT INTO worktree_removals (path, sessions_json) VALUES (?1, ?2)",
        rusqlite::params![
            path_to_js(path),
            serde_json::to_string(&saved).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}

fn finish_removal(
    conn: &rusqlite::Connection,
    path: &str,
    restore: &[SessionBeforeRemoval],
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for session in restore {
        // A cleanup failure can leave a journal until the next launch. Do not
        // overwrite sessions which the user has since reattached or deleted.
        let changed = tx
            .execute(
                "UPDATE sessions SET cwd = ?2, worktree_cwd = ?3, branch = ?4,
               provider_session_id = ?5, context_used = ?6, context_window = ?7,
               worktree_removed = 0
             WHERE id = ?1 AND worktree_removed = 1 AND cwd = ?8 AND worktree_cwd = ?9",
                rusqlite::params![
                    session.id,
                    session.cwd,
                    session.worktree_cwd,
                    session.branch,
                    session.provider_session_id,
                    session.context_used,
                    session.context_window,
                    session.detached_cwd,
                    session
                        .worktree_cwd
                        .as_deref()
                        .filter(|cwd| !cwd.is_empty())
                        .unwrap_or(&session.cwd)
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed > 0 {
            if let (Some(cwd), Some(index)) = (&session.in_flight_cwd, session.in_flight_sort_index)
            {
                tx.execute(
                    "INSERT OR REPLACE INTO in_flight_sessions (session_id, cwd, sort_index) VALUES (?1, ?2, ?3)",
                    rusqlite::params![session.id, cwd, index],
                ).map_err(|e| e.to_string())?;
            }
        }
    }
    tx.execute("DELETE FROM worktree_removals WHERE path = ?1", [path])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn reconcile_removals(conn: &rusqlite::Connection) -> Result<(), String> {
    let pending = conn
        .prepare("SELECT path, sessions_json FROM worktree_removals")
        .map_err(|e| e.to_string())?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    for (path, json) in pending {
        // A surviving Git link means removal did not finish. If the link/folder
        // is gone, the already-persisted detached sessions are the final state.
        let restore = if Path::new(&path)
            .join(".git")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            serde_json::from_str::<Vec<SessionBeforeRemoval>>(&json).map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        finish_removal(conn, &path, &restore)?;
    }
    Ok(())
}

fn remove_with_sessions(
    conn: &rusqlite::Connection,
    root: &Path,
    path: &Path,
    force: bool,
    keep_sessions: bool,
) -> Result<WorktreeRemoval, String> {
    let tree = removal_target(root, path)?;
    let worktrees = list(root)?;
    let main = worktrees
        .iter()
        .find(|tree| tree.is_main)
        .ok_or("No main working copy found")?;
    let ids = session_ids(conn, path)?;
    if !keep_sessions && !ids.is_empty() {
        return Err("Sessions still use this worktree. Move or delete those sessions first (including archived sessions).".into());
    }
    let saved = prepare_removal(conn, Path::new(&tree.path), &main.path, &ids)?;
    // Use the main copy even if Settings was opened directly on the target.
    if let Err(error) = remove(Path::new(&main.path), path, force) {
        finish_removal(conn, &tree.path, &saved).map_err(|restore| {
            format!("{error}. Sessions remain detached until recovery on restart: {restore}")
        })?;
        return Err(error);
    }
    // Git has succeeded and the sessions are already durably detached. A
    // cleanup failure must not tell the UI to resume them in the deleted path.
    if let Err(error) = finish_removal(conn, &tree.path, &[]) {
        eprintln!("Worktree removed; recovery journal cleanup will retry on restart: {error}");
    }
    Ok(WorktreeRemoval {
        session_ids: ids,
        project_cwd: main.path.clone(),
    })
}

#[tauri::command(async)]
pub fn git_worktree_remove(
    cwd: String,
    path: String,
    force: bool,
    keep_sessions: Option<bool>,
    store: State<'_, SessionStore>,
    terminals: State<'_, crate::pty::PtyHost>,
    agents: State<'_, crate::harness::HarnessHost>,
) -> Result<WorktreeRemoval, String> {
    let path = expand_home(&path);
    let _reservation = crate::worktree_lifecycle::reserve_removal(&path)?;
    if terminals.has_working_dir(&path) || agents.has_working_dir(&path) {
        return Err("Close the terminals and agent processes using this worktree first.".into());
    }
    let conn = store.lock_conn()?;
    remove_with_sessions(
        &conn,
        &expand_home(&cwd),
        &path,
        force,
        keep_sessions.unwrap_or(false),
    )
}

#[tauri::command(async)]
pub fn git_orchestration_worktree_remove(
    cwd: String,
    path: String,
    store: State<'_, SessionStore>,
    terminals: State<'_, crate::pty::PtyHost>,
    agents: State<'_, crate::harness::HarnessHost>,
) -> Result<WorktreeRemoval, String> {
    let root = expand_home(&cwd);
    let path = expand_home(&path);
    let _reservation = crate::worktree_lifecycle::reserve_removal(&path)?;
    if terminals.has_working_dir(&path) || agents.has_working_dir(&path) {
        return Err("Close the terminals and agent processes using this worktree first.".into());
    }
    let tree = removal_target(&root, &path)?;
    let branch = tree.branch.clone();
    let conn = store.lock_conn()?;
    let removed = remove_with_sessions(&conn, &root, &path, true, true)?;
    if let Some(branch) = branch {
        if branch.starts_with("mc/orch-") {
            if let Err(error) = git(Path::new(&removed.project_cwd), &["branch", "-D", &branch]) {
                eprintln!("Orchestration worktree removed; temporary branch cleanup will need a retry: {error}");
            }
        }
    }
    Ok(removed)
}

#[tauri::command(async)]
pub async fn git_orchestration_branch_remove(cwd: String, branch: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = expand_home(&cwd);
        let branch = branch.trim();
        if !branch.starts_with("mc/orch-") {
            return Err("Only orchestration temporary branches can be removed here".into());
        }
        git(&root, &["check-ref-format", "--branch", branch])?;
        let branch_ref = format!("refs/heads/{branch}");
        if git(&root, &["rev-parse", "--verify", &branch_ref]).is_err() {
            return Ok(());
        }
        if list(&root)?
            .iter()
            .any(|tree| tree.branch.as_deref() == Some(branch))
        {
            return Err("The orchestration branch still has a worktree".into());
        }
        git(&root, &["branch", "-D", branch]).map(|_| ())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Repo(PathBuf);
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn repo() -> Repo {
        let dir =
            std::env::temp_dir().join(format!("monocode-worktree-test-{}", uuid::Uuid::new_v4()));
        let root = dir.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git_checked(&root, &["init", "-b", "main"]).unwrap();
        git_checked(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        )
        .unwrap();
        Repo(dir)
    }

    #[test]
    fn parses_literal_paths_and_worktree_flags() {
        let trees = parse_worktrees("worktree /repo\0HEAD abc\0branch refs/heads/main\0\0worktree /a\nquoted\"path\0HEAD def\0detached\0locked reason\0prunable missing\0\0");
        assert!(trees[0].is_main);
        assert_eq!(trees[1].path, "/a\nquoted\"path");
        assert!(trees[1].locked && trees[1].prunable);
        assert!(!trees[1].is_main);
    }

    #[test]
    fn isolates_changes_and_preserves_branch_on_removal() {
        let repo = repo();
        let root = repo.0.join("repo");
        std::fs::write(root.join("main-only"), "main change").unwrap();
        git_checked(&root, &["add", "main-only"]).unwrap();
        let tree = create(&root, "feature/test", "main", false).unwrap();
        let path = Path::new(&tree.path);
        assert!(!path.join("main-only").exists());
        assert!(git(path, &["diff", "--cached", "--name-only"])
            .unwrap()
            .is_empty());
        std::fs::write(path.join("feature-only"), "feature change").unwrap();
        git_checked(path, &["add", "feature-only"]).unwrap();
        git_checked(
            path,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "feature commit",
            ],
        )
        .unwrap();
        let commit = git(path, &["rev-parse", "HEAD"]).unwrap();
        assert_ne!(commit, git(&root, &["rev-parse", "HEAD"]).unwrap());
        assert_eq!(
            git(&root, &["diff", "--cached", "--name-only"])
                .unwrap()
                .trim(),
            "main-only"
        );
        assert!(!root.join("feature-only").exists());
        std::fs::write(path.join("uncommitted"), "keep unless forced").unwrap();
        assert!(remove(&root, path, false).is_err());
        assert!(path.join("uncommitted").exists());
        assert!(remove(&root, &root, true).is_err());
        remove(&root, path, true).unwrap();
        assert!(root.join("main-only").exists());
        assert_eq!(
            git(&root, &["rev-parse", "--verify", "refs/heads/feature/test"]).unwrap(),
            commit
        );
    }

    #[test]
    fn orchestration_worktree_starts_from_the_lead_checkout_contents() {
        let repo = repo();
        let root = repo.0.join("repo");
        std::fs::write(root.join("tracked.txt"), "head\n").unwrap();
        std::fs::write(root.join("deleted.txt"), "delete me\n").unwrap();
        git_checked(&root, &["add", "."]).unwrap();
        git_checked(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "files",
            ],
        )
        .unwrap();
        std::fs::write(root.join("tracked.txt"), "lead dirty\n").unwrap();
        std::fs::write(root.join("untracked.txt"), "lead new\n").unwrap();
        std::fs::remove_file(root.join("deleted.txt")).unwrap();

        let tree = create_seeded(&root, "mc/orch-testworker").unwrap();
        let worker = Path::new(&tree.path);
        assert_eq!(
            std::fs::read_to_string(worker.join("tracked.txt")).unwrap(),
            "lead dirty\n"
        );
        assert_eq!(
            std::fs::read_to_string(worker.join("untracked.txt")).unwrap(),
            "lead new\n"
        );
        assert!(!worker.join("deleted.txt").exists());
        assert_eq!(
            create_seeded(&root, "mc/orch-testworker").unwrap().path,
            tree.path
        );

        std::fs::write(root.join("tracked.txt"), "later lead edit\n").unwrap();
        assert!(create_seeded(&root, "mc/orch-testworker").is_err());
        assert_eq!(
            std::fs::read_to_string(worker.join("tracked.txt")).unwrap(),
            "lead dirty\n"
        );
        remove(&root, worker, true).unwrap();
    }

    #[test]
    fn reuses_branches_and_rejects_locked_or_unregistered_paths() {
        let repo = repo();
        let root = repo.0.join("repo");
        git_checked(&root, &["branch", "existing"]).unwrap();
        let tree = create(&root, "existing", "HEAD", true).unwrap();
        assert!(create(&root, "existing", "HEAD", true).is_err());
        assert!(create(&root, "main", "HEAD", true).is_err());
        assert!(create(&root, "bad name", "HEAD", false).is_err());
        assert!(create(&root, "valid", "--help", false).is_err());
        git_checked(&root, &["worktree", "lock", &tree.path]).unwrap();
        assert!(remove(&root, Path::new(&tree.path), true).is_err());
        assert!(remove(&root, &repo.0, true).is_err());
    }

    #[test]
    fn renames_only_temporary_worktree_branches() {
        let repo = repo();
        let root = repo.0.join("repo");
        let tree = create(&root, "mc/12345678", "main", false).unwrap();
        let renamed = rename_branch(&root, Path::new(&tree.path), "mc/faster-worktrees").unwrap();
        assert_eq!(renamed.branch.as_deref(), Some("mc/faster-worktrees"));
        assert!(git(
            &root,
            &["rev-parse", "--verify", "refs/heads/mc/faster-worktrees"]
        )
        .is_ok());
        assert!(rename_branch(&root, &root, "mc/nope").is_err());

        let regular = create(&root, "feature/manual", "main", false).unwrap();
        assert!(rename_branch(&root, Path::new(&regular.path), "mc/should-not-change").is_err());
    }

    #[test]
    fn removal_preflight_is_read_only_and_rejects_blockers() {
        let repo = repo();
        let root = repo.0.join("repo");
        let tree = create(&root, "feature", "main", false).unwrap();
        let path = Path::new(&tree.path);
        std::fs::write(path.join("keep-me"), "local changes").unwrap();
        assert!(check_removal(&root, path, true, true)
            .unwrap_err()
            .contains("terminals"));
        assert!(check_removal(&root, path, false, false).is_err());
        check_removal(&root, path, true, false).unwrap();
        assert!(path.join("keep-me").exists());
        assert_eq!(list(&root).unwrap().len(), 2);
        assert!(check_removal(&root, &root, true, false).is_err());
        assert!(check_removal(&root, &repo.0, true, false).is_err());
        git_checked(&root, &["worktree", "lock", &tree.path]).unwrap();
        assert!(check_removal(&root, path, true, false)
            .unwrap_err()
            .contains("locked"));
        assert!(path.join("keep-me").exists());
    }

    #[test]
    fn references_include_archived_sessions_and_directly_opened_worktrees() {
        let repo = repo();
        let root = repo.0.join("repo");
        let tree = create(&root, "feature", "main", false).unwrap();
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        for (id, cwd, worktree, archived) in [
            ("shared", path_to_js(&root), Some(tree.path.clone()), 0),
            ("archived", path_to_js(&root), Some(tree.path.clone()), 1),
            ("direct", tree.path.clone(), None, 0),
            ("main", path_to_js(&root), None, 0),
        ] {
            conn.execute(
                "INSERT INTO sessions (id, cwd, harness, model, runtime_mode, title, blocks_json, created_at, updated_at, worktree_cwd, archived) VALUES (?1, ?2, 'codex', 'test', 'supervised', 'Test', '[]', 0, 0, ?3, ?4)",
                rusqlite::params![id, cwd, worktree, archived],
            ).unwrap();
        }
        let mut ids = session_ids(&conn, Path::new(&tree.path)).unwrap();
        ids.sort();
        assert_eq!(ids, vec!["archived", "direct", "shared"]);
        assert!(!contains_working_dir(
            &root,
            &PathBuf::from(format!("{}-other", root.display()))
        ));
    }

    #[test]
    fn removal_preserves_shared_archived_and_direct_sessions() {
        let repo = repo();
        let root = repo.0.join("repo").canonicalize().unwrap();
        let tree = create(&root, "feature", "main", false).unwrap();
        let path = Path::new(&tree.path);
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        let blocks = r#"[{"id":"u","role":"user","text":"Keep my conversation"}]"#;
        for (id, cwd, worktree, archived) in [
            ("shared", path_to_js(&root), Some(tree.path.clone()), 0),
            ("archived", path_to_js(&root), Some(tree.path.clone()), 1),
            ("direct", tree.path.clone(), None, 0),
            ("main", path_to_js(&root), None, 0),
        ] {
            conn.execute(
                "INSERT INTO sessions (id, cwd, harness, model, runtime_mode, title, blocks_json, created_at, updated_at, worktree_cwd, archived, branch, provider_session_id) VALUES (?1, ?2, 'codex', 'test', 'supervised', 'Keep title', ?3, 10, 20, ?4, ?5, 'feature', 'provider')",
                rusqlite::params![id, cwd, blocks, worktree, archived],
            ).unwrap();
        }
        assert!(remove_with_sessions(&conn, &root, path, true, false).is_err());
        // An actual Git removal failure must roll back the database changes.
        std::fs::write(path.join("dirty"), "uncommitted").unwrap();
        assert!(remove_with_sessions(&conn, &root, path, false, true).is_err());
        assert_eq!(session_ids(&conn, path).unwrap().len(), 3);
        let mut removed = remove_with_sessions(&conn, path, path, true, true).unwrap();
        removed.session_ids.sort();
        assert_eq!(removed.session_ids, vec!["archived", "direct", "shared"]);
        // Git omits the verbatim prefix added by canonicalize() on Windows.
        assert!(same_path(Path::new(&removed.project_cwd), &root));
        assert!(!path.exists());
        assert!(session_ids(&conn, path).unwrap().is_empty());
        for id in &removed.session_ids {
            let row: (String, String, String, i64, i64, i64, Option<String>, Option<String>, String) = conn.query_row(
                "SELECT cwd, title, blocks_json, created_at, updated_at, worktree_removed, branch, provider_session_id, worktree_cwd FROM sessions WHERE id = ?1", [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
            ).unwrap();
            let expected_cwd = if id == "direct" {
                removed.project_cwd.clone()
            } else {
                path_to_js(&root)
            };
            assert_eq!(
                row,
                (
                    expected_cwd,
                    "Keep title".into(),
                    blocks.into(),
                    10,
                    20,
                    1,
                    None,
                    None,
                    tree.path.clone()
                )
            );
        }
        let archived: bool = conn
            .query_row(
                "SELECT archived FROM sessions WHERE id = 'archived'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(archived);
        let main_removed: bool = conn
            .query_row(
                "SELECT worktree_removed FROM sessions WHERE id = 'main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!main_removed);
        assert!(git(&root, &["rev-parse", "--verify", "refs/heads/feature"]).is_ok());
    }

    #[test]
    fn startup_recovers_interruptions_before_and_after_git_removal() {
        type StoredContext = (
            String,
            Option<String>,
            bool,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
        );
        for git_removed in [false, true] {
            let repo = repo();
            let root = repo.0.join("repo");
            let tree = create(&root, "feature", "main", false).unwrap();
            let main = list(&root).unwrap().remove(0).path;
            let db = repo.0.join("sessions.db");
            {
                let store = SessionStore::open(db.clone()).unwrap();
                let conn = store.lock_conn().unwrap();
                conn.execute(
                    "INSERT INTO sessions (id, cwd, harness, model, runtime_mode, title, created_at, updated_at, branch, provider_session_id, context_used, context_window)
                     VALUES ('s1', ?1, 'codex', 'test', 'supervised', 'Keep title', 10, 20, 'feature', 'provider', 12, 100)",
                    [&tree.path],
                ).unwrap();
                conn.execute(
                    "INSERT INTO in_flight_sessions VALUES ('s1', ?1, 3)",
                    [&tree.path],
                )
                .unwrap();
                prepare_removal(&conn, Path::new(&tree.path), &main, &["s1".into()]).unwrap();
                if git_removed {
                    remove(&root, Path::new(&tree.path), true).unwrap();
                }
                // Drop the connection without finalizing, as on process exit.
            }
            let store = SessionStore::open(db).unwrap();
            let conn = store.lock_conn().unwrap();
            let state: StoredContext = conn.query_row(
                "SELECT cwd, worktree_cwd, worktree_removed, branch, provider_session_id, context_used, context_window FROM sessions WHERE id = 's1'", [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            ).unwrap();
            if git_removed {
                assert_eq!(state, (main, Some(tree.path), true, None, None, None, None));
            } else {
                assert_eq!(
                    state,
                    (
                        tree.path.clone(),
                        None,
                        false,
                        Some("feature".into()),
                        Some("provider".into()),
                        Some(12),
                        Some(100)
                    )
                );
                let in_flight: (String, i64) = conn
                    .query_row(
                        "SELECT cwd, sort_index FROM in_flight_sessions WHERE session_id = 's1'",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(in_flight, (tree.path, 3));
            }
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM worktree_removals", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0);
        }
    }

    #[test]
    fn database_failures_keep_worktree_and_session_state_consistent() {
        let repo = repo();
        let root = repo.0.join("repo");
        let tree = create(&root, "feature", "main", false).unwrap();
        let store = SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, cwd, harness, model, runtime_mode, title, created_at, updated_at)
             VALUES ('s1', ?1, 'codex', 'test', 'supervised', 'Keep title', 10, 20)",
            [&tree.path],
        ).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_prepare BEFORE INSERT ON worktree_removals
             BEGIN SELECT RAISE(ABORT, 'journal unavailable'); END;",
        )
        .unwrap();
        assert!(remove_with_sessions(&conn, &root, Path::new(&tree.path), true, true).is_err());
        assert!(Path::new(&tree.path).exists());
        assert_eq!(session_ids(&conn, Path::new(&tree.path)).unwrap(), ["s1"]);
        conn.execute_batch("DROP TRIGGER fail_prepare").unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_cleanup BEFORE DELETE ON worktree_removals
             BEGIN SELECT RAISE(ABORT, 'cleanup unavailable'); END;",
        )
        .unwrap();
        let removed =
            remove_with_sessions(&conn, &root, Path::new(&tree.path), true, true).unwrap();
        assert_eq!(removed.session_ids, ["s1"]);
        assert!(!Path::new(&tree.path).exists());
        let detached: bool = conn
            .query_row(
                "SELECT worktree_removed FROM sessions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(detached);
        let pending: i64 = conn
            .query_row("SELECT COUNT(*) FROM worktree_removals", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(pending, 1);
        conn.execute_batch("DROP TRIGGER fail_cleanup").unwrap();
        reconcile_removals(&conn).unwrap();
        let detached: bool = conn
            .query_row(
                "SELECT worktree_removed FROM sessions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(detached);
    }
}
