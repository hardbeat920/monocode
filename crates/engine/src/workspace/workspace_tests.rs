//! Tests for the `Workspace` entity and its hooks, against the runtime's
//! fake store, a fake file system, and fake PTYs. Also the cases of
//! src/features/sessions/model/agentTabs.test.ts that cover tabs.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use gpui::{App, AppContext, Entity, Task, TestAppContext};
use monocode_core::block::{Block, BlockRole};
use monocode_core::{HarnessId, Session};
use monocode_layout::terminal_tab::TerminalMetaPatch;
use monocode_layout::{
    AgentTabSource, OpenEditorTabOptions, PaneEdge, SplitDir, WorkspaceTab, editor_tab_key,
    is_agent_tab, is_filesystem_tab, leaf_ids, new_agent_tab, new_file_tab, new_tab,
    open_editor_tab,
};
use serde_json::Value;

use super::chat_context::ChatContextItem;
use super::delegate::WorkspaceDelegate;
use super::files::backend::ProjectFile;
use super::files::backend::fake::FakeFs;
use super::hooks::{WorkspaceSetup, init};
use super::session_factory::ModelEnvSessions;
use super::terminals::pty::fake::{Call, FakePty, init_fake_terminals};
use super::terminals::{TerminalMetaChanged, Terminals};
use super::workspace::{FileOpenOptions, Workspace, WorkspaceConfig, WorkspaceEvent};
use crate::runtime::testing::{FakeBackend, init_test_engine};
use crate::runtime::{Engine, ResumedWorkspace};

const PROJECT: &str = "/Users/me/repo";

#[derive(Default)]
struct TestDelegate {
    confirms: RefCell<Vec<String>>,
    answer: Cell<bool>,
    history: RefCell<Vec<String>>,
    remote_cwds: RefCell<HashMap<String, String>>,
}

impl WorkspaceDelegate for TestDelegate {
    fn confirm(&self, message: &str, _ok_label: &str, _cx: &mut App) -> Task<bool> {
        self.confirms.borrow_mut().push(message.to_string());
        Task::ready(self.answer.get())
    }

    fn refresh_history(&self, cwd: &str, _cx: &mut App) {
        self.history.borrow_mut().push(cwd.to_string());
    }

    fn remote_working_cwd(&self, project: &str, shell_id: &str, _cx: &App) -> Option<String> {
        monocode_layout::paths::is_remote_project_path(project)
            .then(|| self.remote_cwds.borrow().get(shell_id).cloned())
            .flatten()
    }
}

struct Harness {
    store: Arc<FakeBackend>,
    fs: Arc<FakeFs>,
    pty: Arc<FakePty>,
    delegate: Rc<TestDelegate>,
    workspace: Entity<Workspace>,
}

fn setup(cx: &mut TestAppContext) -> Harness {
    let store = init_test_engine(cx);
    let fs = FakeFs::new();
    fs.set_files(vec![
        ProjectFile::new("lib.rs", format!("{PROJECT}/src/lib.rs"), "src/lib.rs"),
        ProjectFile::new("README.md", format!("{PROJECT}/README.md"), "README.md"),
    ]);
    let pty = cx.update(init_fake_terminals);
    let delegate = Rc::new(TestDelegate::default());
    delegate.answer.set(true);
    let setup = WorkspaceSetup {
        fs: fs.clone(),
        sessions: Rc::new(ModelEnvSessions::default()),
        delegate: delegate.clone(),
        terminals: false,
    };
    cx.update(|cx| init(setup, cx));
    let workspace = cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some(PROJECT)), cx));
    cx.run_until_parked();
    Harness {
        store,
        fs,
        pty,
        delegate,
        workspace,
    }
}

fn sessions(cx: &mut TestAppContext) -> Vec<Session> {
    cx.update(|cx| Engine::sessions(cx).read(cx).all().to_vec())
}

fn tabs(h: &Harness, cx: &mut TestAppContext) -> Vec<WorkspaceTab> {
    h.workspace
        .read_with(cx, |workspace, _| workspace.tabs().to_vec())
}

fn active(h: &Harness, cx: &mut TestAppContext) -> WorkspaceTab {
    h.workspace
        .read_with(cx, |workspace, _| workspace.active_tab().cloned())
        .unwrap()
}

fn settle<T: 'static>(task: Task<T>, cx: &mut TestAppContext) -> T {
    cx.run_until_parked();
    task.now_or_never().expect("task finished")
}

fn with_user_turn(session_id: &str, cx: &mut TestAppContext) {
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                session
                    .blocks
                    .push(Block::new("u1", BlockRole::User, "hello"));
            });
        })
    });
}

#[gpui::test]
fn starts_with_one_tab_holding_a_new_chat(cx: &mut TestAppContext) {
    let h = setup(cx);
    let open = sessions(cx);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].cwd, PROJECT);
    let tab = active(&h, cx);
    assert_eq!(leaf_ids(&tab.layout), vec![open[0].id.clone()]);
    let ids = cx.update(|cx| Engine::hooks(cx).workspace.tab_session_ids(cx));
    assert_eq!(ids, vec![open[0].id.clone()]);
}

#[gpui::test]
fn git_cwd_uses_the_remote_host_checkout_and_preserves_focused_files(cx: &mut TestAppContext) {
    let h = setup(cx);
    let session_id = sessions(cx)[0].id.clone();
    let project = "remote://host/repo";
    let checkout = "remote://host/worktrees/review";
    h.delegate
        .remote_cwds
        .borrow_mut()
        .insert(session_id.clone(), checkout.into());
    // A delegate result for another project must not change a local checkout.
    assert_eq!(
        h.workspace
            .read_with(cx, |workspace, cx| workspace.git_cwd(cx)),
        PROJECT,
    );
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(&session_id, cx, |session| session.cwd = project.into());
        });
    });
    assert_eq!(
        h.workspace
            .read_with(cx, |workspace, cx| workspace.git_cwd(cx)),
        checkout,
    );
    h.delegate.remote_cwds.borrow_mut().clear();
    assert_eq!(
        h.workspace
            .read_with(cx, |workspace, cx| workspace.git_cwd(cx)),
        project,
    );
    h.delegate
        .remote_cwds
        .borrow_mut()
        .insert(session_id.clone(), checkout.into());
    let file_cwd = "remote://host/worktrees/other";
    let file = new_file_tab(
        &format!("{file_cwd}/README.md"),
        file_cwd,
        false,
        None,
        None,
    );
    let tab = WorkspaceTab {
        layout: monocode_layout::leaf("file-pane"),
        focused_id: "file-pane".into(),
        editor_panes: vec![monocode_layout::EditorPane::new(
            "file-pane",
            vec![file.clone()],
            file.id.clone(),
        )],
        ..active(&h, cx)
    };
    let config = WorkspaceConfig::transferred(
        vec![tab.clone()],
        tab.id.clone(),
        project.into(),
        Vec::new(),
        Vec::new(),
    );
    let workspace = cx.new(|cx| Workspace::new(config, cx));
    assert_eq!(
        workspace.read_with(cx, |workspace, cx| workspace.git_cwd(cx)),
        file_cwd,
    );
}

#[gpui::test]
fn saves_the_snapshot_after_the_debounce(cx: &mut TestAppContext) {
    let h = setup(cx);
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    let saved = h.store.workspace_snapshot().expect("snapshot saved");
    let ids: Vec<&str> = saved["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| tab["id"].as_str().unwrap())
        .collect();
    let expected: Vec<String> = tabs(&h, cx).into_iter().map(|tab| tab.id).collect();
    assert_eq!(ids, expected);
    assert_eq!(saved["projectCwd"], PROJECT);
}

#[gpui::test]
fn splits_and_closes_panes(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first = sessions(cx)[0].id.clone();
    h.workspace
        .update(cx, |workspace, cx| workspace.split(SplitDir::Right, cx));
    let tab = active(&h, cx);
    let ids = leaf_ids(&tab.layout);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], first);
    assert_eq!(tab.focused_id, ids[1]);
    assert_eq!(sessions(cx).len(), 2);
    assert!(
        h.workspace
            .read_with(cx, |workspace, _| workspace.composer_focused())
    );

    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_pane(None, cx));
    settle(task, cx);
    let tab = active(&h, cx);
    assert_eq!(leaf_ids(&tab.layout), vec![first.clone()]);
    assert_eq!(tab.focused_id, first);
    assert_eq!(
        h.delegate.history.borrow().as_slice(),
        [PROJECT.to_string()]
    );
}

#[gpui::test]
fn navigates_tabs_and_the_visit_history(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first = active(&h, cx).id;
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    let second = active(&h, cx).id;
    assert_ne!(first, second);
    assert_eq!(tabs(&h, cx).len(), 2);
    assert_eq!(
        h.workspace
            .read_with(cx, |workspace, _| workspace.tab_visit_nav()),
        (true, false)
    );

    h.workspace
        .update(cx, |workspace, cx| workspace.next_tab(cx));
    assert_eq!(active(&h, cx).id, first);
    h.workspace
        .update(cx, |workspace, cx| workspace.prev_tab(cx));
    assert_eq!(active(&h, cx).id, second);
    h.workspace
        .update(cx, |workspace, cx| workspace.activate_slot(0, cx));
    assert_eq!(active(&h, cx).id, first);
    h.workspace
        .update(cx, |workspace, cx| workspace.visit_back(cx));
    assert_eq!(active(&h, cx).id, second);
    h.workspace
        .update(cx, |workspace, cx| workspace.visit_forward(cx));
    assert_eq!(active(&h, cx).id, first);
}

#[gpui::test]
fn closes_a_tab_and_activates_its_neighbor(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first = active(&h, cx).id;
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    let second = active(&h, cx).id;
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_tab(&second, &[], cx));
    settle(task, cx);
    assert_eq!(tabs(&h, cx).len(), 1);
    assert_eq!(active(&h, cx).id, first);

    // The project's last tab stays: closing it from the title bar clears it.
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_tab(&first, &[], cx));
    settle(task, cx);
    assert_eq!(tabs(&h, cx).len(), 1);
}

#[gpui::test]
fn asks_before_closing_a_tab_with_unsaved_files(cx: &mut TestAppContext) {
    let h = setup(cx);
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    let task = h.workspace.update(cx, |workspace, cx| {
        workspace.open_file(
            &format!("{PROJECT}/src/lib.rs"),
            None,
            FileOpenOptions::default(),
            cx,
        )
    });
    settle(task, cx);
    let tab = active(&h, cx);
    let file = tab.editor_panes[0].files[0].clone();
    h.workspace.update(cx, |workspace, cx| {
        workspace.file_dirty_change(&file.id, true, cx)
    });
    // An edited preview is pinned.
    assert_eq!(active(&h, cx).editor_panes[0].files[0].preview, None);

    h.delegate.answer.set(false);
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_tab(&tab.id, &[], cx));
    settle(task, cx);
    assert_eq!(
        h.delegate.confirms.borrow().as_slice(),
        ["Close this tab with unsaved files?".to_string()]
    );
    assert_eq!(tabs(&h, cx).len(), 2);

    h.delegate.answer.set(true);
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_tab(&tab.id, &[], cx));
    settle(task, cx);
    assert_eq!(tabs(&h, cx).len(), 1);
    assert!(
        h.workspace
            .read_with(cx, |workspace, _| workspace.dirty_files().is_empty())
    );
}

#[gpui::test]
fn opens_files_beside_the_chat_and_moves_the_editor(cx: &mut TestAppContext) {
    let h = setup(cx);
    let navigations = Rc::new(RefCell::new(Vec::new()));
    let seen = navigations.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&h.workspace, move |_, event: &WorkspaceEvent, _| {
            if let WorkspaceEvent::EditorNavigation(target) = event {
                seen.borrow_mut().push(target.clone());
            }
        })
    });
    // A shortened reference resolves through the project index.
    let task = h.workspace.update(cx, |workspace, cx| {
        workspace.open_file(
            "lib.rs",
            Some(super::paths::FileNavigation {
                line: 3,
                column: None,
            }),
            FileOpenOptions::default(),
            cx,
        )
    });
    settle(task, cx);
    let tab = active(&h, cx);
    assert_eq!(tab.editor_panes.len(), 1);
    let file = &tab.editor_panes[0].files[0];
    assert_eq!(file.path, format!("{PROJECT}/src/lib.rs"));
    // A blank chat puts the editor on the left.
    assert_eq!(leaf_ids(&tab.layout)[0], tab.editor_panes[0].id);
    assert_eq!(navigations.borrow()[0].line, 3);
    assert_eq!(h.fs.list_calls(), vec![PROJECT.to_string()]);
    assert!(
        !h.workspace
            .read_with(cx, |workspace, _| workspace.composer_focused())
    );

    h.workspace.update(cx, |workspace, cx| {
        workspace.file_moved(&format!("{PROJECT}/src"), &format!("{PROJECT}/lib"), cx)
    });
    assert_eq!(
        active(&h, cx).editor_panes[0].files[0].path,
        format!("{PROJECT}/lib/lib.rs")
    );
    h.workspace.update(cx, |workspace, cx| {
        workspace.file_deleted(&format!("{PROJECT}/lib"), cx)
    });
    let tab = active(&h, cx);
    assert!(tab.editor_panes.is_empty());
    assert_eq!(leaf_ids(&tab.layout).len(), 1);
}

#[gpui::test]
fn opens_terminals_in_the_project_dock_and_stops_closed_ones(cx: &mut TestAppContext) {
    let h = setup(cx);
    h.workspace
        .update(cx, |workspace, cx| workspace.new_terminal(cx));
    h.workspace
        .update(cx, |workspace, cx| workspace.new_terminal(cx));
    let terminals = h
        .workspace
        .read_with(cx, |workspace, _| workspace.terminals().clone());
    let ids = terminals.read_with(cx, |terminals, _| terminals.file_ids());
    assert_eq!(ids.len(), 2);
    assert!(terminals.read_with(cx, |terminals, _| terminals.is_focused()));
    assert!(
        !h.workspace
            .read_with(cx, |workspace, _| workspace.composer_focused())
    );
    assert!(
        h.workspace
            .read_with(cx, |workspace, cx| workspace.dock_layout(cx))
            .is_some()
    );

    // A view attaches, the shell starts, and the title follows the process.
    let _pty = cx.update(|cx| Terminals::global(cx).attach(&ids[0], PROJECT, cx));
    cx.run_until_parked();
    let patch = TerminalMetaPatch {
        title: Some("vite".into()),
        cwd: None,
        foreground: Some(Some("vite".into())),
    };
    cx.update(|cx| {
        let signal = Terminals::global(cx).signal.clone();
        signal.update(cx, |_, cx| {
            cx.emit(TerminalMetaChanged {
                file_id: ids[0].clone(),
                patch,
            })
        });
    });
    cx.run_until_parked();
    let running = h
        .workspace
        .read_with(cx, |workspace, cx| workspace.running_terminals(cx));
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].process, "vite");

    // Closing a busy terminal asks first, then stops its shell.
    h.pty.set_foreground(&ids[0], Some("vite"));
    let task = h.workspace.update(cx, |workspace, cx| {
        workspace.close_project_terminal(&ids[0], cx)
    });
    settle(task, cx);
    assert_eq!(h.delegate.confirms.borrow().len(), 1);
    assert!(h.delegate.confirms.borrow()[0].contains("\"vite\" is still running"));
    cx.run_until_parked();
    assert_eq!(
        terminals.read_with(cx, |terminals, _| terminals.file_ids()),
        vec![ids[1].clone()]
    );
    assert!(h.pty.calls().contains(&Call::Kill(ids[0].clone())));

    h.workspace
        .update(cx, |workspace, cx| workspace.toggle_project_terminal(cx));
    assert!(
        h.workspace
            .read_with(cx, |workspace, cx| workspace.dock_layout(cx))
            .is_none()
    );
}

#[gpui::test]
fn closing_the_last_file_pane_of_a_file_only_tab_keeps_a_chat(cx: &mut TestAppContext) {
    let h = setup(cx);
    let chat = sessions(cx)[0].id.clone();
    let tab = active(&h, cx);
    let file = new_file_tab(&format!("{PROJECT}/README.md"), PROJECT, false, None, None);
    // Turn the only tab into a file-only tab.
    let file_only = WorkspaceTab {
        layout: monocode_layout::leaf("pane"),
        focused_id: "pane".into(),
        editor_panes: vec![monocode_layout::EditorPane::new(
            "pane",
            vec![file.clone()],
            file.id.clone(),
        )],
        ..tab
    };
    // A transfer builds a window from raw tabs.
    let config = WorkspaceConfig::transferred(
        vec![file_only.clone()],
        file_only.id.clone(),
        PROJECT.into(),
        Vec::new(),
        Vec::new(),
    );
    let workspace = cx.new(|cx| Workspace::new(config, cx));
    let task = workspace.update(cx, |workspace, cx| {
        workspace.close_file("pane", &file.id, cx)
    });
    settle(task, cx);
    let tab = workspace
        .read_with(cx, |workspace, _| workspace.active_tab().cloned())
        .unwrap();
    let ids = leaf_ids(&tab.layout);
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0], "pane");
    assert!(sessions(cx).iter().any(|session| session.id == ids[0]));
    assert!(sessions(cx).iter().any(|session| session.id == chat));
    assert_eq!(tabs(&h, cx).len(), 1);
}

#[gpui::test]
fn closes_all_files_then_all_tabs(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first_chat = sessions(cx)[0].id.clone();
    with_user_turn(&first_chat, cx);
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    let task = h.workspace.update(cx, |workspace, cx| {
        workspace.open_file(
            &format!("{PROJECT}/README.md"),
            None,
            FileOpenOptions {
                exact: true,
                pin: true,
            },
            cx,
        )
    });
    settle(task, cx);
    assert_eq!(active(&h, cx).editor_panes.len(), 1);

    // Stage one closes the files only.
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_all_tabs(cx));
    settle(task, cx);
    assert!(active(&h, cx).editor_panes.is_empty());
    assert_eq!(tabs(&h, cx).len(), 2);

    // Stage two closes the other tabs and resets this one.
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.close_all_tabs(cx));
    settle(task, cx);
    assert_eq!(tabs(&h, cx).len(), 1);
}

#[gpui::test]
fn adds_a_chat_beside_a_file_only_tab(cx: &mut TestAppContext) {
    let h = setup(cx);
    let file = new_file_tab(&format!("{PROJECT}/README.md"), PROJECT, false, None, None);
    let file_only = WorkspaceTab {
        editor_panes: vec![monocode_layout::EditorPane::new(
            "pane",
            vec![file.clone()],
            file.id.clone(),
        )],
        ..new_tab("unused")
    };
    let config = WorkspaceConfig::transferred(
        vec![file_only.clone()],
        file_only.id.clone(),
        PROJECT.into(),
        Vec::new(),
        Vec::new(),
    );
    let workspace = cx.new(|cx| Workspace::new(config, cx));
    let item = ChatContextItem::Code {
        path: "src/lib.rs".into(),
        start_line: 1,
        end_line: 2,
    };
    let created = workspace
        .update(cx, |workspace, cx| workspace.add_to_chat(&item, cx))
        .unwrap();
    let session = sessions(cx)
        .into_iter()
        .find(|session| session.id == created)
        .unwrap();
    assert!(session.composer_seed.unwrap().contains("code_selection"));
    let tab = workspace
        .read_with(cx, |workspace, _| workspace.active_tab().cloned())
        .unwrap();
    assert_eq!(
        leaf_ids(&tab.layout),
        vec!["unused".to_string(), created.clone()]
    );
    // The original window's chat tab already shows a session, so it bails.
    assert_eq!(
        h.workspace
            .update(cx, |workspace, cx| workspace.add_to_chat(&item, cx)),
        None
    );
}

#[gpui::test]
fn drops_a_chat_onto_a_blank_pane(cx: &mut TestAppContext) {
    let h = setup(cx);
    let blank = sessions(cx)[0].id.clone();
    let mut stored = Session::blank("stored", HarnessId::Claude, "", PROJECT);
    stored.blocks.push(Block::new("u1", BlockRole::User, "hi"));
    h.store.insert_session(&stored);
    let task = h.workspace.update(cx, |workspace, cx| {
        workspace.place_session_on_pane("stored", &blank, PaneEdge::Right, cx)
    });
    settle(task, cx);
    let tab = active(&h, cx);
    assert_eq!(leaf_ids(&tab.layout), vec!["stored".to_string()]);
    let open: Vec<String> = sessions(cx).into_iter().map(|session| session.id).collect();
    assert!(open.contains(&"stored".to_string()));
    assert!(!open.contains(&blank));
}

#[gpui::test]
fn opens_a_stored_session_in_the_blank_pane(cx: &mut TestAppContext) {
    let h = setup(cx);
    let mut stored = Session::blank("stored", HarnessId::Claude, "", PROJECT);
    stored.blocks.push(Block::new("u1", BlockRole::User, "hi"));
    h.store.insert_session(&stored);
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.open_session("stored", cx));
    settle(task, cx);
    assert_eq!(leaf_ids(&active(&h, cx).layout), vec!["stored".to_string()]);
    assert_eq!(tabs(&h, cx).len(), 1);

    // Opening it again only focuses its tab.
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    let task = h
        .workspace
        .update(cx, |workspace, cx| workspace.open_session("stored", cx));
    settle(task, cx);
    assert_eq!(leaf_ids(&active(&h, cx).layout), vec!["stored".to_string()]);
    assert_eq!(tabs(&h, cx).len(), 2);
}

#[gpui::test]
fn removes_an_archived_session_from_its_tabs(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first = sessions(cx)[0].id.clone();
    h.workspace
        .update(cx, |workspace, cx| workspace.split(SplitDir::Down, cx));
    let removal = h.workspace.read_with(cx, |workspace, cx| {
        workspace.plan_session_removal(&first, cx)
    });
    assert!(removal.closed_tabs.is_empty());
    h.workspace.update(cx, |workspace, cx| {
        workspace.apply_session_removal(&first, removal, cx)
    });
    let tab = active(&h, cx);
    assert_eq!(leaf_ids(&tab.layout).len(), 1);
    assert!(!sessions(cx).iter().any(|session| session.id == first));
}

#[gpui::test]
fn round_trips_the_snapshot_through_the_hooks(cx: &mut TestAppContext) {
    let h = setup(cx);
    let chat = sessions(cx)[0].id.clone();
    with_user_turn(&chat, cx);
    h.workspace
        .update(cx, |workspace, cx| workspace.new_terminal(cx));
    let open = sessions(cx);
    let snapshot: Value = cx
        .update(|cx| Engine::hooks(cx).workspace.collect_snapshot(&open, cx))
        .unwrap();
    let hooks = cx.update(|cx| Engine::hooks(cx).workspace.clone());
    assert_eq!(hooks.snapshot_session_ids(&snapshot), vec![chat.clone()]);

    let loaded: HashMap<String, Session> = open
        .iter()
        .map(|session| (session.id.clone(), session.clone()))
        .collect();
    let resumed: ResumedWorkspace = hooks
        .hydrate_snapshot(&snapshot, &loaded, &HashSet::new())
        .unwrap();
    assert_eq!(resumed.project_cwd, PROJECT);
    assert_eq!(resumed.sessions.len(), 1);
    let config = WorkspaceConfig::resumed(&resumed);
    assert_eq!(config.tabs, tabs(&h, cx));
    assert_eq!(config.project_terminals.len(), 1);
    assert!(config.composer_focused);
    assert_eq!(
        hooks.collect_resumed_snapshot(&resumed).unwrap()["tabs"],
        snapshot["tabs"]
    );

    let layout = hooks.layout_for_sessions(&["a".into(), "b".into()]);
    assert_eq!(layout["tabs"].as_array().unwrap().len(), 2);
    assert_eq!(layout["activeTabId"], layout["tabs"][0]["id"]);
}

#[gpui::test]
fn reports_foreground_chats_and_hidden_windows(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first = sessions(cx)[0].id.clone();
    let hooks = cx.update(|cx| Engine::hooks(cx).workspace.clone());
    assert!(cx.update(|cx| hooks.is_foreground(&first, cx)));
    h.workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    assert!(!cx.update(|cx| hooks.is_foreground(&first, cx)));
    assert!(!cx.update(|cx| hooks.window_hidden(cx)));
    h.workspace
        .update(cx, |workspace, cx| workspace.set_window_hidden(true, cx));
    assert!(cx.update(|cx| hooks.window_hidden(cx)));
    assert_eq!(
        hooks.resolve_workspace_path("src/a.rs", PROJECT).as_deref(),
        Some("/Users/me/repo/src/a.rs")
    );
}

#[gpui::test]
fn inbox_ask_focus_preserves_the_workspace_and_tracks_page_visibility(cx: &mut TestAppContext) {
    let h = setup(cx);
    let before = active(&h, cx);
    let hooks = cx.update(|cx| Engine::hooks(cx).workspace.clone());
    h.workspace.update(cx, |workspace, cx| {
        workspace.set_inbox_session(Some("ask".into()), cx);
        workspace.set_full_page_open(true, cx);
        workspace.set_inbox_visible(true, cx);
        workspace.set_composer_focused(false, cx);
        workspace.focus_pane("ask", cx);
    });
    assert_eq!(active(&h, cx), before);
    assert!(
        h.workspace
            .read_with(cx, |workspace, _| workspace.composer_focused())
    );
    assert!(cx.update(|cx| hooks.is_foreground("ask", cx)));
    assert!(!cx.update(|cx| hooks.is_foreground(&before.focused_id, cx)));

    h.workspace
        .update(cx, |workspace, cx| workspace.set_inbox_visible(false, cx));
    assert!(!cx.update(|cx| hooks.is_foreground("ask", cx)));
    h.workspace.update(cx, |workspace, cx| {
        workspace.set_inbox_visible(true, cx);
        workspace.set_window_hidden(true, cx);
    });
    assert!(!cx.update(|cx| hooks.is_foreground("ask", cx)));
    h.workspace.update(cx, |workspace, cx| {
        workspace.set_window_hidden(false, cx);
        workspace.set_inbox_session(None, cx);
    });
    assert!(!cx.update(|cx| hooks.is_foreground("ask", cx)));
    h.workspace.update(cx, |workspace, cx| {
        workspace.set_full_page_open(false, cx);
        workspace.set_inbox_visible(false, cx);
    });
    assert!(cx.update(|cx| hooks.is_foreground(&before.focused_id, cx)));
}

#[gpui::test]
fn describes_tabs_for_the_title_bar(cx: &mut TestAppContext) {
    let h = setup(cx);
    let title = h.workspace.read_with(cx, |workspace, cx| {
        workspace.title_tabs(&HashSet::new(), cx)
    });
    assert_eq!(title.len(), 1);
    assert_eq!(title[0].project, "repo");
    assert!(title[0].blank);
}

// agentTabs.test.ts

fn agent(session_id: &str, title: &str) -> monocode_layout::FilePaneTab {
    new_agent_tab(
        title,
        "/repo",
        AgentTabSource::new(session_id, "lead", HarnessId::Codex),
    )
}

#[test]
fn gathers_every_agent_of_a_run_into_one_pane_beside_the_lead() {
    let options = OpenEditorTabOptions::default();
    let tab = open_editor_tab(
        &new_tab("lead"),
        &agent("worker-a", "Audit the engine"),
        &options,
    );
    let pane_id = tab.editor_panes[0].id.clone();
    let tab = open_editor_tab(&tab, &agent("worker-b", "Audit the UI"), &options);
    assert_eq!(tab.editor_panes.len(), 1);
    assert_eq!(tab.editor_panes[0].id, pane_id);
    let paths: Vec<&str> = tab.editor_panes[0]
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, vec!["Audit the engine", "Audit the UI"]);
    // The pane sits beside the lead, which keeps its own leaf.
    assert_eq!(
        leaf_ids(&tab.layout),
        vec!["lead".to_string(), pane_id.clone()]
    );
    assert_eq!(tab.focused_id, pane_id);
}

#[test]
fn focuses_the_tab_an_agent_already_has_instead_of_opening_a_second() {
    let options = OpenEditorTabOptions::default();
    let tab = open_editor_tab(
        &new_tab("lead"),
        &agent("worker-a", "Audit the engine"),
        &options,
    );
    let tab = open_editor_tab(&tab, &agent("worker-b", "Audit the UI"), &options);
    let first = tab.editor_panes[0].files[0].clone();
    let tab = open_editor_tab(&tab, &agent("worker-a", "Audit the engine"), &options);
    assert_eq!(tab.editor_panes[0].files.len(), 2);
    assert_eq!(tab.editor_panes[0].active_file_id, first.id);
}

#[test]
fn is_keyed_by_its_worker_and_is_not_a_file_on_disk() {
    let file = agent("worker-a", "Audit the engine");
    assert_eq!(editor_tab_key(&file), "agent:worker-a");
    assert!(is_agent_tab(&file));
    assert_eq!(file.agent.as_ref().unwrap().harness, HarnessId::Codex);
    assert!(!is_filesystem_tab(&file));
    assert!(is_filesystem_tab(&new_file_tab(
        "/repo/a.ts",
        "/repo",
        false,
        None,
        None
    )));
}

#[gpui::test]
fn collects_a_window_transfer_with_the_docks_that_follow(cx: &mut TestAppContext) {
    let h = setup(cx);
    let first = active(&h, cx).id;
    h.workspace
        .update(cx, |workspace, cx| workspace.new_terminal(cx));
    let payload = h
        .workspace
        .read_with(cx, |workspace, cx| {
            workspace.window_transfer(std::slice::from_ref(&first), cx)
        })
        .unwrap();
    assert_eq!(payload.active_tab_id, first);
    assert_eq!(payload.sessions.len(), 1);
    assert_eq!(payload.project_terminals.map(|docks| docks.len()), Some(1));
    let config = WorkspaceConfig::transferred(
        payload.tabs,
        payload.active_tab_id,
        payload.project_cwd,
        payload.dirty_file_ids,
        Vec::new(),
    );
    assert!(!config.autosave);
}
