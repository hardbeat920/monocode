//! Port of src/features/sessions/model/sessionSelection.ts: the sidebar's
//! multi-select.
//!
//! A JS `Set` keeps insertion order, so the selection is a `Vec` without
//! repeats. The TypeScript returned the same set when pruning changed
//! nothing; here callers compare with `==`.

/// `toggleSessionSelection`.
pub fn toggle_session_selection(selected: &[String], session_id: &str) -> Vec<String> {
    let mut next = selected.to_vec();
    match next.iter().position(|id| id == session_id) {
        Some(index) => {
            next.remove(index);
        }
        None => next.push(session_id.to_string()),
    }
    next
}

/// `orderedSessionActionIds`: the whole selection, in list order, when the
/// clicked card is part of a multi-selection; otherwise just that card.
pub fn ordered_session_action_ids(
    clicked_session_id: &str,
    selected: &[String],
    ordered_session_ids: &[String],
) -> Vec<String> {
    if !selected.iter().any(|id| id == clicked_session_id) || selected.len() <= 1 {
        return vec![clicked_session_id.to_string()];
    }
    let ordered: Vec<String> = ordered_session_ids
        .iter()
        .filter(|id| selected.contains(id))
        .cloned()
        .collect();
    if ordered.is_empty() {
        vec![clicked_session_id.to_string()]
    } else {
        ordered
    }
}

/// `pruneSessionSelection`: drop selections that are no longer available.
pub fn prune_session_selection(
    selected: &[String],
    available: &std::collections::HashSet<String>,
) -> Vec<String> {
    selected
        .iter()
        .filter(|id| available.contains(*id))
        .cloned()
        .collect()
}

/// A selection with each id once, in first-seen order (`new Set([...])`).
pub fn unique_selection(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn strings(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn toggles_cards_without_mutating_the_current_selection() {
        let selected = strings(&["one"]);
        let added = toggle_session_selection(&selected, "two");
        let removed = toggle_session_selection(&added, "one");
        assert_eq!(selected, strings(&["one"]));
        assert_eq!(added, strings(&["one", "two"]));
        assert_eq!(removed, strings(&["two"]));
    }

    #[test]
    fn uses_the_whole_selection_when_its_card_opens_the_action_menu() {
        assert_eq!(
            ordered_session_action_ids(
                "one",
                &strings(&["three", "one"]),
                &strings(&["one", "two", "three"])
            ),
            strings(&["one", "three"])
        );
    }

    #[test]
    fn uses_only_an_unselected_card_when_it_opens_the_action_menu() {
        assert_eq!(
            ordered_session_action_ids(
                "two",
                &strings(&["one", "three"]),
                &strings(&["one", "two", "three"])
            ),
            strings(&["two"])
        );
    }

    #[test]
    fn drops_selections_that_are_no_longer_available() {
        let available: HashSet<String> = ["two".to_string()].into_iter().collect();
        assert_eq!(
            prune_session_selection(&strings(&["one", "two"]), &available),
            strings(&["two"])
        );
    }
}
