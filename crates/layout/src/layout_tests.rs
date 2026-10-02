//! Port of src/features/workspace/model/layout.test.ts.

use super::*;
use crate::ids::random_uuid;

fn pin() -> OpenEditorTabOptions {
    OpenEditorTabOptions {
        pin: true,
        ..Default::default()
    }
}

fn plain() -> OpenEditorTabOptions {
    OpenEditorTabOptions::default()
}

fn file(path: &str, cwd: &str) -> FilePaneTab {
    new_file_tab(path, cwd, false, None, None)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> LayoutRect {
    LayoutRect { x, y, w, h }
}

fn ids(node: &LayoutNode) -> Vec<String> {
    layout_leaves(node)
        .into_iter()
        .map(|pane| pane.id)
        .collect()
}

fn all_files(tab: &WorkspaceTab) -> Vec<FilePaneTab> {
    tab.editor_panes
        .iter()
        .flat_map(|pane| pane.files.clone())
        .collect()
}

/// `toBeCloseTo` with the default precision of 2 digits.
fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.005,
        "expected {actual} to be close to {expected}"
    );
}

fn paths(tab: &WorkspaceTab) -> Vec<(String, bool)> {
    tab.editor_panes[0]
        .files
        .iter()
        .map(|file| (file.path.clone(), file.preview == Some(true)))
        .collect()
}

fn commit(sha: &str, short_sha: &str, subject: &str) -> CommitTabSource {
    CommitTabSource::new(sha, short_sha, subject)
}

// preview tabs

#[test]
fn keeps_remote_files_from_different_machines_in_distinct_editor_tabs() {
    let first = file("remote://machine-a/repo/a.ts", "remote://machine-a/repo");
    let second = file("remote://machine-b/repo/a.ts", "remote://machine-b/repo");
    assert_ne!(editor_tab_key(&first), editor_tab_key(&second));
    let tab = open_editor_tab(&new_tab("s"), &first, &pin());
    let tab = open_editor_tab(&tab, &second, &pin());
    assert_eq!(tab.editor_panes[0].files.len(), 2);
}

#[test]
fn keeps_remote_file_and_review_tabs_distinct_and_retargets_one_unified_review() {
    let cwd = "remote://machine/repo";
    let ordinary = file(&format!("{cwd}/a.ts"), cwd);
    let review = FilePaneTab {
        id: random_uuid(),
        review: Some(true),
        change_kind: Some(GitFileDiffKind::Staged),
        ..ordinary.clone()
    };
    let changes = new_changes_tab(
        cwd,
        Some(&format!("{cwd}/a.ts")),
        Some(GitFileDiffKind::Staged),
        None,
    );
    assert_ne!(editor_tab_key(&ordinary), editor_tab_key(&review));
    assert_ne!(editor_tab_key(&review), editor_tab_key(&changes));
    let tab = open_editor_tab(&new_tab("s"), &ordinary, &pin());
    let tab = open_editor_tab(&tab, &review, &pin());
    let tab = open_editor_tab(
        &tab,
        &FilePaneTab {
            change_kind: Some(GitFileDiffKind::Unstaged),
            ..review.clone()
        },
        &plain(),
    );
    assert_eq!(
        all_files(&tab)
            .into_iter()
            .find(|file| file.review == Some(true) && file.changes != Some(true))
            .and_then(|file| file.change_kind),
        Some(GitFileDiffKind::Unstaged)
    );
    let tab = open_changes_tab(
        &tab,
        cwd,
        Some(&format!("{cwd}/a.ts")),
        Some(GitFileDiffKind::Staged),
        None,
    );
    let tab = open_changes_tab(
        &tab,
        cwd,
        Some(&format!("{cwd}/b.ts")),
        Some(GitFileDiffKind::Unstaged),
        None,
    );
    let open = all_files(&tab);
    assert_eq!(open.len(), 2);
    let changes = open.iter().find(|file| file.changes == Some(true)).unwrap();
    assert_eq!(changes.path, format!("{cwd}/b.ts"));
    assert_eq!(changes.change_kind, Some(GitFileDiffKind::Unstaged));
}

#[test]
fn replaces_the_panes_preview_in_place_and_keeps_permanent_tabs() {
    let tab = open_editor_tab(&new_tab("s"), &file("/r/a.ts", "/r"), &pin());
    let tab = open_editor_tab(&tab, &file("/r/b.ts", "/r"), &plain());
    let tab = open_editor_tab(
        &tab,
        &new_file_tab("/r/c.ts", "/r", true, None, None),
        &plain(),
    );
    let tab = open_editor_tab(
        &tab,
        &new_commit_tab("/r", commit("1", "1", "x"), None),
        &plain(),
    );
    assert_eq!(
        paths(&tab),
        vec![
            ("/r/a.ts".to_string(), false),
            ("commit:1".to_string(), true)
        ]
    );
    assert_eq!(
        tab.editor_panes[0].active_file_id,
        tab.editor_panes[0].files[1].id
    );
}

#[test]
fn promotes_an_open_preview_when_reopened_pinned_and_pin_editor_file_does_the_same() {
    let tab = open_editor_tab(&new_tab("s"), &file("/r/a.ts", "/r"), &plain());
    let tab = open_editor_tab(&tab, &file("/r/a.ts", "/r"), &pin());
    let tab = open_editor_tab(&tab, &file("/r/b.ts", "/r"), &plain());
    assert_eq!(
        paths(&tab),
        vec![
            ("/r/a.ts".to_string(), false),
            ("/r/b.ts".to_string(), true)
        ]
    );
    let preview_id = tab.editor_panes[0].files[1].id.clone();
    let tab = pin_editor_file(&tab, &preview_id);
    let tab = open_editor_tab(&tab, &file("/r/c.ts", "/r"), &plain());
    assert_eq!(
        paths(&tab)
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>(),
        vec!["/r/a.ts", "/r/b.ts", "/r/c.ts"]
    );
    assert_eq!(pin_editor_file(&tab, &preview_id), tab);
}

#[test]
fn never_makes_plans_or_changes_reviews_previews() {
    let tab = open_editor_tab(
        &new_tab("s"),
        &new_plan_tab("s", "p", "Plan", "/r"),
        &plain(),
    );
    let tab = open_changes_tab(&tab, "/r", None, None, None);
    let tab = open_editor_tab(&tab, &file("/r/a.ts", "/r"), &plain());
    assert_eq!(
        paths(&tab)
            .into_iter()
            .map(|(_, preview)| preview)
            .collect::<Vec<_>>(),
        vec![false, false, true]
    );
}

#[test]
fn workspace_mode_back_to_back_opens_share_one_preview_per_project() {
    let append = |tabs: &[WorkspaceTab], tab: WorkspaceTab| {
        let mut next = tabs.to_vec();
        next.push(tab);
        next
    };
    let open = |tabs: &[WorkspaceTab], path: &str, cwd: &str, pin: bool| {
        let file = file(path, cwd);
        let created = new_editor_workspace_tab(if pin {
            file.clone()
        } else {
            FilePaneTab {
                preview: Some(true),
                ..file.clone()
            }
        });
        open_workspace_file(tabs, &file, created, append, pin).tabs
    };
    // Each open reads the previous result, as chained state updaters do.
    let tabs = open(&[new_tab("s")], "/r/a.ts", "/r", false);
    let tabs = open(&tabs, "/r/b.ts", "/r", false);
    let tabs = open(&tabs, "/other/c.ts", "/other", false);
    let files = |tabs: &[WorkspaceTab]| {
        tabs[1..]
            .iter()
            .map(|tab| {
                tab.editor_panes[0]
                    .files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(files(&tabs), vec![vec!["/r/b.ts"], vec!["/other/c.ts"]]);

    let tabs = open(&tabs, "/r/d.ts", "/r", true);
    let tabs = open(&tabs, "/r/e.ts", "/r", false);
    assert_eq!(
        files(&tabs),
        vec![vec!["/r/e.ts"], vec!["/other/c.ts"], vec!["/r/d.ts"]]
    );
}

// splitSizesAtBoundary

#[test]
fn split_sizes_at_boundary_moves_only_the_adjacent_panes_and_preserves_their_total() {
    let result = split_sizes_at_boundary(&[0.2, 0.3, 0.5], 1, 0.7);
    assert_eq!(result[0], 0.2);
    assert_close(result[1], 0.5);
    assert_close(result[2], 0.3);
}

#[test]
fn split_sizes_at_boundary_clamps_both_panes_to_the_minimum_size() {
    let right = split_sizes_at_boundary(&[0.5, 0.5], 0, 0.99);
    assert_close(right[0], 0.92);
    assert_close(right[1], 0.08);

    let left = split_sizes_at_boundary(&[0.5, 0.5], 0, 0.01);
    assert_close(left[0], 0.08);
    assert_close(left[1], 0.92);
}

#[test]
fn split_sizes_at_boundary_leaves_invalid_boundaries_unchanged() {
    let sizes = [0.5, 0.5];
    assert_eq!(split_sizes_at_boundary(&sizes, 2, 0.5), sizes.to_vec());
}

#[test]
fn set_split_ratio_moves_the_named_sash() {
    let tree = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let LayoutNode::Split(split) = &tree else {
        panic!("expected a split");
    };
    let moved = set_split_ratio(&tree, &split.id, 0, 0.3);
    let leaves = layout_leaves(&moved);
    assert_close(leaves[0].rect.w, 0.3);
    assert_eq!(set_split_ratio(&tree, &split.id, 1, 0.3), tree);
    assert_eq!(set_split_ratio(&tree, "missing", 0, 0.3), tree);
}

// layoutLeaves

#[test]
fn layout_leaves_keeps_a_single_pane_filling_the_tab() {
    assert_eq!(
        layout_leaves(&leaf("a")),
        vec![LayoutLeaf {
            id: "a".into(),
            rect: rect(0.0, 0.0, 1.0, 1.0),
            axis: Axis::X,
        }]
    );
}

#[test]
fn layout_leaves_places_a_right_split_side_by_side_without_changing_leaf_ids() {
    let tree = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let leaves = layout_leaves(&tree);
    assert_eq!(ids(&tree), vec!["a", "b"]);
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 0.5, 1.0));
    assert_eq!(leaves[1].rect, rect(0.5, 0.0, 0.5, 1.0));
}

// layoutSashes

#[test]
fn layout_sashes_puts_a_sash_on_the_shared_edge_of_a_right_split() {
    let tree = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let sashes = layout_sashes(&tree);
    assert_eq!(sashes.len(), 1);
    assert_eq!(sashes[0].index, 0);
    assert_eq!(sashes[0].dir, SplitDir::Right);
    assert_eq!(sashes[0].group, rect(0.0, 0.0, 1.0, 1.0));
}

// editorTabKey

#[test]
fn editor_tab_key_keeps_a_working_tree_tab_distinct_from_a_normal_file_tab() {
    let cwd = "/repo";
    let path = "/repo/EventStore.swift";
    assert_eq!(editor_tab_key(&file(path, cwd)), format!("file:{path}"));
    assert_eq!(
        editor_tab_key(&new_file_tab(path, cwd, true, None, None)),
        format!("review:{path}")
    );
    assert_eq!(
        editor_tab_key(&new_changes_tab(cwd, Some(path), None, None)),
        format!("changes:{cwd}")
    );
    assert_eq!(
        editor_tab_key(&new_changes_tab(
            cwd,
            Some(&format!("{cwd}/other.ts")),
            None,
            None
        )),
        format!("changes:{cwd}")
    );
    assert_eq!(
        editor_tab_key(&new_session_changes_tab(cwd, "s1", Some(path), None)),
        format!("session-changes:{cwd}:s1")
    );
    assert!(!is_filesystem_tab(&new_session_changes_tab(
        cwd,
        "s1",
        Some(path),
        None
    )));
    assert_eq!(
        editor_tab_key(&new_commit_tab(
            cwd,
            commit("abc1234deadbeef", "abc1234", "Fix the graph"),
            None
        )),
        format!("commit:{cwd}:abc1234deadbeef")
    );
    assert_eq!(
        editor_tab_key(&new_plan_tab("s", "b", "Plan", cwd)),
        "plan:b"
    );
    let terminal = new_terminal_file(cwd, None, None);
    assert_eq!(
        editor_tab_key(&terminal),
        format!("terminal:{}", terminal.id)
    );
    assert!(is_terminal_tab(&terminal));
    assert!(!is_terminal_tab(&file(path, cwd)));
}

// openSessionChangesTab

#[test]
fn open_session_changes_tab_reuses_one_review_per_session_without_merging_different_sessions() {
    let cwd = "/repo";
    let first = open_session_changes_tab(
        &new_tab("session-a"),
        cwd,
        "session-a",
        Some("/repo/a.ts"),
        None,
        true,
    );
    let focused =
        open_session_changes_tab(&first, cwd, "session-a", Some("/repo/b.ts"), None, false);
    let second =
        open_session_changes_tab(&focused, cwd, "session-b", Some("/repo/c.ts"), None, false);
    let reviews: Vec<FilePaneTab> = all_files(&second)
        .into_iter()
        .filter(is_session_changes_tab)
        .collect();
    assert_eq!(reviews.len(), 2);
    assert_eq!(
        reviews
            .iter()
            .find(|file| file.session_changes.as_ref().unwrap().session_id == "session-a")
            .map(|file| file.path.as_str()),
        Some("/repo/b.ts")
    );
}

// openChangesTab

#[test]
fn open_changes_tab_keeps_changes_and_per_file_reviews_independent_across_worktrees() {
    let main = open_changes_tab(&new_tab("session-a"), "/repo", None, None, None);
    let main_review = new_file_tab("/repo/a.ts", "/repo", true, None, None);
    let with_review = open_editor_tab(&main, &main_review, &pin());
    let worktree = open_changes_tab(
        &with_review,
        "/repo-worktrees/feature",
        None,
        None,
        Some("/repo"),
    );
    let files = all_files(&worktree);
    assert_eq!(
        files
            .iter()
            .filter(|file| is_changes_tab(file))
            .map(|file| file.cwd.as_str())
            .collect::<Vec<_>>(),
        vec!["/repo", "/repo-worktrees/feature"]
    );
    assert!(files.contains(&main_review));
    let pane = worktree
        .editor_panes
        .iter()
        .find(|pane| pane.id == worktree.focused_id)
        .unwrap();
    let active = pane
        .files
        .iter()
        .find(|file| file.id == pane.active_file_id)
        .unwrap();
    assert_eq!(active.cwd, "/repo-worktrees/feature");
    assert_eq!(active.project_cwd.as_deref(), Some("/repo"));
    let back = open_changes_tab(&worktree, "/repo", None, None, None);
    assert_eq!(
        all_files(&back)
            .iter()
            .filter(|file| is_changes_tab(file))
            .count(),
        2
    );
    assert_eq!(
        back.editor_panes[0].active_file_id,
        main.editor_panes[0].active_file_id
    );
}

#[test]
fn open_changes_tab_reuses_one_changes_tab_and_updates_the_focused_file() {
    let cwd = "/repo";
    let first = open_changes_tab(
        &new_tab("session-a"),
        cwd,
        Some("/repo/a.ts"),
        Some(GitFileDiffKind::Staged),
        None,
    );
    let second = open_changes_tab(
        &first,
        cwd,
        Some("/repo/b.ts"),
        Some(GitFileDiffKind::Unstaged),
        None,
    );
    let files = &second.editor_panes[0].files;
    assert_eq!(files.iter().filter(|file| is_changes_tab(file)).count(), 1);
    assert_eq!(files.iter().filter(|file| is_review_tab(file)).count(), 1);
    let changes = files.iter().find(|file| is_changes_tab(file)).unwrap();
    assert_eq!(changes.path, "/repo/b.ts");
    assert_eq!(changes.change_kind, Some(GitFileDiffKind::Unstaged));
}

#[test]
fn open_changes_tab_opens_a_changes_tab_without_a_focused_file() {
    let cwd = "/repo";
    let next = open_changes_tab(&new_tab("session-a"), cwd, None, None, None);
    assert_eq!(
        next.editor_panes[0]
            .files
            .iter()
            .find(|file| is_changes_tab(file))
            .map(|file| file.path.as_str()),
        Some(cwd)
    );
}

#[test]
fn open_changes_tab_drops_per_file_review_tabs_in_the_same_pane() {
    let cwd = "/repo";
    let with_review = open_editor_tab(
        &new_tab("session-a"),
        &new_file_tab("/repo/a.ts", cwd, true, None, None),
        &plain(),
    );
    let next = open_changes_tab(&with_review, cwd, Some("/repo/b.ts"), None, None);
    let files = &next.editor_panes[0].files;
    assert!(
        !files
            .iter()
            .any(|file| editor_tab_key(file) == format!("review:{cwd}/a.ts"))
    );
    assert_eq!(files.iter().filter(|file| is_changes_tab(file)).count(), 1);
}

#[test]
fn open_changes_tab_keeps_a_commit_tab_when_opening_changes() {
    let cwd = "/repo";
    let with_commit = open_commit_tab(
        &new_tab("session-a"),
        cwd,
        commit("abc1234deadbeef", "abc1234", "Fix the graph"),
        None,
        false,
    );
    let next = open_changes_tab(&with_commit, cwd, None, None, None);
    let files = &next.editor_panes[0].files;
    assert_eq!(files.iter().filter(|file| is_commit_tab(file)).count(), 1);
    assert_eq!(files.iter().filter(|file| is_changes_tab(file)).count(), 1);
}

// openCommitTab

#[test]
fn open_commit_tab_reuses_one_tab_per_commit() {
    let cwd = "/repo";
    let source = commit("abc1234deadbeef", "abc1234", "Fix the graph");
    let first = open_commit_tab(&new_tab("session-a"), cwd, source.clone(), None, false);
    let second = open_commit_tab(
        &first,
        cwd,
        CommitTabSource {
            subject: "other".into(),
            ..source
        },
        None,
        false,
    );
    let files = &second.editor_panes[0].files;
    let commit_file = files.iter().find(|file| is_commit_tab(file)).unwrap();
    assert_eq!(files.iter().filter(|file| is_commit_tab(file)).count(), 1);
    assert_eq!(
        commit_file.commit.as_ref().unwrap().subject,
        "Fix the graph"
    );
    assert!(!is_filesystem_tab(commit_file));
}

// newReleaseNotesWorkspaceTab

#[test]
fn new_release_notes_workspace_tab_creates_a_projectless_editor_only_workspace_tab() {
    let tab = new_release_notes_workspace_tab(ReleaseNotesTabSource::new("0.1.23"));
    let file = &tab.editor_panes[0].files[0];

    assert!(is_release_notes_tab(file));
    assert_eq!(editor_tab_key(file), "release-notes:0.1.23");
    assert!(!is_review_tab(file));
    assert!(!is_filesystem_tab(file));
    assert!(tab.terminal_panes.is_empty());
    assert_eq!(ids(&tab.layout), vec![tab.editor_panes[0].id.clone()]);
    assert_eq!(tab.focused_id, tab.editor_panes[0].id);
}

#[test]
fn new_release_notes_workspace_tab_deduplicates_release_files_by_version() {
    let first = new_release_notes_workspace_tab(ReleaseNotesTabSource::new("0.1.23"));
    let second = new_release_notes_workspace_tab(ReleaseNotesTabSource::new("0.1.23"));
    assert_eq!(
        editor_tab_key(&first.editor_panes[0].files[0]),
        editor_tab_key(&second.editor_panes[0].files[0])
    );
}

// openTerminalTab

#[test]
fn open_terminal_tab_occupies_a_session_pane_instead_of_splitting_a_leftover_chat() {
    let tab = new_tab("session-a");
    let file = new_terminal_file("/repo", None, None);
    let next = open_terminal_tab(&tab, &file, Some("session-a"));
    assert_eq!(ids(&next.layout), vec![next.terminal_panes[0].id.clone()]);
    assert_eq!(next.focused_id, next.terminal_panes[0].id);
    assert!(next.editor_panes.is_empty());
    assert_eq!(next.terminal_panes[0].files, vec![file]);
}

#[test]
fn open_terminal_tab_splits_a_terminal_pane_below_the_session_when_no_terminal_pane_exists() {
    let tab = new_tab("session-a");
    let file = new_terminal_file("/repo", None, None);
    let next = open_terminal_tab(&tab, &file, None);
    let leaves = layout_leaves(&next.layout);
    assert_eq!(
        ids(&next.layout),
        vec!["session-a".to_string(), next.terminal_panes[0].id.clone()]
    );
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 1.0, 0.5));
    assert_eq!(leaves[1].rect, rect(0.0, 0.5, 1.0, 0.5));
    assert!(next.editor_panes.is_empty());
    assert_eq!(next.terminal_panes[0].files, vec![file]);
}

#[test]
fn open_terminal_tab_keeps_terminals_out_of_the_file_pane_tab_strip() {
    let with_file = open_editor_tab(
        &new_tab("session-a"),
        &file("/repo/App.tsx", "/repo"),
        &plain(),
    );
    let first = new_terminal_file("/repo", None, None);
    let with_terminal = open_terminal_tab(&with_file, &first, None);
    let extra = new_terminal_file(
        "/repo",
        Some(&next_terminal_title(&with_terminal, "/repo")),
        None,
    );
    let next = open_terminal_tab(&with_terminal, &extra, None);
    assert_eq!(next.editor_panes.len(), 1);
    assert_eq!(
        next.editor_panes[0]
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/repo/App.tsx"]
    );
    assert_eq!(next.terminal_panes.len(), 1);
    assert_eq!(
        next.terminal_panes[0]
            .files
            .iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>(),
        vec![first.id.clone(), extra.id.clone()]
    );
    assert_eq!(extra.path, "repo 2");
    assert_eq!(layout_leaves(&next.layout).len(), 3);
}

// closeLeaf

#[test]
fn close_leaf_keeps_a_file_pane_when_the_last_chat_is_closed() {
    let file = file("/repo/App.tsx", "/repo");
    let tab = open_editor_tab(&new_tab("session-a"), &file, &pin());
    let next = close_leaf(&tab, "session-a").unwrap();
    assert_eq!(ids(&next.layout), vec![next.editor_panes[0].id.clone()]);
    assert_eq!(next.focused_id, next.editor_panes[0].id);
    assert_eq!(next.editor_panes[0].files, vec![file]);
}

#[test]
fn close_leaf_keeps_a_terminal_pane_when_the_last_chat_is_closed() {
    let file = new_terminal_file("/repo", None, None);
    let tab = open_terminal_tab(&new_tab("session-a"), &file, None);
    let next = close_leaf(&tab, "session-a").unwrap();
    assert_eq!(ids(&next.layout), vec![next.terminal_panes[0].id.clone()]);
    assert_eq!(next.focused_id, next.terminal_panes[0].id);
    assert_eq!(next.terminal_panes[0].files, vec![file]);
}

#[test]
fn close_leaf_returns_none_when_closing_the_last_remaining_pane() {
    assert_eq!(close_leaf(&new_tab("session-a"), "session-a"), None);
}

// closeSurfacePanes

#[test]
fn close_surface_panes_keeps_the_chat_and_clears_every_editor_pane() {
    let tab = open_editor_tab(
        &open_editor_tab(
            &new_tab("session-a"),
            &file("/repo/a.ts", "/repo"),
            &plain(),
        ),
        &file("/repo/b.ts", "/repo"),
        &OpenEditorTabOptions {
            split: Some(EditorSplitSide::Right),
            ..Default::default()
        },
    );
    let next = close_surface_panes(&tab, SurfaceKind::Editor).unwrap();
    assert_eq!(ids(&next.layout), vec!["session-a"]);
    assert_eq!(next.focused_id, "session-a");
    assert!(next.editor_panes.is_empty());
}

#[test]
fn close_surface_panes_keeps_a_terminal_pane_when_the_editor_panes_go() {
    let tab = open_editor_tab(
        &open_terminal_tab(
            &new_tab("session-a"),
            &new_terminal_file("/repo", None, None),
            None,
        ),
        &file("/repo/a.ts", "/repo"),
        &plain(),
    );
    let next =
        close_surface_panes(&close_leaf(&tab, "session-a").unwrap(), SurfaceKind::Editor).unwrap();
    assert_eq!(ids(&next.layout), vec![next.terminal_panes[0].id.clone()]);
    assert!(next.editor_panes.is_empty());
}

#[test]
fn close_surface_panes_returns_none_when_only_editor_panes_remain() {
    let tab = close_leaf(
        &open_editor_tab(
            &new_tab("session-a"),
            &file("/repo/a.ts", "/repo"),
            &plain(),
        ),
        "session-a",
    )
    .unwrap();
    assert_eq!(close_surface_panes(&tab, SurfaceKind::Editor), None);
}

#[test]
fn close_surface_panes_leaves_a_tab_without_panes_of_that_kind_unchanged() {
    let tab = new_tab("session-a");
    assert_eq!(close_surface_panes(&tab, SurfaceKind::Editor), Some(tab));
}

// resetTabToSession

#[test]
fn reset_tab_to_session_keeps_id_and_group_and_replaces_contents_with_one_session_leaf() {
    let tab = WorkspaceTab {
        group_id: Some("group-1".into()),
        diff_open: Some(true),
        diff_focused: Some(true),
        ..open_editor_tab(
            &new_tab("session-a"),
            &file("/repo/a.ts", "/repo"),
            &plain(),
        )
    };
    let next = reset_tab_to_session(&tab, "session-b");
    assert_eq!(next.id, tab.id);
    assert_eq!(next.group_id.as_deref(), Some("group-1"));
    assert_eq!(next.layout, leaf("session-b"));
    assert_eq!(next.focused_id, "session-b");
    assert!(next.editor_panes.is_empty());
    assert!(next.terminal_panes.is_empty());
    assert_eq!(next.diff_open, Some(false));
    assert_eq!(next.diff_focused, Some(false));
}

// openEditorTab

#[test]
fn open_editor_tab_can_put_the_first_file_before_a_focused_session() {
    let file = file("/repo/App.tsx", "/repo");
    let next = open_editor_tab(
        &new_tab("session-a"),
        &file,
        &OpenEditorTabOptions {
            split: Some(EditorSplitSide::Left),
            ..Default::default()
        },
    );
    let leaves = layout_leaves(&next.layout);

    assert_eq!(
        ids(&next.layout),
        vec![next.editor_panes[0].id.clone(), "session-a".to_string()]
    );
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 0.5, 1.0));
    assert_eq!(leaves[1].rect, rect(0.5, 0.0, 0.5, 1.0));
    assert_eq!(next.focused_id, next.editor_panes[0].id);
}

#[test]
fn open_editor_tab_does_not_open_files_into_a_terminal_pane() {
    let terminal = open_terminal_tab(
        &new_tab("session-a"),
        &new_terminal_file("/repo", None, None),
        None,
    );
    let next = open_editor_tab(&terminal, &file("/repo/App.tsx", "/repo"), &plain());
    assert!(next.terminal_panes[0].files.iter().all(is_terminal_tab));
    assert_eq!(
        next.editor_panes[0]
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/repo/App.tsx"]
    );
    assert_eq!(layout_leaves(&next.layout).len(), 3);
}

// newTerminalWorkspaceTab

#[test]
fn new_terminal_workspace_tab_creates_a_tab_whose_only_leaf_is_the_terminal_pane() {
    let file = new_terminal_file("/repo", None, None);
    let tab = new_terminal_workspace_tab(file.clone());
    assert_eq!(ids(&tab.layout), vec![tab.terminal_panes[0].id.clone()]);
    assert_eq!(tab.focused_id, tab.terminal_panes[0].id);
    assert!(tab.editor_panes.is_empty());
    assert_eq!(tab.terminal_panes[0].files, vec![file]);
}

// newEditorWorkspaceTab

#[test]
fn new_editor_workspace_tab_creates_a_top_level_tab_whose_only_leaf_is_the_file_pane() {
    let file = file("/repo/App.tsx", "/repo");
    let tab = new_editor_workspace_tab(file.clone());
    assert_eq!(ids(&tab.layout), vec![tab.editor_panes[0].id.clone()]);
    assert_eq!(tab.focused_id, tab.editor_panes[0].id);
    assert_eq!(tab.editor_panes[0].files, vec![file]);
    assert!(tab.terminal_panes.is_empty());
}

// updateTerminalTab

#[test]
fn update_terminal_tab_stores_the_foreground_process_on_the_matching_terminal() {
    let file = new_terminal_file("/repo", None, None);
    let tab = open_terminal_tab(&new_tab("session-a"), &file, None);
    let next = update_terminal_tab(
        &tab,
        &file.id,
        &TerminalMetaPatch {
            title: Some("vite".into()),
            foreground: Some(Some("vite".into())),
            ..Default::default()
        },
    );
    let updated = &next.terminal_panes[0].files[0];
    assert_eq!(updated.path, "vite");
    assert_eq!(updated.foreground.as_deref(), Some("vite"));
    assert_eq!(
        update_terminal_tab(
            &next,
            &file.id,
            &TerminalMetaPatch {
                foreground: Some(Some("vite".into())),
                ..Default::default()
            },
        ),
        next
    );
}

// isolateTerminalPanes

#[test]
fn isolate_terminal_panes_splits_mixed_file_and_terminal_tabs_into_separate_panes() {
    let file = file("/repo/App.tsx", "/repo");
    let terminal = new_terminal_file("/repo", None, None);
    let mixed = open_editor_tab(&new_tab("session-a"), &file, &plain());
    let pane = mixed.editor_panes[0].clone();
    let tab = WorkspaceTab {
        editor_panes: vec![EditorPane {
            files: vec![file.clone(), terminal.clone()],
            active_file_id: terminal.id.clone(),
            ..pane
        }],
        ..mixed
    };
    let next = isolate_terminal_panes(&tab);
    assert_eq!(
        next.editor_panes[0]
            .files
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        vec![file.id.clone()]
    );
    assert_eq!(
        next.terminal_panes[0]
            .files
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        vec![terminal.id.clone()]
    );
    assert_eq!(next.focused_id, next.terminal_panes[0].id);
    assert_eq!(layout_leaves(&next.layout).len(), 3);
}

// paneEdgeFromPoint

#[test]
fn pane_edge_from_point_picks_the_nearest_edge_from_the_pane_center() {
    let rect = PaneRect {
        left: 0.0,
        top: 0.0,
        width: 100.0,
        height: 100.0,
    };
    assert_eq!(pane_edge_from_point(20.0, 50.0, rect), PaneEdge::Left);
    assert_eq!(pane_edge_from_point(80.0, 50.0, rect), PaneEdge::Right);
    assert_eq!(pane_edge_from_point(50.0, 20.0, rect), PaneEdge::Top);
    assert_eq!(pane_edge_from_point(50.0, 80.0, rect), PaneEdge::Bottom);
}

// movePane

#[test]
fn move_pane_reorders_siblings_along_a_horizontal_split() {
    let tree = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let next = move_pane(&tree, "a", "b", PaneEdge::Right);
    let leaves = layout_leaves(&next);
    assert_eq!(ids(&next), vec!["b", "a"]);
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 0.5, 1.0));
    assert_eq!(leaves[1].rect, rect(0.5, 0.0, 0.5, 1.0));
}

#[test]
fn move_pane_stacks_a_dragged_pane_under_its_sibling() {
    let tree = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let next = move_pane(&tree, "b", "a", PaneEdge::Bottom);
    let leaves = layout_leaves(&next);
    assert_eq!(ids(&next), vec!["a", "b"]);
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 1.0, 0.5));
    assert_eq!(leaves[1].rect, rect(0.0, 0.5, 1.0, 0.5));
}

#[test]
fn move_pane_nests_a_pane_under_one_column_of_a_row() {
    let row = split_pane(
        &split_pane(&leaf("a"), "a", SplitDir::Right, "b"),
        "b",
        SplitDir::Right,
        "c",
    );
    let next = move_pane(&row, "c", "a", PaneEdge::Bottom);
    let leaves = layout_leaves(&next);
    assert_eq!(ids(&next), vec!["a", "c", "b"]);
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 0.5, 0.5));
    assert_eq!(leaves[1].rect, rect(0.0, 0.5, 0.5, 0.5));
    assert_eq!(leaves[2].rect, rect(0.5, 0.0, 0.5, 1.0));
}

#[test]
fn move_pane_inserts_into_an_existing_vertical_split_on_a_matching_edge() {
    let stacked = split_pane(&leaf("a"), "a", SplitDir::Down, "b");
    let row = split_pane(&stacked, "a", SplitDir::Right, "c");
    let next = move_pane(&row, "c", "a", PaneEdge::Bottom);
    let leaves = layout_leaves(&next);
    assert_eq!(ids(&next), vec!["a", "c", "b"]);
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 1.0, 0.25));
    assert_eq!(leaves[1].rect, rect(0.0, 0.25, 1.0, 0.25));
    assert_eq!(leaves[2].rect, rect(0.0, 0.5, 1.0, 0.5));
}

// placePane

#[test]
fn place_pane_splits_a_lone_pane_toward_the_drop_edge() {
    let right_tree = place_pane(&leaf("a"), "b", "a", PaneEdge::Right);
    let right = layout_leaves(&right_tree);
    assert_eq!(ids(&right_tree), vec!["a", "b"]);
    assert_eq!(right[0].rect, rect(0.0, 0.0, 0.5, 1.0));
    assert_eq!(right[1].rect, rect(0.5, 0.0, 0.5, 1.0));

    let left = place_pane(&leaf("a"), "b", "a", PaneEdge::Left);
    assert_eq!(ids(&left), vec!["b", "a"]);

    let below_tree = place_pane(&leaf("a"), "b", "a", PaneEdge::Bottom);
    let below = layout_leaves(&below_tree);
    assert_eq!(ids(&below_tree), vec!["a", "b"]);
    assert_eq!(below[1].rect, rect(0.0, 0.5, 1.0, 0.5));
}

#[test]
fn place_pane_inserts_into_an_existing_row_on_a_matching_edge() {
    let row = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let next = place_pane(&row, "c", "a", PaneEdge::Right);
    assert_eq!(ids(&next), vec!["a", "c", "b"]);
}

#[test]
fn place_pane_nests_onto_one_column_of_a_row() {
    let row = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let next = place_pane(&row, "c", "a", PaneEdge::Bottom);
    let leaves = layout_leaves(&next);
    assert_eq!(ids(&next), vec!["a", "c", "b"]);
    assert_eq!(leaves[0].rect, rect(0.0, 0.0, 0.5, 0.5));
    assert_eq!(leaves[1].rect, rect(0.0, 0.5, 0.5, 0.5));
    assert_eq!(leaves[2].rect, rect(0.5, 0.0, 0.5, 1.0));
}

#[test]
fn place_pane_moves_an_existing_pane_instead_of_duplicating_it() {
    let row = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let next = place_pane(&row, "a", "b", PaneEdge::Right);
    assert_eq!(ids(&next), vec!["b", "a"]);
}

#[test]
fn place_pane_ignores_a_missing_target_or_a_drop_onto_itself() {
    let tree = leaf("a");
    assert_eq!(place_pane(&tree, "b", "missing", PaneEdge::Right), tree);
    assert_eq!(place_pane(&tree, "a", "a", PaneEdge::Right), tree);
}

// Behavior the TypeScript tests reach only through callers.

#[test]
fn neighbor_leaf_id_prefers_panes_that_share_an_edge() {
    let row = split_pane(&leaf("a"), "a", SplitDir::Right, "b");
    let grid = split_pane(&row, "b", SplitDir::Down, "c");
    assert_eq!(
        neighbor_leaf_id(&grid, "a", FocusDir::Right).as_deref(),
        Some("b")
    );
    assert_eq!(
        neighbor_leaf_id(&grid, "c", FocusDir::Up).as_deref(),
        Some("b")
    );
    assert_eq!(
        neighbor_leaf_id(&grid, "c", FocusDir::Left).as_deref(),
        Some("a")
    );
    assert_eq!(neighbor_leaf_id(&grid, "a", FocusDir::Left), None);
    assert_eq!(neighbor_leaf_id(&grid, "missing", FocusDir::Left), None);
}

#[test]
fn remove_pane_collapses_and_renormalizes() {
    let row = split_pane(
        &split_pane(&leaf("a"), "a", SplitDir::Right, "b"),
        "b",
        SplitDir::Right,
        "c",
    );
    let next = remove_pane(&row, "b").unwrap();
    let LayoutNode::Split(split) = &next else {
        panic!("expected a split");
    };
    assert_eq!(split.sizes, vec![0.5, 0.5]);
    assert_eq!(ids(&next), vec!["a", "c"]);
    assert_eq!(remove_pane(&leaf("a"), "a"), None);
    assert_eq!(sibling_leaf_id(&row, "a").as_deref(), Some("b"));
    assert_eq!(sibling_leaf_id(&row, "b").as_deref(), Some("a"));
}

#[test]
fn layout_round_trips_through_json() {
    let tab = open_terminal_tab(
        &open_editor_tab(&new_tab("s"), &file("/r/a.ts", "/r"), &plain()),
        &new_terminal_file("/r", None, None),
        None,
    );
    let json = serde_json::to_value(&tab).unwrap();
    assert_eq!(json["kind"], "session");
    assert_eq!(json["layout"]["type"], "split");
    assert_eq!(json["layout"]["children"][0]["type"], "leaf");
    assert_eq!(json["editorPanes"][0]["files"][0]["preview"], true);
    assert!(json["editorPanes"][0]["files"][0].get("review").is_none());
    let back: WorkspaceTab = serde_json::from_value(json).unwrap();
    assert_eq!(back, tab);

    let raw = serde_json::json!({
        "type": "split",
        "id": "s1",
        "dir": "down",
        "children": [{ "type": "leaf", "id": "a", "note": 1 }, { "type": "leaf", "id": "b" }],
        "sizes": [0.5, 0.5],
        "future": true
    });
    let node: LayoutNode = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&node).unwrap(), raw);
}
