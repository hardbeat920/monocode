//! Port of `moveItem` from src/shared/lib/reorder.ts.

/// `moveItem`: `items` with the entry at `from` moved to `to`. Out of range
/// or equal indices return an unchanged copy.
pub fn move_item<T: Clone>(items: &[T], from: usize, to: usize) -> Vec<T> {
    if from == to || from >= items.len() || to >= items.len() {
        return items.to_vec();
    }
    let mut next = items.to_vec();
    let item = next.remove(from);
    next.insert(to, item);
    next
}

#[cfg(test)]
mod tests {
    use super::move_item;

    #[test]
    fn moves_an_entry_forward_and_back() {
        assert_eq!(move_item(&["a", "b", "c"], 0, 2), ["b", "c", "a"]);
        assert_eq!(move_item(&["a", "b", "c"], 2, 0), ["c", "a", "b"]);
        assert_eq!(move_item(&["a", "b", "c"], 1, 1), ["a", "b", "c"]);
        assert_eq!(move_item(&["a", "b", "c"], 1, 5), ["a", "b", "c"]);
    }
}
