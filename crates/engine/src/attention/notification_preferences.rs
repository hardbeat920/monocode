//! Port of src/features/notifications/model/notificationPreferences.ts:
//! per-project notification categories, mutes, and the time rule native
//! delivery shares.
//!
//! localStorage becomes `Kv` with the same key and JSON. `Date.now()` is a
//! `now` argument. `subscribeNotificationPreferences` becomes the
//! `Notifier` entity, which re-notifies on a store change and when the next
//! mute deadline passes (`next_mute_deadline`).

use std::collections::BTreeMap;

use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The preferences key.
pub const PROJECT_NOTIFICATIONS_KEY: &str = "monocode.projectNotifications.v1";

/// `NotificationCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NotificationCategory {
    #[serde(rename = "pullRequests")]
    PullRequests,
    #[serde(rename = "issues")]
    Issues,
    #[serde(rename = "agentFinished")]
    AgentFinished,
    #[serde(rename = "agentInput")]
    AgentInput,
    #[serde(rename = "reminders")]
    Reminders,
}

impl NotificationCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            NotificationCategory::PullRequests => "pullRequests",
            NotificationCategory::Issues => "issues",
            NotificationCategory::AgentFinished => "agentFinished",
            NotificationCategory::AgentInput => "agentInput",
            NotificationCategory::Reminders => "reminders",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            NotificationCategory::PullRequests => "Pull requests / Merge requests",
            NotificationCategory::Issues => "Issues and Linear tasks",
            NotificationCategory::AgentFinished => "Agent finished",
            NotificationCategory::AgentInput => "Agent approvals and questions",
            NotificationCategory::Reminders => "Reminders",
        }
    }
}

/// `NOTIFICATION_CATEGORIES`, in display order.
pub const NOTIFICATION_CATEGORIES: [NotificationCategory; 5] = [
    NotificationCategory::PullRequests,
    NotificationCategory::Issues,
    NotificationCategory::AgentFinished,
    NotificationCategory::AgentInput,
    NotificationCategory::Reminders,
];

/// `NOTIFICATION_MUTE_HOURS`.
pub const NOTIFICATION_MUTE_HOURS: [i64; 3] = [1, 4, 8];

/// `NotificationSubject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSubject {
    pub project_id: String,
    pub category: NotificationCategory,
    pub occurred_at: Option<i64>,
}

/// `mutedUntil` when present: a deadline, or `null` (muted until resumed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mute {
    UntilResumed,
    Until(i64),
}

/// `ProjectNotificationPreference`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectNotificationPreference {
    pub disabled: Vec<NotificationCategory>,
    /// `None` means no override.
    pub muted_until: Option<Mute>,
    /// Suppress delayed activity from before a manual resume.
    pub resumed_at: Option<i64>,
    pub enabled_after: Option<BTreeMap<NotificationCategory, i64>>,
}

/// `Partial<ProjectNotificationPreference>` for `updateNotificationPreferences`.
/// `muted_until: Some(None)` is an explicit `mutedUntil: undefined`, which
/// resumes a muted project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreferencePatch {
    pub disabled: Option<Vec<NotificationCategory>>,
    pub muted_until: Option<Option<Mute>>,
}

impl PreferencePatch {
    pub fn disabled(disabled: Vec<NotificationCategory>) -> Self {
        Self {
            disabled: Some(disabled),
            muted_until: None,
        }
    }

    pub fn mute(mute: Option<Mute>) -> Self {
        Self {
            disabled: None,
            muted_until: Some(mute),
        }
    }
}

/// Every project's preference, by project id.
pub type Preferences = BTreeMap<String, ProjectNotificationPreference>;

/// `ProjectNotificationRule`: `after` is the last suppressed millisecond,
/// shared with native delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectNotificationRule {
    pub enabled: bool,
    pub after: i64,
}

/// `getProjectNotificationRule`.
pub fn get_project_notification_rule(
    kv: &Kv,
    project_id: &str,
    category: NotificationCategory,
) -> ProjectNotificationRule {
    rule_for(load_notification_preferences(kv).get(project_id), category)
}

fn rule_for(
    preference: Option<&ProjectNotificationPreference>,
    category: NotificationCategory,
) -> ProjectNotificationRule {
    let Some(preference) = preference else {
        return ProjectNotificationRule {
            enabled: true,
            after: 0,
        };
    };
    // `(mutedUntil ?? 1) - 1`: a scheduled event at the deadline belongs to
    // the resumed interval. A null mute is nullish, so it counts as 1 too.
    let mute_after = match preference.muted_until {
        Some(Mute::Until(deadline)) => deadline - 1,
        None | Some(Mute::UntilResumed) => 0,
    };
    let enabled_after = preference
        .enabled_after
        .as_ref()
        .and_then(|after| after.get(&category).copied())
        .unwrap_or(0);
    ProjectNotificationRule {
        enabled: preference.muted_until != Some(Mute::UntilResumed)
            && !preference.disabled.contains(&category),
        after: preference
            .resumed_at
            .unwrap_or(0)
            .max(mute_after)
            .max(enabled_after),
    }
}

fn valid_timestamp(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    (number.is_finite() && number >= 0.0).then_some(number as i64)
}

/// `loadNotificationPreferences`: malformed entries are dropped without
/// losing valid project choices.
pub fn load_notification_preferences(kv: &Kv) -> Preferences {
    let raw = kv.get_item(PROJECT_NOTIFICATIONS_KEY);
    let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(raw.as_deref().unwrap_or("{}"))
    else {
        return Preferences::new();
    };
    let mut preferences = Preferences::new();
    for (id, value) in parsed {
        let Value::Object(value) = value else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let disabled = match value.get("disabled") {
            Some(Value::Array(items)) => NOTIFICATION_CATEGORIES
                .into_iter()
                .filter(|category| {
                    items
                        .iter()
                        .any(|item| item.as_str() == Some(category.as_str()))
                })
                .collect(),
            _ => Vec::new(),
        };
        let muted_until = match value.get("mutedUntil") {
            Some(Value::Null) => Some(Mute::UntilResumed),
            other => valid_timestamp(other).map(Mute::Until),
        };
        let enabled_after: BTreeMap<NotificationCategory, i64> = match value.get("enabledAfter") {
            Some(Value::Object(after)) => NOTIFICATION_CATEGORIES
                .into_iter()
                .filter_map(|category| {
                    Some((category, valid_timestamp(after.get(category.as_str()))?))
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        preferences.insert(
            id,
            ProjectNotificationPreference {
                disabled,
                muted_until,
                resumed_at: valid_timestamp(value.get("resumedAt")),
                enabled_after: (!enabled_after.is_empty()).then_some(enabled_after),
            },
        );
    }
    preferences
}

fn preference_json(preference: &ProjectNotificationPreference) -> Value {
    let mut out = Map::new();
    out.insert(
        "disabled".into(),
        Value::Array(
            preference
                .disabled
                .iter()
                .map(|category| Value::String(category.as_str().into()))
                .collect(),
        ),
    );
    match preference.muted_until {
        Some(Mute::UntilResumed) => {
            out.insert("mutedUntil".into(), Value::Null);
        }
        Some(Mute::Until(deadline)) => {
            out.insert("mutedUntil".into(), Value::from(deadline));
        }
        None => {}
    }
    if let Some(resumed_at) = preference.resumed_at {
        out.insert("resumedAt".into(), Value::from(resumed_at));
    }
    if let Some(after) = &preference.enabled_after {
        let after: Map<String, Value> = after
            .iter()
            .map(|(category, at)| (category.as_str().to_string(), Value::from(*at)))
            .collect();
        out.insert("enabledAfter".into(), Value::Object(after));
    }
    Value::Object(out)
}

/// `updateNotificationPreferences`. The `Kv` change replaces the change
/// event the TypeScript dispatched.
pub fn update_notification_preferences(
    kv: &Kv,
    project_ids: &[&str],
    patch: &PreferencePatch,
    now: i64,
) {
    let mut preferences = load_notification_preferences(kv);
    for &id in project_ids {
        let previous = preferences.get(id).cloned();
        let enabled: Vec<NotificationCategory> = match &patch.disabled {
            None => Vec::new(),
            Some(disabled) => previous
                .iter()
                .flat_map(|previous| previous.disabled.iter().copied())
                .filter(|category| !disabled.contains(category))
                .collect(),
        };
        let enabled_after = if enabled.is_empty() {
            previous
                .as_ref()
                .and_then(|previous| previous.enabled_after.clone())
        } else {
            let mut after = previous
                .as_ref()
                .and_then(|previous| previous.enabled_after.clone())
                .unwrap_or_default();
            for category in enabled {
                after.insert(category, now);
            }
            Some(after)
        };
        let resuming = matches!(patch.muted_until, Some(None))
            && previous
                .as_ref()
                .is_some_and(|previous| previous.muted_until.is_some());
        let mut next = previous.unwrap_or_default();
        if let Some(disabled) = &patch.disabled {
            next.disabled = disabled.clone();
        }
        if let Some(muted_until) = patch.muted_until {
            next.muted_until = muted_until;
        }
        if resuming {
            next.resumed_at = Some(now);
        }
        if enabled_after.is_some() {
            next.enabled_after = enabled_after;
        }
        preferences.insert(id.to_string(), next);
    }
    let json: Map<String, Value> = preferences
        .iter()
        .map(|(id, preference)| (id.clone(), preference_json(preference)))
        .collect();
    kv.set_item(PROJECT_NOTIFICATIONS_KEY, &Value::Object(json).to_string());
}

/// `allowsProjectNotification`.
pub fn allows_project_notification(kv: &Kv, subject: &NotificationSubject, now: i64) -> bool {
    let rule = get_project_notification_rule(kv, &subject.project_id, subject.category);
    rule.enabled
        && now > rule.after
        && subject
            .occurred_at
            .is_none_or(|occurred_at| occurred_at > rule.after)
}

/// `isProjectMuted`.
pub fn is_project_muted(preference: &ProjectNotificationPreference, now: i64) -> bool {
    match preference.muted_until {
        Some(Mute::UntilResumed) => true,
        Some(Mute::Until(deadline)) => deadline > now,
        None => false,
    }
}

/// `allowsProjectNotificationIndicator`: indicators follow the current
/// preferences; unread history is never consumed.
pub fn allows_project_notification_indicator(
    project_id: &str,
    category: NotificationCategory,
    preferences: &Preferences,
    now: i64,
) -> bool {
    preferences.get(project_id).is_none_or(|preference| {
        !is_project_muted(preference, now) && !preference.disabled.contains(&category)
    })
}

/// `notificationPreferencesSnapshot`: includes the effective mute state so
/// controls update when a deadline passes.
pub fn notification_preferences_snapshot(kv: &Kv, now: i64) -> String {
    let preferences = load_notification_preferences(kv);
    let json: Map<String, Value> = preferences
        .iter()
        .map(|(id, preference)| (id.clone(), preference_json(preference)))
        .collect();
    let muted: Vec<Value> = preferences
        .iter()
        .filter(|(_, preference)| is_project_muted(preference, now))
        .map(|(id, _)| Value::String(id.clone()))
        .collect();
    serde_json::json!({ "preferences": json, "muted": muted }).to_string()
}

/// The schedule half of `subscribeNotificationPreferences`: the earliest
/// timed mute still ahead of `now`, when listeners must hear a change.
pub fn next_mute_deadline(preferences: &Preferences, now: i64) -> Option<i64> {
    preferences
        .values()
        .filter_map(|preference| match preference.muted_until {
            Some(Mute::Until(deadline)) if deadline > now => Some(deadline),
            _ => None,
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use NotificationCategory::*;

    fn subject(
        project_id: &str,
        category: NotificationCategory,
        occurred_at: Option<i64>,
    ) -> NotificationSubject {
        NotificationSubject {
            project_id: project_id.into(),
            category,
            occurred_at,
        }
    }

    #[test]
    fn temporarily_silences_the_whole_project_and_restores_only_its_selected_categories_at_expiry()
    {
        let kv = Kv::in_memory();
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::disabled(vec![Issues, AgentFinished, AgentInput]),
            0,
        );
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::mute(Some(Mute::Until(5000))),
            0,
        );
        assert!(!allows_project_notification(
            &kv,
            &subject("private", PullRequests, None),
            4999
        ));
        assert!(allows_project_notification(
            &kv,
            &subject("work", PullRequests, None),
            4999
        ));
        assert!(allows_project_notification(
            &kv,
            &subject("private", PullRequests, None),
            5000
        ));
        assert!(!allows_project_notification(
            &kv,
            &subject("private", Issues, None),
            5000
        ));
    }

    #[test]
    fn allows_an_event_scheduled_exactly_at_the_mute_deadline() {
        let kv = Kv::in_memory();
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::mute(Some(Mute::Until(2000))),
            0,
        );
        assert!(allows_project_notification(
            &kv,
            &subject("private", Reminders, Some(2000)),
            2000
        ));
    }

    #[test]
    fn gives_native_delivery_the_same_category_and_time_rule() {
        let kv = Kv::in_memory();
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch {
                disabled: Some(vec![Issues]),
                muted_until: Some(Some(Mute::Until(5000))),
            },
            0,
        );
        assert_eq!(
            get_project_notification_rule(&kv, "private", Reminders),
            ProjectNotificationRule {
                enabled: true,
                after: 4999
            }
        );
        assert_eq!(
            get_project_notification_rule(&kv, "private", Issues),
            ProjectNotificationRule {
                enabled: false,
                after: 4999
            }
        );
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::mute(Some(Mute::UntilResumed)),
            0,
        );
        assert!(!get_project_notification_rule(&kv, "private", Reminders).enabled);
    }

    #[test]
    fn re_enabling_one_category_resumes_new_events_without_replaying_its_delayed_history() {
        let kv = Kv::in_memory();
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::disabled(vec![Issues]),
            1000,
        );
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::disabled(vec![]),
            2000,
        );
        assert!(!allows_project_notification(
            &kv,
            &subject("private", Issues, Some(1500)),
            2100
        ));
        assert!(allows_project_notification(
            &kv,
            &subject("private", PullRequests, Some(1500)),
            2100
        ));
        assert!(allows_project_notification(
            &kv,
            &subject("private", Issues, Some(2050)),
            2100
        ));
    }

    #[test]
    fn does_not_replay_activity_from_a_mute_period_after_expiry_or_manual_resume() {
        let kv = Kv::in_memory();
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::mute(Some(Mute::Until(2000))),
            1000,
        );
        assert!(!allows_project_notification(
            &kv,
            &subject("private", Issues, Some(1500)),
            2100
        ));
        assert!(allows_project_notification(
            &kv,
            &subject("private", Issues, Some(2050)),
            2100
        ));
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::mute(Some(Mute::UntilResumed)),
            1000,
        );
        update_notification_preferences(&kv, &["private"], &PreferencePatch::mute(None), 3000);
        assert!(!allows_project_notification(
            &kv,
            &subject("private", Issues, Some(2900)),
            3100
        ));
        assert!(allows_project_notification(
            &kv,
            &subject("private", Issues, Some(3050)),
            3100
        ));
    }

    #[test]
    fn the_snapshot_changes_when_a_mute_expires() {
        let kv = Kv::in_memory();
        update_notification_preferences(
            &kv,
            &["private"],
            &PreferencePatch::mute(Some(Mute::Until(2000))),
            1000,
        );
        let muted = notification_preferences_snapshot(&kv, 1000);
        assert_eq!(
            next_mute_deadline(&load_notification_preferences(&kv), 1000),
            Some(2000)
        );
        assert_ne!(notification_preferences_snapshot(&kv, 2000), muted);
        assert_eq!(
            next_mute_deadline(&load_notification_preferences(&kv), 2000),
            None
        );
    }

    #[test]
    fn ignores_malformed_persisted_entries_without_losing_valid_project_choices() {
        let kv = Kv::in_memory();
        kv.set_item(
            PROJECT_NOTIFICATIONS_KEY,
            &serde_json::json!({
                "private": { "disabled": ["issues", "future-category", 5], "mutedUntil": "forever" },
                "broken": null,
            })
            .to_string(),
        );
        let preferences = load_notification_preferences(&kv);
        assert_eq!(preferences.len(), 1);
        assert_eq!(
            preferences["private"],
            ProjectNotificationPreference {
                disabled: vec![Issues],
                ..Default::default()
            }
        );
        assert!(allows_project_notification(
            &kv,
            &subject("broken", Issues, None),
            1
        ));
    }

    #[test]
    fn writes_the_typescript_json_shape() {
        let kv = Kv::in_memory();
        update_notification_preferences(&kv, &["a"], &PreferencePatch::disabled(vec![Issues]), 10);
        update_notification_preferences(
            &kv,
            &["a"],
            &PreferencePatch::mute(Some(Mute::UntilResumed)),
            20,
        );
        update_notification_preferences(&kv, &["a"], &PreferencePatch::disabled(vec![]), 30);
        update_notification_preferences(&kv, &["a"], &PreferencePatch::mute(None), 40);
        let stored: Value =
            serde_json::from_str(&kv.get_item(PROJECT_NOTIFICATIONS_KEY).unwrap()).unwrap();
        assert_eq!(
            stored,
            serde_json::json!({
                "a": { "disabled": [], "resumedAt": 40, "enabledAfter": { "issues": 30 } },
            })
        );
    }
}
