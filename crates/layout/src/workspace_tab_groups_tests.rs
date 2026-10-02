//! Port of src/features/workspace/model/workspaceTabGroups.test.ts.

use super::*;
use crate::layout::{EditorPane, LayoutNode, new_file_tab, new_tab, new_terminal_file};
use crate::project_return::{ProjectReturnDecision, ProjectReturnMemory, plan_project_return};
use monocode_core::{HarnessId, Session};

fn session(id: &str, cwd: &str) -> Session {
    let mut session = Session::blank(id, HarnessId::Cursor, "", cwd);
    session.title = String::new();
    session.busy = Some(false);
    session
}

fn tab(id: &str, session_id: &str) -> WorkspaceTab {
    WorkspaceTab {
        id: id.into(),
        ..new_tab(session_id)
    }
}

fn no_sessions() -> Vec<Session> {
    Vec::new()
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Editor,
    Terminal,
}

fn surface(kind: Kind, pane: EditorPane, base: WorkspaceTab) -> WorkspaceTab {
    WorkspaceTab {
        editor_panes: if matches!(kind, Kind::Editor) {
            vec![pane.clone()]
        } else {
            vec![]
        },
        terminal_panes: if matches!(kind, Kind::Terminal) {
            vec![pane]
        } else {
            vec![]
        },
        ..base
    }
}

// focusedWorkspaceTabCwd

#[test]
fn keeps_a_worktree_only_tab_under_its_owning_project() {
    for kind in [Kind::Editor, Kind::Terminal] {
        let cwd = "/repo-worktrees/feature";
        let file = match kind {
            Kind::Editor => {
                new_file_tab(&format!("{cwd}/readme.md"), cwd, false, None, Some("/repo"))
            }
            Kind::Terminal => new_terminal_file(cwd, None, Some("/repo")),
        };
        let pane = EditorPane::new("surface", vec![file.clone()], file.id.clone());
        let worktree_tab = surface(kind, pane.clone(), tab("tree-tab", &pane.id));
        assert_eq!(
            workspace_tab_cwd(&worktree_tab, &no_sessions()).as_deref(),
            Some("/repo"),
            "{kind:?}"
        );
        assert_eq!(
            focused_workspace_tab_cwd(&worktree_tab, &no_sessions()).as_deref(),
            Some("/repo")
        );
        let decision = plan_project_return(
            &ProjectReturnMemory::new(),
            std::slice::from_ref(&worktree_tab),
            &[],
            "elsewhere",
            "/repo",
        );
        assert!(matches!(
            decision,
            ProjectReturnDecision::Activate { ref tab_id, .. } if tab_id == "tree-tab"
        ));
    }
}

#[test]
fn uses_the_restored_pane_project_instead_of_the_first_chat_project() {
    for kind in [Kind::Editor, Kind::Terminal] {
        let sessions = [session("chat", "/alpha")];
        let file = match kind {
            Kind::Editor => new_file_tab("/beta/readme.md", "/beta", false, None, None),
            Kind::Terminal => new_terminal_file("/beta", None, None),
        };
        let pane = EditorPane::new("surface", vec![file.clone()], file.id.clone());
        let mixed = surface(
            kind,
            pane.clone(),
            WorkspaceTab {
                layout: split_pane(&new_tab("chat").layout, "chat", SplitDir::Right, &pane.id),
                ..tab("mixed", "chat")
            },
        );
        let memory: ProjectReturnMemory = [("/beta", pane.id.as_str())].into_iter().collect();
        let decision = plan_project_return(
            &memory,
            std::slice::from_ref(&mixed),
            &sessions,
            &mixed.id,
            "/beta",
        );
        assert_eq!(
            decision,
            ProjectReturnDecision::Activate {
                tab_id: mixed.id.clone(),
                pane_id: Some(pane.id.clone()),
            },
            "{kind:?}"
        );
        let ProjectReturnDecision::Activate {
            pane_id: Some(pane_id),
            ..
        } = decision
        else {
            panic!("Expected pane activation");
        };
        let focused_tab = WorkspaceTab {
            focused_id: pane_id,
            ..mixed.clone()
        };
        assert_eq!(
            workspace_tab_cwd(&focused_tab, &sessions).as_deref(),
            Some("/alpha")
        );
        assert_eq!(
            focused_workspace_tab_cwd(&focused_tab, &sessions).as_deref(),
            Some("/beta")
        );
        assert_eq!(mixed.focused_id, "chat");
    }
}

#[test]
fn prefers_the_focused_chat_over_the_first_chat() {
    let mixed = WorkspaceTab {
        layout: split_pane(&new_tab("first").layout, "first", SplitDir::Right, "second"),
        focused_id: "second".into(),
        ..tab("mixed", "first")
    };
    assert_eq!(
        focused_workspace_tab_cwd(
            &mixed,
            &[session("first", "/alpha"), session("second", "/beta")]
        )
        .as_deref(),
        Some("/beta")
    );
}

#[test]
fn retains_the_tab_fallback_when_the_focused_pane_has_no_cwd() {
    let mixed = WorkspaceTab {
        focused_id: "missing".into(),
        ..tab("mixed", "chat")
    };
    assert_eq!(
        focused_workspace_tab_cwd(&mixed, &[session("chat", "/alpha")]).as_deref(),
        Some("/alpha")
    );
    assert_eq!(focused_workspace_tab_cwd(&mixed, &no_sessions()), None);
}

// findOpenSessionTab

#[test]
fn does_not_focus_a_ghost_tab_whose_session_has_been_parked() {
    let ghost = tab("ghost-tab", "parked-session");
    let tabs = [ghost.clone()];
    assert_eq!(
        find_open_session_tab(&tabs, &no_sessions(), "parked-session"),
        None
    );
    assert_eq!(
        find_open_session_tab(
            &tabs,
            &[session("parked-session", "/workspace")],
            "parked-session"
        ),
        Some(&ghost)
    );
}

// switchSessionInTab

#[test]
fn replaces_the_focused_session_while_keeping_the_tab_and_its_other_panes() {
    let split = WorkspaceTab {
        layout: split_pane(
            &new_tab("current").layout,
            "current",
            SplitDir::Right,
            "other",
        ),
        ..tab("current-tab", "current")
    };
    let next = switch_session_in_tab(std::slice::from_ref(&split), &split.id, "current", "target")
        .unwrap();
    assert_eq!(next[0].id, split.id);
    assert_eq!(leaf_ids(&next[0].layout), vec!["target", "other"]);
    assert_eq!(next[0].focused_id, "target");
}

#[test]
fn focuses_a_session_already_in_the_current_tab() {
    let split = WorkspaceTab {
        layout: split_pane(
            &new_tab("current").layout,
            "current",
            SplitDir::Right,
            "target",
        ),
        ..tab("current-tab", "current")
    };
    let next = switch_session_in_tab(std::slice::from_ref(&split), &split.id, "current", "target")
        .unwrap();
    assert_eq!(leaf_ids(&next[0].layout), vec!["current", "target"]);
    assert_eq!(next[0].focused_id, "target");
}

#[test]
fn swaps_sessions_across_tabs_instead_of_mounting_one_twice() {
    let current = tab("current-tab", "current");
    let other = tab("other-tab", "target");
    let next =
        switch_session_in_tab(&[current.clone(), other], &current.id, "current", "target").unwrap();
    assert_eq!(
        next.iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["current-tab", "other-tab"]
    );
    assert_eq!(
        next.iter()
            .map(|entry| leaf_ids(&entry.layout))
            .collect::<Vec<_>>(),
        vec![vec!["target"], vec!["current"]]
    );
    assert_eq!(
        next.iter()
            .map(|entry| entry.focused_id.as_str())
            .collect::<Vec<_>>(),
        vec!["target", "current"]
    );
}

#[test]
fn ignores_a_switch_after_the_focused_session_has_changed() {
    let current = tab("current-tab", "new-focus");
    assert_eq!(
        switch_session_in_tab(
            std::slice::from_ref(&current),
            &current.id,
            "old-focus",
            "target"
        ),
        None
    );
}

// openAddToChatSessionPane

#[test]
fn opens_and_focuses_a_chat_beside_the_focused_file_pane() {
    let file = new_file_tab("/workspace/readme.md", "/workspace", false, None, None);
    let pane = EditorPane::new("editor", vec![file.clone()], file.id.clone());
    let file_only = WorkspaceTab {
        id: "file-tab".into(),
        editor_panes: vec![pane.clone()],
        diff_focused: Some(true),
        ..new_tab(&pane.id)
    };

    let opened = open_add_to_chat_session_pane(&file_only, &no_sessions(), "new-chat").unwrap();

    assert_eq!(
        leaf_ids(&opened.layout),
        vec![pane.id.clone(), "new-chat".to_string()]
    );
    assert_eq!(opened.focused_id, "new-chat");
    assert_eq!(opened.diff_focused, Some(false));
    assert_eq!(opened.editor_panes, vec![pane]);
}

#[test]
fn leaves_add_to_chat_routing_to_an_existing_session_pane() {
    let chat_tab = tab("chat-tab", "existing-chat");
    assert_eq!(
        open_add_to_chat_session_pane(
            &chat_tab,
            &[session("existing-chat", "/workspace")],
            "unused-chat"
        ),
        None
    );
}

#[test]
fn splits_beside_unmapped_leaves_whose_sessions_are_not_mounted() {
    let ghost_tab = tab("ghost-tab", "ghost-leaf");
    let opened = open_add_to_chat_session_pane(&ghost_tab, &no_sessions(), "new-chat").unwrap();

    assert_eq!(leaf_ids(&opened.layout), vec!["ghost-leaf", "new-chat"]);
    assert_eq!(opened.focused_id, "new-chat");
    assert_eq!(opened.diff_focused, Some(false));
}

// workspaceTabProject

#[test]
fn reads_project_from_the_tab_session_cwd() {
    let workspace = tab("t1", "s1");
    let sessions = [session("s1", "/Users/me/agent-terminal")];
    assert_eq!(
        workspace_tab_project(&workspace, &sessions).as_deref(),
        Some("agent-terminal")
    );
}

// findTabForProject

#[test]
fn matches_a_tab_by_project_path_ignoring_trailing_slashes() {
    let tabs = [tab("t1", "s1"), tab("t2", "s2")];
    let sessions = [session("s1", "/tmp/alpha"), session("s2", "/tmp/beta")];
    assert_eq!(
        find_tab_for_project(&tabs, &sessions, "/tmp/beta/").map(|tab| tab.id.as_str()),
        Some("t2")
    );
}

#[test]
fn returns_none_when_no_open_tab_belongs_to_the_project() {
    let tabs = [tab("t1", "s1")];
    let sessions = [session("s1", "/tmp/alpha")];
    assert_eq!(find_tab_for_project(&tabs, &sessions, "/tmp/beta"), None);
}

// filterTabsForProject

#[test]
fn keeps_only_tabs_that_belong_to_the_project() {
    let tabs = [tab("t1", "s1"), tab("t2", "s2"), tab("t3", "s3")];
    let sessions = [
        session("s1", "/tmp/alpha"),
        session("s2", "/tmp/beta"),
        session("s3", "/tmp/beta"),
    ];
    assert_eq!(
        filter_tabs_for_project(&tabs, &sessions, "/tmp/beta")
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["t2", "t3"]
    );
}

// planWorkspaceTabClose

fn close_fixture() -> (Vec<Session>, Vec<WorkspaceTab>) {
    (
        vec![
            session("m1", "/projects/monocode"),
            session("r1", "/projects/ruler"),
            session("m2", "/projects/monocode"),
        ],
        vec![tab("tm1", "m1"), tab("tr1", "r1"), tab("tm2", "m2")],
    )
}

fn close_to(id: &str) -> WorkspaceTabClosePlan {
    WorkspaceTabClosePlan::Close {
        next_active_tab_id: Some(id.into()),
    }
}

#[test]
fn uses_the_global_neighbor_in_workspace_scope() {
    let (sessions, tabs) = close_fixture();
    assert_eq!(
        plan_workspace_tab_close(&tabs, &sessions, "tm2", WorkspaceTabCloseScope::Workspace),
        close_to("tr1")
    );
}

#[test]
fn prefers_the_previous_same_project_tab_in_project_scope() {
    let (sessions, tabs) = close_fixture();
    assert_eq!(
        plan_workspace_tab_close(&tabs, &sessions, "tm2", WorkspaceTabCloseScope::Project),
        close_to("tm1")
    );
}

#[test]
fn uses_the_next_same_project_tab_when_none_exists_to_the_left() {
    let (sessions, tabs) = close_fixture();
    assert_eq!(
        plan_workspace_tab_close(&tabs, &sessions, "tm1", WorkspaceTabCloseScope::Project),
        close_to("tm2")
    );
}

#[test]
fn keeps_the_last_tab_of_a_project_instead_of_jumping_to_another() {
    let (sessions, tabs) = close_fixture();
    assert_eq!(
        plan_workspace_tab_close(
            &tabs[..2],
            &sessions,
            "tm1",
            WorkspaceTabCloseScope::Project
        ),
        WorkspaceTabClosePlan::Keep
    );
}

#[test]
fn still_jumps_across_projects_in_workspace_scope_when_a_project_is_emptied() {
    let (sessions, tabs) = close_fixture();
    assert_eq!(
        plan_workspace_tab_close(
            &tabs[..2],
            &sessions,
            "tm1",
            WorkspaceTabCloseScope::Workspace
        ),
        close_to("tr1")
    );
}

#[test]
fn uses_the_global_neighbor_for_a_projectless_tab() {
    let (mut sessions, tabs) = close_fixture();
    sessions.push(session("blank1", "~"));
    sessions.push(session("blank2", "~"));
    assert_eq!(
        plan_workspace_tab_close(
            &[tab("projectless", "blank1"), tabs[1].clone()],
            &sessions,
            "projectless",
            WorkspaceTabCloseScope::Project
        ),
        close_to("tr1")
    );
    assert_eq!(
        plan_workspace_tab_close(
            &[
                tab("blank1", "blank1"),
                tabs[1].clone(),
                tab("blank2", "blank2")
            ],
            &sessions,
            "blank2",
            WorkspaceTabCloseScope::Project
        ),
        close_to("tr1")
    );
}

#[test]
fn keeps_the_sole_tab_or_an_unknown_tab() {
    let (sessions, tabs) = close_fixture();
    assert_eq!(
        plan_workspace_tab_close(
            &tabs[..1],
            &sessions,
            "tm1",
            WorkspaceTabCloseScope::Workspace
        ),
        WorkspaceTabClosePlan::Keep
    );
    assert_eq!(
        plan_workspace_tab_close(&tabs, &sessions, "missing", WorkspaceTabCloseScope::Project),
        WorkspaceTabClosePlan::Keep
    );
}

// applyPlaceSessionOnPane

fn place_sessions() -> Vec<Session> {
    vec![
        session("m1", "/projects/monocode"),
        session("m2", "/projects/monocode"),
        session("r1", "/projects/ruler"),
    ]
}

fn replacement_from(seed: Option<&Session>) -> Session {
    session(
        "replacement",
        seed.map(|seed| seed.cwd.as_str())
            .unwrap_or("/tmp/fallback"),
    )
}

fn place<'a>(
    tabs: &'a [WorkspaceTab],
    sessions: &'a [Session],
    session_id: &'a str,
    target_id: &'a str,
    edge: PaneEdge,
    replace_target: bool,
    scope: WorkspaceTabCloseScope,
) -> PlaceSessionOnPane<'a, Session> {
    PlaceSessionOnPane {
        tabs,
        sessions,
        session_id,
        target_id,
        edge,
        replace_target,
        scope,
    }
}

#[test]
fn splits_the_target_pane_toward_the_drop_edge() {
    let sessions = place_sessions();
    let tabs = [tab("tm1", "m1")];
    let next = apply_place_session_on_pane(
        place(
            &tabs,
            &sessions,
            "m2",
            "m1",
            PaneEdge::Right,
            false,
            WorkspaceTabCloseScope::Workspace,
        ),
        replacement_from,
    )
    .unwrap();
    assert_eq!(next.active_tab_id, "tm1");
    assert_eq!(next.tabs[0].focused_id, "m2");
    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["m1", "m2"]);
}

#[test]
fn replaces_a_blank_target_instead_of_splitting_it() {
    let mut sessions = place_sessions();
    sessions.push(session("blank", "/projects/monocode"));
    let tabs = [tab("tm1", "blank")];
    let next = apply_place_session_on_pane(
        place(
            &tabs,
            &sessions,
            "m2",
            "blank",
            PaneEdge::Right,
            true,
            WorkspaceTabCloseScope::Workspace,
        ),
        replacement_from,
    )
    .unwrap();
    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["m2"]);
    assert_eq!(
        next.sessions
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["m1", "m2", "r1"]
    );
}

#[test]
fn relocates_a_session_from_another_tab_and_closes_that_tab() {
    let sessions = place_sessions();
    let tabs = [tab("tm1", "m1"), tab("tm2", "m2")];
    let next = apply_place_session_on_pane(
        place(
            &tabs,
            &sessions,
            "m2",
            "m1",
            PaneEdge::Left,
            false,
            WorkspaceTabCloseScope::Workspace,
        ),
        replacement_from,
    )
    .unwrap();
    assert_eq!(
        next.tabs
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tm1"]
    );
    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["m2", "m1"]);
}

#[test]
fn keeps_the_last_tab_of_a_project_and_fills_it_with_a_replacement() {
    let sessions = place_sessions();
    let tabs = [tab("tr1", "r1"), tab("tm1", "m1")];
    let next = apply_place_session_on_pane(
        place(
            &tabs,
            &sessions,
            "m1",
            "r1",
            PaneEdge::Right,
            false,
            WorkspaceTabCloseScope::Project,
        ),
        replacement_from,
    )
    .unwrap();
    assert_eq!(
        next.tabs
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tr1", "tm1"]
    );
    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["r1", "m1"]);
    assert_eq!(next.tabs[1].focused_id, "replacement");
    assert_eq!(
        next.sessions.last().map(|s| s.cwd.as_str()),
        Some("/projects/monocode")
    );
}

// applyPlaceTabOnPane

fn tab_sessions() -> Vec<Session> {
    vec![
        session("target", "/projects/monocode"),
        session("source", "/projects/monocode"),
        session("other", "/projects/monocode"),
    ]
}

#[test]
fn turns_a_separate_tab_into_a_split_beside_the_target_pane() {
    let sessions = tab_sessions();
    let next = apply_place_tab_on_pane(
        &[tab("target-tab", "target"), tab("source-tab", "source")],
        &sessions,
        "source-tab",
        "target",
        PaneEdge::Left,
        false,
    )
    .unwrap();

    assert_eq!(next.active_tab_id, "target-tab");
    assert_eq!(
        next.tabs
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["target-tab"]
    );
    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["source", "target"]);
    assert_eq!(next.tabs[0].focused_id, "source");
}

#[test]
fn keeps_every_pane_and_the_nested_layout_of_the_dragged_tab() {
    let sessions = tab_sessions();
    let file = new_file_tab(
        "/projects/monocode/readme.md",
        "/projects/monocode",
        false,
        None,
        None,
    );
    let pane = EditorPane::new("editor", vec![file.clone()], file.id.clone());
    let source = WorkspaceTab {
        layout: split_pane(
            &new_tab("source").layout,
            "source",
            SplitDir::Down,
            &pane.id,
        ),
        focused_id: pane.id.clone(),
        editor_panes: vec![pane.clone()],
        ..tab("source-tab", "source")
    };
    let next = apply_place_tab_on_pane(
        &[tab("target-tab", "target"), source.clone()],
        &sessions,
        &source.id,
        "target",
        PaneEdge::Right,
        false,
    )
    .unwrap();

    assert_eq!(
        leaf_ids(&next.tabs[0].layout),
        vec!["target", "source", "editor"]
    );
    let LayoutNode::Split(root) = &next.tabs[0].layout else {
        panic!("expected a split");
    };
    assert_eq!(root.dir, SplitDir::Right);
    assert!(root.children[0].is_leaf("target"));
    assert!(matches!(&root.children[1], LayoutNode::Split(inner) if inner.dir == SplitDir::Down));
    assert_eq!(next.tabs[0].editor_panes, vec![pane]);
    assert_eq!(next.focused_id, "editor");
}

#[test]
fn replaces_a_blank_target_without_leaving_its_session_mounted() {
    let mut sessions = tab_sessions();
    sessions.push(session("blank", "/projects/monocode"));
    let next = apply_place_tab_on_pane(
        &[tab("target-tab", "blank"), tab("source-tab", "source")],
        &sessions,
        "source-tab",
        "blank",
        PaneEdge::Bottom,
        true,
    )
    .unwrap();

    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["source"]);
    assert!(!next.sessions.iter().any(|entry| entry.id == "blank"));
}

// applyDetachPaneToTab

#[test]
fn turns_one_chat_pane_into_a_separate_tab_at_the_drop_position() {
    let source = WorkspaceTab {
        layout: split_pane(&new_tab("first").layout, "first", SplitDir::Right, "second"),
        focused_id: "second".into(),
        ..tab("source-tab", "first")
    };
    let target = tab("target-tab", "third");
    let create = || "detached-tab".to_string();
    let next = apply_detach_pane_to_tab(
        &[source, target.clone()],
        "second",
        &target.id,
        PanePlace::Before,
        Some(&create),
    )
    .unwrap();

    assert_eq!(next.active_tab_id, "detached-tab");
    assert_eq!(next.focused_id, "second");
    assert_eq!(
        next.tabs
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["source-tab", "detached-tab", "target-tab"]
    );
    assert_eq!(leaf_ids(&next.tabs[0].layout), vec!["first"]);
    assert_eq!(leaf_ids(&next.tabs[1].layout), vec!["second"]);
}

#[test]
fn moves_the_complete_pane_metadata_into_the_new_tab() {
    for kind in [Kind::Editor, Kind::Terminal] {
        let file = match kind {
            Kind::Editor => new_file_tab(
                "/projects/monocode/readme.md",
                "/projects/monocode",
                false,
                None,
                None,
            ),
            Kind::Terminal => new_terminal_file("/projects/monocode", None, None),
        };
        let pane_id = match kind {
            Kind::Editor => "editor-pane",
            Kind::Terminal => "terminal-pane",
        };
        let pane = EditorPane::new(pane_id, vec![file.clone()], file.id.clone());
        let source = surface(
            kind,
            pane.clone(),
            WorkspaceTab {
                layout: split_pane(&new_tab("chat").layout, "chat", SplitDir::Down, &pane.id),
                ..tab("source-tab", "chat")
            },
        );
        let create = || "detached-tab".to_string();
        let next = apply_detach_pane_to_tab(
            &[source, tab("target-tab", "other")],
            &pane.id,
            "target-tab",
            PanePlace::After,
            Some(&create),
        )
        .unwrap();

        assert_eq!(
            next.tabs
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["source-tab", "target-tab", "detached-tab"]
        );
        assert!(next.tabs[0].editor_panes.is_empty());
        assert!(next.tabs[0].terminal_panes.is_empty());
        let expected_editor = if matches!(kind, Kind::Editor) {
            vec![pane.clone()]
        } else {
            vec![]
        };
        let expected_terminal = if matches!(kind, Kind::Terminal) {
            vec![pane.clone()]
        } else {
            vec![]
        };
        assert_eq!(next.tabs[2].editor_panes, expected_editor, "{kind:?}");
        assert_eq!(next.tabs[2].terminal_panes, expected_terminal, "{kind:?}");
    }
}

#[test]
fn does_not_detach_the_only_pane_in_a_tab() {
    assert_eq!(
        apply_detach_pane_to_tab(
            &[tab("source-tab", "only")],
            "only",
            "source-tab",
            PanePlace::After,
            None
        ),
        None
    );
}

#[test]
fn keeps_the_source_group_when_detached_inside_it() {
    let grouped = |id: &str, session: &str| WorkspaceTab {
        group_id: Some("g".into()),
        ..tab(id, session)
    };
    let source = WorkspaceTab {
        layout: split_pane(&new_tab("a").layout, "a", SplitDir::Right, "b"),
        ..grouped("source-tab", "a")
    };
    let next = apply_detach_pane_to_tab(
        &[source, grouped("next-tab", "c")],
        "b",
        "next-tab",
        PanePlace::Before,
        None,
    )
    .unwrap();
    assert_eq!(next.tabs[1].group_id.as_deref(), Some("g"));
    assert_eq!(next.tabs[1].id.len(), 36);
}

// replaceGroupInTabOrder

#[test]
fn swaps_a_contiguous_slice_of_ids() {
    let all: Vec<String> = ["a", "b", "c", "d"]
        .iter()
        .map(|id| id.to_string())
        .collect();
    let group: Vec<String> = ["d", "c"].iter().map(|id| id.to_string()).collect();
    assert_eq!(
        replace_group_in_tab_order(&all, 1, 2, &group),
        vec!["a", "d", "c", "d"]
    );
    assert!(is_groupable_project(Some("repo")));
    assert!(!is_groupable_project(Some("~")));
    assert!(!is_groupable_project(None));
}
