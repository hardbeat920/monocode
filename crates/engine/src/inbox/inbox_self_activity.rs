//! Port of src/features/inbox/model/inboxSelfActivity.ts: the mutations this
//! app made, so the revision they produce is not announced back to its author.
//!
//! The module-level list becomes `InboxSelfActivity`, owned by the
//! `InboxClient`. Listeners become the client's `InboxSignal::SelfActivity`.

use super::text::normalized;
use super::types::{InboxItem, InboxKind, InboxProvider};
use crate::runtime::util::project_path::same_project_path;

/// `MAX_PENDING_AGE_MS`.
pub const MAX_PENDING_AGE_MS: i64 = 10 * 60_000;

/// `InboxSelfActivityTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxSelfActivityTarget {
    pub provider: InboxProvider,
    pub kind: Option<InboxKind>,
    pub repo: Option<String>,
    pub number: Option<i64>,
    pub id: Option<String>,
    pub project_path: Option<String>,
}

impl InboxSelfActivityTarget {
    pub fn new(provider: InboxProvider) -> Self {
        Self {
            provider,
            kind: None,
            repo: None,
            number: None,
            id: None,
            project_path: None,
        }
    }

    /// `{ provider, kind, repo, number }`.
    pub fn work_item(provider: InboxProvider, kind: InboxKind, repo: &str, number: i64) -> Self {
        Self {
            kind: Some(kind),
            repo: Some(repo.to_string()),
            number: Some(number),
            ..Self::new(provider)
        }
    }

    /// `{ provider, kind, id }`.
    pub fn tracker(provider: InboxProvider, kind: InboxKind, id: &str) -> Self {
        Self {
            kind: Some(kind),
            id: Some(id.to_string()),
            ..Self::new(provider)
        }
    }
}

#[derive(Debug, Clone)]
struct PendingActivity {
    target: InboxSelfActivityTarget,
    recorded_at: i64,
}

/// The pending mutations.
#[derive(Debug, Default)]
pub struct InboxSelfActivity {
    pending: Vec<PendingActivity>,
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

fn matches(activity: &InboxSelfActivityTarget, item: &InboxItem) -> bool {
    if activity.provider != item.provider {
        return false;
    }
    if let Some(id) = non_empty(&activity.id)
        && Some(id) != item.id.as_deref()
    {
        return false;
    }
    if let Some(kind) = activity.kind
        && kind != item.kind
    {
        return false;
    }
    if let Some(number) = activity.number
        && number != item.number
    {
        return false;
    }
    if let Some(repo) = non_empty(&activity.repo)
        && normalized(Some(repo)) != normalized(Some(&item.repo))
    {
        return false;
    }
    if let Some(path) = non_empty(&activity.project_path)
        && !same_project_path(path, &item.project_path)
    {
        return false;
    }
    non_empty(&activity.id).is_some()
        || non_empty(&activity.repo).is_some()
        || activity.number.is_some()
        || non_empty(&activity.project_path).is_some()
}

impl InboxSelfActivity {
    fn prune(&mut self, now: i64) {
        let oldest = now - MAX_PENDING_AGE_MS;
        self.pending
            .retain(|activity| activity.recorded_at >= oldest);
    }

    /// `recordInboxSelfActivity` without the listener call, which the client
    /// makes.
    pub fn record(&mut self, target: InboxSelfActivityTarget, now: i64) {
        self.prune(now);
        self.pending.push(PendingActivity {
            target,
            recorded_at: now,
        });
    }

    /// `consumeInboxSelfActivity`: consume every coalesced mutation for this
    /// revision. A later revision can notify normally.
    pub fn consume(&mut self, item: &InboxItem, now: i64) -> bool {
        self.prune(now);
        let before = self.pending.len();
        self.pending
            .retain(|activity| !matches(&activity.target, item));
        self.pending.len() != before
    }

    /// `clearPendingInboxSelfActivity`.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::github_tasks::test_items::{item, with};

    fn github_pr() -> InboxItem {
        with(item(42, "2026-09-13T12:00:00Z"), |row| {
            row.kind = InboxKind::Pr;
            row.repo = "acme/app".into();
            row.project_path = "/tmp/app".into();
        })
    }

    #[test]
    fn coalesces_exact_mutations_into_one_acknowledged_revision() {
        let mut activity = InboxSelfActivity::default();
        activity.record(
            InboxSelfActivityTarget::work_item(
                InboxProvider::Github,
                InboxKind::Pr,
                "ACME/App",
                42,
            ),
            0,
        );
        activity.record(
            InboxSelfActivityTarget::work_item(
                InboxProvider::Github,
                InboxKind::Pr,
                "acme/app",
                42,
            ),
            0,
        );
        assert!(activity.consume(&github_pr(), 0));
        assert!(!activity.consume(&github_pr(), 0));
    }

    #[test]
    fn does_not_consume_activity_for_another_item() {
        let mut activity = InboxSelfActivity::default();
        activity.record(
            InboxSelfActivityTarget {
                kind: Some(InboxKind::Issue),
                number: Some(42),
                project_path: Some("/tmp/app".into()),
                ..InboxSelfActivityTarget::new(InboxProvider::Github)
            },
            0,
        );
        assert!(!activity.consume(&github_pr(), 0));
    }

    #[test]
    fn matches_linear_mutations_by_id() {
        let mut activity = InboxSelfActivity::default();
        activity.record(
            InboxSelfActivityTarget::tracker(InboxProvider::Linear, InboxKind::Linear, "lin-7"),
            0,
        );
        let linear = with(item(7, "2026-09-13T12:00:00Z"), |row| {
            row.provider = InboxProvider::Linear;
            row.kind = InboxKind::Linear;
            row.id = Some("lin-7".into());
            row.repo = "ENG".into();
            row.project_path = "linear:team".into();
        });
        assert!(activity.consume(&linear, 0));
    }

    #[test]
    fn expires_a_mutation_instead_of_hiding_unrelated_later_activity() {
        let mut activity = InboxSelfActivity::default();
        activity.record(
            InboxSelfActivityTarget {
                project_path: Some("/tmp/app".into()),
                ..InboxSelfActivityTarget::work_item(
                    InboxProvider::Github,
                    InboxKind::Pr,
                    "acme/app",
                    42,
                )
            },
            1_000,
        );
        assert!(!activity.consume(&github_pr(), 1_000 + 10 * 60_000 + 1));
    }
}
