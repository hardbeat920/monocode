//! Port of src/features/workspace/hooks/useTabCloseMotion.ts: keep a closed
//! tab on screen while its width collapses, and grow a new tab from zero.
//!
//! The hook measured each tab with a `ResizeObserver`. Here the owner
//! records widths with [`TabCloseMotion::record_width`] after layout, calls
//! [`TabCloseMotion::sync`] with the current items on every render, and
//! calls [`TabCloseMotion::finish_motion`] when a tab's motion ends.

use std::collections::{HashMap, HashSet};

use monocode_layout::FilePaneTab;

/// An item with a stable id.
pub trait MotionItem: Clone {
    fn motion_id(&self) -> &str;
}

impl MotionItem for FilePaneTab {
    fn motion_id(&self) -> &str {
        &self.id
    }
}

impl MotionItem for String {
    fn motion_id(&self) -> &str {
        self
    }
}

/// `TabMotionEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct TabMotionEntry<T> {
    pub id: String,
    pub item: T,
    pub closing: bool,
    pub opening: bool,
    /// The last measured width of a closing tab, or 0 when it was never
    /// measured.
    pub width: f32,
}

fn idle_entry<T: MotionItem>(item: &T) -> TabMotionEntry<T> {
    TabMotionEntry {
        id: item.motion_id().to_string(),
        item: item.clone(),
        closing: false,
        opening: false,
        width: 0.0,
    }
}

/// `orderWithClosingTabs`: put each closing tab back after the live tab it
/// followed before.
fn order_with_closing_tabs(
    old_order: &[String],
    live_ids: &[String],
    closing_ids: &HashSet<String>,
) -> Vec<String> {
    let mut order = live_ids.to_vec();
    for id in old_order {
        if !closing_ids.contains(id) || order.contains(id) {
            continue;
        }
        let old_index = old_order.iter().position(|old| old == id).unwrap_or(0);
        let mut insert_at = 0;
        for index in (0..old_index).rev() {
            if let Some(previous) = order.iter().position(|entry| *entry == old_order[index]) {
                insert_at = previous + 1;
                break;
            }
        }
        order.insert(insert_at, id.clone());
    }
    order
}

/// The state behind `useTabCloseMotion`.
#[derive(Debug, Clone)]
pub struct TabCloseMotion<T> {
    measured: HashMap<String, f32>,
    tracked: Vec<String>,
    entries: Vec<TabMotionEntry<T>>,
}

impl<T: MotionItem> TabCloseMotion<T> {
    pub fn new(items: &[T]) -> Self {
        Self {
            measured: HashMap::new(),
            tracked: items
                .iter()
                .map(|item| item.motion_id().to_string())
                .collect(),
            entries: items.iter().map(idle_entry).collect(),
        }
    }

    /// Records a tab's rendered width. Widths of 1px or less are ignored.
    pub fn record_width(&mut self, id: &str, width: f32) {
        if width > 1.0 {
            self.measured.insert(id.to_string(), width);
        }
    }

    /// The render-time update for a new item list. `skip_motion` is reduced
    /// motion or `monocode.tabAnimationsEnabled` off. Returns the ids whose
    /// closing or opening motion starts now.
    pub fn sync(&mut self, items: &[T], skip_motion: bool) -> Vec<String> {
        let next_ids: Vec<String> = items
            .iter()
            .map(|item| item.motion_id().to_string())
            .collect();
        if next_ids == self.tracked {
            return Vec::new();
        }
        let next_by_id: HashMap<&str, &T> =
            items.iter().map(|item| (item.motion_id(), item)).collect();
        let live_entries: Vec<&TabMotionEntry<T>> =
            self.entries.iter().filter(|entry| !entry.closing).collect();
        let previous_ids: Vec<String> = live_entries.iter().map(|entry| entry.id.clone()).collect();
        let next_set: HashSet<&str> = next_ids.iter().map(String::as_str).collect();
        let replace_all = !previous_ids.is_empty()
            && !next_ids.is_empty()
            && previous_ids
                .iter()
                .all(|id| !next_set.contains(id.as_str()));

        let rendered: Vec<TabMotionEntry<T>> = if replace_all || skip_motion {
            items.iter().map(idle_entry).collect()
        } else {
            let mut closing: Vec<TabMotionEntry<T>> = self
                .entries
                .iter()
                .filter(|entry| entry.closing && !next_set.contains(entry.id.as_str()))
                .cloned()
                .collect();
            let mut closing_ids: HashSet<String> =
                closing.iter().map(|entry| entry.id.clone()).collect();
            for entry in &live_entries {
                if next_set.contains(entry.id.as_str()) {
                    continue;
                }
                closing.push(TabMotionEntry {
                    closing: true,
                    opening: false,
                    width: self.measured.get(&entry.id).copied().unwrap_or(0.0),
                    ..(*entry).clone()
                });
                closing_ids.insert(entry.id.clone());
            }
            let old_order: Vec<String> =
                self.entries.iter().map(|entry| entry.id.clone()).collect();
            let order = order_with_closing_tabs(&old_order, &next_ids, &closing_ids);
            order
                .into_iter()
                .filter_map(|id| match next_by_id.get(id.as_str()) {
                    None => closing.iter().find(|entry| entry.id == id).cloned(),
                    Some(item) => {
                        let previous = live_entries.iter().find(|entry| entry.id == id);
                        Some(TabMotionEntry {
                            id: id.clone(),
                            item: (*item).clone(),
                            closing: false,
                            opening: match previous {
                                Some(previous) => previous.opening,
                                None => !previous_ids.is_empty(),
                            },
                            width: 0.0,
                        })
                    }
                })
                .collect()
        };

        let started: Vec<String> = rendered
            .iter()
            .filter(|entry| entry.closing || entry.opening)
            .filter(|entry| {
                !self.entries.iter().any(|old| {
                    old.id == entry.id
                        && old.closing == entry.closing
                        && old.opening == entry.opening
                })
            })
            .map(|entry| entry.id.clone())
            .collect();
        self.tracked = next_ids;
        self.entries = rendered;
        started
    }

    /// `displayed`: the entries with each live item's current value.
    pub fn displayed(&self, items: &[T]) -> Vec<TabMotionEntry<T>> {
        self.entries
            .iter()
            .map(|entry| {
                let item = items
                    .iter()
                    .find(|item| item.motion_id() == entry.id)
                    .cloned()
                    .unwrap_or_else(|| entry.item.clone());
                TabMotionEntry {
                    item,
                    ..entry.clone()
                }
            })
            .collect()
    }

    /// `finishMotion`: a closing tab leaves; an opening tab settles.
    pub fn finish_motion(&mut self, id: &str) {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return;
        };
        if self.entries[index].closing {
            self.measured.remove(id);
            self.entries.remove(index);
        } else {
            self.entries[index].opening = false;
        }
    }

    /// Whether any tab is mid-motion.
    pub fn is_moving(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.closing || entry.opening)
    }
}

#[cfg(test)]
mod tests {
    //! The motion cases of TabCloseMotion.test.ts, on the model. The view
    //! cases live in surface_tabs_tests.rs.

    use super::*;

    fn ids(items: &[&str]) -> Vec<String> {
        items.iter().map(|id| id.to_string()).collect()
    }

    fn shown(motion: &TabCloseMotion<String>, items: &[String]) -> Vec<(String, bool, bool)> {
        motion
            .displayed(items)
            .into_iter()
            .map(|entry| (entry.id, entry.closing, entry.opening))
            .collect()
    }

    #[test]
    fn collapses_the_closed_tab_in_place_before_removing_it() {
        let before = ids(&["first", "second", "third"]);
        let mut motion = TabCloseMotion::new(&before);
        for id in &before {
            motion.record_width(id, 140.0);
        }
        let after = ids(&["first", "third"]);
        assert_eq!(motion.sync(&after, false), ids(&["second"]));
        let entries = motion.displayed(&after);
        assert_eq!(entries[1].id, "second");
        assert!(entries[1].closing);
        assert_eq!(entries[1].width, 140.0);
        motion.finish_motion("second");
        assert_eq!(
            shown(&motion, &after),
            vec![
                ("first".into(), false, false),
                ("third".into(), false, false)
            ]
        );
    }

    #[test]
    fn collapses_the_last_tab_in_place() {
        let before = ids(&["first", "second", "third"]);
        let mut motion = TabCloseMotion::new(&before);
        let after = ids(&["first", "second"]);
        motion.sync(&after, false);
        assert_eq!(
            shown(&motion, &after),
            vec![
                ("first".into(), false, false),
                ("second".into(), false, false),
                ("third".into(), true, false)
            ]
        );
    }

    #[test]
    fn still_collapses_when_the_width_was_not_measured() {
        let before = ids(&["first", "second", "third"]);
        let mut motion = TabCloseMotion::new(&before);
        motion.record_width("second", 0.0);
        let after = ids(&["first", "third"]);
        motion.sync(&after, false);
        let ghost = motion
            .displayed(&after)
            .into_iter()
            .find(|entry| entry.closing)
            .unwrap();
        assert_eq!(ghost.width, 0.0);
    }

    #[test]
    fn skips_the_motion_under_reduced_motion_or_when_tab_animations_are_off() {
        let before = ids(&["first", "second", "third"]);
        let mut motion = TabCloseMotion::new(&before);
        let after = ids(&["first", "third"]);
        assert!(motion.sync(&after, true).is_empty());
        assert_eq!(
            shown(&motion, &after),
            vec![
                ("first".into(), false, false),
                ("third".into(), false, false)
            ]
        );
    }

    #[test]
    fn expands_a_new_tab_from_zero_width() {
        let before = ids(&["first", "second"]);
        let mut motion = TabCloseMotion::new(&before);
        let after = ids(&["first", "second", "third"]);
        assert_eq!(motion.sync(&after, false), ids(&["third"]));
        assert_eq!(shown(&motion, &after)[2], ("third".into(), false, true));
        motion.finish_motion("third");
        assert!(!motion.is_moving());

        let mut skipped = TabCloseMotion::new(&before);
        assert!(skipped.sync(&after, true).is_empty());
        assert!(!skipped.is_moving());
    }

    #[test]
    fn replacing_every_tab_skips_the_motion() {
        let mut motion = TabCloseMotion::new(&ids(&["a", "b"]));
        let after = ids(&["c", "d"]);
        assert!(motion.sync(&after, false).is_empty());
        assert!(!motion.is_moving());
    }

    #[test]
    fn a_second_close_keeps_the_first_ghost_in_its_slot() {
        let mut motion = TabCloseMotion::new(&ids(&["a", "b", "c", "d"]));
        motion.sync(&ids(&["a", "c", "d"]), false);
        motion.sync(&ids(&["a", "d"]), false);
        let after = ids(&["a", "d"]);
        assert_eq!(
            shown(&motion, &after),
            vec![
                ("a".into(), false, false),
                ("b".into(), true, false),
                ("c".into(), true, false),
                ("d".into(), false, false)
            ]
        );
    }
}
