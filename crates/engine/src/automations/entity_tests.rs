//! Entity tests: the App.tsx automation launch, scheduler, recovery, and
//! Inbox claims over an in-memory store, and the useSessionReminders.test.ts
//! cases over the reminder command mock.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Entity, TestAppContext};
use monocode_core::inbox::WorkItemKind;
use monocode_core::session::WorkspaceMode;
use monocode_core::{HarnessId, RuntimeMode};
use monocode_settings::Kv;

use super::automations::{Automations, REJECTION_MESSAGE};
use super::backend::{AutomationsBackend, ReminderRule, ReminderTarget, SessionReminder};
use super::model::*;
use super::reminders::Reminders;
use super::testing::*;
use crate::attention::notification_preferences::{
    Mute, NotificationCategory, PreferencePatch, update_notification_preferences,
};
use crate::attention::notifications::NOTIFICATIONS_KEY;
use crate::attention::sounds::SOUNDS_KEY;
use crate::attention::testing::TestClock;
use crate::history::session_folders::{SessionFolder, load_session_folders, save_session_folders};
use crate::runtime::Engine;
use crate::runtime::testing::init_test_engine;
use crate::submit::{ControlOutcome, ControlStatus, SubmissionAcceptance};

struct Setup {
    backend: Arc<FlakyAutomations>,
    automations: Entity<Automations>,
    host: Rc<FakeHost>,
    clock: TestClock,
    kv: Kv,
}

/// The result of a task that finished once the executor parked.
fn done<T: 'static>(task: gpui::Task<T>) -> T {
    futures::FutureExt::now_or_never(task).expect("task finished")
}

fn real_now() -> i64 {
    monocode_store::session_store::now_millis()
}

fn setup(cx: &mut TestAppContext) -> Setup {
    setup_with(cx, FlakyAutomations::new(), real_now())
}

fn setup_with(cx: &mut TestAppContext, backend: Arc<FlakyAutomations>, now: i64) -> Setup {
    init_test_engine(cx);
    let clock = TestClock::new(now);
    let kv = Kv::in_memory();
    let host = FakeHost::new();
    let automations = cx.update(|cx| {
        cx.new(|cx| {
            let mut automations = Automations::new(backend.clone(), kv.clone(), clock.clock(), cx);
            automations.set_host(host.clone());
            automations
        })
    });
    Setup {
        backend,
        automations,
        host,
        clock,
        kv,
    }
}

fn draft(name: &str, triggers: Vec<AutomationTrigger>) -> AutomationDraft {
    let mut draft = new_automation_draft("/tmp/web", HarnessId::Codex, "codex:gpt-5.5");
    draft.name = name.into();
    draft.prompt = format!("{name} prompt");
    draft.triggers = triggers;
    draft
}

fn hourly() -> AutomationTrigger {
    create_automation_trigger(
        AutomationTriggerKind::Time,
        "hourly",
        TriggerExtras::default(),
    )
}

fn save(setup: &Setup, draft: AutomationDraft, cx: &mut TestAppContext) -> Automation {
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.save(draft, cx));
    cx.run_until_parked();
    done(task).expect("saved")
}

fn runs(setup: &Setup, automation_id: &str) -> Vec<AutomationRun> {
    futures::executor::block_on(setup.backend.runs(automation_id.to_string())).unwrap()
}

fn session_of(cx: &mut TestAppContext, id: &str) -> monocode_core::Session {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned())
        .expect("open session")
}

#[gpui::test]
fn saving_selects_the_automation_and_loads_its_runs(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let saved = save(&setup, draft("Nightly", vec![hourly()]), cx);
    setup.automations.read_with(cx, |automations, _| {
        assert_eq!(automations.selected_id(), Some(saved.id.as_str()));
        assert_eq!(automations.automations().len(), 1);
        assert!(!automations.is_loading());
        assert!(!automations.is_saving());
        assert!(automations.runs().is_empty());
    });
    assert!(saved.next_run_at > real_now());
}

#[gpui::test]
fn run_now_opens_a_new_session_in_front_and_settles_the_run(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let saved = save(&setup, draft("Find bugs", vec![hourly()]), cx);
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
    cx.run_until_parked();
    done(task).unwrap();

    let submit = setup.host.submits.borrow()[0].clone();
    assert_eq!(submit.text, "Find bugs prompt");
    assert!(!submit.refresh_title);
    let session = session_of(cx, &submit.session_id);
    assert_eq!(session.title, "codex · Find bugs");
    assert_eq!(session.automation_id.as_deref(), Some(saved.id.as_str()));
    assert_eq!(session.workspace_mode, Some(WorkspaceMode::Worktree));
    assert_eq!(session.worktree_base.as_deref(), Some("HEAD"));
    assert_eq!(session.runtime_mode, RuntimeMode::Auto);
    let log = setup.host.log();
    assert!(log[0].starts_with("append:"));
    assert!(log[1].starts_with("activate:"));
    assert_eq!(log[2], "show:/tmp/web");

    let run = &runs(&setup, &saved.id)[0];
    assert_eq!(run.status, AutomationRunStatus::Running);
    assert_eq!(run.session_id.as_deref(), Some(submit.session_id.as_str()));
    setup.automations.read_with(cx, |automations, _| {
        assert!(automations.reservations().contains(&submit.session_id));
        assert_eq!(automations.running(), None);
    });

    cx.update(|cx| {
        setup.host.settle(
            0,
            ControlOutcome {
                status: ControlStatus::Completed,
                text: "done".into(),
                error: None,
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(
        runs(&setup, &saved.id)[0].status,
        AutomationRunStatus::Succeeded
    );
    setup.automations.read_with(cx, |automations, _| {
        assert!(automations.reservations().is_empty());
        assert_eq!(
            automations.selected().unwrap().last_run_status,
            Some(AutomationRunStatus::Succeeded)
        );
    });
}

#[gpui::test]
fn a_rejected_submission_fails_the_run_and_releases_the_session(cx: &mut TestAppContext) {
    let setup = setup(cx);
    setup
        .host
        .on_submit(|_, _, _| SubmissionAcceptance::Ready(false));
    let saved = save(&setup, draft("Rejected", vec![hourly()]), cx);
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
    cx.run_until_parked();
    done(task).unwrap();
    let run = &runs(&setup, &saved.id)[0];
    assert_eq!(run.status, AutomationRunStatus::Failed);
    assert_eq!(run.error.as_deref(), Some(REJECTION_MESSAGE));
    setup.automations.read_with(cx, |automations, _| {
        assert!(automations.reservations().is_empty())
    });
}

#[gpui::test]
fn reuses_the_last_idle_session_in_the_same_folder(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let mut reuse = draft("Reuse", vec![hourly()]);
    reuse.workspace_mode = AutomationWorkspaceMode::Current;
    reuse.reuse_session = true;
    let saved = save(&setup, reuse, cx);
    let first = setup
        .automations
        .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
    cx.run_until_parked();
    done(first).unwrap();
    let first_session = setup.host.submits.borrow()[0].session_id.clone();
    cx.update(|cx| {
        setup.host.settle(
            0,
            ControlOutcome {
                status: ControlStatus::Completed,
                text: String::new(),
                error: None,
            },
            cx,
        )
    });
    cx.run_until_parked();

    let second = setup
        .automations
        .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
    cx.run_until_parked();
    done(second).unwrap();
    assert_eq!(setup.host.submits.borrow()[1].session_id, first_session);
    assert_eq!(setup.host.tabs.borrow().len(), 1);
    assert!(setup.host.log().contains(&format!("focus:{first_session}")));
}

#[gpui::test]
fn a_reserved_session_is_not_reused_while_its_run_is_live(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let mut reuse = draft("Busy", vec![hourly()]);
    reuse.workspace_mode = AutomationWorkspaceMode::Current;
    reuse.reuse_session = true;
    let saved = save(&setup, reuse, cx);
    for _ in 0..2 {
        let task = setup
            .automations
            .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
        cx.run_until_parked();
        done(task).unwrap();
    }
    let submits = setup.host.submits.borrow();
    assert_ne!(submits[0].session_id, submits[1].session_id);
}

#[gpui::test]
fn places_new_sessions_in_the_automations_folder(cx: &mut TestAppContext) {
    let setup = setup(cx);
    save_session_folders(
        &setup.kv,
        "/tmp/web",
        &[SessionFolder::new(
            "folder-1",
            "Automations",
            vec!["older-session".into()],
        )],
    );
    let mut foldered = draft("Foldered", vec![hourly()]);
    foldered.session_folder_id = "folder-1".into();
    let saved = save(&setup, foldered, cx);
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
    cx.run_until_parked();
    done(task).unwrap();
    let session_id = setup.host.submits.borrow()[0].session_id.clone();
    let folders = load_session_folders(&setup.kv, "/tmp/web");
    assert!(folders[0].session_ids.contains(&session_id));
    assert!(
        folders[0]
            .session_ids
            .contains(&"older-session".to_string())
    );
}

#[gpui::test]
fn the_scheduler_claims_due_runs_every_thirty_seconds(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let saved = save(&setup, draft("Hourly", vec![hourly()]), cx);
    setup
        .automations
        .update(cx, |automations, cx| automations.start_scheduler(cx));
    cx.run_until_parked();
    assert!(setup.host.submits.borrow().is_empty());

    // Step to just past the run in 5 s ticks, like a machine that stayed awake.
    setup.clock.set(saved.next_run_at - 10_000);
    for _ in 0..8 {
        setup.clock.advance(cx, Duration::from_secs(5));
    }
    assert_eq!(setup.host.submits.borrow().len(), 1);
    let runs = runs(&setup, &saved.id);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].trigger, AutomationRunTrigger::Scheduled);
    assert_eq!(runs[0].scheduled_for, saved.next_run_at);
}

#[gpui::test]
fn the_scheduler_catches_up_after_the_machine_sleeps(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let mut sleepy = draft("Sleepy", vec![hourly()]);
    sleepy.missed_run_grace_minutes = 90;
    let saved = save(&setup, sleepy, cx);
    setup
        .automations
        .update(cx, |automations, cx| automations.start_scheduler(cx));
    cx.run_until_parked();

    // The wall clock moves three hours while the timers stand still.
    setup.clock.set(saved.next_run_at + 3 * 3_600_000 + 60_000);
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    // Two runs inside the grace period launch; two older ones are skipped.
    assert_eq!(setup.host.submits.borrow().len(), 2);
    let runs = runs(&setup, &saved.id);
    assert_eq!(
        runs.iter()
            .filter(|run| run.status == AutomationRunStatus::Skipped)
            .count(),
        2
    );
}

#[gpui::test]
fn recovery_cancels_interrupted_runs_and_relaunches_pending_ones(cx: &mut TestAppContext) {
    let backend = FlakyAutomations::new();
    let earlier = real_now();
    let mut stored = draft("Recovered", vec![hourly()]);
    stored.id = Some("automation-1".into());
    let saved = backend.save(automation_upsert(&stored, earlier));
    // The last app run left one run running and one pending.
    let running =
        futures::executor::block_on(backend.run_now(saved.id.clone(), earlier - 2_000)).unwrap();
    futures::executor::block_on(backend.run_update(
        running.id.clone(),
        AutomationRunStatus::Running,
        Some("old-session".into()),
        None,
        earlier - 1_500,
    ))
    .unwrap();
    let pending =
        futures::executor::block_on(backend.run_now(saved.id.clone(), earlier - 1_000)).unwrap();

    let setup = setup_with(cx, backend, earlier);
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.evaluate(cx));
    cx.run_until_parked();
    done(task);

    let all = runs(&setup, &saved.id);
    let interrupted = all.iter().find(|run| run.id == running.id).unwrap();
    assert_eq!(interrupted.status, AutomationRunStatus::Cancelled);
    assert_eq!(
        interrupted.error.as_deref(),
        Some("Interrupted when MonoCode last stopped.")
    );
    let relaunched = all.iter().find(|run| run.id == pending.id).unwrap();
    assert_eq!(relaunched.status, AutomationRunStatus::Running);
    assert_eq!(setup.host.submits.borrow().len(), 1);
    assert_eq!(setup.backend.calls("automation_runs_recover"), 1);

    // Recovery runs once per app run.
    let again = setup
        .automations
        .update(cx, |automations, cx| automations.evaluate(cx));
    cx.run_until_parked();
    done(again);
    assert_eq!(setup.backend.calls("automation_runs_recover"), 1);
}

#[gpui::test]
fn a_failed_recovery_runs_again_on_the_next_pass(cx: &mut TestAppContext) {
    let setup = setup(cx);
    setup
        .backend
        .fail_next("automation_runs_recover", "database busy");
    for _ in 0..2 {
        let task = setup
            .automations
            .update(cx, |automations, cx| automations.evaluate(cx));
        cx.run_until_parked();
        done(task);
    }
    // The first pass stops at the failed recovery; the second recovers and
    // claims.
    assert_eq!(setup.backend.calls("automation_runs_recover"), 2);
    assert_eq!(setup.backend.calls("automations_list"), 1);
}

#[gpui::test]
fn inbox_items_launch_matching_automations_with_their_work_item(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let saved = save(
        &setup,
        draft(
            "Review",
            vec![create_automation_trigger(
                AutomationTriggerKind::Github,
                "pull_request_opened",
                TriggerExtras::default(),
            )],
        ),
        cx,
    );
    let task = setup.automations.update(cx, |automations, cx| {
        automations.inbox_appeared(vec![inbox_item()], cx)
    });
    cx.run_until_parked();
    done(task);
    let submit = setup.host.submits.borrow()[0].clone();
    assert!(submit.refresh_title);
    assert!(
        submit
            .text
            .starts_with("Review prompt\n\nWork on this GitHub pull request:")
    );
    let session = session_of(cx, &submit.session_id);
    assert_eq!(session.title, "codex");
    let linked = session.linked_work_item.unwrap();
    assert_eq!(linked.kind, WorkItemKind::Pr);
    assert_eq!(linked.number, 12);
    let run = &runs(&setup, &saved.id)[0];
    assert_eq!(run.trigger, AutomationRunTrigger::Event);
    assert_eq!(run.event_key.as_deref(), Some("github:pr:acme/web:12"));

    // The same item never launches twice.
    let again = setup.automations.update(cx, |automations, cx| {
        automations.inbox_appeared(vec![inbox_item()], cx)
    });
    cx.run_until_parked();
    done(again);
    assert_eq!(setup.host.submits.borrow().len(), 1);
}

#[gpui::test]
fn a_failed_running_update_records_the_failure(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let saved = save(&setup, draft("Broken", vec![hourly()]), cx);
    setup
        .backend
        .fail_next("automation_run_update", "database busy");
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.run_now(&saved.id, cx));
    cx.run_until_parked();
    let result = done(task);
    assert_eq!(result, Err("database busy".into()));
    let run = &runs(&setup, &saved.id)[0];
    assert_eq!(run.status, AutomationRunStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("database busy"));
    assert!(setup.host.submits.borrow().is_empty());
    setup.automations.read_with(cx, |automations, _| {
        assert_eq!(automations.error(), Some("database busy"));
        assert!(automations.reservations().is_empty());
    });
}

#[gpui::test]
fn disabling_and_deleting_update_the_list(cx: &mut TestAppContext) {
    let setup = setup(cx);
    let saved = save(&setup, draft("Toggle", vec![hourly()]), cx);
    let task = setup.automations.update(cx, |automations, cx| {
        automations.set_enabled(&saved, false, cx)
    });
    cx.run_until_parked();
    done(task).unwrap();
    setup.automations.read_with(cx, |automations, _| {
        assert!(!automations.automations()[0].enabled);
    });
    let task = setup
        .automations
        .update(cx, |automations, cx| automations.delete(&saved.id, cx));
    cx.run_until_parked();
    done(task).unwrap();
    setup.automations.read_with(cx, |automations, _| {
        assert!(automations.automations().is_empty());
        assert_eq!(automations.selected_id(), None);
    });
}

// useSessionReminders.test.ts

struct ReminderSetup {
    backend: Arc<FakeReminders>,
    reminders: Entity<Reminders>,
    app: Rc<FakeReminderApp>,
    window: Rc<FakeReminderWindow>,
    kv: Kv,
    clock: TestClock,
}

/// `reminder`: due at 100, in a project outside the window's.
fn reminder() -> SessionReminder {
    SessionReminder {
        session_id: "saved-session".into(),
        due_at: 100,
        fired_at: None,
        title: "Continue this work".into(),
        harness: HarnessId::Codex,
        cwd: "/another-project".into(),
    }
}

fn reminder_setup(cx: &mut TestAppContext, stored: Vec<SessionReminder>) -> ReminderSetup {
    init_test_engine(cx);
    let backend = FakeReminders::new(stored);
    let kv = Kv::in_memory();
    // The tests mock both settings off.
    kv.set_item(NOTIFICATIONS_KEY, "false");
    kv.set_item(SOUNDS_KEY, "false");
    let clock = TestClock::new(real_now());
    let app = Rc::new(FakeReminderApp::default());
    let window = Rc::new(FakeReminderWindow::default());
    let reminders = cx.update(|cx| {
        cx.new(|cx| {
            Reminders::new(
                backend.clone(),
                kv.clone(),
                clock.clock(),
                None,
                app.clone(),
                cx,
            )
        })
    });
    cx.update(|cx| open_session("saved-session", "/another-project", cx));
    ReminderSetup {
        backend,
        reminders,
        app,
        window,
        kv,
        clock,
    }
}

fn mount(setup: &ReminderSetup, cx: &mut TestAppContext) {
    let window = setup.window.clone();
    setup.reminders.update(cx, |reminders, cx| {
        reminders.attach_window("main", window, cx);
        reminders.register_window("main", vec!["current-session".into()]);
    });
    cx.run_until_parked();
}

fn due(setup: &ReminderSetup, cx: &mut TestAppContext) -> Vec<SessionReminder> {
    setup
        .reminders
        .read_with(cx, |reminders, _| reminders.due())
}

fn rule(setup: &ReminderSetup, session_id: &str) -> ReminderRule {
    setup
        .backend
        .last_configuration()
        .unwrap()
        .project_rules
        .get(session_id)
        .copied()
        .unwrap()
}

fn mute(setup: &ReminderSetup, patch: PreferencePatch) {
    update_notification_preferences(
        &setup.kv,
        &["local:/another-project"],
        &patch,
        setup.clock.now(),
    );
}

#[gpui::test]
fn applies_a_path_based_project_mute_without_discovery(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mute(&setup, PreferencePatch::mute(Some(Mute::UntilResumed)));
    mount(&setup, cx);
    assert!(due(&setup, cx).is_empty());
    assert_eq!(
        rule(&setup, "saved-session"),
        ReminderRule {
            enabled: false,
            after: 0
        }
    );
    setup.reminders.read_with(cx, |reminders, _| {
        assert_eq!(reminders.reminders(), [reminder()])
    });
}

#[gpui::test]
fn keeps_reminders_actionable_even_when_a_project_path_no_longer_exists(cx: &mut TestAppContext) {
    let unavailable = SessionReminder {
        session_id: "unavailable-session".into(),
        cwd: "/deleted-worktree".into(),
        ..reminder()
    };
    let setup = reminder_setup(cx, vec![reminder(), unavailable.clone()]);
    mount(&setup, cx);
    setup.reminders.read_with(cx, |reminders, _| {
        assert_eq!(reminders.reminders(), [reminder(), unavailable.clone()]);
        assert_eq!(reminders.due(), [reminder(), unavailable.clone()]);
        assert_eq!(reminders.error(), None);
    });
    let rules = setup.backend.last_configuration().unwrap().project_rules;
    assert_eq!(
        rules.get("saved-session"),
        Some(&ReminderRule {
            enabled: true,
            after: 0
        })
    );
    assert_eq!(
        rules.get("unavailable-session"),
        Some(&ReminderRule {
            enabled: true,
            after: 0
        })
    );
    let task = setup.reminders.update(cx, |reminders, cx| {
        reminders.cancel(vec![unavailable.session_id.clone()], None, cx)
    });
    cx.run_until_parked();
    done(task).unwrap();
    setup.reminders.read_with(cx, |reminders, _| {
        assert_eq!(reminders.reminders(), [reminder()]);
        assert_eq!(reminders.error(), None);
    });
}

#[gpui::test]
fn serializes_native_configuration_so_a_slower_old_update_cannot_overwrite_a_newer_mute(
    cx: &mut TestAppContext,
) {
    let setup = reminder_setup(cx, vec![reminder()]);
    let release = setup.backend.hold_configure();
    mount(&setup, cx);
    mute(&setup, PreferencePatch::mute(Some(Mute::UntilResumed)));
    cx.run_until_parked();
    assert_eq!(setup.backend.configurations.lock().len(), 1);
    release.send(()).unwrap();
    cx.run_until_parked();
    let configurations = setup.backend.configurations.lock().clone();
    assert_eq!(configurations.len(), 2);
    assert!(!configurations[1].project_rules["saved-session"].enabled);
}

#[gpui::test]
fn applies_reminder_category_changes_immediately_without_replaying_an_old_due_reminder_on_resume(
    cx: &mut TestAppContext,
) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    assert_eq!(due(&setup, cx), [reminder()]);
    mute(
        &setup,
        PreferencePatch::disabled(vec![NotificationCategory::Reminders]),
    );
    cx.run_until_parked();
    assert!(due(&setup, cx).is_empty());
    assert!(!rule(&setup, "saved-session").enabled);
    mute(&setup, PreferencePatch::disabled(Vec::new()));
    cx.run_until_parked();
    assert!(due(&setup, cx).is_empty());
    let resumed = rule(&setup, "saved-session");
    assert!(resumed.enabled);
    assert!(resumed.after > reminder().due_at);
}

#[gpui::test]
fn suppresses_a_muted_projects_reminders_in_native_delivery_and_notices_while_keeping_sidebar_data(
    cx: &mut TestAppContext,
) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mute(&setup, PreferencePatch::mute(Some(Mute::UntilResumed)));
    mount(&setup, cx);
    setup.reminders.read_with(cx, |reminders, _| {
        assert_eq!(reminders.reminders(), [reminder()]);
        assert!(reminders.due().is_empty());
    });
    let rules = setup.backend.last_configuration().unwrap().project_rules;
    assert_eq!(rules.len(), 1);
    assert!(!rules["saved-session"].enabled);
}

#[gpui::test]
fn shows_missed_reminders_on_startup_even_with_desktop_notifications_off(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    assert_eq!(due(&setup, cx), [reminder()]);
    let configuration = setup.backend.last_configuration().unwrap();
    assert!(!configuration.notifications_enabled);
    assert!(!configuration.sound);
    assert!(setup.window.opened.borrow().is_empty());
}

#[gpui::test]
fn saves_the_conversation_before_scheduling_and_does_not_claim_success_on_failure(
    cx: &mut TestAppContext,
) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    setup
        .app
        .save_failures
        .borrow_mut()
        .push_back("Save failed".into());
    let due_at = setup.clock.now() + 60_000;
    let task = setup.reminders.update(cx, |reminders, cx| {
        reminders.schedule(vec!["saved-session".into()], due_at, cx)
    });
    cx.run_until_parked();
    assert!(done(task).is_err());
    assert!(setup.backend.calls("reminder_set").is_empty());
    assert_eq!(*setup.app.errors.borrow(), ["Save failed"]);
    setup.reminders.read_with(cx, |reminders, _| {
        assert_eq!(reminders.reminders(), [reminder()])
    });

    let task = setup.reminders.update(cx, |reminders, cx| {
        reminders.schedule(vec!["saved-session".into()], due_at, cx)
    });
    cx.run_until_parked();
    done(task).unwrap();
    assert!(due(&setup, cx).is_empty());
    assert_eq!(
        setup.app.saved.borrow().last().unwrap(),
        &vec!["saved-session".to_string()]
    );
}

#[gpui::test]
fn routes_opening_through_the_window_choice_then_loads_the_saved_session_and_clears_its_due_reminder(
    cx: &mut TestAppContext,
) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    setup
        .reminders
        .update(cx, |reminders, cx| {
            reminders.open(ReminderTarget::from(&reminder()), cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(*setup.window.opened.borrow(), ["saved-session"]);
    assert_eq!(setup.window.forward.get(), 1);
    assert_eq!(
        setup.backend.calls("reminder_clear"),
        [serde_json::json!({ "sessionIds": ["saved-session"], "expectedDueAt": 100 })]
    );
    assert!(due(&setup, cx).is_empty());
}

#[gpui::test]
fn dismisses_the_current_due_reminder_when_its_session_continues(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    let task = setup.reminders.update(cx, |reminders, cx| {
        reminders.dismiss_due("saved-session", cx)
    });
    cx.run_until_parked();
    done(task).unwrap();
    assert_eq!(
        setup.backend.calls("reminder_clear"),
        [serde_json::json!({ "sessionIds": ["saved-session"], "expectedDueAt": 100 })]
    );
    assert!(due(&setup, cx).is_empty());
}

#[gpui::test]
fn keeps_a_future_reminder_when_its_session_continues_early(cx: &mut TestAppContext) {
    let future = SessionReminder {
        due_at: real_now() + 60_000,
        ..reminder()
    };
    let setup = reminder_setup(cx, vec![future.clone()]);
    mount(&setup, cx);
    let task = setup.reminders.update(cx, |reminders, cx| {
        reminders.dismiss_due("saved-session", cx)
    });
    cx.run_until_parked();
    done(task).unwrap();
    assert!(setup.backend.calls("reminder_clear").is_empty());
    setup.reminders.read_with(cx, |reminders, _| {
        assert_eq!(reminders.reminders(), std::slice::from_ref(&future))
    });
}

#[gpui::test]
fn handles_a_notification_click_queued_before_the_window_mounted(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    setup.reminders.update(cx, |reminders, cx| {
        reminders.open_from_notification("reminder:saved-session:100", cx)
    });
    assert_eq!(setup.app.new_windows.get(), 1);
    assert!(setup.window.opened.borrow().is_empty());
    mount(&setup, cx);
    assert_eq!(*setup.window.opened.borrow(), ["saved-session"]);
    assert!(due(&setup, cx).is_empty());
}

#[gpui::test]
fn keeps_the_reminder_if_opening_the_session_fails(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    *setup.window.fail.borrow_mut() = Some("Unavailable".into());
    setup.reminders.update(cx, |reminders, cx| {
        reminders.open_from_notification("reminder:saved-session:100", cx)
    });
    mount(&setup, cx);
    assert_eq!(due(&setup, cx), [reminder()]);
    assert!(setup.backend.calls("reminder_clear").is_empty());
    assert_eq!(*setup.app.errors.borrow(), ["Unavailable"]);
}

#[gpui::test]
fn updates_when_another_window_cancels_and_forgets_a_closed_window(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    setup.backend.stored.lock().clear();
    setup
        .reminders
        .update(cx, |reminders, cx| reminders.notify_changed(cx));
    cx.run_until_parked();
    setup
        .reminders
        .read_with(cx, |reminders, _| assert!(reminders.reminders().is_empty()));
    setup
        .reminders
        .update(cx, |reminders, _| reminders.detach_window("main"));
    setup.reminders.read_with(cx, |reminders, _| {
        assert!(reminders.window_labels().is_empty())
    });
}

#[gpui::test]
fn opens_in_the_window_that_shows_the_session(cx: &mut TestAppContext) {
    let setup = reminder_setup(cx, vec![reminder()]);
    mount(&setup, cx);
    let other = Rc::new(FakeReminderWindow::default());
    setup.reminders.update(cx, |reminders, cx| {
        reminders.attach_window("window-2", other.clone(), cx);
        reminders.register_window("window-2", vec!["saved-session".into()]);
    });
    cx.run_until_parked();
    setup
        .reminders
        .update(cx, |reminders, cx| {
            reminders.open(ReminderTarget::from(&reminder()), cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(*other.opened.borrow(), ["saved-session"]);
    assert!(setup.window.opened.borrow().is_empty());
}

#[gpui::test]
fn the_poller_shows_banners_for_claimed_reminders(cx: &mut TestAppContext) {
    init_test_engine(cx);
    let store = memory_store();
    seed_session(&store, "saved-session", "/one", "A saved conversation");
    let backend = Arc::new(super::backend::StoreRemindersBackend::inline(store));
    let kv = Kv::in_memory();
    kv.set_item(NOTIFICATIONS_KEY, "true");
    let platform = Arc::new(crate::attention::testing::FakePlatform::default());
    let reminders = cx.update(|cx| {
        cx.new(|cx| {
            Reminders::new(
                backend.clone(),
                kv.clone(),
                TestClock::new(real_now()).clock(),
                Some(platform.clone()),
                Rc::new(FakeReminderApp::default()),
                cx,
            )
        })
    });
    let due_at = real_now() + 20;
    futures::executor::block_on(super::backend::RemindersBackend::set(
        backend.as_ref(),
        vec!["saved-session".into()],
        due_at,
    ))
    .unwrap();
    let task = reminders.update(cx, |reminders, cx| reminders.refresh(cx));
    cx.run_until_parked();
    done(task);
    std::thread::sleep(Duration::from_millis(40));
    reminders.update(cx, |reminders, cx| reminders.start(cx));
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    let banners = platform.banners.lock().clone();
    assert_eq!(banners.len(), 1);
    assert_eq!(
        banners[0].session_id,
        format!("reminder:saved-session:{due_at}")
    );
    assert_eq!(banners[0].text.subtitle, "A saved conversation");
    assert_eq!(
        banners[0].text.body,
        "Reminder: continue this conversation."
    );
    reminders.read_with(cx, |reminders, _| {
        assert!(reminders.reminders()[0].fired_at.is_some());
    });
}

// sessionReminders.test.ts

use super::local_time::local_ms;
use super::reminders::reminder_time;

#[test]
fn uses_elapsed_hours_across_midnight() {
    let now = local_ms(2026, 8, 11, 23, 45);
    assert_eq!(
        reminder_time("reminder:1h", now),
        Some(local_ms(2026, 8, 12, 0, 45))
    );
    assert_eq!(reminder_time("reminder:3h", now), Some(now + 10_800_000));
}

#[test]
fn never_schedules_this_evening_in_the_past() {
    assert_eq!(
        reminder_time("reminder:evening", local_ms(2026, 8, 11, 17, 59)),
        Some(local_ms(2026, 8, 11, 18, 0))
    );
    assert_eq!(
        reminder_time("reminder:evening", local_ms(2026, 8, 11, 18, 0)),
        None
    );
    assert_eq!(
        reminder_time("reminder:evening", local_ms(2026, 8, 11, 23, 0)),
        None
    );
}

#[test]
fn uses_local_9am_tomorrow_across_a_year_boundary() {
    assert_eq!(
        reminder_time("reminder:tomorrow", local_ms(2026, 11, 31, 21, 0)),
        Some(local_ms(2027, 0, 1, 9, 0))
    );
}

#[test]
fn chooses_next_monday() {
    // Friday to Monday, Sunday to Monday, Monday to next Monday even before 9.
    for (day, monday) in [(11, 14), (13, 14), (14, 21)] {
        assert_eq!(
            reminder_time("reminder:next-week", local_ms(2026, 8, day, 8, 0)),
            Some(local_ms(2026, 8, monday, 9, 0)),
            "September {day}"
        );
    }
}

#[test]
fn rejects_unknown_and_cancel_actions() {
    assert_eq!(reminder_time("reminder:cancel", 0), None);
    assert_eq!(reminder_time("unknown", 0), None);
    assert!(
        super::reminders::format_reminder_time(local_ms(2026, 8, 21, 9, 0))
            .starts_with("Mon, Sep 21, 9:00")
    );
}
