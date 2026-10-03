//! The pure parts of `GithubPrActions` in src/features/inbox/ui/InboxView.tsx
//! and the `GithubPrAction` type from src/features/inbox/model/githubTasks.ts.

/// `GithubPrAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GithubPrAction {
    Merge,
    Squash,
    Rebase,
    Draft,
    Ready,
    Close,
    Reopen,
}

impl GithubPrAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Close => "close",
            Self::Reopen => "reopen",
        }
    }

    pub fn is_merge(self) -> bool {
        matches!(self, Self::Merge | Self::Squash | Self::Rebase)
    }
}

/// One `GITHUB_PR_MERGE_OPTIONS` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeOption {
    pub action: GithubPrAction,
    pub label: &'static str,
    pub description: &'static str,
}

pub const GITHUB_PR_MERGE_OPTIONS: [MergeOption; 3] = [
    MergeOption {
        action: GithubPrAction::Merge,
        label: "Create a merge commit",
        description: "Add every commit to the base branch.",
    },
    MergeOption {
        action: GithubPrAction::Squash,
        label: "Squash and merge",
        description: "Combine the commits into one.",
    },
    MergeOption {
        action: GithubPrAction::Rebase,
        label: "Rebase and merge",
        description: "Add the commits without a merge commit.",
    },
];

/// `githubPrActionCopy`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionCopy {
    pub title: String,
    pub detail: String,
    pub confirm: &'static str,
    pub progress: &'static str,
}

pub fn github_pr_action_copy(action: GithubPrAction, base_ref: &str, head_ref: &str) -> ActionCopy {
    let source = if head_ref.is_empty() {
        "this branch".to_string()
    } else {
        format!("“{head_ref}”")
    };
    let destination = if base_ref.is_empty() {
        "the base branch".to_string()
    } else {
        format!("“{base_ref}”")
    };
    let (title, detail, confirm, progress) = match action {
        GithubPrAction::Merge => (
            "Merge this pull request?",
            format!(
                "Every commit from {source} will be added to {destination} with a merge commit."
            ),
            "Merge pull request",
            "Merging…",
        ),
        GithubPrAction::Squash => (
            "Squash and merge?",
            format!("The commits from {source} will be combined into one commit on {destination}."),
            "Squash and merge",
            "Merging…",
        ),
        GithubPrAction::Rebase => (
            "Rebase and merge?",
            format!("The commits from {source} will be rebased individually onto {destination}."),
            "Rebase and merge",
            "Merging…",
        ),
        GithubPrAction::Draft => (
            "Convert to draft?",
            "Reviewers will see that this pull request is not ready to merge.".to_string(),
            "Convert to draft",
            "Converting…",
        ),
        GithubPrAction::Ready => (
            "Mark as ready for review?",
            "Reviewers will see that this pull request is ready for feedback.".to_string(),
            "Ready for review",
            "Updating…",
        ),
        GithubPrAction::Close => (
            "Close this pull request?",
            "The pull request will close without merging. You can reopen it later.".to_string(),
            "Close pull request",
            "Closing…",
        ),
        GithubPrAction::Reopen => (
            "Reopen this pull request?",
            "The pull request will return to the open state.".to_string(),
            "Reopen pull request",
            "Reopening…",
        ),
    };
    ActionCopy {
        title: title.to_string(),
        detail,
        confirm,
        progress,
    }
}

/// The merge button's label for the chosen method.
pub fn merge_button_label(action: GithubPrAction) -> &'static str {
    if action == GithubPrAction::Merge {
        return "Merge pull request";
    }
    GITHUB_PR_MERGE_OPTIONS
        .iter()
        .find(|option| option.action == action)
        .map_or("Merge pull request", |option| option.label)
}

/// The notice after an action: a merge that GitHub queued instead of
/// finishing.
pub fn merge_notice(action: GithubPrAction, next_state: &str) -> Option<&'static str> {
    (action.is_merge() && next_state.trim().to_lowercase() != "merged")
        .then_some("Merge queued or auto-merge enabled.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_names_both_branches() {
        let copy = github_pr_action_copy(GithubPrAction::Squash, "main", "feature/inbox");
        assert_eq!(copy.title, "Squash and merge?");
        assert!(
            copy.detail
                .contains("from “feature/inbox” will be combined into one commit on “main”")
        );
        let fallback = github_pr_action_copy(GithubPrAction::Merge, "", "");
        assert!(
            fallback
                .detail
                .contains("from this branch will be added to the base branch")
        );
    }

    #[test]
    fn notices_and_labels() {
        assert_eq!(
            merge_notice(GithubPrAction::Squash, "OPEN"),
            Some("Merge queued or auto-merge enabled.")
        );
        assert_eq!(merge_notice(GithubPrAction::Squash, "merged"), None);
        assert_eq!(merge_notice(GithubPrAction::Close, "closed"), None);
        assert_eq!(
            merge_button_label(GithubPrAction::Merge),
            "Merge pull request"
        );
        assert_eq!(
            merge_button_label(GithubPrAction::Rebase),
            "Rebase and merge"
        );
    }
}
