//! Port of host/git-worktrees.ts: the worktrees of a host project, creating
//! one from a validated base, and renaming the temporary branch of an
//! automatically created worktree.
//!
//! monocode-git's worktree commands serve the local app: they record which
//! sessions use each worktree in the local database and name folders
//! differently, so the host keeps its own small versions.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use monocode_remote::host::exec::{ExecError, ExecOptions, exec};
use monocode_remote::host::protocol::HostWorktree;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

static AUTO_BRANCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^mc/[a-z0-9]{8}$").unwrap());

/// True for a branch name the composer generates for a new worktree.
pub fn is_auto_worktree_branch(branch: &str) -> bool {
    AUTO_BRANCH.is_match(branch)
}

fn git(cwd: &str, args: &[&str]) -> Result<String, ExecError> {
    exec(
        "git",
        args,
        ExecOptions {
            cwd: Some(PathBuf::from(cwd)),
            timeout: Duration::from_secs(10),
            max_buffer: 1024 * 1024,
            ..Default::default()
        },
    )
    .map(|output| output.stdout)
}

fn available(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// `realpathSync.native`, as a string.
fn real(path: &str) -> String {
    dunce::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// `HostWorktrees`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostWorktrees {
    pub worktrees: Vec<HostWorktree>,
    pub default_root: String,
}

fn parse(text: &str) -> Vec<HostWorktree> {
    let mut trees: Vec<HostWorktree> = Vec::new();
    for field in text.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            let is_main = trees.is_empty();
            trees.push(HostWorktree {
                path: path.to_string(),
                branch: None,
                head: String::new(),
                is_main,
                missing: false,
            });
        } else if let Some(tree) = trees.last_mut() {
            if let Some(head) = field.strip_prefix("HEAD ") {
                tree.head = head.to_string();
            } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                tree.branch = Some(branch.to_string());
            }
        }
    }
    trees
}

fn registered(cwd: &str) -> Result<Vec<HostWorktree>, String> {
    let output = git(cwd, &["worktree", "list", "--porcelain", "-z"])?;
    Ok(parse(&output)
        .into_iter()
        .map(|mut tree| {
            if available(&tree.path) {
                tree.path = real(&tree.path);
            }
            tree
        })
        .collect())
}

fn requested_path(project_cwd: &str, requested: Option<&Value>) -> Result<Option<String>, String> {
    match requested {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() || text == project_cwd => Ok(None),
        Some(Value::String(text))
            if !text.contains('\0') && monocode_core::js::len(text) <= 4096 =>
        {
            Ok(Some(text.clone()))
        }
        Some(_) => Err("Invalid working copy".into()),
    }
}

/// `resolveHostWorktree`: the project checkout, or one of its registered,
/// available worktrees.
pub fn resolve_host_worktree(
    project_cwd: &str,
    requested: Option<&Value>,
) -> Result<String, String> {
    let Some(requested) = requested_path(project_cwd, requested)? else {
        return Ok(project_cwd.into());
    };
    let actual = if available(&requested) {
        real(&requested)
    } else {
        requested
    };
    if actual == project_cwd {
        return Ok(project_cwd.into());
    }
    let target = registered(project_cwd)?
        .into_iter()
        .find(|tree| tree.path == actual);
    match target {
        Some(target) if available(&target.path) => Ok(real(&target.path)),
        _ => Err("Choose an available worktree of this project".into()),
    }
}

/// `resolveHostWorktreeAsync`: the same check through `hostWorktrees`.
pub fn resolve_host_worktree_listed(
    project_cwd: &str,
    requested: Option<&Value>,
) -> Result<String, String> {
    let Some(requested) = requested_path(project_cwd, requested)? else {
        return Ok(project_cwd.into());
    };
    let actual = if available(&requested) {
        real(&requested)
    } else {
        requested
    };
    if actual == project_cwd {
        return Ok(project_cwd.into());
    }
    let listed = host_worktrees(project_cwd)?;
    if !listed
        .worktrees
        .iter()
        .any(|tree| tree.path == actual && !tree.missing)
    {
        return Err("Choose an available worktree of this project".into());
    }
    Ok(actual)
}

/// `hostWorktrees`.
pub fn host_worktrees(cwd: &str) -> Result<HostWorktrees, String> {
    let output = git(cwd, &["worktree", "list", "--porcelain", "-z"])?;
    let worktrees: Vec<HostWorktree> = parse(&output)
        .into_iter()
        .map(|mut tree| {
            let exists = available(&tree.path);
            if exists {
                tree.path = real(&tree.path);
            }
            tree.missing = !exists;
            tree
        })
        .collect();
    let main = worktrees.first().ok_or("No working copies found")?;
    let main_path = Path::new(&main.path);
    let name = main_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let default_root = main_path
        .parent()
        .unwrap_or(Path::new(""))
        .join(format!("{name}-worktrees"))
        .to_string_lossy()
        .into_owned();
    Ok(HostWorktrees {
        worktrees,
        default_root,
    })
}

fn valid_branch_name(branch: &str) -> bool {
    !branch.is_empty()
        && monocode_core::js::len(branch) <= 120
        && !branch.starts_with('-')
        && !branch.starts_with('@')
}

/// `createHostWorktree`. `source_cwd` is the checkout whose refs resolve
/// `base`; it defaults to `cwd`.
pub fn create_host_worktree(
    cwd: &str,
    branch: Option<&Value>,
    base: Option<&Value>,
    existing: Option<&Value>,
    source_cwd: Option<&str>,
) -> Result<HostWorktree, String> {
    let source_cwd = source_cwd.unwrap_or(cwd);
    let branch = match branch {
        Some(Value::String(branch)) if valid_branch_name(branch) => branch.as_str(),
        _ => return Err("Enter a valid branch name".into()),
    };
    git(cwd, &["check-ref-format", "--branch", branch])?;
    let (base, existing) = match (base, existing) {
        (Some(Value::String(base)), Some(Value::Bool(existing)))
            if monocode_core::js::len(base) <= 255 =>
        {
            (base.as_str(), *existing)
        }
        _ => return Err("Invalid worktree base".into()),
    };
    let listed = host_worktrees(cwd)?;
    if listed
        .worktrees
        .iter()
        .any(|tree| tree.branch.as_deref() == Some(branch))
    {
        return Err("This branch already has a working copy. Select it from the picker.".into());
    }
    let slug: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = Path::new(&listed.default_root)
        .join(format!("wt-{slug}"))
        .to_string_lossy()
        .into_owned();
    if Path::new(&path).exists() {
        return Err(format!(
            "{path} already exists. Choose another branch name."
        ));
    }
    let refs: Vec<String> = git(
        cwd,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?
    .split('\n')
    .filter(|line| !line.is_empty())
    .map(str::to_string)
    .collect();
    let source = if existing {
        Some(format!("refs/heads/{branch}"))
    } else if base == "HEAD" {
        Some("HEAD".to_string())
    } else {
        refs.iter()
            .find(|reference| {
                **reference == format!("refs/heads/{base}")
                    || **reference == format!("refs/remotes/{base}")
            })
            .cloned()
    };
    let source = match source {
        Some(source) if !existing || refs.contains(&source) => source,
        _ => return Err("Choose an available base branch".into()),
    };
    let commit = git(
        source_cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{source}^{{commit}}"),
        ],
    )?
    .trim()
    .to_string();
    std::fs::create_dir_all(&listed.default_root).map_err(|error| error.to_string())?;
    if existing {
        git(cwd, &["worktree", "add", "--", &path, branch])?;
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
                &path,
                &commit,
            ],
        )?;
    }
    host_worktrees(cwd)?
        .worktrees
        .into_iter()
        .find(|tree| tree.path == path)
        .ok_or_else(|| "Worktree created, but could not be found. Refresh the picker.".into())
}

/// `renameHostWorktreeBranch`: renames only the temporary branch created for
/// this worktree, while `still_owned` says the session still wants it.
pub fn rename_host_worktree_branch(
    cwd: &str,
    path: &str,
    expected_branch: &str,
    branch: &str,
    still_owned: &dyn Fn() -> bool,
) -> Result<HostWorktree, String> {
    if !is_auto_worktree_branch(expected_branch) {
        return Err("This is not an automatically created worktree branch".into());
    }
    if !valid_branch_name(branch) {
        return Err("Enter a valid branch name".into());
    }
    git(cwd, &["check-ref-format", "--branch", branch])?;
    let tree = host_worktrees(cwd)?
        .worktrees
        .into_iter()
        .find(|entry| entry.path == path && !entry.missing);
    let tree = match tree {
        Some(tree) if !tree.is_main && tree.branch.as_deref() == Some(expected_branch) => tree,
        _ => return Err("The worktree branch has changed".into()),
    };
    if branch == expected_branch {
        return Ok(tree);
    }
    if !still_owned() {
        return Err("The session no longer owns this branch".into());
    }
    git(&tree.path, &["branch", "-m", expected_branch, branch])?;
    host_worktrees(cwd)?
        .worktrees
        .into_iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| "Branch renamed, but its worktree could not be found".into())
}

/// `namedWorktreeBranch` from src/features/source-control/model/worktrees.ts:
/// a fragment under `mc/`, without a repeated `mc/` or `monocode/` prefix.
pub fn named_worktree_branch(fragment: &str) -> Option<String> {
    let mut clean = monocode_core::js::trim(fragment);
    for prefix in ["mc/", "monocode/"] {
        if let Some(rest) = clean.strip_prefix(prefix) {
            clean = rest.trim_start_matches('/');
            break;
        }
    }
    let clean = clean.trim_matches('/');
    (!clean.is_empty()).then(|| format!("mc/{clean}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_branches::tests::run_git;
    use serde_json::json;

    pub(crate) fn repository() -> (tempfile::TempDir, String) {
        let directory = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(directory.path()).unwrap();
        let git = |args: &[&str]| run_git(&root, args);
        git(&["init", "-q"]);
        git(&["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join("file.txt"), "initial\n").unwrap();
        git(&["add", "file.txt"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-q",
            "-m",
            "initial",
        ]);
        (directory, root.to_string_lossy().into_owned())
    }

    /// The worktree half of git-worktrees.test.ts: "creates registered host
    /// worktrees and binds new sessions to the selected checkout". The
    /// session half is in engine tests.
    #[test]
    fn creates_registered_host_worktrees() {
        let (_directory, cwd) = repository();
        run_git(Path::new(&cwd), &["branch", "existing"]);
        let root = host_worktrees(&cwd).unwrap().default_root;
        let tree = create_host_worktree(
            &cwd,
            Some(&json!("feature/task")),
            Some(&json!("HEAD")),
            Some(&json!(false)),
            None,
        )
        .unwrap();
        assert_eq!(tree.branch.as_deref(), Some("feature/task"));
        assert!(!tree.is_main);
        assert!(!tree.missing);
        assert!(tree.path.ends_with("wt-feature-task"));
        assert_eq!(
            resolve_host_worktree(&cwd, Some(&json!(tree.path))).unwrap(),
            tree.path
        );
        assert_eq!(
            resolve_host_worktree_listed(&cwd, Some(&json!(tree.path))).unwrap(),
            tree.path
        );
        let listed = host_worktrees(&cwd).unwrap();
        assert_eq!(
            listed
                .worktrees
                .iter()
                .map(|item| item.branch.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
            ["main", "feature/task"]
        );
        let outside = std::env::temp_dir().to_string_lossy().into_owned();
        assert!(
            resolve_host_worktree(&cwd, Some(&json!(outside)))
                .unwrap_err()
                .contains("Choose an available worktree")
        );
        assert!(
            create_host_worktree(
                &cwd,
                Some(&json!("feature/task")),
                Some(&json!("HEAD")),
                Some(&json!(false)),
                None
            )
            .unwrap_err()
            .contains("already has a working copy")
        );
        assert!(
            create_host_worktree(
                &cwd,
                Some(&json!("other")),
                Some(&json!("no-such")),
                Some(&json!(false)),
                None
            )
            .unwrap_err()
            .contains("available base branch")
        );
        let existing = create_host_worktree(
            &cwd,
            Some(&json!("existing")),
            Some(&json!("HEAD")),
            Some(&json!(true)),
            None,
        )
        .unwrap();
        assert_eq!(existing.branch.as_deref(), Some("existing"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renames_only_the_temporary_branch_it_owns() {
        let (_directory, cwd) = repository();
        let root = host_worktrees(&cwd).unwrap().default_root;
        let tree = create_host_worktree(
            &cwd,
            Some(&json!("mc/12345678")),
            Some(&json!("HEAD")),
            Some(&json!(false)),
            None,
        )
        .unwrap();
        assert!(
            rename_host_worktree_branch(&cwd, &tree.path, "mc/12345678", "mc/named", &|| false)
                .unwrap_err()
                .contains("no longer owns")
        );
        let renamed =
            rename_host_worktree_branch(&cwd, &tree.path, "mc/12345678", "mc/named", &|| true)
                .unwrap();
        assert_eq!(renamed.branch.as_deref(), Some("mc/named"));
        assert!(
            rename_host_worktree_branch(&cwd, &tree.path, "mc/12345678", "mc/other", &|| true)
                .unwrap_err()
                .contains("changed")
        );
        assert!(
            rename_host_worktree_branch(&cwd, &tree.path, "feature", "mc/other", &|| true)
                .unwrap_err()
                .contains("not an automatically created")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn names_worktree_branches_under_mc() {
        assert_eq!(named_worktree_branch(" fix-it "), Some("mc/fix-it".into()));
        assert_eq!(named_worktree_branch("mc/fix-it"), Some("mc/fix-it".into()));
        assert_eq!(
            named_worktree_branch("monocode//fix-it/"),
            Some("mc/fix-it".into())
        );
        assert_eq!(named_worktree_branch("mc/"), None);
    }
}
