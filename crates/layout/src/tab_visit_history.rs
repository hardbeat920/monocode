//! Port of src/features/workspace/model/tabVisitHistory.ts: a browser-style
//! visit stack for workspace tabs.

use std::collections::HashSet;

/// `MAX_STACK`.
const MAX_STACK: usize = 50;

/// `TabVisitHistory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabVisitHistory {
    pub back: Vec<String>,
    pub forward: Vec<String>,
    pub current: String,
}

/// `emptyTabVisitHistory`.
pub fn empty_tab_visit_history(current: &str) -> TabVisitHistory {
    TabVisitHistory {
        back: Vec::new(),
        forward: Vec::new(),
        current: current.to_string(),
    }
}

/// `canTabVisitBack`.
pub fn can_tab_visit_back(history: &TabVisitHistory) -> bool {
    !history.back.is_empty()
}

/// `canTabVisitForward`.
pub fn can_tab_visit_forward(history: &TabVisitHistory) -> bool {
    !history.forward.is_empty()
}

/// `recordTabVisit`.
pub fn record_tab_visit(history: &TabVisitHistory, id: &str) -> TabVisitHistory {
    if history.current == id {
        return history.clone();
    }
    TabVisitHistory {
        back: push_visit(&history.back, &history.current),
        forward: Vec::new(),
        current: id.to_string(),
    }
}

/// `tabVisitBack`.
pub fn tab_visit_back(history: &TabVisitHistory) -> Option<TabVisitHistory> {
    let (id, back) = history.back.split_last()?;
    let mut forward = vec![history.current.clone()];
    forward.extend(history.forward.iter().cloned());
    Some(TabVisitHistory {
        back: back.to_vec(),
        forward,
        current: id.clone(),
    })
}

/// `tabVisitForward`.
pub fn tab_visit_forward(history: &TabVisitHistory) -> Option<TabVisitHistory> {
    let (id, forward) = history.forward.split_first()?;
    Some(TabVisitHistory {
        back: push_visit(&history.back, &history.current),
        forward: forward.to_vec(),
        current: id.clone(),
    })
}

/// `pruneTabVisitHistory`: drop closed tabs and snap `current` onto an open id.
pub fn prune_tab_visit_history(
    history: &TabVisitHistory,
    open_ids: &HashSet<String>,
    active_id: &str,
) -> TabVisitHistory {
    let back = drop_missing(&history.back, open_ids);
    let forward = drop_missing(&history.forward, open_ids);
    let current = if open_ids.contains(&history.current) {
        history.current.clone()
    } else if open_ids.contains(active_id) {
        active_id.to_string()
    } else {
        back.last()
            .or(forward.first())
            .cloned()
            .unwrap_or_else(|| active_id.to_string())
    };
    collapse_adjacent(TabVisitHistory {
        back,
        forward,
        current,
    })
}

/// `pushVisit`.
fn push_visit(stack: &[String], id: &str) -> Vec<String> {
    if stack.last().map(String::as_str) == Some(id) {
        return stack.to_vec();
    }
    let mut next = stack.to_vec();
    next.push(id.to_string());
    if next.len() > MAX_STACK {
        next.drain(..next.len() - MAX_STACK);
    }
    next
}

/// `dropMissing`.
fn drop_missing(stack: &[String], open_ids: &HashSet<String>) -> Vec<String> {
    stack
        .iter()
        .filter(|id| open_ids.contains(*id))
        .cloned()
        .collect()
}

/// `collapseAdjacent`.
fn collapse_adjacent(history: TabVisitHistory) -> TabVisitHistory {
    let mut back = history.back;
    while back.last() == Some(&history.current) {
        back.pop();
    }
    let mut forward = history.forward;
    while forward.first() == Some(&history.current) {
        forward.remove(0);
    }
    TabVisitHistory {
        back,
        forward,
        current: history.current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn returns_to_the_previous_tab() {
        let history = record_tab_visit(&empty_tab_visit_history("a"), "b");
        assert!(can_tab_visit_back(&history));
        assert!(!can_tab_visit_forward(&history));

        let history = tab_visit_back(&history).unwrap();
        assert_eq!(history.current, "a");
        assert!(!can_tab_visit_back(&history));
        assert!(can_tab_visit_forward(&history));
    }

    #[test]
    fn restores_the_tab_after_going_back() {
        let history = record_tab_visit(&empty_tab_visit_history("a"), "b");
        let history = tab_visit_back(&history).unwrap();
        let history = tab_visit_forward(&history).unwrap();
        assert_eq!(history.current, "b");
        assert!(!can_tab_visit_forward(&history));
    }

    #[test]
    fn drops_the_forward_stack_when_visiting_a_new_tab_after_back() {
        let history = record_tab_visit(&empty_tab_visit_history("a"), "b");
        let history = tab_visit_back(&history).unwrap();
        let history = record_tab_visit(&history, "c");
        assert_eq!(history.current, "c");
        assert!(!can_tab_visit_forward(&history));
        assert_eq!(
            tab_visit_back(&history).map(|h| h.current).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn ignores_recording_the_tab_already_current() {
        let start = empty_tab_visit_history("a");
        assert_eq!(record_tab_visit(&start, "a"), start);
    }

    #[test]
    fn does_not_keep_a_closed_current_tab_on_the_back_stack() {
        let history = prune_tab_visit_history(
            &record_tab_visit(&empty_tab_visit_history("a"), "b"),
            &open(&["a"]),
            "a",
        );
        assert_eq!(history, empty_tab_visit_history("a"));
        assert!(!can_tab_visit_back(&history));
    }

    #[test]
    fn skips_a_closed_tab_in_the_middle_of_the_stack() {
        let history = record_tab_visit(&empty_tab_visit_history("a"), "b");
        let history = record_tab_visit(&history, "c");
        let history = prune_tab_visit_history(&history, &open(&["a", "c"]), "c");
        assert_eq!(
            tab_visit_back(&history).map(|h| h.current).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn collapses_a_round_trip_once_the_other_tab_closes() {
        let history = record_tab_visit(&empty_tab_visit_history("a"), "b");
        let history = record_tab_visit(&history, "a");
        let history = prune_tab_visit_history(&history, &open(&["a"]), "a");
        assert_eq!(history, empty_tab_visit_history("a"));
    }

    #[test]
    fn caps_the_back_stack() {
        let mut history = empty_tab_visit_history("t0");
        for i in 1..=60 {
            history = record_tab_visit(&history, &format!("t{i}"));
        }
        assert_eq!(history.back.len(), MAX_STACK);
        assert_eq!(history.back[0], "t10");
    }
}
