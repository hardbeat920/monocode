//! Ports of automations.test.ts, automationTemplates.test.ts, and
//! automationEvents.test.ts.

use std::cell::RefCell;

use monocode_core::HarnessId;
use monocode_core::inbox::{InboxKind, InboxProvider, WorkItemKind};
use monocode_settings::Kv;

use super::backend::AutomationsBackend;
use super::events::*;
use super::local_time::{self, local_ms};
use super::model::*;
use super::templates::*;
use super::testing::*;

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
            event: "weekdays",
            schedule_kind: Some(AutomationScheduleKind::Weekdays),
            time: Some("09:00"),
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

// automation templates

#[test]
fn keeps_popular_examples_in_their_real_category_too() {
    let popular: Vec<&str> = templates_for_category(TemplateCategory::Popular)
        .iter()
        .map(|template| template.id)
        .collect();
    assert_eq!(
        popular,
        [
            "find-critical-bugs",
            "scan-vulnerabilities",
            "generate-docs",
            "add-test-coverage"
        ]
    );
    assert!(
        templates_for_category(TemplateCategory::Review)
            .iter()
            .any(|template| template.popular)
    );
    assert!(
        templates_for_category(TemplateCategory::Security)
            .iter()
            .any(|template| template.popular)
    );
}

#[test]
fn covers_every_gallery_category_with_at_least_one_example() {
    for category in AUTOMATION_TEMPLATE_CATEGORIES {
        assert!(!templates_for_category(category).is_empty());
    }
    assert!(
        AUTOMATION_TEMPLATES
            .iter()
            .all(|template| !template.prompt.trim().is_empty())
    );
}

#[test]
fn only_uses_inbox_events_that_actually_fire() {
    for template in &AUTOMATION_TEMPLATES {
        if template.trigger.kind == AutomationTriggerKind::Time {
            continue;
        }
        assert!(
            supported_inbox_trigger_events(template.trigger.kind).contains(&template.trigger.event),
            "{}",
            template.id
        );
    }
}

// inbox automation events

fn trigger(kind: AutomationTriggerKind, event: &str) -> AutomationTrigger {
    create_automation_trigger(kind, event, TriggerExtras::default())
}

fn github(event: &str) -> AutomationTrigger {
    trigger(AutomationTriggerKind::Github, event)
}

fn item_with(edit: impl FnOnce(&mut InboxEventItem)) -> InboxEventItem {
    let mut item = inbox_item();
    edit(&mut item);
    item
}

#[test]
fn matches_account_wide_jira_issues_and_keeps_their_event_identity_after_a_project_move() {
    let jira = item_with(|item| {
        item.provider = InboxProvider::Jira;
        item.kind = InboxKind::Jira;
        item.id = Some("10042".into());
        item.identifier = Some("ENG-42".into());
        item.repo = "ENG".into();
        item.project_path = String::new();
    });
    let mut jira_trigger = trigger(AutomationTriggerKind::Jira, "issue_created");
    jira_trigger.repos = vec!["ENG".into()];
    assert_eq!(
        inbox_appeared_event(&jira),
        Some(InboxEvent {
            kind: AutomationTriggerKind::Jira,
            event: "issue_created"
        })
    );
    assert_eq!(automation_event_key(&jira), "jira:issue:10042");
    let moved = item_with(|item| {
        *item = jira.clone();
        item.identifier = Some("OPS-17".into());
        item.repo = "OPS".into();
        item.number = 17;
    });
    assert_eq!(automation_event_key(&moved), "jira:issue:10042");
    let automation = review_automation(vec![jira_trigger]);
    let matches = match_inbox_automations(
        std::slice::from_ref(&automation),
        std::slice::from_ref(&jira),
    );
    assert_eq!(matches.len(), 1);
    assert!(matches[0].prompt.contains("Work on this Jira issue:"));
    let other = item_with(|item| {
        *item = jira;
        item.repo = "OPS".into();
    });
    assert!(match_inbox_automations(&[automation], &[other]).is_empty());
}

#[test]
fn maps_opened_prs_drafts_and_issues() {
    let event = |item: InboxEventItem| {
        inbox_appeared_event(&item).map(|event| (event.kind.as_str(), event.event))
    };
    assert_eq!(event(inbox_item()), Some(("github", "pull_request_opened")));
    assert_eq!(
        event(item_with(|item| item.draft = true)),
        Some(("github", "draft_opened"))
    );
    assert_eq!(
        event(item_with(|item| item.kind = InboxKind::Issue)),
        Some(("github", "issue_opened"))
    );
    assert_eq!(
        event(item_with(|item| item.provider = InboxProvider::Gitlab)),
        Some(("gitlab", "merge_request_opened"))
    );
    assert_eq!(
        event(item_with(|item| {
            item.provider = InboxProvider::Gitlab;
            item.kind = InboxKind::Issue;
        })),
        Some(("gitlab", "issue_opened"))
    );
    assert_eq!(
        event(item_with(|item| {
            item.provider = InboxProvider::Linear;
            item.kind = InboxKind::Linear;
            item.id = Some("issue-1".into());
            item.identifier = Some("ENG-12".into());
            item.project_path = String::new();
        })),
        Some(("linear", "issue_created"))
    );
    assert_eq!(
        event(item_with(|item| item.provider = InboxProvider::AzureDevops)),
        Some(("azuredevops", "pull_request_appeared"))
    );
    assert_eq!(
        event(item_with(|item| {
            item.provider = InboxProvider::AzureDevops;
            item.kind = InboxKind::Issue;
        })),
        Some(("azuredevops", "work_item_appeared"))
    );
}

#[test]
fn fires_a_same_project_opened_pr_into_the_matching_automation() {
    let review = review_automation(vec![github("pull_request_opened")]);
    let matches = match_inbox_automations(&[review], &[inbox_item()]);
    let found = &matches[0];
    assert_eq!(found.automation.id, "automation-id");
    assert_eq!(found.event_key, "github:pr:acme/web:12");
    assert_eq!(
        found.occurred_at,
        parse_date("2026-09-19T15:00:00Z").unwrap()
    );
    assert!(
        found
            .prompt
            .contains("Review the newly opened pull request.")
    );
    assert!(found.prompt.contains("Work on this GitHub pull request:"));
    assert!(found.prompt.contains("https://github.com/acme/web/pull/12"));
}

#[test]
fn does_not_treat_a_draft_as_a_ready_pull_request() {
    let review = review_automation(vec![github("pull_request_opened")]);
    assert!(match_inbox_automations(&[review], &[item_with(|item| item.draft = true)]).is_empty());
    let mut drafts = review_automation(vec![github("draft_opened")]);
    drafts.id = "draft-id".into();
    assert_eq!(
        match_inbox_automations(&[drafts], &[item_with(|item| item.draft = true)]).len(),
        1
    );
}

#[test]
fn keeps_automations_scoped_to_their_project() {
    let mut review = review_automation(vec![github("pull_request_opened")]);
    review.cwd = "/tmp/other".into();
    assert!(match_inbox_automations(&[review], &[inbox_item()]).is_empty());
}

#[test]
fn fires_a_same_project_opened_github_issue_into_the_matching_automation() {
    let mut triage = review_automation(vec![github("issue_opened")]);
    triage.name = "Triage GitHub issues".into();
    triage.prompt = "Triage the newly opened GitHub issue.".into();
    let issue = item_with(|item| {
        item.kind = InboxKind::Issue;
        item.url = "https://github.com/acme/web/issues/12".into();
    });
    let matches = match_inbox_automations(std::slice::from_ref(&triage), &[issue]);
    assert_eq!(matches[0].event_key, "github:issue:acme/web:12");
    assert!(
        matches[0]
            .prompt
            .contains("Triage the newly opened GitHub issue.")
    );
    assert!(matches[0].prompt.contains("Work on this GitHub issue:"));
    assert!(match_inbox_automations(&[triage], &[inbox_item()]).is_empty());
}

#[test]
fn fires_gitlab_merge_requests_into_the_matching_project() {
    let review = review_automation(vec![trigger(
        AutomationTriggerKind::Gitlab,
        "merge_request_opened",
    )]);
    let matches = match_inbox_automations(
        &[review],
        &[item_with(|item| {
            item.provider = InboxProvider::Gitlab;
            item.url = "https://gitlab.example.com/acme/web/-/merge_requests/12".into();
        })],
    );
    assert_eq!(matches[0].event_key, "gitlab:pr:acme/web:12");
    assert!(
        matches[0]
            .prompt
            .contains("Work on this GitLab merge request:")
    );
}

#[test]
fn fires_gitlab_issues_separately_from_merge_requests() {
    let triage = review_automation(vec![trigger(AutomationTriggerKind::Gitlab, "issue_opened")]);
    assert!(
        match_inbox_automations(
            std::slice::from_ref(&triage),
            &[item_with(|item| item.provider = InboxProvider::Gitlab)]
        )
        .is_empty()
    );
    let matches = match_inbox_automations(
        &[triage],
        &[item_with(|item| {
            item.provider = InboxProvider::Gitlab;
            item.kind = InboxKind::Issue;
            item.url = "https://gitlab.example.com/acme/web/-/issues/12".into();
        })],
    );
    assert_eq!(matches[0].event_key, "gitlab:issue:acme/web:12");
    assert!(matches[0].prompt.contains("Work on this GitLab issue:"));
}

#[test]
fn fires_azure_devops_pull_requests_into_the_matching_project() {
    let review = review_automation(vec![trigger(
        AutomationTriggerKind::AzureDevops,
        "pull_request_appeared",
    )]);
    let matches = match_inbox_automations(
        &[review],
        &[item_with(|item| item.provider = InboxProvider::AzureDevops)],
    );
    assert_eq!(matches[0].event_key, "azuredevops:pr:acme/web:12");
    assert!(matches[0].prompt.contains("Work on this ADO pull request:"));
}

#[test]
fn fires_azure_devops_work_items_separately_from_pull_requests() {
    let triage = review_automation(vec![trigger(
        AutomationTriggerKind::AzureDevops,
        "work_item_appeared",
    )]);
    assert!(
        match_inbox_automations(
            std::slice::from_ref(&triage),
            &[item_with(|item| item.provider = InboxProvider::AzureDevops)]
        )
        .is_empty()
    );
    let matches = match_inbox_automations(
        &[triage],
        &[item_with(|item| {
            item.provider = InboxProvider::AzureDevops;
            item.kind = InboxKind::Issue;
        })],
    );
    assert_eq!(matches[0].event_key, "azuredevops:issue:acme/web:12");
    assert!(matches[0].prompt.contains("Work on this ADO issue:"));
}

fn linear_item(project_path: &str) -> InboxEventItem {
    item_with(|item| {
        item.provider = InboxProvider::Linear;
        item.kind = InboxKind::Linear;
        item.id = Some("issue-1".into());
        item.identifier = Some("ENG-12".into());
        item.title = "Fix auth".into();
        item.url = "https://linear.app/acme/issue/ENG-12".into();
        item.project_path = project_path.into();
    })
}

#[test]
fn fires_new_linear_issues_into_the_automations_project() {
    let mut triage = review_automation(vec![trigger(
        AutomationTriggerKind::Linear,
        "issue_created",
    )]);
    triage.name = "Triage new issues".into();
    triage.prompt = "Triage the new Linear issue.".into();
    let matches = match_inbox_automations(&[triage], &[linear_item("")]);
    assert_eq!(matches[0].event_key, "linear:issue:issue-1");
    assert_eq!(matches[0].automation.cwd, "/tmp/web");
    assert!(matches[0].prompt.contains("Triage the new Linear issue."));
    assert!(matches[0].prompt.contains("Work on this Linear issue:"));
    assert!(matches[0].prompt.contains("ENG-12 Fix auth"));
}

#[test]
fn still_scopes_linear_issues_when_they_carry_a_project_path() {
    let mut triage = review_automation(vec![trigger(
        AutomationTriggerKind::Linear,
        "issue_created",
    )]);
    triage.cwd = "/tmp/other".into();
    assert!(match_inbox_automations(&[triage], &[linear_item("/tmp/web")]).is_empty());
}

#[test]
fn honors_an_explicit_repo_filter_and_ignores_unverifiable_actors() {
    let filtered = review_automation(vec![create_automation_trigger(
        AutomationTriggerKind::Github,
        "pull_request_opened",
        TriggerExtras {
            repos: Some(vec!["acme/api".into()]),
            ..TriggerExtras::default()
        },
    )]);
    assert!(match_inbox_automations(&[filtered], &[inbox_item()]).is_empty());
    let mut allowed = review_automation(vec![create_automation_trigger(
        AutomationTriggerKind::Github,
        "pull_request_opened",
        TriggerExtras {
            repo: Some("acme/web".into()),
            ..TriggerExtras::default()
        },
    )]);
    allowed.id = "allowed".into();
    assert_eq!(
        match_inbox_automations(&[allowed], &[inbox_item()]).len(),
        1
    );
    let mut authored = review_automation(vec![create_automation_trigger(
        AutomationTriggerKind::Github,
        "pull_request_opened",
        TriggerExtras {
            actor: Some("ada".into()),
            ..TriggerExtras::default()
        },
    )]);
    authored.id = "authored".into();
    assert!(match_inbox_automations(&[authored], &[inbox_item()]).is_empty());
}

#[test]
fn does_not_launch_disabled_or_time_only_automations() {
    let mut disabled = review_automation(vec![github("pull_request_opened")]);
    disabled.enabled = false;
    let mut scheduled = review_automation(vec![trigger(AutomationTriggerKind::Time, "weekdays")]);
    scheduled.id = "time-id".into();
    assert!(match_inbox_automations(&[disabled, scheduled], &[inbox_item()]).is_empty());
}

#[test]
fn launches_each_automation_at_most_once_per_work_item() {
    let review = review_automation(vec![github("pull_request_opened"), github("draft_opened")]);
    assert_eq!(
        match_inbox_automations(&[review], &[inbox_item(), inbox_item()]).len(),
        1
    );
    assert_eq!(
        automation_event_key(&item_with(|item| item.repo = "ACME/web".into())),
        "github:pr:acme/web:12"
    );
}

/// Save `review` through the store and return the stored automation.
fn stored_review(backend: &FlakyAutomations) -> Automation {
    let mut draft = new_automation_draft("/tmp/web", HarnessId::Codex, "model");
    draft.name = "Review pull requests".into();
    draft.prompt = "Review the newly opened pull request.".into();
    draft.triggers = vec![github("pull_request_opened")];
    let now = monocode_store::session_store::now_millis();
    backend.save(automation_upsert(&draft, now))
}

#[test]
fn retries_a_failed_backend_event_claim_on_a_later_poll() {
    let backend = FlakyAutomations::new();
    let review = stored_review(&backend);
    let kv = Kv::in_memory();
    let retries = RefCell::new(InboxRetries::default());
    let now = monocode_store::session_store::now_millis();
    backend.fail_next("automations_claim_event", "database busy");
    let first = futures::executor::block_on(claim_inbox_automation_runs(
        backend.as_ref(),
        &kv,
        &retries,
        &[inbox_item()],
        now,
    ))
    .unwrap();
    assert!(first.is_empty());
    assert!(kv.get_item(RETRY_STORAGE_KEY).is_some());

    // A new window starts with an empty cache and reads the saved list.
    let fresh = RefCell::new(InboxRetries::default());
    let retried = futures::executor::block_on(claim_inbox_automation_runs(
        backend.as_ref(),
        &kv,
        &fresh,
        &[],
        now,
    ))
    .unwrap();
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].due.automation.id, review.id);
    let linked = retried[0].linked_work_item.clone().unwrap();
    assert_eq!(linked.kind, WorkItemKind::Pr);
    assert_eq!(linked.repo, "acme/web");
    assert_eq!(linked.number, 12);
    assert_eq!(linked.url, "https://github.com/acme/web/pull/12");
    assert_eq!(backend.calls("automations_claim_event"), 2);
    assert_eq!(kv.get_item(RETRY_STORAGE_KEY), None);

    // The ledger keeps a second claim of the same item from launching again.
    let again = futures::executor::block_on(claim_inbox_automation_runs(
        backend.as_ref(),
        &kv,
        &fresh,
        &[inbox_item()],
        now,
    ))
    .unwrap();
    assert!(again.is_empty());
}

#[test]
fn claims_due_occurrences_and_skips_runs_past_their_grace() {
    let backend = FlakyAutomations::new();
    let mut draft = new_automation_draft("/tmp/web", HarnessId::Codex, "model");
    draft.name = "Hourly".into();
    draft.prompt = "Check the build.".into();
    draft.missed_run_grace_minutes = 90;
    draft.triggers = vec![create_automation_trigger(
        AutomationTriggerKind::Time,
        "hourly",
        TriggerExtras {
            minute: Some(0),
            ..TriggerExtras::default()
        },
    )];
    let real_now = monocode_store::session_store::now_millis();
    let saved = backend.save(automation_upsert(&draft, real_now));
    // Three hours past the first occurrence: two are past the 90 minute grace.
    let now = saved.next_run_at + 3 * 3_600_000 + 60_000;
    let (claimed, due) =
        futures::executor::block_on(claim_due_automations(backend.as_ref(), now)).unwrap();
    assert!(due);
    assert_eq!(claimed.len(), 2);
    assert!(
        claimed
            .iter()
            .all(|item| item.run.status == AutomationRunStatus::Pending)
    );
    let runs = futures::executor::block_on(backend.runs(saved.id.clone())).unwrap();
    assert_eq!(runs.len(), 4);
    assert_eq!(
        runs.iter()
            .filter(|run| run.status == AutomationRunStatus::Skipped)
            .count(),
        2
    );
    // Nothing is due again until the next hour.
    let (again, due) =
        futures::executor::block_on(claim_due_automations(backend.as_ref(), now)).unwrap();
    assert!(again.is_empty());
    assert!(!due);
}

#[test]
fn keeps_earlier_claims_when_a_later_occurrence_fails() {
    let backend = FlakyAutomations::new();
    let mut draft = new_automation_draft("/tmp/web", HarnessId::Codex, "model");
    draft.name = "Hourly".into();
    draft.prompt = "Check the build.".into();
    draft.triggers = vec![create_automation_trigger(
        AutomationTriggerKind::Time,
        "hourly",
        TriggerExtras::default(),
    )];
    let saved = backend.save(automation_upsert(
        &draft,
        monocode_store::session_store::now_millis(),
    ));
    let now = saved.next_run_at + 2 * 3_600_000;
    let first = futures::executor::block_on(backend.inner.claim_due(
        saved.id.clone(),
        saved.next_run_at,
        saved.next_run_at + 3_600_000,
        now,
    ))
    .unwrap();
    assert!(first.is_some());
    backend.fail_next("automations_claim_due", "database busy");
    let (claimed, _) =
        futures::executor::block_on(claim_due_automations(backend.as_ref(), now)).unwrap();
    assert!(claimed.is_empty());
    let (claimed, _) =
        futures::executor::block_on(claim_due_automations(backend.as_ref(), now)).unwrap();
    assert_eq!(claimed.len(), 2);
}

#[test]
fn event_keys_replace_unsafe_characters() {
    let key = automation_event_key(&item_with(|item| item.repo = "Acme Corp/wéb".into()));
    assert_eq!(key, "github:pr:acme_corp/w_b:12");
    assert_eq!(
        local_ms(2026, 8, 19, 15, 0) - local_ms(2026, 8, 19, 14, 0),
        3_600_000
    );
}
