//! Port of src/features/workspace/model/workspaceSnapshot.test.ts.
//!
//! The Codex continue case stops before `canAutoContinue` and `appendUser`,
//! which live in the transcript reducer and are not ported yet.

use super::*;
use crate::layout::{
    AgentTabSource, new_agent_tab, new_changes_tab, new_commit_tab, new_editor_workspace_tab,
    new_file_tab, new_release_notes_workspace_tab, new_session_changes_tab, new_terminal_file,
    split_pane,
};
use crate::project_terminal::create_project_terminal;
use monocode_core::block::{Block, BlockRole};
use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelPrefs};
use monocode_core::project_providers::ProjectProviders;
use serde_json::json;

/// `INTERRUPT_MESSAGE` from src/features/sessions/model/inFlight.ts.
const INTERRUPT_MESSAGE: &str = "Turn interrupted when MonoCode quit.";

#[derive(Default)]
struct TestEnv {
    catalog: ModelCatalog,
    prefs: ModelPrefs,
    availability: HarnessAvailability,
    projects: ProjectProviders,
}

impl TestEnv {
    fn env(&self) -> ModelEnv<'_> {
        ModelEnv {
            catalog: &self.catalog,
            prefs: &self.prefs,
            availability: &self.availability,
            projects: &self.projects,
        }
    }
}

/// A stand-in for `markTurnInterrupted`, which needs the reducer.
fn mark_interrupted(mut session: Session) -> Session {
    session.busy = Some(false);
    session.blocks.push(Block::new(
        crate::ids::random_uuid(),
        BlockRole::System,
        INTERRUPT_MESSAGE,
    ));
    session
}

fn chat(id: &str, cwd: &str) -> Session {
    let test = TestEnv::default();
    let mut session = new_session(&test.env(), id, HarnessId::Cursor, cwd, None, None, None);
    session.blocks = vec![Block::new("u1", BlockRole::User, "hello")];
    session.provider_session_id = Some("p1".into());
    session
}

fn hydrate(
    snapshot: &WorkspaceSnapshot,
    loaded: &HashMap<String, Session>,
    interrupted: &[&str],
) -> Option<ResumedWorkspace> {
    let test = TestEnv::default();
    let interrupted: Vec<String> = interrupted.iter().map(|id| id.to_string()).collect();
    hydrate_workspace_snapshot(
        snapshot,
        loaded,
        &interrupted,
        &test.env(),
        mark_interrupted,
    )
}

fn hydrate_value(raw: &Value) -> Option<ResumedWorkspace> {
    let test = TestEnv::default();
    hydrate_workspace_snapshot_value(raw, &HashMap::new(), &[], &test.env(), mark_interrupted)
}

fn no_memory() -> ProjectReturnMemory {
    ProjectReturnMemory::new()
}

fn collect(
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    active: &str,
    project: &str,
) -> WorkspaceSnapshot {
    collect_workspace_snapshot(tabs, sessions, active, project, &no_memory(), &[], None)
}

fn with_id(tab: WorkspaceTab, id: &str) -> WorkspaceTab {
    WorkspaceTab {
        id: id.into(),
        ..tab
    }
}

fn memory_entries(memory: &ProjectReturnMemory) -> Vec<(String, String)> {
    memory
        .iter()
        .map(|(project, id)| (project.to_string(), id.to_string()))
        .collect()
}

fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn target(project_path: &str, tab_id: &str) -> ProjectReturnTarget {
    ProjectReturnTarget {
        project_path: project_path.into(),
        tab_id: Some(tab_id.into()),
        pane_id: None,
        extra: Extra::new(),
    }
}

fn remote_file_tab(
    path: &str,
    cwd: &str,
    review: bool,
    kind: Option<GitFileDiffKind>,
    project: &str,
    relative: &str,
) -> FilePaneTab {
    let mut file = new_file_tab(path, cwd, review, kind, Some(project));
    file.extra.insert(
        "remoteFile".into(),
        json!({ "machineId": "machine", "projectId": "project", "relativePath": relative }),
    );
    file
}

fn first_file(snapshot: &WorkspaceSnapshot) -> &FilePaneTab {
    &snapshot.tabs[0].editor_panes[0].files[0]
}

fn editor_tab(session: &str, tab_id: &str, file: &FilePaneTab) -> WorkspaceTab {
    WorkspaceTab {
        editor_panes: vec![EditorPane::new("e1", vec![file.clone()], file.id.clone())],
        ..with_id(new_tab(session), tab_id)
    }
}

// project return snapshots

#[test]
fn migrates_saved_host_file_tabs_to_shared_remote_paths() {
    let project = "remote://env/repo";
    let file = remote_file_tab(
        "/repo/src/index.ts",
        "/repo",
        false,
        None,
        project,
        "src/index.ts",
    );
    let tab = new_editor_workspace_tab(file);
    let snapshot = collect(std::slice::from_ref(&tab), &[], &tab.id, project);
    let restored = parse_workspace_snapshot(&to_value(&snapshot)).unwrap();
    let file = first_file(&restored);
    assert_eq!(file.path, "remote://env/repo/src/index.ts");
    assert_eq!(file.cwd, project);
    assert_eq!(file.extra.get("remoteFile"), None);
    let mut malformed = to_value(&snapshot);
    malformed["tabs"][0]["editorPanes"][0]["files"][0]["remoteFile"] = json!({ "machineId": 42 });
    assert_eq!(parse_workspace_snapshot(&malformed), None);
}

#[test]
fn restores_a_host_diff_tab_with_its_review_state() {
    let project = "remote://env/repo";
    let file = remote_file_tab(
        "/repo/a.ts",
        "/repo",
        true,
        Some(GitFileDiffKind::Staged),
        project,
        "a.ts",
    );
    let tab = new_editor_workspace_tab(file);
    let snapshot = collect(std::slice::from_ref(&tab), &[], &tab.id, project);
    let restored = parse_workspace_snapshot(&to_value(&snapshot)).unwrap();
    let file = first_file(&restored);
    assert_eq!(file.review, Some(true));
    assert_eq!(file.change_kind, Some(GitFileDiffKind::Staged));
    assert_eq!(file.path, "remote://env/repo/a.ts");
}

fn saved() -> WorkspaceSnapshot {
    let sessions = [
        chat("a1", "/alpha"),
        chat("a2", "/alpha"),
        chat("b1", "/beta"),
        chat("b2", "/beta"),
    ];
    let tabs: Vec<WorkspaceTab> = sessions
        .iter()
        .map(|session| with_id(new_tab(&session.id), &format!("tab-{}", session.id)))
        .collect();
    WorkspaceSnapshot {
        project_return_targets: Some(vec![target("/alpha", "a2"), target("/beta", "b2")]),
        ..collect(&tabs, &sessions, "tab-b2", "/beta")
    }
}

#[test]
fn collects_and_round_trips_choices_while_rejecting_stale_references() {
    let sessions = [
        chat("a1", "/alpha"),
        chat("a2", "/alpha"),
        chat("b2", "/beta"),
    ];
    let tabs: Vec<WorkspaceTab> = sessions
        .iter()
        .map(|session| with_id(new_tab(&session.id), &format!("tab-{}", session.id)))
        .collect();
    let memory: ProjectReturnMemory = [("/alpha", "a2"), ("/beta", "b2"), ("/gone", "missing")]
        .into_iter()
        .collect();
    let snapshot =
        collect_workspace_snapshot(&tabs, &sessions, "tab-b2", "/beta", &memory, &[], None);
    let restored = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    assert_eq!(
        memory_entries(restored.project_return_memory.as_ref().unwrap()),
        pairs(&[("/alpha", "a2"), ("/beta", "b2")])
    );
    assert_eq!(memory.len(), 3);
}

#[test]
fn restores_both_project_choices_not_just_the_active_tab() {
    let restored = hydrate(&saved(), &HashMap::new(), &[]).unwrap();
    let memory = restored.project_return_memory.unwrap();
    assert_eq!(memory.get("/alpha"), Some("a2"));
    assert_eq!(memory.get("/beta"), Some("b2"));
}

#[test]
fn loads_old_snapshots_and_seeds_only_the_active_project() {
    let old = WorkspaceSnapshot {
        project_return_targets: None,
        ..saved()
    };
    let restored = hydrate(&old, &HashMap::new(), &[]).unwrap();
    assert_eq!(
        memory_entries(restored.project_return_memory.as_ref().unwrap()),
        pairs(&[("/beta", "b2")])
    );
}

#[test]
fn prunes_invalid_entries_and_uses_the_last_valid_duplicate() {
    let mut raw = to_value(&saved());
    raw["projectReturnTargets"] = json!([
        null,
        42,
        {},
        { "projectPath": "/alpha", "tabId": 12 },
        { "projectPath": "/alpha/", "tabId": "a1" },
        { "projectPath": "/alpha", "tabId": "a2" },
        { "projectPath": "/gone", "tabId": "missing" },
        { "projectPath": "/beta", "tabId": "a1" },
    ]);
    let parsed = parse_workspace_snapshot(&raw).unwrap();
    assert_eq!(
        parsed.project_return_targets,
        Some(vec![target("/alpha", "a2")])
    );
}

#[test]
fn lets_the_restored_active_tab_override_inconsistent_saved_preference() {
    let mut raw = saved();
    raw.project_return_targets.as_mut().unwrap()[1].tab_id = Some("tab-b1".into());
    assert_eq!(
        hydrate(&raw, &HashMap::new(), &[])
            .unwrap()
            .project_return_memory
            .unwrap()
            .get("/beta"),
        Some("b2")
    );
}

#[test]
fn ignores_a_malformed_choice_list() {
    for targets in [json!(null), json!("broken"), json!({})] {
        let mut raw = to_value(&saved());
        raw["projectReturnTargets"] = targets.clone();
        let restored = hydrate_value(&raw).unwrap();
        assert_eq!(restored.tabs.len(), 4, "{targets}");
        assert_eq!(
            memory_entries(restored.project_return_memory.as_ref().unwrap()),
            pairs(&[("/beta", "b2")])
        );
    }
}

#[test]
fn rechecks_project_membership_against_loaded_sessions() {
    let loaded = HashMap::from([("a2".to_string(), chat("a2", "/moved"))]);
    let memory = hydrate(&saved(), &loaded, &[])
        .unwrap()
        .project_return_memory
        .unwrap();
    assert!(!memory.has("/alpha"));
    assert_eq!(memory.get("/beta"), Some("b2"));
}

// collectWorkspaceSnapshot

#[test]
fn stores_tabs_stubs_and_the_focused_tab_not_transcripts() {
    let mut session = chat("s1", "/tmp/a");
    session.worktree_cwd = Some("/tmp/a-worktrees/feature".into());
    session.worktree_removed = Some(true);
    session
        .blocks
        .push(Block::new("a1", BlockRole::Assistant, "hi"));
    let file = new_file_tab("/tmp/a/README.md", "/tmp/a", false, None, None);
    let tab = editor_tab("s1", "t1", &file);
    let snapshot = collect(&[tab], &[session], "t1", "/tmp/a");
    assert_eq!(snapshot.active_tab_id, "t1");
    assert_eq!(first_file(&snapshot).path, "/tmp/a/README.md");
    assert_eq!(snapshot.sessions.len(), 1);
    let stub = &snapshot.sessions[0];
    assert_eq!(stub.id, "s1");
    assert_eq!(stub.cwd, "/tmp/a");
    assert_eq!(stub.provider_session_id.as_deref(), Some("p1"));
    assert_eq!(
        stub.worktree_cwd.as_deref(),
        Some("/tmp/a-worktrees/feature")
    );
    assert_eq!(stub.worktree_removed, Some(true));
    assert!(to_value(stub).get("blocks").is_none());
    assert!(snapshot.project_terminals.is_empty());
}

#[test]
fn drops_agent_tabs_and_the_pane_holding_only_them() {
    let file = new_file_tab("/tmp/a/README.md", "/tmp/a", false, None, None);
    let agent = new_agent_tab(
        "Audit the UI",
        "/tmp/a",
        AgentTabSource::new("worker", "s1", HarnessId::Codex),
    );
    let mixed = new_agent_tab(
        "Audit the engine",
        "/tmp/a",
        AgentTabSource::new("worker-2", "s1", HarnessId::Codex),
    );
    let tab = WorkspaceTab {
        layout: split_pane(&new_tab("s1").layout, "s1", SplitDir::Right, "e1"),
        editor_panes: vec![
            EditorPane::new("e1", vec![agent.clone()], agent.id.clone()),
            EditorPane::new("e2", vec![file.clone(), mixed.clone()], mixed.id.clone()),
        ],
        ..with_id(new_tab("s1"), "t1")
    };
    let snapshot = collect(&[tab], &[], "t1", "/tmp/a");
    let panes = &snapshot.tabs[0].editor_panes;
    // The agent-only pane is gone along with its leaf; the mixed one keeps its
    // file and falls back to it as the active tab.
    assert_eq!(
        panes
            .iter()
            .map(|pane| pane.id.as_str())
            .collect::<Vec<_>>(),
        vec!["e2"]
    );
    assert_eq!(
        panes[0]
            .files
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/tmp/a/README.md"]
    );
    assert_eq!(panes[0].active_file_id, file.id);
    assert_eq!(leaf_ids(&snapshot.tabs[0].layout), vec!["s1"]);
}

#[test]
fn round_trips_a_unified_changes_tab() {
    let file = new_changes_tab(
        "/tmp/a",
        Some("/tmp/a/src/lib.rs"),
        Some(GitFileDiffKind::Staged),
        None,
    );
    let snapshot = collect(&[editor_tab("s1", "t1", &file)], &[], "t1", "/tmp/a");
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    let restored = &workspace.tabs[0].editor_panes[0].files[0];
    assert_eq!(restored.changes, Some(true));
    assert_eq!(restored.review, Some(true));
    assert_eq!(restored.path, "/tmp/a/src/lib.rs");
    assert_eq!(restored.change_kind, Some(GitFileDiffKind::Staged));
}

#[test]
fn round_trips_a_session_scoped_changes_tab() {
    let file = new_session_changes_tab("/tmp/a", "session-a", Some("/tmp/a/src/lib.rs"), None);
    let snapshot = collect(&[editor_tab("s1", "t1", &file)], &[], "t1", "/tmp/a");
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    let restored = &workspace.tabs[0].editor_panes[0].files[0];
    assert_eq!(
        restored.session_changes,
        Some(SessionChangesSource::new("session-a"))
    );
    assert_eq!(restored.review, Some(true));
    assert_eq!(restored.path, "/tmp/a/src/lib.rs");
}

#[test]
fn preserves_a_worktree_editors_execution_directory_and_owning_project() {
    let file = new_file_tab(
        "/repo-worktrees/feature/readme.md",
        "/repo-worktrees/feature",
        false,
        None,
        Some("/repo"),
    );
    let tab = WorkspaceTab {
        editor_panes: vec![EditorPane::new(
            "editor",
            vec![file.clone()],
            file.id.clone(),
        )],
        ..new_tab("editor")
    };
    let snapshot = collect(std::slice::from_ref(&tab), &[], &tab.id, "/repo");
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    let restored = &workspace.tabs[0].editor_panes[0].files[0];
    assert_eq!(restored.cwd, "/repo-worktrees/feature");
    assert_eq!(restored.project_cwd.as_deref(), Some("/repo"));
}

#[test]
fn round_trips_a_commit_review_tab() {
    let file = new_commit_tab(
        "/tmp/a",
        CommitTabSource::new("abc1234deadbeef", "abc1234", "Fix the graph"),
        None,
    );
    let snapshot = collect(&[editor_tab("s1", "t1", &file)], &[], "t1", "/tmp/a");
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    assert_eq!(
        workspace.tabs[0].editor_panes[0].files[0].commit,
        Some(CommitTabSource::new(
            "abc1234deadbeef",
            "abc1234",
            "Fix the graph"
        ))
    );
}

#[test]
fn round_trips_a_release_note_descriptor() {
    let tab = new_release_notes_workspace_tab(ReleaseNotesTabSource::new("0.1.22"));
    let snapshot = collect(std::slice::from_ref(&tab), &[], &tab.id, "~");
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();

    assert_eq!(
        workspace.tabs[0].editor_panes[0].files[0].release_notes,
        Some(ReleaseNotesTabSource::new("0.1.22"))
    );
    assert!(workspace.sessions.is_empty());
}

#[test]
fn stores_the_project_terminal_dock() {
    let term = new_terminal_file("/tmp/a", Some("zsh"), None);
    let dock = create_project_terminal("/tmp/a", term.clone(), None);
    let snapshot = collect_workspace_snapshot(
        &[with_id(new_tab("s1"), "t1")],
        &[],
        "t1",
        "/tmp/a",
        &no_memory(),
        &[dock],
        None,
    );
    assert_eq!(snapshot.project_terminals.len(), 1);
    let stored = &snapshot.project_terminals[0];
    assert_eq!(stored.project_path, "/tmp/a");
    assert_eq!(stored.side, DockSide::Bottom);
    assert!(stored.open);
    assert_eq!(stored.pane.files[0].id, term.id);
}

#[test]
fn stores_the_last_dock_side_for_new_projects() {
    let term = new_terminal_file("/tmp/a", Some("zsh"), None);
    let dock = create_project_terminal("/tmp/a", term, None);
    let snapshot = collect_workspace_snapshot(
        &[with_id(new_tab("s1"), "t1")],
        &[],
        "t1",
        "/tmp/a",
        &no_memory(),
        &[dock],
        Some(DockSide::Right),
    );
    assert_eq!(snapshot.last_dock_side, Some(DockSide::Right));
    assert_eq!(
        collect(&[with_id(new_tab("s1"), "t1")], &[], "t1", "/tmp/a").last_dock_side,
        None
    );
}

// parseWorkspaceSnapshot

#[test]
fn returns_none_for_empty_or_invalid_payloads() {
    assert_eq!(parse_workspace_snapshot(&Value::Null), None);
    assert_eq!(
        parse_workspace_snapshot(&json!({ "tabs": [], "activeTabId": "t1" })),
        None
    );
    assert_eq!(
        parse_workspace_snapshot(&json!({ "tabs": [{}], "activeTabId": "t1" })),
        None
    );
}

#[test]
fn keeps_a_valid_last_dock_side_and_drops_an_invalid_one() {
    let term = new_terminal_file("/tmp/a", Some("zsh"), None);
    let snapshot = collect_workspace_snapshot(
        &[with_id(new_tab("s1"), "t1")],
        &[],
        "t1",
        "/tmp/a",
        &no_memory(),
        &[create_project_terminal("/tmp/a", term, None)],
        Some(DockSide::Right),
    );
    let mut raw = to_value(&snapshot);
    assert_eq!(
        parse_workspace_snapshot(&raw).unwrap().last_dock_side,
        Some(DockSide::Right)
    );
    raw["lastDockSide"] = json!("diagonal");
    assert_eq!(parse_workspace_snapshot(&raw).unwrap().last_dock_side, None);
}

#[test]
fn drops_unknown_fields_and_repairs_a_missing_active_tab() {
    let mut tab = to_value(&with_id(new_tab("s1"), "t1"));
    tab["extra"] = json!(true);
    let parsed = parse_workspace_snapshot(&json!({
        "tabs": [tab],
        "sessions": [{ "id": "s1", "harness": "cursor", "runtimeMode": "supervised", "cwd": "/tmp/a" }],
        "activeTabId": "missing",
        "projectCwd": "/tmp/a",
    }))
    .unwrap();
    assert_eq!(parsed.active_tab_id, "t1");
    assert!(parsed.tabs[0].extra.is_empty());
    assert_eq!(parsed.sessions[0].model, "");
    assert_eq!(parsed.sessions[0].title, "");
}

#[test]
fn rejects_a_tab_whose_release_pane_is_invalid() {
    let descriptors = [
        json!({ "releaseNotes": { "version": "" } }),
        json!({ "releaseNotes": { "version": 123 } }),
        json!({
            "releaseNotes": { "version": "0.1.22" },
            "plan": { "sessionId": "s", "blockId": "b", "title": "Plan" },
        }),
        json!({ "releaseNotes": { "version": "0.1.22" }, "review": true }),
        json!({ "releaseNotes": { "version": "0.1.22" }, "changes": true }),
        json!({ "releaseNotes": { "version": "0.1.22" }, "terminal": true }),
        json!({
            "releaseNotes": { "version": "0.1.22" },
            "commit": { "sha": "abc", "shortSha": "abc", "subject": "x" },
        }),
    ];
    for descriptor in descriptors {
        let valid = to_value(&with_id(new_tab("session-a"), "valid-tab"));
        let invalid_pane_id = "invalid-release-pane";
        let mut release_file =
            json!({ "id": "release-file", "path": "release-notes:0.1.22", "cwd": "~" });
        for (key, value) in descriptor.as_object().unwrap() {
            release_file[key] = value.clone();
        }
        let invalid = json!({
            "kind": "session",
            "id": "invalid-tab",
            "layout": { "type": "leaf", "id": invalid_pane_id },
            "focusedId": invalid_pane_id,
            "editorPanes": [{
                "id": invalid_pane_id,
                "activeFileId": "release-file",
                "files": [release_file],
            }],
            "terminalPanes": [],
        });

        let parsed = parse_workspace_snapshot(&json!({
            "tabs": [valid, invalid],
            "sessions": [],
            "activeTabId": "invalid-tab",
            "projectCwd": "~",
        }))
        .unwrap();
        assert_eq!(
            parsed
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            vec!["valid-tab"],
            "{descriptor}"
        );

        let workspace = hydrate(&parsed, &HashMap::new(), &[]).unwrap();
        assert!(
            !workspace
                .sessions
                .iter()
                .any(|session| session.id == invalid_pane_id)
        );
    }
}

// hydrateWorkspaceSnapshot

#[test]
fn reopens_splits_file_panes_and_stored_transcripts() {
    let left = chat("s1", "/tmp/a");
    let right = chat("s2", "/tmp/a");
    let file = new_file_tab("/tmp/a/src/lib.rs", "/tmp/a", false, None, None);
    let tab = WorkspaceTab {
        layout: LayoutNode::split(
            "split1",
            SplitDir::Right,
            vec![leaf("s1"), leaf("e1")],
            vec![0.5, 0.5],
        ),
        focused_id: "e1".into(),
        ..editor_tab("s1", "t1", &file)
    };
    let snapshot = collect(
        std::slice::from_ref(&tab),
        &[left.clone(), right],
        "t1",
        "/tmp/a",
    );
    let mut stored = left.clone();
    stored
        .blocks
        .push(Block::new("a1", BlockRole::Assistant, "stored"));
    let loaded = HashMap::from([("s1".to_string(), stored.clone())]);
    let workspace = hydrate(&snapshot, &loaded, &[]).unwrap();
    assert_eq!(workspace.tabs.len(), 1);
    assert_eq!(workspace.tabs[0].layout, tab.layout);
    assert_eq!(
        workspace.tabs[0].editor_panes[0].files[0].path,
        "/tmp/a/src/lib.rs"
    );
    let find = |id: &str| {
        workspace
            .sessions
            .iter()
            .find(|session| session.id == id)
            .unwrap()
    };
    assert_eq!(find("s1").blocks, stored.blocks);
    assert!(find("s2").blocks.is_empty());
}

#[test]
fn marks_in_flight_chats_interrupted_and_adds_a_tab_if_they_were_parked() {
    let open = chat("s1", "/tmp/a");
    let mut parked = chat("s2", "/tmp/a");
    parked.busy = Some(true);
    let snapshot = collect(
        &[with_id(new_tab("s1"), "t1")],
        &[open.clone(), parked.clone()],
        "t1",
        "/tmp/a",
    );
    let loaded = HashMap::from([("s1".to_string(), open), ("s2".to_string(), parked)]);
    let workspace = hydrate(&snapshot, &loaded, &["s2"]).unwrap();
    assert_eq!(workspace.tabs.len(), 2);
    let resumed = workspace
        .sessions
        .iter()
        .find(|session| session.id == "s2")
        .unwrap();
    assert_eq!(resumed.busy, Some(false));
    assert!(
        resumed
            .blocks
            .iter()
            .any(|block| block.text == INTERRUPT_MESSAGE)
    );
}

#[test]
fn continues_a_codex_snapshot_with_its_saved_model_and_settings_before_discovery() {
    let mut session = chat("s1", "/tmp/a");
    session.harness = HarnessId::Codex;
    session.model = "codex:gpt-5.6-sol".into();
    session.model_settings = [("reasoningEffort", "high"), ("serviceTier", "priority")]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let tab = new_tab(&session.id);
    let snapshot = collect(
        std::slice::from_ref(&tab),
        std::slice::from_ref(&session),
        &tab.id,
        &session.cwd,
    );
    let restored = hydrate(&snapshot, &HashMap::new(), &["s1"])
        .unwrap()
        .sessions
        .remove(0);
    assert_eq!(restored.model, "codex:gpt-5.6-sol");
    assert_eq!(restored.model_settings, session.model_settings);
    assert_eq!(restored.harness, HarnessId::Codex);
    assert_eq!(restored.provider_session_id.as_deref(), Some("p1"));
    assert_eq!(restored.busy, Some(false));
}

#[test]
fn keeps_terminal_only_tabs() {
    let term = new_terminal_file("/tmp/a", None, None);
    let tab = WorkspaceTab {
        terminal_panes: vec![EditorPane::new("p1", vec![term.clone()], term.id.clone())],
        ..WorkspaceTab::new("t1", leaf("p1"), "p1")
    };
    let snapshot = collect(&[tab], &[], "t1", "/tmp/a");
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    assert_eq!(
        workspace.tabs[0].terminal_panes[0].files[0].terminal,
        Some(true)
    );
}

#[test]
fn restores_a_project_terminal_dock() {
    let term = FilePaneTab {
        foreground: Some("vite".into()),
        ..new_terminal_file("/tmp/a", None, None)
    };
    let dock = ProjectTerminalDock {
        side: DockSide::Left,
        size: 300,
        open: false,
        ..create_project_terminal("/tmp/a", term, None)
    };
    let snapshot = collect_workspace_snapshot(
        &[with_id(new_tab("s1"), "t1")],
        &[],
        "t1",
        "/tmp/a",
        &no_memory(),
        &[dock],
        None,
    );
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    let docks = workspace.project_terminals.unwrap();
    assert_eq!(docks.len(), 1);
    assert_eq!(docks[0].project_path, "/tmp/a");
    assert_eq!(docks[0].side, DockSide::Left);
    assert_eq!(docks[0].size, 300);
    assert!(!docks[0].open);
    assert_eq!(docks[0].pane.files[0].terminal, Some(true));
    assert_eq!(docks[0].pane.files[0].foreground, None);
}

#[test]
fn restores_the_last_dock_side() {
    let term = new_terminal_file("/tmp/a", Some("zsh"), None);
    let snapshot = collect_workspace_snapshot(
        &[with_id(new_tab("s1"), "t1")],
        &[],
        "t1",
        "/tmp/a",
        &no_memory(),
        &[create_project_terminal("/tmp/a", term, None)],
        Some(DockSide::Left),
    );
    let workspace = hydrate(&snapshot, &HashMap::new(), &[]).unwrap();
    assert_eq!(workspace.last_dock_side, Some(DockSide::Left));
}

// Rust-only checks of the serde shape.

#[test]
fn drops_inbox_sessions_and_their_leaves() {
    let raw = json!({
        "tabs": [
            { "kind": "session", "id": "t1", "layout": { "type": "split", "id": "sp", "dir": "right",
              "children": [{ "type": "leaf", "id": "s1" }, { "type": "leaf", "id": "inbox" }], "sizes": [0.5, 0.5] },
              "focusedId": "inbox", "editorPanes": [], "terminalPanes": [] },
            { "kind": "session", "id": "t2", "layout": { "type": "leaf", "id": "inbox" },
              "focusedId": "inbox", "editorPanes": [], "terminalPanes": [] },
        ],
        "sessions": [
            { "id": "s1", "harness": "claude", "runtimeMode": "auto", "cwd": "/repo" },
            { "id": "inbox", "harness": "claude", "runtimeMode": "auto", "cwd": "/repo", "inboxAsk": { "partial": true } },
        ],
        "activeTabId": "t2",
        "projectCwd": " /repo ",
    });
    let parsed = parse_workspace_snapshot(&raw).unwrap();
    assert_eq!(parsed.tabs.len(), 1);
    assert_eq!(leaf_ids(&parsed.tabs[0].layout), vec!["s1"]);
    assert_eq!(parsed.tabs[0].focused_id, "s1");
    assert_eq!(parsed.active_tab_id, "t1");
    assert_eq!(parsed.project_cwd, "/repo");
    assert_eq!(
        parsed
            .sessions
            .iter()
            .map(|stub| stub.id.as_str())
            .collect::<Vec<_>>(),
        vec!["s1"]
    );
}

#[test]
fn serde_round_trip_keeps_unknown_fields() {
    let raw = json!({
        "tabs": [{
            "kind": "session", "id": "t1", "layout": { "type": "leaf", "id": "s1", "zoom": 2 },
            "focusedId": "s1",
            "editorPanes": [{ "id": "e1", "activeFileId": "f1", "pinned": true,
              "files": [{ "id": "f1", "path": "/r/a.ts", "cwd": "/r", "remoteFile": { "machineId": "m" } }] }],
            "terminalPanes": [], "color": "red",
        }],
        "sessions": [{ "id": "s1", "cwd": "/r", "harness": "codex", "model": "m", "modelSettings": { "a": "b" },
          "runtimeMode": "full-access", "title": "t", "future": 1 }],
        "activeTabId": "t1",
        "projectCwd": "/r",
        "projectTerminals": [{ "projectPath": "/r", "side": "bottom", "size": 220, "open": true,
          "pane": { "id": "p", "activeFileId": "x", "files": [{ "id": "x", "path": "zsh", "cwd": "/r", "terminal": true }] } }],
        "projectReturnTargets": [{ "projectPath": "/r", "tabId": "s1" }],
        "windowId": 7,
    });
    let snapshot: WorkspaceSnapshot = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), raw);
    assert_eq!(
        unknown_fields(&snapshot),
        vec![
            "file.remoteFile",
            "leaf.zoom",
            "pane.pinned",
            "session.future",
            "snapshot.windowId",
            "tab.color"
        ]
    );
    let key = workspace_snapshot_key(&snapshot);
    assert_eq!(serde_json::from_str::<Value>(&key).unwrap(), raw);
    let moved = WorkspaceSnapshot {
        active_tab_id: "t2".into(),
        ..snapshot
    };
    assert_ne!(workspace_snapshot_key(&moved), key);
}

// worktree tab cleanup

#[test]
fn drops_tabs_the_caller_leaves_out_with_sessions_only_they_showed() {
    let main = chat("main", "/repo");
    let mut feature = chat("feature", "/repo");
    feature.worktree_cwd = Some("/trees/a".into());
    let main_tab = with_id(new_tab("main"), "tab-main");
    let feature_tab = with_id(new_tab("feature"), "tab-feature");
    let snapshot = collect_workspace_snapshot_keeping(
        &[main_tab, feature_tab],
        &[main, feature],
        "tab-feature",
        "/repo",
        &no_memory(),
        &[],
        None,
        &|tab| tab.id != "tab-feature",
    );
    assert_eq!(
        snapshot
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        ["tab-main"]
    );
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .map(|stub| stub.id.as_str())
            .collect::<Vec<_>>(),
        ["main"]
    );
}
