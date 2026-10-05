//! View tests for the search page over [`LocalSearch`]: the empty state,
//! scopes, keyboard highlight, opening hits, and Escape. The search model
//! (debounce, cancellation, grouping) is the engine's `Search` entity.

use std::rc::Rc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use monocode_core::HarnessId;

use super::{
    AppSearchHit, ContentHit, ConversationHit, FileHit, LocalSearch, ProjectHit, SearchScope,
    SearchView,
};
use crate::data::StaticProjects;
use crate::test_support::{Calls, click, exists, keys, mount, type_text};

fn hits() -> Vec<AppSearchHit> {
    vec![
        AppSearchHit::Conversation(ConversationHit {
            id: "conversation:s1".into(),
            session_id: "s1".into(),
            cwd: "/work/app".into(),
            harness: HarnessId::Claude,
            title: "Plan the release".into(),
            updated_at: 1,
            score: 10,
            positions: vec![0, 1, 2, 3],
        }),
        AppSearchHit::File(FileHit {
            id: "file:/work/app/plan.md".into(),
            path: "/work/app/plan.md".into(),
            relative: "plan.md".into(),
            name: "plan.md".into(),
            score: 5,
            positions: vec![0, 1, 2, 3],
        }),
        AppSearchHit::Content(ContentHit {
            id: "content:/work/app/main.rs:3:1".into(),
            path: "/work/app/main.rs".into(),
            relative: "main.rs".into(),
            name: "main.rs".into(),
            line: 3,
            column: 1,
            preview: "let plan = 1;".into(),
        }),
        AppSearchHit::Project(ProjectHit {
            id: "project:/work/planet".into(),
            path: "/work/planet".into(),
            name: "planet".into(),
            score: 4,
            positions: vec![0, 1, 2, 3],
        }),
    ]
}

struct Page<'a> {
    view: Entity<SearchView>,
    data: LocalSearch,
    closes: Calls<()>,
    cx: &'a mut VisualTestContext,
}

fn render(cx: &mut TestAppContext) -> Page<'_> {
    let closes = Calls::<()>::new();
    let record = closes.recorder();
    let slot: Rc<std::cell::RefCell<Option<LocalSearch>>> = Rc::default();
    let built = slot.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        let data = LocalSearch::new(hits(), cx);
        *built.borrow_mut() = Some(data.clone());
        let projects = Rc::new(StaticProjects::default());
        let view = cx.new(|cx| {
            SearchView::new(Rc::new(data), projects, window, cx).on_close(move |_, _| record(()))
        });
        view.update(cx, |view, cx| {
            view.open("/work/app", Vec::new(), window, cx)
        });
        view
    });
    let data = slot.borrow().clone().unwrap();
    Page {
        view,
        data,
        closes,
        cx,
    }
}

fn state(page: &mut Page) -> super::SearchState {
    page.view.read_with(page.cx, |view, _| view.state().clone())
}

#[gpui::test]
fn shows_the_empty_state_until_a_query_arrives(cx: &mut TestAppContext) {
    let mut page = render(cx);
    assert!(exists(page.cx, "search-empty"));
    type_text(page.cx, "plan");
    assert!(!exists(page.cx, "search-empty"));
    assert_eq!(state(&mut page).hits.len(), 4);
    assert!(exists(page.cx, "search-hit conversation:s1"));
}

#[gpui::test]
fn scope_tabs_filter_the_results(cx: &mut TestAppContext) {
    let mut page = render(cx);
    type_text(page.cx, "plan");
    click(page.cx, "search-scope Files");
    let state = state(&mut page);
    assert_eq!(state.scope, SearchScope::Files);
    assert_eq!(
        state
            .hits
            .iter()
            .map(|hit| hit.id().to_string())
            .collect::<Vec<_>>(),
        vec!["file:/work/app/plan.md", "content:/work/app/main.rs:3:1"]
    );
    click(page.cx, "search-scope Projects");
    assert_eq!(
        page.view
            .read_with(page.cx, |view, _| view.state().hits.len()),
        1
    );
}

#[gpui::test]
fn arrows_move_the_highlight_and_enter_opens_it(cx: &mut TestAppContext) {
    let mut page = render(cx);
    type_text(page.cx, "plan");
    assert_eq!(state(&mut page).active, 0);
    keys(page.cx, "down down");
    assert_eq!(state(&mut page).active, 2);
    keys(page.cx, "up");
    assert_eq!(state(&mut page).active, 1);
    keys(page.cx, "up up");
    assert_eq!(state(&mut page).active, 3);
    keys(page.cx, "enter");
    let opened = page.cx.update(|_, cx| page.data.opened(cx));
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].id(), "project:/work/planet");
    assert_eq!(page.closes.len(), 1);
    assert!(!page.cx.update(|_, cx| page.data.is_open(cx)));
}

#[gpui::test]
fn clicking_a_hit_opens_it(cx: &mut TestAppContext) {
    let page = render(cx);
    type_text(page.cx, "plan");
    click(page.cx, "search-hit file:/work/app/plan.md");
    let opened = page.cx.update(|_, cx| page.data.opened(cx));
    assert_eq!(opened[0].id(), "file:/work/app/plan.md");
    assert_eq!(page.closes.len(), 1);
}

#[gpui::test]
fn escape_closes_the_page(cx: &mut TestAppContext) {
    let page = render(cx);
    keys(page.cx, "escape");
    assert_eq!(page.closes.len(), 1);
}

#[gpui::test]
fn no_results_and_the_limit_notice(cx: &mut TestAppContext) {
    let mut page = render(cx);
    type_text(page.cx, "zzz");
    assert!(state(&mut page).hits.is_empty());
    let data = page.data.clone();
    page.cx.update(|_, cx| {
        data.state_entity().update(cx, |state, cx| {
            state.truncated = true;
            cx.notify();
        })
    });
    crate::test_support::draw(page.cx);
    assert!(state(&mut page).truncated);
    assert!(exists(page.cx, "search-results"));
}
