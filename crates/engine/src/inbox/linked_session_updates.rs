//! Port of src/features/inbox/model/linkedSessionUpdates.ts: which sessions
//! linked to a GitHub issue or pull request have remote activity newer than
//! their last local turn or acknowledged snapshot.

use std::collections::{HashMap, HashSet};

use monocode_core::js;
use monocode_core::session::LinkedWorkItem;

use super::time::date_parse;
use super::types::{GithubWorkItem, WorkItemKind, work_kind_str};
use crate::runtime::session_store::SessionSummary;

/// `LinkedWorkItemTarget`.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedWorkItemTarget {
    pub key: String,
    pub item: LinkedWorkItem,
}

/// `LinkedSessionUpdate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedSessionUpdate {
    pub session_id: String,
    pub item: GithubWorkItem,
    /// The last local turn or acknowledged remote snapshot, whichever is
    /// newer.
    pub since: i64,
    pub updated_at: i64,
}

/// `linkedWorkItemUpdateKey`.
pub fn linked_work_item_update_key(repo: &str, kind: WorkItemKind, number: i64) -> String {
    format!(
        "{}:{}:{}",
        js::trim(repo).to_lowercase(),
        work_kind_str(kind),
        number
    )
}

fn linked_key(item: &LinkedWorkItem) -> String {
    linked_work_item_update_key(&item.repo, item.kind, item.number)
}

/// `linkedWorkItemTargets`: one target per linked item across unarchived
/// sessions.
pub fn linked_work_item_targets(sessions: &[SessionSummary]) -> Vec<LinkedWorkItemTarget> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for session in sessions {
        let Some(linked) = session.linked_work_item.as_ref() else {
            continue;
        };
        if session.archived == Some(true) {
            continue;
        }
        let key = linked_key(linked);
        if seen.insert(key.clone()) {
            targets.push(LinkedWorkItemTarget {
                key,
                item: linked.clone(),
            });
        }
    }
    targets
}

/// `linkedSessionUpdates`: sessions whose GitHub item changed after the
/// last local turn or read snapshot, in session order.
pub fn linked_session_updates(
    sessions: &[SessionSummary],
    work_items: &HashMap<String, GithubWorkItem>,
    seen_at: &dyn Fn(&str) -> i64,
) -> Vec<LinkedSessionUpdate> {
    let mut updates: Vec<LinkedSessionUpdate> = Vec::new();
    for session in sessions {
        let Some(linked) = session.linked_work_item.as_ref() else {
            continue;
        };
        if session.archived == Some(true) {
            continue;
        }
        let Some(item) = work_items.get(&linked_key(linked)) else {
            continue;
        };
        let since = session.updated_at.max(seen_at(&session.id));
        let Some(remote_updated_at) = date_parse(&item.updated_at) else {
            continue;
        };
        if remote_updated_at > since {
            let update = LinkedSessionUpdate {
                session_id: session.id.clone(),
                item: item.clone(),
                since,
                updated_at: remote_updated_at,
            };
            // `Map.set` on a repeated id keeps the first position.
            match updates
                .iter_mut()
                .find(|entry| entry.session_id == session.id)
            {
                Some(entry) => *entry = update,
                None => updates.push(update),
            }
        }
    }
    updates
}

/// `linkedSessionUpdateIds`.
pub fn linked_session_update_ids(
    sessions: &[SessionSummary],
    work_items: &HashMap<String, GithubWorkItem>,
    seen_at: &dyn Fn(&str) -> i64,
) -> Vec<String> {
    linked_session_updates(sessions, work_items, seen_at)
        .into_iter()
        .map(|update| update.session_id)
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use monocode_core::Extra;
    use monocode_core::harness::{HarnessId, RuntimeMode};

    use super::*;
    use crate::inbox::time::to_iso_string;

    pub(crate) fn linked() -> LinkedWorkItem {
        LinkedWorkItem {
            kind: WorkItemKind::Pr,
            repo: "Acme/App".into(),
            number: 42,
            url: "https://github.com/Acme/App/pull/42".into(),
            extra: Extra::new(),
        }
    }

    fn remote(updated_at: i64) -> GithubWorkItem {
        GithubWorkItem {
            kind: WorkItemKind::Pr,
            number: 42,
            title: "Update sidebar activity".into(),
            url: "https://github.com/Acme/App/pull/42".into(),
            state: "open".into(),
            state_reason: None,
            created_at: None,
            updated_at: to_iso_string(updated_at),
            labels: vec![],
            assignees: vec![],
            draft: false,
            repo: "Acme/App".into(),
        }
    }

    pub(crate) fn session(id: &str, updated_at: i64) -> SessionSummary {
        let mut summary = SessionSummary::new(id, "/tmp/app", HarnessId::Codex);
        summary.model = "gpt-5".into();
        summary.runtime_mode = RuntimeMode::Supervised;
        summary.title = format!("codex · {id}");
        summary.created_at = 1;
        summary.updated_at = updated_at;
        summary.linked_work_item = Some(linked());
        summary
    }

    fn snapshots(item: &LinkedWorkItem, updated_at: i64) -> HashMap<String, GithubWorkItem> {
        HashMap::from([(linked_key(item), remote(updated_at))])
    }

    fn never(_: &str) -> i64 {
        0
    }

    #[test]
    fn marks_a_session_when_its_linked_item_changed_after_the_local_session() {
        assert_eq!(
            linked_session_update_ids(&[session("old", 100)], &snapshots(&linked(), 200), &never),
            ["old"]
        );
    }

    #[test]
    fn clears_naturally_once_the_session_advances_past_the_remote_update() {
        assert!(
            linked_session_update_ids(
                &[session("continued", 201)],
                &snapshots(&linked(), 200),
                &never
            )
            .is_empty()
        );
    }

    #[test]
    fn tracks_related_sessions_independently_and_ignores_archived_sessions() {
        let mut archived = session("archived", 100);
        archived.archived = Some(true);
        let mut unlinked = session("unlinked", 100);
        unlinked.linked_work_item = None;
        let ids = linked_session_update_ids(
            &[
                session("stale", 100),
                session("current", 250),
                archived,
                unlinked,
            ],
            &snapshots(&linked(), 200),
            &never,
        );
        assert_eq!(ids, ["stale"]);
    }

    #[test]
    fn normalizes_repository_case_and_deduplicates_lookup_targets() {
        let mut lower = linked();
        lower.repo = "acme/app".into();
        assert_eq!(
            linked_session_update_ids(&[session("same", 100)], &snapshots(&lower, 200), &never),
            ["same"]
        );
        let mut second = session("second", 150);
        second.linked_work_item = Some(lower);
        assert_eq!(
            linked_work_item_targets(&[session("first", 100), second]).len(),
            1
        );
    }

    #[test]
    fn uses_the_acknowledged_snapshot_as_the_next_activity_baseline() {
        let items = snapshots(&linked(), 200);
        assert!(linked_session_update_ids(&[session("read", 100)], &items, &|_| 200).is_empty());
        assert_eq!(
            linked_session_update_ids(&[session("newer", 100)], &items, &|_| 150),
            ["newer"]
        );
    }
}
