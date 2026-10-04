//! The data half of the pickers the quick git popup shows: BranchPicker.tsx
//! (rows, the create row, switch errors) and WorkspacePicker.tsx (worktree
//! base rows, existing worktrees).

use monocode_core::js;

use super::launch::{GitBranchInfo, GitBranches, Worktree};

/// `BranchRow`: a branch with `current` set for the selected one only.
pub fn branch_rows(branches: Option<&GitBranches>, query: &str) -> Vec<GitBranchInfo> {
    let Some(branches) = branches else {
        return Vec::new();
    };
    let needle = js::trim(query).to_lowercase();
    let selected = branches.current.as_deref();
    branches
        .branches
        .iter()
        .filter(|entry| {
            if needle.is_empty() {
                return true;
            }
            let hay = match &entry.remote {
                Some(remote) => format!("{} {remote}", entry.name),
                None => entry.name.clone(),
            };
            hay.to_lowercase().contains(&needle)
        })
        .map(|entry| GitBranchInfo {
            name: entry.name.clone(),
            current: Some(entry.name.as_str()) == selected && entry.remote.is_none(),
            remote: entry.remote.clone(),
        })
        .collect()
}

/// `createRow`: offered unless a local branch already has the typed name.
/// An empty name means "New branch", which opens the name form.
pub fn create_row(branches: Option<&GitBranches>, query: &str) -> Option<String> {
    let name = js::trim(query);
    let taken = branches.is_some_and(|branches| {
        branches
            .branches
            .iter()
            .any(|entry| entry.remote.is_none() && entry.name == name)
    });
    (!taken).then(|| name.to_string())
}

/// The create row's label.
pub fn create_label(name: &str) -> String {
    if name.is_empty() {
        "New branch".into()
    } else {
        format!("Create and checkout {name}")
    }
}

/// The trigger label: `detached <sha>` for a detached head.
pub fn branch_label(branches: Option<&GitBranches>) -> String {
    match branches.and_then(|branches| branches.current.as_deref()) {
        Some(current) if branches.is_some_and(|b| b.detached) => format!("detached {current}"),
        Some(current) => current.to_string(),
        None => "No repo".into(),
    }
}

/// `WorktreeBasePicker` rows: unique by `remote/name`, filtered by the
/// query.
pub fn base_rows(branches: &[GitBranchInfo], query: &str) -> Vec<GitBranchInfo> {
    let needle = js::trim(query).to_lowercase();
    let mut unique: Vec<GitBranchInfo> = Vec::new();
    for branch in branches {
        let reference = branch.reference();
        match unique.iter_mut().find(|item| item.reference() == reference) {
            // A Map keeps the first key's place and the last value.
            Some(existing) => *existing = branch.clone(),
            None => unique.push(branch.clone()),
        }
    }
    unique
        .into_iter()
        .filter(|branch| branch.reference().to_lowercase().contains(&needle))
        .collect()
}

/// The worktrees the "Existing worktree" submenu lists.
pub fn existing_worktrees(worktrees: &[Worktree]) -> Vec<Worktree> {
    worktrees
        .iter()
        .filter(|tree| !tree.is_main && !tree.missing)
        .cloned()
        .collect()
}

/// A worktree row's title: its branch, else the short head.
pub fn worktree_title(tree: &Worktree) -> String {
    match &tree.branch {
        Some(branch) => branch.clone(),
        None => format!("Detached {}", tree.head.chars().take(7).collect::<String>()),
    }
}

/// `isSwitchBlockedByRunningSessions`: a connected machine refused because
/// sessions are running there.
pub fn is_switch_blocked_by_running_sessions(message: &str) -> bool {
    message
        .to_lowercase()
        .contains("switching branches changes the files")
}

/// `isCheckoutBlockedByChanges`: git refused because the working tree would
/// be overwritten.
pub fn is_checkout_blocked_by_changes(message: &str) -> bool {
    let text = message.to_lowercase();
    text.contains("would be overwritten")
        || text.contains("commit your changes or stash")
        || text.contains("please move or remove them before")
}

/// The host's explanation without its transport prefix.
pub fn running_sessions_message(message: &str) -> String {
    let trimmed = message
        .strip_prefix("Host rejected request:")
        .map(|rest| rest.trim_start_matches(js::is_space))
        .unwrap_or(message);
    trimmed.to_string()
}

/// A branch switch the popup is about to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingSwitch {
    Create {
        name: String,
        force: bool,
    },
    Checkout {
        name: String,
        remote: Option<String>,
        force: bool,
    },
}

impl PendingSwitch {
    pub fn name(&self) -> &str {
        match self {
            PendingSwitch::Create { name, .. } | PendingSwitch::Checkout { name, .. } => name,
        }
    }

    pub fn creating(&self) -> bool {
        matches!(self, PendingSwitch::Create { .. })
    }

    /// The same switch, agreed to under running sessions.
    pub fn forced(&self) -> Self {
        match self.clone() {
            PendingSwitch::Create { name, .. } => PendingSwitch::Create { name, force: true },
            PendingSwitch::Checkout { name, remote, .. } => PendingSwitch::Checkout {
                name,
                remote,
                force: true,
            },
        }
    }
}

/// The "Uncommitted changes" explanation.
pub fn blocked_message(pending: &PendingSwitch) -> String {
    let verb = if pending.creating() {
        "Creating"
    } else {
        "Switching to"
    };
    format!(
        "{verb} \u{201c}{}\u{201d} would overwrite your local changes. Stash them for later, or commit them on this branch first.",
        pending.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> GitBranches {
        GitBranches {
            current: Some("main".into()),
            detached: false,
            branches: vec![
                GitBranchInfo::local("main", true),
                GitBranchInfo::remote("develop", "origin"),
                GitBranchInfo::local("feature/popup", false),
            ],
        }
    }

    #[test]
    fn rows_mark_only_the_local_current_branch() {
        let rows = branch_rows(Some(&snapshot()), "");
        assert_eq!(rows.len(), 3);
        assert!(rows[0].current);
        assert!(!rows[1].current);
        let remote = branch_rows(Some(&snapshot()), "origin");
        assert_eq!(remote.len(), 1);
        assert_eq!(remote[0].name, "develop");
        assert!(branch_rows(None, "").is_empty());
    }

    #[test]
    fn create_row_hides_for_an_existing_local_name() {
        assert_eq!(create_row(Some(&snapshot()), ""), Some(String::new()));
        assert_eq!(create_row(Some(&snapshot()), " main "), None);
        assert_eq!(
            create_row(Some(&snapshot()), "develop"),
            Some("develop".into())
        );
        assert_eq!(create_label(""), "New branch");
        assert_eq!(create_label("x"), "Create and checkout x");
    }

    #[test]
    fn labels_detached_heads_and_missing_repos() {
        let mut detached = snapshot();
        detached.detached = true;
        detached.current = Some("abc1234".into());
        assert_eq!(branch_label(Some(&detached)), "detached abc1234");
        assert_eq!(branch_label(None), "No repo");
        assert_eq!(branch_label(Some(&snapshot())), "main");
    }

    #[test]
    fn base_rows_are_unique_references() {
        let mut branches = snapshot().branches;
        branches.push(GitBranchInfo::remote("develop", "origin"));
        let rows = base_rows(&branches, "");
        let refs: Vec<String> = rows.iter().map(GitBranchInfo::reference).collect();
        assert_eq!(refs, ["main", "origin/develop", "feature/popup"]);
        assert_eq!(base_rows(&branches, "ORIGIN/").len(), 1);
    }

    #[test]
    fn existing_worktrees_skip_the_main_and_missing_ones() {
        let mut main = Worktree::linked("/repo", "main", "aaa");
        main.is_main = true;
        let mut gone = Worktree::linked("/gone", "gone", "bbb");
        gone.missing = true;
        let mut detached = Worktree::linked("/repo-x", "x", "abcdef123");
        detached.branch = None;
        let trees = existing_worktrees(&[main, gone, detached]);
        assert_eq!(trees.len(), 1);
        assert_eq!(worktree_title(&trees[0]), "Detached abcdef1");
    }

    #[test]
    fn recognizes_switch_errors() {
        assert!(is_checkout_blocked_by_changes(
            "error: Your local changes to the following files would be overwritten by checkout"
        ));
        assert!(is_switch_blocked_by_running_sessions(
            "Host rejected request: Switching branches changes the files 2 sessions use"
        ));
        assert_eq!(
            running_sessions_message("Host rejected request:  Two sessions are running"),
            "Two sessions are running"
        );
        let pending = PendingSwitch::Create {
            name: "x".into(),
            force: false,
        };
        assert!(blocked_message(&pending).starts_with("Creating \u{201c}x\u{201d}"));
        assert_eq!(
            pending.forced(),
            PendingSwitch::Create {
                name: "x".into(),
                force: true
            }
        );
    }
}
