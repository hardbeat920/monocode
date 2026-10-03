//! Port of src/features/inbox/model/inboxNotifications.ts: the notification
//! subject of an inbox item, and the tracker that tells new and changed
//! revisions apart from the user's read state.
//!
//! `inboxNotificationProject` belongs to the attention package, so the
//! project id comes in as a function (`InboxHooks::notification_project_id`).

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::time::date_parse;
use super::types::{INBOX_PROVIDERS, InboxItem, InboxKind, InboxProvider, kind_str};

/// `NotificationSubject["category"]` for inbox items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InboxNotificationCategory {
    Issues,
    PullRequests,
}

impl InboxNotificationCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issues => "issues",
            Self::PullRequests => "pullRequests",
        }
    }
}

/// `NotificationSubject`: the project, category, and time a preference
/// check reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxNotificationSubject {
    pub project_id: String,
    pub category: InboxNotificationCategory,
    /// `Date.parse(item.updatedAt)`; `None` stands for `NaN`.
    pub occurred_at: Option<i64>,
}

/// `inboxNotificationSubject`.
pub fn inbox_notification_subject(
    project_id: String,
    kind: InboxKind,
    updated_at: &str,
) -> InboxNotificationSubject {
    InboxNotificationSubject {
        project_id,
        category: if kind == InboxKind::Pr {
            InboxNotificationCategory::PullRequests
        } else {
            InboxNotificationCategory::Issues
        },
        occurred_at: date_parse(updated_at),
    }
}

/// `InboxObservation`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InboxObservation {
    pub changed: Vec<InboxItem>,
    pub appeared: Vec<InboxItem>,
}

/// `InboxNotificationTracker`: fetched revisions, kept apart from the
/// user's read state.
#[derive(Debug, Default)]
pub struct InboxNotificationTracker {
    revisions: HashMap<String, i64>,
    primed: HashSet<InboxProvider>,
    scope: Option<String>,
}

impl InboxNotificationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// `observe`. A provider rings only after one successful baseline, and a
    /// changed scope (a different query) starts a new baseline.
    pub fn observe(
        &mut self,
        items: &[InboxItem],
        scope: &str,
        failed_providers: &[InboxProvider],
        project_id: &dyn Fn(&InboxItem) -> String,
    ) -> InboxObservation {
        if self.scope.as_deref() != Some(scope) {
            self.primed.clear();
        }
        self.scope = Some(scope.to_string());
        let failed: HashSet<InboxProvider> = failed_providers.iter().copied().collect();
        let mut observation = InboxObservation::default();
        for item in items {
            if failed.contains(&item.provider) {
                continue;
            }
            // `item.id || item.number`.
            let identity = match item.id.as_deref().filter(|id| !id.is_empty()) {
                Some(id) => json!(id),
                None => json!(item.number),
            };
            let key = json!([project_id(item), kind_str(item.kind), identity]).to_string();
            let Some(updated_at) = date_parse(&item.updated_at) else {
                continue;
            };
            let previous = self.revisions.get(&key).copied();
            let primed = self.primed.contains(&item.provider);
            match previous {
                None if primed => {
                    observation.appeared.push(item.clone());
                    observation.changed.push(item.clone());
                }
                Some(previous) if primed && updated_at > previous => {
                    observation.changed.push(item.clone());
                }
                _ => {}
            }
            self.revisions
                .insert(key, previous.unwrap_or(0).max(updated_at));
        }
        for provider in INBOX_PROVIDERS {
            if !failed.contains(&provider) {
                self.primed.insert(provider);
            }
        }
        observation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::github_tasks::test_items::{item, with};

    fn pr(repo: &str, updated_at: &str) -> InboxItem {
        with(item(1, updated_at), |row| {
            row.kind = InboxKind::Pr;
            row.repo = repo.into();
            row.title = "PR".into();
            row.url = format!("https://github.com/{repo}/pull/1");
            row.project_path = format!("/tmp/{repo}");
        })
    }

    fn project(item: &InboxItem) -> String {
        format!("local:{}", item.project_path)
    }

    fn observation(changed: Vec<InboxItem>, appeared: Vec<InboxItem>) -> InboxObservation {
        InboxObservation { changed, appeared }
    }

    #[test]
    fn detects_a_new_revision_even_while_another_projects_activity_remains_unread() {
        let mut tracker = InboxNotificationTracker::new();
        let a = pr("acme/private", "2026-09-14T08:00:00Z");
        let b = pr("acme/work", "2026-09-14T08:01:00Z");
        assert_eq!(
            tracker.observe(std::slice::from_ref(&a), "all", &[], &project),
            observation(vec![], vec![])
        );
        assert_eq!(
            tracker.observe(&[a.clone(), b.clone()], "all", &[], &project),
            observation(vec![b.clone()], vec![b.clone()])
        );
        assert_eq!(
            tracker.observe(&[a.clone(), b.clone()], "all", &[], &project),
            observation(vec![], vec![])
        );
        let updated = with(b, |row| row.updated_at = "2026-09-14T08:02:00Z".into());
        assert_eq!(
            tracker.observe(&[a, updated.clone()], "all", &[], &project),
            observation(vec![updated], vec![])
        );
    }

    #[test]
    fn waits_for_a_successful_provider_baseline_instead_of_ringing_for_recovered_history() {
        let mut tracker = InboxNotificationTracker::new();
        let a = pr("acme/app", "2026-09-14T08:00:00Z");
        tracker.observe(&[], "all", &[InboxProvider::Github], &project);
        assert_eq!(
            tracker.observe(std::slice::from_ref(&a), "all", &[], &project),
            observation(vec![], vec![])
        );
        let changed = with(a, |row| row.updated_at = "2026-09-14T08:01:00Z".into());
        assert_eq!(
            tracker.observe(std::slice::from_ref(&changed), "all", &[], &project),
            observation(vec![changed], vec![])
        );
    }

    #[test]
    fn does_not_announce_history_exposed_by_a_changed_query_or_repeat_items_after_a_failed_or_partial_refresh()
     {
        let mut tracker = InboxNotificationTracker::new();
        let a = pr("acme/app", "2026-09-14T08:00:00Z");
        let history = with(a.clone(), |row| {
            row.number = 2;
            row.updated_at = "2026-08-01T08:00:00Z".into();
        });
        tracker.observe(std::slice::from_ref(&a), "open", &[], &project);
        assert_eq!(
            tracker.observe(&[a.clone(), history.clone()], "all", &[], &project),
            observation(vec![], vec![])
        );
        assert_eq!(
            tracker.observe(&[], "all", &[], &project),
            observation(vec![], vec![])
        );
        assert_eq!(
            tracker.observe(&[a.clone(), history], "all", &[], &project),
            observation(vec![], vec![])
        );
        let invalid = with(a, |row| row.updated_at = "invalid".into());
        assert_eq!(
            tracker.observe(&[invalid], "all", &[], &project),
            observation(vec![], vec![])
        );
    }

    #[test]
    fn subjects_name_the_category() {
        let subject = inbox_notification_subject("p".into(), InboxKind::Pr, "2026-09-14T08:00:00Z");
        assert_eq!(subject.category.as_str(), "pullRequests");
        assert!(subject.occurred_at.is_some());
        let subject = inbox_notification_subject("p".into(), InboxKind::Linear, "bad");
        assert_eq!(subject.category, InboxNotificationCategory::Issues);
        assert_eq!(subject.occurred_at, None);
    }
}
