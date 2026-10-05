//! Port of src/features/source-control/model/workingTreeDiff.ts.

use crate::git::{GitChangedFile, GitFileDiffKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingTreeDiffEntry {
    pub id: String,
    pub kind: GitFileDiffKind,
    pub file: GitChangedFile,
}

pub fn working_tree_diff_entry_id(kind: GitFileDiffKind, relative: &str) -> String {
    format!("{}:{relative}", kind.as_str())
}

/// Staged entries come first, matching the source control sidebar. `scope`
/// keeps only that side, for a review opened from one sidebar section.
pub fn working_tree_diff_entries(
    files: &[GitChangedFile],
    scope: Option<GitFileDiffKind>,
) -> Vec<WorkingTreeDiffEntry> {
    let mut entries = Vec::new();
    let kinds = match scope {
        Some(kind) => vec![kind],
        None => vec![GitFileDiffKind::Staged, GitFileDiffKind::Unstaged],
    };
    for kind in kinds {
        for file in files {
            let included = match kind {
                GitFileDiffKind::Staged => file.staged,
                GitFileDiffKind::Unstaged => file.unstaged,
            };
            if included {
                entries.push(WorkingTreeDiffEntry {
                    id: working_tree_diff_entry_id(kind, &file.relative),
                    kind,
                    file: file.clone(),
                });
            }
        }
    }
    entries
}

pub fn working_tree_diff_entry_label(entry: &WorkingTreeDiffEntry) -> String {
    if !entry.file.staged || !entry.file.unstaged {
        return entry.file.relative.clone();
    }
    let side = match entry.kind {
        GitFileDiffKind::Staged => "Staged",
        GitFileDiffKind::Unstaged => "Unstaged",
    };
    format!("{} ({side})", entry.file.relative)
}

pub fn working_tree_diff_focus_id(
    entries: &[WorkingTreeDiffEntry],
    focus_path: Option<&str>,
    focus_kind: Option<GitFileDiffKind>,
) -> Option<String> {
    let focus_path = focus_path.filter(|path| !path.is_empty())?;
    entries
        .iter()
        .find(|entry| {
            focus_kind.is_none_or(|kind| entry.kind == kind)
                && (entry.file.path == focus_path || entry.file.relative == focus_path)
        })
        .map(|entry| entry.id.clone())
}

pub fn prioritize_working_tree_diff_entries(
    entries: &[WorkingTreeDiffEntry],
    focus_path: Option<&str>,
    focus_kind: Option<GitFileDiffKind>,
) -> Vec<WorkingTreeDiffEntry> {
    let Some(focus_id) = working_tree_diff_focus_id(entries, focus_path, focus_kind) else {
        return entries.to_vec();
    };
    let Some(focused) = entries.iter().find(|entry| entry.id == focus_id) else {
        return entries.to_vec();
    };
    let mut out = vec![focused.clone()];
    out.extend(entries.iter().filter(|entry| entry.id != focus_id).cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(relative: &str, staged: bool, unstaged: bool) -> GitChangedFile {
        GitChangedFile {
            path: format!("/repo/{relative}"),
            relative: relative.into(),
            status: "modified".into(),
            additions: 1,
            deletions: 0,
            staged,
            unstaged,
        }
    }

    #[test]
    fn creates_a_staged_entry_for_a_clean_staged_file() {
        let entries = working_tree_diff_entries(&[file("a.ts", true, false)], None);
        let ids: Vec<(&str, GitFileDiffKind)> = entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry.kind))
            .collect();
        assert_eq!(ids, vec![("staged:a.ts", GitFileDiffKind::Staged)]);
    }

    #[test]
    fn creates_both_comparisons_for_a_partially_staged_file() {
        let entries = working_tree_diff_entries(&[file("a.ts", true, true)], None);
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, vec!["staged:a.ts", "unstaged:a.ts"]);
        let labels: Vec<String> = entries.iter().map(working_tree_diff_entry_label).collect();
        assert_eq!(labels, vec!["a.ts (Staged)", "a.ts (Unstaged)"]);
    }

    #[test]
    fn keeps_only_the_scoped_side() {
        let files = [
            file("a.ts", true, true),
            file("b.ts", true, false),
            file("c.ts", false, true),
        ];
        let ids = |scope| -> Vec<String> {
            working_tree_diff_entries(&files, Some(scope))
                .into_iter()
                .map(|entry| entry.id)
                .collect()
        };
        assert_eq!(
            ids(GitFileDiffKind::Unstaged),
            vec!["unstaged:a.ts", "unstaged:c.ts"]
        );
        assert_eq!(
            ids(GitFileDiffKind::Staged),
            vec!["staged:a.ts", "staged:b.ts"]
        );
    }

    #[test]
    fn focuses_and_prioritizes_the_selected_comparison() {
        let entries =
            working_tree_diff_entries(&[file("a.ts", true, true), file("b.ts", false, true)], None);
        assert_eq!(
            working_tree_diff_focus_id(
                &entries,
                Some("/repo/a.ts"),
                Some(GitFileDiffKind::Unstaged)
            ),
            Some("unstaged:a.ts".to_string())
        );
        let ordered = prioritize_working_tree_diff_entries(
            &entries,
            Some("/repo/a.ts"),
            Some(GitFileDiffKind::Unstaged),
        );
        assert_eq!(ordered[0].id, "unstaged:a.ts");
    }
}
