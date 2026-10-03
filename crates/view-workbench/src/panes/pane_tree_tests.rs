//! Port of PaneTreeTitleDrop.test.ts, plus the sash, pane move, focus, and
//! external drop behavior PaneTree.tsx had without its own tests.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AppContext as _, Bounds, Context, Entity, IntoElement, Modifiers, MouseButton, Pixels, Render,
    TestAppContext, VisualTestContext, Window, div, point, px,
};
use monocode_layout::pane_drop::{TitleTabDrop, TitleTabDropPosition};
use monocode_layout::{LayoutNode, PaneEdge, SplitDir, leaf};

use super::*;
use crate::panes::test_support::{draw, init};

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn split(dir: SplitDir, ids: &[&str]) -> LayoutNode {
    LayoutNode::split(
        "split",
        dir,
        ids.iter().map(|id| leaf(*id)).collect(),
        vec![1.0 / ids.len() as f64; ids.len()],
    )
}

struct Harness<'a> {
    tree: Entity<PaneTree>,
    events: Rc<RefCell<Vec<PaneTreeEvent>>>,
    cx: &'a mut VisualTestContext,
}

fn mount<'a>(layout: LayoutNode, kind: PaneLeafKind, cx: &'a mut TestAppContext) -> Harness<'a> {
    cx.update(init);
    let ids = monocode_layout::leaf_ids(&layout);
    let focused = ids[0].clone();
    let (tree, cx) = cx.add_window_view(move |_, cx| {
        let mut tree = PaneTree::new(layout, focused, cx);
        let leaves: Vec<PaneLeaf> = ids
            .iter()
            .map(|id| PaneLeaf {
                id: id.clone(),
                kind: kind.clone(),
                view: cx.new(|_| Blank).into(),
            })
            .collect();
        tree.set_leaves(leaves, cx);
        tree
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&tree, move |_, event: &PaneTreeEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    Harness { tree, events, cx }
}

impl Harness<'_> {
    fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element {selector}"))
    }

    fn down(&mut self, at: gpui::Point<Pixels>) {
        self.cx
            .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        draw(self.cx);
    }

    fn drag_to(&mut self, at: gpui::Point<Pixels>) {
        self.cx
            .simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
        draw(self.cx);
    }

    fn up(&mut self, at: gpui::Point<Pixels>) {
        self.cx
            .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        draw(self.cx);
    }

    fn events(&self) -> Vec<PaneTreeEvent> {
        self.events.borrow().clone()
    }
}

#[gpui::test]
fn detaches_a_split_pane_at_the_title_tab_insertion_point(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Right, &["first-pane", "second-pane"]),
        PaneLeafKind::Surface,
        cx,
    );
    // The title strip's only tab spans x 100..200 above y 40.
    let hit_test: TitleTabHitTest = Rc::new(|at, _, _| {
        let (x, y) = (f32::from(at.x), f32::from(at.y));
        if y >= 40.0 {
            return None;
        }
        let position = if x < 150.0 {
            TitleTabDropPosition::Before
        } else {
            TitleTabDropPosition::After
        };
        Some(("target-tab".to_string(), position))
    });
    h.tree
        .update(h.cx, |tree, _| tree.set_title_tab_hit_test(Some(hit_test)));

    let second = h.bounds("pane:second-pane");
    let start = point(second.origin.x + px(80.), second.origin.y + px(100.));
    // The pane's tab strip forwards its press to the tree.
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.start_pane_drag("second-pane", start, window, cx)
    });
    draw(h.cx);
    h.drag_to(point(px(120.), px(20.)));
    assert!(
        h.events()
            .contains(&PaneTreeEvent::TitleTabDropChanged(Some(TitleTabDrop {
                from_id: "second-pane".into(),
                target_tab_id: "target-tab".into(),
                position: TitleTabDropPosition::Before,
            })))
    );
    h.up(point(px(120.), px(20.)));

    let events = h.events();
    let detached: Vec<_> = events
        .iter()
        .filter(|event| matches!(event, PaneTreeEvent::DetachPane { .. }))
        .collect();
    assert_eq!(
        detached,
        [&PaneTreeEvent::DetachPane {
            pane_id: "second-pane".into(),
            target_tab_id: "target-tab".into(),
            position: TitleTabDropPosition::Before,
        }]
    );
    // The marker clears before the detach, as `setExternalTitleTabDrop(null)`
    // ran before `onDetachPane`.
    let cleared = events
        .iter()
        .rposition(|event| *event == PaneTreeEvent::TitleTabDropChanged(None))
        .expect("the marker clears");
    let detach = events
        .iter()
        .position(|event| matches!(event, PaneTreeEvent::DetachPane { .. }))
        .unwrap();
    assert!(cleared < detach);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, PaneTreeEvent::MovePane { .. }))
    );
}

#[gpui::test]
fn drags_a_sash_to_preview_and_commit_a_ratio(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Right, &["a", "b"]),
        PaneLeafKind::Surface,
        cx,
    );
    let tree = h.bounds("pane-tree-bounds");
    let sash = h.bounds("sash:split:0");
    h.down(sash.center());
    let quarter = tree.origin.x + tree.size.width * 0.25;
    h.drag_to(point(quarter, sash.center().y));
    let sizes = h.tree.read_with(h.cx, |tree, _| match tree.shown_layout() {
        LayoutNode::Split(split) => split.sizes.clone(),
        LayoutNode::Leaf(_) => Vec::new(),
    });
    assert!((sizes[0] - 0.25).abs() < 0.01, "{sizes:?}");
    // The committed layout waits for the drop.
    assert!(
        h.events()
            .iter()
            .all(|event| !matches!(event, PaneTreeEvent::Ratio { .. }))
    );
    h.up(point(quarter, sash.center().y));
    let ratio = h.events().iter().find_map(|event| match event {
        PaneTreeEvent::Ratio {
            split_id,
            index,
            ratio,
        } => Some((split_id.clone(), *index, *ratio)),
        _ => None,
    });
    let (split_id, index, ratio) = ratio.expect("a ratio event");
    assert_eq!((split_id.as_str(), index), ("split", 0));
    assert!((ratio - 0.25).abs() < 0.01);
}

#[gpui::test]
fn keeps_the_minimum_share_and_cancels_a_sash_drag_on_escape(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Down, &["a", "b"]),
        PaneLeafKind::Surface,
        cx,
    );
    let tree = h.bounds("pane-tree-bounds");
    let sash = h.bounds("sash:split:0");
    h.down(sash.center());
    h.drag_to(point(sash.center().x, tree.origin.y));
    let first = h.tree.read_with(h.cx, |tree, _| match tree.shown_layout() {
        LayoutNode::Split(split) => split.sizes[0],
        LayoutNode::Leaf(_) => 1.0,
    });
    assert!((first - monocode_layout::MIN_SIZE).abs() < 1e-9, "{first}");
    h.cx.simulate_keystrokes("escape");
    draw(h.cx);
    h.up(point(sash.center().x, tree.origin.y));
    assert!(
        h.events()
            .iter()
            .all(|event| !matches!(event, PaneTreeEvent::Ratio { .. }))
    );
    let sizes = h.tree.read_with(h.cx, |tree, _| match tree.shown_layout() {
        LayoutNode::Split(split) => split.sizes.clone(),
        LayoutNode::Leaf(_) => Vec::new(),
    });
    assert_eq!(sizes, vec![0.5, 0.5]);
}

#[gpui::test]
fn moves_a_session_pane_onto_another_panes_edge(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Right, &["a", "b"]),
        PaneLeafKind::Session {
            title: "Chat".into(),
        },
        cx,
    );
    let header = h.bounds("pane-header:a");
    let target = h.bounds("pane:b");
    h.down(header.center());
    let right_edge = point(
        target.origin.x + target.size.width - px(10.),
        target.center().y,
    );
    h.drag_to(right_edge);
    assert!(h.events().contains(&PaneTreeEvent::Focus {
        pane_id: "a".into()
    }));
    assert_eq!(
        h.tree.read_with(h.cx, |tree, _| tree.pane_drag().cloned()),
        Some(PaneDragState {
            from_id: "a".into(),
            over_id: Some("b".into()),
            edge: PaneEdge::Right,
        })
    );
    assert!(h.cx.debug_bounds("pane-drop-hint:Right").is_some());
    h.up(right_edge);
    assert!(h.events().contains(&PaneTreeEvent::MovePane {
        from_id: "a".into(),
        to_id: "b".into(),
        edge: PaneEdge::Right,
    }));
    assert!(h.cx.debug_bounds("pane-drop-hint:Right").is_none());
}

#[gpui::test]
fn escape_cancels_a_pane_drag_and_a_short_press_does_not_move(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Right, &["a", "b"]),
        PaneLeafKind::Session {
            title: "Chat".into(),
        },
        cx,
    );
    let header = h.bounds("pane-header:a");
    let target = h.bounds("pane:b");
    h.down(header.center());
    h.drag_to(target.center());
    h.cx.simulate_keystrokes("escape");
    draw(h.cx);
    h.up(target.center());
    assert!(
        !h.events()
            .iter()
            .any(|event| matches!(event, PaneTreeEvent::MovePane { .. }))
    );

    h.events.borrow_mut().clear();
    h.down(header.center());
    h.drag_to(header.center() + point(px(2.), px(1.)));
    h.up(header.center() + point(px(2.), px(1.)));
    assert!(
        !h.events()
            .iter()
            .any(|event| matches!(event, PaneTreeEvent::MovePane { .. }))
    );
}

#[gpui::test]
fn closes_and_focuses_from_the_split_header(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Right, &["a", "b"]),
        PaneLeafKind::Session {
            title: "Chat".into(),
        },
        cx,
    );
    let close = h.bounds("pane-close:b");
    h.cx.simulate_click(close.center(), Modifiers::none());
    draw(h.cx);
    assert!(h.events().contains(&PaneTreeEvent::Close {
        session_id: "b".into()
    }));
    assert!(
        !h.events()
            .iter()
            .any(|event| matches!(event, PaneTreeEvent::Focus { pane_id } if pane_id == "b"))
    );

    let body = h.bounds("pane:a");
    h.cx.simulate_click(
        point(body.center().x, body.origin.y + body.size.height - px(20.)),
        Modifiers::none(),
    );
    draw(h.cx);
    assert!(h.events().contains(&PaneTreeEvent::Focus {
        pane_id: "a".into()
    }));
}

#[gpui::test]
fn a_single_pane_has_no_header_and_does_not_drag(cx: &mut TestAppContext) {
    let mut h = mount(
        leaf("solo"),
        PaneLeafKind::Session {
            title: "Chat".into(),
        },
        cx,
    );
    assert!(h.cx.debug_bounds("pane-header:solo").is_none());
    let pane = h.bounds("pane:solo");
    h.tree.update_in(h.cx, |tree, window, cx| {
        tree.start_pane_drag("solo", pane.center(), window, cx)
    });
    h.drag_to(pane.center() + point(px(40.), px(0.)));
    assert_eq!(
        h.tree.read_with(h.cx, |tree, _| tree.pane_drag().cloned()),
        None
    );
}

#[gpui::test]
fn shows_and_drops_an_outside_drag_on_a_pane_edge(cx: &mut TestAppContext) {
    let mut h = mount(
        split(SplitDir::Right, &["a", "b"]),
        PaneLeafKind::Surface,
        cx,
    );
    let target = h.bounds("pane:b");
    let top = point(target.center().x, target.origin.y + px(8.));
    let over = h.tree.update(h.cx, |tree, cx| {
        tree.external_drag_move(PaneDragSource::WorkspaceTab("tab-2".into()), top, cx)
    });
    assert_eq!(over, Some(("b".to_string(), PaneEdge::Top)));
    draw(h.cx);
    assert!(h.cx.debug_bounds("pane-drop-hint:Top").is_some());
    h.tree
        .update(h.cx, |tree, cx| tree.external_drag_end(top, true, cx));
    draw(h.cx);
    assert!(h.cx.debug_bounds("pane-drop-hint:Top").is_none());
    assert!(h.events().contains(&PaneTreeEvent::Drop {
        source: PaneDragSource::WorkspaceTab("tab-2".into()),
        pane_id: "b".into(),
        edge: PaneEdge::Top,
    }));

    // A session card dropped back on its own pane does nothing.
    h.events.borrow_mut().clear();
    let own = h.bounds("pane:a").center();
    h.tree.update(h.cx, |tree, cx| {
        tree.external_drag_move(PaneDragSource::Session("a".into()), own, cx);
        tree.external_drag_end(own, true, cx);
    });
    assert!(h.events().is_empty());
}
