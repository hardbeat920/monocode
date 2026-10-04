//! The notification data the mute controls and the project notification
//! card draw. These mirror `notification_preferences.rs` and
//! `notification_projects.rs` in the engine's attention package, which owns
//! loading and saving them.
//!
//! Also a port of src/features/notifications/ui/notificationMuteActions.ts:
//! the mute presets, their labels, and the status line.

use std::collections::BTreeMap;

use chrono::{DateTime, Local, TimeZone as _};
use serde::{Deserialize, Serialize};

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

/// `mutedUntil` when present: a deadline, or `null` (muted until resumed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mute {
    UntilResumed,
    Until(i64),
}

/// The fields of `ProjectNotificationPreference` the views read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectNotificationPreference {
    pub disabled: Vec<NotificationCategory>,
    /// `None` means no override.
    pub muted_until: Option<Mute>,
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

/// `isProjectMuted`.
pub fn is_project_muted(preference: &ProjectNotificationPreference, now: i64) -> bool {
    match preference.muted_until {
        Some(Mute::UntilResumed) => true,
        Some(Mute::Until(deadline)) => deadline > now,
        None => false,
    }
}

/// The earliest timed mute still ahead of `now`, when the controls must
/// redraw (the schedule half of `subscribeNotificationPreferences`).
pub fn next_mute_deadline(preferences: &Preferences, now: i64) -> Option<i64> {
    preferences
        .values()
        .filter_map(|preference| match preference.muted_until {
            Some(Mute::Until(deadline)) if deadline > now => Some(deadline),
            _ => None,
        })
        .min()
}

/// `NotificationProject["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationProjectKind {
    #[serde(rename = "repository")]
    Repository,
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "jira")]
    Jira,
}

/// `NotificationProject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationProject {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub kind: NotificationProjectKind,
    pub paths: Vec<String>,
}

/// The categories a project offers: Linear and Jira projects only issues,
/// local folders everything but pull requests and issues.
pub fn project_categories(kind: NotificationProjectKind) -> Vec<NotificationCategory> {
    NOTIFICATION_CATEGORIES
        .into_iter()
        .filter(|category| match kind {
            NotificationProjectKind::Linear | NotificationProjectKind::Jira => {
                *category == NotificationCategory::Issues
            }
            NotificationProjectKind::Local => !matches!(
                category,
                NotificationCategory::PullRequests | NotificationCategory::Issues
            ),
            NotificationProjectKind::Repository => true,
        })
        .collect()
}

// notificationMuteActions.ts.

/// The local time for a JavaScript timestamp.
pub fn local_time(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// A medium date and short time in the system locale.
pub fn format_medium_date_time(ms: i64) -> String {
    monocode_platform::date_time::format_local(
        ms,
        monocode_platform::date_time::DateTimeStyle::MediumDateTime,
    )
}

/// `notificationMuteStatus`: one status label for the project rail, menus,
/// and mute controls.
pub fn notification_mute_status(
    preference: Option<&ProjectNotificationPreference>,
    now: i64,
) -> Option<String> {
    let preference = preference.filter(|preference| is_project_muted(preference, now))?;
    Some(match preference.muted_until {
        Some(Mute::Until(deadline)) => {
            format!("Muted until {}", format_medium_date_time(deadline))
        }
        _ => "Muted until resumed".into(),
    })
}

/// One `mutePresets` entry with its label for now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuteAction {
    pub id: &'static str,
    pub label: String,
}

/// `mutePresets`: the id, base label, and duration (`None` until resumed;
/// the custom entry has no duration at all).
const MUTE_PRESETS: [(&str, &str, Option<Option<i64>>); 5] = [
    ("mute:1", "1 hour", Some(Some(3_600_000))),
    ("mute:4", "4 hours", Some(Some(4 * 3_600_000))),
    ("mute:8", "8 hours", Some(Some(8 * 3_600_000))),
    ("mute:indefinite", "Until resumed", Some(None)),
    ("mute:custom", "Choose date and time", None),
];

/// The custom entry's id.
pub const MUTE_CUSTOM: &str = "mute:custom";

/// `notificationMuteActions`: each timed preset says when it ends, as
/// "8 hours (16:00)" or "8 hours (Tomorrow, 0:30)".
pub fn notification_mute_actions(now: i64) -> Vec<MuteAction> {
    let today = local_time(now).map(|time| time.date_naive());
    MUTE_PRESETS
        .iter()
        .map(|(id, label, duration)| {
            let Some(Some(ms)) = duration else {
                return MuteAction {
                    id,
                    label: (*label).to_string(),
                };
            };
            let Some(until) = local_time(now + ms) else {
                return MuteAction {
                    id,
                    label: (*label).to_string(),
                };
            };
            let time = until.format("%-H:%M");
            let day = if Some(until.date_naive()) == today {
                ""
            } else {
                "Tomorrow, "
            };
            MuteAction {
                id,
                label: format!("{label} ({day}{time})"),
            }
        })
        .collect()
}

/// `notificationMuteDeadline`: `Some(Some(mute))` for a preset, `None` for
/// an unknown id or the custom entry.
pub fn notification_mute_deadline(id: &str, now: i64) -> Option<Mute> {
    let (_, _, duration) = MUTE_PRESETS.iter().find(|(preset, _, _)| *preset == id)?;
    match duration {
        None => None,
        Some(None) => Some(Mute::UntilResumed),
        Some(Some(ms)) => Some(Mute::Until(now + ms)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_ms(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn labels_presets_with_their_end_time() {
        let now = local_ms(2030, 1, 15, 12, 0);
        let labels: Vec<String> = notification_mute_actions(now)
            .into_iter()
            .map(|action| action.label)
            .collect();
        assert_eq!(
            labels,
            [
                "1 hour (13:00)",
                "4 hours (16:00)",
                "8 hours (20:00)",
                "Until resumed",
                "Choose date and time"
            ]
        );
        let late = local_ms(2030, 1, 15, 20, 5);
        assert_eq!(
            notification_mute_actions(late)[1].label,
            "4 hours (Tomorrow, 0:05)"
        );
    }

    #[test]
    fn maps_preset_ids_to_deadlines() {
        let now = 1_800_000_000_000;
        assert_eq!(
            notification_mute_deadline("mute:8", now),
            Some(Mute::Until(1_800_028_800_000))
        );
        assert_eq!(
            notification_mute_deadline("mute:indefinite", now),
            Some(Mute::UntilResumed)
        );
        assert_eq!(notification_mute_deadline(MUTE_CUSTOM, now), None);
        assert_eq!(notification_mute_deadline("mute:2", now), None);
    }

    #[test]
    fn describes_the_mute() {
        let now = local_ms(2026, 9, 14, 8, 0);
        let until = ProjectNotificationPreference {
            disabled: vec![],
            muted_until: Some(Mute::Until(local_ms(2026, 9, 14, 16, 0))),
        };
        assert_eq!(
            notification_mute_status(Some(&until), now),
            Some(format!(
                "Muted until {}",
                format_medium_date_time(local_ms(2026, 9, 14, 16, 0))
            ))
        );
        let resumed = ProjectNotificationPreference {
            muted_until: Some(Mute::UntilResumed),
            ..Default::default()
        };
        assert_eq!(
            notification_mute_status(Some(&resumed), now).as_deref(),
            Some("Muted until resumed")
        );
        let expired = ProjectNotificationPreference {
            muted_until: Some(Mute::Until(now - 1)),
            ..Default::default()
        };
        assert_eq!(notification_mute_status(Some(&expired), now), None);
        assert_eq!(notification_mute_status(None, now), None);
    }

    #[test]
    fn offers_categories_by_project_kind() {
        assert_eq!(
            project_categories(NotificationProjectKind::Linear),
            [NotificationCategory::Issues]
        );
        assert_eq!(
            project_categories(NotificationProjectKind::Local),
            [
                NotificationCategory::AgentFinished,
                NotificationCategory::AgentInput,
                NotificationCategory::Reminders
            ]
        );
        assert_eq!(
            project_categories(NotificationProjectKind::Repository).len(),
            5
        );
    }
}
