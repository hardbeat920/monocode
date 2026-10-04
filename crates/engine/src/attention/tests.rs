//! Entity tests for the attention package, ported from the App.tsx queue and
//! usage limit behavior, accountUsage.test.ts, rateLimitsCache behavior,
//! useInputNotifications.test.ts, notificationDelivery.test.ts, and the
//! policy half of projectNotificationFlow.test.ts.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{Entity, TestAppContext};
use monocode_core::HarnessId;
use monocode_core::block::{Block, BlockNotice, BlockRole, TurnIntent};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::session::{MessageQueueStatus, QueuedMessage, Session, UsageLimit};
use monocode_core::user_question::UserQuestionReply;

use super::notification_preferences::{
    Mute, NotificationCategory, NotificationSubject, PreferencePatch,
    update_notification_preferences,
};
use super::notifications::{NotificationEvent, save_notifications_enabled};
use super::queue::tests::queued;
use super::rate_limits::{ProviderRateLimits, RateLimitProvider, RateLimitStatus, RateLimitWindow};
use super::sound_synth::SoundName;
use super::sounds::SoundCue;
use super::testing::{
    Routed, TestAttention, TestWorkspaceGlobal, init_test_attention, init_test_attention_with,
    init_test_engine_for_attention,
};
use super::*;
use crate::runtime::in_flight::CONTINUE_PROMPT;
use crate::runtime::sessions::Sessions;

const NOW: i64 = 1_790_000_000_000;

/// The output of a future that already finished under `run_until_parked`.
fn finished<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::FutureExt::now_or_never(future).expect("the future finished")
}

fn chat(id: &str, harness: HarnessId, cwd: &str) -> Session {
    Session::blank(id, harness, "model", cwd)
}

fn sessions(cx: &TestAppContext) -> Entity<Sessions> {
    cx.read(Engine::sessions)
}

/// Open `session` in a tab, so it stays in memory while idle.
fn upsert(cx: &mut TestAppContext, session: Session) {
    if let Some(workspace) = cx.read(|cx| {
        cx.try_global::<TestWorkspaceGlobal>()
            .map(|global| global.0.clone())
    }) {
        let mut tabs = workspace.tab_session_ids.borrow_mut();
        if !tabs.contains(&session.id) {
            tabs.push(session.id.clone());
        }
    }
    let sessions = sessions(cx);
    sessions.update(cx, |sessions, cx| sessions.upsert(session, cx));
    cx.run_until_parked();
}

fn get(cx: &TestAppContext, id: &str) -> Session {
    sessions(cx).read_with(cx, |sessions, _| {
        sessions.get(id).cloned().expect("open session")
    })
}

fn attention(cx: &TestAppContext) -> (Entity<Notifier>, Entity<Approvals>, Entity<RateLimits>) {
    cx.update(|cx| {
        let attention = Attention::global(cx);
        (
            attention.notifier.clone(),
            attention.approvals.clone(),
            attention.rate_limits.clone(),
        )
    })
}

fn window(used_percent: f64, resets_at: Option<i64>) -> RateLimitWindow {
    RateLimitWindow {
        used_percent,
        window_minutes: 300,
        resets_at,
    }
}

fn limits(provider: RateLimitProvider, session: Option<RateLimitWindow>) -> ProviderRateLimits {
    ProviderRateLimits {
        provider,
        session,
        weekly: None,
        monthly: None,
        reset_credits: None,
        updated_at: NOW,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

// Queues.

fn queued_chat(id: &str) -> Session {
    let mut session = chat(id, HarnessId::Claude, "/tmp/project");
    session.queued_messages = Some(vec![queued("a", "first"), queued("b", "second")]);
    session.queue_status = Some(MessageQueueStatus::Active);
    session
}

#[gpui::test]
fn dispatches_an_idle_sessions_queued_head_once(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    upsert(cx, queued_chat("s1"));
    let requests = test.submit.requests.borrow().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].session_id, "s1");
    assert_eq!(requests[0].text, "first");
    assert_eq!(requests[0].queued_message_id.as_deref(), Some("a"));
    assert_eq!(requests[0].follow_up_behavior, None);
}

#[gpui::test]
fn holds_busy_sessions_and_an_edited_head(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let mut busy = queued_chat("busy");
    busy.busy = Some(true);
    upsert(cx, busy);
    let mut editing = queued_chat("editing");
    editing.editing_queued_message_id = Some("a".into());
    upsert(cx, editing);
    assert!(test.submit.requests.borrow().is_empty());

    cx.update(|cx| Queues::set_editing("editing", None, cx));
    cx.run_until_parked();
    let requests = test.submit.requests.borrow().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].session_id, "editing");
}

#[gpui::test]
fn a_resuming_queue_turns_active_once_idle_and_then_dispatches(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let mut resuming = queued_chat("s1");
    resuming.queue_status = Some(MessageQueueStatus::Resuming);
    upsert(cx, resuming);
    assert_eq!(get(cx, "s1").queue_status, Some(MessageQueueStatus::Active));
    assert_eq!(test.submit.requests.borrow().len(), 1);
}

#[gpui::test]
fn steer_sends_any_row_now_but_orchestration_waits_for_a_busy_turn(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let mut busy = queued_chat("s1");
    busy.busy = Some(true);
    let mut orchestrate = queued("o", "plan it");
    orchestrate.intent = Some(TurnIntent::Orchestrate);
    busy.queued_messages.as_mut().unwrap().push(orchestrate);
    upsert(cx, busy);

    cx.update(|cx| Queues::steer("s1", "b", cx));
    let requests = test.submit.requests.borrow().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].queued_message_id.as_deref(), Some("b"));
    assert_eq!(
        requests[0].follow_up_behavior,
        Some(monocode_core::settings::FollowUpBehavior::Steer)
    );

    cx.update(|cx| Queues::steer("s1", "o", cx));
    cx.run_until_parked();
    assert_eq!(test.submit.requests.borrow().len(), 1);
    assert!(
        get(cx, "s1")
            .blocks
            .iter()
            .any(|block| block.text == queues::ORCHESTRATION_WAITS_STATUS)
    );
    cx.update(|cx| Queues::steer("s1", "missing", cx));
    assert_eq!(test.submit.requests.borrow().len(), 1);
}

#[gpui::test]
fn resume_continues_a_paused_queue(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    test.submit.start_turns.set(true);
    let mut paused = queued_chat("s1");
    paused.queue_status = Some(MessageQueueStatus::Paused);
    upsert(cx, paused);
    assert!(test.submit.requests.borrow().is_empty());

    cx.update(|cx| Queues::resume("s1", cx));
    let requests = test.submit.requests.borrow().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].text, CONTINUE_PROMPT);
    assert_eq!(
        requests[0].follow_up_behavior,
        Some(monocode_core::settings::FollowUpBehavior::Steer)
    );
    assert_eq!(
        get(cx, "s1").queue_status,
        Some(MessageQueueStatus::Resuming)
    );
}

#[gpui::test]
fn edits_and_deletes_queued_rows(cx: &mut TestAppContext) {
    let _test = init_test_attention(cx, NOW);
    let mut busy = queued_chat("s1");
    busy.busy = Some(true);
    busy.editing_queued_message_id = Some("b".into());
    upsert(cx, busy);
    cx.update(|cx| Queues::edit("s1", "b", "changed", cx));
    let session = get(cx, "s1");
    let texts: Vec<String> = session
        .queued_messages
        .iter()
        .flatten()
        .map(|message: &QueuedMessage| message.text.clone())
        .collect();
    assert_eq!(texts, ["first", "changed"]);
    assert_eq!(session.editing_queued_message_id, None);
    cx.update(|cx| Queues::delete("s1", "a", cx));
    cx.update(|cx| Queues::delete("s1", "b", cx));
    let session = get(cx, "s1");
    assert_eq!(session.queued_messages, None);
    assert_eq!(session.queue_status, None);
}

// Usage limits.

fn limited(id: &str, harness: HarnessId, limit: UsageLimit) -> Session {
    let mut session = chat(id, harness, "/tmp/project");
    session.usage_limit = Some(limit);
    session
}

#[gpui::test]
fn resumes_an_armed_limit_once_the_reset_and_grace_period_pass(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    upsert(
        cx,
        limited(
            "s1",
            HarnessId::Codex,
            UsageLimit {
                resets_at: Some(NOW + 10_000),
                resume_at_reset: Some(true),
            },
        ),
    );
    assert!(test.submit.requests.borrow().is_empty());
    test.clock.advance(cx, Duration::from_millis(39_000));
    assert!(test.submit.requests.borrow().is_empty());
    test.clock.advance(cx, Duration::from_millis(1_000));
    let requests = test.submit.requests.borrow().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].text, CONTINUE_PROMPT);
    assert_eq!(get(cx, "s1").usage_limit, None);
}

#[gpui::test]
fn rechecks_at_most_every_minute_and_skips_unarmed_or_busy_sessions(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let mut busy = limited(
        "busy",
        HarnessId::Codex,
        UsageLimit {
            resets_at: Some(NOW - 60_000),
            resume_at_reset: Some(true),
        },
    );
    busy.busy = Some(true);
    upsert(cx, busy);
    upsert(
        cx,
        limited(
            "unarmed",
            HarnessId::Codex,
            UsageLimit {
                resets_at: Some(NOW - 60_000),
                resume_at_reset: None,
            },
        ),
    );
    upsert(
        cx,
        limited(
            "later",
            HarnessId::Codex,
            UsageLimit {
                resets_at: Some(NOW + 3_600_000),
                resume_at_reset: Some(true),
            },
        ),
    );
    test.clock.advance(cx, Duration::from_millis(60_000));
    assert!(test.submit.requests.borrow().is_empty());
    test.clock
        .advance(cx, Duration::from_millis(3_600_000 - 60_000 + 30_000));
    // The clock jumped past the reset; the next minute's check resumes it.
    test.clock.advance(cx, Duration::from_millis(60_000));
    let ids: Vec<String> = test
        .submit
        .requests
        .borrow()
        .iter()
        .map(|request| request.session_id.clone())
        .collect();
    assert_eq!(ids, ["later"]);
}

#[gpui::test]
fn asks_the_provider_when_the_stream_did_not_say_when_the_limit_resets(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let mut spent = limits(
        RateLimitProvider::Claude,
        Some(window(100.0, Some(NOW + 5_000))),
    );
    spent.weekly = Some(window(100.0, Some(NOW + 9_000)));
    test.fetcher.set_default(RateLimitProvider::Claude, spent);
    let mut session = limited("s1", HarnessId::Claude, UsageLimit::default());
    session.provider_account_id = Some("account-work".into());
    upsert(cx, session);
    assert_eq!(
        get(cx, "s1").usage_limit,
        Some(UsageLimit {
            resets_at: Some(NOW + 9_000),
            resume_at_reset: None,
        })
    );
    assert_eq!(
        test.fetcher.accounts_called(RateLimitProvider::Claude),
        ["account-work"]
    );

    // Pi has no usage feed for this; nothing is asked.
    upsert(cx, limited("pi", HarnessId::Pi, UsageLimit::default()));
    assert_eq!(test.fetcher.calls.lock().len(), 1);
}

#[gpui::test]
fn dismissing_and_arming_change_only_that_sessions_limit(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    upsert(
        cx,
        limited(
            "s1",
            HarnessId::Codex,
            UsageLimit {
                resets_at: Some(NOW + 3_600_000),
                resume_at_reset: None,
            },
        ),
    );
    cx.update(|cx| UsageLimits::set_resume_at_reset("s1", true, cx));
    assert_eq!(
        get(cx, "s1").usage_limit.unwrap().resume_at_reset,
        Some(true)
    );
    cx.update(|cx| UsageLimits::dismiss("s1", cx));
    assert_eq!(get(cx, "s1").usage_limit, None);
    cx.update(|cx| UsageLimits::resume("s1", cx));
    assert!(test.submit.requests.borrow().is_empty());
}

// Rate limits.

#[gpui::test]
fn shares_an_in_flight_account_request(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let (_, _, rate_limits) = attention(cx);
    let gate = test.fetcher.hold_next(RateLimitProvider::Claude);
    let (first, second) = rate_limits.update(cx, |rate_limits, cx| {
        (
            rate_limits.load(RateLimitProvider::Claude, "default", false, cx),
            rate_limits.load(RateLimitProvider::Claude, "default", false, cx),
        )
    });
    assert_eq!(test.fetcher.call_count(RateLimitProvider::Claude), 1);
    rate_limits.read_with(cx, |rate_limits, _| {
        assert_eq!(
            rate_limits.get(RateLimitProvider::Claude, "default").status,
            RateLimitStatus::Fetching
        );
    });
    let _ = gate.send(limits(RateLimitProvider::Claude, Some(window(15.0, None))));
    cx.run_until_parked();
    let first = finished(first);
    let second = finished(second);
    assert_eq!(first, second);
    let cached = rate_limits.update(cx, |rate_limits, cx| {
        rate_limits.load(RateLimitProvider::Claude, "default", false, cx)
    });
    assert_eq!(finished(cached).session.unwrap().used_percent, 15.0);
    assert_eq!(test.fetcher.call_count(RateLimitProvider::Claude), 1);
}

#[gpui::test]
fn runs_an_explicit_refresh_after_an_in_flight_first_load(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let (_, _, rate_limits) = attention(cx);
    let gate = test.fetcher.hold_next(RateLimitProvider::Claude);
    test.fetcher.push(
        RateLimitProvider::Claude,
        limits(RateLimitProvider::Claude, Some(window(75.0, None))),
    );
    let (first, refreshed, again) = rate_limits.update(cx, |rate_limits, cx| {
        (
            rate_limits.load(RateLimitProvider::Claude, "default", false, cx),
            rate_limits.load(RateLimitProvider::Claude, "default", true, cx),
            rate_limits.load(RateLimitProvider::Claude, "default", true, cx),
        )
    });
    assert_eq!(test.fetcher.call_count(RateLimitProvider::Claude), 1);
    let _ = gate.send(limits(RateLimitProvider::Claude, Some(window(10.0, None))));
    cx.run_until_parked();
    assert_eq!(finished(first).session.unwrap().used_percent, 10.0);
    assert_eq!(finished(refreshed).session.unwrap().used_percent, 75.0);
    assert_eq!(finished(again).session.unwrap().used_percent, 75.0);
    assert_eq!(test.fetcher.call_count(RateLimitProvider::Claude), 2);
}

#[gpui::test]
fn loads_every_account_of_the_provider_once_and_refreshes_on_request(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    test.kv.set_item(
        "monocode.providerAccounts.v1",
        &serde_json::json!({ "claude": [{ "id": "account-work", "provider": "claude", "label": "Work" }] })
            .to_string(),
    );
    test.fetcher.set_default(
        RateLimitProvider::Claude,
        limits(RateLimitProvider::Claude, Some(window(12.0, None))),
    );
    let (_, _, rate_limits) = attention(cx);
    let load = rate_limits.update(cx, |rate_limits, cx| {
        rate_limits.load_accounts(Some(HarnessId::Claude), cx)
    });
    assert!(rate_limits.read_with(cx, |rate_limits, _| rate_limits.accounts_refreshing()));
    cx.run_until_parked();
    drop(load);
    let mut called = test.fetcher.accounts_called(RateLimitProvider::Claude);
    called.sort();
    assert_eq!(called, ["account-work", "default"]);
    rate_limits.read_with(cx, |rate_limits, _| {
        assert!(!rate_limits.accounts_refreshing());
        assert_eq!(
            rate_limits.all()["claude:account-work"]
                .session
                .unwrap()
                .used_percent,
            12.0
        );
    });

    rate_limits
        .update(cx, |rate_limits, cx| {
            rate_limits.load_accounts(Some(HarnessId::Claude), cx)
        })
        .detach();
    cx.run_until_parked();
    assert_eq!(test.fetcher.call_count(RateLimitProvider::Claude), 2);

    rate_limits
        .update(cx, |rate_limits, cx| {
            rate_limits.refresh_accounts(Some(HarnessId::Claude), cx)
        })
        .detach();
    cx.run_until_parked();
    assert_eq!(test.fetcher.call_count(RateLimitProvider::Claude), 4);
}

#[gpui::test]
fn polls_pi_usage_while_watched(cx: &mut TestAppContext) {
    use super::pi_usage::PiUsageProvider;
    use super::rate_limits::{RATE_LIMIT_MIN_REFETCH_MS, RATE_LIMIT_POLL_MS};
    let test = init_test_attention(cx, NOW);
    test.fetcher.set_pi(
        PiUsageProvider::Anthropic,
        limits(RateLimitProvider::Claude, Some(window(30.0, None))),
    );
    let (_, _, rate_limits) = attention(cx);
    let watch = rate_limits.update(cx, |rate_limits, cx| {
        rate_limits.watch_pi_usage(PiUsageProvider::Anthropic, cx)
    });
    cx.run_until_parked();
    assert_eq!(test.fetcher.pi_calls.lock().len(), 1);
    rate_limits.read_with(cx, |rate_limits, _| {
        assert_eq!(
            rate_limits
                .pi_usage(PiUsageProvider::Anthropic)
                .session
                .unwrap()
                .used_percent,
            30.0
        );
    });

    // Focus right after a fetch reads the snapshot without another request.
    test.clock.advance(cx, Duration::from_millis(1_000));
    cx.update(|cx| Attention::set_window_focused(cx, true));
    cx.run_until_parked();
    assert_eq!(test.fetcher.pi_calls.lock().len(), 1);
    test.clock
        .advance(cx, Duration::from_millis(RATE_LIMIT_MIN_REFETCH_MS as u64));
    cx.update(|cx| Attention::set_window_focused(cx, true));
    cx.run_until_parked();
    assert_eq!(test.fetcher.pi_calls.lock().len(), 2);

    test.clock
        .advance(cx, Duration::from_millis(RATE_LIMIT_POLL_MS as u64));
    assert_eq!(test.fetcher.pi_calls.lock().len(), 3);
    drop(watch);
    test.clock
        .advance(cx, Duration::from_millis(2 * RATE_LIMIT_POLL_MS as u64));
    assert_eq!(test.fetcher.pi_calls.lock().len(), 3);
}

#[gpui::test]
fn a_codex_reset_reloads_the_account_and_a_failure_keeps_the_snapshot(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    test.fetcher.push(
        RateLimitProvider::Codex,
        limits(RateLimitProvider::Codex, Some(window(100.0, None))),
    );
    test.fetcher.push(
        RateLimitProvider::Codex,
        limits(RateLimitProvider::Codex, Some(window(0.0, None))),
    );
    let (_, _, rate_limits) = attention(cx);
    drop(rate_limits.update(cx, |rate_limits, cx| {
        rate_limits.load(RateLimitProvider::Codex, "default", false, cx)
    }));
    cx.run_until_parked();
    let reset = rate_limits.update(cx, |rate_limits, cx| {
        rate_limits.consume_codex_reset(None, "default", cx)
    });
    assert!(rate_limits.read_with(cx, |rate_limits, _| rate_limits.footer_refreshing()));
    cx.run_until_parked();
    assert_eq!(
        finished(reset),
        Ok(rate_limits_fetch::CodexResetOutcome::Reset)
    );
    rate_limits.read_with(cx, |rate_limits, _| {
        assert!(!rate_limits.footer_refreshing());
        assert_eq!(
            rate_limits
                .get(RateLimitProvider::Codex, "default")
                .session
                .unwrap()
                .used_percent,
            0.0
        );
    });

    *test.fetcher.reset_outcome.lock() = Some(Err("No reset left".into()));
    let reset = rate_limits.update(cx, |rate_limits, cx| {
        rate_limits.consume_codex_reset(None, "default", cx)
    });
    cx.run_until_parked();
    assert_eq!(finished(reset), Err("No reset left".into()));
    rate_limits.read_with(cx, |rate_limits, _| {
        let snapshot = rate_limits.get(RateLimitProvider::Codex, "default");
        assert_eq!(snapshot.status, RateLimitStatus::Error);
        assert_eq!(snapshot.error.as_deref(), Some("No reset left"));
        assert_eq!(snapshot.session.unwrap().used_percent, 0.0);
    });
}

// Notifier: input notifications (useInputNotifications.test.ts).

fn approval(request_id: i64, text: &str) -> Block {
    super::notifications::tests::approval_block(
        &format!("approval-{request_id}"),
        BlockRole::Approval,
        text,
        request_id,
    )
}

fn question(request_id: i64, title: &str) -> monocode_core::user_question::UserQuestionPrompt {
    super::notifications::tests::question(request_id, Some(title), &[])
}

/// Notifications on, permission granted, window in the background, and an
/// idle session already open.
fn notifying(cx: &mut TestAppContext) -> TestAttention {
    let (backend, workspace) = init_test_engine_for_attention(cx);
    let kv = monocode_settings::Kv::in_memory();
    save_notifications_enabled(&kv, true);
    let test = init_test_attention_with(cx, backend, workspace, kv, NOW);
    cx.update(|cx| Attention::set_window_focused(cx, false));
    let mut session = chat("first", HarnessId::Codex, "/repo");
    session.blocks = Vec::new();
    upsert(cx, session);
    test
}

fn bodies(test: &TestAttention) -> Vec<(String, String)> {
    test.platform.banner_bodies()
}

fn pair(session_id: &str, body: &str) -> (String, String) {
    (session_id.to_string(), body.to_string())
}

#[gpui::test]
fn describes_a_new_approval_while_an_older_question_is_still_pending(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let mut session = get(cx, "first");
    session.pending_question = Some(question(1, "Which source?"));
    upsert(cx, session.clone());
    session.blocks = vec![approval(2, "Read the changelog")];
    upsert(cx, session.clone());
    assert_eq!(
        bodies(&test),
        [
            pair("first", "Which source?"),
            pair("first", "Approve: Read the changelog")
        ]
    );
    session.pending_question = None;
    upsert(cx, session);
    assert_eq!(bodies(&test).len(), 2);
}

#[gpui::test]
fn notifies_the_next_concurrent_approval_after_the_first_is_resolved(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let mut session = get(cx, "first");
    session.blocks = vec![approval(1, "First request"), approval(2, "Second request")];
    upsert(cx, session.clone());
    assert_eq!(bodies(&test).len(), 1);
    session.blocks = vec![approval(2, "Second request")];
    upsert(cx, session.clone());
    assert_eq!(
        bodies(&test),
        [
            pair("first", "Approve: First request"),
            pair("first", "Approve: Second request")
        ]
    );
    session.title = "Updated title".into();
    upsert(cx, session);
    assert_eq!(bodies(&test).len(), 2);
}

#[gpui::test]
fn keeps_concurrent_questions_approvals_and_sessions_with_the_same_request_id_distinct(
    cx: &mut TestAppContext,
) {
    let test = notifying(cx);
    let mut session = get(cx, "first");
    session.blocks = vec![approval(1, "Read source")];
    session.pending_question = Some(question(1, "Choose a source"));
    let mut other = session.clone();
    other.id = "other".into();
    other.blocks = Vec::new();
    other.pending_question = Some(question(1, "Choose a branch"));
    let sessions = sessions(cx);
    sessions.update(cx, |sessions, cx| {
        sessions.set_all(vec![session.clone(), other.clone()], cx)
    });
    cx.run_until_parked();
    assert_eq!(bodies(&test).len(), 2);
    session.blocks = Vec::new();
    upsert(cx, session);
    assert_eq!(
        bodies(&test),
        [
            pair("first", "Approve: Read source"),
            pair("other", "Choose a branch"),
            pair("first", "Choose a source"),
        ]
    );
}

#[gpui::test]
fn describes_a_new_question_while_an_approval_remains_pending(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let mut session = get(cx, "first");
    session.blocks = vec![approval(1, "Read source")];
    upsert(cx, session.clone());
    session.pending_question = Some(question(2, "Which branch?"));
    upsert(cx, session);
    assert_eq!(
        bodies(&test),
        [
            pair("first", "Approve: Read source"),
            pair("first", "Which branch?")
        ]
    );
}

fn focus(cx: &mut TestAppContext, session_id: Option<&str>) {
    cx.update(|cx| {
        Attention::set_focus(
            cx,
            AttentionFocus {
                active_session_id: session_id.map(str::to_string),
                ..Default::default()
            },
        )
    });
    cx.run_until_parked();
}

#[gpui::test]
fn forgets_completed_requests_and_does_not_replay_pending_ones_on_focus_changes(
    cx: &mut TestAppContext,
) {
    let test = notifying(cx);
    let mut session = get(cx, "first");
    session.blocks = vec![approval(1, "Read source")];
    focus(cx, Some("first"));
    upsert(cx, session.clone());
    focus(cx, Some("other"));
    assert_eq!(bodies(&test).len(), 1);
    session.blocks = Vec::new();
    upsert(cx, session.clone());
    session.blocks = vec![approval(1, "Read another source")];
    upsert(cx, session);
    assert_eq!(
        bodies(&test),
        [
            pair("first", "Approve: Read source"),
            pair("first", "Approve: Read another source")
        ]
    );
}

#[gpui::test]
fn retains_the_notification_setting_and_focused_session_policy(cx: &mut TestAppContext) {
    let test = notifying(cx);
    cx.update(|cx| Attention::set_window_focused(cx, true));
    focus(cx, Some("first"));
    let mut session = get(cx, "first");
    session.blocks = vec![approval(1, "Visible request")];
    upsert(cx, session.clone());
    assert!(bodies(&test).is_empty());
    focus(cx, Some("other"));
    session.pending_question = Some(question(2, "Hidden session question"));
    upsert(cx, session.clone());
    assert_eq!(bodies(&test), [pair("first", "Hidden session question")]);
    save_notifications_enabled(&test.kv, false);
    session.blocks = vec![approval(3, "Disabled request")];
    upsert(cx, session);
    assert_eq!(bodies(&test).len(), 1);
}

// Notifier: delivery (notificationDelivery.test.ts).

#[gpui::test]
fn returns_false_without_a_banner_or_sound_for_a_non_project_path(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let (notifier, _, _) = attention(cx);
    let root = chat("root", HarnessId::Claude, "/");
    let sent = notifier.update(cx, |notifier, cx| {
        notifier.notify_session(&root, NotificationEvent::Finished, false, cx)
    });
    cx.run_until_parked();
    assert!(!finished(sent));
    let announce = notifier.update(cx, |notifier, cx| {
        notifier.announce_session_finished(&root, false, cx)
    });
    cx.run_until_parked();
    finished(announce);
    assert!(bodies(&test).is_empty());
    assert!(test.platform.sounds.lock().is_empty());
}

#[gpui::test]
fn blocks_every_project_banner_including_approvals_and_questions_while_muted(
    cx: &mut TestAppContext,
) {
    let test = notifying(cx);
    update_notification_preferences(
        &test.kv,
        &["local:/private"],
        &PreferencePatch::mute(Some(Mute::UntilResumed)),
        NOW,
    );
    let (notifier, _, _) = attention(cx);
    let private = chat("private", HarnessId::Claude, "/private");
    for event in [
        NotificationEvent::Finished,
        NotificationEvent::Input {
            kind: super::notifications::InputKind::Approval,
            request_id: 1,
        },
        NotificationEvent::Input {
            kind: super::notifications::InputKind::Question,
            request_id: 2,
        },
    ] {
        let sent = notifier.update(cx, |notifier, cx| {
            notifier.notify_session(&private, event, false, cx)
        });
        cx.run_until_parked();
        assert!(!finished(sent));
    }
    assert!(bodies(&test).is_empty());
}

#[gpui::test]
fn does_not_deliver_an_input_event_observed_during_a_mute_after_expiry(cx: &mut TestAppContext) {
    let test = notifying(cx);
    test.clock.set(1000);
    update_notification_preferences(
        &test.kv,
        &["local:/private"],
        &PreferencePatch::mute(Some(Mute::Until(2000))),
        1000,
    );
    let (notifier, _, _) = attention(cx);
    let private = chat("private", HarnessId::Claude, "/private");
    let sent = notifier.update(cx, |notifier, cx| {
        notifier.notify_session(
            &private,
            NotificationEvent::Input {
                kind: super::notifications::InputKind::Question,
                request_id: 1,
            },
            false,
            cx,
        )
    });
    test.clock.set(3000);
    cx.run_until_parked();
    assert!(!finished(sent));
}

#[gpui::test]
fn a_finished_turn_plays_the_cue_only_when_the_banner_does_not_go_out(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let mut done = chat("done", HarnessId::Claude, "/repo/app");
    done.title = "claude · Fix the sidebar".into();
    done.blocks = vec![Block::new("a1", BlockRole::Assistant, "All done.")];
    upsert(cx, done);
    let (notifier, _, _) = attention(cx);
    notifier.update(cx, |notifier, cx| {
        notifier.announce_finished_later("done", cx)
    });
    cx.run_until_parked();
    assert_eq!(bodies(&test), [pair("done", "All done.")]);
    assert!(test.platform.sounds.lock().is_empty());

    *test.platform.fail_banners.lock() = true;
    notifier.update(cx, |notifier, cx| {
        notifier.announce_finished_later("done", cx)
    });
    cx.run_until_parked();
    assert_eq!(*test.platform.sounds.lock(), [SoundName::Success]);
}

#[gpui::test]
fn syncs_the_dock_badge_when_the_waiting_count_changes(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let sessions = sessions(cx);
    sessions.update(cx, |sessions, cx| {
        sessions.enqueue_event(
            "first",
            monocode_core::harness_event::HarnessEvent::ApprovalRequested {
                request_id: 7,
                title: "Run tests".into(),
                kind: None,
                call_id: None,
                preview: None,
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(test.platform.badges.lock().last(), Some(&1));
    let count = test.platform.badges.lock().len();
    let (notifier, _, _) = attention(cx);
    notifier.update(cx, |notifier, cx| notifier.sync_dock_badge_now(cx));
    assert_eq!(test.platform.badges.lock().len(), count);
}

#[gpui::test]
fn keeps_sessions_that_finish_out_of_view_until_they_are_seen(cx: &mut TestAppContext) {
    let _test = notifying(cx);
    focus(cx, Some("other"));
    let mut session = get(cx, "first");
    session.busy = Some(true);
    upsert(cx, session.clone());
    session.busy = None;
    upsert(cx, session);
    let (notifier, _, _) = attention(cx);
    let unseen = cx.update(|cx| Engine::hooks(cx).attention.unseen_finished_ids(cx));
    assert!(unseen.contains("first"));
    assert_eq!(
        notifier
            .read_with(cx, |notifier, cx| notifier.live_agents(cx))
            .len(),
        1
    );
    focus(cx, Some("first"));
    assert!(notifier.read_with(cx, |notifier, _| notifier.unseen_finished_ids().is_empty()));
}

#[gpui::test]
fn never_marks_a_finished_worker_unseen(cx: &mut TestAppContext) {
    let _test = notifying(cx);
    focus(cx, Some("other"));
    let mut worker = get(cx, "first");
    worker.id = "worker".into();
    worker.orchestration_lead_id = Some("first".into());
    worker.busy = Some(true);
    upsert(cx, worker.clone());
    let mut lead = get(cx, "first");
    lead.busy = Some(true);
    upsert(cx, lead.clone());
    worker.busy = None;
    upsert(cx, worker);
    lead.busy = None;
    upsert(cx, lead);
    // The lead can be looked at; the worker cannot, so it is never kept
    // loaded waiting to be seen.
    let (notifier, _, _) = attention(cx);
    let unseen = notifier.read_with(cx, |notifier, _| notifier.unseen_finished_ids().clone());
    assert!(unseen.contains("first"));
    assert!(!unseen.contains("worker"));
}

#[gpui::test]
fn tells_listeners_when_a_timed_mute_expires(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let (notifier, _, _) = attention(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let _subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&notifier, move |_, event: &NotifierEvent, _| {
            events.borrow_mut().push(event.clone())
        })
    });
    update_notification_preferences(
        &test.kv,
        &["local:/repo"],
        &PreferencePatch::mute(Some(Mute::Until(NOW + 60_000))),
        NOW,
    );
    cx.run_until_parked();
    assert_eq!(*events.borrow(), [NotifierEvent::PreferencesChanged]);
    test.clock.advance(cx, Duration::from_millis(60_000));
    assert_eq!(events.borrow().len(), 2);
}

#[gpui::test]
fn a_banner_click_opens_the_session(cx: &mut TestAppContext) {
    let test = notifying(cx);
    let (notifier, _, _) = attention(cx);
    notifier.update(cx, |notifier, cx| {
        notifier.notification_clicked("first", cx);
        notifier.notification_clicked("unknown", cx);
    });
    cx.run_until_parked();
    assert_eq!(
        *test.router.routed.borrow(),
        [
            Routed::Focus("first".into()),
            Routed::OpenHistory("first".into())
        ]
    );
}

// The policy half of projectNotificationFlow.test.ts: Inbox cues across
// category choices, a project mute, and a resume.

#[gpui::test]
fn honors_category_choices_before_and_after_a_project_mute(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let (notifier, _, _) = attention(cx);
    let one = "local:/one";
    let mut cue = |category: NotificationCategory, project: &str, occurred_at: i64| {
        let subject = NotificationSubject {
            project_id: project.into(),
            category,
            occurred_at: Some(occurred_at),
        };
        notifier.update(cx, |notifier, _| {
            notifier.play_cue(SoundCue::InboxUnseen, Some(&subject))
        })
    };
    update_notification_preferences(
        &test.kv,
        &[one],
        &PreferencePatch::disabled(vec![NotificationCategory::Issues]),
        NOW,
    );
    assert!(!cue(NotificationCategory::Issues, one, NOW + 1_000));
    assert!(cue(NotificationCategory::PullRequests, one, NOW + 1_000));
    update_notification_preferences(
        &test.kv,
        &[one],
        &PreferencePatch::mute(Some(Mute::UntilResumed)),
        NOW + 2_000,
    );
    assert!(!cue(NotificationCategory::PullRequests, one, NOW + 3_000));
    assert!(cue(
        NotificationCategory::PullRequests,
        "local:/two",
        NOW + 3_000
    ));
    test.clock.set(NOW + 4_000);
    update_notification_preferences(&test.kv, &[one], &PreferencePatch::mute(None), NOW + 4_000);
    // Activity from the muted period stays quiet; the issue category stays off.
    test.clock.set(NOW + 5_000);
    assert!(!cue(NotificationCategory::PullRequests, one, NOW + 3_500));
    assert!(!cue(NotificationCategory::Issues, one, NOW + 4_500));
    assert!(cue(NotificationCategory::PullRequests, one, NOW + 4_500));
    assert_eq!(test.platform.sounds.lock().len(), 3);
    assert!(
        test.platform
            .sounds
            .lock()
            .iter()
            .all(|sound| *sound == SoundName::Bloom)
    );
}

// Approvals.

#[gpui::test]
fn tracks_sessions_waiting_on_the_user_with_their_leads(cx: &mut TestAppContext) {
    let _test = init_test_attention(cx, NOW);
    let (_, approvals, _) = attention(cx);
    let mut worker = chat("worker", HarnessId::Claude, "/repo");
    worker.orchestration_lead_id = Some("lead".into());
    worker.blocks = vec![approval(1, "Run tests")];
    upsert(cx, worker);
    approvals.read_with(cx, |approvals, cx| {
        let mut ids: Vec<&String> = approvals.approval_session_ids().iter().collect();
        ids.sort();
        assert_eq!(ids, ["lead", "worker"]);
        assert_eq!(approvals.pending_for("worker", cx).unwrap().request_id, 1);
    });
}

#[gpui::test]
fn routes_answers_to_the_harness_or_the_remote_host(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    upsert(cx, chat("local", HarnessId::Claude, "/repo"));
    upsert(cx, chat("remote", HarnessId::Codex, "remote://host/repo"));
    let mut removed = chat("removed", HarnessId::Claude, "/repo");
    removed.worktree_removed = Some(true);
    upsert(cx, removed);
    test.router
        .remote_sessions
        .borrow_mut()
        .push("remote".into());
    let reply = UserQuestionReply::Skipped;
    cx.update(|cx| {
        Approvals::approve("local", 1, ApprovalDecision::Allow, cx);
        Approvals::approve("remote", 2, ApprovalDecision::Deny, cx);
        Approvals::approve("removed", 3, ApprovalDecision::Allow, cx);
        Approvals::answer_question("local", 4, &reply, cx);
        Approvals::answer_question("remote", 5, &reply, cx);
        Approvals::question_interaction("local", 6, cx);
        Approvals::question_interaction("remote", 7, cx);
    });
    assert_eq!(
        *test.router.routed.borrow(),
        [
            Routed::Approval(
                HarnessId::Claude,
                "local".into(),
                1,
                ApprovalDecision::Allow
            ),
            Routed::RemoteApproval("remote".into(), 2, ApprovalDecision::Deny),
            Routed::Question(HarnessId::Claude, "local".into(), 4, reply.clone()),
            Routed::RemoteAnswer("remote".into(), 5, reply.clone()),
            Routed::KeepOpen(HarnessId::Claude, "local".into(), 6),
        ]
    );
}

#[gpui::test]
fn opens_a_workers_approval_in_its_leads_panel(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    let mut worker = chat("worker", HarnessId::Claude, "/repo");
    worker.orchestration_lead_id = Some("lead".into());
    upsert(cx, worker);
    test.router.open_sessions.borrow_mut().push("lead".into());
    test.router
        .leads
        .borrow_mut()
        .insert("parked".into(), "lead-2".into());
    cx.update(|cx| {
        Approvals::open_approval_session("worker", cx);
        Approvals::open_approval_session("parked", cx);
        Approvals::open_approval_session("solo", cx);
    });
    assert_eq!(
        *test.router.routed.borrow(),
        [
            Routed::InspectWorker("worker".into()),
            Routed::Focus("lead".into()),
            Routed::InspectWorker("parked".into()),
            Routed::Focus("lead-2".into()),
            Routed::OpenHistory("lead-2".into()),
            Routed::Focus("solo".into()),
            Routed::OpenHistory("solo".into()),
        ]
    );
}

fn signed_out(id: &str) -> Session {
    let mut session = chat(id, HarnessId::Claude, "/repo");
    let mut error = Block::new(
        format!("{id}-error"),
        BlockRole::System,
        "Authentication required",
    );
    error.notice = Some(BlockNotice::Error);
    session.blocks = vec![Block::new(format!("{id}-u1"), BlockRole::User, "hi"), error];
    session
}

#[gpui::test]
fn asks_for_a_sign_in_once_per_failed_turn_of_the_active_session(cx: &mut TestAppContext) {
    let (backend, workspace) = init_test_engine_for_attention(cx);
    // A failure restored at launch does not prompt.
    let sessions = sessions(cx);
    sessions.update(cx, |sessions, cx| {
        sessions.upsert(signed_out("restored"), cx)
    });
    let _test = init_test_attention_with(
        cx,
        backend,
        workspace,
        monocode_settings::Kv::in_memory(),
        NOW,
    );
    let (_, approvals, _) = attention(cx);
    focus(cx, Some("restored"));
    assert!(approvals.read_with(cx, |approvals, _| approvals.sign_in_request().is_none()));

    upsert(cx, signed_out("fresh"));
    focus(cx, Some("fresh"));
    let request = approvals
        .read_with(cx, |approvals, _| approvals.sign_in_request().cloned())
        .unwrap();
    assert_eq!(request.key, "fresh:fresh-u1");
    assert_eq!(request.harness, HarnessId::Claude);

    focus(cx, Some("restored"));
    assert!(approvals.read_with(cx, |approvals, _| approvals.sign_in_request().is_none()));
    focus(cx, Some("fresh"));
    assert!(approvals.read_with(cx, |approvals, _| approvals.sign_in_request().is_none()));
}

// Runtime hooks.

#[gpui::test]
fn fills_in_the_runtime_attention_hooks(cx: &mut TestAppContext) {
    let test = init_test_attention(cx, NOW);
    assert!(cx.update(|cx| Engine::hooks(cx).attention.live_agents_enabled(cx)));
    monocode_settings::settings_store::save_live_agents_enabled(&test.kv, false);
    assert!(!cx.update(|cx| Engine::hooks(cx).attention.live_agents_enabled(cx)));
}
