//! Port of FilePane.agent.test.ts and FilePane.remote.test.ts, plus the
//! surface choice for each tab kind. The agent transcript itself, and its
//! "Waiting for orchestrator" approval state, belong to the transcript view
//! the factory returns, so those cases are tested there.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use monocode_core::HarnessId;
use monocode_layout::{
    AgentTabSource, CommitTabSource, GitFileDiffKind, new_agent_tab, new_changes_tab,
    new_editor_pane, new_file_tab, new_plan_tab, new_terminal_file,
};

use super::*;
use crate::test_support::FakeFiles;

struct Placeholder;

impl Render for Placeholder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

type Requests = Rc<RefCell<Vec<(ExternalSurface, String)>>>;

fn factory(requests: Requests) -> SurfaceFactory {
    Rc::new(
        move |request: &SurfaceRequest<'_>, _: &mut Window, cx: &mut App| {
            requests
                .borrow_mut()
                .push((request.kind, request.file.path.clone()));
            Some(cx.new(|_| Placeholder).into())
        },
    )
}

fn mount(
    pane: EditorPane,
    fs: Rc<FakeFiles>,
    cx: &mut TestAppContext,
) -> (
    Entity<FilePane>,
    Requests,
    Rc<RefCell<Vec<FilePaneEvent>>>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        crate::test_support::init(cx);
        monocode_markdown::init(cx);
    });
    let requests: Requests = Rc::default();
    let data: Rc<dyn FilesData> = fs;
    let surfaces = factory(requests.clone());
    let (view, cx) =
        cx.add_window_view(move |window, cx| FilePane::new(data, pane, Some(surfaces), window, cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FilePaneEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    cx.run_until_parked();
    (view, requests, events, cx)
}

fn agent_pane() -> EditorPane {
    new_editor_pane(new_agent_tab(
        "Audit the engine",
        "/repo",
        AgentTabSource::new("worker", "lead", HarnessId::Codex),
    ))
}

#[gpui::test]
fn shows_a_worker_transcript_from_the_factory_and_keeps_it(cx: &mut TestAppContext) {
    let (pane, requests, _, cx) = mount(agent_pane(), FakeFiles::new(), cx);
    assert_eq!(
        requests.borrow().as_slice(),
        [(ExternalSurface::Agent, "Audit the engine".to_string())]
    );
    let file_id = pane.read_with(cx, |pane, _| pane.pane().active_file_id.clone());
    assert!(pane.read_with(cx, |pane, _| matches!(
        pane.surface(&file_id),
        Some(Surface::External(_))
    )));
    // New session data reaches the transcript through its own entity; the
    // pane does not build the view again.
    pane.update_in(cx, |pane, window, cx| {
        pane.set_sessions(Rc::new(Vec::new()), window, cx)
    });
    assert_eq!(requests.borrow().len(), 1);
}

#[gpui::test]
fn omits_the_tab_strip_when_the_title_bar_names_the_file(cx: &mut TestAppContext) {
    let (pane, _, _, cx) = mount(agent_pane(), FakeFiles::new(), cx);
    let strip: TabStrip = Rc::new(|_, _, _| div().into_any_element());
    pane.update(cx, |pane, cx| pane.set_tab_strip(Some(strip), cx));
    assert!(pane.read_with(cx, |pane, _| pane.show_tabs()));
    pane.update(cx, |pane, cx| pane.set_show_tabs(false, cx));
    assert!(!pane.read_with(cx, |pane, _| pane.show_tabs()));
}

#[gpui::test]
fn renders_a_remote_path_in_the_shared_editor(cx: &mut TestAppContext) {
    let fs = FakeFiles::new();
    fs.set_file("remote://env/repo/src/index.ts", "export {};\n");
    let file = new_file_tab(
        "remote://env/repo/src/index.ts",
        "remote://env/repo",
        false,
        None,
        None,
    );
    let id = file.id.clone();
    let (pane, requests, _, cx) = mount(new_editor_pane(file), fs, cx);
    let editor = pane.read_with(cx, |pane, _| match pane.surface(&id) {
        Some(Surface::Editor(editor)) => Some(editor.clone()),
        _ => None,
    });
    let editor = editor.expect("an editor surface");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.load_state().clone()),
        crate::file_editor::LoadState::Ready
    );
    assert!(requests.borrow().is_empty());
}

#[gpui::test]
fn picks_a_surface_for_each_tab_kind(cx: &mut TestAppContext) {
    let image = new_file_tab("/repo/logo.png", "/repo", false, None, None);
    let terminal = new_terminal_file("/repo", None, None);
    let plan = new_plan_tab("session", "block", "Plan", "/repo");
    let mut pane = new_editor_pane(image.clone());
    pane.files.push(terminal.clone());
    pane.files.push(plan.clone());
    let (view, requests, _, cx) = mount(pane, FakeFiles::new(), cx);
    view.read_with(cx, |view, _| {
        assert!(matches!(view.surface(&image.id), Some(Surface::Binary(_))));
        assert!(matches!(
            view.surface(&terminal.id),
            Some(Surface::External(_))
        ));
        assert!(matches!(view.surface(&plan.id), Some(Surface::Plan(_))));
    });
    assert_eq!(requests.borrow()[0].0, ExternalSurface::Terminal);

    // Closing a tab drops its surface.
    let mut next = view.read_with(cx, |view, _| view.pane().clone());
    next.files.retain(|file| file.id != terminal.id);
    view.update_in(cx, |view, window, cx| view.set_pane(next, window, cx));
    assert!(view.read_with(cx, |view, _| view.surface(&terminal.id).is_none()));
}

#[gpui::test]
fn draws_review_tabs_with_the_pane_wide_diffs(cx: &mut TestAppContext) {
    let fs = FakeFiles::new();
    fs.set_file("/repo/a.ts", "a\n");
    let review = new_file_tab(
        "/repo/a.ts",
        "/repo",
        true,
        Some(GitFileDiffKind::Unstaged),
        None,
    );
    let changes = new_changes_tab("/repo", None, None, None);
    let mut pane = new_editor_pane(review.clone());
    pane.files.push(changes.clone());
    pane.active_file_id = changes.id.clone();
    let (view, requests, _, cx) = mount(pane, fs, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.review()),
        Some(ExternalSurface::WorkingTreeDiff)
    );
    // While the unified diff shows, review tabs have no surface of their own.
    assert!(view.read_with(cx, |view, _| view.surface(&review.id).is_none()));

    // With the editor diff viewer, the active review tab gets its editor.
    let mut next = view.read_with(cx, |view, _| view.pane().clone());
    next.active_file_id = review.id.clone();
    view.update_in(cx, |view, window, cx| view.set_pane(next, window, cx));
    view.read_with(cx, |view, _| {
        assert_eq!(view.review(), None);
        assert!(matches!(view.surface(&review.id), Some(Surface::Editor(_))));
    });

    view.update_in(cx, |view, window, cx| {
        view.set_unified_diffs(true, window, cx)
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.review(), Some(ExternalSurface::WorkingTreeDiff));
        assert!(view.surface(&review.id).is_none());
    });
    assert!(
        requests
            .borrow()
            .iter()
            .all(|(kind, _)| *kind == ExternalSurface::WorkingTreeDiff)
    );

    let mut commit = new_file_tab("/repo", "/repo", false, None, None);
    commit.commit = Some(CommitTabSource::new("abc123", "abc", "Fix"));
    let mut next = view.read_with(cx, |view, _| view.pane().clone());
    next.files.push(commit.clone());
    next.active_file_id = commit.id.clone();
    view.update_in(cx, |view, window, cx| view.set_pane(next, window, cx));
    assert_eq!(
        view.read_with(cx, |view, _| view.review()),
        Some(ExternalSurface::Commit)
    );
}

#[gpui::test]
fn reports_focus_and_dirty_changes(cx: &mut TestAppContext) {
    let fs = FakeFiles::new();
    fs.set_file("/repo/a.ts", "a\n");
    let file = new_file_tab("/repo/a.ts", "/repo", false, None, None);
    let id = file.id.clone();
    let pane = new_editor_pane(file);
    let pane_id = pane.id.clone();
    let (view, _, events, cx) = mount(pane, fs, cx);
    view.update_in(cx, |view, window, cx| view.set_focused(true, window, cx));
    cx.run_until_parked();
    cx.simulate_click(point(px(40.), px(40.)), Modifiers::none());
    assert!(events.borrow().contains(&FilePaneEvent::Focus { pane_id }));

    let editor = view.read_with(cx, |view, _| match view.surface(&id) {
        Some(Surface::Editor(editor)) => editor.clone(),
        _ => panic!("editor surface"),
    });
    let code = editor
        .read_with(cx, |editor, _| editor.editor().cloned())
        .unwrap();
    let state = code.read_with(cx, |code, _| code.editor_state().clone());
    state.update_in(cx, |state, window, cx| {
        state.set_selected_range(0..0, cx);
        state.replace("x", window, cx);
    });
    cx.run_until_parked();
    assert!(events.borrow().contains(&FilePaneEvent::DirtyChanged {
        file_id: id,
        dirty: true
    }));
}

#[test]
fn chooses_surfaces_by_the_react_branch_order() {
    let file = new_file_tab("/repo/README.md", "/repo", false, None, None);
    assert_eq!(tab_surface(&file, false), TabSurface::Editor);
    let pdf = new_file_tab("/repo/doc.PDF", "/repo", false, None, None);
    assert_eq!(tab_surface(&pdf, false), TabSurface::Binary);
    let review = new_file_tab("/repo/a.ts", "/repo", true, None, None);
    assert_eq!(tab_surface(&review, false), TabSurface::Editor);
    assert_eq!(tab_surface(&review, true), TabSurface::None);
    assert_eq!(
        review_surface(Some(&review), true),
        Some(ExternalSurface::WorkingTreeDiff)
    );
    assert_eq!(review_surface(Some(&review), false), None);
    assert_eq!(review_surface(None, true), None);
}
