//! The parts of src/features/source-control/model/worktrees.ts the views
//! use (`worktreeSessionIds`, `NO_BRANCH_LABEL`), and the pure logic of
//! WorktreePicker.tsx, WorktreesPage.tsx, and DeleteWorktreeDialog.tsx.

use std::collections::BTreeSet;

use crate::git::Worktree;
use crate::paths::{is_equal_or_inside, path_key, pretty_cwd};

pub const NO_BRANCH_LABEL: &str = "No branch selected";

/// The session fields `worktreeSessionIds` reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveSession {
    pub id: String,
    pub cwd: String,
    pub worktree_cwd: Option<String>,
    pub worktree_removed: bool,
}

/// `worktreeSessionIds`: the saved sessions in a working copy, corrected by
/// the live sessions (which override their last saved context).
pub fn worktree_session_ids(tree: &Worktree, sessions: &[LiveSession]) -> Vec<String> {
    // Insertion order matters to the caller, as with a JS Set.
    let mut ids: Vec<String> = Vec::new();
    for id in &tree.session_ids {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    for session in sessions {
        ids.retain(|id| id != &session.id);
        let cwd = session
            .worktree_cwd
            .as_deref()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or(&session.cwd);
        if !session.worktree_removed && is_equal_or_inside(cwd, &tree.path) {
            ids.push(session.id.clone());
        }
    }
    ids
}

/// The picker's rows matching `query` on branch and path.
pub fn filter_worktrees<'a>(trees: &'a [Worktree], query: &str) -> Vec<&'a Worktree> {
    let needle = query.to_lowercase();
    trees
        .iter()
        .filter(|tree| {
            format!(
                "{} {}",
                tree.branch.as_deref().unwrap_or("detached"),
                tree.path
            )
            .to_lowercase()
            .contains(&needle)
        })
        .collect()
}

/// The highlighted row: the one at `active_path`, else the current working
/// copy, else the first. Tracking by path keeps background refreshes from
/// moving the highlight.
pub fn active_worktree_index(
    rows: &[&Worktree],
    active_path: Option<&str>,
    execution_cwd: &str,
) -> usize {
    let target = path_key(active_path.unwrap_or(execution_cwd));
    rows.iter()
        .position(|tree| path_key(&tree.path) == target)
        .unwrap_or(0)
}

/// The picker trigger's label.
pub fn worktree_trigger_label(
    worktree_removed: bool,
    current: Option<&str>,
    detached: bool,
    settled: bool,
    in_worktree: bool,
) -> String {
    if worktree_removed {
        return NO_BRANCH_LABEL.into();
    }
    match current {
        Some(current) if detached => format!("detached {current}"),
        Some(current) => current.to_string(),
        None if settled && in_worktree => "Worktree unavailable".into(),
        None if settled => "No repo".into(),
        None => "Loading…".into(),
    }
}

/// A picker row's two lines.
pub fn worktree_row_text(tree: &Worktree) -> (String, String) {
    let title = match &tree.branch {
        Some(branch) => branch.clone(),
        None => format!("Detached {}", short_head(&tree.head)),
    };
    let mut detail = if tree.is_main {
        "Project folder".to_string()
    } else {
        pretty_cwd(&tree.path)
    };
    if tree.missing {
        detail.push_str(" · Missing");
    }
    (title, detail)
}

pub fn short_head(head: &str) -> &str {
    let end = head.char_indices().nth(7).map_or(head.len(), |(i, _)| i);
    &head[..end]
}

/// Why a worktree cannot be deleted from the settings page.
pub fn delete_blocked_reason(tree: &Worktree) -> Option<&'static str> {
    if tree.locked {
        Some("Unlock this worktree in Git first")
    } else if tree.branch.is_none() {
        Some("Create a branch before deleting this detached worktree")
    } else {
        None
    }
}

/// The page's status text for a worktree.
pub fn worktree_status_label(tree: &Worktree) -> &'static str {
    if tree.missing {
        "Missing folder"
    } else {
        match tree.dirty {
            None => "Status unavailable",
            Some(true) => "Uncommitted changes",
            Some(false) => "Clean",
        }
    }
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The page's project choices: the current folder first, then recents, then
/// archived projects, without repeats.
pub fn project_choices(cwd: &str, recents: &[String], archived: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut choices = Vec::new();
    for path in std::iter::once(cwd)
        .chain(recents.iter().map(String::as_str))
        .chain(archived.iter().map(String::as_str))
    {
        if path.is_empty() || path == "~" {
            continue;
        }
        if seen.insert(path_key(path)) {
            choices.push(path.to_string());
        }
    }
    choices
}

/// The delete dialog's sessions line.
pub fn delete_sessions_text(count: usize, delete_sessions: bool) -> String {
    format!(
        "{count} session{} using this worktree {} {}",
        plural(count),
        if count == 1 { "is" } else { "are" },
        if delete_sessions {
            "permanently deleted."
        } else {
            "kept. Select a branch or worktree to continue them."
        }
    )
}

pub fn delete_unpushed_text(unpushed: i64) -> String {
    format!(
        "{unpushed} commit{} not on a remote. They stay on the branch.",
        if unpushed == 1 { " is" } else { "s are" }
    )
}

pub fn delete_button_label(session_count: usize, delete_sessions: bool) -> String {
    if session_count > 0 && delete_sessions {
        format!("Delete worktree and session{}", plural(session_count))
    } else {
        "Delete worktree".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn tree() -> Worktree {
        Worktree {
            path: "/repo-worktrees/feature".into(),
            branch: Some("feature".into()),
            head: "abc".into(),
            is_main: false,
            locked: false,
            prunable: false,
            missing: false,
            dirty: Some(true),
            unpushed: Some(2),
            session_ids: Vec::new(),
        }
    }

    #[test]
    fn live_sessions_override_saved_ids() {
        let mut saved = tree();
        saved.session_ids = vec!["moved".into(), "kept".into()];
        let ids = worktree_session_ids(
            &saved,
            &[
                LiveSession {
                    id: "moved".into(),
                    cwd: "/repo".into(),
                    ..Default::default()
                },
                LiveSession {
                    id: "new".into(),
                    cwd: "/repo".into(),
                    worktree_cwd: Some("/repo-worktrees/feature/src".into()),
                    worktree_removed: false,
                },
            ],
        );
        assert_eq!(ids, vec!["kept", "new"]);
    }

    #[test]
    fn rows_filter_and_highlight_by_path() {
        let main = Worktree {
            path: "/repo".into(),
            branch: Some("main".into()),
            is_main: true,
            ..tree()
        };
        let trees = vec![main, tree()];
        let rows = filter_worktrees(&trees, "main");
        assert_eq!(rows.len(), 1);
        let all = filter_worktrees(&trees, "");
        assert_eq!(
            active_worktree_index(&all, None, "/repo-worktrees/feature"),
            1
        );
        assert_eq!(
            active_worktree_index(&all, Some("/repo"), "/repo-worktrees/feature"),
            0
        );
        assert_eq!(active_worktree_index(&[], None, "/x"), 0);
    }

    #[test]
    fn labels() {
        assert_eq!(
            worktree_trigger_label(true, Some("main"), false, true, false),
            NO_BRANCH_LABEL
        );
        assert_eq!(
            worktree_trigger_label(false, None, false, true, true),
            "Worktree unavailable"
        );
        assert_eq!(worktree_row_text(&tree()).1, "/repo-worktrees/feature");
        assert_eq!(
            delete_sessions_text(2, false),
            "2 sessions using this worktree are kept. Select a branch or worktree to continue them."
        );
        assert_eq!(
            delete_sessions_text(1, true),
            "1 session using this worktree is permanently deleted."
        );
        assert_eq!(delete_button_label(1, true), "Delete worktree and session");
        assert_eq!(
            delete_unpushed_text(2),
            "2 commits are not on a remote. They stay on the branch."
        );
        assert_eq!(
            project_choices("/a", &["/b".into(), "/a/".into()], &["~".into()]),
            vec!["/a", "/b"]
        );
    }
}
