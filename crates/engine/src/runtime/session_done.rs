//! Port of src/features/sessions/model/sessionDone.ts: sessions that
//! finished while unfocused, until the user looks at them.

use std::collections::HashSet;

/// `nextUnseenFinishedSessions`. `untracked_ids` are sessions the user
/// cannot focus or see as done, such as orchestration workers and inbox
/// discussions. An unseen session stays loaded until it is looked at, so
/// marking one of these would keep it in memory for good.
pub fn next_unseen_finished_sessions(
    previous_busy_ids: &HashSet<String>,
    busy_ids: &HashSet<String>,
    previous_unseen_ids: &HashSet<String>,
    focused_session_id: Option<&str>,
    untracked_ids: &HashSet<String>,
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
    next.retain(|id| !untracked_ids.contains(id));
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
            next_unseen_finished_sessions(&set(&["a"]), &set(&[]), &set(&[]), Some("b"), &set(&[])),
            set(&["a"])
        );
    }

    #[test]
    fn does_not_mark_a_session_done_when_it_finishes_while_focused() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&["a"]), &set(&[]), &set(&[]), Some("a"), &set(&[])),
            set(&[])
        );
    }

    #[test]
    fn clears_done_when_the_session_is_focused() {
        assert_eq!(
            next_unseen_finished_sessions(&set(&[]), &set(&[]), &set(&["a"]), Some("a"), &set(&[])),
            set(&[])
        );
    }

    #[test]
    fn clears_done_when_the_session_starts_working_again() {
        assert_eq!(
            next_unseen_finished_sessions(
                &set(&[]),
                &set(&["a"]),
                &set(&["a"]),
                Some("b"),
                &set(&[])
            ),
            set(&[])
        );
    }

    #[test]
    fn keeps_done_on_other_sessions_while_one_is_focused() {
        assert_eq!(
            next_unseen_finished_sessions(
                &set(&[]),
                &set(&[]),
                &set(&["a", "b"]),
                Some("a"),
                &set(&[])
            ),
            set(&["b"])
        );
    }

    #[test]
    fn never_marks_sessions_the_user_cannot_look_at() {
        // An unseen session stays loaded until focused; a worker never is.
        assert_eq!(
            next_unseen_finished_sessions(
                &set(&["lead", "worker"]),
                &set(&[]),
                &set(&[]),
                Some("other"),
                &set(&["worker"]),
            ),
            set(&["lead"])
        );
    }

    #[test]
    fn drops_untracked_sessions_that_were_already_marked() {
        assert_eq!(
            next_unseen_finished_sessions(
                &set(&[]),
                &set(&[]),
                &set(&["lead", "worker"]),
                Some("other"),
                &set(&["worker"]),
            ),
            set(&["lead"])
        );
    }
}
