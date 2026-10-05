//! Port of host/git-branches.ts: lists, switches, and creates branches in a
//! host checkout. Only clean checkouts switch, and the desktop recognizes the
//! "commit your changes or stash" message and offers to stash or commit.

use std::path::Path;
use std::time::Duration;

use monocode_remote::host::exec::{ExecError, ExecOptions, exec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DIRTY_CHECKOUT: &str =
    "Commit your changes or stash them before switching branches on the host.";

fn git(cwd: &str, args: &[&str]) -> Result<String, ExecError> {
    exec(
        "git",
        args,
        ExecOptions {
            cwd: Some(Path::new(cwd).to_path_buf()),
            timeout: Duration::from_secs(10),
            max_buffer: 1024 * 1024,
            ..Default::default()
        },
    )
    .map(|output| output.stdout)
}

/// One remote-tracking branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRemoteBranch {
    pub remote: String,
    pub name: String,
}

/// `HostBranches`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostBranches {
    pub current: Option<String>,
    pub branches: Vec<String>,
    pub remotes: Vec<HostRemoteBranch>,
}

/// `hostBranches`.
pub fn host_branches(cwd: &str) -> Result<HostBranches, String> {
    let names = git(
        cwd,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    let remote_names = git(
        cwd,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(symref)",
            "refs/remotes",
        ],
    )?;
    let current = git(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|stdout| stdout.trim().to_string());
    let remotes = remote_names
        .split('\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let reference = parts.next().unwrap_or("");
            let symref = parts.next().unwrap_or("");
            let slash = reference.find('/')?;
            (slash > 0 && symref.is_empty()).then(|| HostRemoteBranch {
                remote: reference[..slash].to_string(),
                name: reference[slash + 1..].to_string(),
            })
        })
        .collect();
    Ok(HostBranches {
        current,
        branches: names
            .split('\n')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect(),
        remotes,
    })
}

/// `switchHostBranch`.
pub fn switch_host_branch(
    cwd: &str,
    branch: Option<&Value>,
    remote: Option<&Value>,
) -> Result<HostBranches, String> {
    let branch = match branch {
        Some(Value::String(branch))
            if monocode_core::js::len(branch) <= 255
                && !branch.is_empty()
                && !branch.starts_with('-') =>
        {
            branch.as_str()
        }
        _ => return Err("Invalid branch".into()),
    };
    let state = host_branches(cwd)?;
    let remote_ref = match remote {
        Some(Value::String(remote))
            if state
                .remotes
                .iter()
                .any(|entry| entry.remote == *remote && entry.name == branch) =>
        {
            Some(format!("refs/remotes/{remote}/{branch}"))
        }
        _ => None,
    };
    if !matches!(remote, None | Some(Value::Null)) && remote_ref.is_none() {
        return Err("Choose an available remote branch".into());
    }
    let local = state.branches.iter().any(|name| name == branch);
    if !local && remote_ref.is_none() {
        return Err("Choose an existing local branch".into());
    }
    if state.current.as_deref() == Some(branch) {
        return Ok(state);
    }
    let changes = git(cwd, &["status", "--porcelain", "--untracked-files=all"])?;
    if !changes.is_empty() {
        return Err(DIRTY_CHECKOUT.into());
    }
    match (&remote_ref, local) {
        (Some(remote_ref), false) => {
            git(cwd, &["switch", "--track", "-c", branch, remote_ref])?;
        }
        _ => {
            git(cwd, &["switch", branch])?;
        }
    }
    host_branches(cwd)
}

/// `createHostBranch`.
pub fn create_host_branch(cwd: &str, branch: Option<&Value>) -> Result<HostBranches, String> {
    let branch = match branch {
        Some(Value::String(branch))
            if !branch.is_empty()
                && monocode_core::js::len(branch) <= 255
                && !branch.starts_with('-')
                && !branch.starts_with('@') =>
        {
            branch.as_str()
        }
        _ => return Err("Enter a valid branch name".into()),
    };
    git(cwd, &["check-ref-format", "--branch", branch])?;
    let state = host_branches(cwd)?;
    if state.branches.iter().any(|name| name == branch) {
        return Err("Branch already exists".into());
    }
    let changes = git(cwd, &["status", "--porcelain", "--untracked-files=all"])?;
    if !changes.is_empty() {
        return Err(DIRTY_CHECKOUT.into());
    }
    git(cwd, &["switch", "-c", branch])?;
    host_branches(cwd)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn run_git(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    /// git-branches.test.ts: "lists and switches only clean existing local
    /// branches".
    #[test]
    fn lists_and_switches_only_clean_existing_local_branches() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let cwd = root.to_str().unwrap();
        let git = |args: &[&str]| run_git(root, args);
        git(&["init", "-q"]);
        git(&["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join("file.txt"), "initial").unwrap();
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
        git(&["branch", "feature"]);
        assert_eq!(
            host_branches(cwd).unwrap(),
            HostBranches {
                current: Some("main".into()),
                branches: vec!["feature".into(), "main".into()],
                remotes: vec![],
            }
        );
        assert_eq!(
            switch_host_branch(cwd, Some(&json!("feature")), None)
                .unwrap()
                .current
                .as_deref(),
            Some("feature")
        );
        assert!(
            switch_host_branch(cwd, Some(&json!("missing")), None)
                .unwrap_err()
                .contains("existing local branch")
        );
        std::fs::write(root.join("file.txt"), "changed").unwrap();
        assert!(
            switch_host_branch(cwd, Some(&json!("main")), None)
                .unwrap_err()
                .contains("Commit your changes or stash")
        );
        assert_eq!(
            host_branches(cwd).unwrap().current.as_deref(),
            Some("feature")
        );
        assert!(
            create_host_branch(cwd, Some(&json!("new-feature")))
                .unwrap_err()
                .contains("Commit your changes or stash")
        );
        std::fs::write(root.join("file.txt"), "initial").unwrap();
        assert_eq!(
            create_host_branch(cwd, Some(&json!("new-feature")))
                .unwrap()
                .current
                .as_deref(),
            Some("new-feature")
        );
        git(&["remote", "add", "origin", cwd]);
        git(&["update-ref", "refs/remotes/origin/review", "HEAD"]);
        assert!(
            host_branches(cwd)
                .unwrap()
                .remotes
                .contains(&HostRemoteBranch {
                    remote: "origin".into(),
                    name: "review".into(),
                })
        );
        assert_eq!(
            switch_host_branch(cwd, Some(&json!("review")), Some(&json!("origin")))
                .unwrap()
                .current
                .as_deref(),
            Some("review")
        );
    }
}
