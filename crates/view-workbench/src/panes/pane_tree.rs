//! Port of src/features/workspace/ui/PaneTree.tsx: the split tree of one
//! workspace tab.
//!
//! Each leaf is a view the host supplies (a session pane or a file pane),
//! placed at its fractional rect. Sashes between leaves drag to resize, with
//! a minimum share of `MIN_SIZE` (0.08) that `set_split_ratio` enforces. In
//! a split, a pane drags by its header or its tab strip onto another pane's
//! edge, which shows a drop hint, or into the title tab strip, which
//! detaches it into a new workspace tab. Drags that start outside the tree
//! (a title tab or a sidebar card) show the same hint through
//! [`PaneTree::external_drag_move`].
//!
//! The session pane's split header (grip, focus dot, title, close) lived in
//! SessionPane.tsx; the tree draws it here for [`PaneLeafKind::Session`]
//! leaves, because the tree owns the drag that the header starts.
//!
//! A pane split into an existing layout slides in from the edge it was added
//! on (`data-pane-enter` in index.css). Panes present when the tree is built,
//! or swapped in place, just appear.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, AnyView, App, Bounds, Context, CursorStyle,
    DispatchPhase, ElementId, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
    Render, SharedString, Styled as _, Window, canvas, div, px, relative,
};
use monocode_layout::pane_drop::{
    PaneDrop, PaneHit, TitleTabDrop, TitleTabDropPosition, pane_drop_from_point,
};
use monocode_layout::{
    Axis, LayoutLeaf, LayoutNode, LayoutSash, PaneEdge, PaneRect, SplitDir, layout_leaves,
    layout_sashes, set_split_ratio,
};
use monocode_ui::widgets::icon_button;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

/// `DRAG_THRESHOLD`: how far a pane drag travels before it starts, in px.
const DRAG_THRESHOLD: f32 = 5.0;

/// `pane-enter`'s 260ms, the linked work item panel's slide.
pub const PANE_ENTER_DURATION: Duration = Duration::from_millis(260);

/// `PaneEnterFrom`: where a new split pane enters from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneEnterFrom {
    Left,
    Right,
    Top,
    Bottom,
    /// Not on an outer edge of its split: it fades in where it is.
    Fade,
}

/// `paneEnterFrom`: the outer edge of the split the leaf sits on, along the
/// axis its parent split runs.
pub fn pane_enter_from(leaf: &LayoutLeaf) -> PaneEnterFrom {
    const EDGE: f64 = 0.001;
    let rect = leaf.rect;
    match leaf.axis {
        Axis::X if rect.x + rect.w >= 1.0 - EDGE => PaneEnterFrom::Right,
        Axis::X if rect.x <= EDGE => PaneEnterFrom::Left,
        Axis::Y if rect.y + rect.h >= 1.0 - EDGE => PaneEnterFrom::Bottom,
        Axis::Y if rect.y <= EDGE => PaneEnterFrom::Top,
        _ => PaneEnterFrom::Fade,
    }
}

/// What a leaf shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneLeafKind {
    /// A session pane. In a split, the tree draws its header.
    Session { title: SharedString },
    /// A file, diff, or terminal surface. Its tab strip starts pane drags
    /// through [`PaneTree::start_pane_drag`].
    Surface,
}

/// One leaf's content, keyed by the layout's leaf id.
#[derive(Clone)]
pub struct PaneLeaf {
    pub id: String,
    pub kind: PaneLeafKind,
    pub view: AnyView,
}

/// What a drag that started outside the tree carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneDragSource {
    /// A workspace tab from the title bar.
    WorkspaceTab(String),
    /// A session card from the sidebar.
    Session(String),
}

impl PaneDragSource {
    /// The id the hint compares with leaf ids, like `PaneDrop.fromId`.
    pub fn id(&self) -> &str {
        match self {
            PaneDragSource::WorkspaceTab(id) | PaneDragSource::Session(id) => id,
        }
    }
}

/// What the user did in the tree.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneTreeEvent {
    /// `onFocus`: a press in a pane, or the start of a pane drag.
    Focus { pane_id: String },
    /// `onClose`: the split header's close button.
    Close { session_id: String },
    /// `onRatio`: a sash drag ended. `ratio` is the boundary between child
    /// `index` and the next, as a share of the split.
    Ratio {
        split_id: String,
        index: usize,
        ratio: f64,
    },
    /// `onMovePane`: a pane dropped on another pane's edge.
    MovePane {
        from_id: String,
        to_id: String,
        edge: PaneEdge,
    },
    /// `onDetachPane`: a pane dropped into the title tab strip.
    DetachPane {
        pane_id: String,
        target_tab_id: String,
        position: TitleTabDropPosition,
    },
    /// `setExternalTitleTabDrop`: where the title bar should show its
    /// insertion marker while a pane drags over it.
    TitleTabDropChanged(Option<TitleTabDrop>),
    /// A drag from outside the tree dropped on a pane edge: a title tab or
    /// a session card placed beside that pane.
    Drop {
        source: PaneDragSource,
        pane_id: String,
        edge: PaneEdge,
    },
}

/// Finds the title tab insertion point under a window position. The title
/// bar owns the tab rects (`titleTabDropFromPoint`).
pub type TitleTabHitTest =
    Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App) -> Option<(String, TitleTabDropPosition)>>;

/// `PaneDrag`: the pane being dragged and the edge under the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneDragState {
    pub from_id: String,
    pub over_id: Option<String>,
    pub edge: PaneEdge,
}

#[derive(Debug, Clone)]
struct PendingPaneDrag {
    from_id: String,
    start: Point<Pixels>,
    last: Point<Pixels>,
    active: bool,
}

#[derive(Debug, Clone)]
struct SashDrag {
    split_id: String,
    index: usize,
    row: bool,
    origin: f32,
    span: f32,
    next_boundary: f64,
    moved: bool,
}

/// The split tree.
pub struct PaneTree {
    layout: LayoutNode,
    focused_id: String,
    visible: bool,
    leaves: HashMap<String, PaneLeaf>,
    /// The layout while a sash drags.
    draft: Option<LayoutNode>,
    pane_drag: Option<PaneDragState>,
    pending: Option<PendingPaneDrag>,
    external_drop: Option<PaneDrop>,
    external_source: Option<PaneDragSource>,
    sash: Option<SashDrag>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    title_tab_hit_test: Option<TitleTabHitTest>,
    title_tab_drop: Option<TitleTabDrop>,
    /// Takes the keyboard during a drag so Escape cancels it, as the
    /// window `keydown` listener did.
    focus: FocusHandle,
    /// The focus to give back when a drag ends.
    restore_focus: Option<FocusHandle>,
    /// The leaf ids of the last layout (`knownLeafIds`).
    known_leaf_ids: HashSet<String>,
    /// Panes sliding in, with a count that keys each run's animation.
    entering: HashMap<String, (PaneEnterFrom, u64)>,
    enter_runs: u64,
}

impl EventEmitter<PaneTreeEvent> for PaneTree {}

impl PaneTree {
    pub fn new(layout: LayoutNode, focused_id: impl Into<String>, cx: &mut App) -> Self {
        Self {
            focused_id: focused_id.into(),
            visible: true,
            leaves: HashMap::new(),
            draft: None,
            pane_drag: None,
            pending: None,
            external_drop: None,
            external_source: None,
            sash: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            title_tab_hit_test: None,
            title_tab_drop: None,
            focus: cx.focus_handle(),
            restore_focus: None,
            known_leaf_ids: layout_leaves(&layout)
                .into_iter()
                .map(|leaf| leaf.id)
                .collect(),
            entering: HashMap::new(),
            enter_runs: 0,
            layout,
        }
    }

    /// Takes the keyboard for a drag, remembering who had it.
    fn grab_keyboard(&mut self, window: &mut Window, cx: &mut App) {
        if self.restore_focus.is_none() {
            self.restore_focus = window.focused(cx);
        }
        window.focus(&self.focus, cx);
    }

    /// Gives the keyboard back once no drag runs.
    fn release_keyboard(&mut self, window: &mut Window, cx: &mut App) {
        if self.pending.is_some() || self.sash.is_some() {
            return;
        }
        if let Some(previous) = self.restore_focus.take() {
            window.focus(&previous, cx);
        } else if self.focus.is_focused(window) {
            window.blur();
        }
    }

    /// A new layout drops any sash preview, as the `[layout]` effect did.
    pub fn set_layout(
        &mut self,
        layout: LayoutNode,
        focused_id: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let focused_id = focused_id.into();
        if self.layout != layout {
            self.layout = layout;
            self.draft = None;
            self.sash = None;
            self.track_entering_panes(cx);
        }
        self.focused_id = focused_id;
        cx.notify();
    }

    /// Start the slide for leaves a split added. The app reuses one tree
    /// for every workspace tab, so a layout that keeps none of the known
    /// leaves is a tab switch and does not animate.
    fn track_entering_panes(&mut self, cx: &mut Context<Self>) {
        let leaves = layout_leaves(&self.layout);
        let grew = leaves.len() > self.known_leaf_ids.len()
            && leaves
                .iter()
                .any(|leaf| self.known_leaf_ids.contains(&leaf.id));
        self.entering
            .retain(|id, _| leaves.iter().any(|leaf| &leaf.id == id));
        if grew && !cx.reduce_motion() {
            for leaf in &leaves {
                if self.known_leaf_ids.contains(&leaf.id) {
                    continue;
                }
                self.enter_runs += 1;
                let run = self.enter_runs;
                self.entering
                    .insert(leaf.id.clone(), (pane_enter_from(leaf), run));
                // `onAnimationEnd`: drop the wrapper's motion once it ends.
                let id = leaf.id.clone();
                cx.spawn(async move |tree, cx| {
                    cx.background_executor().timer(PANE_ENTER_DURATION).await;
                    tree.update(cx, |tree, cx| {
                        if tree.entering.get(&id).is_some_and(|(_, at)| *at == run) {
                            tree.entering.remove(&id);
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
        self.known_leaf_ids = leaves.into_iter().map(|leaf| leaf.id).collect();
    }

    /// The edge a pane is sliding in from, while it does.
    pub fn entering_from(&self, pane_id: &str) -> Option<PaneEnterFrom> {
        self.entering.get(pane_id).map(|(from, _)| *from)
    }

    /// The leaves' content. Leaves without an entry draw empty.
    pub fn set_leaves(
        &mut self,
        leaves: impl IntoIterator<Item = PaneLeaf>,
        cx: &mut Context<Self>,
    ) {
        self.leaves = leaves
            .into_iter()
            .map(|leaf| (leaf.id.clone(), leaf))
            .collect();
        cx.notify();
    }

    /// Hidden trees take no external drops (`useExternalPaneDrop(visible)`).
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        cx.notify();
    }

    pub fn set_title_tab_hit_test(&mut self, hit_test: Option<TitleTabHitTest>) {
        self.title_tab_hit_test = hit_test;
    }

    pub fn layout(&self) -> &LayoutNode {
        &self.layout
    }

    /// The tree as drawn: the sash preview while a sash drags.
    pub fn shown_layout(&self) -> &LayoutNode {
        self.draft.as_ref().unwrap_or(&self.layout)
    }

    pub fn focused_id(&self) -> &str {
        &self.focused_id
    }

    /// The pane drag in progress, once past its threshold.
    pub fn pane_drag(&self) -> Option<&PaneDragState> {
        self.pane_drag.as_ref()
    }

    /// The drop hint to draw: a pane drag, else an external drag.
    pub fn drop_hint(&self) -> Option<PaneDrop> {
        if let Some(drag) = &self.pane_drag {
            return Some(PaneDrop {
                from_id: drag.from_id.clone(),
                over_id: drag.over_id.clone(),
                edge: drag.edge,
            });
        }
        self.external_drop.clone().filter(|_| self.visible)
    }

    /// Whether more than one leaf shares the tab.
    pub fn in_split(&self) -> bool {
        layout_leaves(self.shown_layout()).len() > 1
    }

    /// Each leaf's bounds in window coordinates, from the last layout.
    pub fn pane_hits(&self) -> Vec<PaneHit> {
        let bounds = self.bounds.get();
        let (left, top) = (
            f32::from(bounds.origin.x) as f64,
            f32::from(bounds.origin.y) as f64,
        );
        let (width, height) = (
            f32::from(bounds.size.width) as f64,
            f32::from(bounds.size.height) as f64,
        );
        layout_leaves(self.shown_layout())
            .into_iter()
            .map(|leaf| PaneHit {
                id: leaf.id,
                rect: PaneRect {
                    left: left + leaf.rect.x * width,
                    top: top + leaf.rect.y * height,
                    width: leaf.rect.w * width,
                    height: leaf.rect.h * height,
                },
            })
            .collect()
    }

    /// `paneDropFromPoint`: the pane under a window position and its nearest
    /// edge.
    pub fn pane_drop_at(&self, position: Point<Pixels>) -> Option<(String, PaneEdge)> {
        pane_drop_from_point(
            f32::from(position.x) as f64,
            f32::from(position.y) as f64,
            &self.pane_hits(),
        )
    }

    /// `setExternalPaneDrop`, for drags the tree did not start.
    pub fn set_external_drop(&mut self, drop: Option<PaneDrop>, cx: &mut Context<Self>) {
        if self.external_drop != drop {
            self.external_drop = drop;
            cx.notify();
        }
    }

    /// A drag from outside the tree moved to `position`. Updates the hint
    /// and returns the pane and edge under the pointer.
    pub fn external_drag_move(
        &mut self,
        source: PaneDragSource,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<(String, PaneEdge)> {
        let over = self.pane_drop_at(position);
        let drop = PaneDrop {
            from_id: source.id().to_string(),
            over_id: over.as_ref().map(|(id, _)| id.clone()),
            edge: over.as_ref().map_or(PaneEdge::Left, |(_, edge)| *edge),
        };
        self.external_source = Some(source);
        self.set_external_drop(Some(drop), cx);
        over
    }

    /// The outside drag ended. With `commit`, a drop on a pane other than
    /// the dragged one emits [`PaneTreeEvent::Drop`].
    pub fn external_drag_end(
        &mut self,
        position: Point<Pixels>,
        commit: bool,
        cx: &mut Context<Self>,
    ) {
        let source = self.external_source.take();
        self.set_external_drop(None, cx);
        let (Some(source), true) = (source, commit) else {
            return;
        };
        if !self.visible {
            return;
        }
        if let Some((pane_id, edge)) = self.pane_drop_at(position)
            && pane_id != source.id()
        {
            cx.emit(PaneTreeEvent::Drop {
                source,
                pane_id,
                edge,
            });
        }
    }

    /// `startPaneDrag`: a press on a pane's header or tab strip at a window
    /// position. Only panes in a split drag.
    pub fn start_pane_drag(
        &mut self,
        pane_id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.in_split() || self.pending.is_some() {
            return;
        }
        self.pending = Some(PendingPaneDrag {
            from_id: pane_id.to_string(),
            start: position,
            last: position,
            active: false,
        });
        self.grab_keyboard(window, cx);
        cx.notify();
    }

    /// A pointer move during a pane drag.
    pub fn pane_drag_moved(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        pending.last = position;
        let from_id = pending.from_id.clone();
        if !pending.active {
            let distance = f32::from(position.x - pending.start.x)
                .hypot(f32::from(position.y - pending.start.y));
            if distance < DRAG_THRESHOLD {
                return;
            }
            pending.active = true;
            cx.emit(PaneTreeEvent::Focus {
                pane_id: from_id.clone(),
            });
            self.pane_drag = Some(PaneDragState {
                from_id: from_id.clone(),
                over_id: None,
                edge: PaneEdge::Left,
            });
        }
        let title_tab = self.title_tab_at(position, window, cx);
        self.set_title_tab_drop(
            title_tab
                .as_ref()
                .map(|(target_tab_id, position)| TitleTabDrop {
                    from_id: from_id.clone(),
                    target_tab_id: target_tab_id.clone(),
                    position: *position,
                }),
            cx,
        );
        let next = if title_tab.is_some() {
            PaneDragState {
                from_id,
                over_id: None,
                edge: PaneEdge::Left,
            }
        } else {
            match self.pane_drop_at(position) {
                Some((over, edge)) if over != from_id => PaneDragState {
                    from_id,
                    over_id: Some(over),
                    edge,
                },
                over => PaneDragState {
                    over_id: over.as_ref().map(|_| from_id.clone()),
                    edge: over.map_or(PaneEdge::Left, |(_, edge)| edge),
                    from_id,
                },
            }
        };
        if self.pane_drag.as_ref() != Some(&next) {
            self.pane_drag = Some(next);
        }
        cx.notify();
    }

    /// Pointer up (`commit`) or Escape during a pane drag.
    pub fn finish_pane_drag(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.release_keyboard(window, cx);
        self.pane_drag = None;
        self.set_title_tab_drop(None, cx);
        cx.notify();
        if !pending.active || !commit {
            return;
        }
        if let Some((target_tab_id, position)) = self.title_tab_at(pending.last, window, cx) {
            cx.emit(PaneTreeEvent::DetachPane {
                pane_id: pending.from_id,
                target_tab_id,
                position,
            });
            return;
        }
        if let Some((over, edge)) = self.pane_drop_at(pending.last)
            && over != pending.from_id
        {
            cx.emit(PaneTreeEvent::MovePane {
                from_id: pending.from_id,
                to_id: over,
                edge,
            });
        }
    }

    fn title_tab_at(
        &self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(String, TitleTabDropPosition)> {
        let hit_test = self.title_tab_hit_test.clone()?;
        hit_test(position, window, cx)
    }

    fn set_title_tab_drop(&mut self, drop: Option<TitleTabDrop>, cx: &mut Context<Self>) {
        if self.title_tab_drop != drop {
            self.title_tab_drop = drop.clone();
            cx.emit(PaneTreeEvent::TitleTabDropChanged(drop));
        }
    }

    /// A press on a sash's handle.
    fn start_sash_drag(&mut self, sash: &LayoutSash, window: &mut Window, cx: &mut Context<Self>) {
        let rect = self.bounds.get();
        let row = sash.dir == SplitDir::Right;
        let group = sash.group;
        let (origin, span) = if row {
            (
                f32::from(rect.origin.x) + group.x as f32 * f32::from(rect.size.width),
                group.w as f32 * f32::from(rect.size.width),
            )
        } else {
            (
                f32::from(rect.origin.y) + group.y as f32 * f32::from(rect.size.height),
                group.h as f32 * f32::from(rect.size.height),
            )
        };
        self.sash = Some(SashDrag {
            split_id: sash.split_id.clone(),
            index: sash.index,
            row,
            origin,
            span,
            next_boundary: sash_boundary(sash),
            moved: false,
        });
        self.grab_keyboard(window, cx);
        cx.notify();
    }

    /// A pointer move during a sash drag: preview the new ratio.
    pub fn sash_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(sash) = self.sash.as_mut() else {
            return;
        };
        let pos = f32::from(if sash.row { position.x } else { position.y });
        if sash.span <= 0.0 {
            return;
        }
        sash.moved = true;
        sash.next_boundary = ((pos - sash.origin) / sash.span) as f64;
        self.draft = Some(set_split_ratio(
            &self.layout,
            &sash.split_id,
            sash.index,
            sash.next_boundary,
        ));
        cx.notify();
    }

    /// Pointer up (`commit`) or Escape during a sash drag.
    pub fn finish_sash_drag(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sash) = self.sash.take() else {
            return;
        };
        self.release_keyboard(window, cx);
        cx.notify();
        if !sash.moved {
            return;
        }
        self.draft = None;
        if commit {
            cx.emit(PaneTreeEvent::Ratio {
                split_id: sash.split_id,
                index: sash.index,
                ratio: sash.next_boundary,
            });
        }
    }

    fn dragging(&self) -> bool {
        self.pending.is_some() || self.sash.is_some()
    }

    /// The window-wide listeners a running drag needs, registered for one
    /// frame from a canvas.
    fn drag_listeners(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tree = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let on_move = tree.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    on_move
                        .update(cx, |tree, cx| {
                            if tree.sash.is_some() {
                                tree.sash_moved(event.position, cx);
                            } else {
                                tree.pane_drag_moved(event.position, window, cx);
                            }
                        })
                        .ok();
                });
                let on_up = tree.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    on_up
                        .update(cx, |tree, cx| {
                            if tree.sash.is_some() {
                                tree.finish_sash_drag(true, window, cx);
                            } else {
                                if let Some(pending) = tree.pending.as_mut() {
                                    pending.last = event.position;
                                }
                                tree.finish_pane_drag(true, window, cx);
                            }
                        })
                        .ok();
                });
            },
        )
        .absolute()
        .size_full()
    }

    fn render_leaf(
        &self,
        id: &str,
        rect: monocode_layout::LayoutRect,
        in_split: bool,
        hint: Option<&PaneDrop>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let leaf = self.leaves.get(id).cloned();
        let dragging = hint.is_some_and(|drop| drop.from_id == id);
        let show_hint = hint
            .filter(|drop| drop.over_id.as_deref() == Some(id) && drop.from_id != id)
            .map(|drop| drop.edge);
        let focus_id = id.to_string();
        let mut pane = div()
            .id(ElementId::Name(format!("pane:{id}").into()))
            .debug_selector({
                let id = id.to_string();
                move || format!("pane:{id}")
            })
            .absolute()
            .left(relative(rect.x as f32))
            .top(relative(rect.y as f32))
            .w(relative(rect.w as f32))
            .h(relative(rect.h as f32))
            .flex()
            .flex_col()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .on_any_mouse_down(cx.listener(move |_, _, _, cx| {
                cx.emit(PaneTreeEvent::Focus {
                    pane_id: focus_id.clone(),
                });
            }));
        if dragging {
            pane = pane.opacity(0.4);
        }
        if let Some(leaf) = leaf {
            // The neighbours reflow at once; only the new pane's content
            // moves, so nothing rewraps mid-slide.
            let mut content = div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .min_w_0();
            if let (PaneLeafKind::Session { title }, true) = (&leaf.kind, in_split) {
                content = content.child(self.render_split_header(id, title.clone(), theme, cx));
            }
            content = content.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(leaf.view),
            );
            pane = pane.child(match self.entering.get(id) {
                Some((from, run)) => pane_enter(content, id, *from, *run, theme),
                None => content.into_any_element(),
            });
        }
        if let Some(edge) = show_hint {
            pane = pane.child(pane_drop_hint(edge, theme));
        }
        pane
    }

    /// SessionPane's `inSplit` header: grip, focus dot, title, and close.
    fn render_split_header(
        &self,
        id: &str,
        title: SharedString,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.focused_id == id;
        let drag_id = id.to_string();
        let close_id = id.to_string();
        div()
            .id(ElementId::Name(format!("pane-header:{id}").into()))
            .debug_selector({
                let id = id.to_string();
                move || format!("pane-header:{id}")
            })
            .flex()
            .flex_none()
            .h(u(36.))
            .items_center()
            .gap(u(6.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(8.))
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |tree, event: &gpui::MouseDownEvent, window, cx| {
                    tree.start_pane_drag(&drag_id, event.position, window, cx);
                }),
            )
            .child(
                icon(IconName::GripVertical)
                    .flex_none()
                    .size(u(14.))
                    .text_color(theme.content(0.35)),
            )
            .child(
                div()
                    .flex_none()
                    .size(u(8.))
                    .rounded_full()
                    .when(focused, |dot| dot.bg(theme.colors.accent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(12.)
                    .text_color(theme.colors.content)
                    .child(title),
            )
            .child(
                div()
                    .debug_selector({
                        let id = id.to_string();
                        move || format!("pane-close:{id}")
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        icon_button(
                            ElementId::Name(format!("pane-close:{id}").into()),
                            IconName::X,
                        )
                        .size(20.)
                        .icon_size(12.)
                        .tooltip(close_pane_tooltip())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(PaneTreeEvent::Close {
                                session_id: close_id.clone(),
                            });
                        })),
                    ),
            )
    }

    fn render_sash(
        &self,
        sash: &LayoutSash,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let row = sash.dir == SplitDir::Right;
        let boundary = sash_boundary(sash) as f32;
        let group = sash.group;
        let key = format!("sash:{}:{}", sash.split_id, sash.index);
        let line = div().absolute().bg(theme.colors.stroke);
        let line = if row {
            line.left(relative(group.x as f32 + boundary * group.w as f32))
                .top(relative(group.y as f32))
                .h(relative(group.h as f32))
                .w(px(1.))
        } else {
            line.left(relative(group.x as f32))
                .top(relative(group.y as f32 + boundary * group.h as f32))
                .w(relative(group.w as f32))
                .h(px(1.))
        };
        let handle = div()
            .id(ElementId::Name(key.clone().into()))
            .debug_selector(move || key.clone())
            .absolute();
        let handle = if row {
            handle
                .top_0()
                .bottom_0()
                .left(u(-6.))
                .w(u(13.))
                .cursor(CursorStyle::ResizeColumn)
        } else {
            handle
                .left_0()
                .right_0()
                .top(u(-6.))
                .h(u(13.))
                .cursor(CursorStyle::ResizeRow)
        };
        let sash = sash.clone();
        line.child(handle.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |tree, _: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                tree.start_sash_drag(&sash, window, cx);
            }),
        ))
    }
}

/// `[data-pane-enter]`: the content starts a full pane away on its edge, or
/// transparent for `Fade`, and eases into place.
fn pane_enter(
    content: gpui::Div,
    id: &str,
    from: PaneEnterFrom,
    run: u64,
    theme: &Theme,
) -> AnyElement {
    content
        .with_animation(
            ElementId::Name(format!("pane-enter:{id}:{run}").into()),
            Animation::new(PANE_ENTER_DURATION).with_easing(theme.motion.ease_out.easing()),
            move |el, t| {
                let rest = 1.0 - t;
                match from {
                    PaneEnterFrom::Right => el.left(relative(rest)),
                    PaneEnterFrom::Left => el.left(relative(-rest)),
                    PaneEnterFrom::Bottom => el.top(relative(rest)),
                    PaneEnterFrom::Top => el.top(relative(-rest)),
                    PaneEnterFrom::Fade => el.opacity(t),
                }
            },
        )
        .into_any_element()
}

/// The share of the split before the sash.
fn sash_boundary(sash: &LayoutSash) -> f64 {
    sash.sizes.iter().take(sash.index + 1).sum()
}

fn close_pane_tooltip() -> String {
    if cfg!(target_os = "macos") {
        "Close Pane (⌘W)".into()
    } else {
        "Close Pane (Ctrl+W)".into()
    }
}

/// `PaneDropHint`: a wash over the half of the pane the drop takes, and a
/// 2px line on its edge.
pub fn pane_drop_hint(edge: PaneEdge, theme: &Theme) -> impl IntoElement + use<> {
    let wash = div().absolute().bg(theme.accent(0.15));
    let line = div().absolute().bg(theme.colors.accent);
    let (wash, line) = match edge {
        PaneEdge::Left => (
            wash.top_0().bottom_0().left_0().w(relative(0.5)),
            line.top_0().bottom_0().left_0().w(u(2.)),
        ),
        PaneEdge::Right => (
            wash.top_0().bottom_0().right_0().w(relative(0.5)),
            line.top_0().bottom_0().right_0().w(u(2.)),
        ),
        PaneEdge::Top => (
            wash.left_0().right_0().top_0().h(relative(0.5)),
            line.left_0().right_0().top_0().h(u(2.)),
        ),
        PaneEdge::Bottom => (
            wash.left_0().right_0().bottom_0().h(relative(0.5)),
            line.left_0().right_0().bottom_0().h(u(2.)),
        ),
    };
    div()
        .debug_selector(move || format!("pane-drop-hint:{edge:?}"))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(wash)
        .child(line)
}

impl Render for PaneTree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let tree = self.shown_layout().clone();
        let leaves = layout_leaves(&tree);
        let sashes = layout_sashes(&tree);
        let in_split = leaves.len() > 1;
        let hint = self.drop_hint();
        let bounds = self.bounds.clone();

        let mut root = div()
            .id("pane-tree")
            .debug_selector(|| "pane-tree-bounds".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|tree, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key != "escape" || !tree.dragging() {
                    return;
                }
                cx.stop_propagation();
                tree.finish_sash_drag(false, window, cx);
                tree.finish_pane_drag(false, window, cx);
            }))
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(
                canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            );
        for leaf in &leaves {
            root = root.child(self.render_leaf(
                &leaf.id,
                leaf.rect,
                in_split,
                hint.as_ref(),
                &theme,
                cx,
            ));
        }
        for sash in &sashes {
            root = root.child(self.render_sash(sash, &theme, cx));
        }
        if self.dragging() {
            // While a drag runs, an overlay holds the cursor and keeps the
            // panes from reacting to hover, as `suppressTextSelection` and
            // the body cursor did.
            let cursor = match &self.sash {
                Some(sash) if sash.row => CursorStyle::ResizeColumn,
                Some(_) => CursorStyle::ResizeRow,
                None if self.pane_drag.is_some() => CursorStyle::ClosedHand,
                None => CursorStyle::Arrow,
            };
            root = root.child(
                div()
                    .id("pane-tree-drag")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .cursor(cursor)
                    .when(self.pane_drag.is_some() || self.sash.is_some(), |overlay| {
                        overlay.block_mouse_except_scroll()
                    })
                    .child(self.drag_listeners(cx)),
            );
        }
        root
    }
}

#[cfg(test)]
#[path = "pane_tree_tests.rs"]
mod tests;
