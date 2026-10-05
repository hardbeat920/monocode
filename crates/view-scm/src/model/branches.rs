//! The pure parts of src/features/source-control/ui/BranchPicker.tsx: the
//! filtered rows, the create row, and the trigger label.

use crate::git::{GitBranchEntry, GitBranches};

/// `PendingSwitch`. `force` is set once the user agreed to switch under
/// running sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
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
            Self::Create { name, .. } | Self::Checkout { name, .. } => name,
        }
    }

    pub fn creating(&self) -> bool {
        matches!(self, Self::Create { .. })
    }

    pub fn forced(&self) -> Self {
        let mut next = self.clone();
        match &mut next {
            Self::Create { force, .. } | Self::Checkout { force, .. } => *force = true,
        }
        next
    }
}

/// `rows`: branches matching `query`, with `current` recomputed for the
/// branch the picker shows as selected.
pub fn branch_rows(
    branches: Option<&GitBranches>,
    branch: Option<&str>,
    query: &str,
) -> Vec<GitBranchEntry> {
    let Some(branches) = branches else {
        return Vec::new();
    };
    let needle = query.trim().to_lowercase();
    let selected = branch
        .filter(|branch| !branch.is_empty())
        .or(branches.current.as_deref());
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
        .map(|entry| GitBranchEntry {
            name: entry.name.clone(),
            current: Some(entry.name.as_str()) == selected && entry.remote.is_none(),
            remote: entry.remote.clone(),
        })
        .collect()
}

/// `createRow`: the fixed create action, or `None` when a local branch
/// already has the typed name. An empty name opens the dialog.
pub fn create_row(branches: Option<&GitBranches>, query: &str) -> Option<String> {
    let name = query.trim();
    let taken = branches.is_some_and(|branches| {
        branches
            .branches
            .iter()
            .any(|entry| entry.remote.is_none() && entry.name == name)
    });
    (!taken).then(|| name.to_string())
}

pub fn create_row_label(name: &str) -> String {
    if name.is_empty() {
        "New branch".into()
    } else {
        format!("Create and checkout {name}")
    }
}

/// What the trigger shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchTrigger {
    pub awaiting: bool,
    pub missing_git: bool,
    pub label: String,
    pub title: String,
    pub aria_label: String,
    pub interactive: bool,
}

pub fn branch_trigger(
    cwd: &str,
    branch: Option<&str>,
    branches: Option<&GitBranches>,
    settled: bool,
    enabled: bool,
) -> BranchTrigger {
    let in_project = !cwd.is_empty() && cwd != "~";
    let branch = branch.filter(|branch| !branch.is_empty());
    let current = branch.or(branches.and_then(|branches| branches.current.as_deref()));
    let detached = branch.is_none() && branches.is_some_and(|branches| branches.detached);
    let awaiting = in_project && current.is_none() && !settled;
    let missing_git = current.is_none() && !awaiting;
    let label = match current {
        Some(current) if detached => format!("detached {current}"),
        Some(current) => current.to_string(),
        None => "No repo".into(),
    };
    let title = if awaiting {
        "Loading branch…".into()
    } else if missing_git {
        "No git repository".into()
    } else {
        label.clone()
    };
    let aria_label = if awaiting {
        "Loading branch".into()
    } else if missing_git {
        "No git repository".into()
    } else {
        format!("Branch {label}")
    };
    BranchTrigger {
        awaiting,
        missing_git,
        title,
        aria_label,
        interactive: enabled && !awaiting && !missing_git,
        label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branches(names: &[(&str, Option<&str>)]) -> GitBranches {
        GitBranches {
            current: Some("main".into()),
            detached: false,
            branches: names
                .iter()
                .map(|(name, remote)| GitBranchEntry {
                    name: name.to_string(),
                    current: *name == "main",
                    remote: remote.map(str::to_string),
                })
                .collect(),
        }
    }

    #[test]
    fn filters_by_name_and_remote() {
        let list = branches(&[
            ("main", None),
            ("feature/picker", None),
            ("release", Some("origin")),
        ]);
        let rows = branch_rows(Some(&list), Some("main"), "picker");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "feature/picker");
        assert_eq!(branch_rows(Some(&list), None, "origin")[0].name, "release");
        let all = branch_rows(Some(&list), Some("main"), "");
        assert!(all[0].current);
    }

    #[test]
    fn the_create_row_hides_for_an_existing_local_branch() {
        let list = branches(&[("main", None), ("release", Some("origin"))]);
        assert_eq!(create_row(Some(&list), " main "), None);
        assert_eq!(create_row(Some(&list), "release"), Some("release".into()));
        assert_eq!(create_row(Some(&list), ""), Some(String::new()));
        assert_eq!(create_row_label(""), "New branch");
        assert_eq!(
            create_row_label("feature/picker"),
            "Create and checkout feature/picker"
        );
    }

    #[test]
    fn the_trigger_tells_loading_from_missing_git() {
        let loading = branch_trigger("/repo", None, None, false, true);
        assert!(loading.awaiting && !loading.interactive);
        let missing = branch_trigger("/repo", None, None, true, true);
        assert_eq!(missing.label, "No repo");
        assert_eq!(missing.title, "No git repository");
        let mut detached = branches(&[("main", None)]);
        detached.detached = true;
        detached.current = Some("abc1234".into());
        let trigger = branch_trigger("/repo", None, Some(&detached), true, true);
        assert_eq!(trigger.label, "detached abc1234");
        assert!(trigger.interactive);
    }
}
