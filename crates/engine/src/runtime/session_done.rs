//! Port of src/features/sessions/model/sessionDone.ts: sessions that
//! finished while unfocused, until the user looks at them.

use std::collections::HashSet;

/// `nextUnseenFinishedSessions`.
pub fn next_unseen_finished_sessions(
    previous_busy_ids: &HashSet<String>,
    busy_ids: &HashSet<String>,
    previous_unseen_ids: &HashSet<String>,
    focused_session_id: Option<&str>,
) -> HashSet<String> {
    let mut next = previous_unseen_ids.clone();
    for id in previous_busy_ids {
        if !busy_ids.contains(id) && Some(id.as_str()) != focused_session_id {
            next.insert(id.clone());
        }
    }
    for id in busy_ids {
        next.remove(id);
    }
    if let Some(focused) = focused_session_id {
        next.remove(focused);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn marks_a_session_done_when_it_finishes_while_unfocused() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&["a"]), &set(&[]), &set(&[]), Some("b")),
            set(&["a"])
        );
    }

    #[test]
    fn does_not_mark_a_session_done_when_it_finishes_while_focused() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&["a"]), &set(&[]), &set(&[]), Some("a")),
            set(&[])
        );
    }

    #[test]
    fn clears_done_when_the_session_is_focused() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&[]), &set(&[]), &set(&["a"]), Some("a")),
            set(&[])
        );
    }

    #[test]
    fn clears_done_when_the_session_starts_working_again() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&[]), &set(&["a"]), &set(&["a"]), Some("b")),
            set(&[])
        );
    }

    #[test]
    fn keeps_done_on_other_sessions_while_one_is_focused() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&[]), &set(&[]), &set(&["a", "b"]), Some("a")),
            set(&["b"])
        );
    }
}
