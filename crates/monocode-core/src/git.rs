//! Working-copy operations used by the native composer. Git protects dirty files;
//! these operations never force a checkout, stash, or discard changes.
pub mod naming;
mod remote;
pub mod report;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Deserialize)]
pub struct Branch {
    pub name: String,
    pub remote: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
    pub head: String,
    #[serde(alias = "isMain")]
    pub main: bool,
    pub missing: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub current: Option<String>,
    pub detached: bool,
    pub linked: bool,
    pub branches: Vec<Branch>,
    pub worktrees: Vec<Worktree>,
}
pub(crate) fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    if crate::files::remote::is_remote(cwd) {
        return Err("This isn’t available for projects on another machine yet.".into());
    }
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    crate::process::configure_child(&mut command);
    let out = command
        .output()
        .map_err(|e| format!("Could not run Git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}
/// Where a session's own paths stand against `HEAD`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionChanges {
    /// Some of the changes are in the index.
    pub staged: bool,
    /// Some of the changes, or untracked files, are only in the working tree.
    pub unstaged: bool,
}
impl SessionChanges {
    pub fn uncommitted(self) -> bool {
        self.staged || self.unstaged
    }
}
/// Whether any of `paths` has uncommitted changes, staged or not. Paths
/// outside the checkout count as clean. Never takes Git's index lock.
pub fn uncommitted(cwd: &Path, paths: &[String]) -> bool {
    session_changes(cwd, paths).uncommitted()
}
/// Which of `paths`' uncommitted changes are staged, as `uncommitted` reads them.
pub fn session_changes(cwd: &Path, paths: &[String]) -> SessionChanges {
    let mut changes = SessionChanges::default();
    if paths.is_empty() {
        return changes;
    }
    let Ok(root) = git(cwd, &["rev-parse", "--show-toplevel"]) else {
        return changes;
    };
    let root = Path::new(root.trim());
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
    let inside: Vec<String> = paths
        .iter()
        .filter(|path| !path.is_empty())
        .map(|path| crate::files::document::resolve_path(Path::new(path), cwd))
        // Resolve directory aliases even for deleted files, without following
        // a final symlink that Git tracks as a file in its own right.
        .map(|path| {
            path.parent()
                .and_then(|parent| std::fs::canonicalize(parent).ok())
                .zip(path.file_name())
                .map_or_else(|| path.clone(), |(parent, name)| parent.join(name))
        })
        .filter(|path| path.starts_with(&root) && path != &root)
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    if inside.is_empty() {
        return changes;
    }
    let mut args = vec![
        "--no-optional-locks",
        "--literal-pathspecs",
        "status",
        "--porcelain",
        "--untracked-files=all",
        "--",
    ];
    args.extend(inside.iter().map(String::as_str));
    let Ok(out) = git(&root, &args) else {
        return changes;
    };
    for line in out.lines() {
        let mut status = line.chars();
        let (Some(index), Some(tree)) = (status.next(), status.next()) else {
            continue;
        };
        changes.staged |= !matches!(index, ' ' | '?');
        changes.unstaged |= tree != ' ';
    }
    changes
}
/// `commit` is on the current branch but on no remote-tracking branch yet.
pub fn unpushed(cwd: &Path, commit: &str) -> bool {
    if commit.is_empty() || commit.starts_with('-') {
        return false;
    }
    let on_branch = git(cwd, &["symbolic-ref", "-q", "HEAD"]).is_ok()
        && git(cwd, &["merge-base", "--is-ancestor", commit, "HEAD"]).is_ok();
    on_branch
        && git(
            cwd,
            &["--no-optional-locks", "branch", "-r", "--contains", commit],
        )
        .is_ok_and(|out| out.trim().is_empty())
}
/// One file of a commit, as `git show --numstat` counts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitFile {
    pub path: String,
    /// `None` for binary files.
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}
/// The files `commit` changed against its first parent.
pub fn commit_files(cwd: &Path, commit: &str) -> Result<Vec<CommitFile>, String> {
    if commit.is_empty() || commit.starts_with('-') {
        return Err("Not a commit".into());
    }
    let out = git(
        cwd,
        &[
            "-c",
            "core.quotepath=false",
            "--no-optional-locks",
            "show",
            "--no-renames",
            "--numstat",
            "--format=",
            commit,
            "--",
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let additions = parts.next()?.parse().ok();
            let deletions = parts.next()?.parse().ok();
            let path = parts.next()?.to_owned();
            Some(CommitFile {
                path,
                additions,
                deletions,
            })
        })
        .collect())
}
pub fn snapshot(cwd: &Path) -> Result<Snapshot, String> {
    if crate::files::remote::is_remote(cwd) {
        return remote::snapshot(cwd);
    }
    let root = git(cwd, &["rev-parse", "--show-toplevel"])?;
    let current = git(cwd, &["symbolic-ref", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_owned());
    let detached = current.is_none();
    let current = current.or_else(|| {
        git(cwd, &["rev-parse", "--short", "HEAD"])
            .ok()
            .map(|s| s.trim().to_owned())
    });
    let trees = worktrees(cwd)?;
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    let root = std::fs::canonicalize(root.trim()).unwrap_or(canonical);
    let linked = trees
        .iter()
        .any(|t| !t.main && std::fs::canonicalize(&t.path).ok().as_ref() == Some(&root));
    let branches = git(
        cwd,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)",
            "refs/heads/",
            "refs/remotes/",
        ],
    )?
    .lines()
    .filter_map(|line| {
        if let Some(name) = line.strip_prefix("refs/heads/") {
            Some(Branch {
                name: name.into(),
                remote: None,
            })
        } else {
            let (remote, name) = line.strip_prefix("refs/remotes/")?.split_once('/')?;
            (name != "HEAD").then(|| Branch {
                name: name.into(),
                remote: Some(remote.into()),
            })
        }
    })
    .collect();
    Ok(Snapshot {
        current,
        detached,
        linked,
        branches,
        worktrees: trees,
    })
}

/// Listing working copies does not require reading every local/remote branch.
pub fn worktrees(cwd: &Path) -> Result<Vec<Worktree>, String> {
    if crate::files::remote::is_remote(cwd) {
        return remote::worktrees(cwd);
    }
    let mut trees = Vec::new();
    let mut tree = Worktree::default();
    for field in git(cwd, &["worktree", "list", "--porcelain", "-z"])?.split('\0') {
        if field.is_empty() {
            if !tree.path.is_empty() {
                tree.main = trees.is_empty();
                tree.missing = !Path::new(&tree.path).is_dir();
                trees.push(std::mem::take(&mut tree));
            }
        } else if let Some(path) = field.strip_prefix("worktree ") {
            tree.path = path.into();
        } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
            tree.branch = Some(branch.into());
        } else if let Some(head) = field.strip_prefix("HEAD ") {
            tree.head = head.into();
        }
    }
    Ok(trees)
}
fn valid_branch(cwd: &Path, name: &str) -> Result<(), String> {
    if name.is_empty() || name.starts_with(['-', '@']) {
        return Err("Enter a valid branch name".into());
    }
    git(cwd, &["check-ref-format", "--branch", name]).map(|_| ())
}
pub fn checkout(cwd: &Path, branch: &Branch) -> Result<(), String> {
    if crate::files::remote::is_remote(cwd) {
        return remote::checkout(cwd, branch);
    }
    valid_branch(cwd, &branch.name)?;
    if let Some(remote) = &branch.remote {
        let reference = format!("{remote}/{}", branch.name);
        git(
            cwd,
            &["switch", "--track", "-c", &branch.name, "--", &reference],
        )?;
    } else {
        git(cwd, &["switch", "--", &branch.name])?;
    }
    Ok(())
}
pub fn create_branch(cwd: &Path, name: &str) -> Result<(), String> {
    if crate::files::remote::is_remote(cwd) {
        return remote::create_branch(cwd, name);
    }
    valid_branch(cwd, name)?;
    git(cwd, &["switch", "-c", name])?;
    Ok(())
}
pub fn create_worktree(cwd: &Path, base: &str, session: i64) -> Result<PathBuf, String> {
    if crate::files::remote::is_remote(cwd) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        return remote::create_worktree(
            cwd,
            &format!("mc/native-{session}-{stamp}"),
            Some(base),
            false,
        )
        .map(|tree| PathBuf::from(tree.path));
    }
    let state = snapshot(cwd)?;
    let main = state.worktrees.first().ok_or("No working copies found")?;
    let main = Path::new(&main.path);
    let parent = main.with_file_name(format!(
        "{}-worktrees",
        main.file_name().unwrap_or_default().to_string_lossy()
    ));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = format!("mc/native-{session}-{stamp}");
    let path = parent.join(name.replace('/', "-"));
    let commit = git(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )?;
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    git(
        cwd,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &name,
            "--",
            &path.to_string_lossy(),
            commit.trim(),
        ],
    )?;
    Ok(path)
}

/// SidebarWorktreeSwitcher: create a named copy from the selected checkout's
/// HEAD, alongside the main project's other worktrees. Existing files and
/// branches are never replaced.
pub fn create_named_worktree(cwd: &Path, branch: &str) -> Result<Worktree, String> {
    create_project_worktree(cwd, branch, None, false)
}
pub fn create_project_worktree(
    cwd: &Path,
    branch: &str,
    base: Option<&str>,
    existing: bool,
) -> Result<Worktree, String> {
    if crate::files::remote::is_remote(cwd) {
        return remote::create_worktree(cwd, branch.trim(), base, existing);
    }
    let branch = branch.trim();
    valid_branch(cwd, branch)?;
    let state = snapshot(cwd)?;
    if state
        .worktrees
        .iter()
        .any(|tree| tree.branch.as_deref() == Some(branch))
    {
        return Err("This branch already has a working copy. Select it from the picker.".into());
    }
    let main = Path::new(
        &state
            .worktrees
            .first()
            .ok_or("No working copies found")?
            .path,
    );
    let parent = main.with_file_name(format!(
        "{}-worktrees",
        main.file_name().unwrap_or_default().to_string_lossy()
    ));
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
    let reference = if existing {
        format!("refs/heads/{branch}")
    } else {
        base.unwrap_or("HEAD").into()
    };
    let commit = git(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    if existing {
        git(
            cwd,
            &["worktree", "add", "--", &path.to_string_lossy(), branch],
        )?;
    } else {
        git(
            cwd,
            &[
                "worktree",
                "add",
                "--no-track",
                "-b",
                branch,
                "--",
                &path.to_string_lossy(),
                commit.trim(),
            ],
        )?;
    }
    snapshot(cwd)?
        .worktrees
        .into_iter()
        .find(|tree| Path::new(&tree.path) == path)
        .ok_or_else(|| {
            "Worktree created, but could not be found. Refresh the working copies.".into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncommitted_only_checks_the_sessions_paths_staged_or_unstaged() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        git(&cwd, &["init"]).unwrap();
        git(&cwd, &["config", "core.autocrlf", "false"]).unwrap();
        git(&cwd, &["config", "user.name", "Native test"]).unwrap();
        git(&cwd, &["config", "user.email", "native@example.invalid"]).unwrap();
        for name in ["a[1].rs", "a1.rs"] {
            std::fs::write(cwd.join(name), "base\n").unwrap();
        }
        assert!(uncommitted(&cwd, &["a[1].rs".into()]), "unborn repository");
        git(&cwd, &["add", "."]).unwrap();
        git(
            &cwd,
            &["-c", "commit.gpgsign=false", "commit", "-m", "Base"],
        )
        .unwrap();
        let own = vec![cwd.join("a[1].rs").to_string_lossy().into_owned()];
        let outside = temp
            .path()
            .join("outside.rs")
            .to_string_lossy()
            .into_owned();
        std::fs::write(&outside, "outside\n").unwrap();
        std::fs::write(cwd.join("a1.rs"), "another session\n").unwrap();
        assert!(!uncommitted(&cwd, &[]));
        assert!(
            !uncommitted(&cwd, &own),
            "pathspec metacharacters are literal"
        );
        assert!(!uncommitted(&cwd, std::slice::from_ref(&outside)));

        std::fs::write(cwd.join("a[1].rs"), "this session\n").unwrap();
        assert!(uncommitted(&cwd, &own));
        assert!(uncommitted(&cwd, &["./a[1].rs".into()]));
        assert!(uncommitted(&cwd, &[outside, own[0].clone()]));
        let nested = cwd.join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert!(uncommitted(&nested, &["../a[1].rs".into()]));
        assert!(uncommitted(&nested, &own));

        git(&cwd, &["--literal-pathspecs", "add", "--", "a[1].rs"]).unwrap();
        assert!(uncommitted(&cwd, &own), "Stage is still uncommitted");
        git(
            &cwd,
            &["-c", "commit.gpgsign=false", "commit", "-m", "Own edits"],
        )
        .unwrap();
        assert!(!uncommitted(&cwd, &own), "other dirty files do not count");

        std::fs::remove_file(cwd.join("a[1].rs")).unwrap();
        assert!(uncommitted(&cwd, &own), "deleted paths still count");
        std::fs::write(cwd.join("a[1].rs"), "this session\n").unwrap();
        assert!(!uncommitted(&cwd, &own), "Undo clears the pill's condition");
        std::fs::write(cwd.join("new file.rs"), "new\n").unwrap();
        assert!(uncommitted(&cwd, &["new file.rs".into()]));
        git(&cwd, &["mv", "--", "a[1].rs", "renamed.rs"]).unwrap();
        assert!(uncommitted(&cwd, &own), "renamed source paths still count");
        assert!(uncommitted(&cwd, &["renamed.rs".into()]));
        assert!(!uncommitted(temp.path(), &own), "not a repository");
    }

    #[test]
    fn session_changes_split_staged_from_unstaged_and_track_pushes() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        git(&cwd, &["init", "-b", "main"]).unwrap();
        git(&cwd, &["config", "core.autocrlf", "false"]).unwrap();
        git(&cwd, &["config", "user.name", "Native test"]).unwrap();
        git(&cwd, &["config", "user.email", "native@example.invalid"]).unwrap();
        git(&cwd, &["config", "commit.gpgsign", "false"]).unwrap();
        std::fs::write(cwd.join("a.rs"), "base\n").unwrap();
        git(&cwd, &["add", "."]).unwrap();
        git(&cwd, &["commit", "-m", "Base"]).unwrap();
        let own = vec!["a.rs".to_owned(), "new.rs".to_owned()];
        assert_eq!(session_changes(&cwd, &own), SessionChanges::default());

        std::fs::write(cwd.join("a.rs"), "edited\n").unwrap();
        std::fs::write(cwd.join("new.rs"), "new\n").unwrap();
        let changes = session_changes(&cwd, &own);
        assert!(!changes.staged && changes.unstaged);
        git(&cwd, &["add", "a.rs"]).unwrap();
        let changes = session_changes(&cwd, &own);
        assert!(changes.staged && changes.unstaged, "new.rs is untracked");
        git(&cwd, &["add", "new.rs"]).unwrap();
        let changes = session_changes(&cwd, &own);
        assert!(changes.staged && !changes.unstaged, "everything is staged");

        git(&cwd, &["commit", "-m", "Own edits"]).unwrap();
        let head = git(&cwd, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
        let files = commit_files(&cwd, &head).unwrap();
        assert_eq!(
            files,
            [
                CommitFile {
                    path: "a.rs".into(),
                    additions: Some(1),
                    deletions: Some(1),
                },
                CommitFile {
                    path: "new.rs".into(),
                    additions: Some(1),
                    deletions: Some(0),
                },
            ]
        );
        assert!(unpushed(&cwd, &head), "no remote has it yet");
        let remote = temp.path().join("remote.git");
        git(temp.path(), &["init", "--bare", remote.to_str().unwrap()]).unwrap();
        git(&cwd, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
        git(&cwd, &["push", "-u", "origin", "main"]).unwrap();
        assert!(!unpushed(&cwd, &head), "pushed");
        assert!(!unpushed(&cwd, "--all"));
    }

    #[test]
    fn sidebar_named_copies_preserve_dirty_files_and_refuse_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        git(&cwd, &["init", "-b", "main"]).unwrap();
        git(&cwd, &["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(cwd.join("file"), "committed\n").unwrap();
        git(&cwd, &["add", "file"]).unwrap();
        git(
            &cwd,
            &[
                "-c",
                "user.name=Native test",
                "-c",
                "user.email=native@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "Initial",
            ],
        )
        .unwrap();
        std::fs::write(cwd.join("file"), "unsaved checkout changes\n").unwrap();
        let tree = create_named_worktree(&cwd, "feature/sidebar").unwrap();
        assert_eq!(tree.branch.as_deref(), Some("feature/sidebar"));
        assert_eq!(
            Path::new(&tree.path).file_name().unwrap(),
            "feature-sidebar"
        );
        assert_eq!(
            std::fs::read_to_string(cwd.join("file")).unwrap(),
            "unsaved checkout changes\n"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("file")).unwrap(),
            "committed\n"
        );
        assert!(create_named_worktree(&cwd, "feature/sidebar")
            .unwrap_err()
            .contains("already has a working copy"));
        assert!(create_named_worktree(&cwd, "-invalid").is_err());
        let collision = temp.path().join("project-worktrees/collision");
        std::fs::create_dir(&collision).unwrap();
        std::fs::write(collision.join("keep"), "keep").unwrap();
        assert!(create_named_worktree(&cwd, "collision")
            .unwrap_err()
            .contains("already exists"));
        assert_eq!(
            std::fs::read_to_string(collision.join("keep")).unwrap(),
            "keep"
        );
    }
    #[test]
    fn working_copy_operations_preserve_dirty_files_and_track_remotes() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir(&cwd).unwrap();
        git(&cwd, &["init", "-b", "main"]).unwrap();
        git(&cwd, &["config", "core.autocrlf", "false"]).unwrap();
        git(&cwd, &["config", "user.name", "Native review"]).unwrap();
        git(&cwd, &["config", "user.email", "native@example.invalid"]).unwrap();
        std::fs::write(cwd.join("file"), "main\n").unwrap();
        git(&cwd, &["add", "file"]).unwrap();
        git(
            &cwd,
            &["-c", "commit.gpgsign=false", "commit", "-m", "Initial"],
        )
        .unwrap();
        create_branch(&cwd, "feature").unwrap();
        std::fs::write(cwd.join("file"), "feature\n").unwrap();
        git(&cwd, &["add", "file"]).unwrap();
        git(
            &cwd,
            &["-c", "commit.gpgsign=false", "commit", "-m", "Feature"],
        )
        .unwrap();
        checkout(
            &cwd,
            &Branch {
                name: "main".into(),
                remote: None,
            },
        )
        .unwrap();
        std::fs::write(cwd.join("file"), "my unsaved work\n").unwrap();
        assert!(checkout(
            &cwd,
            &Branch {
                name: "feature".into(),
                remote: None
            }
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(cwd.join("file")).unwrap(),
            "my unsaved work\n"
        );
        assert_eq!(snapshot(&cwd).unwrap().current.as_deref(), Some("main"));
        let tree = create_worktree(&cwd, "feature", 7).unwrap();
        let state = snapshot(&tree).unwrap();
        assert!(state.linked);
        assert_eq!(state.worktrees.len(), 2);
        assert_eq!(
            std::fs::read_to_string(tree.join("file")).unwrap(),
            "feature\n"
        );
        git(&cwd, &["remote", "add", "origin", "/unused"]).unwrap();
        git(
            &cwd,
            &["update-ref", "refs/remotes/origin/remote-feature", "HEAD"],
        )
        .unwrap();
        checkout(
            &cwd,
            &Branch {
                name: "remote-feature".into(),
                remote: Some("origin".into()),
            },
        )
        .unwrap();
        assert_eq!(
            snapshot(&cwd).unwrap().current.as_deref(),
            Some("remote-feature")
        );
        assert_eq!(
            git(&cwd, &["rev-parse", "--abbrev-ref", "@{upstream}"])
                .unwrap()
                .trim(),
            "origin/remote-feature"
        );
        assert!(create_branch(&cwd, "--orphan").is_err());
    }
}
