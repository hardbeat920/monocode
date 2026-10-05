//! Ports of SurfaceTabs.test.ts, SurfaceTabsMenu.test.ts, and the file and
//! terminal cases of TabMiddleClick.test.ts and TabCloseMotion.test.ts. The
//! workspace tab cases belong to the title bar.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px,
};
use monocode_layout::{
    CommitTabSource, ReleaseNotesTabSource, new_changes_tab, new_commit_tab, new_file_tab,
    new_release_notes_workspace_tab, new_session_changes_tab, new_terminal_file,
};

use super::*;
use crate::panes::test_support::{draw, init};

#[test]
fn labels_release_notes_from_their_version() {
    let tab = new_release_notes_workspace_tab(ReleaseNotesTabSource::new("0.1.23"));
    let file = &tab.editor_panes[0].files[0];
    let title = release_notes_title("0.1.23");
    assert_eq!(
        surface_tab_presentation(file),
        SurfaceTabPresentation {
            name: title.clone(),
            label: title.clone(),
            icon_name: "CHANGELOG.md".into(),
            tooltip: title,
        }
    );
}

#[test]
fn labels_the_unified_working_tree_tab_as_changes() {
    assert_eq!(
        surface_tab_presentation(&new_changes_tab("/repo", Some("/repo/App.tsx"), None, None)),
        SurfaceTabPresentation {
            name: "Changes".into(),
            label: "Changes".into(),
            icon_name: "CHANGES".into(),
            tooltip: "Working tree changes".into(),
        }
    );
}

#[test]
fn labels_a_changes_tab_opened_from_staged_changes() {
    let staged = new_changes_tab("/repo", None, Some(GitFileDiffKind::Staged), None);
    assert_eq!(
        surface_tab_presentation(&staged),
        SurfaceTabPresentation {
            name: "Staged Changes".into(),
            label: "Staged Changes".into(),
            icon_name: "CHANGES".into(),
            tooltip: "Staged changes".into(),
        }
    );
    let unstaged = new_changes_tab("/repo", None, Some(GitFileDiffKind::Unstaged), None);
    assert_eq!(surface_tab_presentation(&unstaged).label, "Changes");
}

#[test]
fn labels_a_session_scoped_review_distinctly() {
    assert_eq!(
        surface_tab_presentation(&new_session_changes_tab(
            "/repo",
            "session-a",
            Some("/repo/App.tsx"),
            None
        )),
        SurfaceTabPresentation {
            name: "Session Changes".into(),
            label: "Session Changes".into(),
            icon_name: "CHANGES".into(),
            tooltip: "Changes captured for this session only".into(),
        }
    );
}

fn commit() -> CommitTabSource {
    CommitTabSource::new("abc1234deadbeef", "abc1234", "Fix the graph")
}

#[test]
fn labels_a_commit_tab_from_the_subject() {
    assert_eq!(
        surface_tab_presentation(&new_commit_tab("/repo", commit(), None)),
        SurfaceTabPresentation {
            name: "Fix the graph".into(),
            label: "Fix the graph".into(),
            icon_name: "CHANGES".into(),
            tooltip: "abc1234 — Fix the graph".into(),
        }
    );
}

#[test]
fn append_problems_leaves_a_clean_tooltip_and_counts_the_rest() {
    assert_eq!(append_problems("/repo/src/app.ts", 0), "/repo/src/app.ts");
    assert_eq!(
        append_problems("/repo/src/app.ts", 1),
        "/repo/src/app.ts — 1 problem"
    );
    assert_eq!(
        append_problems("/repo/src/app.ts", 4),
        "/repo/src/app.ts — 4 problems"
    );
}

fn labels(items: &[SurfaceTabMenuItem]) -> Vec<&'static str> {
    items
        .iter()
        .filter_map(|item| match item {
            SurfaceTabMenuItem::Item { label, .. } => Some(*label),
            SurfaceTabMenuItem::Separator => None,
        })
        .collect()
}

#[test]
fn offers_filesystem_actions_for_regular_and_review_file_tabs() {
    for review in [false, true] {
        let items = surface_tab_menu_items(
            &new_file_tab("/repo/src/app.ts", "/repo", review, None, None),
            true,
        );
        assert_eq!(
            labels(&items),
            [
                "Open in Default App",
                REVEAL_LABEL,
                "Copy Path",
                "Copy Relative Path",
                "Copy File Name",
                "Close",
                "Close Others"
            ]
        );
    }
}

#[test]
fn offers_close_actions_when_a_tab_has_no_real_file() {
    for file in [
        new_changes_tab("/repo", None, None, None),
        new_commit_tab("/repo", commit(), None),
        new_session_changes_tab("/repo", "session-a", None, None),
        new_terminal_file("/repo", None, None),
    ] {
        assert_eq!(
            surface_tab_menu_items(&file, true),
            vec![
                SurfaceTabMenuItem::Item {
                    id: "close",
                    label: "Close",
                    disabled: None
                },
                SurfaceTabMenuItem::Item {
                    id: "close-others",
                    label: "Close Others",
                    disabled: Some(false)
                },
            ]
        );
    }
}

#[test]
fn disables_close_others_when_there_are_no_sibling_tabs() {
    let items = surface_tab_menu_items(
        &new_file_tab("/repo/src/app.ts", "/repo", false, None, None),
        false,
    );
    assert_eq!(
        items.last(),
        Some(&SurfaceTabMenuItem::Item {
            id: "close-others",
            label: "Close Others",
            disabled: Some(true)
        })
    );
}

/// Records what the menu asked the platform to do.
#[derive(Default)]
struct FakeActions {
    calls: RefCell<Vec<(String, String)>>,
    fail_open: RefCell<Option<String>>,
}

impl SurfaceTabActions for FakeActions {
    fn open_with_default_app(&self, path: &str, _: &mut App) -> Task<Result<(), String>> {
        self.calls.borrow_mut().push(("open".into(), path.into()));
        match self.fail_open.borrow_mut().take() {
            Some(error) => Task::ready(Err(error)),
            None => Task::ready(Ok(())),
        }
    }

    fn reveal(&self, path: &str, _: &mut App) -> Task<Result<(), String>> {
        self.calls.borrow_mut().push(("reveal".into(), path.into()));
        Task::ready(Ok(()))
    }

    fn copy_text(&self, text: &str, _: &mut App) -> Task<Result<(), String>> {
        self.calls.borrow_mut().push(("copy".into(), text.into()));
        Task::ready(Ok(()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Terminal,
}

fn files(kind: Kind, ids: &[&str]) -> Vec<FilePaneTab> {
    ids.iter()
        .map(|id| {
            let mut file = FilePaneTab::new(*id, *id, "/project");
            if kind == Kind::Terminal {
                file.terminal = Some(true);
                file.foreground = Some("vite".into());
            }
            file
        })
        .collect()
}

struct Harness<'a> {
    tabs: Entity<SurfaceTabs>,
    events: Rc<RefCell<Vec<SurfaceTabsEvent>>>,
    actions: Rc<FakeActions>,
    cx: &'a mut VisualTestContext,
}

/// Mounts a strip. With `live`, a host applies Select, Close, and Reorder
/// back to the props, as the pane would.
fn mount<'a>(
    files: Vec<FilePaneTab>,
    active: &str,
    live: bool,
    cx: &'a mut TestAppContext,
) -> Harness<'a> {
    cx.update(init);
    let actions = Rc::new(FakeActions::default());
    let props = SurfaceTabsProps {
        files,
        active_file_id: active.into(),
        dirty_file_ids: HashSet::from(["second".to_string()]),
        ..SurfaceTabsProps::default()
    };
    let host_actions: Rc<dyn SurfaceTabActions> = actions.clone();
    let (tabs, cx) = cx.add_window_view(move |_, cx| SurfaceTabs::new(props, host_actions, cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&tabs, move |tabs, event: &SurfaceTabsEvent, cx| {
            sink.borrow_mut().push(event.clone());
            if !live {
                return;
            }
            tabs.update(cx, |tabs, cx| {
                let mut props = tabs.props().clone();
                match event {
                    SurfaceTabsEvent::Select(id) => props.active_file_id = id.clone(),
                    SurfaceTabsEvent::Close(id) => props.files.retain(|file| file.id != *id),
                    SurfaceTabsEvent::Reorder { ids, .. } => {
                        props
                            .files
                            .sort_by_key(|file| ids.iter().position(|id| *id == file.id));
                    }
                    _ => return,
                }
                tabs.set_props(props, cx);
            });
        })
        .detach();
    });
    draw(cx);
    Harness {
        tabs,
        events,
        actions,
        cx,
    }
}

impl Harness<'_> {
    fn bounds(&mut self, selector: String) -> Bounds<Pixels> {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element {selector}"))
    }

    fn tab(&mut self, id: &str) -> Bounds<Pixels> {
        self.bounds(format!("surface-tab:{id}"))
    }

    fn click(&mut self, at: gpui::Point<Pixels>, button: MouseButton) {
        self.cx.simulate_mouse_down(at, button, Modifiers::none());
        draw(self.cx);
        self.cx.simulate_mouse_up(at, button, Modifiers::none());
        draw(self.cx);
    }

    fn events(&self) -> Vec<SurfaceTabsEvent> {
        self.events.borrow().clone()
    }

    fn closes(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                SurfaceTabsEvent::Close(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn selects(&self) -> usize {
        self.events()
            .iter()
            .filter(|event| matches!(event, SurfaceTabsEvent::Select(_)))
            .count()
    }

    fn reorders(&self) -> Vec<(Vec<String>, String)> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                SurfaceTabsEvent::Reorder { ids, moved_id } => Some((ids, moved_id)),
                _ => None,
            })
            .collect()
    }

    fn shown(&self) -> Vec<(String, bool, bool)> {
        self.tabs.read_with(self.cx, |tabs, _| {
            tabs.displayed()
                .into_iter()
                .map(|entry| (entry.id, entry.closing, entry.opening))
                .collect()
        })
    }

    fn open_menu(&mut self, id: &str) {
        let at = self.tab(id).center();
        self.click(at, MouseButton::Right);
    }

    fn pick(&mut self, id: &str, action: &str) {
        self.open_menu(id);
        self.tabs.update_in(self.cx, |tabs, window, cx| {
            tabs.pick_menu(action, window, cx)
        });
        draw(self.cx);
    }
}

fn readme_and_app() -> Vec<FilePaneTab> {
    vec![
        FilePaneTab::new("first", "/repo/README.md", "/repo"),
        FilePaneTab::new("second", "/repo/src/app.ts", "/repo"),
    ]
}

#[gpui::test]
fn selects_the_right_clicked_tab_and_exposes_its_file_actions(cx: &mut TestAppContext) {
    let mut h = mount(readme_and_app(), "first", false, cx);
    h.open_menu("second");
    assert!(
        h.events()
            .contains(&SurfaceTabsEvent::Select("second".into()))
    );
    assert_eq!(
        h.tabs
            .read_with(h.cx, |tabs, _| tabs.menu_file_id().map(str::to_string)),
        Some("second".into())
    );
    assert!(h.closes().is_empty());
}

#[gpui::test]
fn runs_path_external_open_reveal_and_close_actions_for_the_tab(cx: &mut TestAppContext) {
    let mut h = mount(readme_and_app(), "first", false, cx);
    h.pick("second", "copy-path");
    h.pick("second", "copy-relative-path");
    h.pick("second", "copy-name");
    h.pick("second", "open-default");
    h.pick("second", "reveal");
    assert_eq!(
        h.actions.calls.borrow().as_slice(),
        [
            ("copy".to_string(), "/repo/src/app.ts".to_string()),
            ("copy".into(), "src/app.ts".into()),
            ("copy".into(), "app.ts".into()),
            ("open".into(), "/repo/src/app.ts".into()),
            ("reveal".into(), "/repo/src/app.ts".into()),
        ]
    );
    h.pick("second", "close");
    assert_eq!(h.closes(), ["second"]);
    h.pick("second", "close-others");
    assert!(
        h.events()
            .contains(&SurfaceTabsEvent::CloseOthers("second".into()))
    );
}

#[gpui::test]
fn shows_a_file_opening_error_instead_of_failing_silently(cx: &mut TestAppContext) {
    let mut h = mount(readme_and_app(), "first", false, cx);
    *h.actions.fail_open.borrow_mut() = Some("No application can open this file".into());
    h.pick("second", "open-default");
    let error = h
        .tabs
        .read_with(h.cx, |tabs, _| tabs.file_action_error().map(str::to_string));
    assert!(
        error
            .unwrap_or_default()
            .contains("No application can open this file")
    );
    assert!(h.cx.debug_bounds("file-action-error").is_some());
}

#[gpui::test]
fn moves_neighboring_tabs_during_a_drag_and_saves_the_order_after_settling(
    cx: &mut TestAppContext,
) {
    for kind in [Kind::File, Kind::Terminal] {
        let mut h = mount(files(kind, &["first", "second"]), "first", false, &mut *cx);
        let first = h.tab("first");
        let second = h.tab("second");
        h.cx.simulate_mouse_down(second.center(), MouseButton::Left, Modifiers::none());
        draw(h.cx);
        h.cx.simulate_mouse_move(
            point(first.origin.x + px(10.), second.center().y),
            MouseButton::Left,
            Modifiers::none(),
        );
        draw(h.cx);
        let shift = f32::from(second.origin.x - first.origin.x);
        let offsets = h.tabs.read_with(h.cx, |tabs, _| {
            (tabs.reorder.offset("first"), tabs.reorder.offset("second"))
        });
        assert_eq!(offsets.0, Some(shift));
        assert!(offsets.1.unwrap() < 0.0);
        h.cx.simulate_mouse_up(
            point(first.origin.x + px(10.), second.center().y),
            MouseButton::Left,
            Modifiers::none(),
        );
        draw(h.cx);
        assert!(h.reorders().is_empty());
        h.cx.executor().advance_clock(Duration::from_millis(400));
        draw(h.cx);
        assert_eq!(
            h.reorders(),
            vec![(
                vec!["second".to_string(), "first".to_string()],
                "second".to_string()
            )]
        );
    }
}

#[gpui::test]
fn preserves_the_reordered_tabs_when_closing_another_soon_after_the_drop(cx: &mut TestAppContext) {
    for (middle, delay) in [(false, 60), (true, 60), (false, 160), (true, 160)] {
        let mut h = mount(
            files(Kind::File, &["first", "second", "third"]),
            "first",
            true,
            &mut *cx,
        );
        let first = h.tab("first");
        let second = h.tab("second");
        h.cx.simulate_mouse_down(second.center(), MouseButton::Left, Modifiers::none());
        draw(h.cx);
        let drop = point(first.origin.x + px(10.), second.center().y);
        h.cx.simulate_mouse_move(drop, MouseButton::Left, Modifiers::none());
        draw(h.cx);
        h.cx.simulate_mouse_up(drop, MouseButton::Left, Modifiers::none());
        draw(h.cx);
        assert!(h.reorders().is_empty());
        h.cx.executor().advance_clock(Duration::from_millis(delay));
        draw(h.cx);

        let target = if middle {
            h.bounds("surface-tab-button:third".into()).center()
        } else {
            h.bounds("surface-tab-close:third".into()).center()
        };
        h.click(
            target,
            if middle {
                MouseButton::Middle
            } else {
                MouseButton::Left
            },
        );
        assert_eq!(
            h.closes(),
            ["third"],
            "middle {middle} delay {delay}: {:?}",
            h.events()
        );
        h.cx.executor().advance_clock(Duration::from_millis(400));
        draw(h.cx);
        let order: Vec<String> = h.tabs.read_with(h.cx, |tabs, _| {
            tabs.props().files.iter().map(|f| f.id.clone()).collect()
        });
        assert_eq!(order, ["second", "first"]);
        assert_eq!(
            h.reorders(),
            vec![(
                vec!["second".into(), "first".into(), "third".into()],
                "second".into()
            )]
        );
        let events = h.events();
        let reorder_at = events
            .iter()
            .position(|e| matches!(e, SurfaceTabsEvent::Reorder { .. }))
            .unwrap();
        let close_at = events
            .iter()
            .position(|e| matches!(e, SurfaceTabsEvent::Close(_)))
            .unwrap();
        assert!(reorder_at < close_at);
    }
}

#[gpui::test]
fn closes_on_middle_release_without_selecting(cx: &mut TestAppContext) {
    for kind in [Kind::File, Kind::Terminal] {
        for active in ["first", "second"] {
            let mut h = mount(files(kind, &["first", "second"]), active, false, &mut *cx);
            let tab = h.bounds("surface-tab-button:second".into()).center();
            h.click(tab, MouseButton::Middle);
            assert_eq!(h.closes(), ["second"]);
            assert_eq!(h.selects(), 0);
            assert!(h.reorders().is_empty());
        }
    }
}

#[gpui::test]
fn does_not_close_on_middle_press_alone(cx: &mut TestAppContext) {
    let mut h = mount(files(Kind::File, &["first", "second"]), "first", false, cx);
    let tab = h.bounds("surface-tab-button:second".into()).center();
    h.cx.simulate_mouse_down(tab, MouseButton::Middle, Modifiers::none());
    draw(h.cx);
    assert!(h.closes().is_empty());
    assert_eq!(h.selects(), 0);
}

#[gpui::test]
fn accepts_middle_clicks_on_the_icon_padding_and_close_button(cx: &mut TestAppContext) {
    for target in ["icon", "padding", "close button"] {
        let mut h = mount(
            files(Kind::File, &["first", "second"]),
            "first",
            false,
            &mut *cx,
        );
        let button = h.bounds("surface-tab-button:second".into());
        let slot = h.tab("second");
        let at = match target {
            "icon" => point(button.origin.x + px(14.), button.center().y),
            "padding" => point(slot.center().x, slot.origin.y + px(1.)),
            _ => h.bounds("surface-tab-close:second".into()).center(),
        };
        h.click(at, MouseButton::Middle);
        assert_eq!(h.closes(), ["second"], "{target}");
        assert_eq!(h.selects(), 0);
    }
}

#[gpui::test]
fn keeps_left_click_selection_and_the_close_button_working(cx: &mut TestAppContext) {
    let mut h = mount(files(Kind::File, &["first", "second"]), "first", false, cx);
    let tab = h.bounds("surface-tab-button:second".into()).center();
    h.click(tab, MouseButton::Left);
    assert!(
        h.events()
            .contains(&SurfaceTabsEvent::Select("second".into()))
    );
    assert!(h.closes().is_empty());
    h.events.borrow_mut().clear();
    let close = h.bounds("surface-tab-close:second".into()).center();
    h.click(close, MouseButton::Left);
    assert_eq!(h.closes(), ["second"]);
    assert_eq!(h.selects(), 0);
}

#[gpui::test]
fn does_not_close_on_right_click(cx: &mut TestAppContext) {
    let mut h = mount(files(Kind::File, &["first", "second"]), "first", false, cx);
    let tab = h.bounds("surface-tab-button:second".into()).center();
    h.click(tab, MouseButton::Right);
    assert!(h.closes().is_empty());
}

#[gpui::test]
fn collapses_the_closed_tab_before_removing_it(cx: &mut TestAppContext) {
    for last in [false, true] {
        let mut h = mount(
            files(Kind::File, &["first", "second", "third"]),
            "first",
            true,
            &mut *cx,
        );
        let closed = if last { "third" } else { "second" };
        let width = f32::from(h.tab(closed).size.width);
        let close = h.bounds(format!("surface-tab-close:{closed}")).center();
        h.click(close, MouseButton::Left);
        let ghost = h.tabs.read_with(h.cx, |tabs, _| {
            tabs.displayed().into_iter().find(|entry| entry.closing)
        });
        let ghost = ghost.expect("a closing tab");
        assert_eq!(ghost.id, closed);
        assert_eq!(ghost.width, width);
        h.cx.executor().advance_clock(Duration::from_millis(250));
        draw(h.cx);
        let remaining: Vec<String> = h.shown().into_iter().map(|(id, _, _)| id).collect();
        let expected: Vec<&str> = if last {
            vec!["first", "second"]
        } else {
            vec!["first", "third"]
        };
        assert_eq!(remaining, expected);
    }
}

#[gpui::test]
fn skips_the_collapse_under_reduced_motion_or_with_tab_animations_off(cx: &mut TestAppContext) {
    for reduced in [true, false] {
        let mut h = mount(
            files(Kind::File, &["first", "second", "third"]),
            "first",
            true,
            &mut *cx,
        );
        if reduced {
            h.cx.update(|_, cx| cx.set_reduce_motion(true));
        } else {
            h.tabs.update(h.cx, |tabs, cx| {
                let mut props = tabs.props().clone();
                props.tab_animations = false;
                tabs.set_props(props, cx);
            });
        }
        draw(h.cx);
        let close = h.bounds("surface-tab-close:second".into()).center();
        h.click(close, MouseButton::Left);
        assert_eq!(
            h.shown(),
            vec![
                ("first".into(), false, false),
                ("third".into(), false, false)
            ]
        );
        h.cx.update(|_, cx| cx.set_reduce_motion(false));
    }
}

#[gpui::test]
fn expands_a_new_tab_from_zero_width(cx: &mut TestAppContext) {
    let h = mount(files(Kind::File, &["first", "second"]), "second", false, cx);
    h.tabs.update(h.cx, |tabs, cx| {
        let mut props = tabs.props().clone();
        props.files = files(Kind::File, &["first", "second", "third"]);
        props.active_file_id = "third".into();
        tabs.set_props(props, cx);
    });
    draw(h.cx);
    assert_eq!(h.shown()[2], ("third".into(), false, true));
    assert!(h.cx.debug_bounds("surface-tab-close:third").is_some());
    h.cx.executor().advance_clock(Duration::from_millis(250));
    draw(h.cx);
    assert_eq!(h.shown()[2], ("third".into(), false, false));
}
