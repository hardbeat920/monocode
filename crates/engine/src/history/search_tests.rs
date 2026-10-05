//! Port of src/features/search/model/search.test.ts, plus the search page
//! effects from SearchView.tsx: the debounce, cancellation, scopes, and
//! result merging.

use std::cell::RefCell;

use futures::channel::oneshot;
use gpui::{AppContext, TestAppContext};
use monocode_core::block::{Block, BlockRole};
use monocode_core::{HarnessId, Session};
use monocode_git::search::SearchMatch;
use monocode_settings::Kv;
use parking_lot::Mutex;

use super::*;
use crate::runtime::testing::{FakeBackend, init_test_engine};

const CWD: &str = "/repo";

#[derive(Default)]
struct FakeProjectSearch {
    searches: Mutex<Vec<(String, String, String)>>,
    cancels: Mutex<Vec<(String, String)>>,
    hold: Mutex<Option<oneshot::Receiver<()>>>,
    failure: Mutex<Option<String>>,
}

impl ProjectSearchBackend for FakeProjectSearch {
    fn search(&self, options: SearchOptions) -> StoreFuture<SearchResult> {
        self.searches.lock().push((
            options.cwd.clone(),
            options.query.clone(),
            options.search_id.clone(),
        ));
        let hold = self.hold.lock().take();
        let failure = self.failure.lock().clone();
        async move {
            if let Some(hold) = hold {
                let _ = hold.await;
            }
            if let Some(failure) = failure {
                return Err(failure);
            }
            Ok(SearchResult {
                matches: vec![SearchMatch {
                    path: format!("{CWD}/src/App.tsx"),
                    relative: "src/App.tsx".into(),
                    line: 3,
                    column: 1,
                    preview: format!("const {} = 1;", options.query),
                }],
                truncated: false,
            })
        }
        .boxed()
    }

    fn cancel(&self, cwd: String, search_id: String) {
        self.cancels.lock().push((cwd, search_id));
    }
}

struct Files;

impl SearchFiles for Files {
    fn rank(&self, cwd: &str, query: &str, _limit: usize, _cx: &App) -> Vec<FileRank> {
        vec![FileRank {
            path: format!("{cwd}/{query}.rs"),
            relative: format!("{query}.rs"),
            name: format!("{query}.rs"),
            score: 50,
            positions: Vec::new(),
        }]
    }
}

struct T {
    backend: Arc<FakeBackend>,
    project: Arc<FakeProjectSearch>,
    search: Entity<Search>,
}

fn setup(cx: &mut TestAppContext) -> T {
    let backend = init_test_engine(cx);
    let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
    let project = Arc::new(FakeProjectSearch::default());
    let backend_project: Arc<dyn ProjectSearchBackend> = project.clone();
    let search = cx.new(|cx| Search::new(history, backend_project, cx));
    T {
        backend,
        project,
        search,
    }
}

impl T {
    fn open(&self, cx: &mut TestAppContext) {
        self.search.update(cx, |search, cx| {
            search.set_files(Rc::new(Files));
            search.open(CWD, vec!["/Users/me/code/needle-app".into()], cx);
        });
    }

    fn type_query(&self, cx: &mut TestAppContext, query: &str) {
        self.search
            .update(cx, |search, cx| search.set_query(query, cx));
    }

    fn settle(&self, cx: &mut TestAppContext) {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        cx.run_until_parked();
    }

    fn hits(&self, cx: &mut TestAppContext) -> Vec<String> {
        self.search
            .read_with(cx, |search, cx| search.hits(cx))
            .iter()
            .map(|hit| hit.id().to_string())
            .collect()
    }
}

#[gpui::test]
fn passes_the_owner_id_with_project_search_and_cancel_commands(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx);
    let (release, hold) = oneshot::channel::<()>();
    *t.project.hold.lock() = Some(hold);
    t.type_query(cx, "needle");
    t.settle(cx);
    let (cwd, query, search_id) = t.project.searches.lock()[0].clone();
    assert_eq!((cwd.as_str(), query.as_str()), (CWD, "needle"));
    assert!(t.search.read_with(cx, |search, _| search.is_loading()));
    t.type_query(cx, "other");
    assert_eq!(
        *t.project.cancels.lock(),
        vec![(CWD.to_string(), search_id)]
    );
    drop(release);
}

#[gpui::test]
fn passes_the_owner_id_with_session_search_and_cancel_commands(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx);
    let gate = t.backend.hold_next("session_search");
    t.type_query(cx, "needle");
    t.settle(cx);
    assert_eq!(t.backend.calls("session_search")[0]["query"], "needle");
    t.search.update(cx, |search, cx| search.close(cx));
    cx.run_until_parked();
    let cancels = t.backend.calls("cancel_session_search");
    assert_eq!(cancels.len(), 1);
    assert!(!cancels[0]["searchOwner"].as_str().unwrap().is_empty());
    assert!(!t.search.read_with(cx, |search, _| search.is_loading()));
    gate.release();
}

#[test]
fn does_not_send_remote_cancellation_to_this_computer() {
    let project = FakeProjectSearch::default();
    cancel_project_search(&project, "remote://env/home/me/repo", "remote-search");
    assert!(project.cancels.lock().is_empty());
    cancel_project_search(&project, CWD, "local");
    assert_eq!(project.cancels.lock().len(), 1);
}

#[gpui::test]
fn waits_for_the_query_to_settle(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx);
    t.type_query(cx, "nee");
    cx.executor().advance_clock(SEARCH_DEBOUNCE / 2);
    t.type_query(cx, "needle");
    cx.executor().advance_clock(SEARCH_DEBOUNCE / 2);
    cx.run_until_parked();
    assert!(t.backend.calls("session_search").is_empty());
    t.settle(cx);
    assert_eq!(t.backend.calls("session_search").len(), 1);
    assert_eq!(t.project.searches.lock().len(), 1);
}

#[gpui::test]
fn merges_titles_transcripts_files_contents_and_projects(cx: &mut TestAppContext) {
    let t = setup(cx);
    let mut session = Session::blank("s1", HarnessId::Cursor, "cursor:auto", CWD);
    session.title = "cursor · Find the needle".into();
    session.blocks = vec![Block::new("u1", BlockRole::User, "where is the needle?")];
    let sessions = cx.update(|cx| Engine::sessions(cx));
    sessions.update(cx, |sessions, cx| {
        sessions.insert(session, cx);
    });
    t.open(cx);
    t.type_query(cx, "needle");
    t.settle(cx);
    assert_eq!(
        t.hits(cx),
        vec![
            "conversation:s1",
            "message:s1:u1",
            "file:/repo/needle.rs",
            "content:/repo/src/App.tsx:3:1",
            "project:/Users/me/code/needle-app",
        ]
    );
    t.search
        .update(cx, |search, cx| search.set_scope(SearchScope::Projects, cx));
    assert_eq!(t.hits(cx), vec!["project:/Users/me/code/needle-app"]);
}

#[gpui::test]
fn searches_only_what_the_scope_shows(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx);
    t.search
        .update(cx, |search, cx| search.set_scope(SearchScope::Files, cx));
    t.type_query(cx, "needle");
    t.settle(cx);
    assert!(t.backend.calls("session_search").is_empty());
    assert_eq!(t.project.searches.lock().len(), 1);
    t.search.update(cx, |search, cx| {
        search.set_scope(SearchScope::Conversations, cx)
    });
    t.settle(cx);
    assert_eq!(t.backend.calls("session_search").len(), 1);
    assert_eq!(t.project.searches.lock().len(), 1);
}

#[gpui::test]
fn shows_a_project_search_error_and_clears_its_hits(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx);
    *t.project.failure.lock() = Some("/repo: Not a directory".into());
    t.type_query(cx, "needle");
    t.settle(cx);
    t.search.read_with(cx, |search, _| {
        assert_eq!(search.error(), Some("/repo: Not a directory"));
        assert!(!search.is_loading());
    });
    assert!(!t.hits(cx).iter().any(|id| id.starts_with("content:")));
}

#[gpui::test]
fn skips_files_for_a_remote_project(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.search.update(cx, |search, cx| {
        search.set_files(Rc::new(Files));
        search.open("remote://env/home/me/repo", Vec::new(), cx);
        search.set_query("needle", cx);
    });
    t.settle(cx);
    assert!(t.project.searches.lock().is_empty());
    assert!(!t.hits(cx).iter().any(|id| id.starts_with("file:")));
}

#[gpui::test]
fn moves_the_highlight_and_wraps(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx);
    t.type_query(cx, "needle");
    t.settle(cx);
    let count = t.hits(cx).len();
    assert!(count > 1);
    t.search.update(cx, |search, cx| search.move_active(-1, cx));
    assert_eq!(
        t.search.read_with(cx, |search, _| search.active_index()),
        count - 1
    );
    t.search.update(cx, |search, cx| search.move_active(1, cx));
    assert_eq!(t.search.read_with(cx, |search, _| search.active_index()), 0);
    let first = RefCell::new(None);
    t.search
        .read_with(cx, |search, cx| *first.borrow_mut() = search.active_hit(cx));
    assert_eq!(
        first.into_inner().map(|hit| hit.id().to_string()),
        Some(t.hits(cx)[0].clone())
    );
}

#[test]
fn normalizes_and_compares_editor_paths() {
    assert_eq!(normalize_editor_path("C:\\repo\\src\\"), "C:/repo/src");
    assert_eq!(normalize_editor_path("/"), "/");
    assert!(editor_paths_equal("/repo/a.ts", "/repo/a.ts"));
}
