//! Port of src/shared/lib/reorder.ts.

use std::collections::HashSet;

/// Something with a stable string id.
pub trait HasId {
    fn id(&self) -> &str;
}

/// `moveItem`: the list with `from` moved to `to`. `None` when the move is a
/// no-op or out of range, where the TypeScript returned the same array.
pub fn move_item<T: Clone>(items: &[T], from: usize, to: usize) -> Option<Vec<T>> {
    if from == to || from >= items.len() || to >= items.len() {
        return None;
    }
    let mut next = items.to_vec();
    let item = next.remove(from);
    next.insert(to, item);
    Some(next)
}

/// `orderByIds`: items in `ids` order first, then the rest in their order.
pub fn order_by_ids<T: HasId + Clone>(items: &[T], ids: &[String]) -> Vec<T> {
    let mut next = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    for id in ids {
        // The TypeScript Map kept the last item for a repeated id.
        let Some(item) = items.iter().rev().find(|item| item.id() == id) else {
            continue;
        };
        if !seen.insert(id.as_str()) {
            continue;
        }
        next.push(item.clone());
    }
    for item in items {
        if !seen.contains(item.id()) {
            next.push(item.clone());
        }
    }
    next
}

/// `mergeOrderedSubset`: replace only the slots a reordered subset
/// occupies, keeping every other item in place. `None` when the subset has a
/// duplicate or unknown id, where the TypeScript returned the same array.
pub fn merge_ordered_subset<T: HasId + Clone>(items: &[T], ordered_subset: &[T]) -> Option<Vec<T>> {
    let subset_ids: HashSet<&str> = ordered_subset.iter().map(HasId::id).collect();
    if subset_ids.len() != ordered_subset.len() {
        return None;
    }
    let item_ids: HashSet<&str> = items.iter().map(HasId::id).collect();
    if ordered_subset
        .iter()
        .any(|item| !item_ids.contains(item.id()))
    {
        return None;
    }
    let mut subset = ordered_subset.iter();
    Some(
        items
            .iter()
            .map(|item| {
                if subset_ids.contains(item.id()) {
                    subset.next().cloned().unwrap_or_else(|| item.clone())
                } else {
                    item.clone()
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Item {
        id: &'static str,
        value: Option<&'static str>,
    }

    impl HasId for Item {
        fn id(&self) -> &str {
            self.id
        }
    }

    fn item(id: &'static str) -> Item {
        Item { id, value: None }
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|item| item.id).collect()
    }

    #[test]
    fn reorders_a_project_scoped_subset_without_moving_hidden_items() {
        let items = vec![
            item("project-a-1"),
            item("project-b-1"),
            item("project-a-2"),
        ];
        let next = merge_ordered_subset(&items, &[items[2].clone(), items[0].clone()]).unwrap();
        assert_eq!(
            ids(&next),
            vec!["project-a-2", "project-b-1", "project-a-1"]
        );
    }

    #[test]
    fn keeps_updated_subset_entries() {
        let items = vec![item("a"), item("hidden"), item("b")];
        let updated = Item {
            id: "b",
            value: Some("updated"),
        };
        let next = merge_ordered_subset(&items, &[updated.clone(), items[0].clone()]).unwrap();
        assert_eq!(next, vec![updated, item("hidden"), item("a")]);
    }

    #[test]
    fn rejects_duplicate_or_unknown_subset_ids() {
        let items = vec![item("a"), item("b")];
        assert!(merge_ordered_subset(&items, &[items[0].clone(), items[0].clone()]).is_none());
        assert!(merge_ordered_subset(&items, &[item("missing")]).is_none());
    }

    #[test]
    fn moves_and_orders_items() {
        let items = vec![item("a"), item("b"), item("c")];
        assert_eq!(ids(&move_item(&items, 0, 2).unwrap()), vec!["b", "c", "a"]);
        assert!(move_item(&items, 1, 1).is_none());
        assert!(move_item(&items, 0, 3).is_none());
        let ordered = order_by_ids(&items, &["c".into(), "x".into(), "a".into(), "c".into()]);
        assert_eq!(ids(&ordered), vec!["c", "a", "b"]);
    }
}
