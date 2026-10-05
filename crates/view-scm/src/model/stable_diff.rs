//! Port of src/features/source-control/model/stableDiff.ts.
//!
//! Refresh helpers that keep unchanged diff data shared, so a live-updating
//! review only rebuilds the files that changed. JavaScript compared object
//! identity; here identity is the `Arc` pointer, and [`ShallowEq`] compares
//! a value's own fields the way `Object.is` did: shared parts by pointer,
//! plain values by `==`.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

/// Field-by-field identity, as `reuseIfShallowEqual` checked it.
pub trait ShallowEq {
    fn shallow_eq(&self, other: &Self) -> bool;
}

/// Something with a stable id (`{ id: string }`).
pub trait HasId {
    fn id(&self) -> &str;
}

/// `reuseIfShallowEqual`: `previous` when `next` has the same fields.
pub fn reuse_if_shallow_equal<T: ShallowEq>(previous: Option<&Arc<T>>, next: Arc<T>) -> Arc<T> {
    match previous {
        Some(previous) if previous.shallow_eq(&next) => previous.clone(),
        _ => next,
    }
}

/// `reuseUnchangedById`: keeps each unchanged item (matched by id), and the
/// whole list when nothing changed.
pub fn reuse_unchanged_by_id<T: ShallowEq + HasId>(
    previous: &Arc<Vec<Arc<T>>>,
    next: Vec<Arc<T>>,
) -> Arc<Vec<Arc<T>>> {
    let by_id: HashMap<&str, &Arc<T>> = previous.iter().map(|item| (item.id(), item)).collect();
    let mut same = previous.len() == next.len();
    let merged: Vec<Arc<T>> = next
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let kept = reuse_if_shallow_equal(by_id.get(item.id()).copied(), item);
            if previous
                .get(index)
                .is_none_or(|old| !Arc::ptr_eq(old, &kept))
            {
                same = false;
            }
            kept
        })
        .collect();
    if same {
        previous.clone()
    } else {
        Arc::new(merged)
    }
}

/// `pruneMap`: drops entries whose keys are gone, keeping the map itself
/// when none are.
pub fn prune_map<K: Eq + Hash + Clone, V: Clone>(
    map: &Arc<HashMap<K, V>>,
    keep: &HashSet<K>,
) -> Arc<HashMap<K, V>> {
    if map.keys().all(|key| keep.contains(key)) {
        return map.clone();
    }
    Arc::new(
        map.iter()
            .filter(|(key, _)| keep.contains(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Model {
        a: i64,
        blocks: Arc<Vec<i64>>,
    }

    impl ShallowEq for Model {
        fn shallow_eq(&self, other: &Self) -> bool {
            self.a == other.a && Arc::ptr_eq(&self.blocks, &other.blocks)
        }
    }

    #[derive(Debug, PartialEq)]
    struct Item {
        id: String,
        n: i64,
    }

    impl ShallowEq for Item {
        fn shallow_eq(&self, other: &Self) -> bool {
            self.id == other.id && self.n == other.n
        }
    }

    impl HasId for Item {
        fn id(&self) -> &str {
            &self.id
        }
    }

    fn item(id: &str, n: i64) -> Arc<Item> {
        Arc::new(Item { id: id.into(), n })
    }

    // describe("reuseIfShallowEqual")

    #[test]
    fn keeps_the_previous_object_when_every_field_is_identical() {
        let blocks = Arc::new(Vec::new());
        let previous = Arc::new(Model {
            a: 1,
            blocks: blocks.clone(),
        });
        let kept = reuse_if_shallow_equal(Some(&previous), Arc::new(Model { a: 1, blocks }));
        assert!(Arc::ptr_eq(&kept, &previous));
    }

    #[test]
    fn takes_the_next_object_when_a_field_changed() {
        let next = Arc::new(Model {
            a: 1,
            blocks: Arc::new(Vec::new()),
        });
        let previous = Arc::new(Model {
            a: 1,
            blocks: Arc::new(Vec::new()),
        });
        let kept = reuse_if_shallow_equal(Some(&previous), next.clone());
        assert!(Arc::ptr_eq(&kept, &next));
        // The TypeScript case of an object that gained a key has no Rust
        // counterpart: a struct's fields are fixed.
    }

    // describe("reuseUnchangedById")

    #[test]
    fn returns_the_previous_array_when_nothing_changed() {
        let previous = Arc::new(vec![item("a", 1), item("b", 2)]);
        let merged = reuse_unchanged_by_id(&previous, vec![item("a", 1), item("b", 2)]);
        assert!(Arc::ptr_eq(&merged, &previous));
    }

    #[test]
    fn keeps_unchanged_items_when_one_item_changes_or_the_list_grows() {
        let a = item("a", 1);
        let previous = Arc::new(vec![a.clone(), item("b", 2)]);
        let merged =
            reuse_unchanged_by_id(&previous, vec![item("c", 0), item("a", 1), item("b", 3)]);
        assert!(!Arc::ptr_eq(&merged, &previous));
        assert!(Arc::ptr_eq(&merged[1], &a));
        assert_eq!(
            *merged[2],
            Item {
                id: "b".into(),
                n: 3
            }
        );
    }

    // describe("pruneMap")

    #[test]
    fn keeps_the_same_map_when_no_keys_are_removed() {
        let map = Arc::new(HashMap::from([("a", 1)]));
        let pruned = prune_map(&map, &HashSet::from(["a", "b"]));
        assert!(Arc::ptr_eq(&pruned, &map));
    }

    #[test]
    fn drops_keys_that_are_no_longer_present() {
        let map = Arc::new(HashMap::from([("a", 1), ("b", 2)]));
        let pruned = prune_map(&map, &HashSet::from(["b"]));
        assert_eq!(*pruned, HashMap::from([("b", 2)]));
    }
}
