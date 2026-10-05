//! Port of src/features/projects/model/projectReturn.test.ts against
//! `monocode_layout::project_return`, which ports projectReturn.ts.
//!
//! The session removal case needs `removeSessionFromWorkspace` from the
//! workspace package, which this package cannot build against; the
//! workspace package's lifecycle tests cover it.

use monocode_core::block::{Block, BlockRole};
use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelEnv, ModelPrefs};
use monocode_core::project_providers::ProjectProviders;
use monocode_core::session::new_session;
use monocode_core::{HarnessId, Session};
use monocode_layout::layout::{
    EditorPane, PaneEdge, WorkspaceTab, new_file_tab, new_tab, place_pane,
};
use monocode_layout::project_return::{
    ProjectReturnDecision, ProjectReturnMemory, plan_project_return, reconcile_project_return,
};
use monocode_layout::tab_visit_history::{
    empty_tab_visit_history, record_tab_visit, tab_visit_back,
};
use monocode_layout::workspace_tab_groups::{
    PlaceSessionOnPane, WorkspaceTabCloseScope, apply_place_session_on_pane,
};

fn chat(id: &str, cwd: &str) -> Session {
    let catalog = ModelCatalog::new();
    let prefs = ModelPrefs::default();
    let availability = HarnessAvailability::default();
    let projects = ProjectProviders::default();
    let env = ModelEnv {
        catalog: &catalog,
        prefs: &prefs,
        availability: &availability,
        projects: &projects,
    };
    let mut session = new_session(&env, id, HarnessId::Cursor, cwd, None, None, None);
    session.blocks = vec![Block::new(format!("{id}-user"), BlockRole::User, id)];
    session
}

struct State {
    sessions: Vec<Session>,
    tabs: Vec<WorkspaceTab>,
    active_tab_id: String,
    memory: ProjectReturnMemory,
}

fn workspace() -> State {
    let sessions = vec![
        chat("a1", "/alpha"),
        chat("a2", "/alpha"),
        chat("b1", "/beta"),
        chat("b2", "/beta"),
    ];
    let tabs = sessions
        .iter()
        .map(|session| WorkspaceTab {
            id: format!("tab-{}", session.id),
            ..new_tab(&session.id)
        })
        .collect();
    State {
        sessions,
        tabs,
        active_tab_id: "tab-b2".into(),
        memory: ProjectReturnMemory::new(),
    }
}

fn memory(entries: &[(&str, &str)]) -> ProjectReturnMemory {
    entries.iter().map(|(key, id)| (*key, *id)).collect()
}

fn entries(memory: &ProjectReturnMemory) -> Vec<(String, String)> {
    memory
        .iter()
        .map(|(key, id)| (key.to_string(), id.to_string()))
        .collect()
}

fn reconcile(state: &State, memory: &ProjectReturnMemory, active: &str) -> ProjectReturnMemory {
    reconcile_project_return(memory, &state.tabs, &state.sessions, active)
}

fn plan(
    state: &State,
    memory: &ProjectReturnMemory,
    active: &str,
    path: &str,
) -> ProjectReturnDecision {
    plan_project_return(memory, &state.tabs, &state.sessions, active, path)
}

fn activate(tab_id: &str, pane_id: &str) -> ProjectReturnDecision {
    ProjectReturnDecision::Activate {
        tab_id: tab_id.into(),
        pane_id: Some(pane_id.into()),
    }
}

fn editor_tab() -> WorkspaceTab {
    let file = new_file_tab("/gamma/readme.md", "/gamma", false, None, None);
    let file_id = file.id.clone();
    WorkspaceTab {
        id: "files".into(),
        editor_panes: vec![EditorPane::new("editor", vec![file], file_id)],
        ..new_tab("editor")
    }
}

// project return memory

#[test]
fn records_visits_independently_and_changes_on_explicit_selection_or_back() {
    let state = workspace();
    let alpha = reconcile(&state, &state.memory, "tab-a2");
    let beta = reconcile(&state, &alpha, &state.active_tab_id);
    assert_eq!(
        entries(&beta),
        [
            ("/alpha".to_string(), "a2".to_string()),
            ("/beta".to_string(), "b2".to_string())
        ]
    );
    let changed = reconcile(&state, &beta, "tab-a1");
    assert_eq!(changed.get("/alpha"), Some("a1"));
    let history = record_tab_visit(&empty_tab_visit_history("tab-a2"), "tab-b2");
    let back = tab_visit_back(&history).expect("a visit to go back to");
    let returned = reconcile(&state, &changed, &back.current);
    assert_eq!(returned.get("/alpha"), Some("a2"));
    assert_eq!(returned.get("/beta"), Some("b2"));
    assert!(state.memory.is_empty());
    assert_eq!(alpha.len(), 1);
}

#[test]
fn does_not_change_for_repeated_observation_or_streaming_updates() {
    let mut state = workspace();
    let memory = reconcile(&state, &state.memory, &state.active_tab_id);
    state.sessions[0]
        .blocks
        .push(Block::new("response", BlockRole::Assistant, "working"));
    state.sessions[0].busy = Some(true);
    assert_eq!(reconcile(&state, &memory, &state.active_tab_id), memory);
}

#[test]
fn prunes_removed_and_retargeted_tabs_without_changing_the_input_map() {
    let mut state = workspace();
    let saved = memory(&[("/alpha", "a2"), ("/gone", "missing"), ("/beta", "b2")]);
    state.sessions[1].cwd = "/gamma".into();
    assert_eq!(
        entries(&reconcile(&state, &saved, &state.active_tab_id)),
        [("/beta".to_string(), "b2".to_string())]
    );
    assert_eq!(saved.len(), 3);
}

#[test]
fn records_editor_only_tabs_and_keeps_the_selected_file_pane() {
    let mut state = workspace();
    let tab = editor_tab();
    state.tabs.push(tab.clone());
    let saved = reconcile(&state, &state.memory, &tab.id);
    assert_eq!(
        plan(&state, &saved, &state.active_tab_id, "/gamma"),
        activate(&tab.id, "editor")
    );
    assert_eq!(tab.focused_id, "editor");
}

#[test]
fn records_canonical_windows_keys_and_does_not_combine_matching_display_names() {
    let mut state = workspace();
    state.sessions[1].cwd = "C:/Work/Alpha/".into();
    let saved = reconcile(&state, &state.memory, "tab-a2");
    assert_eq!(saved.get("c:/work/alpha"), Some("a2"));
    assert_eq!(
        plan(&state, &saved, &state.active_tab_id, "c:\\work\\ALPHA"),
        activate("tab-a2", "a2")
    );
    assert_eq!(
        plan(&state, &saved, &state.active_tab_id, "/elsewhere/Alpha"),
        ProjectReturnDecision::Create
    );
}

#[test]
fn returns_to_the_focused_session_after_actual_pane_placement() {
    let state = workspace();
    let placed = apply_place_session_on_pane(
        PlaceSessionOnPane {
            tabs: &state.tabs,
            sessions: &state.sessions,
            session_id: "a2",
            target_id: "a1",
            edge: PaneEdge::Right,
            replace_target: false,
            scope: WorkspaceTabCloseScope::Workspace,
        },
        |seed| {
            chat(
                "replacement",
                seed.map(|seed| seed.cwd.as_str()).unwrap_or("/alpha"),
            )
        },
    )
    .expect("placed workspace");
    let placed_state = State {
        sessions: placed.sessions.clone(),
        tabs: placed.tabs.clone(),
        active_tab_id: placed.active_tab_id.clone(),
        memory: ProjectReturnMemory::new(),
    };
    let saved = reconcile(
        &placed_state,
        &memory(&[("/alpha", "a2")]),
        &placed.active_tab_id,
    );
    let beta = reconcile(&placed_state, &saved, "tab-b2");
    assert_eq!(
        plan(&placed_state, &beta, "tab-b2", "/alpha"),
        activate("tab-a1", "a2")
    );
    assert_eq!(
        placed
            .tabs
            .iter()
            .find(|tab| tab.id == "tab-a1")
            .map(|tab| tab.focused_id.as_str()),
        Some("a2")
    );
}

#[test]
fn drops_a_removed_projects_choice_rather_than_reopening_it() {
    let mut state = workspace();
    let saved = memory(&[("/alpha", "a2")]);
    state.tabs.retain(|tab| tab.id.starts_with("tab-b"));
    let next = reconcile(&state, &saved, &state.active_tab_id);
    assert!(!next.has("/alpha"));
    assert_eq!(
        plan(&state, &next, &state.active_tab_id, "/alpha"),
        ProjectReturnDecision::Create
    );
}

#[test]
fn does_not_record_a_missing_active_tab_or_a_projectless_tab() {
    let mut state = workspace();
    assert_eq!(reconcile(&state, &state.memory, "missing"), state.memory);
    state.sessions[3].cwd = "~".into();
    assert_eq!(
        reconcile(&state, &state.memory, &state.active_tab_id),
        state.memory
    );
}

// project selection

#[test]
fn rejects_invalid_alpha_targets() {
    for target in ["missing", "tab-b1"] {
        let mut state = workspace();
        state.memory.set("/alpha", target);
        assert_eq!(
            plan(&state, &state.memory, &state.active_tab_id, "/alpha"),
            activate("tab-a1", "a1")
        );
    }
}

#[test]
fn keeps_the_focused_project() {
    let state = workspace();
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/beta/"),
        ProjectReturnDecision::Keep
    );
}

#[test]
fn selects_the_first_destination_tab_without_a_remembered_choice() {
    let state = workspace();
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/alpha"),
        activate("tab-a1", "a1")
    );
}

#[test]
fn prefers_an_existing_destination_over_a_blank_source() {
    let mut state = workspace();
    state.sessions[3].blocks.clear();
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/alpha"),
        activate("tab-a1", "a1")
    );
}

#[test]
fn returns_to_a2_and_b2_independently_of_tab_order() {
    let mut state = workspace();
    state.memory = memory(&[("/alpha", "a2"), ("/beta", "b2")]);
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/alpha"),
        activate("tab-a2", "a2")
    );
    assert_eq!(
        plan(&state, &state.memory, "tab-a2", "/beta"),
        activate("tab-b2", "b2")
    );
    state.tabs.reverse();
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/alpha"),
        activate("tab-a2", "a2")
    );
}

#[test]
fn reuses_a_blank_only_when_the_destination_has_no_open_tab() {
    let mut state = workspace();
    state.sessions[3].blocks.clear();
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/gamma"),
        ProjectReturnDecision::ReuseBlank {
            session_id: "b2".into()
        }
    );
}

#[test]
fn never_reuses_a_busy_session_even_when_it_has_no_user_blocks() {
    let mut state = workspace();
    state.sessions[3].blocks.clear();
    state.sessions[3].busy = Some(true);
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/gamma"),
        ProjectReturnDecision::Create
    );
}

#[test]
fn creates_a_session_when_no_destination_exists_and_the_current_chat_is_not_blank() {
    let state = workspace();
    assert_eq!(
        plan(&state, &state.memory, &state.active_tab_id, "/gamma"),
        ProjectReturnDecision::Create
    );
}

#[test]
fn keeps_a_focused_beta_session_in_an_alpha_first_split() {
    let mut state = workspace();
    let first = state.tabs[0].clone();
    state.tabs[0] = WorkspaceTab {
        layout: place_pane(&first.layout, "b2", "a1", PaneEdge::Right),
        focused_id: "b2".into(),
        ..first.clone()
    };
    state.tabs.retain(|tab| tab.id != "tab-b2");
    assert_eq!(
        plan(&state, &state.memory, &first.id, "/beta"),
        ProjectReturnDecision::Keep
    );
}

#[test]
fn keeps_a_focused_file_in_the_selected_project() {
    let mut state = workspace();
    let tab = editor_tab();
    state.tabs.push(tab.clone());
    assert_eq!(
        plan(&state, &state.memory, &tab.id, "/gamma"),
        ProjectReturnDecision::Keep
    );
}
