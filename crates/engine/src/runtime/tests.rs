//! Entity tests for `Sessions`, `Lifecycle`, and the session writer, ported
//! from the App.tsx behavior and from appLifecycle.test.ts,
//! bootWorkspace.test.ts, sessionStoreConcurrency.test.ts, and
//! sessionStoreLinkedWorkItem.test.ts. `FakeBackend` stands in for the
//! mocked `invoke`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Entity, Task, TestAppContext};
use monocode_core::block::{Block, BlockRole, BlockTool};
use monocode_core::inbox::{InboxAskContext, WorkItemKind};
use monocode_core::session::LinkedWorkItem;
use monocode_core::{Extra, HarnessEvent, HarnessId, Session};
use serde_json::{Value, json};

use super::engine::Engine;
use super::harness_flush::{BACKGROUND_FLUSH, FRAME_FLUSH, FlushKind};
use super::hooks::{EngineHooks, HarnessHooks, OrchestrationHooks};
use super::in_flight::{INTERRUPT_MESSAGE, mark_turn_interrupted};
use super::lifecycle::{Lifecycle, QuitMode, persist_quit_state};
use super::sessions::{
    PERSIST_DEBOUNCE, SESSION_DETACH_DELAY, Sessions, SessionsEvent, WORKSPACE_SNAPSHOT_DEBOUNCE,
};
use super::testing::{FakeBackend, TestWorkspace, init_test_engine_with};

const CWD: &str = "/repo";

fn chat(id: &str) -> Session {
    let mut session = Session::blank(id, HarnessId::Cursor, "cursor:auto", CWD);
    session.blocks = vec![Block::new(format!("{id}-u1"), BlockRole::User, "hello")];
    session
}

fn busy(id: &str) -> Session {
    Session {
        busy: Some(true),
        ..chat(id)
    }
}

fn delta(text: &str) -> HarnessEvent {
    HarnessEvent::MessageDelta { text: text.into() }
}

/// Records harness registry calls.
#[derive(Default)]
struct RecordingHarness {
    live: bool,
    calls: RefCell<Vec<String>>,
}

impl HarnessHooks for RecordingHarness {
    fn is_live_harness(&self, _harness: HarnessId) -> bool {
        self.live
    }

    fn bind_session(&self, session: &Session, _cx: &mut App) {
        self.calls.borrow_mut().push(format!("bind {}", session.id));
    }

    fn forget_session(&self, harness: HarnessId, session_id: &str, _cx: &mut App) -> Task<()> {
        self.calls
            .borrow_mut()
            .push(format!("forget {} {session_id}", harness.as_str()));
        Task::ready(())
    }

    fn cancel_turn(&self, _harness: HarnessId, session_id: &str, _cx: &mut App) -> Task<()> {
        self.calls.borrow_mut().push(format!("cancel {session_id}"));
        Task::ready(())
    }

    fn kill_all_children(&self, _cx: &mut App) -> Task<()> {
        self.calls.borrow_mut().push("kill all".into());
        Task::ready(())
    }
}

impl RecordingHarness {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

#[derive(Default)]
struct RunningLeads(RefCell<HashSet<String>>);

impl OrchestrationHooks for RunningLeads {
    fn running_lead_ids(&self, _cx: &App) -> HashSet<String> {
        self.0.borrow().clone()
    }
}

struct Harness {
    backend: Arc<FakeBackend>,
    workspace: Rc<TestWorkspace>,
    harness: Rc<RecordingHarness>,
    leads: Rc<RunningLeads>,
    sessions: Entity<Sessions>,
    lifecycle: Entity<Lifecycle>,
    events: Rc<RefCell<Vec<SessionsEvent>>>,
}

fn setup(cx: &mut TestAppContext) -> Harness {
    setup_with(cx, false)
}

fn setup_with(cx: &mut TestAppContext, live_harness: bool) -> Harness {
    let workspace = TestWorkspace::new();
    let harness = Rc::new(RecordingHarness {
        live: live_harness,
        ..RecordingHarness::default()
    });
    let leads = Rc::new(RunningLeads::default());
    let hooks = EngineHooks {
        workspace: workspace.clone(),
        harness: harness.clone(),
        orchestration: leads.clone(),
        ..EngineHooks::default()
    };
    let backend = init_test_engine_with(cx, hooks);
    let (sessions, lifecycle) = cx.update(|cx| (Engine::sessions(cx), Engine::lifecycle(cx)));
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    cx.update(|cx| {
        cx.subscribe(&sessions, move |_, event: &SessionsEvent, _| {
            recorded.borrow_mut().push(event.clone())
        })
        .detach()
    });
    Harness {
        backend,
        workspace,
        harness,
        leads,
        sessions,
        lifecycle,
        events,
    }
}

impl Harness {
    fn open(&self, cx: &mut TestAppContext, sessions: Vec<Session>) {
        self.sessions
            .update(cx, |state, cx| state.set_all(sessions, cx));
    }

    fn tabs(&self, ids: &[&str]) {
        *self.workspace.tab_session_ids.borrow_mut() =
            ids.iter().map(|id| id.to_string()).collect();
    }

    fn get(&self, cx: &mut TestAppContext, id: &str) -> Option<Session> {
        self.sessions
            .read_with(cx, |state, _| state.get(id).cloned())
    }

    fn count(&self, command: &str) -> usize {
        self.backend
            .commands()
            .iter()
            .filter(|c| *c == command)
            .count()
    }

    fn applied(&self) -> usize {
        self.events
            .borrow()
            .iter()
            .filter(|event| matches!(event, SessionsEvent::EventsApplied { .. }))
            .count()
    }
}

// Harness event batching (`enqueueHarnessEvent`, `flushHarnessEvents`).

#[gpui::test]
fn batches_visible_output_until_the_next_frame(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, vec![busy("s1")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("s1", delta("Hel"), cx);
        state.enqueue_event("s1", delta("lo"), cx);
        assert_eq!(state.queued_events("s1").len(), 2);
        assert_eq!(state.scheduled_flush(), Some(FlushKind::Frame));
    });
    assert_eq!(t.applied(), 0);
    cx.executor().advance_clock(FRAME_FLUSH);
    let session = t.get(cx, "s1").unwrap();
    assert_eq!(session.blocks.last().unwrap().text, "Hello");
    assert_eq!(t.applied(), 1);
    t.sessions.read_with(cx, |state, _| {
        assert!(state.queued_events("s1").is_empty());
        assert_eq!(state.scheduled_flush(), None);
    });
}

#[gpui::test]
fn approvals_and_questions_apply_at_once_with_the_queued_output(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, vec![busy("s1")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("s1", delta("Working"), cx);
        state.enqueue_event(
            "s1",
            HarnessEvent::ApprovalRequested {
                request_id: 7,
                title: "Run ls?".into(),
                kind: None,
                call_id: None,
                preview: None,
            },
            cx,
        );
        assert!(state.queued_events("s1").is_empty());
    });
    let session = t.get(cx, "s1").unwrap();
    assert!(session.blocks.iter().any(|b| b.text == "Working"));
    assert!(
        session
            .blocks
            .iter()
            .any(|b| b.approval.as_ref().is_some_and(|a| a.request_id == 7))
    );
    assert_eq!(t.applied(), 1);
}

#[gpui::test]
fn background_output_waits_for_the_slow_cadence(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.foreground.borrow_mut() = Some(HashSet::new());
    t.open(cx, vec![busy("s1")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("s1", delta("x"), cx);
        assert_eq!(state.scheduled_flush(), Some(FlushKind::Timeout));
    });
    cx.executor().advance_clock(FRAME_FLUSH);
    assert_eq!(t.applied(), 0);
    cx.executor().advance_clock(BACKGROUND_FLUSH - FRAME_FLUSH);
    assert_eq!(t.applied(), 1);
}

#[gpui::test]
fn visible_output_promotes_a_background_timer(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.foreground.borrow_mut() = Some(["s2".to_string()].into());
    t.open(cx, vec![busy("s1"), busy("s2")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("s1", delta("bg"), cx);
        assert_eq!(state.scheduled_flush(), Some(FlushKind::Timeout));
        state.enqueue_event("s2", delta("fg"), cx);
        assert_eq!(state.scheduled_flush(), Some(FlushKind::Frame));
    });
    cx.executor().advance_clock(FRAME_FLUSH);
    assert_eq!(t.applied(), 1);
    assert_eq!(t.get(cx, "s1").unwrap().blocks.last().unwrap().text, "bg");
    assert_eq!(t.get(cx, "s2").unwrap().blocks.last().unwrap().text, "fg");
    cx.executor().advance_clock(BACKGROUND_FLUSH);
    assert_eq!(t.applied(), 1);
}

#[gpui::test]
fn a_hidden_window_never_waits_for_a_frame(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.hidden.borrow_mut() = true;
    t.open(cx, vec![busy("s1")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("s1", delta("x"), cx);
        assert_eq!(state.scheduled_flush(), Some(FlushKind::Timeout));
    });
}

#[gpui::test]
fn flush_applies_now_and_cancels_the_timer(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, vec![busy("s1")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("s1", delta("now"), cx);
        state.flush(cx);
        assert_eq!(state.scheduled_flush(), None);
    });
    assert_eq!(t.applied(), 1);
    cx.executor().advance_clock(BACKGROUND_FLUSH);
    assert_eq!(t.applied(), 1);
}

#[gpui::test]
fn events_for_sessions_that_are_not_open_are_dropped(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, vec![busy("s1")]);
    t.sessions.update(cx, |state, cx| {
        state.enqueue_event("gone", delta("x"), cx);
        state.flush(cx);
    });
    assert_eq!(t.applied(), 0);
}

#[gpui::test]
fn one_reducer_call_per_session_per_flush(cx: &mut TestAppContext) {
    thread_local! {
        static CALLS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }
    fn counting(session: &mut Session, events: &[HarnessEvent]) -> bool {
        CALLS.with(|calls| calls.borrow_mut().push(events.len()));
        session.title = format!("{} events", events.len());
        true
    }
    CALLS.with(|calls| calls.borrow_mut().clear());
    let t = setup(cx);
    t.open(cx, vec![busy("s1"), busy("s2")]);
    t.sessions.update(cx, |state, cx| {
        state.set_reducer(counting);
        for _ in 0..3 {
            state.enqueue_event("s1", delta("a"), cx);
        }
        state.enqueue_event("s2", HarnessEvent::Status { text: "ok".into() }, cx);
    });
    cx.executor().advance_clock(FRAME_FLUSH);
    let mut calls = CALLS.with(|calls| calls.borrow().clone());
    calls.sort_unstable();
    assert_eq!(calls, vec![1, 3]);
    assert_eq!(t.get(cx, "s1").unwrap().title, "3 events");
}

#[gpui::test]
fn bumps_turn_generations(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.sessions.update(cx, |state, _| {
        assert_eq!(state.turn_gen("s1"), 0);
        assert_eq!(state.bump_turn_gen("s1"), 1);
        assert_eq!(state.bump_turn_gen("s1"), 2);
        assert_eq!(state.turn_gen("s1"), 2);
    });
}

#[gpui::test]
async fn stop_for_removal_cancels_a_busy_turn_and_flushes(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, vec![busy("s1"), chat("idle")]);
    t.sessions
        .update(cx, |state, cx| state.enqueue_event("s1", delta("tail"), cx));
    let stop = t
        .sessions
        .update(cx, |state, cx| state.stop_for_removal("s1", cx));
    let stopped = stop.await.unwrap();
    assert!(stopped.blocks.iter().any(|b| b.text == "tail"));
    assert_eq!(t.harness.calls(), vec!["cancel s1"]);
    t.sessions
        .read_with(cx, |state, _| assert_eq!(state.turn_gen("s1"), 1));
    let idle = t
        .sessions
        .update(cx, |state, cx| state.stop_for_removal("idle", cx));
    assert_eq!(idle.await.unwrap().id, "idle");
    assert_eq!(t.harness.calls(), vec!["cancel s1"]);
}

#[gpui::test]
fn tracks_busy_sessions_and_their_leads(cx: &mut TestAppContext) {
    let t = setup(cx);
    let worker = Session {
        orchestration_lead_id: Some("lead".into()),
        ..busy("worker")
    };
    t.open(cx, vec![chat("lead"), worker]);
    t.sessions.read_with(cx, |state, _| {
        let busy: HashSet<String> = ["worker".into(), "lead".into()].into();
        assert_eq!(state.busy_session_ids(), &busy);
    });
    assert!(t.events.borrow().contains(&SessionsEvent::BusyChanged));
}

// Saving (`persistSession` and the debounced save effect).

#[gpui::test]
fn a_new_user_turn_saves_at_once(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["s1"]);
    t.open(cx, vec![busy("s1")]);
    cx.run_until_parked();
    assert_eq!(t.count("session_upsert"), 1);
    assert!(
        t.events
            .borrow()
            .iter()
            .any(|e| matches!(e, SessionsEvent::Persisted(s) if s.id == "s1"))
    );
    // The debounce sees the same fingerprint and skips a second write.
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), 1);
}

#[gpui::test]
fn a_busy_visible_session_saves_once_it_goes_idle(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["s1"]);
    t.open(cx, vec![busy("s1")]);
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), 1);
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| {
            s.blocks
                .push(Block::new("a1", BlockRole::Assistant, "partial"))
        });
    });
    cx.executor().advance_clock(PERSIST_DEBOUNCE * 2);
    assert_eq!(t.count("session_upsert"), 1);
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| s.busy = Some(false))
    });
    cx.executor()
        .advance_clock(PERSIST_DEBOUNCE - Duration::from_millis(1));
    assert_eq!(t.count("session_upsert"), 1);
    cx.executor().advance_clock(Duration::from_millis(1));
    assert_eq!(t.count("session_upsert"), 2);
}

#[gpui::test]
fn a_parked_busy_session_saves_after_the_debounce(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.open(cx, vec![busy("s1")]);
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    let first = t.count("session_upsert");
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| {
            s.blocks
                .push(Block::new("a1", BlockRole::Assistant, "more"))
        });
    });
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), first + 1);
}

#[gpui::test]
fn every_change_restarts_the_debounce(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["s1", "s2"]);
    t.open(cx, vec![chat("s1"), busy("s2")]);
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    let saved = t.count("session_upsert");
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| s.title = "renamed".into())
    });
    for _ in 0..3 {
        cx.executor().advance_clock(PERSIST_DEBOUNCE / 2);
        t.sessions.update(cx, |state, cx| {
            state.update("s2", cx, |s| {
                s.blocks.push(Block::new(
                    uuid::Uuid::new_v4().to_string(),
                    BlockRole::Assistant,
                    "x",
                ))
            });
        });
    }
    assert_eq!(t.count("session_upsert"), saved);
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), saved + 1);
}

#[gpui::test]
fn blank_remote_inbox_and_removing_sessions_never_save(cx: &mut TestAppContext) {
    let t = setup(cx);
    let blank = Session::blank("blank", HarnessId::Cursor, "m", CWD);
    let remote = Session {
        cwd: "remote://env/home/me".into(),
        ..chat("remote")
    };
    let inbox = Session {
        inbox_ask: Some(inbox_context()),
        ..chat("inbox")
    };
    t.sessions
        .update(cx, |state, _| state.begin_removal("removing"));
    t.open(cx, vec![blank, remote, inbox, chat("removing")]);
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), 0);
}

fn inbox_context() -> InboxAskContext {
    InboxAskContext {
        key: "github:issue:1".into(),
        title: "Issue".into(),
        url: "https://github.com/a/b/issues/1".into(),
        provider: monocode_core::inbox::InboxProvider::Github,
        description: None,
        extra: Extra::new(),
    }
}

#[gpui::test]
fn persist_skips_an_unchanged_session(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["s1"]);
    t.open(cx, vec![chat("s1")]);
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    let saved = t.count("session_upsert");
    t.sessions.update(cx, |state, cx| state.persist("s1", cx));
    cx.run_until_parked();
    assert_eq!(t.count("session_upsert"), saved);
    t.sessions.update(cx, |state, cx| {
        state.forget_persisted("s1");
        state.persist("s1", cx);
    });
    cx.run_until_parked();
    assert_eq!(t.count("session_upsert"), saved + 1);
}

#[gpui::test]
fn restored_sessions_count_as_saved(cx: &mut TestAppContext) {
    let t = setup(cx);
    let restored = vec![chat("s1")];
    t.tabs(&["s1"]);
    t.sessions.update(cx, |state, cx| {
        state.adopt_restored(&restored);
        state.set_all(restored.clone(), cx);
    });
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), 0);
}

// The in-flight and workspace snapshot effects.

#[gpui::test]
fn the_quit_list_follows_running_turns(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["s1"]);
    t.open(cx, vec![chat("s1")]);
    cx.run_until_parked();
    // The first idle paint must not wipe a stored snapshot.
    assert_eq!(t.count("session_set_in_flight"), 0);
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| s.busy = Some(true))
    });
    cx.run_until_parked();
    assert_eq!(
        t.backend.in_flight(),
        vec![("s1".to_string(), CWD.to_string())]
    );
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| s.title = "x".into())
    });
    cx.run_until_parked();
    assert_eq!(t.count("session_set_in_flight"), 1);
    t.sessions.update(cx, |state, cx| {
        state.update("s1", cx, |s| s.busy = Some(false))
    });
    cx.run_until_parked();
    assert_eq!(t.count("session_set_in_flight"), 2);
    assert!(t.backend.in_flight().is_empty());
}

#[gpui::test]
fn saves_the_workspace_snapshot_after_its_debounce(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.snapshot.borrow_mut() = Some(json!({ "tabs": [] }));
    t.tabs(&["s1", "s2"]);
    t.open(cx, vec![chat("s1")]);
    cx.executor()
        .advance_clock(WORKSPACE_SNAPSHOT_DEBOUNCE - Duration::from_millis(1));
    assert_eq!(t.count("workspace_set_snapshot"), 0);
    cx.executor().advance_clock(Duration::from_millis(1));
    assert_eq!(t.count("workspace_set_snapshot"), 1);
    assert_eq!(
        t.backend.workspace_snapshot().unwrap()["sessionIds"],
        json!(["s1"])
    );
    t.sessions
        .update(cx, |state, cx| state.schedule_workspace_snapshot(cx));
    cx.executor().advance_clock(WORKSPACE_SNAPSHOT_DEBOUNCE);
    assert_eq!(t.count("workspace_set_snapshot"), 1);
    t.sessions
        .update(cx, |state, _| state.set_workspace_autosave(false));
    t.open(cx, vec![chat("s1"), chat("s2")]);
    cx.executor().advance_clock(WORKSPACE_SNAPSHOT_DEBOUNCE);
    assert_eq!(t.count("workspace_set_snapshot"), 1);
}

// Idle detach (`detachIdleSessions`).

#[gpui::test]
fn detaches_hidden_idle_sessions_into_the_load_cache(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["visible"]);
    t.open(cx, vec![chat("visible"), chat("hidden"), busy("running")]);
    cx.executor().advance_clock(SESSION_DETACH_DELAY);
    let ids = t.sessions.read_with(cx, |state, _| state.ids());
    assert_eq!(ids, vec!["visible", "running"]);
    t.sessions.read_with(cx, |state, _| {
        assert!(state.loaded_cache().contains("hidden"))
    });
    assert_eq!(t.harness.calls(), vec!["forget cursor hidden"]);
    assert!(t.events.borrow().contains(&SessionsEvent::Closed {
        session_ids: vec!["hidden".into()]
    }));
}

#[gpui::test]
fn keeps_inbox_asks_workers_and_opening_sessions_attached(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.tabs(&["lead"]);
    let worker = Session {
        orchestration_lead_id: Some("lead".into()),
        ..chat("worker")
    };
    let parked_worker = Session {
        orchestration_lead_id: Some("running-lead".into()),
        ..chat("parked-worker")
    };
    let orphan = Session {
        orchestration_lead_id: Some("stopped-lead".into()),
        ..chat("orphan")
    };
    t.leads.0.borrow_mut().insert("running-lead".into());
    t.open(cx, vec![chat("lead"), worker, parked_worker, orphan]);
    t.sessions
        .update(cx, |state, _| state.set_skip_forget("orphan", true));
    cx.executor().advance_clock(SESSION_DETACH_DELAY);
    let ids = t.sessions.read_with(cx, |state, _| state.ids());
    assert_eq!(ids, vec!["lead", "worker", "parked-worker", "orphan"]);
    assert!(t.harness.calls().is_empty());
}

// Loading (`loadStoredSession`, `ensureOpenSession`, prefetch).

#[gpui::test]
async fn ensure_open_reads_a_stored_session_once(cx: &mut TestAppContext) {
    let t = setup_with(cx, true);
    let stored = Session {
        provider_session_id: Some("thread-1".into()),
        ..chat("s1")
    };
    t.backend.insert_session(&stored);
    let (first, second) = t.sessions.update(cx, |state, cx| {
        (state.ensure_open("s1", cx), state.ensure_open("s1", cx))
    });
    let (first, second) = futures::future::join(first, second).await;
    assert_eq!(first.unwrap().id, "s1");
    assert_eq!(second.unwrap().id, "s1");
    assert_eq!(t.count("session_get"), 1);
    assert_eq!(t.harness.calls(), vec!["bind s1"]);
    let open = t.get(cx, "s1").unwrap();
    assert_eq!(open.busy, Some(false));
    assert_eq!(open.provider_session_id.as_deref(), Some("thread-1"));
    // A restored session counts as saved.
    cx.executor().advance_clock(PERSIST_DEBOUNCE);
    assert_eq!(t.count("session_upsert"), 0);
    let again = t
        .sessions
        .update(cx, |state, cx| state.ensure_open("s1", cx));
    assert!(again.await.is_some());
    assert_eq!(t.count("session_get"), 1);
}

#[gpui::test]
async fn a_missing_session_reports_load_failed(cx: &mut TestAppContext) {
    let t = setup(cx);
    let open = t
        .sessions
        .update(cx, |state, cx| state.ensure_open("nope", cx));
    assert!(open.await.is_none());
    assert!(t.events.borrow().contains(&SessionsEvent::LoadFailed {
        session_id: "nope".into()
    }));
}

#[gpui::test]
async fn an_invalidated_load_resolves_to_nothing(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("s1"));
    let gate = t.backend.hold_next("session_get");
    let load = t
        .sessions
        .update(cx, |state, cx| state.load_stored("s1", cx));
    cx.run_until_parked();
    t.sessions
        .update(cx, |state, _| state.invalidate_loaded("s1"));
    gate.release();
    assert!(load.await.is_none());
    let reload = t
        .sessions
        .update(cx, |state, cx| state.load_stored("s1", cx));
    assert!(reload.await.is_some());
}

#[gpui::test]
async fn a_session_being_removed_does_not_open(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("s1"));
    t.sessions.update(cx, |state, _| state.begin_removal("s1"));
    let open = t
        .sessions
        .update(cx, |state, cx| state.ensure_open("s1", cx));
    assert!(open.await.is_none());
    assert!(t.get(cx, "s1").is_none());
}

#[gpui::test]
async fn prefetch_fills_the_cache_and_open_takes_it(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&chat("s1"));
    t.backend.insert_session(&chat("s2"));
    t.sessions.update(cx, |state, cx| {
        state.prefetch("s1", cx);
        // One prefetch at a time.
        state.prefetch("s2", cx);
    });
    cx.run_until_parked();
    assert_eq!(t.count("session_get"), 1);
    t.sessions
        .read_with(cx, |state, _| assert!(state.loaded_cache().contains("s1")));
    let open = t
        .sessions
        .update(cx, |state, cx| state.ensure_open("s1", cx));
    assert_eq!(open.await.unwrap().id, "s1");
    assert_eq!(t.count("session_get"), 1);
    t.sessions
        .read_with(cx, |state, _| assert!(!state.loaded_cache().contains("s1")));
}

#[gpui::test]
async fn loading_a_claude_session_restores_bare_shell_rows(cx: &mut TestAppContext) {
    let t = setup(cx);
    let mut stored = Session {
        harness: HarnessId::Claude,
        provider_session_id: Some("claude-thread".into()),
        ..chat("s1")
    };
    stored.blocks.push(Block {
        tool: Some(BlockTool {
            call_id: Some("toolu_1".into()),
            kind: Some("execute".into()),
            title: Some("Shell".into()),
            ..BlockTool::default()
        }),
        ..Block::new("t1", BlockRole::Tool, "Shell")
    });
    t.backend.insert_session(&stored);
    t.backend
        .set_shell_commands([("toolu_1".to_string(), "cargo test".to_string())].into());
    let open = t
        .sessions
        .update(cx, |state, cx| state.ensure_open("s1", cx));
    let session = open.await.unwrap();
    assert_ne!(session.blocks[1].text, "Shell");
    assert_eq!(t.count("claude_shell_commands"), 1);
    assert_eq!(t.count("session_upsert"), 1);
    let saved = t.backend.record("s1").unwrap();
    assert_eq!(saved.blocks[1]["text"], json!(session.blocks[1].text));
}

// Lifecycle (appLifecycle.test.ts).

fn attach(t: &Harness, cx: &mut TestAppContext, sessions: Vec<Session>) {
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect::<Vec<_>>();
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    *t.workspace.tab_session_ids.borrow_mut() = ids;
    t.open(cx, sessions);
    t.lifecycle
        .update(cx, |lifecycle, _| lifecycle.attach_workspace());
    cx.run_until_parked();
    t.backend.clear_calls();
}

#[gpui::test]
async fn a_live_quit_saves_transcripts_the_snapshot_and_the_quit_list(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.snapshot.borrow_mut() =
        Some(json!({ "memory": [["/alpha", "a2"], ["/beta", "b2"]] }));
    attach(&t, cx, vec![chat("a1"), busy("b2")]);
    let quit = t
        .lifecycle
        .update(cx, |lifecycle, cx| lifecycle.handle_quit_requested(cx));
    assert!(quit.await);
    assert!(
        t.lifecycle
            .read_with(cx, |lifecycle, _| lifecycle.is_quitting())
    );
    let snapshot = t.backend.workspace_snapshot().unwrap();
    assert_eq!(
        snapshot["memory"],
        json!([["/alpha", "a2"], ["/beta", "b2"]])
    );
    assert_eq!(
        t.backend.in_flight(),
        vec![("b2".to_string(), CWD.to_string())]
    );
    let saved = t.backend.record("b2").unwrap();
    assert_eq!(
        saved.blocks.as_array().unwrap().last().unwrap()["text"],
        INTERRUPT_MESSAGE
    );
    assert_eq!(t.count("session_upsert"), 2);
}

#[gpui::test]
async fn an_unload_save_keeps_a_restored_quit_list(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.snapshot.borrow_mut() = Some(json!({}));
    attach(&t, cx, vec![chat("a1")]);
    t.backend.set_in_flight(vec![("restored", CWD)]);
    let sessions = t.sessions.read_with(cx, |state, _| state.all().to_vec());
    let save = cx.update(|cx| persist_quit_state(sessions, QuitMode::Unload, cx));
    save.await.unwrap();
    assert_eq!(t.count("workspace_set_snapshot"), 1);
    assert_eq!(t.count("session_set_in_flight"), 0);
    assert_eq!(t.backend.in_flight().len(), 1);
}

#[gpui::test]
async fn closing_a_busy_window_stops_only_its_sessions_and_closes_only_it(cx: &mut TestAppContext) {
    let t = setup(cx);
    attach(&t, cx, vec![busy("s1")]);
    let close = t
        .lifecycle
        .update(cx, |lifecycle, cx| lifecycle.close_busy_window(cx));
    close.await;
    assert_eq!(t.workspace.confirms.borrow().len(), 1);
    assert_eq!(t.harness.calls(), vec!["forget cursor s1"]);
    assert_eq!(*t.workspace.closed_windows.borrow(), 1);
}

#[gpui::test]
async fn a_cancelled_close_leaves_the_window_and_sessions_running(cx: &mut TestAppContext) {
    let t = setup(cx);
    attach(&t, cx, vec![busy("s1")]);
    *t.workspace.confirm_answer.borrow_mut() = false;
    let close = t
        .lifecycle
        .update(cx, |lifecycle, cx| lifecycle.close_busy_window(cx));
    close.await;
    assert!(t.harness.calls().is_empty());
    assert!(t.backend.commands().is_empty());
    assert_eq!(*t.workspace.closed_windows.borrow(), 0);
    assert!(
        !t.lifecycle
            .read_with(cx, |lifecycle, _| lifecycle.is_quitting())
    );
}

#[gpui::test]
fn reports_this_workspaces_live_turns(cx: &mut TestAppContext) {
    let t = setup(cx);
    assert_eq!(t.lifecycle.update(cx, |l, cx| l.in_flight_count(cx)), 0);
    let inbox = Session {
        inbox_ask: Some(inbox_context()),
        ..busy("inbox")
    };
    attach(&t, cx, vec![busy("s1"), inbox, chat("idle")]);
    // An Inbox Ask counts: it is work nobody agreed to throw away.
    assert_eq!(t.lifecycle.update(cx, |l, cx| l.in_flight_count(cx)), 2);
}

#[gpui::test]
async fn the_quit_dialog_passes_back_the_answer_and_asks_once(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.confirm_answer.borrow_mut() = false;
    let declined = t
        .lifecycle
        .update(cx, |l, cx| l.ask_quit_confirmation(2, cx));
    assert!(!declined.await);
    *t.workspace.confirm_answer.borrow_mut() = true;
    let confirmed = t
        .lifecycle
        .update(cx, |l, cx| l.ask_quit_confirmation(5, cx));
    assert!(confirmed.await);
    let confirms = t.workspace.confirms.borrow();
    assert_eq!(confirms.len(), 2);
    assert!(confirms[1].contains("5 chats are still running"));
}

#[gpui::test]
async fn a_failed_quit_write_calls_the_quit_off(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.snapshot.borrow_mut() = Some(json!({}));
    attach(&t, cx, vec![busy("s1")]);
    t.backend.set_failing("workspace_set_snapshot", true);
    let commit = t.lifecycle.update(cx, |l, cx| l.commit_quit(cx));
    assert!(!commit.await);
    assert!(!t.lifecycle.read_with(cx, |l, _| l.is_quitting()));
}

#[gpui::test]
async fn commit_quit_persists_and_reports_ready(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.snapshot.borrow_mut() = Some(json!({}));
    attach(&t, cx, vec![busy("s1")]);
    let commit = t.lifecycle.update(cx, |l, cx| l.commit_quit(cx));
    assert!(commit.await);
    assert_eq!(t.count("workspace_set_snapshot"), 1);
    assert!(t.lifecycle.read_with(cx, |l, _| l.is_quitting()));
}

#[gpui::test]
async fn confirm_reload_only_asks_with_unsaved_files(cx: &mut TestAppContext) {
    let t = setup(cx);
    let clean = cx.update(|cx| Lifecycle::confirm_reload(false, cx));
    assert!(clean.await);
    assert!(t.workspace.confirms.borrow().is_empty());
    let dirty = cx.update(|cx| Lifecycle::confirm_reload(true, cx));
    assert!(dirty.await);
    assert_eq!(
        t.workspace.confirms.borrow().as_slice(),
        ["Reload MonoCode and discard unsaved changes?"]
    );
    *t.workspace.confirm_answer.borrow_mut() = false;
    let kept = cx.update(|cx| Lifecycle::confirm_reload(true, cx));
    assert!(!kept.await);
}

#[gpui::test]
async fn closing_an_idle_window_saves_then_hides_or_closes(cx: &mut TestAppContext) {
    let t = setup(cx);
    *t.workspace.snapshot.borrow_mut() = Some(json!({}));
    attach(&t, cx, vec![chat("s1")]);
    let close = t
        .lifecycle
        .update(cx, |l, cx| l.handle_close_requested(false, cx));
    close.await;
    assert_eq!(*t.workspace.closed_windows.borrow(), 1);
    assert_eq!(t.count("workspace_set_snapshot"), 1);
    let to_tray = t
        .lifecycle
        .update(cx, |l, cx| l.handle_close_requested(true, cx));
    to_tray.await;
    assert_eq!(*t.workspace.hidden_windows.borrow(), 1);
}

// Boot (bootWorkspace.test.ts and the boot half of appLifecycle.test.ts).

fn saved_workspace(t: &Harness) -> Vec<Session> {
    let sessions: Vec<Session> = (0..10)
        .map(|index| {
            let mut session =
                Session::blank(format!("session-{index}"), HarnessId::Codex, "m", CWD);
            session.blocks = vec![Block::new(
                format!("user-{index}"),
                BlockRole::User,
                "Hello",
            )];
            session
        })
        .collect();
    for session in &sessions {
        t.backend.insert_session(session);
    }
    let ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    t.backend.set_workspace_snapshot(Some(json!({
        "projectCwd": CWD, "layout": { "tabs": ids.clone() }, "sessionIds": ids
    })));
    t.backend.clear_calls();
    sessions
}

#[gpui::test]
async fn restores_ten_idle_tabs_without_rewriting_their_transcripts_before_first_paint(
    cx: &mut TestAppContext,
) {
    let t = setup(cx);
    let sessions = saved_workspace(&t);
    let restore = t.lifecycle.update(cx, |l, cx| l.load_resumed_workspace(cx));
    let restored = restore.await.unwrap();
    let ids: Vec<&str> = restored.sessions.iter().map(|s| s.id.as_str()).collect();
    let expected: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, expected);
    assert_eq!(restored.layout["tabs"].as_array().unwrap().len(), 10);
    assert_eq!(t.count("session_upsert"), 0);
}

#[gpui::test]
async fn still_persists_the_interruption_marker_for_a_recovered_running_turn(
    cx: &mut TestAppContext,
) {
    let t = setup(cx);
    let sessions = saved_workspace(&t);
    t.backend
        .set_in_flight(vec![(sessions[0].id.as_str(), CWD)]);
    let restore = t.lifecycle.update(cx, |l, cx| l.load_resumed_workspace(cx));
    let restored = restore.await.unwrap();
    assert_eq!(t.count("session_upsert"), 1);
    let first = &restored.sessions[0];
    assert_eq!(
        first.blocks.last().unwrap().notice,
        Some(monocode_core::block::BlockNotice::Interrupt)
    );
    let saved = t.backend.record(&first.id).unwrap();
    assert_eq!(
        saved.blocks.as_array().unwrap().last().unwrap()["text"],
        INTERRUPT_MESSAGE
    );
}

#[gpui::test]
async fn without_a_snapshot_the_quit_list_opens_one_tab_per_chat(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.backend.insert_session(&busy("a"));
    t.backend.insert_session(&chat("b"));
    t.backend.set_in_flight(vec![("a", CWD), ("missing", CWD)]);
    let restore = t.lifecycle.update(cx, |l, cx| l.load_resumed_workspace(cx));
    let restored = restore.await.unwrap();
    assert_eq!(restored.sessions.len(), 1);
    assert_eq!(restored.project_cwd, CWD);
    assert_eq!(restored.layout, json!({ "tabs": ["a"] }));
    assert_eq!(
        restored.sessions[0].blocks.last().unwrap().text,
        INTERRUPT_MESSAGE
    );
}

#[gpui::test]
async fn boot_lists_the_restored_projects_history(cx: &mut TestAppContext) {
    let t = setup(cx);
    saved_workspace(&t);
    let boot = t.lifecycle.update(cx, |l, cx| {
        l.load_boot_workspace(Some(format!("{CWD}/")), cx)
    });
    let boot = boot.await;
    assert!(boot.window_transfer.is_none());
    assert_eq!(boot.history_cwd.as_deref(), Some(CWD));
    assert_eq!(boot.history.len(), 10);
    // The hinted project matched, so it was listed once.
    assert_eq!(t.count("session_list_by_project"), 1);
    let again = t
        .lifecycle
        .update(cx, |l, cx| l.load_boot_workspace(None, cx));
    assert_eq!(again.await, boot);
}

#[gpui::test]
async fn a_window_transfer_boots_without_restoring(cx: &mut TestAppContext) {
    let t = setup(cx);
    t.lifecycle.update(cx, |l, _| {
        l.put_window_transfer(json!({ "projectCwd": "~", "tabs": [] }))
    });
    let boot = t
        .lifecycle
        .update(cx, |l, cx| l.load_boot_workspace(None, cx));
    let boot = boot.await;
    assert_eq!(boot.window_transfer.unwrap()["tabs"], json!([]));
    assert!(boot.resumed.is_none());
    assert_eq!(t.count("workspace_get_snapshot"), 0);
    assert!(
        t.lifecycle
            .update(cx, |l, _| l.take_window_transfer())
            .is_none()
    );
}

#[gpui::test]
async fn quitting_before_the_workspace_attaches_saves_the_restore(cx: &mut TestAppContext) {
    let t = setup(cx);
    let sessions = saved_workspace(&t);
    t.backend
        .set_in_flight(vec![(sessions[1].id.as_str(), CWD)]);
    let quit = t.lifecycle.update(cx, |l, cx| l.handle_quit_requested(cx));
    assert!(quit.await);
    let snapshot = t.backend.workspace_snapshot().unwrap();
    assert_eq!(snapshot["layout"]["tabs"].as_array().unwrap().len(), 10);
    assert_eq!(
        t.backend.in_flight(),
        vec![(sessions[1].id.clone(), CWD.to_string())]
    );
}

#[gpui::test]
async fn interrupted_sessions_get_one_note(cx: &mut TestAppContext) {
    let session = busy("s1");
    let once = mark_turn_interrupted(&session);
    assert_eq!(mark_turn_interrupted(&once).blocks.len(), once.blocks.len());
    let _ = cx;
}

// Session store concurrency (sessionStoreConcurrency.test.ts).

fn writer_session(id: &str) -> Session {
    Session {
        busy: Some(true),
        blocks: vec![Block::new("user", BlockRole::User, "hello")],
        ..Session::blank(id, HarnessId::Cursor, "", "/tmp/project")
    }
}

#[gpui::test]
async fn drains_worker_writes_before_deleting_a_lead_and_strips_ownership_from_later_snapshots(
    cx: &mut TestAppContext,
) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let worker = Session {
        orchestration_lead_id: Some("lead".into()),
        ..writer_session("worker")
    };
    let gate = t.backend.hold_next("session_upsert");
    let writing = writer.upsert_session(&worker);
    cx.run_until_parked();
    assert_eq!(t.backend.commands(), vec!["session_upsert"]);
    let queued = writer.upsert_session(&worker);
    let deleting = writer.delete_session("lead", Vec::new());
    cx.run_until_parked();
    assert_eq!(t.backend.commands(), vec!["session_upsert"]);
    gate.release();
    let (a, b, c) = futures::future::join3(writing, queued, deleting).await;
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(
        t.backend.commands(),
        vec!["session_upsert", "session_upsert", "session_delete"]
    );
    writer.upsert_session(&worker).await.unwrap();
    let upserts = t.backend.calls("session_upsert");
    assert_eq!(upserts.len(), 3);
    assert_eq!(
        upserts[0]["session"]["blocks"][0]["orchestrationLeadId"],
        "lead"
    );
    assert!(
        upserts[2]["session"]["blocks"][0]
            .get("orchestrationLeadId")
            .is_none()
    );
}

#[gpui::test]
async fn serializes_deletion_after_an_active_write_and_drops_a_queued_late_upsert(
    cx: &mut TestAppContext,
) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let gate = t.backend.hold_next("session_upsert");
    let writing = writer.upsert_session(&writer_session("s1"));
    cx.run_until_parked();
    let late = writer.upsert_session(&Session {
        title: "late".into(),
        ..writer_session("s1")
    });
    let deleting = writer.delete_session("s1", Vec::new());
    cx.run_until_parked();
    assert_eq!(t.backend.commands(), vec!["session_upsert"]);
    gate.release();
    let (_, late, _) = futures::future::join3(writing, late, deleting).await;
    assert_eq!(late, Ok(None));
    assert_eq!(
        t.backend.commands(),
        vec!["session_upsert", "session_delete"]
    );
    assert!(writer.is_deleted("s1"));
    cx.executor().advance_clock(Duration::from_secs(60));
    assert!(!writer.is_deleted("s1"));
}

#[gpui::test]
async fn discards_a_draft_only_record_before_reusing_its_open_session_id(cx: &mut TestAppContext) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let gate = t.backend.hold_next("session_upsert");
    let draft = writer.upsert_session(&writer_session("s1"));
    cx.run_until_parked();
    let discarding = writer.discard_draft_session_record("s1");
    let later = writer.upsert_session(&Session {
        title: "later".into(),
        ..writer_session("s1")
    });
    gate.release();
    let _ = futures::future::join3(draft, discarding, later).await;
    assert_eq!(
        t.backend.commands(),
        vec!["session_upsert", "session_delete", "session_upsert"]
    );
}

#[gpui::test]
async fn drains_queued_saves_before_detaching_a_removed_worktree(cx: &mut TestAppContext) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let gate = t.backend.hold_next("session_upsert");
    let active = writer.upsert_session(&writer_session("s1"));
    cx.run_until_parked();
    let queued = writer.upsert_session(&Session {
        title: "latest".into(),
        ..writer_session("s1")
    });
    let flushed = Rc::new(RefCell::new(false));
    let flag = flushed.clone();
    let flush = writer.flush_session_writes();
    let flushing = cx.spawn(async move |_| {
        flush.await;
        *flag.borrow_mut() = true;
    });
    cx.run_until_parked();
    assert!(!*flushed.borrow());
    gate.release();
    let _ = futures::future::join3(active, queued, flushing).await;
    assert_eq!(t.count("session_upsert"), 2);
    assert!(*flushed.borrow());
}

#[gpui::test]
async fn archives_only_after_the_final_active_turn_snapshot_is_durable(cx: &mut TestAppContext) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let gate = t.backend.hold_next("session_upsert");
    let writing = writer.upsert_session(&writer_session("s1"));
    cx.run_until_parked();
    let mut last = writer_session("s1");
    last.blocks.push(Block::new(
        "assistant",
        BlockRole::Assistant,
        "final buffered output",
    ));
    let final_snapshot = writer.upsert_session(&last);
    let archiving = writer.set_session_archived("s1", true);
    cx.run_until_parked();
    assert_eq!(t.backend.commands(), vec!["session_upsert"]);
    gate.release();
    let _ = futures::future::join3(writing, final_snapshot, archiving).await;
    assert_eq!(
        t.backend.commands(),
        vec!["session_upsert", "session_upsert", "session_set_archived"]
    );
    let second = &t.backend.calls("session_upsert")[1];
    assert_eq!(
        second["session"]["blocks"][1]["text"],
        "final buffered output"
    );
}

#[gpui::test]
async fn in_flight_and_workspace_writes_land_in_call_order(cx: &mut TestAppContext) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let gate = t.backend.hold_next("session_set_in_flight");
    let first = writer.replace_in_flight_sessions(vec![super::InFlightRef {
        session_id: "a".into(),
        cwd: "/repo/".into(),
    }]);
    let second = writer.replace_in_flight_sessions(Vec::new());
    cx.run_until_parked();
    assert_eq!(t.count("session_set_in_flight"), 1);
    gate.release();
    let _ = futures::future::join(first, second).await;
    let calls = t.backend.calls("session_set_in_flight");
    assert_eq!(
        calls[0]["sessions"],
        json!([{ "sessionId": "a", "cwd": "/repo" }])
    );
    assert_eq!(calls[1]["sessions"], json!([]));
    assert!(t.backend.in_flight().is_empty());
}

// sessionStoreLinkedWorkItem.test.ts.

#[gpui::test]
async fn persists_a_canonical_github_work_item_through_the_metadata_command(
    cx: &mut TestAppContext,
) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    let item = LinkedWorkItem {
        kind: WorkItemKind::Pr,
        repo: "openai/codex".into(),
        number: 42,
        url: "https://example.com/untrusted".into(),
        extra: Extra::new(),
    };
    writer
        .set_session_linked_work_item("session-1", Some(&item))
        .await
        .unwrap();
    assert_eq!(
        t.backend.calls("session_set_linked_work_item"),
        vec![json!({
            "sessionId": "session-1",
            "linkedWorkItem": {
                "kind": "pr", "repo": "openai/codex", "number": 42,
                "url": "https://github.com/openai/codex/pull/42"
            }
        })]
    );
}

#[gpui::test]
async fn uses_null_to_remove_a_persisted_link(cx: &mut TestAppContext) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    writer
        .set_session_linked_work_item("session-1", None)
        .await
        .unwrap();
    assert_eq!(
        t.backend.calls("session_set_linked_work_item"),
        vec![json!({ "sessionId": "session-1", "linkedWorkItem": Value::Null })]
    );
}

#[gpui::test]
async fn lists_and_searches_through_the_writer(cx: &mut TestAppContext) {
    let t = setup(cx);
    let writer = cx.update(|cx| Engine::writer(cx));
    t.backend.insert_session(&chat("s1"));
    assert!(
        writer
            .list_sessions_by_project("~")
            .await
            .unwrap()
            .is_empty()
    );
    let rows = writer.list_sessions_by_project("/repo/").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].harness, HarnessId::Cursor);
    assert_eq!(
        t.backend.calls("session_list_by_project"),
        vec![json!({ "cwd": "/repo" })]
    );
    let empty = writer
        .search_sessions("  ", "owner", None, false)
        .await
        .unwrap();
    assert!(empty.hits.is_empty());
    assert_eq!(t.count("session_search"), 0);
    writer
        .search_sessions(" fix ", "owner", Some("~"), false)
        .await
        .unwrap();
    assert_eq!(
        t.backend.calls("session_search"),
        vec![json!({ "query": "fix", "cwd": null })]
    );
}

// Edits (`trackSessionEdits`, `nudgeOpenEditors`).

fn edit_event(updated: bool, status: &str) -> HarnessEvent {
    if updated {
        HarnessEvent::ToolUpdated {
            agent_model: None,
            call_id: "c".into(),
            title: Some("Edit".into()),
            kind: Some("edit".into()),
            status: Some(status.into()),
            detail: None,
            preview: None,
            paths: Some(vec!["src/a.rs".into(), "src/a.rs".into()]),
        }
    } else {
        HarnessEvent::ToolStarted {
            agent_model: None,
            call_id: "c".into(),
            title: "Edit".into(),
            kind: Some("edit".into()),
            status: None,
            background: None,
            preview: None,
            paths: Some(vec!["src/a.rs".into()]),
        }
    }
}

#[gpui::test]
fn agent_edits_snapshot_before_and_capture_after(cx: &mut TestAppContext) {
    let t = setup(cx);
    let review = cx.update(|cx| Engine::global(cx).review.clone());
    let changed = Rc::new(RefCell::new(Vec::new()));
    let recorded = changed.clone();
    cx.update(|cx| {
        cx.subscribe(&review, move |_, event: &super::ReviewChanged, _| {
            recorded.borrow_mut().push(event.session_id.clone())
        })
        .detach()
    });
    cx.update(|cx| super::edits::track_session_edits("s1", CWD, &edit_event(false, ""), cx));
    cx.run_until_parked();
    assert_eq!(t.backend.commands(), vec!["session_checkpoint_prepare"]);
    cx.update(|cx| {
        super::edits::track_session_edits("s1", CWD, &edit_event(true, "completed"), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        t.backend.commands(),
        vec!["session_checkpoint_prepare", "session_checkpoint_capture"]
    );
    assert_eq!(
        t.backend.calls("session_checkpoint_capture")[0]["paths"],
        json!(["src/a.rs"])
    );
    assert_eq!(changed.borrow().as_slice(), ["s1"]);
    cx.update(|cx| {
        super::edits::track_session_edits("s1", "~", &edit_event(true, "completed"), cx)
    });
    cx.update(|cx| super::edits::track_session_edits("s1", CWD, &delta("x"), cx));
    cx.run_until_parked();
    assert_eq!(t.backend.commands().len(), 2);
}

#[gpui::test]
async fn checkpoints_run_in_order_per_session(cx: &mut TestAppContext) {
    let t = setup(cx);
    let checkpoints = cx.update(|cx| Engine::checkpoints(cx));
    let gate = t.backend.hold_next("session_checkpoint_prepare");
    let prepare = checkpoints.prepare("s1", CWD, vec!["a".into()]);
    let capture = checkpoints.capture("s1", CWD, vec!["a".into()]);
    let other = checkpoints.ensure("s2", CWD);
    cx.run_until_parked();
    // Different sessions run concurrently; s1's capture waits for its prepare.
    let mut started = t.backend.commands();
    started.sort();
    assert_eq!(
        started,
        vec!["session_checkpoint_ensure", "session_checkpoint_prepare"]
    );
    let flush = checkpoints.flush_session_checkpoint("s1");
    gate.release();
    let _ = futures::future::join3(prepare, capture, other).await;
    flush.await;
    assert_eq!(t.count("session_checkpoint_capture"), 1);
    assert!(checkpoints.prepare("s1", CWD, Vec::new()).await.is_ok());
    assert_eq!(t.count("session_checkpoint_prepare"), 1);
}
