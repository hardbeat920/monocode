//! Port of src/features/workspace/model/paneDrop.ts.
//!
//! The TypeScript kept the drop overlay state in module globals with a
//! listener set and read the hovered element from the DOM. Here
//! `ExternalDrops` holds the same state and its setters return whether
//! listeners should be told; the hit tests take the pane and tab rects the
//! view measured instead of calling `document.elementFromPoint`.

use crate::layout::{PaneEdge, PaneRect, pane_edge_from_point};

/// `PaneDrop`: a drag that started outside the pane tree (a sidebar card).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneDrop {
    pub from_id: String,
    pub over_id: Option<String>,
    pub edge: PaneEdge,
}

/// `TitleTabDropPosition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleTabDropPosition {
    Before,
    After,
}

/// `TitleTabDrop`: a pane being detached into the title tab strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleTabDrop {
    pub from_id: String,
    pub target_tab_id: String,
    pub position: TitleTabDropPosition,
}

/// The module state behind `setExternalPaneDrop` and
/// `setExternalTitleTabDrop`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalDrops {
    drop: Option<PaneDrop>,
    title_tab_drop: Option<TitleTabDrop>,
}

impl ExternalDrops {
    /// `setExternalPaneDrop`. Returns `true` when the value changed and
    /// listeners should re-render.
    pub fn set_external_pane_drop(&mut self, next: Option<PaneDrop>) -> bool {
        if self.drop == next {
            return false;
        }
        self.drop = next;
        true
    }

    /// `getExternalPaneDrop`.
    pub fn external_pane_drop(&self) -> Option<&PaneDrop> {
        self.drop.as_ref()
    }

    /// `useExternalPaneDrop(enabled)`: the overlay to draw, or `None` when the
    /// pane tree does not take external drops.
    pub fn visible_pane_drop(&self, enabled: bool) -> Option<&PaneDrop> {
        if enabled { self.drop.as_ref() } else { None }
    }

    /// `setExternalTitleTabDrop`. Returns `true` when the value changed.
    pub fn set_external_title_tab_drop(&mut self, next: Option<TitleTabDrop>) -> bool {
        if self.title_tab_drop == next {
            return false;
        }
        self.title_tab_drop = next;
        true
    }

    /// `getExternalTitleTabDrop`, also what `useExternalTitleTabDrop` returned.
    pub fn external_title_tab_drop(&self) -> Option<&TitleTabDrop> {
        self.title_tab_drop.as_ref()
    }
}

/// A pane the view drew, with its `data-pane-id` and bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneHit {
    pub id: String,
    pub rect: PaneRect,
}

fn contains(rect: &PaneRect, x: f64, y: f64) -> bool {
    x >= rect.left && x < rect.left + rect.width && y >= rect.top && y < rect.top + rect.height
}

/// `paneDropFromPoint`: the pane under the pointer and the edge nearest to it.
pub fn pane_drop_from_point(x: f64, y: f64, panes: &[PaneHit]) -> Option<(String, PaneEdge)> {
    let pane = panes
        .iter()
        .find(|pane| !pane.id.is_empty() && contains(&pane.rect, x, y))?;
    Some((pane.id.clone(), pane_edge_from_point(x, y, pane.rect)))
}

/// A tab in the title tab strip, with its `data-title-tab-id` and
/// horizontal bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleTabHit {
    pub id: String,
    pub left: f64,
    pub width: f64,
}

/// `titleTabDropFromPoint`: where a pane dropped at `x` becomes a new tab.
///
/// Pass the strip's tabs only when the pointer is over the strip, and
/// `hovered` when it is directly over one of them. Otherwise the nearest
/// tab by center wins, the earlier tab on a tie.
pub fn title_tab_drop_from_point(
    x: f64,
    hovered: Option<&str>,
    tabs: &[TitleTabHit],
) -> Option<(String, TitleTabDropPosition)> {
    let center = |tab: &TitleTabHit| (tab.left + (tab.left + tab.width)) / 2.0;
    let direct = hovered.and_then(|id| tabs.iter().find(|tab| tab.id == id));
    let target = match direct {
        Some(tab) => tab,
        None => tabs.iter().reduce(|nearest, tab| {
            if (x - center(tab)).abs() < (x - center(nearest)).abs() {
                tab
            } else {
                nearest
            }
        })?,
    };
    if target.id.is_empty() {
        return None;
    }
    let position = if x < target.left + target.width / 2.0 {
        TitleTabDropPosition::Before
    } else {
        TitleTabDropPosition::After
    };
    Some((target.id.clone(), position))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drop(over: Option<&str>) -> PaneDrop {
        PaneDrop {
            from_id: "s1".into(),
            over_id: over.map(str::to_string),
            edge: PaneEdge::Left,
        }
    }

    #[test]
    fn notifies_only_on_a_change() {
        let mut drops = ExternalDrops::default();
        assert!(!drops.set_external_pane_drop(None));
        assert!(drops.set_external_pane_drop(Some(drop(None))));
        assert!(!drops.set_external_pane_drop(Some(drop(None))));
        assert!(drops.set_external_pane_drop(Some(drop(Some("p1")))));
        assert_eq!(drops.visible_pane_drop(false), None);
        assert_eq!(drops.visible_pane_drop(true), Some(&drop(Some("p1"))));
        assert!(drops.set_external_pane_drop(None));

        let tab_drop = TitleTabDrop {
            from_id: "p1".into(),
            target_tab_id: "t1".into(),
            position: TitleTabDropPosition::After,
        };
        assert!(drops.set_external_title_tab_drop(Some(tab_drop.clone())));
        assert!(!drops.set_external_title_tab_drop(Some(tab_drop.clone())));
        assert_eq!(drops.external_title_tab_drop(), Some(&tab_drop));
    }

    #[test]
    fn finds_the_pane_and_edge_under_the_pointer() {
        let panes = [
            PaneHit {
                id: "a".into(),
                rect: PaneRect {
                    left: 0.0,
                    top: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            },
            PaneHit {
                id: "b".into(),
                rect: PaneRect {
                    left: 100.0,
                    top: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            },
        ];
        assert_eq!(
            pane_drop_from_point(180.0, 50.0, &panes),
            Some(("b".into(), PaneEdge::Right))
        );
        assert_eq!(
            pane_drop_from_point(50.0, 10.0, &panes),
            Some(("a".into(), PaneEdge::Top))
        );
        assert_eq!(pane_drop_from_point(250.0, 50.0, &panes), None);
    }

    #[test]
    fn places_a_title_tab_drop_before_or_after_the_nearest_tab() {
        let tabs = [
            TitleTabHit {
                id: "t1".into(),
                left: 0.0,
                width: 100.0,
            },
            TitleTabHit {
                id: "t2".into(),
                left: 100.0,
                width: 100.0,
            },
        ];
        assert_eq!(
            title_tab_drop_from_point(120.0, Some("t2"), &tabs),
            Some(("t2".into(), TitleTabDropPosition::Before))
        );
        assert_eq!(
            title_tab_drop_from_point(260.0, None, &tabs),
            Some(("t2".into(), TitleTabDropPosition::After))
        );
        assert_eq!(
            title_tab_drop_from_point(10.0, None, &tabs),
            Some(("t1".into(), TitleTabDropPosition::Before))
        );
        assert_eq!(title_tab_drop_from_point(10.0, None, &[]), None);
    }
}
