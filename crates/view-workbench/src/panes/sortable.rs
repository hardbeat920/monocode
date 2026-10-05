//! Port of src/shared/hooks/useSortable.ts: pointer reordering with drop
//! targets, used by tab strips that can also group a tab with another tab
//! or drop it into a tab group.
//!
//! The hook read rects from the DOM on every move. [`Sortable`] takes the
//! measured rects from its owner instead and returns what the hook would
//! have called back.

use super::animated_reorder::Axis;
use super::reorder::move_item;

const THRESHOLD: f32 = 5.0;
/// `DROP_ON_INSET`: the outer quarter of a tab on each side reorders; the
/// center groups.
const DROP_ON_INSET: f32 = 0.25;
const CLICK_SUPPRESS_MS: f64 = 400.0;

/// A rect in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    fn right(&self) -> f32 {
        self.left + self.width
    }

    fn bottom(&self) -> f32 {
        self.top + self.height
    }
}

/// `"tab" | "group"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropKind {
    Tab,
    Group,
}

/// `SortableDropTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortableDropTarget {
    pub kind: DropKind,
    pub id: String,
    /// False when the drop is refused: the target is flagged, not acted on.
    pub allowed: bool,
}

/// What a finished gesture asks the owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortableEffect {
    /// `onReorder(ids, movedId)`.
    Reorder { ids: Vec<String>, moved_id: String },
    /// `onDropOnItem(draggedId, targetId)`.
    DropOnItem {
        dragged_id: String,
        target_id: String,
    },
    /// `onDropOnGroup(draggedId, groupId)`.
    DropOnGroup {
        dragged_id: String,
        group_id: String,
    },
}

#[derive(Debug, Clone)]
struct DragState {
    id: String,
    start_x: f32,
    start_y: f32,
    active: bool,
    to_index: usize,
    drop_target: Option<SortableDropTarget>,
}

/// What the owner measured for a move: the items in order and the group
/// drop zones.
#[derive(Debug, Clone, Default)]
pub struct SortableLayout<'a> {
    pub ids: &'a [String],
    pub items: &'a [(String, Rect)],
    pub groups: &'a [(String, Rect)],
}

impl SortableLayout<'_> {
    fn item(&self, id: &str) -> Option<Rect> {
        self.items
            .iter()
            .find(|(item, _)| item == id)
            .map(|(_, rect)| *rect)
    }
}

/// The gesture state behind `useSortable`.
#[derive(Debug, Clone, Default)]
pub struct Sortable {
    axis: Axis,
    /// `onDropOnItem` was passed: a tab's center groups instead of reordering.
    drop_on_items: bool,
    drag: Option<DragState>,
    suppress_click_until: f64,
}

impl Sortable {
    pub fn new(axis: Axis, drop_on_items: bool) -> Self {
        Self {
            axis,
            drop_on_items,
            drag: None,
            suppress_click_until: 0.0,
        }
    }

    /// `draggingId`.
    pub fn dragging_id(&self) -> Option<&str> {
        self.drag
            .as_ref()
            .filter(|drag| drag.active)
            .map(|drag| drag.id.as_str())
    }

    /// `fromIndex`.
    pub fn from_index(&self, ids: &[String]) -> Option<usize> {
        let id = self.dragging_id()?;
        ids.iter().position(|item| item == id)
    }

    /// `toIndex`: where the dragged item would land, unless it is over a
    /// drop target.
    pub fn to_index(&self) -> Option<usize> {
        let drag = self.drag.as_ref().filter(|drag| drag.active)?;
        if drag.drop_target.is_some() {
            return None;
        }
        Some(drag.to_index)
    }

    /// `dropTarget`.
    pub fn drop_target(&self) -> Option<&SortableDropTarget> {
        self.drag
            .as_ref()
            .filter(|drag| drag.active)
            .and_then(|drag| drag.drop_target.as_ref())
    }

    /// `onItemPointerDown`. `on_no_drag` is true when the press landed on a
    /// `[data-no-drag]` control, such as a close button.
    pub fn press(&mut self, id: &str, ids: &[String], x: f32, y: f32, on_no_drag: bool) -> bool {
        if ids.len() < 2 || on_no_drag {
            return false;
        }
        let Some(from) = ids.iter().position(|item| item == id) else {
            return false;
        };
        self.drag = Some(DragState {
            id: id.to_string(),
            start_x: x,
            start_y: y,
            active: false,
            to_index: from,
            drop_target: None,
        });
        true
    }

    /// The pointer moved. Returns `Some(id)` when this move crossed the
    /// threshold (`onActivate`), and whether the preview changed.
    pub fn pointer_move(
        &mut self,
        x: f32,
        y: f32,
        layout: &SortableLayout<'_>,
        can_drop_on: &dyn Fn(&str, DropKind, &str) -> bool,
    ) -> (Option<String>, bool) {
        let axis = self.axis;
        let drop_on_items = self.drop_on_items;
        let Some(drag) = self.drag.as_mut() else {
            return (None, false);
        };
        let mut activated = None;
        if !drag.active {
            if (x - drag.start_x).hypot(y - drag.start_y) < THRESHOLD {
                return (None, false);
            }
            drag.active = true;
            drag.to_index = layout
                .ids
                .iter()
                .position(|id| *id == drag.id)
                .unwrap_or(drag.to_index);
            activated = Some(drag.id.clone());
        }
        let drop_on = drop_target_at(axis, drop_on_items, &drag.id, x, y, layout, can_drop_on);
        let next = index_at(axis, x, y, layout);
        if next == drag.to_index && drag.drop_target == drop_on {
            return (activated.clone(), activated.is_some());
        }
        drag.to_index = next;
        drag.drop_target = drop_on;
        (activated, true)
    }

    /// Pointer up: the drop.
    pub fn release(&mut self, ids: &[String], now: f64) -> Option<SortableEffect> {
        self.stop(true, ids, now)
    }

    /// Escape or `pointercancel`.
    pub fn cancel(&mut self, ids: &[String], now: f64) {
        self.stop(false, ids, now);
    }

    /// `consumeClick`.
    pub fn consume_click(&self, now: f64) -> bool {
        now < self.suppress_click_until
    }

    fn stop(&mut self, commit: bool, ids: &[String], now: f64) -> Option<SortableEffect> {
        let current = self.drag.take()?;
        if !current.active || !commit {
            return None;
        }
        self.suppress_click_until = now + CLICK_SUPPRESS_MS;
        if let Some(target) = current.drop_target {
            // A refused target still swallows the drop: the tab stays put
            // rather than reordering into a group it cannot join.
            if !target.allowed {
                return None;
            }
            return Some(match target.kind {
                DropKind::Tab => SortableEffect::DropOnItem {
                    dragged_id: current.id,
                    target_id: target.id,
                },
                DropKind::Group => SortableEffect::DropOnGroup {
                    dragged_id: current.id,
                    group_id: target.id,
                },
            });
        }
        let from = ids.iter().position(|id| *id == current.id)?;
        if current.to_index == from {
            return None;
        }
        Some(SortableEffect::Reorder {
            ids: move_item(ids, from, current.to_index),
            moved_id: current.id,
        })
    }
}

/// `indexAt`: the first item whose midpoint is past the pointer, or the
/// last item.
fn index_at(axis: Axis, x: f32, y: f32, layout: &SortableLayout<'_>) -> usize {
    let pos = if axis == Axis::X { x } else { y };
    let mut next = layout.ids.len().saturating_sub(1);
    for (index, id) in layout.ids.iter().enumerate() {
        let Some(rect) = layout.item(id) else {
            continue;
        };
        let mid = if axis == Axis::X {
            rect.left + rect.width / 2.0
        } else {
            rect.top + rect.height / 2.0
        };
        if pos < mid {
            next = index;
            break;
        }
    }
    next
}

/// `dropTargetAt`.
fn drop_target_at(
    axis: Axis,
    drop_on_items: bool,
    dragged_id: &str,
    x: f32,
    y: f32,
    layout: &SortableLayout<'_>,
    can_drop_on: &dyn Fn(&str, DropKind, &str) -> bool,
) -> Option<SortableDropTarget> {
    let target = |kind: DropKind, id: &str| SortableDropTarget {
        kind,
        id: id.to_string(),
        allowed: can_drop_on(dragged_id, kind, id),
    };
    for (group_id, rect) in layout.groups {
        if x >= rect.left && x <= rect.right() && y >= rect.top && y <= rect.bottom() {
            return Some(target(DropKind::Group, group_id));
        }
    }
    if !drop_on_items {
        return None;
    }
    for id in layout.ids {
        if id == dragged_id {
            continue;
        }
        let Some(rect) = layout.item(id) else {
            continue;
        };
        let in_center = if axis == Axis::X {
            let inset = rect.width * DROP_ON_INSET;
            x >= rect.left + inset
                && x <= rect.right() - inset
                && y >= rect.top
                && y <= rect.bottom()
        } else {
            let inset = rect.height * DROP_ON_INSET;
            y >= rect.top + inset
                && y <= rect.bottom() - inset
                && x >= rect.left
                && x <= rect.right()
        };
        if in_center {
            return Some(target(DropKind::Tab, id));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        ["a", "b", "c"].map(str::to_string).to_vec()
    }

    fn items() -> Vec<(String, Rect)> {
        ids()
            .into_iter()
            .enumerate()
            .map(|(index, id)| (id, Rect::new(index as f32 * 100.0, 0.0, 100.0, 32.0)))
            .collect()
    }

    fn allow(_: &str, _: DropKind, _: &str) -> bool {
        true
    }

    #[test]
    fn reorders_past_the_midpoint_after_the_threshold() {
        let ids = ids();
        let items = items();
        let layout = SortableLayout {
            ids: &ids,
            items: &items,
            groups: &[],
        };
        let mut sortable = Sortable::new(Axis::X, false);
        assert!(sortable.press("a", &ids, 50.0, 16.0, false));
        assert_eq!(
            sortable.pointer_move(53.0, 16.0, &layout, &allow),
            (None, false)
        );
        assert_eq!(sortable.dragging_id(), None);
        let (activated, changed) = sortable.pointer_move(160.0, 16.0, &layout, &allow);
        assert_eq!(activated.as_deref(), Some("a"));
        assert!(changed);
        assert_eq!(sortable.to_index(), Some(2));
        assert_eq!(
            sortable.release(&ids, 0.0),
            Some(SortableEffect::Reorder {
                ids: ["b", "c", "a"].map(str::to_string).to_vec(),
                moved_id: "a".into()
            })
        );
        assert!(sortable.consume_click(100.0));
        assert!(!sortable.consume_click(500.0));
    }

    #[test]
    fn a_tab_center_groups_and_a_refused_group_swallows_the_drop() {
        let ids = ids();
        let items = items();
        let groups = vec![("g1".to_string(), Rect::new(0.0, 40.0, 300.0, 20.0))];
        let layout = SortableLayout {
            ids: &ids,
            items: &items,
            groups: &groups,
        };
        let mut sortable = Sortable::new(Axis::X, true);
        sortable.press("a", &ids, 50.0, 16.0, false);
        sortable.pointer_move(150.0, 16.0, &layout, &allow);
        assert_eq!(
            sortable.drop_target(),
            Some(&SortableDropTarget {
                kind: DropKind::Tab,
                id: "b".into(),
                allowed: true
            })
        );
        assert_eq!(sortable.to_index(), None);
        assert_eq!(
            sortable.release(&ids, 0.0),
            Some(SortableEffect::DropOnItem {
                dragged_id: "a".into(),
                target_id: "b".into()
            })
        );

        let refuse = |_: &str, kind: DropKind, _: &str| kind != DropKind::Group;
        sortable.press("a", &ids, 50.0, 16.0, false);
        sortable.pointer_move(150.0, 50.0, &layout, &refuse);
        assert_eq!(
            sortable
                .drop_target()
                .map(|target| (target.kind, target.allowed)),
            Some((DropKind::Group, false))
        );
        assert_eq!(sortable.release(&ids, 0.0), None);
    }

    #[test]
    fn ignores_presses_on_no_drag_controls_and_cancels_on_escape() {
        let ids = ids();
        let items = items();
        let layout = SortableLayout {
            ids: &ids,
            items: &items,
            groups: &[],
        };
        let mut sortable = Sortable::new(Axis::X, false);
        assert!(!sortable.press("a", &ids, 50.0, 16.0, true));
        sortable.press("a", &ids, 50.0, 16.0, false);
        sortable.pointer_move(250.0, 16.0, &layout, &allow);
        sortable.cancel(&ids, 0.0);
        assert_eq!(sortable.dragging_id(), None);
        assert!(!sortable.consume_click(0.0));
    }
}
