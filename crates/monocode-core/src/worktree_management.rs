//! Settings worktree guards; removal keeps the branch and commits.
use std::path::{Path, PathBuf};
pub fn default_root(main: &Path) -> PathBuf {
    main.with_file_name(format!(
        "{}-worktrees",
        main.file_name().unwrap_or_default().to_string_lossy()
    ))
}
pub fn status(cwd: &Path, tree: &crate::git::Worktree) -> (bool, Option<bool>, usize) {
    let locked = crate::git::git(cwd, &["worktree", "list", "--porcelain", "-z"])
        .ok()
        .is_some_and(|s| {
            s.split("\0\0").any(|block| {
                block.starts_with(&format!("worktree {}\0", tree.path))
                    && block
                        .split('\0')
                        .any(|v| v == "locked" || v.starts_with("locked "))
            })
        });
    let dirty = crate::git::git(
        Path::new(&tree.path),
        &["status", "--porcelain", "--untracked-files=normal"],
    )
    .ok()
    .map(|s| !s.is_empty());
    let unpushed = crate::git::git(
        Path::new(&tree.path),
        &["rev-list", "--count", "HEAD", "--not", "--remotes"],
    )
    .ok()
    .and_then(|s| s.trim().parse().ok())
    .unwrap_or(0);
    (locked, dirty, unpushed)
}
pub fn check(cwd: &Path, path: &Path) -> Result<crate::git::Worktree, String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let key = crate::files::path_key(&canonical.to_string_lossy());
    let tree = crate::git::snapshot(cwd)?
        .worktrees
        .into_iter()
        .find(|t| {
            let canonical = Path::new(&t.path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&t.path));
            crate::files::path_key(&canonical.to_string_lossy()) == key
        })
        .ok_or("This path is not a registered worktree of this repository")?;
    if tree.main {
        return Err("The main working copy cannot be deleted".into());
    }
    if tree.branch.is_none() {
        return Err("Create a branch for this detached worktree before deleting it.".into());
    }
    if status(cwd, &tree).0 {
        return Err("This worktree is locked. Unlock it in Git before deleting it.".into());
    }
    Ok(tree)
}
pub fn remove(cwd: &Path, path: &Path) -> Result<(), String> {
    let tree = check(cwd, path)?;
    crate::provider::discard_worktree(Path::new(&tree.path));
    crate::git::git(cwd, &["worktree", "remove", "--force", "--", &tree.path]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| crate::git::git(dir.path(), args).unwrap();
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        git(&["config", "user.name", "Fixture"]);
        std::fs::write(dir.path().join("file"), "before").unwrap();
        git(&["add", "file"]);
        git(&["commit", "-m", "Initial"]);
        dir
    }
    #[test]
    fn removal_guards_and_branch_retention() {
        let dir = repo();
        assert!(check(dir.path(), dir.path()).unwrap_err().contains("main"));
        let tree = crate::git::create_named_worktree(dir.path(), "settings-parity").unwrap();
        let path = Path::new(&tree.path);
        std::fs::write(path.join("untracked"), "discard in disposable test").unwrap();
        assert_eq!(status(dir.path(), &tree).1, Some(true));
        crate::git::git(dir.path(), &["worktree", "lock", "--", &tree.path]).unwrap();
        assert!(check(dir.path(), path).unwrap_err().contains("locked"));
        crate::git::git(dir.path(), &["worktree", "unlock", "--", &tree.path]).unwrap();
        remove(dir.path(), path).unwrap();
        assert!(!path.exists());
        assert!(crate::git::git(
            dir.path(),
            &["show-ref", "--verify", "refs/heads/settings-parity"]
        )
        .is_ok());
        let _ = std::fs::remove_dir(default_root(dir.path()));
    }
    #[test]
    fn detached_commits_cannot_be_orphaned() {
        let dir = repo();
        let path = dir.path().join("detached");
        crate::git::git(
            dir.path(),
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str().unwrap(),
                "HEAD",
            ],
        )
        .unwrap();
        assert!(remove(dir.path(), &path).unwrap_err().contains("detached"));
        crate::git::git(
            dir.path(),
            &["worktree", "remove", "--force", path.to_str().unwrap()],
        )
        .unwrap();
    }
}
