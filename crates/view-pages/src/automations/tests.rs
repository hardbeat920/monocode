//! Tests for the automations page: the schedule and draft helpers (ported
//! from automations.test.ts, as the engine's model_tests.rs has them), the
//! trigger and run labels, and the page over [`LocalAutomations`].

use super::local_time::{self, local_ms};
use super::model::*;
use monocode_core::HarnessId;
use monocode_core::block::Extra;

/// `new Date(year, month - 1, day, hours, minutes).getTime()`.
fn at(year: i32, month: i64, day: i64, hours: i64, minutes: i64) -> i64 {
    local_ms(year, month - 1, day, hours, minutes)
}

/// An automation built from a draft, like the TypeScript tests' spread.
fn automation_from(draft: AutomationDraft, id: &str) -> Automation {
    Automation {
        id: id.to_string(),
        name: draft.name,
        prompt: draft.prompt,
        harness: draft.harness,
        model: draft.model,
        model_settings: Some(draft.model_settings),
        cwd: draft.cwd,
        workspace_mode: draft.workspace_mode,
        worktree_cwd: Some(draft.worktree_cwd),
        session_folder_id: Some(draft.session_folder_id),
        reuse_session: draft.reuse_session,
        runtime_mode: draft.runtime_mode,
        trigger_kind: draft.trigger_kind,
        trigger_event: draft.trigger_event,
        schedule_kind: draft.schedule_kind,
        minute: draft.minute,
        time: draft.time,
        day_of_week: draft.day_of_week,
        triggers: Some(draft.triggers),
        missed_run_grace_minutes: draft.missed_run_grace_minutes,
        enabled: draft.enabled,
        next_run_at: at(2026, 9, 21, 9, 0),
        last_run_at: None,
        last_run_status: None,
        last_run_error: None,
        last_session_id: None,
        created_at: at(2026, 9, 19, 9, 0),
        updated_at: at(2026, 9, 19, 10, 0),
        extra: Extra::new(),
    }
}

fn schedule(kind: AutomationScheduleKind, minute: i64, time: &str, day: i64) -> Schedule<'_> {
    Schedule {
        schedule_kind: kind,
        minute,
        time,
        day_of_week: day,
    }
}

fn time_trigger(event: &str, time: &str, day_of_week: Option<i64>) -> AutomationTrigger {
    create_automation_trigger(
        AutomationTriggerKind::Time,
        event,
        TriggerExtras {
            time: Some(time.into()),
            day_of_week,
            ..TriggerExtras::default()
        },
    )
}

// automation schedules

#[test]
fn moves_hourly_schedules_to_the_next_occurrence() {
    assert_eq!(
        next_automation_run_at(
            &schedule(AutomationScheduleKind::Hourly, 15, "09:00", 1),
            at(2026, 9, 19, 10, 20)
        ),
        at(2026, 9, 19, 11, 15)
    );
}

#[test]
fn skips_weekends_for_weekday_schedules() {
    assert_eq!(
        next_automation_run_at(
            &schedule(AutomationScheduleKind::Weekdays, 0, "09:00", 1),
            at(2026, 9, 18, 10, 0)
        ),
        at(2026, 9, 21, 9, 0)
    );
}

#[test]
fn keeps_a_later_occurrence_on_the_same_day() {
    assert_eq!(
        next_automation_run_at(
            &schedule(AutomationScheduleKind::Daily, 0, "18:00", 1),
            at(2026, 9, 19, 10, 0)
        ),
        at(2026, 9, 19, 18, 0)
    );
}

#[test]
fn moves_weekly_schedules_to_the_chosen_day() {
    // Saturday 19 September to Monday 21, and a Monday past its time to the
    // Monday after.
    assert_eq!(
        next_automation_run_at(
            &schedule(AutomationScheduleKind::Weekly, 0, "09:00", 1),
            at(2026, 9, 19, 10, 0)
        ),
        at(2026, 9, 21, 9, 0)
    );
    assert_eq!(
        next_automation_run_at(
            &schedule(AutomationScheduleKind::Weekly, 0, "09:00", 1),
            at(2026, 9, 21, 9, 0)
        ),
        at(2026, 9, 28, 9, 0)
    );
}

#[test]
fn describes_weekly_schedules() {
    assert!(
        automation_schedule_label(&schedule(AutomationScheduleKind::Weekly, 0, "09:00", 1))
            .starts_with("Monday at ")
    );
    assert_eq!(
        automation_schedule_label(&schedule(AutomationScheduleKind::Hourly, 5, "09:00", 1)),
        "Hourly at :05"
    );
    assert_eq!(
        automation_schedule_label(&schedule(AutomationScheduleKind::Weekdays, 0, "13:30", 1)),
        format!("Weekdays at {}", local_time::clock_label(13, 30))
    );
}

#[test]
fn starts_without_triggers_and_hydrates_a_legacy_provider_trigger() {
    let initial = new_automation_draft("/repo", HarnessId::Codex, "model");
    assert!(initial.triggers.is_empty());
    assert_eq!(initial.trigger_kind, AutomationTriggerKind::Time);
    assert!(initial.model_settings.is_empty());
    assert_eq!(initial.session_folder_id, "");

    let mut stored = automation_from(initial, "automation-id");
    stored.trigger_kind = AutomationTriggerKind::Gitlab;
    stored.trigger_event = "merge_request_opened".into();
    stored.triggers = None;
    let restored = draft_from_automation(&stored);
    assert_eq!(restored.trigger_kind, AutomationTriggerKind::Gitlab);
    assert_eq!(restored.trigger_event, "merge_request_opened");
    assert_eq!(restored.triggers.len(), 1);
    assert_eq!(restored.triggers[0].kind, AutomationTriggerKind::Gitlab);
    assert_eq!(restored.triggers[0].event, "merge_request_opened");
}

#[test]
fn defaults_missing_model_settings_to_an_empty_map() {
    let initial = new_automation_draft("/repo", HarnessId::Codex, "model");
    let mut stored = automation_from(initial, "automation-id");
    stored.model_settings = None;
    assert!(draft_from_automation(&stored).model_settings.is_empty());
}

#[test]
fn keeps_an_explicit_empty_trigger_list_empty() {
    let initial = new_automation_draft("/repo", HarnessId::Codex, "model");
    let mut stored = automation_from(initial, "automation-id");
    stored.triggers = Some(Vec::new());
    assert!(draft_from_automation(&stored).triggers.is_empty());
}

#[test]
fn syncs_legacy_fields_from_the_earliest_time_trigger() {
    let draft = new_automation_draft("/repo", HarnessId::Codex, "model");
    let github = create_automation_trigger(
        AutomationTriggerKind::Github,
        "draft_opened",
        TriggerExtras::default(),
    );
    let weekly = time_trigger("weekly", "09:00", Some(1));
    let next = apply_triggers(&draft, vec![github, weekly]);
    assert_eq!(next.trigger_kind, AutomationTriggerKind::Time);
    assert_eq!(next.trigger_event, "weekly");
    assert_eq!(next.schedule_kind, AutomationScheduleKind::Weekly);
    assert_eq!(next.triggers.len(), 2);
}

#[test]
fn picks_the_earliest_next_run_across_time_triggers() {
    let daily = time_trigger("daily", "18:00", None);
    let weekly = time_trigger("weekly", "09:00", Some(1));
    assert_eq!(
        next_triggers_run_at(&[weekly, daily], at(2026, 9, 19, 10, 0)),
        at(2026, 9, 19, 18, 0)
    );
}

#[test]
fn preserves_each_overdue_occurrence_after_a_delayed_poll() {
    let morning = time_trigger("daily", "09:00", None);
    let later = time_trigger("daily", "10:00", None);
    let first_run_at = at(2026, 9, 19, 9, 0);
    let occurrences = overdue_trigger_occurrences(
        &[morning, later],
        first_run_at,
        at(2026, 9, 19, 10, 30),
        OVERDUE_OCCURRENCE_LIMIT,
    );
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| occurrence.scheduled_for)
            .collect::<Vec<_>>(),
        [first_run_at, at(2026, 9, 19, 10, 0)]
    );
}

#[test]
fn labels_the_timezone_offset_and_next_run() {
    assert!(gmt_offset_label(at(2026, 9, 19, 12, 0)).starts_with("GMT"));
    let label = gmt_offset_label(at(2026, 9, 19, 12, 0));
    assert!(matches!(label.as_bytes()[3], b'+' | b'-'));
    assert!(label.as_bytes()[4].is_ascii_digit());
    assert!(next_run_preview(at(2026, 9, 21, 9, 0)).starts_with("Next run "));
    let stamp = at(2026, 9, 21, 9, 0);
    let date = monocode_platform::date_time::format_local(
        stamp,
        monocode_platform::date_time::DateTimeStyle::WeekdayMonthDay,
    )
    .replace(',', "");
    assert!(next_run_preview(stamp).starts_with(&format!("Next run {date}, 09:00 ")));
}

#[test]
fn hydrates_missing_trigger_arrays_from_legacy_fields() {
    let triggers = automation_triggers(LegacyTrigger {
        id: "automation-id",
        triggers: None,
        trigger_kind: AutomationTriggerKind::Github,
        trigger_event: "push_to_branch",
        schedule: schedule(AutomationScheduleKind::Weekdays, 0, "09:00", 1),
    });
    assert_eq!(
        triggers
            .iter()
            .map(|trigger| format!("{}:{}", trigger.kind.as_str(), trigger.event))
            .collect::<Vec<_>>(),
        ["github:push_to_branch"]
    );
    assert_eq!(triggers[0].id, "automation-id:legacy");
}

#[test]
fn prefills_a_draft_from_a_template_trigger() {
    let draft = draft_from_template(
        "/repo",
        HarnessId::Codex,
        "model",
        "Find critical bugs",
        "Review recent commits.",
        &TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekdays".into(),
            schedule_kind: Some(AutomationScheduleKind::Weekdays),
            time: Some("09:00".into()),
            day_of_week: None,
            minute: None,
        },
    );
    assert_eq!(draft.name, "Find critical bugs");
    assert_eq!(draft.prompt, "Review recent commits.");
    assert_eq!(draft.triggers.len(), 1);
    assert_eq!(draft.trigger_kind, AutomationTriggerKind::Time);
    assert_eq!(draft.trigger_event, "weekdays");
    assert_eq!(draft.schedule_kind, AutomationScheduleKind::Weekdays);
    assert_eq!(draft.time, "09:00");
}

#[test]
fn saving_syncs_triggers_and_schedules_after_now() {
    let mut draft = new_automation_draft("/repo", HarnessId::Codex, "model");
    draft.triggers = vec![time_trigger("daily", "18:00", None)];
    let upsert = automation_upsert(&draft, at(2026, 9, 19, 10, 0));
    assert!(!upsert.id.is_empty());
    assert_eq!(upsert.schedule_kind, AutomationScheduleKind::Daily);
    assert_eq!(upsert.next_run_at, at(2026, 9, 19, 18, 0));
    // No time trigger: due a year out.
    draft.triggers = vec![create_automation_trigger(
        AutomationTriggerKind::Github,
        "issue_opened",
        TriggerExtras::default(),
    )];
    let now = at(2026, 9, 19, 10, 0);
    assert_eq!(
        automation_upsert(&draft, now).next_run_at,
        now + 365 * 24 * 60 * 60 * 1000
    );
}

// automation run display

#[test]
fn formats_the_triggered_timestamp_as_day_month_24h_time() {
    let stamp = local_ms(2026, 8, 19, 13, 36);
    assert_eq!(
        format_automation_run_at(stamp),
        format!("19 {}, 13:36", local_time::short_month(8))
    );
    assert_eq!(format_automation_run_at(0), "—");
}

#[test]
fn summarizes_run_duration_the_way_the_history_list_does() {
    let created_at = at(2026, 9, 19, 13, 0);
    let timing = |status, started_at, completed_at| RunTiming {
        status,
        created_at,
        started_at,
        completed_at,
    };
    assert_eq!(
        format_automation_run_duration(timing(AutomationRunStatus::Pending, None, None), 0),
        "—"
    );
    assert_eq!(
        format_automation_run_duration(
            timing(
                AutomationRunStatus::Failed,
                Some(created_at),
                Some(created_at + 20_000)
            ),
            0
        ),
        "< 1m"
    );
    assert_eq!(
        format_automation_run_duration(
            timing(
                AutomationRunStatus::Succeeded,
                Some(created_at),
                Some(created_at + 5 * 60_000)
            ),
            0
        ),
        "5m"
    );
    assert_eq!(
        format_automation_run_duration(
            timing(AutomationRunStatus::Running, Some(created_at), None),
            created_at + 90_000
        ),
        "1m"
    );
    assert_eq!(
        format_automation_run_duration(
            timing(
                AutomationRunStatus::Succeeded,
                Some(created_at),
                Some(created_at + 125 * 60_000)
            ),
            0
        ),
        "2h 5m"
    );
}

// Trigger and run labels (AutomationsView.tsx helpers).

mod labels {
    use super::super::triggers::*;
    use super::*;

    fn draft() -> AutomationDraft {
        new_automation_draft("/repo", HarnessId::Codex, "model")
    }

    #[test]
    fn card_labels_show_the_first_trigger_and_the_rest_as_a_count() {
        let mut stored = automation_from(draft(), "a");
        stored.triggers = Some(Vec::new());
        assert_eq!(trigger_label(&stored), "No trigger");
        stored.triggers = Some(vec![
            create_automation_trigger(
                AutomationTriggerKind::Github,
                "pull_request_opened",
                TriggerExtras::default(),
            ),
            time_trigger("weekly", "09:00", Some(1)),
        ]);
        assert_eq!(trigger_label(&stored), "Pull request opened +1");
        stored.triggers = Some(vec![time_trigger("daily", "18:30", None)]);
        assert_eq!(
            trigger_label(&stored),
            format!("Daily at {}", local_time::clock_label(18, 30))
        );
        stored.triggers = Some(vec![create_automation_trigger(
            AutomationTriggerKind::Jira,
            "unknown_event",
            TriggerExtras::default(),
        )]);
        assert_eq!(trigger_label(&stored), "Jira");
    }

    fn run(trigger: &str, extra: serde_json::Value) -> AutomationRun {
        let mut value = serde_json::json!({
            "id": "r", "automationId": "a", "trigger": trigger,
            "scheduledFor": 1, "createdAt": 1, "status": "succeeded",
        });
        for (key, field) in extra.as_object().unwrap() {
            value[key] = field.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn run_rows_name_their_trigger() {
        let mut draft = draft();
        draft.triggers = vec![
            create_automation_trigger(
                AutomationTriggerKind::Github,
                "issue_opened",
                TriggerExtras::default(),
            ),
            time_trigger("weekdays", "09:00", None),
        ];
        assert_eq!(
            run_trigger_meta(&run("manual", serde_json::json!({})), &draft),
            (AutomationTriggerKind::Time, "Test run".to_string())
        );
        assert_eq!(
            run_trigger_meta(
                &run(
                    "event",
                    serde_json::json!({"eventKind": "gitlab", "event": "merge_request_opened"})
                ),
                &draft
            ),
            (
                AutomationTriggerKind::Gitlab,
                "Merge request opened".to_string()
            )
        );
        assert_eq!(
            run_trigger_meta(&run("scheduled", serde_json::json!({})), &draft),
            (
                AutomationTriggerKind::Time,
                format!("Scheduled · Weekdays at {}", local_time::clock_label(9, 0))
            )
        );
        draft.triggers.truncate(1);
        assert_eq!(
            run_trigger_meta(&run("scheduled", serde_json::json!({})), &draft),
            (AutomationTriggerKind::Github, "Issue opened".to_string())
        );
        draft.triggers.clear();
        assert_eq!(
            run_trigger_meta(&run("scheduled", serde_json::json!({})), &draft),
            (AutomationTriggerKind::Time, "Scheduled".to_string())
        );
    }

    #[test]
    fn time_options_cover_half_hours_and_keep_an_odd_time_first() {
        let options = time_options("09:00");
        assert_eq!(options.len(), 48);
        assert_eq!(options[0].0, "00:00");
        assert_eq!(options[47].0, "23:30");
        let odd = time_options("09:10");
        assert_eq!(odd.len(), 49);
        assert_eq!(odd[0].0, "09:10");
        assert_eq!(minute_options()[1], ("15".to_string(), ":15".to_string()));
        assert_eq!(day_options()[0], ("0".to_string(), "Sunday".to_string()));
    }

    #[test]
    fn sentences_and_statuses() {
        assert_eq!(
            time_sentence_prefix(AutomationScheduleKind::Weekly),
            "Every week on"
        );
        let push = create_automation_trigger(
            AutomationTriggerKind::Github,
            "push_to_branch",
            TriggerExtras::default(),
        );
        assert_eq!(event_sentence_stem(&push), "Push");
        assert_eq!(
            run_status_label(AutomationRunStatus::Cancelled),
            "Cancelled"
        );
        assert_eq!(run_status_tone(AutomationRunStatus::Pending), RunTone::Info);
        assert_eq!(
            trigger_name(AutomationTriggerKind::AzureDevops),
            "Azure DevOps"
        );
        assert_eq!(trigger_categories("git").len(), 2);
        let mut draft = draft();
        assert!(!draft_is_valid(&draft));
        draft.name = "Nightly".into();
        draft.prompt = "Do it".into();
        assert!(draft_is_valid(&draft));
        draft.cwd = "~".into();
        assert!(!draft_is_valid(&draft));
    }
}
