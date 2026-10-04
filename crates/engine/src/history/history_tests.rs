//! Entity tests for `History`, ported from the App.tsx history behavior and
//! from the sidebar multiselection cases in src/app/shell/SidebarRename.test.ts.
//! `FakeBackend` stands in for the mocked `invoke`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AppContext, Entity, TestAppContext};
use monocode_core::Extra;
use monocode_core::block::{Block, BlockRole};
use monocode_core::harness::RuntimeMode;
use monocode_core::inbox::WorkItemKind;
use monocode_core::session::LinkedWorkItem;
use monocode_layout::layout::new_tab;

use super::*;
use crate::history::host::{ActivePane, WorkspaceTabs};
use crate::history::session_filters::SessionSidebarFilters;
use crate::history::session_folders::{
    SessionListDropTarget, load_session_folders, save_session_folders,
};
use crate::history::session_workspace_lifecycle::SessionWorkspaceRemoval;
use crate::history::sidebar::{
    CardClick, ReminderDue, SessionListInput, SessionListView, SessionMenuAction,
};
use crate::runtime::testing::{FakeBackend, init_test_engine};

const CWD: &str = "/workspace/project";

#[derive(Default)]
struct TestHost {
    tabs: RefCell<WorkspaceTabs>,
    active: RefCell<Option<ActivePane>>,
    opened: RefCell<Vec<String>>,
    inspected: RefCell<Vec<String>>,
    switched: RefCell<Vec<(String, String, String)>>,
    alerts: RefCell<Vec<String>>,
    confirms: RefCell<Vec<String>>,
    confirm_answer: Cell<bool>,
    commits: RefCell<Vec<SessionWorkspaceRemoval>>,
    leads: RefCell<HashMap<String, String>>,
    linked_changes: RefCell<Vec<String>>,
}

impl HistoryHost for TestHost {
    fn workspace_tabs(&self, _cx: &App) -> WorkspaceTabs {
        self.tabs.borrow().clone()
    }

    fn commit_removal(&self, removal: &SessionWorkspaceRemoval, _cx: &mut App) {
        let mut tabs = self.tabs.borrow_mut();
        tabs.tabs = removal.tabs.clone();
        tabs.active_tab_id = removal.active_tab_id.clone();
        self.commits.borrow_mut().push(removal.clone());
    }

    fn open_session(&self, session: &Session, _cx: &mut App) {
        self.opened.borrow_mut().push(session.id.clone());
    }

    fn active_pane(&self, _cx: &App) -> Option<ActivePane> {
        self.active.borrow().clone()
    }

    fn switch_session_in_tab(&self, tab_id: &str, focused_id: &str, next_id: &str, _cx: &mut App) {
        self.switched
            .borrow_mut()
            .push((tab_id.into(), focused_id.into(), next_id.into()));
    }

    fn inspect_worker(&self, session_id: &str, _cx: &mut App) {
        self.inspected.borrow_mut().push(session_id.into());
    }

    fn linked_work_item_changed(&self, session_id: &str, _cx: &mut App) {
        self.linked_changes.borrow_mut().push(session_id.into());
    }

    fn alert(&self, message: &str, _kind: AlertKind, _cx: &mut App) {
        self.alerts.borrow_mut().push(message.into());
    }

    fn confirm(&self, message: &str, _cx: &mut App) -> Task<bool> {
        self.confirms.borrow_mut().push(message.into());
        Task::ready(self.confirm_answer.get())
    }

    fn run_lead_for_session(&self, session_id: &str, _cx: &App) -> Option<String> {
        self.leads.borrow().get(session_id).cloned()
    }
}

struct T {
    backend: Arc<FakeBackend>,
    history: Entity<History>,
    host: Rc<TestHost>,
    kv: Kv,
}

fn setup(cx: &mut TestAppContext) -> T {
    let backend = init_test_engine(cx);
    let kv = Kv::in_memory();
    let host = Rc::new(TestHost::default());
    host.confirm_answer.set(true);
    let history = cx.new(|cx| History::new(kv.clone(), cx));
    let installed: Rc<dyn HistoryHost> = host.clone();
    history.update(cx, |history, _| history.set_host(installed));
    T {
        backend,
        history,
        host,
        kv,
    }
}

impl T {
    fn sessions(&self, cx: &mut TestAppContext) -> Entity<Sessions> {
        cx.update(|cx| Engine::sessions(cx))
    }

    fn open(&self, cx: &mut TestAppContext, session: Session) {
        let sessions = self.sessions(cx);
        sessions.update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
    }

    fn open_ids(&self, cx: &mut TestAppContext) -> Vec<String> {
        let sessions = self.sessions(cx);
        sessions.read_with(cx, |sessions, _| sessions.ids())
    }

    fn rows(&self, cx: &mut TestAppContext) -> Vec<SessionSummary> {
        self.history
            .read_with(cx, |history, _| history.rows().to_vec())
    }

    fn show(&self, cx: &mut TestAppContext, cwd: &str) {
        self.history
            .update(cx, |history, cx| history.set_sidebar_cwd(cwd, cx));
        cx.run_until_parked();
    }
}

fn chat(id: &str) -> Session {
    let mut session = Session::blank(id, HarnessId::Codex, "codex:test", CWD);
    session.title = format!("codex · {id}");
    session.blocks = vec![Block::new(format!("{id}-u1"), BlockRole::User, "hello")];
    session
}

fn row(id: &str, updated_at: i64) -> SessionSummary {
    SessionSummary {
        model: String::new(),
        runtime_mode: RuntimeMode::Supervised,
        title: format_session_title(HarnessId::Codex, "Original conversation"),
        created_at: updated_at,
        updated_at,
        ..SessionSummary::new(id, CWD, HarnessId::Codex)
    }
}

fn ids(rows: &[SessionSummary]) -> Vec<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}

// Listing.

#[gpui::test]
fn lists_a_project_and_reports_its_first_load(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("s1"));
    t.history
        .update(cx, |history, cx| history.set_sidebar_cwd(CWD, cx));
    assert!(t.history.read_with(cx, |history, _| history.is_pending()));
    cx.run_until_parked();
    t.history.read_with(cx, |history, _| {
        assert!(!history.is_pending());
        assert!(!history.has_failed());
        assert_eq!(ids(history.rows()), vec!["s1"]);
        assert_eq!(ids(&history.project_history()), vec!["s1"]);
    });
}

#[gpui::test]
fn a_failed_first_listing_reports_an_error_but_a_failed_revalidate_keeps_rows(
    cx: &mut TestAppContext,
) {
    let t = setup(cx);
    t.backend.set_failing("session_list_by_project", true);
    t.show(cx, CWD);
    assert!(t.history.read_with(cx, |history, _| history.has_failed()));
    assert!(!t.history.read_with(cx, |history, _| history.is_pending()));

    t.backend.set_failing("session_list_by_project", false);
    t.backend.insert_session(&chat("s1"));
    t.history.update(cx, |history, cx| history.refresh(CWD, cx));
    cx.run_until_parked();
    assert!(!t.history.read_with(cx, |history, _| history.has_failed()));
    t.backend.set_failing("session_list_by_project", true);
    t.history.update(cx, |history, cx| history.refresh(CWD, cx));
    cx.run_until_parked();
    assert!(!t.history.read_with(cx, |history, _| history.has_failed()));
    assert_eq!(ids(&t.rows(cx)), vec!["s1"]);
}

#[gpui::test]
fn merges_saved_sessions_of_the_sidebar_project_only(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.show(cx, CWD);
    let sessions = t.sessions(cx);
    let mut elsewhere = chat("other");
    elsewhere.cwd = "/workspace/other".into();
    t.open(cx, chat("s1"));
    t.open(cx, elsewhere);
    sessions.update(cx, |sessions, cx| {
        sessions.persist("s1", cx);
        sessions.persist("other", cx);
    });
    cx.run_until_parked();
    assert_eq!(ids(&t.rows(cx)), vec!["s1"]);
}

#[gpui::test]
fn shows_live_sessions_with_the_project_overlay(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.show(cx, CWD);
    let mut live = chat("live");
    live.busy = Some(true);
    let rows = t.history.read_with(cx, |history, _| {
        history.sidebar_history(std::slice::from_ref(&live), Some("main"), &[])
    });
    assert_eq!(ids(&rows), vec!["live"]);
    assert_eq!(rows[0].repo.as_deref(), Some("project"));
    assert_eq!(rows[0].branch.as_deref(), Some("main"));
    let open = t.history.read_with(cx, |history, _| {
        history.open_project_sessions(&[live], None)
    });
    assert_eq!(ids(&open), vec!["live"]);
}

// Pins, archive, and links.

#[gpui::test]
fn pins_an_open_session_after_saving_it(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.show(cx, CWD);
    t.open(cx, chat("s1"));
    cx.run_until_parked();
    t.backend.clear_calls();
    t.history
        .update(cx, |history, cx| history.pin_session("s1", true, cx))
        .detach();
    cx.run_until_parked();
    assert_eq!(
        t.backend.commands(),
        vec!["session_upsert", "session_set_pinned"]
    );
    let rows = t.rows(cx);
    assert_eq!(
        rows.iter().find(|r| r.id == "s1").unwrap().pinned,
        Some(true)
    );
}

#[gpui::test]
fn unarchives_a_row_in_place(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.history.update(cx, |history, cx| {
        history.set_boot_rows(
            vec![SessionSummary {
                archived: Some(true),
                ..row("s1", 1)
            }],
            Some(CWD),
            cx,
        )
    });
    let done = t
        .history
        .update(cx, |history, cx| history.archive_session("s1", false, cx));
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(done), Some(true));
    assert_eq!(t.rows(cx)[0].archived, Some(false));
    assert_eq!(
        t.backend.calls("session_set_archived")[0]["archived"],
        false
    );
}

#[gpui::test]
fn reports_a_failed_unarchive(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.set_failing("session_set_archived", true);
    let done = t
        .history
        .update(cx, |history, cx| history.archive_session("s1", false, cx));
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(done), Some(false));
    assert!(t.host.alerts.borrow()[0].starts_with("Could not unarchive this conversation."));
}

fn link() -> LinkedWorkItem {
    LinkedWorkItem {
        kind: WorkItemKind::Issue,
        repo: "acme/app".into(),
        number: 7,
        url: "https://github.com/acme/app/issues/7".into(),
        extra: Extra::new(),
    }
}

#[gpui::test]
fn links_a_work_item_and_rolls_back_when_the_store_refuses(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.show(cx, CWD);
    t.open(cx, chat("s1"));
    t.history.update(cx, |history, cx| {
        history.update_rows(cx, |rows| rows.push(row("s1", 1)));
    });
    t.backend.set_failing("session_set_linked_work_item", true);
    t.history.update(cx, |history, cx| {
        history.set_linked_work_item("s1", Some(link()), cx)
    });
    let sessions = t.sessions(cx);
    assert_eq!(
        sessions.read_with(cx, |s, _| s.get("s1").unwrap().linked_work_item.clone()),
        Some(link())
    );
    assert_eq!(t.rows(cx)[0].linked_work_item, Some(link()));
    assert_eq!(*t.host.linked_changes.borrow(), vec!["s1".to_string()]);
    cx.run_until_parked();
    assert_eq!(
        sessions.read_with(cx, |s, _| s.get("s1").unwrap().linked_work_item.clone()),
        None
    );
    assert!(t.rows(cx).iter().all(|row| row.linked_work_item.is_none()));
    assert!(
        t.host.alerts.borrow()[0].starts_with("Could not update this conversation's GitHub link.")
    );
}

// Renaming.

#[gpui::test]
fn renames_an_open_session_with_its_harness_prefix(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.show(cx, CWD);
    t.open(cx, chat("s1"));
    cx.run_until_parked();
    t.history
        .update(cx, |history, cx| {
            history.rename_session("s1", "  New name  ", cx)
        })
        .detach();
    cx.run_until_parked();
    let sessions = t.sessions(cx);
    assert_eq!(
        sessions.read_with(cx, |s, _| s.get("s1").unwrap().title.clone()),
        format_session_title(HarnessId::Codex, "New name")
    );
    let upserts = t.backend.calls("session_upsert");
    assert_eq!(
        upserts.last().unwrap()["session"]["title"],
        format_session_title(HarnessId::Codex, "New name")
    );
}

#[gpui::test]
fn renames_a_stored_session_without_opening_it(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("s1"));
    t.show(cx, CWD);
    t.history
        .update(cx, |history, cx| history.rename_session("s1", "Stored", cx))
        .detach();
    cx.run_until_parked();
    assert_eq!(
        t.backend.record("s1").unwrap().title,
        format_session_title(HarnessId::Codex, "Stored")
    );
    assert!(t.open_ids(cx).is_empty());
    let sessions = t.sessions(cx);
    assert!(sessions.read_with(cx, |s, _| s.loaded_cache().contains("s1")));
    assert_eq!(
        t.rows(cx)[0].title,
        format_session_title(HarnessId::Codex, "Stored")
    );
}

// Removal.

#[gpui::test]
fn archives_an_open_session_and_marks_its_row(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.show(cx, CWD);
    t.open(cx, chat("s1"));
    t.open(cx, chat("s2"));
    {
        let mut tabs = t.host.tabs.borrow_mut();
        tabs.tabs = vec![new_tab("s1"), new_tab("s2")];
        tabs.active_tab_id = tabs.tabs[0].id.clone();
    }
    cx.run_until_parked();
    t.backend.clear_calls();
    let done = t
        .history
        .update(cx, |history, cx| history.archive_session("s1", true, cx));
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(done), Some(true));
    assert_eq!(
        t.backend.commands(),
        vec!["session_upsert", "session_set_archived"]
    );
    assert_eq!(t.open_ids(cx), vec!["s2"]);
    let rows = t.rows(cx);
    assert_eq!(
        rows.iter().find(|r| r.id == "s1").unwrap().archived,
        Some(true)
    );
    let commits = t.host.commits.borrow();
    assert_eq!(commits[0].tabs.len(), 1);
    assert_eq!(commits[0].active_tab_id, commits[0].tabs[0].id);
    let sessions = t.sessions(cx);
    assert!(
        sessions.read_with(cx, |s, _| s.loaded_cache().contains("s1")
            && !s.is_removing("s1"))
    );
}

#[gpui::test]
fn deletes_a_stored_session_and_drops_its_row(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("s1"));
    t.backend.insert_session(&chat("s2"));
    t.show(cx, CWD);
    assert_eq!(t.rows(cx).len(), 2);
    t.backend.clear_calls();
    let done = t
        .history
        .update(cx, |history, cx| history.delete_session("s1", cx));
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(done), Some(true));
    assert_eq!(t.backend.commands()[0], "session_delete");
    assert!(
        t.backend
            .commands()
            .contains(&"session_list_by_project".to_string())
    );
    assert_eq!(ids(&t.rows(cx)), vec!["s2"]);
}

#[gpui::test]
fn reports_a_failed_delete_and_releases_the_session(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, chat("s1"));
    t.backend.set_failing("session_delete", true);
    let done = t
        .history
        .update(cx, |history, cx| history.delete_session("s1", cx));
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(done), Some(false));
    assert!(t.host.alerts.borrow()[0].starts_with("Could not delete this conversation."));
    let sessions = t.sessions(cx);
    assert!(!sessions.read_with(cx, |s, _| s.is_removing("s1")));
    assert_eq!(t.open_ids(cx), vec!["s1"]);
}

#[gpui::test]
fn ignores_a_session_already_being_removed(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, chat("s1"));
    let sessions = t.sessions(cx);
    sessions.update(cx, |s, _| s.begin_removal("s1"));
    let done = t
        .history
        .update(cx, |history, cx| history.delete_session("s1", cx));
    assert_eq!(futures::FutureExt::now_or_never(done), Some(false));
}

#[gpui::test]
fn deletes_several_sessions_after_one_confirmation(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, chat("s1"));
    t.open(cx, chat("s2"));
    t.host.confirm_answer.set(false);
    t.history
        .update(cx, |history, cx| {
            history.delete_sessions(vec!["s1".into(), "s2".into()], cx)
        })
        .detach();
    cx.run_until_parked();
    assert_eq!(
        *t.host.confirms.borrow(),
        vec!["Delete 2 selected conversations? This can’t be undone.".to_string()]
    );
    assert_eq!(t.open_ids(cx).len(), 2);
    t.host.confirm_answer.set(true);
    t.history
        .update(cx, |history, cx| {
            history.delete_sessions(vec!["s1".into(), "s2".into()], cx)
        })
        .detach();
    cx.run_until_parked();
    assert!(t.open_ids(cx).is_empty());
    assert_eq!(t.backend.calls("session_delete").len(), 2);
}

// Opening and navigation.

#[gpui::test]
fn opens_the_lead_of_an_orchestration_worker(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("lead"));
    t.backend.insert_session(&chat("worker"));
    t.host
        .leads
        .borrow_mut()
        .insert("worker".into(), "lead".into());
    t.history
        .update(cx, |history, cx| history.select_session("worker", cx))
        .detach();
    cx.run_until_parked();
    assert_eq!(*t.host.inspected.borrow(), vec!["worker".to_string()]);
    assert_eq!(*t.host.opened.borrow(), vec!["lead".to_string()]);
}

#[gpui::test]
fn steps_through_the_sidebar_order_in_the_current_tab(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, chat("a"));
    t.backend.insert_session(&chat("b"));
    t.backend.insert_session(&chat("c"));
    *t.host.active.borrow_mut() = Some(ActivePane {
        tab_id: "tab".into(),
        focused_id: "a".into(),
        diff_focused: false,
    });
    t.history.update(cx, |history, cx| {
        history.set_navigation_order(vec!["a".into(), "b".into(), "c".into()]);
        history.navigate_session_list(1, true, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        *t.host.switched.borrow(),
        vec![("tab".to_string(), "a".to_string(), "b".to_string())]
    );
    // The next step ahead was prefetched into the cache.
    let sessions = t.sessions(cx);
    assert!(sessions.read_with(cx, |s, _| s.loaded_cache().contains("c")));
}

#[gpui::test]
fn archives_the_focused_session_from_the_shortcut(cx: &mut TestAppContext) {
    use crate::history::archive_shortcut::{ArchiveContext, ArchiveKeyEvent, ArchiveTab};
    let t = setup(cx);
    t.open(cx, chat("s1"));
    let context = ArchiveContext {
        active_tab_id: "tab".into(),
        tabs: vec![ArchiveTab {
            id: "tab".into(),
            focused_id: "s1".into(),
            diff_focused: false,
        }],
        session_ids: vec!["s1".into()],
        ..Default::default()
    };
    let handled = t.history.update(cx, |history, cx| {
        history.archive_focused_session(&ArchiveKeyEvent::default(), &context, cx)
    });
    assert!(handled);
    cx.run_until_parked();
    assert!(t.open_ids(cx).is_empty());
    assert!(
        t.backend
            .commands()
            .contains(&"session_set_archived".to_string())
    );
}

// The sidebar list.

struct Live {
    busy: HashSet<String>,
    approval: HashSet<String>,
    unseen: HashSet<String>,
    reminders: Vec<ReminderDue>,
}

fn live() -> Live {
    Live {
        busy: ["session-1".to_string()].into_iter().collect(),
        approval: HashSet::new(),
        unseen: HashSet::new(),
        reminders: Vec::new(),
    }
}

fn input<'a>(
    rows: &'a [SessionSummary],
    live: &'a Live,
    active: Option<&'a str>,
) -> SessionListInput<'a> {
    SessionListInput {
        project_sessions: rows,
        open_sessions: &[],
        busy_ids: &live.busy,
        approval_ids: &live.approval,
        unseen_finished_ids: &live.unseen,
        reminders: &live.reminders,
        active_listed_session_id: active,
        active_session_id: active,
        remote: false,
        listing_pending: false,
        listing_failed: false,
        remote_loaded: true,
    }
}

fn render(
    t: &T,
    cx: &mut TestAppContext,
    rows: &[SessionSummary],
    live: &Live,
    active: Option<&str>,
) -> SessionListView {
    t.history.update(cx, |history, cx| {
        let input = input(rows, live, active);
        let view = history.session_list(&input);
        history.sync_session_list(&view, &input, cx);
        view
    })
}

fn numbered(count: usize) -> Vec<SessionSummary> {
    (1..=count)
        .map(|n| row(&format!("session-{n}"), 100 - n as i64))
        .collect()
}

fn selected(t: &T, cx: &mut TestAppContext) -> Vec<String> {
    t.history
        .read_with(cx, |history, _| history.sidebar().selected.clone())
}

fn click(
    t: &T,
    cx: &mut TestAppContext,
    view: &SessionListView,
    id: &str,
    click: CardClick,
    active: Option<&str>,
) -> Option<String> {
    let mounted = view.mounted_navigation_ids.clone();
    t.history.update(cx, |history, cx| {
        history.select_card(id, click, &mounted, active, cx)
    })
}

fn sidebar(cx: &mut TestAppContext) -> T {
    let t = setup(cx);
    t.show(cx, CWD);
    t.history
        .update(cx, |history, cx| history.set_sessions_tab_active(true, cx));
    t
}

const CTRL: CardClick = CardClick {
    shift: false,
    toggle: true,
};
const SHIFT: CardClick = CardClick {
    shift: true,
    toggle: false,
};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[gpui::test]
fn resets_the_toggle_anchor_after_acting_on_another_sessions_context_menu(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let rows = numbered(4);
    let live = live();
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    assert_eq!(
        click(&t, cx, &view, "session-3", CTRL, Some("session-1")),
        None
    );
    t.history
        .update(cx, |history, cx| history.open_session_menu("session-2", cx));
    let state = t
        .history
        .read_with(cx, |history, _| history.session_menu_state(&view.listed));
    assert_eq!(state.session_ids, strings(&["session-2"]));
    t.history.update(cx, |history, cx| {
        history.pick_session_menu(SessionMenuAction::TogglePin, &view.listed, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        t.backend.calls("session_set_pinned")[0]["sessionId"],
        "session-2"
    );
    assert!(selected(&t, cx).is_empty());
    assert_eq!(
        click(&t, cx, &view, "session-4", SHIFT, Some("session-1")),
        None
    );
    assert_eq!(
        selected(&t, cx),
        strings(&["session-1", "session-2", "session-3", "session-4"])
    );
}

#[gpui::test]
fn starts_a_range_at_the_active_session_when_pagination_hides_the_anchor(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let rows = numbered(80);
    let live = live();
    let view = render(&t, cx, &rows, &live, Some("session-80"));
    assert_eq!(
        click(
            &t,
            cx,
            &view,
            "session-40",
            CardClick::default(),
            Some("session-80")
        ),
        Some("session-40".into())
    );
    t.history.update(cx, |history, cx| {
        history.set_search_query("Original conversation", cx)
    });
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    assert!(
        !view
            .mounted_navigation_ids
            .contains(&"session-40".to_string())
    );
    assert_eq!(view.mounted_navigation_ids.len(), 32);
    assert_eq!(
        click(&t, cx, &view, "session-3", SHIFT, Some("session-1")),
        None
    );
    assert_eq!(
        selected(&t, cx),
        strings(&["session-1", "session-2", "session-3"])
    );
}

#[gpui::test]
fn starts_the_next_range_at_the_active_session_after_the_toggle_clears_the_last_selection(
    cx: &mut TestAppContext,
) {
    let t = sidebar(cx);
    let rows = numbered(4);
    let live = live();
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    click(&t, cx, &view, "session-3", CTRL, Some("session-1"));
    assert_eq!(selected(&t, cx).len(), 1);
    click(&t, cx, &view, "session-3", CTRL, Some("session-1"));
    assert!(selected(&t, cx).is_empty());
    assert_eq!(
        click(&t, cx, &view, "session-4", SHIFT, Some("session-1")),
        None
    );
    assert_eq!(
        selected(&t, cx),
        strings(&["session-1", "session-2", "session-3", "session-4"])
    );
}

#[gpui::test]
fn selects_the_visible_range_from_a_plain_click_with_shift_click(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let rows = numbered(4);
    let live = live();
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    assert_eq!(
        click(
            &t,
            cx,
            &view,
            "session-1",
            CardClick::default(),
            Some("session-1")
        ),
        Some("session-1".into())
    );
    click(&t, cx, &view, "session-3", SHIFT, Some("session-1"));
    assert_eq!(
        selected(&t, cx),
        strings(&["session-1", "session-2", "session-3"])
    );
}

#[gpui::test]
fn creates_a_folder_containing_the_toggled_sessions_without_opening_them(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let rows = numbered(3);
    let live = live();
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    for id in ["session-1", "session-3"] {
        assert_eq!(click(&t, cx, &view, id, CTRL, Some("session-1")), None);
    }
    assert_eq!(selected(&t, cx).len(), 2);
    t.history.update(cx, |history, cx| {
        history.open_session_menu("session-1", cx);
        history.pick_session_menu(SessionMenuAction::NewFolder, &view.listed, cx);
    });
    let folders = load_session_folders(&t.kv, CWD);
    assert_eq!(folders[0].session_ids, strings(&["session-1", "session-3"]));
    let renaming = t.history.read_with(cx, |history, _| {
        history.sidebar().renaming_folder_id.clone()
    });
    assert_eq!(renaming, Some(folders[0].id.clone()));
}

#[gpui::test]
fn builds_the_list_with_reminders_folders_pins_filters_and_paging(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let mut rows = numbered(40);
    rows[5].pinned = Some(true);
    rows[6].archived = Some(true);
    let mut live = live();
    live.reminders = vec![
        ReminderDue {
            session_id: "session-9".into(),
            due_at: 20,
        },
        ReminderDue {
            session_id: "session-8".into(),
            due_at: 10,
        },
    ];
    save_session_folders(
        &t.kv,
        CWD,
        &[crate::history::session_folders::SessionFolder::new(
            "f",
            "Work",
            strings(&["session-2", "session-3"]),
        )],
    );
    cx.run_until_parked();
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    let kinds: Vec<&str> = view
        .entries
        .iter()
        .take(3)
        .map(|entry| match entry {
            crate::history::session_folders::SessionListEntry::Reminders { .. } => "reminders",
            crate::history::session_folders::SessionListEntry::Folder { .. } => "folder",
            crate::history::session_folders::SessionListEntry::Pinned { .. } => "pinned",
            crate::history::session_folders::SessionListEntry::Session { .. } => "session",
        })
        .collect();
    assert_eq!(kinds, vec!["reminders", "folder", "pinned"]);
    assert_eq!(
        &view.navigation_ids[..2],
        &strings(&["session-8", "session-9"])[..]
    );
    assert!(!view.navigation_ids.contains(&"session-7".to_string()));
    assert_eq!(view.visible.len(), 39);
    assert!(view.has_more);
    assert_eq!(view.folder_ids, strings(&["f"]));
    assert_eq!(view.harnesses, vec![HarnessId::Codex]);

    let filters = SessionSidebarFilters {
        status: crate::history::session_filters::SessionStatusFilter {
            working: true,
            ..Default::default()
        },
        ..Default::default()
    };
    t.history
        .update(cx, |history, cx| history.set_filters(filters, cx));
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    assert_eq!(ids(&view.visible), vec!["session-1"]);
    assert!(view.filters_active && view.narrowed_by_user);
}

#[gpui::test]
fn prunes_folder_members_that_left_the_project_and_keeps_pending_new_ones(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let rows = numbered(2);
    let live = live();
    save_session_folders(
        &t.kv,
        CWD,
        &[crate::history::session_folders::SessionFolder::new(
            "f",
            "Work",
            strings(&["session-1", "gone"]),
        )],
    );
    cx.run_until_parked();
    t.history
        .update(cx, |history, cx| history.new_in_folder("f", "fresh", cx));
    render(&t, cx, &rows, &live, Some("session-1"));
    assert_eq!(
        load_session_folders(&t.kv, CWD)[0].session_ids,
        strings(&["session-1", "fresh"])
    );
}

#[gpui::test]
fn reloads_folders_saved_elsewhere_and_makes_folders_by_drop(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    t.history.update(cx, |history, cx| {
        history.drop_on_session_list("a", &SessionListDropTarget::Session { id: "b".into() }, cx)
    });
    let (folders, renaming) = t.history.read_with(cx, |history, _| {
        (
            history.sidebar().folders.clone(),
            history.sidebar().renaming_folder_id.clone(),
        )
    });
    assert_eq!(folders[0].session_ids, strings(&["a", "b"]));
    assert_eq!(renaming, Some(folders[0].id.clone()));

    crate::history::sidebar::place_session_in_project_folder(
        &t.kv,
        CWD,
        "c",
        &crate::history::session_folders::SessionFolderTarget::Existing {
            folder_id: folders[0].id.clone(),
        },
    );
    cx.run_until_parked();
    let folders = t
        .history
        .read_with(cx, |history, _| history.sidebar().folders.clone());
    assert_eq!(folders[0].session_ids, strings(&["a", "b", "c"]));
}

#[gpui::test]
fn patches_rows_and_follows_a_moved_project(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.history.update(cx, |history, cx| {
        history.set_boot_rows(
            vec![SessionSummary {
                linked_work_item: Some(link()),
                ..row("s1", 1)
            }],
            Some("/workspace/old"),
            cx,
        );
        history.patch_summaries(
            &|row| {
                (row.id == "s1").then(|| SessionSummary {
                    branch: Some("feature".into()),
                    ..row.clone()
                })
            },
            cx,
        );
        history.rebase_loaded_project("/workspace/old", CWD, cx);
    });
    t.history.read_with(cx, |history, _| {
        assert_eq!(history.rows()[0].branch.as_deref(), Some("feature"));
        assert_eq!(
            history.stored_linked_sessions()[0].branch.as_deref(),
            Some("feature")
        );
    });
    t.history
        .update(cx, |history, cx| history.set_sidebar_cwd(CWD, cx));
    assert!(!t.history.read_with(cx, |history, _| history.is_pending()));
}

#[gpui::test]
fn leaving_the_sessions_tab_clears_the_selection_and_search(cx: &mut TestAppContext) {
    let t = sidebar(cx);
    let rows = numbered(2);
    let live = live();
    let view = render(&t, cx, &rows, &live, Some("session-1"));
    click(&t, cx, &view, "session-2", CTRL, Some("session-1"));
    t.history.update(cx, |history, cx| {
        history.set_search_query("x", cx);
        history.set_sessions_tab_active(false, cx);
    });
    t.history.read_with(cx, |history, _| {
        assert!(history.sidebar().selected.is_empty());
        assert!(history.sidebar().search_query.is_empty());
    });
}

#[test]
fn releases_an_orchestration_worker_and_its_blocks() {
    let mut worker = chat("w");
    worker.orchestration_lead_id = Some("lead".into());
    worker.blocks[0].orchestration_lead_id = Some("lead".into());
    let released = release_orchestration_worker(&worker, "lead").unwrap();
    assert_eq!(released.orchestration_lead_id, None);
    assert_eq!(released.blocks[0].orchestration_lead_id, None);
    assert!(release_orchestration_worker(&chat("x"), "lead").is_none());
}

#[gpui::test]
async fn releasing_a_lead_invalidates_a_pending_worker_read(cx: &mut TestAppContext) {
    let t = setup(cx);
    let mut worker = chat("worker");
    worker.orchestration_lead_id = Some("lead".into());
    worker.blocks[0].orchestration_lead_id = Some("lead".into());
    t.backend.insert_session(&worker);
    let gate = t.backend.hold_next("session_get");
    let sessions = t.sessions(cx);
    let pending = sessions.update(cx, |sessions, cx| sessions.load_stored("worker", cx));
    cx.run_until_parked();

    t.history.update(cx, |history, cx| {
        history.apply_removal_change(
            "lead",
            None,
            WorkspaceChange::OrchestrationReleased {
                lead_id: "lead".into(),
            },
            cx,
        );
    });
    let released = release_orchestration_worker(&worker, "lead").unwrap();
    gate.release();
    assert!(
        pending.await.is_none(),
        "the pending read carries the deleted lead"
    );

    t.backend.insert_session(&released);
    let fresh = sessions.update(cx, |sessions, cx| sessions.ensure_open("worker", cx));
    let fresh = fresh.await.expect("the released worker can reopen");
    assert_eq!(fresh.orchestration_lead_id, None);
    assert_eq!(fresh.blocks[0].orchestration_lead_id, None);
    assert_eq!(fresh.blocks[0].text, worker.blocks[0].text);
    assert_eq!(t.backend.calls("session_get").len(), 2);
}
