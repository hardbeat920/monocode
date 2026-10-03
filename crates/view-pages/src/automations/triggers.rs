//! The trigger and run display helpers from AutomationsView.tsx:
//! `TRIGGER_CATEGORIES`, `TRIGGER_EVENTS`, `triggerName`,
//! `findTriggerEvent`, `triggerLabel`, `runTriggerMeta`, `timeOptions`, the
//! sentence pieces of `TimeTriggerSentence` and `EventTriggerSentence`, and
//! the run status labels and tones. All UI-only.

use super::model::{
    AUTOMATION_WEEKDAYS, Automation, AutomationDraft, AutomationRun, AutomationRunStatus,
    AutomationRunTrigger, AutomationScheduleKind, AutomationTrigger, AutomationTriggerKind,
    automation_schedule_label, automation_triggers,
};

/// `TRIGGER_CATEGORIES`, in menu order.
pub const TRIGGER_CATEGORIES: [(AutomationTriggerKind, &str); 6] = [
    (AutomationTriggerKind::Time, "Scheduled"),
    (AutomationTriggerKind::Github, "GitHub"),
    (AutomationTriggerKind::Linear, "Linear"),
    (AutomationTriggerKind::Jira, "Jira"),
    (AutomationTriggerKind::Gitlab, "GitLab"),
    (AutomationTriggerKind::AzureDevops, "Azure DevOps"),
];

/// `TriggerEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerEvent {
    pub value: &'static str,
    pub label: &'static str,
}

/// `TRIGGER_EVENTS[kind]`.
pub fn trigger_events(kind: AutomationTriggerKind) -> &'static [TriggerEvent] {
    match kind {
        AutomationTriggerKind::Time => &[
            TriggerEvent {
                value: "hourly",
                label: "Hourly",
            },
            TriggerEvent {
                value: "daily",
                label: "Daily",
            },
            TriggerEvent {
                value: "weekdays",
                label: "Weekdays",
            },
            TriggerEvent {
                value: "weekly",
                label: "Weekly",
            },
        ],
        AutomationTriggerKind::Github => &[
            TriggerEvent {
                value: "draft_opened",
                label: "Draft opened",
            },
            TriggerEvent {
                value: "pull_request_opened",
                label: "Pull request opened",
            },
            TriggerEvent {
                value: "issue_opened",
                label: "Issue opened",
            },
        ],
        AutomationTriggerKind::Linear => &[TriggerEvent {
            value: "issue_created",
            label: "Issue created",
        }],
        AutomationTriggerKind::Jira => &[TriggerEvent {
            value: "issue_created",
            label: "Issue appeared",
        }],
        AutomationTriggerKind::Gitlab => &[
            TriggerEvent {
                value: "merge_request_opened",
                label: "Merge request opened",
            },
            TriggerEvent {
                value: "issue_opened",
                label: "Issue opened",
            },
        ],
        AutomationTriggerKind::AzureDevops => &[
            TriggerEvent {
                value: "pull_request_appeared",
                label: "Pull request appeared",
            },
            TriggerEvent {
                value: "work_item_appeared",
                label: "Work item appeared",
            },
        ],
    }
}

/// `triggerName`.
pub fn trigger_name(kind: AutomationTriggerKind) -> &'static str {
    TRIGGER_CATEGORIES
        .iter()
        .find(|(value, _)| *value == kind)
        .map_or(kind.as_str(), |(_, label)| label)
}

/// `findTriggerEvent`.
pub fn find_trigger_event(kind: AutomationTriggerKind, value: &str) -> Option<TriggerEvent> {
    trigger_events(kind)
        .iter()
        .find(|event| event.value == value)
        .copied()
}

/// `triggerCategories`: the categories whose label contains the query.
pub fn trigger_categories(query: &str) -> Vec<(AutomationTriggerKind, &'static str)> {
    let needle = query.to_lowercase();
    TRIGGER_CATEGORIES
        .iter()
        .filter(|(_, label)| label.to_lowercase().contains(&needle))
        .copied()
        .collect()
}

/// `triggerLabel`: the card's trigger line.
pub fn trigger_label(automation: &Automation) -> String {
    let triggers = automation_triggers(automation);
    let Some(first) = triggers.first() else {
        return "No trigger".into();
    };
    let label = if first.kind == AutomationTriggerKind::Time {
        automation_schedule_label(first)
    } else {
        find_trigger_event(first.kind, &first.event).map_or_else(
            || trigger_name(first.kind).to_string(),
            |event| event.label.to_string(),
        )
    };
    if triggers.len() > 1 {
        format!("{label} +{}", triggers.len() - 1)
    } else {
        label
    }
}

/// `runTriggerMeta`: the run history's trigger cell.
pub fn run_trigger_meta(
    run: &AutomationRun,
    draft: &AutomationDraft,
) -> (AutomationTriggerKind, String) {
    if run.trigger == AutomationRunTrigger::Manual {
        return (AutomationTriggerKind::Time, "Test run".into());
    }
    if run.trigger == AutomationRunTrigger::Event {
        let kind = run.event_kind.unwrap_or(draft.trigger_kind);
        let event = run.event.as_deref().unwrap_or(&draft.trigger_event);
        let label = find_trigger_event(kind, event).map_or_else(
            || trigger_name(kind).to_string(),
            |event| event.label.to_string(),
        );
        return (kind, label);
    }
    if let Some(time) = draft
        .triggers
        .iter()
        .find(|trigger| trigger.kind == AutomationTriggerKind::Time)
    {
        return (
            AutomationTriggerKind::Time,
            format!("Scheduled · {}", automation_schedule_label(time)),
        );
    }
    let Some(first) = draft.triggers.first() else {
        return (AutomationTriggerKind::Time, "Scheduled".into());
    };
    let label = find_trigger_event(first.kind, &first.event).map_or_else(
        || trigger_name(first.kind).to_string(),
        |event| event.label.to_string(),
    );
    (first.kind, label)
}

/// `runStatusLabel`.
pub fn run_status_label(status: AutomationRunStatus) -> &'static str {
    match status {
        AutomationRunStatus::Succeeded => "Succeeded",
        AutomationRunStatus::Failed => "Failed",
        AutomationRunStatus::Skipped => "Skipped",
        AutomationRunStatus::Cancelled => "Cancelled",
        AutomationRunStatus::Running => "Running",
        AutomationRunStatus::Pending => "Pending",
    }
}

/// `runStatusTone`: which Tailwind palette the pill uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTone {
    /// `bg-emerald-500/12 text-emerald-400`.
    Success,
    /// `bg-rose-500/12 text-rose-400`.
    Failure,
    /// `bg-amber-500/12 text-amber-400`.
    Warning,
    /// `bg-content/8 text-content/50`.
    Muted,
    /// `bg-blue-500/12 text-blue-400`.
    Info,
}

pub fn run_status_tone(status: AutomationRunStatus) -> RunTone {
    match status {
        AutomationRunStatus::Succeeded => RunTone::Success,
        AutomationRunStatus::Failed => RunTone::Failure,
        AutomationRunStatus::Skipped => RunTone::Warning,
        AutomationRunStatus::Cancelled => RunTone::Muted,
        AutomationRunStatus::Running | AutomationRunStatus::Pending => RunTone::Info,
    }
}

/// `timeOptions`: every half hour, plus the current time first when it is
/// off the grid.
pub fn time_options(current: &str) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = (0..24)
        .flat_map(|hour| {
            [format!("{hour:02}:00"), format!("{hour:02}:30")].map(|value| (value.clone(), value))
        })
        .collect();
    if !current.is_empty() && !options.iter().any(|(value, _)| value == current) {
        options.insert(0, (current.to_string(), current.to_string()));
    }
    options
}

/// The minute pill's options: `:00`, `:15`, `:30`, `:45`.
pub fn minute_options() -> Vec<(String, String)> {
    [0, 15, 30, 45]
        .into_iter()
        .map(|value| (value.to_string(), format!(":{value:02}")))
        .collect()
}

/// The day pill's options, Sunday first.
pub fn day_options() -> Vec<(String, String)> {
    AUTOMATION_WEEKDAYS
        .iter()
        .enumerate()
        .map(|(value, label)| (value.to_string(), label.to_string()))
        .collect()
}

/// The time sentence's opening words.
pub fn time_sentence_prefix(kind: AutomationScheduleKind) -> &'static str {
    match kind {
        AutomationScheduleKind::Hourly => "Every hour at",
        AutomationScheduleKind::Daily => "Every day at",
        AutomationScheduleKind::Weekdays => "Every weekday at",
        AutomationScheduleKind::Weekly => "Every week on",
    }
}

/// The event sentence's stem: "Push" for a push trigger, else the event's
/// label.
pub fn event_sentence_stem(trigger: &AutomationTrigger) -> String {
    if trigger.event == "push_to_branch" {
        return "Push".into();
    }
    find_trigger_event(trigger.kind, &trigger.event)
        .map_or_else(|| trigger.event.clone(), |event| event.label.to_string())
}

/// The editor can save: a name, instructions, a project, and a model.
pub fn draft_is_valid(draft: &AutomationDraft) -> bool {
    !monocode_core::js::trim(&draft.name).is_empty()
        && !monocode_core::js::trim(&draft.prompt).is_empty()
        && crate::format::looks_like_project(&draft.cwd)
        && !draft.model.is_empty()
}

/// `WORKSPACE_OPTIONS`.
pub const WORKSPACE_OPTIONS: [(&str, &str); 2] =
    [("current", "Current"), ("worktree", "Fresh worktree")];
/// `CONVERSATION_OPTIONS`.
pub const CONVERSATION_OPTIONS: [(&str, &str); 2] =
    [("fresh", "Start fresh"), ("reuse", "Continue last")];
/// `GRACE_OPTIONS`.
pub const GRACE_OPTIONS: [(&str, &str); 5] = [
    ("0", "Do not catch up"),
    ("30", "30 minutes"),
    ("120", "2 hours"),
    ("720", "12 hours"),
    ("1440", "24 hours"),
];
/// The trigger list's limit.
pub const MAX_TRIGGERS: usize = 20;
