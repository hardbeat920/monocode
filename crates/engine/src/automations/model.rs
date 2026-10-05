//! Port of src/features/automations/model/automations.ts: the automation,
//! trigger, and run shapes, schedule math, labels, drafts, and the due-run
//! claim loop. The store calls go through `AutomationsBackend`; the change
//! events the TypeScript dispatched are the `Automations` entity's job.

use monocode_core::block::{Extra, ModelSettings};
use monocode_core::js;
use monocode_core::{HarnessId, RuntimeMode};
use serde::{Deserialize, Serialize};

use super::backend::AutomationsBackend;
use super::local_time::{self, LocalFields};

/// `YEAR_MS`: an automation without time triggers is "due" a year out.
const YEAR_MS: i64 = 365 * 24 * 60 * 60 * 1000;

/// `AutomationWorkspaceMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AutomationWorkspaceMode {
    #[serde(rename = "current")]
    Current,
    #[default]
    #[serde(rename = "worktree")]
    Worktree,
    #[serde(rename = "existing")]
    Existing,
}

/// `AutomationScheduleKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AutomationScheduleKind {
    #[serde(rename = "hourly")]
    Hourly,
    #[serde(rename = "daily")]
    Daily,
    #[default]
    #[serde(rename = "weekdays")]
    Weekdays,
    #[serde(rename = "weekly")]
    Weekly,
}

impl AutomationScheduleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            AutomationScheduleKind::Hourly => "hourly",
            AutomationScheduleKind::Daily => "daily",
            AutomationScheduleKind::Weekdays => "weekdays",
            AutomationScheduleKind::Weekly => "weekly",
        }
    }

    /// `isScheduleKind`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "hourly" => Some(AutomationScheduleKind::Hourly),
            "daily" => Some(AutomationScheduleKind::Daily),
            "weekdays" => Some(AutomationScheduleKind::Weekdays),
            "weekly" => Some(AutomationScheduleKind::Weekly),
            _ => None,
        }
    }
}

/// `isScheduleKind`.
pub fn is_schedule_kind(value: &str) -> bool {
    AutomationScheduleKind::parse(value).is_some()
}

/// `AutomationTriggerKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AutomationTriggerKind {
    #[default]
    #[serde(rename = "time")]
    Time,
    #[serde(rename = "github")]
    Github,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "jira")]
    Jira,
    #[serde(rename = "gitlab")]
    Gitlab,
    #[serde(rename = "azuredevops")]
    AzureDevops,
}

impl AutomationTriggerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            AutomationTriggerKind::Time => "time",
            AutomationTriggerKind::Github => "github",
            AutomationTriggerKind::Linear => "linear",
            AutomationTriggerKind::Jira => "jira",
            AutomationTriggerKind::Gitlab => "gitlab",
            AutomationTriggerKind::AzureDevops => "azuredevops",
        }
    }
}

/// `AutomationRunStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutomationRunStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl AutomationRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            AutomationRunStatus::Pending => "pending",
            AutomationRunStatus::Running => "running",
            AutomationRunStatus::Succeeded => "succeeded",
            AutomationRunStatus::Failed => "failed",
            AutomationRunStatus::Skipped => "skipped",
            AutomationRunStatus::Cancelled => "cancelled",
        }
    }
}

/// `AutomationRunTrigger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutomationRunTrigger {
    #[serde(rename = "scheduled")]
    Scheduled,
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "event")]
    Event,
}

/// `AutomationTrigger`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationTrigger {
    pub id: String,
    pub kind: AutomationTriggerKind,
    pub event: String,
    pub schedule_kind: AutomationScheduleKind,
    pub minute: i64,
    pub time: String,
    pub day_of_week: i64,
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub actor: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `Automation`, as the store returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub harness: HarnessId,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    pub cwd: String,
    pub workspace_mode: AutomationWorkspaceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_folder_id: Option<String>,
    pub reuse_session: bool,
    pub runtime_mode: RuntimeMode,
    #[serde(default)]
    pub trigger_kind: AutomationTriggerKind,
    #[serde(default)]
    pub trigger_event: String,
    pub schedule_kind: AutomationScheduleKind,
    pub minute: i64,
    pub time: String,
    pub day_of_week: i64,
    #[serde(default)]
    pub triggers: Option<Vec<AutomationTrigger>>,
    pub missed_run_grace_minutes: i64,
    pub enabled: bool,
    pub next_run_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<AutomationRunStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `AutomationUpsert`: an automation without its run summary and
/// timestamps, as `automations_upsert` takes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationUpsert {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
    pub cwd: String,
    pub workspace_mode: AutomationWorkspaceMode,
    pub worktree_cwd: String,
    pub session_folder_id: String,
    pub reuse_session: bool,
    pub runtime_mode: RuntimeMode,
    pub trigger_kind: AutomationTriggerKind,
    pub trigger_event: String,
    pub schedule_kind: AutomationScheduleKind,
    pub minute: i64,
    pub time: String,
    pub day_of_week: i64,
    pub triggers: Vec<AutomationTrigger>,
    pub missed_run_grace_minutes: i64,
    pub enabled: bool,
    pub next_run_at: i64,
}

/// `AutomationRun`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    pub id: String,
    pub automation_id: String,
    pub trigger: AutomationRunTrigger,
    pub scheduled_for: i64,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    pub status: AutomationRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<AutomationTriggerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DueAutomationRun`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DueAutomationRun {
    pub automation: Automation,
    pub run: AutomationRun,
}

/// The run fields `formatAutomationRunDuration` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunTiming {
    pub status: AutomationRunStatus,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

impl From<&AutomationRun> for RunTiming {
    fn from(run: &AutomationRun) -> Self {
        Self {
            status: run.status,
            created_at: run.created_at,
            started_at: run.started_at,
            completed_at: run.completed_at,
        }
    }
}

/// `formatAutomationRunAt`: `19 Sep, 13:36` in local time.
pub fn format_automation_run_at(at: i64) -> String {
    if at <= 0 {
        return "—".into();
    }
    let date = LocalFields::of(at);
    format!(
        "{} {}, {:02}:{:02}",
        date.day,
        local_time::short_month(date.month),
        date.hours,
        date.minutes
    )
}

/// `formatAutomationRunDuration`.
pub fn format_automation_run_duration(run: RunTiming, now: i64) -> String {
    let live = matches!(
        run.status,
        AutomationRunStatus::Pending | AutomationRunStatus::Running
    );
    let start = run
        .started_at
        .or(if run.status == AutomationRunStatus::Pending {
            None
        } else {
            Some(run.created_at)
        });
    // `!start` and `live && start` treat 0 as missing.
    let start = start.filter(|start| *start != 0);
    let end = run.completed_at.or(if live && start.is_some() {
        Some(now)
    } else {
        None
    });
    let (Some(start), Some(end)) = (start, end) else {
        return "—".into();
    };
    if end < start {
        return "—".into();
    }
    let minutes = (end - start).div_euclid(60_000);
    if minutes < 1 {
        return "< 1m".into();
    }
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest != 0 {
        format!("{hours}h {rest}m")
    } else {
        format!("{hours}h")
    }
}

/// `AutomationDraft`: the editor's state for a new or existing automation.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationDraft {
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
    pub cwd: String,
    pub workspace_mode: AutomationWorkspaceMode,
    pub worktree_cwd: String,
    pub session_folder_id: String,
    pub reuse_session: bool,
    pub runtime_mode: RuntimeMode,
    pub trigger_kind: AutomationTriggerKind,
    pub trigger_event: String,
    pub schedule_kind: AutomationScheduleKind,
    pub minute: i64,
    pub time: String,
    pub day_of_week: i64,
    pub triggers: Vec<AutomationTrigger>,
    pub missed_run_grace_minutes: i64,
    pub enabled: bool,
}

/// `AUTOMATION_WEEKDAYS`.
pub const AUTOMATION_WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The schedule fields `nextAutomationRunAt` and `automationScheduleLabel`
/// read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule<'a> {
    pub schedule_kind: AutomationScheduleKind,
    pub minute: i64,
    pub time: &'a str,
    pub day_of_week: i64,
}

/// Anything with schedule fields.
pub trait HasSchedule {
    fn schedule(&self) -> Schedule<'_>;
}

impl HasSchedule for Schedule<'_> {
    fn schedule(&self) -> Schedule<'_> {
        *self
    }
}

impl HasSchedule for AutomationTrigger {
    fn schedule(&self) -> Schedule<'_> {
        Schedule {
            schedule_kind: self.schedule_kind,
            minute: self.minute,
            time: &self.time,
            day_of_week: self.day_of_week,
        }
    }
}

impl HasSchedule for Automation {
    fn schedule(&self) -> Schedule<'_> {
        Schedule {
            schedule_kind: self.schedule_kind,
            minute: self.minute,
            time: &self.time,
            day_of_week: self.day_of_week,
        }
    }
}

impl HasSchedule for AutomationDraft {
    fn schedule(&self) -> Schedule<'_> {
        Schedule {
            schedule_kind: self.schedule_kind,
            minute: self.minute,
            time: &self.time,
            day_of_week: self.day_of_week,
        }
    }
}

/// `Partial<AutomationTrigger>` for `createAutomationTrigger`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriggerExtras {
    pub id: Option<String>,
    pub schedule_kind: Option<AutomationScheduleKind>,
    pub minute: Option<i64>,
    pub time: Option<String>,
    pub day_of_week: Option<i64>,
    pub repos: Option<Vec<String>>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub actor: Option<String>,
}

/// `createAutomationTrigger`.
pub fn create_automation_trigger(
    kind: AutomationTriggerKind,
    event: &str,
    extras: TriggerExtras,
) -> AutomationTrigger {
    let schedule_kind = extras.schedule_kind.unwrap_or_else(|| {
        match (kind, AutomationScheduleKind::parse(event)) {
            (AutomationTriggerKind::Time, Some(schedule)) => schedule,
            _ => AutomationScheduleKind::Weekdays,
        }
    });
    AutomationTrigger {
        id: extras
            .id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        kind,
        event: event.to_string(),
        schedule_kind,
        minute: extras.minute.unwrap_or(0),
        time: extras.time.unwrap_or_else(|| "09:00".into()),
        day_of_week: extras.day_of_week.unwrap_or(1),
        repos: extras.repos.unwrap_or_default(),
        repo: extras.repo.unwrap_or_default(),
        branch: extras.branch.unwrap_or_default(),
        actor: extras.actor.unwrap_or_else(|| "anyone".into()),
        extra: Extra::new(),
    }
}

/// The legacy single-trigger fields `automationTriggers` falls back to.
#[derive(Debug, Clone, Copy)]
pub struct LegacyTrigger<'a> {
    pub id: &'a str,
    pub triggers: Option<&'a [AutomationTrigger]>,
    pub trigger_kind: AutomationTriggerKind,
    pub trigger_event: &'a str,
    pub schedule: Schedule<'a>,
}

impl<'a> From<&'a Automation> for LegacyTrigger<'a> {
    fn from(automation: &'a Automation) -> Self {
        Self {
            id: &automation.id,
            triggers: automation.triggers.as_deref(),
            trigger_kind: automation.trigger_kind,
            trigger_event: &automation.trigger_event,
            schedule: automation.schedule(),
        }
    }
}

/// `automationTriggers`: the stored list, or one trigger built from the
/// legacy fields.
pub fn automation_triggers<'a>(automation: impl Into<LegacyTrigger<'a>>) -> Vec<AutomationTrigger> {
    let automation = automation.into();
    if let Some(triggers) = automation.triggers {
        return triggers.to_vec();
    }
    let event = if automation.trigger_event.is_empty() {
        automation.schedule.schedule_kind.as_str()
    } else {
        automation.trigger_event
    };
    vec![create_automation_trigger(
        automation.trigger_kind,
        event,
        TriggerExtras {
            id: Some(format!("{}:legacy", automation.id)),
            schedule_kind: Some(automation.schedule.schedule_kind),
            minute: Some(automation.schedule.minute),
            time: Some(automation.schedule.time.to_string()),
            day_of_week: Some(automation.schedule.day_of_week),
            ..TriggerExtras::default()
        },
    )]
}

/// `applyTriggers`: keep the legacy fields in step with the first time
/// trigger, or the first trigger.
pub fn apply_triggers(
    draft: &AutomationDraft,
    triggers: Vec<AutomationTrigger>,
) -> AutomationDraft {
    let primary = triggers
        .iter()
        .find(|trigger| trigger.kind == AutomationTriggerKind::Time)
        .or(triggers.first())
        .cloned();
    AutomationDraft {
        trigger_kind: primary
            .as_ref()
            .map_or(AutomationTriggerKind::Time, |trigger| trigger.kind),
        trigger_event: primary
            .as_ref()
            .map_or_else(String::new, |trigger| trigger.event.clone()),
        schedule_kind: primary
            .as_ref()
            .map_or(AutomationScheduleKind::Weekdays, |trigger| {
                trigger.schedule_kind
            }),
        minute: primary.as_ref().map_or(0, |trigger| trigger.minute),
        time: primary
            .as_ref()
            .map_or_else(|| "09:00".into(), |trigger| trigger.time.clone()),
        day_of_week: primary.as_ref().map_or(1, |trigger| trigger.day_of_week),
        triggers,
        ..draft.clone()
    }
}

/// `nextTriggersRunAt`: the earliest next run across the time triggers.
pub fn next_triggers_run_at(triggers: &[AutomationTrigger], after: i64) -> i64 {
    triggers
        .iter()
        .filter(|trigger| trigger.kind == AutomationTriggerKind::Time)
        .map(|trigger| next_automation_run_at(trigger, after))
        .min()
        .unwrap_or(after + YEAR_MS)
}

/// One overdue occurrence and the run after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub scheduled_for: i64,
    pub next_run_at: i64,
}

/// `overdueTriggerOccurrences`: every occurrence from `first_run_at` up to
/// `now`, at most `limit`.
pub fn overdue_trigger_occurrences(
    triggers: &[AutomationTrigger],
    first_run_at: i64,
    now: i64,
    limit: usize,
) -> Vec<Occurrence> {
    let mut occurrences = Vec::new();
    let mut scheduled_for = first_run_at;
    while scheduled_for <= now && occurrences.len() < limit {
        let next_run_at = next_triggers_run_at(triggers, scheduled_for);
        if next_run_at <= scheduled_for {
            break;
        }
        occurrences.push(Occurrence {
            scheduled_for,
            next_run_at,
        });
        scheduled_for = next_run_at;
    }
    occurrences
}

/// The default `limit` of `overdueTriggerOccurrences`.
pub const OVERDUE_OCCURRENCE_LIMIT: usize = 100;

/// `gmtOffsetLabel`: `GMT+2`, `GMT-5:30`.
pub fn gmt_offset_label(at: i64) -> String {
    let minutes = local_time::utc_offset_minutes(at);
    let sign = if minutes >= 0 { '+' } else { '-' };
    let absolute = minutes.abs();
    let hours = absolute / 60;
    let rest = absolute % 60;
    if rest == 0 {
        format!("GMT{sign}{hours}")
    } else {
        format!("GMT{sign}{hours}:{rest:02}")
    }
}

/// The next run's localized date and zone, with its 24 hour schedule time.
pub fn next_run_preview(at: i64) -> String {
    use monocode_platform::date_time::{DateTimeStyle, format_local};
    let date = LocalFields::of(at);
    let day = format_local(at, DateTimeStyle::WeekdayMonthDay).replace(',', "");
    let zone = format_local(at, DateTimeStyle::TimeZone);
    let zone = if zone.is_empty() {
        gmt_offset_label(at)
    } else {
        zone
    };
    format!(
        "Next run {day}, {:02}:{:02} {zone}",
        date.hours, date.minutes
    )
}

/// `nextAutomationRunAt`: the first run strictly after `after`, in local
/// time.
pub fn next_automation_run_at(schedule: &impl HasSchedule, after: i64) -> i64 {
    let schedule = schedule.schedule();
    let mut start = LocalFields::of(after);
    start.seconds = 0;
    start.millis = 0;
    let (hour, minute) = parse_time(schedule.time);
    if schedule.schedule_kind == AutomationScheduleKind::Hourly {
        let mut candidate = start;
        candidate.minutes = clamp(Some(schedule.minute as f64), 0, 59);
        let mut at = candidate.to_ms();
        if at <= after {
            let mut fields = LocalFields::of(at);
            fields.hours += 1;
            at = fields.to_ms();
        }
        return at;
    }

    let mut candidate = start;
    candidate.hours = hour;
    candidate.minutes = minute;
    let mut at = candidate.to_ms();
    let add_days = |at: i64, days: i64| {
        let mut fields = LocalFields::of(at);
        fields.day += days;
        fields.to_ms()
    };
    match schedule.schedule_kind {
        AutomationScheduleKind::Daily => {
            if at <= after {
                at = add_days(at, 1);
            }
            at
        }
        AutomationScheduleKind::Weekdays => {
            if at <= after {
                at = add_days(at, 1);
            }
            while matches!(local_time::weekday(at), 0 | 6) {
                at = add_days(at, 1);
            }
            at
        }
        _ => {
            let day = clamp(Some(schedule.day_of_week as f64), 0, 6);
            let mut days = (day - local_time::weekday(at) + 7) % 7;
            if days == 0 && at <= after {
                days = 7;
            }
            add_days(at, days)
        }
    }
}

/// `automationScheduleLabel`.
pub fn automation_schedule_label(automation: &impl HasSchedule) -> String {
    let schedule = automation.schedule();
    let time = format_clock(schedule.time);
    match schedule.schedule_kind {
        AutomationScheduleKind::Hourly => format!("Hourly at :{:02}", schedule.minute),
        AutomationScheduleKind::Daily => format!("Daily at {time}"),
        AutomationScheduleKind::Weekdays => format!("Weekdays at {time}"),
        AutomationScheduleKind::Weekly => {
            let day = usize::try_from(schedule.day_of_week)
                .ok()
                .and_then(|day| AUTOMATION_WEEKDAYS.get(day))
                .copied()
                .unwrap_or("Weekly");
            format!("{day} at {time}")
        }
    }
}

/// `newAutomationDraft`.
pub fn new_automation_draft(cwd: &str, harness: HarnessId, model: &str) -> AutomationDraft {
    AutomationDraft {
        id: None,
        name: String::new(),
        prompt: String::new(),
        harness,
        model: model.to_string(),
        model_settings: ModelSettings::new(),
        cwd: cwd.to_string(),
        workspace_mode: AutomationWorkspaceMode::Worktree,
        worktree_cwd: String::new(),
        session_folder_id: String::new(),
        reuse_session: false,
        runtime_mode: RuntimeMode::Auto,
        trigger_kind: AutomationTriggerKind::Time,
        trigger_event: String::new(),
        schedule_kind: AutomationScheduleKind::Weekdays,
        minute: 0,
        time: "09:00".into(),
        day_of_week: 1,
        triggers: Vec::new(),
        missed_run_grace_minutes: 720,
        enabled: true,
    }
}

/// The trigger part of an automation template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateTrigger {
    pub kind: AutomationTriggerKind,
    pub event: &'static str,
    pub schedule_kind: Option<AutomationScheduleKind>,
    pub time: Option<&'static str>,
    pub day_of_week: Option<i64>,
    pub minute: Option<i64>,
}

/// `draftFromTemplate`.
pub fn draft_from_template(
    cwd: &str,
    harness: HarnessId,
    model: &str,
    name: &str,
    prompt: &str,
    trigger: &TemplateTrigger,
) -> AutomationDraft {
    let draft = new_automation_draft(cwd, harness, model);
    let trigger = create_automation_trigger(
        trigger.kind,
        trigger.event,
        TriggerExtras {
            schedule_kind: trigger.schedule_kind,
            time: trigger.time.map(str::to_string),
            day_of_week: trigger.day_of_week,
            minute: trigger.minute,
            ..TriggerExtras::default()
        },
    );
    apply_triggers(
        &AutomationDraft {
            name: name.to_string(),
            prompt: prompt.to_string(),
            ..draft
        },
        vec![trigger],
    )
}

/// `draftFromAutomation`.
pub fn draft_from_automation(automation: &Automation) -> AutomationDraft {
    AutomationDraft {
        id: Some(automation.id.clone()),
        name: automation.name.clone(),
        prompt: automation.prompt.clone(),
        harness: automation.harness,
        model: automation.model.clone(),
        model_settings: automation.model_settings.clone().unwrap_or_default(),
        cwd: automation.cwd.clone(),
        workspace_mode: automation.workspace_mode,
        worktree_cwd: automation.worktree_cwd.clone().unwrap_or_default(),
        session_folder_id: automation.session_folder_id.clone().unwrap_or_default(),
        reuse_session: automation.reuse_session,
        runtime_mode: automation.runtime_mode,
        trigger_kind: automation.trigger_kind,
        trigger_event: automation.trigger_event.clone(),
        schedule_kind: automation.schedule_kind,
        minute: automation.minute,
        time: automation.time.clone(),
        day_of_week: automation.day_of_week,
        triggers: automation_triggers(automation),
        missed_run_grace_minutes: automation.missed_run_grace_minutes,
        enabled: automation.enabled,
    }
}

/// The upsert half of `saveAutomation`: sync the legacy fields, give a new
/// draft an id, and schedule its next run after `now`.
pub fn automation_upsert(draft: &AutomationDraft, now: i64) -> AutomationUpsert {
    let synced = apply_triggers(draft, draft.triggers.clone());
    AutomationUpsert {
        id: synced
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        next_run_at: next_triggers_run_at(&synced.triggers, now),
        name: synced.name,
        prompt: synced.prompt,
        harness: synced.harness,
        model: synced.model,
        model_settings: synced.model_settings,
        cwd: synced.cwd,
        workspace_mode: synced.workspace_mode,
        worktree_cwd: synced.worktree_cwd,
        session_folder_id: synced.session_folder_id,
        reuse_session: synced.reuse_session,
        runtime_mode: synced.runtime_mode,
        trigger_kind: synced.trigger_kind,
        trigger_event: synced.trigger_event,
        schedule_kind: synced.schedule_kind,
        minute: synced.minute,
        time: synced.time,
        day_of_week: synced.day_of_week,
        triggers: synced.triggers,
        missed_run_grace_minutes: synced.missed_run_grace_minutes,
        enabled: synced.enabled,
    }
}

/// `claimDueAutomations`: claim every overdue occurrence of every enabled
/// automation with a time trigger. A store failure on one occurrence keeps
/// the earlier claims; the rest stay due for the next pass. Returns the
/// claimed runs to launch (skipped runs are recorded but not returned) and
/// whether any automation was due, which is when the TypeScript announced a
/// change.
pub async fn claim_due_automations(
    backend: &dyn AutomationsBackend,
    now: i64,
) -> Result<(Vec<DueAutomationRun>, bool), String> {
    let automations = backend.list().await?;
    let due: Vec<Automation> = automations
        .into_iter()
        .filter(|automation| {
            automation.enabled
                && automation_triggers(automation)
                    .iter()
                    .any(|trigger| trigger.kind == AutomationTriggerKind::Time)
                && automation.next_run_at <= now
        })
        .collect();
    let mut claimed = Vec::new();
    for automation in &due {
        let triggers = automation_triggers(automation);
        // Bound each pass so a long offline period cannot hold the scheduler.
        // Any remaining overdue occurrences stay due for the next pass.
        for occurrence in overdue_trigger_occurrences(
            &triggers,
            automation.next_run_at,
            now,
            OVERDUE_OCCURRENCE_LIMIT,
        ) {
            let result = backend
                .claim_due(
                    automation.id.clone(),
                    occurrence.scheduled_for,
                    occurrence.next_run_at,
                    now,
                )
                .await;
            // Keep earlier successful claims launchable. This occurrence
            // remains due because the store did not accept it.
            let Ok(Some(result)) = result else {
                break;
            };
            if result.run.status == AutomationRunStatus::Pending {
                claimed.push(result);
            }
        }
    }
    Ok((claimed, !due.is_empty()))
}

/// `parseTime`: `"09:30"` as clamped hour and minute.
pub fn parse_time(value: &str) -> (i64, i64) {
    let mut parts = value.split(':').map(js::parse_number);
    let hour = parts.next().flatten();
    let minute = parts.next().flatten();
    (clamp(hour, 0, 23), clamp(minute, 0, 59))
}

/// `clamp`: a missing or non-finite number is `min`.
fn clamp(value: Option<f64>, min: i64, max: i64) -> i64 {
    match value {
        Some(value) if value.is_finite() => (value.trunc() as i64).clamp(min, max),
        _ => min,
    }
}

/// The schedule time in the system locale.
fn format_clock(value: &str) -> String {
    let (hour, minute) = parse_time(value);
    local_time::clock_label(hour, minute)
}

/// The automations view's search: name, prompt, project label, and folder,
/// case-insensitive.
pub fn automation_matches_query(automation: &Automation, query: &str, project_label: &str) -> bool {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return true;
    }
    format!(
        "{}\n{}\n{}\n{}",
        automation.name, automation.prompt, project_label, automation.cwd
    )
    .to_lowercase()
    .contains(&needle)
}
