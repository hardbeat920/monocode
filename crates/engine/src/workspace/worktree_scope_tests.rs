//! Worktree-scoped workspaces on the `Workspace` entity. Ports
//! src/app/hooks/useWorkspaceNavigation.test.ts against a fake worktree
//! move, and the App.tsx side of the worktree workspaces: new sessions in
//! the focused worktree, the deck, closing, clearing, and the saved tabs.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{App, AppContext, Entity, Task, TestAppContext};
use monocode_core::block::{Block, BlockRole};
use monocode_core::session::session_work_cwd;
use monocode_core::{HarnessId, Session};
use monocode_layout::{SplitDir, WorkspaceTab, new_tab, split_pane};
use serde_json::Value;

use super::delegate::{IsCurrent, WorkspaceDelegate, WorktreeTarget};
use super::files::backend::fake::FakeFs;
use super::hooks::{WorkspaceSetup, init};
use super::session_factory::ModelEnvSessions;
use super::workspace::{Workspace, WorkspaceConfig};
use super::worktree_scope::WorktreeFocus;
use crate::runtime::Engine;
use crate::runtime::testing::init_test_engine;

const PROJECT: &str = "/navigation-project";
const OTHER: &str = "/navigation-other";

fn tree(name: &str) -> WorktreeFocus {
    WorktreeFocus {
        path: format!("/navigation-trees/{name}"),
        branch: Some(name.to_string()),
    }
}

type Reply = oneshot::Receiver<Result<(), String>>;

/// The worktree move: each call takes the next queued reply, or succeeds at
/// once. A move that is still current when its reply arrives moves the
/// session, as `onWorktreeChange` does.
#[derive(Default)]
struct MoveDelegate {
    replies: RefCell<VecDeque<Reply>>,
    calls: RefCell<Vec<(String, String, IsCurrent)>>,
}

impl MoveDelegate {
    fn defer(&self) -> oneshot::Sender<Result<(), String>> {
        let (send, receive) = oneshot::channel();
        self.replies.borrow_mut().push_back(receive);
        send
    }

    fn paths(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|(_, path, _)| path.clone())
            .collect()
    }
}

impl WorkspaceDelegate for MoveDelegate {
    fn move_session_to_worktree(
        &self,
        session_id: &str,
        target: WorktreeTarget,
        is_current: IsCurrent,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        self.calls.borrow_mut().push((
            session_id.to_string(),
            target.path.clone(),
            is_current.clone(),
        ));
        let reply = self.replies.borrow_mut().pop_front();
        let id = session_id.to_string();
        cx.spawn(async move |cx| {
            let result = match reply {
                Some(reply) => reply.await.unwrap_or(Ok(())),
                None => Ok(()),
            };
            result?;
            cx.update(|cx| {
                if !is_current(cx) {
                    return;
                }
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    sessions.update(&id, cx, |session| {
                        session.worktree_cwd = (!target.is_main).then(|| target.path.clone());
                        session.branch = target.branch.clone();
                    });
                });
            });
            Ok(())
        })
    }
}

fn chat(id: &str, cwd: &str, worktree: Option<&str>, started: bool) -> Session {
    let mut session = Session::blank(id, HarnessId::Codex, "", cwd);
    session.worktree_cwd = worktree.map(str::to_string);
    if started {
        session.blocks = vec![Block::new(format!("prompt-{id}"), BlockRole::User, "hello")];
    }
    session
}

fn tab_for(id: &str) -> WorkspaceTab {
    WorkspaceTab {
        id: format!("tab-{id}"),
        ..new_tab(id)
    }
}

struct Harness {
    delegate: Rc<MoveDelegate>,
    workspace: Entity<Workspace>,
}

/// A workspace showing `active`'s tab, with one tab per session unless
/// `tabs` is given. Pins start as the upstream hook tests start them: only
/// the tab on screen.
fn mount(
    sessions: Vec<Session>,
    active: &str,
    tabs: Option<Vec<WorkspaceTab>>,
    cx: &mut TestAppContext,
) -> Harness {
    init_test_engine(cx);
    let delegate = Rc::new(MoveDelegate::default());
    let setup = WorkspaceSetup {
        fs: FakeFs::new(),
        sessions: Rc::new(ModelEnvSessions::default()),
        delegate: delegate.clone(),
        terminals: false,
    };
    cx.update(|cx| init(setup, cx));
    let tabs = tabs.unwrap_or_else(|| sessions.iter().map(|s| tab_for(&s.id)).collect());
    let active_tab_id = tabs
        .iter()
        .find(|tab| monocode_layout::leaf_ids(&tab.layout).contains(&active.to_string()))
        .map(|tab| tab.id.clone())
        .unwrap();
    let project = sessions
        .iter()
        .find(|session| session.id == active)
        .map(|session| session.cwd.clone())
        .unwrap();
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |open, cx| {
            for session in sessions {
                open.insert(session, cx);
            }
        })
    });
    let config = WorkspaceConfig {
        tabs,
        active_tab_id,
        project_cwd: project,
        ..WorkspaceConfig::fresh(None)
    };
    let workspace = cx.new(|cx| Workspace::new(config, cx));
    workspace.update(cx, |workspace, cx| workspace.reset_pins(cx));
    cx.run_until_parked();
    Harness {
        delegate,
        workspace,
    }
}

/// What the upstream harness rendered.
#[derive(Debug, PartialEq)]
struct Screen {
    workspace: String,
    active_tab: String,
    checkout: String,
    pending: bool,
    error: Option<String>,
}

fn screen(h: &Harness, cx: &mut TestAppContext) -> Screen {
    h.workspace.read_with(cx, |workspace, cx| {
        let project = workspace.project_cwd().to_string();
        let checkout = workspace
            .active_tab()
            .and_then(|tab| {
                Engine::sessions(cx)
                    .read(cx)
                    .get(&tab.focused_id)
                    .map(|session| session_work_cwd(session).to_string())
            })
            .unwrap_or_default();
        Screen {
            workspace: workspace.current_workspace(&project),
            active_tab: workspace.active_tab_id().to_string(),
            checkout,
            pending: workspace.navigation_pending(&project),
            error: workspace.navigation_error(&project).map(str::to_string),
        }
    })
}

fn expect(workspace: &str, active_tab: &str, checkout: &str, pending: bool) -> Screen {
    Screen {
        workspace: workspace.into(),
        active_tab: active_tab.into(),
        checkout: checkout.into(),
        pending,
        error: None,
    }
}

fn select(h: &Harness, focus: Option<WorktreeFocus>, cx: &mut TestAppContext) {
    h.workspace.update(cx, |workspace, cx| {
        workspace.select_workspace(PROJECT, focus, cx)
    });
    cx.run_until_parked();
}

/// `openProject`: the rail records the request, then lands on a tab.
fn open_project(h: &Harness, path: &str, landing: &str, cx: &mut TestAppContext) {
    h.workspace.update(cx, |workspace, cx| {
        workspace.select_project(path, cx);
        workspace.activate_tab(landing, None, cx);
    });
    cx.run_until_parked();
}

/// `directOpen`: a session opened from the list, search, or the inbox.
fn direct_open(h: &Harness, session_id: &str, cx: &mut TestAppContext) {
    let open = h
        .workspace
        .update(cx, |workspace, cx| workspace.open_session(session_id, cx));
    cx.run_until_parked();
    drop(open);
}

fn resolve(
    reply: oneshot::Sender<Result<(), String>>,
    result: Result<(), String>,
    cx: &mut TestAppContext,
) {
    reply.send(result).ok();
    cx.run_until_parked();
}

fn switching(h: &Harness, id: &str, cx: &mut TestAppContext) -> bool {
    h.workspace
        .read_with(cx, |workspace, cx| workspace.is_switching(id, cx))
}

fn pin(h: &Harness, id: &str, cx: &mut TestAppContext) -> Option<String> {
    h.workspace
        .read_with(cx, |workspace, _| workspace.pin(id).map(str::to_string))
}

fn session(id: &str, cx: &mut TestAppContext) -> Session {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned().unwrap())
}

fn focus_of(h: &Harness, project: &str, cx: &mut TestAppContext) -> Option<String> {
    h.workspace.read_with(cx, |workspace, _| {
        workspace
            .worktree_focus(project)
            .map(|focus| focus.path.clone())
    })
}

fn set_focus(h: &Harness, project: &str, focus: WorktreeFocus, cx: &mut TestAppContext) {
    h.workspace.update(cx, |workspace, cx| {
        workspace.set_focus_for_test(project, Some(focus), cx)
    });
}

// useWorkspaceNavigation.test.ts

#[gpui::test]
fn serializes_a_then_b_and_never_publishes_a_after_b_was_requested(cx: &mut TestAppContext) {
    let h = mount(vec![chat("blank", PROJECT, None, false)], "blank", None, cx);
    let first = h.delegate.defer();
    let second = h.delegate.defer();
    let (a, b) = (tree("a"), tree("b"));
    select(&h, Some(a), cx);
    assert!(switching(&h, "blank", cx));
    assert_eq!(screen(&h, cx), expect(PROJECT, "tab-blank", PROJECT, true));
    select(&h, Some(b.clone()), cx);
    assert_eq!(h.delegate.calls.borrow().len(), 1);
    resolve(first, Ok(()), cx);
    assert_eq!(h.delegate.calls.borrow().len(), 2);
    let stale = h.delegate.calls.borrow()[0].2.clone();
    assert!(!cx.update(|cx| stale(cx)));
    assert_eq!(h.delegate.paths()[1], b.path);
    assert!(switching(&h, "blank", cx));
    assert_eq!(pin(&h, "tab-blank", cx).as_deref(), Some(PROJECT));
    assert_eq!(screen(&h, cx), expect(PROJECT, "tab-blank", PROJECT, true));
    resolve(second, Ok(()), cx);
    assert_eq!(screen(&h, cx), expect(&b.path, "tab-blank", &b.path, false));
    assert_eq!(pin(&h, "tab-blank", cx), Some(b.path.clone()));
    assert!(!switching(&h, "blank", cx));
}

#[gpui::test]
fn coalesces_superseded_selections_and_ignores_an_obsolete_failure(cx: &mut TestAppContext) {
    let h = mount(vec![chat("blank", PROJECT, None, false)], "blank", None, cx);
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    select(&h, Some(tree("b")), cx);
    select(&h, Some(tree("c")), cx);
    resolve(first, Err("obsolete failure".into()), cx);
    assert_eq!(h.delegate.paths(), [tree("a").path, tree("c").path]);
    let c = tree("c").path;
    assert_eq!(screen(&h, cx), expect(&c, "tab-blank", &c, false));
}

#[gpui::test]
fn keeps_the_previous_workspace_and_reports_a_failed_move_then_allows_retry(
    cx: &mut TestAppContext,
) {
    let h = mount(vec![chat("blank", PROJECT, None, false)], "blank", None, cx);
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    resolve(first, Err("Working copy no longer exists".into()), cx);
    assert_eq!(
        screen(&h, cx),
        Screen {
            error: Some("Working copy no longer exists".into()),
            ..expect(PROJECT, "tab-blank", PROJECT, false)
        }
    );
    assert_eq!(pin(&h, "tab-blank", cx).as_deref(), Some(PROJECT));
    assert!(!switching(&h, "blank", cx));
    let b = tree("b").path;
    select(&h, Some(tree("b")), cx);
    assert_eq!(screen(&h, cx), expect(&b, "tab-blank", &b, false));
}

#[gpui::test]
fn an_explicit_session_open_supersedes_a_pending_workspace_move(cx: &mut TestAppContext) {
    let h = mount(
        vec![
            chat("blank", PROJECT, None, false),
            chat("requested", PROJECT, None, true),
        ],
        "blank",
        None,
        cx,
    );
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    direct_open(&h, "requested", cx);
    resolve(first, Ok(()), cx);
    assert_eq!(
        screen(&h, cx),
        expect(PROJECT, "tab-requested", PROJECT, false)
    );
    assert_eq!(session("blank", cx).worktree_cwd, None);
}

#[gpui::test]
fn opening_the_same_session_still_cancels_a_pending_move(cx: &mut TestAppContext) {
    let h = mount(vec![chat("blank", PROJECT, None, false)], "blank", None, cx);
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    direct_open(&h, "blank", cx);
    // The draft stays blocked while the cancelled move cleans up.
    assert!(!screen(&h, cx).pending);
    assert!(switching(&h, "blank", cx));
    resolve(first, Ok(()), cx);
    assert_eq!(screen(&h, cx), expect(PROJECT, "tab-blank", PROJECT, false));
    assert!(!switching(&h, "blank", cx));
}

#[gpui::test]
fn can_return_to_the_original_workspace_while_a_move_is_pending(cx: &mut TestAppContext) {
    let h = mount(vec![chat("blank", PROJECT, None, false)], "blank", None, cx);
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    select(&h, None, cx);
    resolve(first, Ok(()), cx);
    assert_eq!(screen(&h, cx), expect(PROJECT, "tab-blank", PROJECT, false));
    assert_eq!(pin(&h, "tab-blank", cx).as_deref(), Some(PROJECT));
    assert_eq!(h.delegate.calls.borrow().len(), 1);
    assert!(!switching(&h, "blank", cx));
}

fn three_projects() -> Vec<Session> {
    vec![
        chat("worktree", PROJECT, Some(&tree("a").path), true),
        chat("main", PROJECT, None, true),
        chat("other", OTHER, None, true),
    ]
}

#[gpui::test]
fn a_completed_project_switch_cannot_override_a_later_explicit_session_open(
    cx: &mut TestAppContext,
) {
    let h = mount(three_projects(), "worktree", None, cx);
    set_focus(&h, PROJECT, tree("a"), cx);
    open_project(&h, OTHER, "tab-other", cx);
    assert_eq!(screen(&h, cx).active_tab, "tab-other");
    direct_open(&h, "main", cx);
    let a = tree("a").path;
    assert_eq!(screen(&h, cx), expect(&a, "tab-main", PROJECT, false));
    assert_eq!(pin(&h, "tab-main", cx), Some(a));
    assert!(h.delegate.calls.borrow().is_empty());
}

#[gpui::test]
fn project_selection_still_restores_the_last_tab_in_its_remembered_workspace(
    cx: &mut TestAppContext,
) {
    let h = mount(three_projects(), "worktree", None, cx);
    set_focus(&h, PROJECT, tree("a"), cx);
    open_project(&h, OTHER, "tab-other", cx);
    open_project(&h, PROJECT, "tab-main", cx);
    let a = tree("a").path;
    assert_eq!(screen(&h, cx), expect(&a, "tab-worktree", &a, false));
    assert!(h.delegate.calls.borrow().is_empty());
    assert_eq!(pin(&h, "tab-main", cx), None);
}

#[gpui::test]
fn selects_an_existing_workspace_tab_without_moving_the_current_session(cx: &mut TestAppContext) {
    let h = mount(
        vec![
            chat("main", PROJECT, None, true),
            chat("worktree", PROJECT, Some(&tree("a").path), true),
        ],
        "main",
        None,
        cx,
    );
    select(&h, Some(tree("a")), cx);
    let a = tree("a").path;
    assert_eq!(screen(&h, cx), expect(&a, "tab-worktree", &a, false));
    assert!(h.delegate.calls.borrow().is_empty());
}

#[gpui::test]
fn creates_a_session_in_an_empty_workspace_without_moving_a_conversation(cx: &mut TestAppContext) {
    let h = mount(vec![chat("main", PROJECT, None, true)], "main", None, cx);
    select(&h, Some(tree("a")), cx);
    let a = tree("a").path;
    let shown = screen(&h, cx);
    assert_ne!(shown.active_tab, "tab-main");
    assert_eq!(shown.workspace, a);
    assert_eq!(shown.checkout, a);
    assert_eq!(session("main", cx).worktree_cwd, None);
    assert!(h.delegate.calls.borrow().is_empty());
    // The new chat carries the worktree's branch.
    let created = h
        .workspace
        .read_with(cx, |workspace, cx| workspace.active_session(cx).unwrap());
    assert_eq!(created.branch.as_deref(), Some("a"));
}

#[gpui::test]
fn a_delayed_completion_cannot_return_to_the_project_the_user_left(cx: &mut TestAppContext) {
    let h = mount(
        vec![
            chat("blank", PROJECT, None, false),
            chat("other", OTHER, None, true),
        ],
        "blank",
        None,
        cx,
    );
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    open_project(&h, OTHER, "tab-other", cx);
    resolve(first, Ok(()), cx);
    assert_eq!(screen(&h, cx), expect(OTHER, "tab-other", OTHER, false));
    assert_eq!(focus_of(&h, PROJECT, cx), None);
    assert_eq!(session("blank", cx).worktree_cwd, None);
}

#[gpui::test]
fn does_not_commit_after_the_workspace_closes(cx: &mut TestAppContext) {
    let h = mount(vec![chat("blank", PROJECT, None, false)], "blank", None, cx);
    let first = h.delegate.defer();
    select(&h, Some(tree("a")), cx);
    let Harness {
        delegate,
        workspace,
    } = h;
    drop(workspace);
    cx.run_until_parked();
    resolve(first, Ok(()), cx);
    assert_eq!(delegate.calls.borrow().len(), 1);
    assert_eq!(session("blank", cx).worktree_cwd, None);
}

#[gpui::test]
fn a_failed_project_restore_returns_to_its_landing_workspace(cx: &mut TestAppContext) {
    let h = mount(
        vec![
            chat("other", OTHER, None, true),
            chat("blank", PROJECT, None, false),
        ],
        "other",
        None,
        cx,
    );
    set_focus(&h, PROJECT, tree("a"), cx);
    let first = h.delegate.defer();
    open_project(&h, PROJECT, "tab-blank", cx);
    resolve(first, Err("Unable to restore worktree".into()), cx);
    assert_eq!(
        screen(&h, cx),
        Screen {
            error: Some("Unable to restore worktree".into()),
            ..expect(PROJECT, "tab-blank", PROJECT, false)
        }
    );
}

#[gpui::test]
fn preserves_the_landing_conversation_when_the_remembered_workspace_has_no_open_tab(
    cx: &mut TestAppContext,
) {
    let h = mount(
        vec![
            chat("other", OTHER, None, true),
            chat("main", PROJECT, None, true),
        ],
        "other",
        None,
        cx,
    );
    set_focus(&h, PROJECT, tree("a"), cx);
    open_project(&h, PROJECT, "tab-main", cx);
    let a = tree("a").path;
    assert_eq!(screen(&h, cx), expect(&a, "tab-main", PROJECT, false));
    assert_eq!(pin(&h, "tab-main", cx), Some(a));
    assert_eq!(cx.update(|cx| Engine::sessions(cx).read(cx).all().len()), 2);
    assert!(h.delegate.calls.borrow().is_empty());
}

#[gpui::test]
fn keeps_a_split_tab_grouped_under_another_project_without_leaving_navigation_pending(
    cx: &mut TestAppContext,
) {
    let mut mixed = tab_for("main");
    mixed.layout = split_pane(&mixed.layout, "main", SplitDir::Right, "other");
    mixed.focused_id = "other".into();
    let h = mount(
        vec![
            chat("main", PROJECT, None, false),
            chat("other", OTHER, None, false),
        ],
        "other",
        Some(vec![mixed]),
        cx,
    );
    set_focus(&h, OTHER, tree("b"), cx);
    h.workspace
        .update(cx, |workspace, cx| workspace.select_project(OTHER, cx));
    cx.run_until_parked();
    let b = tree("b").path;
    assert_eq!(screen(&h, cx), expect(&b, "tab-main", OTHER, false));
    assert_eq!(session("main", cx).worktree_cwd, None);
    assert_eq!(session("other", cx).worktree_cwd, None);
    assert_eq!(pin(&h, "tab-main", cx).as_deref(), Some(PROJECT));
    assert_eq!(cx.update(|cx| Engine::sessions(cx).read(cx).all().len()), 2);
    assert!(h.delegate.calls.borrow().is_empty());
}

// The worktree workspaces of App.tsx.

#[gpui::test]
fn new_sessions_start_in_the_focused_worktree_and_the_deck_shows_its_tabs(cx: &mut TestAppContext) {
    let h = mount(
        vec![
            chat("main", PROJECT, None, true),
            chat("worktree", PROJECT, Some(&tree("a").path), true),
        ],
        "main",
        None,
        cx,
    );
    select(&h, Some(tree("a")), cx);
    let id = h
        .workspace
        .update(cx, |workspace, cx| workspace.new_session_tab(cx));
    let created = session(&id, cx);
    assert_eq!(created.worktree_cwd, Some(tree("a").path));
    assert_eq!(created.branch.as_deref(), Some("a"));
    let deck: Vec<String> = h.workspace.read_with(cx, |workspace, cx| {
        workspace
            .deck_project_tabs(cx)
            .into_iter()
            .map(|tab| tab.id)
            .collect()
    });
    assert!(!deck.contains(&"tab-main".to_string()));
    assert!(deck.contains(&"tab-worktree".to_string()));
    assert_eq!(deck.len(), 2);
    let stats = h
        .workspace
        .read_with(cx, |workspace, cx| workspace.worktree_tab_stats(cx));
    let key = monocode_core::paths::path_key;
    assert_eq!(stats.get(&key(PROJECT)).map(|stat| stat.tabs), Some(1));
    assert_eq!(
        stats.get(&key(&tree("a").path)).map(|stat| stat.tabs),
        Some(2)
    );
}

#[gpui::test]
fn saved_workspaces_keep_only_the_default_workspace_tabs(cx: &mut TestAppContext) {
    let h = mount(
        vec![
            chat("main", PROJECT, None, true),
            chat("worktree", PROJECT, Some(&tree("a").path), true),
        ],
        "main",
        None,
        cx,
    );
    select(&h, Some(tree("a")), cx);
    let snapshot: Value = cx
        .update(|cx| {
            let sessions = Engine::sessions(cx).read(cx).all().to_vec();
            Engine::hooks(cx).workspace.collect_snapshot(&sessions, cx)
        })
        .unwrap();
    let tab_ids: Vec<&str> = snapshot["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| tab["id"].as_str().unwrap())
        .collect();
    assert_eq!(tab_ids, ["tab-main"]);
    let session_ids: Vec<&str> = snapshot["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stub| stub["id"].as_str().unwrap())
        .collect();
    assert_eq!(session_ids, ["main"]);
}

#[gpui::test]
fn closing_a_tab_stays_in_its_worktree_and_clearing_keeps_the_worktree(cx: &mut TestAppContext) {
    let a = tree("a").path;
    let h = mount(
        vec![
            chat("main", PROJECT, None, true),
            chat("first", PROJECT, Some(&a), true),
            chat("second", PROJECT, Some(&a), true),
        ],
        "main",
        None,
        cx,
    );
    select(&h, Some(tree("a")), cx);
    assert_eq!(screen(&h, cx).active_tab, "tab-second");
    let close = h.workspace.update(cx, |workspace, cx| {
        workspace.close_tab("tab-second", &[], cx)
    });
    cx.run_until_parked();
    drop(close);
    assert_eq!(screen(&h, cx).active_tab, "tab-first");
    // The last tab of the worktree stays and only resets its chat.
    let clear = h.workspace.update(cx, |workspace, cx| {
        workspace.close_title_tab("tab-first", cx)
    });
    cx.run_until_parked();
    drop(clear);
    let shown = screen(&h, cx);
    assert_eq!(shown.active_tab, "tab-first");
    assert_eq!(shown.workspace, a);
    assert_eq!(shown.checkout, a);
}
