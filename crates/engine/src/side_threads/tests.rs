//! Flow tests for side threads, driven through a fake text-prompt adapter
//! and the runtime's `FakeBackend`. They follow the App.tsx behavior of
//! `onSecondOpinion`, `onHandoff`, `onBtwSubmit`, `onBtwStop`,
//! `onBtwRetry`, `onBtwDelete`, `onBtwModelChange`, the `btwRequestsRef`
//! cleanup effects, `onHandoffCardDismiss`,
//! `dismissNoticesForContinuedSession`, and the `liveAgents` memo.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{App, TestAppContext};
use monocode_core::block::{
    Block, BlockRole, BtwMessageRole, BtwThread, BtwThreadStatus, ModelTarget, SecondOpinionKind,
    TurnIntent,
};
use monocode_core::harness_event::{ApprovalDecision, SendTurnInput, SteerTurnInput};
use monocode_core::{HarnessEvent, HarnessId, ModelSettings, Session};
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, HarnessAdapter, HarnessRegistry, RegistryOptions,
    TextPromptInput,
};
use monocode_harness::core::task::SharedSpawner;
use monocode_settings::Kv;
use parking_lot::Mutex;
use serde_json::json;

use super::btw::find_turn;
use super::{BtwSubmit, SideThreadPeers, SideThreads, SideThreadsConfig, live_agents};
use crate::runtime::engine::Engine;
use crate::runtime::hooks::{AttentionHooks, EngineHooks};
use crate::runtime::testing::{FakeBackend, init_test_engine, init_test_engine_with};
use crate::submit::SubmitOptions;

const CWD: &str = "/repo";

/// One scripted reply to `run_text_prompt`.
struct TextScript {
    thread_id: Option<String>,
    events: Vec<HarnessEvent>,
    result: Result<String, String>,
    /// Wait for `release` or the abort signal before resolving.
    hold: bool,
}

impl TextScript {
    fn reply(text: &str) -> Self {
        Self {
            thread_id: None,
            events: Vec::new(),
            result: Ok(text.to_string()),
            hold: false,
        }
    }
}

struct FakeText {
    id: HarnessId,
    scripts: Mutex<VecDeque<TextScript>>,
    prompts: Mutex<Vec<TextPromptInput>>,
    stops: AtomicUsize,
    release: (async_channel::Sender<()>, async_channel::Receiver<()>),
}

impl FakeText {
    fn new(id: HarnessId) -> Arc<Self> {
        Arc::new(Self {
            id,
            scripts: Mutex::default(),
            prompts: Mutex::default(),
            stops: AtomicUsize::new(0),
            release: async_channel::unbounded(),
        })
    }

    fn push(&self, script: TextScript) {
        self.scripts.lock().push_back(script);
    }

    fn prompts(&self) -> Vec<TextPromptInput> {
        self.prompts.lock().clone()
    }

    fn release(&self) {
        let _ = self.release.0.try_send(());
    }
}

impl HarnessAdapter for FakeText {
    fn id(&self) -> HarnessId {
        self.id
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            run_text_prompt: true,
            stop_text_prompt: true,
            ..AdapterCapabilities::default()
        }
    }

    fn send_turn(
        &self,
        _input: SendTurnInput,
        _on_event: EventSink,
        _on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn steer_turn(&self, _input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn cancel_turn(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn respond_approval(&self, _session_id: &str, _request_id: i64, _decision: ApprovalDecision) {}

    fn stop_session(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn forget_session(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn bind_session(
        &self,
        _thread_id: &str,
        _provider_session_id: &str,
        _cwd: &str,
        _account: Option<&str>,
    ) {
    }

    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        self.prompts.lock().push(input.clone());
        let script = self
            .scripts
            .lock()
            .pop_front()
            .unwrap_or_else(|| TextScript::reply("Because."));
        let release = self.release.1.clone();
        async move {
            if let (Some(thread_id), Some(on_thread_id)) = (script.thread_id, &input.on_thread_id) {
                on_thread_id(thread_id);
            }
            if let Some(on_event) = &input.on_event {
                for event in script.events {
                    on_event(event);
                }
            }
            if script.hold {
                let signal = input.signal.clone();
                smol::future::or(
                    async {
                        let _ = release.recv().await;
                    },
                    async {
                        match signal {
                            Some(signal) => signal.aborted().await,
                            None => futures::future::pending().await,
                        }
                    },
                )
                .await;
            }
            script.result.map_err(|error| anyhow!(error))
        }
        .boxed()
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }.boxed()
    }
}

#[derive(Default)]
struct RecordingPeers {
    submits: RefCell<Vec<(String, String, SubmitOptions)>>,
    beside: RefCell<Vec<(String, String, String, bool)>>,
    reminders: RefCell<Vec<String>>,
    seen: RefCell<Vec<(String, i64)>>,
}

impl SideThreadPeers for RecordingPeers {
    fn submit(&self, session_id: &str, text: &str, options: SubmitOptions, _cx: &mut App) -> bool {
        self.submits
            .borrow_mut()
            .push((session_id.into(), text.into(), options));
        true
    }

    fn open_session_beside(
        &self,
        source_id: &str,
        session_id: &str,
        cwd: &str,
        focus_composer: bool,
        _cx: &mut App,
    ) {
        self.beside.borrow_mut().push((
            source_id.into(),
            session_id.into(),
            cwd.into(),
            focus_composer,
        ));
    }

    fn dismiss_due_reminders(&self, session_id: &str, _cx: &mut App) {
        self.reminders.borrow_mut().push(session_id.into());
    }

    fn mark_linked_session_update_seen(
        &self,
        session_id: &str,
        remote_updated_at: i64,
        _cx: &mut App,
    ) {
        self.seen
            .borrow_mut()
            .push((session_id.into(), remote_updated_at));
    }
}

struct Fixture {
    side: SideThreads,
    claude: Arc<FakeText>,
    peers: Rc<RecordingPeers>,
    backend: Arc<FakeBackend>,
}

fn install(cx: &mut TestAppContext, backend: Arc<FakeBackend>) -> Fixture {
    let executor = cx.executor();
    let spawner: SharedSpawner =
        Arc::new(move |future: BoxFuture<'static, ()>| executor.spawn(future).detach());
    let registry = HarnessRegistry::new(
        spawner,
        RegistryOptions {
            turn_control: None,
            idle_park: Duration::from_secs(86_400),
        },
    );
    let claude = FakeText::new(HarnessId::Claude);
    registry.register_harness(claude.clone());
    let config = SideThreadsConfig {
        registry,
        catalog: SharedCatalog::new(),
        kv: Kv::in_memory(),
    };
    let side = cx.update(|cx| SideThreads::init(config, cx));
    let peers = Rc::new(RecordingPeers::default());
    side.set_peers(peers.clone());
    Fixture {
        side,
        claude,
        peers,
        backend,
    }
}

fn setup(cx: &mut TestAppContext) -> Fixture {
    let backend = init_test_engine(cx);
    install(cx, backend)
}

fn user(id: &str, text: &str) -> Block {
    let mut block = Block::new(id, BlockRole::User, text);
    block.started_at = Some(1);
    block.duration_ms = Some(1_000);
    block
}

/// A Claude chat with one finished turn.
fn chat(id: &str) -> Session {
    let mut session = Session::blank(id, HarnessId::Claude, "claude:sonnet", CWD);
    session.title = "claude · Parser work".into();
    session.blocks = vec![
        user("u1", "Explain the parser"),
        Block::new("a1", BlockRole::Assistant, "It parses tokens."),
    ];
    session
}

fn insert(session: Session, cx: &mut TestAppContext) {
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
    });
}

fn session(id: &str, cx: &mut TestAppContext) -> Session {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned())
        .unwrap_or_else(|| panic!("session {id} is not open"))
}

fn turn(session_id: &str, cx: &mut TestAppContext) -> Vec<Block> {
    find_turn(&session(session_id, cx).blocks, "u1", false).unwrap()
}

fn threads(session_id: &str, cx: &mut TestAppContext) -> Vec<BtwThread> {
    session(session_id, cx)
        .blocks
        .into_iter()
        .find(|block| block.id == "u1")
        .and_then(|block| block.btw_threads)
        .unwrap_or_default()
}

fn ask(fixture: &Fixture, thread_id: &str, text: &str, cx: &mut TestAppContext) -> bool {
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| {
        side.btw_submit(
            BtwSubmit {
                session_id: "s1",
                turn: &turn,
                thread_id,
                message_id: &format!("{thread_id}-{text}"),
                text,
                model: None,
                model_settings: None,
            },
            cx,
        )
    })
}

fn target(harness: HarnessId, model: &str) -> ModelTarget {
    ModelTarget {
        harness,
        model: model.into(),
        model_settings: ModelSettings::new(),
    }
}

#[gpui::test]
fn second_opinion_opens_a_chat_beside_the_source_and_sends_the_review(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut source = chat("s1");
    source.worktree_cwd = Some("/repo-wt".into());
    source.branch = Some("feature".into());
    source.blocks[0].ci_context = Some("CI failed on lint".into());
    insert(source, cx);
    let turn = turn("s1", cx);

    let side = fixture.side.clone();
    let id = cx
        .update(|cx| side.second_opinion("s1", &target(HarnessId::Codex, "gpt-5"), &turn, cx))
        .unwrap();

    let opened = session(&id, cx);
    assert_eq!(opened.harness, HarnessId::Codex);
    assert_eq!(opened.cwd, CWD);
    assert_eq!(opened.worktree_cwd.as_deref(), Some("/repo-wt"));
    assert_eq!(opened.branch.as_deref(), Some("feature"));
    assert_eq!(opened.title, "codex · Second opinion");
    assert_eq!(
        *fixture.peers.beside.borrow(),
        [("s1".to_string(), id.clone(), CWD.to_string(), false)]
    );
    let submits = fixture.peers.submits.borrow();
    let (submitted, prompt, options) = &submits[0];
    assert_eq!(submitted, &id);
    assert!(prompt.starts_with("Give a second opinion on work Claude Code just finished"));
    assert_eq!(options.ci_context.as_deref(), Some("CI failed on lint"));
    let card = options.second_opinion.as_ref().unwrap();
    assert_eq!(card.from, HarnessId::Claude);
    assert_eq!(card.to, HarnessId::Codex);
    assert_eq!(card.request.as_deref(), Some("Explain the parser"));
    assert_eq!(card.kind, None);
}

#[gpui::test]
fn second_opinion_and_handoff_skip_a_removed_worktree(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut source = chat("s1");
    source.worktree_removed = Some(true);
    insert(source, cx);
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| {
        assert!(
            side.second_opinion("s1", &target(HarnessId::Codex, "gpt-5"), &turn, cx)
                .is_none()
        );
        assert!(
            side.handoff("s1", &target(HarnessId::Codex, "gpt-5"), &turn, cx)
                .is_none()
        );
        assert!(
            side.handoff("missing", &target(HarnessId::Codex, "gpt-5"), &turn, cx)
                .is_none()
        );
    });
    assert!(fixture.peers.submits.borrow().is_empty());
    assert!(fixture.peers.beside.borrow().is_empty());
}

#[gpui::test]
fn handoff_opens_a_chat_with_the_recap_on_its_composer(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut source = chat("s1");
    source.blocks.push(user("u2", "A later turn"));
    insert(source, cx);
    let turn = turn("s1", cx);

    let side = fixture.side.clone();
    let id = cx
        .update(|cx| side.handoff("s1", &target(HarnessId::Codex, "gpt-5"), &turn, cx))
        .unwrap();

    let opened = session(&id, cx);
    assert_eq!(opened.title, "codex · Parser work");
    let card = opened.handoff_card.unwrap();
    assert_eq!(card.from, HarnessId::Claude);
    assert_eq!(card.to, HarnessId::Codex);
    assert_eq!(card.request.as_deref(), Some("Explain the parser"));
    assert!(card.brief.contains("Explain the parser"));
    assert!(!card.brief.contains("A later turn"));
    assert_eq!(
        *fixture.peers.beside.borrow(),
        [("s1".to_string(), id, CWD.to_string(), true)]
    );
    assert!(fixture.peers.submits.borrow().is_empty());
}

#[gpui::test]
fn handoff_names_an_untitled_chat_handoff(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut source = chat("s1");
    source.title = HarnessId::Claude.label().to_string();
    insert(source, cx);
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    let id = cx
        .update(|cx| side.handoff("s1", &target(HarnessId::Codex, "gpt-5"), &turn, cx))
        .unwrap();
    assert_eq!(session(&id, cx).title, "codex · Handoff");
}

#[gpui::test]
fn btw_streams_activity_then_settles_a_ready_reply(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        thread_id: Some("provider-1".into()),
        events: vec![
            HarnessEvent::ReasoningDelta {
                text: "Checking".into(),
            },
            HarnessEvent::MessageDelta {
                text: "Because it".into(),
            },
        ],
        result: Ok("  Because it tokenizes first.  ".into()),
        hold: true,
    });

    assert!(ask(&fixture, "t1", "Why tokens?", cx));
    cx.run_until_parked();

    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Running);
    assert_eq!(thread.harness, Some(HarnessId::Claude));
    assert_eq!(thread.source_end_block_id, "a1");
    assert_eq!(thread.provider_thread_id.as_deref(), Some("provider-1"));
    let pending = thread.pending_blocks.unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].role, BlockRole::Reasoning);
    assert_eq!(pending[1].role, BlockRole::Assistant);
    assert!(pending[1].is_streaming());
    assert!(fixture.side.is_requesting("s1", "t1"));

    let prompt = fixture.claude.prompts().remove(0);
    assert_eq!(prompt.cwd, CWD);
    assert_eq!(prompt.intent, Some(TurnIntent::Plan));
    assert!(prompt.model.is_some());
    assert_eq!(prompt.thread_id, None);
    assert!(prompt.prompt.contains("User: Explain the parser"));
    assert!(
        prompt
            .prompt
            .contains("## By-the-way conversation\nUser: Why tokens?")
    );

    fixture.claude.release();
    cx.run_until_parked();

    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Ready);
    assert_eq!(thread.pending_blocks, None);
    assert_eq!(thread.error, None);
    assert_eq!(thread.messages.len(), 2);
    let answer = &thread.messages[1];
    assert_eq!(answer.role, BtwMessageRole::Assistant);
    assert_eq!(answer.text, "Because it tokenizes first.");
    let blocks = answer.blocks.as_ref().unwrap();
    assert!(blocks.iter().all(|block| !block.is_streaming()));
    assert!(!fixture.side.is_requesting("s1", "t1"));
    let stored = fixture.backend.record("s1").unwrap();
    assert_eq!(stored.blocks[0]["btwThreads"][0]["status"], json!("ready"));
}

#[gpui::test]
fn btw_follow_up_resumes_the_provider_thread(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        thread_id: Some("provider-1".into()),
        ..TextScript::reply("First answer.")
    });
    assert!(ask(&fixture, "t1", "One?", cx));
    cx.run_until_parked();

    assert!(ask(&fixture, "t1", "Two?", cx));
    cx.run_until_parked();

    let prompts = fixture.claude.prompts();
    assert_eq!(prompts[1].thread_id.as_deref(), Some("provider-1"));
    assert!(
        prompts[1]
            .prompt
            .contains("User: One?\n\nAssistant: First answer.\n\nUser: Two?")
    );
    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.messages.len(), 4);
    assert_eq!(thread.status, BtwThreadStatus::Ready);
}

#[gpui::test]
fn btw_refuses_a_running_thread_and_unsupported_providers(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        hold: true,
        ..TextScript::reply("Later.")
    });
    assert!(ask(&fixture, "t1", "One?", cx));
    cx.run_until_parked();
    assert!(!ask(&fixture, "t1", "Again?", cx));

    let mut fx = chat("s2");
    fx.harness = HarnessId::Fx;
    insert(fx, cx);
    let turn = find_turn(&session("s2", cx).blocks, "u1", false).unwrap();
    let side = fixture.side.clone();
    let accepted = cx.update(|cx| {
        side.btw_submit(
            BtwSubmit {
                session_id: "s2",
                turn: &turn,
                thread_id: "t2",
                message_id: "m2",
                text: "Why?",
                model: None,
                model_settings: None,
            },
            cx,
        )
    });
    assert!(!accepted);
    assert_eq!(fixture.claude.prompts().len(), 1);
}

#[gpui::test]
fn btw_reports_an_empty_answer_and_provider_errors(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript::reply("   "));
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Error);
    assert_eq!(
        thread.error.as_deref(),
        Some("Claude Code returned an empty side answer.")
    );

    fixture.claude.push(TextScript {
        result: Err("rate limited".into()),
        ..TextScript::reply("")
    });
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| side.btw_retry("s1", &turn, "t1", cx));
    cx.run_until_parked();
    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Error);
    assert_eq!(thread.error.as_deref(), Some("rate limited"));
    assert_eq!(thread.pending_blocks, None);
}

#[gpui::test]
fn btw_needs_a_project_folder(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut source = chat("s1");
    source.cwd = "~".into();
    insert(source, cx);
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Error);
    assert_eq!(
        thread.error.as_deref(),
        Some("A project working directory is required for this question.")
    );
    assert!(fixture.claude.prompts().is_empty());
}

#[gpui::test]
fn btw_retry_asks_a_failed_thread_again(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        result: Err("offline".into()),
        ..TextScript::reply("")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    assert_eq!(threads("s1", cx)[0].status, BtwThreadStatus::Error);

    fixture.claude.push(TextScript::reply("Now it works."));
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| side.btw_retry("s1", &turn, "t1", cx));
    cx.run_until_parked();

    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Ready);
    assert_eq!(thread.messages.len(), 2);
    assert_eq!(thread.messages[1].text, "Now it works.");
    assert_eq!(fixture.claude.prompts().len(), 2);

    // A ready thread does not retry.
    cx.update(|cx| side.btw_retry("s1", &turn, "t1", cx));
    cx.run_until_parked();
    assert_eq!(fixture.claude.prompts().len(), 2);
}

#[gpui::test]
fn btw_stop_keeps_what_streamed_and_readies_the_thread(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        events: vec![HarnessEvent::MessageDelta {
            text: "Partial answer".into(),
        }],
        hold: true,
        ..TextScript::reply("Never seen.")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();

    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| side.btw_stop("s1", &turn, "t1", cx));
    cx.run_until_parked();

    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Ready);
    assert_eq!(thread.pending_blocks, None);
    assert_eq!(thread.messages.len(), 2);
    assert_eq!(thread.messages[1].text, "Partial answer");
    assert!(
        thread.messages[1]
            .blocks
            .as_ref()
            .unwrap()
            .iter()
            .all(|block| !block.is_streaming())
    );
    assert!(!fixture.side.is_requesting("s1", "t1"));
}

#[gpui::test]
fn btw_stop_with_nothing_streamed_keeps_only_the_question(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        hold: true,
        ..TextScript::reply("Never seen.")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| side.btw_stop("s1", &turn, "t1", cx));
    cx.run_until_parked();
    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.status, BtwThreadStatus::Ready);
    assert_eq!(thread.messages.len(), 1);
}

#[gpui::test]
fn btw_delete_cancels_and_removes_the_thread(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        hold: true,
        ..TextScript::reply("Never seen.")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| side.btw_delete("s1", &turn, "t1", cx));
    cx.run_until_parked();

    let block = session("s1", cx).blocks.remove(0);
    assert_eq!(block.btw_threads, None);
    assert!(!fixture.side.is_requesting("s1", "t1"));
}

#[gpui::test]
fn btw_model_change_applies_to_the_next_question(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();

    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    let mut settings = ModelSettings::new();
    settings.insert("effort".into(), "high".into());
    cx.update(|cx| side.btw_set_model("s1", &turn, "t1", "  claude:opus  ", settings.clone(), cx));
    let thread = threads("s1", cx).remove(0);
    assert_eq!(thread.model.as_deref(), Some("claude:opus"));
    assert_eq!(thread.model_settings.as_ref(), Some(&settings));

    // A blank model changes nothing.
    cx.update(|cx| side.btw_set_model("s1", &turn, "t1", "  ", ModelSettings::new(), cx));
    assert_eq!(threads("s1", cx)[0].model.as_deref(), Some("claude:opus"));

    assert!(ask(&fixture, "t1", "And now?", cx));
    cx.run_until_parked();
    let prompt = fixture.claude.prompts().remove(1);
    assert_eq!(prompt.model_settings.as_ref(), Some(&settings));
}

#[gpui::test]
fn btw_requests_stop_when_their_session_closes(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        hold: true,
        ..TextScript::reply("Never seen.")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    assert!(fixture.side.is_requesting("s1", "t1"));

    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.remove("s1", cx);
        });
    });
    cx.run_until_parked();
    assert!(!fixture.side.is_requesting("s1", "t1"));
}

#[gpui::test]
fn stop_all_aborts_requests_and_stops_text_runners(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        hold: true,
        ..TextScript::reply("Never seen.")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();

    cx.update(|cx| Engine::hooks(cx).side_threads.stop_all(cx));
    cx.run_until_parked();
    assert!(!fixture.side.is_requesting("s1", "t1"));
    assert!(fixture.claude.stops.load(Ordering::SeqCst) >= 1);
    // The aborted request leaves the thread for the user to stop or delete.
    assert_eq!(threads("s1", cx)[0].status, BtwThreadStatus::Running);
}

#[gpui::test]
fn quitting_the_app_stops_side_threads(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    fixture.claude.push(TextScript {
        hold: true,
        ..TextScript::reply("Never seen.")
    });
    assert!(ask(&fixture, "t1", "Why?", cx));
    cx.run_until_parked();
    assert_eq!(fixture.claude.stops.load(Ordering::SeqCst), 0);

    cx.quit();
    assert!(!fixture.side.is_requesting("s1", "t1"));
    assert_eq!(fixture.claude.stops.load(Ordering::SeqCst), 1);
}

#[gpui::test]
fn dismisses_the_handoff_card(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    let id = cx
        .update(|cx| side.handoff("s1", &target(HarnessId::Codex, "gpt-5"), &turn, cx))
        .unwrap();
    assert!(session(&id, cx).handoff_card.is_some());
    cx.update(|cx| side.dismiss_handoff_card(&id, cx));
    assert!(session(&id, cx).handoff_card.is_none());
}

#[gpui::test]
fn continuing_a_session_dismisses_its_notices(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut source = chat("s1");
    source.linked_work_item_update_card = Some(
        serde_json::from_value(json!({
            "kind": "pr", "repo": "acme/app", "number": 7, "title": "Fix", "url": "https://x",
            "state": "open", "since": 1, "updatedAt": 42, "status": "ready",
            "counts": { "comments": 1, "reviews": 0, "commits": 0 },
            "entries": [], "truncated": false
        }))
        .unwrap(),
    );
    insert(source, cx);
    insert(chat("s2"), cx);
    let side = fixture.side.clone();

    cx.update(|cx| side.dismiss_notices_for_continued_session("s1", cx));
    assert!(session("s1", cx).linked_work_item_update_card.is_none());
    assert_eq!(*fixture.peers.seen.borrow(), [("s1".to_string(), 42)]);

    cx.update(|cx| side.dismiss_notices_for_continued_session("s2", cx));
    assert_eq!(*fixture.peers.reminders.borrow(), ["s1", "s2"]);
    assert_eq!(fixture.peers.seen.borrow().len(), 1);
}

struct Attention {
    enabled: bool,
    unseen: HashSet<String>,
}

impl AttentionHooks for Attention {
    fn unseen_finished_ids(&self, _cx: &App) -> HashSet<String> {
        self.unseen.clone()
    }

    fn live_agents_enabled(&self, _cx: &App) -> bool {
        self.enabled
    }
}

fn live_agent_ids(enabled: bool, cx: &mut TestAppContext) -> Vec<String> {
    let backend = init_test_engine_with(
        cx,
        EngineHooks {
            attention: Rc::new(Attention {
                enabled,
                unseen: HashSet::from(["done".to_string()]),
            }),
            ..EngineHooks::default()
        },
    );
    let _fixture = install(cx, backend);
    let mut busy = chat("busy");
    busy.busy = Some(true);
    insert(busy, cx);
    insert(chat("done"), cx);
    insert(chat("idle"), cx);
    cx.update(|cx| live_agents(cx).into_iter().map(|agent| agent.id).collect())
}

#[gpui::test]
fn lists_live_agents_when_the_setting_is_on(cx: &mut TestAppContext) {
    assert_eq!(live_agent_ids(true, cx), ["busy", "done"]);
}

#[gpui::test]
fn lists_no_live_agents_when_the_setting_is_off(cx: &mut TestAppContext) {
    assert!(live_agent_ids(false, cx).is_empty());
}

#[gpui::test]
fn second_opinion_cards_are_not_handoffs(cx: &mut TestAppContext) {
    // The split-pane handoff card carries kind "handoff"; the review does not.
    let fixture = setup(cx);
    insert(chat("s1"), cx);
    let turn = turn("s1", cx);
    let side = fixture.side.clone();
    cx.update(|cx| side.second_opinion("s1", &target(HarnessId::Claude, "claude:opus"), &turn, cx));
    let submits = fixture.peers.submits.borrow();
    let card = submits[0].2.second_opinion.as_ref().unwrap();
    assert_ne!(card.kind, Some(SecondOpinionKind::Handoff));
    assert_eq!(card.to, HarnessId::Claude);
}
