//! Entity tests for the submit pipeline, driven through a fake harness
//! adapter and the runtime's `FakeBackend`. They follow the App.tsx
//! behavior of `submitSession`, `onStop`, `onCompactContext`,
//! `onModelChange`, `onSaveDraft`, `onRemoveDraft`, `onUpdatePlan`, and
//! `onBuildPlan`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{App, Entity, Task, TestAppContext};
use monocode_core::block::{Block, BlockRole, PlanBlockMeta, PlanStatus, TurnIntent};
use monocode_core::handoff::HandoffComposerCard;
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, RewindLastTurnInput, RewindLastTurnResult,
    SendTurnInput, SteerTurnInput,
};
use monocode_core::session::{
    EditedResendRejection, MessageQueueStatus, PendingHarnessSwitch, QueuedMessage,
};
use monocode_core::settings::FollowUpBehavior;
use monocode_core::{Extra, HarnessEvent, HarnessId, ModelSettings, Session};
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::core::native_commands::NativeCommandProvider;
use monocode_harness::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, HarnessAdapter, HarnessRegistry, RegistryOptions,
    TitleInput,
};
use monocode_harness::core::session_title::GeneratedSessionTitle;
use monocode_harness::core::task::SharedSpawner;
use monocode_process::skills::DiscoveredSkill;
use monocode_settings::Kv;
use parking_lot::Mutex;

use super::{Submit, SubmitConfig, SubmitOptions};
use crate::runtime::engine::Engine;
use crate::runtime::testing::{FakeBackend, init_test_engine};
use crate::submit::acceptance::{ControlOutcome, ControlStatus, OnSettled, ProjectLocationSync};
use crate::submit::hooks::{
    SubmitAttentionHooks, SubmitOrchestrationHooks, SubmitProjectsHooks, WorktreeInfo,
};
use crate::submit::skills::SkillSources;

const CWD: &str = "/repo";

/// One scripted reply to `send_turn`.
#[derive(Clone)]
struct Script {
    events: Vec<HarnessEvent>,
    result: Result<(), String>,
    /// Wait for `cancel_turn` before resolving.
    hold: bool,
}

impl Script {
    fn reply(text: &str) -> Self {
        Self {
            events: vec![
                HarnessEvent::MessageDelta { text: text.into() },
                HarnessEvent::MessageCompleted,
            ],
            result: Ok(()),
            hold: false,
        }
    }
}

#[derive(Default)]
struct Calls {
    sends: Vec<SendTurnInput>,
    steers: Vec<SteerTurnInput>,
    cancels: Vec<String>,
    stops: Vec<String>,
    forgets: Vec<String>,
    rewinds: Vec<RewindLastTurnInput>,
    compacts: usize,
    titles: Vec<TitleInput>,
}

struct FakeAdapter {
    id: HarnessId,
    live: bool,
    steer: bool,
    capabilities: AdapterCapabilities,
    scripts: Mutex<VecDeque<Script>>,
    calls: Mutex<Calls>,
    release: (async_channel::Sender<()>, async_channel::Receiver<()>),
    title: Option<String>,
}

impl FakeAdapter {
    fn new(id: HarnessId) -> Arc<Self> {
        Arc::new(Self {
            id,
            live: true,
            steer: true,
            capabilities: AdapterCapabilities {
                compact_context: true,
                rewind_last_turn: true,
                generate_title: true,
                ..AdapterCapabilities::default()
            },
            scripts: Mutex::default(),
            calls: Mutex::default(),
            release: async_channel::unbounded(),
            title: Some("Generated title".into()),
        })
    }

    fn push(&self, script: Script) {
        self.scripts.lock().push_back(script);
    }
}

impl HarnessAdapter for FakeAdapter {
    fn id(&self) -> HarnessId {
        self.id
    }

    fn live(&self) -> bool {
        self.live
    }

    fn can_steer(&self) -> bool {
        self.steer
    }

    fn capabilities(&self) -> AdapterCapabilities {
        self.capabilities
    }

    fn commands(&self) -> Option<Arc<dyn NativeCommandProvider>> {
        None
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        self.calls.lock().sends.push(input);
        let script = self
            .scripts
            .lock()
            .pop_front()
            .unwrap_or_else(|| Script::reply("done"));
        let release = self.release.1.clone();
        async move {
            if let Some(accepted) = on_accepted {
                accepted();
            }
            for event in script.events {
                on_event(event);
            }
            if script.hold {
                let _ = release.recv().await;
            }
            script.result.map_err(|error| anyhow!(error))
        }
        .boxed()
    }

    fn compact_context(
        &self,
        _input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        self.calls.lock().compacts += 1;
        on_event(HarnessEvent::Status {
            text: "Summarizing".into(),
        });
        async { Ok(()) }.boxed()
    }

    fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        _on_event: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        self.calls.lock().rewinds.push(input);
        async { Ok(RewindLastTurnResult { submitted: false }) }.boxed()
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        self.calls.lock().steers.push(input);
        async { Ok(()) }.boxed()
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        self.calls.lock().cancels.push(session_id);
        let _ = self.release.0.try_send(());
        async { Ok(()) }.boxed()
    }

    fn respond_approval(&self, _session_id: &str, _request_id: i64, _decision: ApprovalDecision) {}

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        self.calls.lock().stops.push(session_id);
        async { Ok(()) }.boxed()
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        self.calls.lock().forgets.push(session_id);
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

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        self.calls.lock().titles.push(input);
        let title = self.title.clone().map(|title| GeneratedSessionTitle {
            title,
            work_item: None,
        });
        async move { Ok(title) }.boxed()
    }
}

struct NoSkills;

impl SkillSources for NoSkills {
    fn command_provider(&self, _harness: HarnessId) -> Option<Arc<dyn NativeCommandProvider>> {
        None
    }

    fn list_skills(
        &self,
        _cwd: String,
        _disabled: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredSkill>, String>> {
        async { Ok(Vec::new()) }.boxed()
    }

    fn read_text_file(&self, _path: String) -> BoxFuture<'static, Result<String, String>> {
        async { Ok(String::new()) }.boxed()
    }

    fn home_dir(&self) -> BoxFuture<'static, Result<String, String>> {
        async { Ok("/home".to_string()) }.boxed()
    }

    fn create_path(
        &self,
        _parent: String,
        _name: String,
        _is_dir: bool,
    ) -> BoxFuture<'static, Result<String, String>> {
        async { Ok(String::new()) }.boxed()
    }

    fn write_text_file(
        &self,
        _path: String,
        _content: String,
    ) -> BoxFuture<'static, Result<(), String>> {
        async { Ok(()) }.boxed()
    }
}

struct Fixture {
    submit: Entity<Submit>,
    codex: Arc<FakeAdapter>,
    fx: Arc<FakeAdapter>,
    backend: Arc<FakeBackend>,
    kv: Kv,
}

fn setup(cx: &mut TestAppContext) -> Fixture {
    let backend = init_test_engine(cx);
    let executor = cx.executor();
    let spawner: SharedSpawner =
        Arc::new(move |future: BoxFuture<'static, ()>| executor.spawn(future).detach());
    let registry = HarnessRegistry::new(
        spawner.clone(),
        RegistryOptions {
            turn_control: None,
            idle_park: Duration::from_secs(86_400),
        },
    );
    let codex = FakeAdapter::new(HarnessId::Codex);
    let fx = Arc::new(FakeAdapter {
        steer: false,
        capabilities: AdapterCapabilities::default(),
        ..Arc::into_inner(FakeAdapter::new(HarnessId::Fx)).unwrap()
    });
    registry.register_harness(codex.clone());
    registry.register_harness(fx.clone());
    let kv = Kv::in_memory();
    let mut config = SubmitConfig::new(registry, SharedCatalog::new(), kv.clone(), spawner);
    config.skill_sources = Arc::new(NoSkills);
    config.app_cli_path =
        Arc::new(|| Ok("/Applications/MonoCode.app/Contents/MacOS/monocode".into()));
    let submit = cx.update(|cx| Submit::init(config, cx));
    Fixture {
        submit,
        codex,
        fx,
        backend,
        kv,
    }
}

fn chat(id: &str, harness: HarnessId) -> Session {
    Session::blank(id, harness, format!("{harness}:default"), CWD)
}

fn insert(session: Session, cx: &mut TestAppContext) {
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
    });
}

fn session(id: &str, cx: &mut TestAppContext) -> Session {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned().unwrap())
}

fn texts(session: &Session) -> Vec<(BlockRole, String)> {
    session
        .blocks
        .iter()
        .map(|block| (block.role, block.text.clone()))
        .collect()
}

fn submit(
    fixture: &Fixture,
    id: &str,
    text: &str,
    options: SubmitOptions,
    cx: &mut TestAppContext,
) -> bool {
    let accepted = cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.on_submit(id, text, Vec::new(), options, cx)
        })
    });
    cx.run_until_parked();
    accepted
}

fn recorder() -> (Rc<RefCell<Vec<ControlOutcome>>>, OnSettled) {
    let outcomes: Rc<RefCell<Vec<ControlOutcome>>> = Rc::default();
    let sink = outcomes.clone();
    (
        outcomes,
        Rc::new(move |outcome: ControlOutcome, _: &mut App| sink.borrow_mut().push(outcome)),
    )
}

#[gpui::test]
async fn sends_a_turn_appends_the_reply_and_settles_completed(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    let (outcomes, on_settled) = recorder();
    let accepted = submit(
        &fixture,
        "s",
        "hello",
        SubmitOptions {
            on_settled: Some(on_settled),
            ..SubmitOptions::default()
        },
        cx,
    );
    assert!(accepted);
    let session = session("s", cx);
    assert_eq!(
        texts(&session),
        [
            (BlockRole::User, "hello".to_string()),
            (BlockRole::Assistant, "done".to_string())
        ]
    );
    assert_eq!(session.busy, Some(false));
    assert_eq!(session.title, "codex · Generated title");
    let sends = &fixture.codex.calls.lock().sends;
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].text, "hello");
    assert_eq!(sends[0].session.cwd, CWD);
    assert_eq!(sends[0].session.intent, Some(TurnIntent::Default));
    assert_eq!(sends[0].session.app_access, Some(false));
    assert_eq!(
        *outcomes.borrow(),
        [ControlOutcome {
            status: ControlStatus::Completed,
            text: "done".into(),
            error: None
        }]
    );
    assert!(
        fixture
            .backend
            .commands()
            .iter()
            .any(|command| command.contains("ensure"))
    );
}

#[gpui::test]
async fn refuses_empty_messages_and_removed_worktrees(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    assert!(!submit(&fixture, "s", "   ", SubmitOptions::default(), cx));
    insert(
        Session {
            worktree_removed: Some(true),
            ..chat("gone", HarnessId::Codex)
        },
        cx,
    );
    assert!(!submit(
        &fixture,
        "gone",
        "hello",
        SubmitOptions::default(),
        cx
    ));
    assert!(!submit(
        &fixture,
        "missing",
        "hello",
        SubmitOptions::default(),
        cx
    ));
    assert!(fixture.codex.calls.lock().sends.is_empty());
}

#[gpui::test]
async fn a_failed_turn_reports_the_error_and_parks_the_provider(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    fixture.codex.push(Script {
        events: vec![],
        result: Err("boom".into()),
        hold: false,
    });
    let (outcomes, on_settled) = recorder();
    submit(
        &fixture,
        "s",
        "hello",
        SubmitOptions {
            on_settled: Some(on_settled),
            ..SubmitOptions::default()
        },
        cx,
    );
    let session = session("s", cx);
    assert!(
        session
            .blocks
            .iter()
            .any(|block| block.role == BlockRole::System && block.text == "boom")
    );
    assert_eq!(session.busy, Some(false));
    assert_eq!(fixture.codex.calls.lock().stops, ["s"]);
    let outcome = &outcomes.borrow()[0];
    assert_eq!(outcome.status, ControlStatus::Failed);
    assert_eq!(outcome.error.as_deref(), Some("boom"));
}

#[gpui::test]
async fn queues_a_follow_up_while_a_turn_runs(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(
        Session {
            busy: Some(true),
            ..chat("s", HarnessId::Codex)
        },
        cx,
    );
    let accepted = submit(
        &fixture,
        "s",
        "next",
        SubmitOptions {
            follow_up_behavior: Some(FollowUpBehavior::Queue),
            ..SubmitOptions::default()
        },
        cx,
    );
    assert!(accepted);
    let session = session("s", cx);
    let queued = session.queued_messages.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].text, "next");
    assert_eq!(session.queue_status, Some(MessageQueueStatus::Active));
    assert!(fixture.codex.calls.lock().sends.is_empty());
}

#[gpui::test]
async fn steers_a_follow_up_into_the_running_turn(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(
        Session {
            busy: Some(true),
            ..chat("s", HarnessId::Codex)
        },
        cx,
    );
    assert!(submit(
        &fixture,
        "s",
        "also check tests",
        SubmitOptions::default(),
        cx
    ));
    let session = session("s", cx);
    assert_eq!(
        texts(&session),
        [(BlockRole::User, "also check tests".to_string())]
    );
    assert_eq!(session.busy, Some(true));
    let steers = &fixture.codex.calls.lock().steers;
    assert_eq!(steers.len(), 1);
    assert_eq!(steers[0].text, "also check tests");
}

#[gpui::test]
async fn says_so_when_a_harness_cannot_take_a_follow_up(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(
        Session {
            busy: Some(true),
            ..chat("s", HarnessId::Fx)
        },
        cx,
    );
    assert!(!submit(&fixture, "s", "more", SubmitOptions::default(), cx));
    let session = session("s", cx);
    assert!(
        session
            .blocks
            .iter()
            .any(|block| block.text.contains("fx cannot take a follow-up mid-turn"))
    );
    assert!(fixture.fx.calls.lock().steers.is_empty());
}

#[gpui::test]
async fn shows_a_notice_when_the_harness_is_not_connected(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Claude), cx);
    let (outcomes, on_settled) = recorder();
    assert!(submit(
        &fixture,
        "s",
        "hello",
        SubmitOptions {
            on_settled: Some(on_settled),
            ..SubmitOptions::default()
        },
        cx,
    ));
    let session = session("s", cx);
    assert_eq!(session.busy, Some(false));
    assert_eq!(session.blocks[0].text, "hello");
    assert!(
        session.blocks[1]
            .text
            .contains("claude is not connected yet")
    );
    assert_eq!(
        outcomes.borrow()[0].error.as_deref(),
        Some("Harness is not connected")
    );
}

#[gpui::test]
async fn operator_turns_get_app_access_and_cli_instructions(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    submit(
        &fixture,
        "s",
        "/operator list my notes",
        SubmitOptions::default(),
        cx,
    );
    let session = session("s", cx);
    assert_eq!(session.blocks[0].text, "list my notes");
    assert_eq!(session.blocks[0].monocode, Some(true));
    let sends = &fixture.codex.calls.lock().sends;
    assert!(sends[0].text.starts_with("list my notes\n\n<monocode_app>"));
    assert!(
        sends[0]
            .text
            .contains("Run `/Applications/MonoCode.app/Contents/MacOS/monocode app --help`")
    );
    assert_eq!(sends[0].session.app_access, Some(true));
    assert_eq!(sends[0].session.controls_agents, Some(true));
}

#[gpui::test]
async fn promotes_a_saved_draft_and_sends_it(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    let saved = cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.save_draft("s", "later", Vec::new(), None, cx)
        })
    });
    assert!(saved);
    let draft = session("s", cx).blocks[0].clone();
    assert!(draft.is_draft());
    assert_eq!(session("s", cx).title, "codex · later");
    submit(
        &fixture,
        "s",
        "later",
        SubmitOptions {
            draft_block_id: Some(draft.id.clone()),
            ..SubmitOptions::default()
        },
        cx,
    );
    let session = session("s", cx);
    assert!(session.blocks.iter().all(|block| !block.is_draft()));
    assert_eq!(texts(&session)[0], (BlockRole::User, "later".to_string()));
}

#[gpui::test]
async fn removes_a_draft_and_discards_a_draft_only_session(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.save_draft("s", "later", Vec::new(), None, cx)
        })
    });
    let draft_id = session("s", cx).blocks[0].id.clone();
    cx.update(|cx| {
        let sessions = Engine::sessions(cx);
        assert_eq!(sessions.read(cx).pending_persist(), &["s"]);
    });
    let removed = cx.update(|cx| {
        fixture
            .submit
            .update(cx, |submit, cx| submit.remove_draft("s", &draft_id, cx))
    });
    cx.update(|cx| {
        let sessions = Engine::sessions(cx);
        assert!(sessions.read(cx).pending_persist().is_empty());
        assert_eq!(sessions.read(cx).last_persisted("s"), None);
    });
    cx.run_until_parked();
    assert!(removed);
    assert!(session("s", cx).blocks.is_empty());
    assert!(
        fixture
            .backend
            .commands()
            .iter()
            .any(|command| command.contains("delete"))
    );
    cx.executor()
        .advance_clock(crate::runtime::sessions::PERSIST_DEBOUNCE);
    assert!(fixture.backend.record("s").is_none());
}

#[gpui::test]
async fn stop_cancels_the_turn_and_pauses_the_queue(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    fixture.codex.push(Script {
        events: vec![HarnessEvent::MessageDelta {
            text: "working".into(),
        }],
        result: Ok(()),
        hold: true,
    });
    let (outcomes, on_settled) = recorder();
    submit(
        &fixture,
        "s",
        "hello",
        SubmitOptions {
            on_settled: Some(on_settled),
            ..SubmitOptions::default()
        },
        cx,
    );
    assert_eq!(session("s", cx).busy, Some(true));
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update("s", cx, |session| {
                session.queued_messages = Some(vec![QueuedMessage {
                    id: "q".into(),
                    text: "next".into(),
                    attachments: vec![],
                    note_card: None,
                    handoff_card: None,
                    intent: None,
                }]);
            });
        });
        fixture
            .submit
            .update(cx, |submit, cx| submit.stop("s", false, cx));
    });
    cx.run_until_parked();
    let session = session("s", cx);
    assert_eq!(session.busy, Some(false));
    assert_eq!(session.queue_status, Some(MessageQueueStatus::Paused));
    assert_eq!(fixture.codex.calls.lock().cancels, ["s"]);
    assert_eq!(outcomes.borrow()[0].status, ControlStatus::Cancelled);
}

#[gpui::test]
async fn compacts_context_or_says_the_provider_cannot(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    insert(chat("f", HarnessId::Fx), cx);
    let handled = cx.update(|cx| {
        fixture
            .submit
            .update(cx, |submit, cx| submit.compact("f", cx))
    });
    assert!(handled);
    assert!(
        session("f", cx).blocks[0]
            .text
            .contains("does not support manual context compaction")
    );

    let started = cx.update(|cx| {
        fixture
            .submit
            .update(cx, |submit, cx| submit.compact("s", cx))
    });
    assert!(started);
    assert_eq!(session("s", cx).busy, Some(true));
    cx.run_until_parked();
    let session = session("s", cx);
    assert_eq!(session.busy, Some(false));
    let lines: Vec<&str> = session
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect();
    assert_eq!(
        lines,
        ["Compacting context…", "Summarizing", "Compacted context"]
    );
    assert_eq!(fixture.codex.calls.lock().compacts, 1);
}

#[gpui::test]
async fn switching_providers_arms_a_handoff_that_the_next_send_delivers(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut started = chat("s", HarnessId::Fx);
    started.blocks = vec![
        Block::new("u1", BlockRole::User, "fix the footer"),
        Block::new("a1", BlockRole::Assistant, "Patched UsageFooter."),
    ];
    insert(started, cx);
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.set_model("s", HarnessId::Codex, "codex:default", cx)
        })
    });
    let armed = session("s", cx);
    assert_eq!(armed.harness, HarnessId::Codex);
    assert_eq!(
        armed.pending_switch.as_ref().map(|pending| pending.from),
        Some(HarnessId::Fx)
    );

    submit(
        &fixture,
        "s",
        "now the header",
        SubmitOptions::default(),
        cx,
    );
    let session = session("s", cx);
    assert_eq!(session.pending_switch, None);
    let handoff = session
        .blocks
        .iter()
        .find(|block| block.role == BlockRole::Handoff)
        .unwrap();
    assert_eq!(handoff.handoff.as_ref().unwrap().pending, Some(false));
    assert!(handoff.text.contains("fix the footer"));
    assert_eq!(fixture.fx.calls.lock().forgets, ["s"]);
    let sent = &fixture.codex.calls.lock().sends[0].text;
    assert!(sent.starts_with("You are continuing an existing conversation handed off from fx."));
    assert!(sent.contains("now the header"));
    assert!(sent.contains("<handoff>"));
}

#[gpui::test]
async fn set_model_on_an_empty_session_forgets_the_old_provider(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Fx), cx);
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.set_model("s", HarnessId::Codex, "codex:default", cx)
        })
    });
    cx.run_until_parked();
    let session = session("s", cx);
    assert_eq!(session.harness, HarnessId::Codex);
    assert_eq!(session.pending_switch, None);
    assert_eq!(session.title, "codex");
    assert_eq!(fixture.fx.calls.lock().forgets, ["s"]);
    assert!(
        fixture
            .kv
            .get_item("monocode.recentModels")
            .unwrap()
            .contains("codex")
    );
}

#[gpui::test]
async fn plan_turns_key_native_plans_and_promote_a_plan_reply(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(chat("s", HarnessId::Codex), cx);
    fixture
        .codex
        .push(Script::reply("# Plan\n\n1. Read the code\n2. Fix it"));
    submit(
        &fixture,
        "s",
        "plan the fix",
        SubmitOptions {
            intent: Some(TurnIntent::Plan),
            ..SubmitOptions::default()
        },
        cx,
    );
    let session = session("s", cx);
    assert_eq!(session.blocks[0].intent, Some(TurnIntent::Plan));
    let plan = session
        .blocks
        .iter()
        .find(|block| block.role == BlockRole::Plan)
        .unwrap();
    assert!(
        plan.plan
            .as_ref()
            .unwrap()
            .key
            .as_deref()
            .unwrap()
            .starts_with("turn:")
    );
    assert_ne!(fixture.codex.calls.lock().sends[0].text, "plan the fix");
}

#[gpui::test]
async fn edits_and_builds_an_approved_plan(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut planned = chat("s", HarnessId::Codex);
    planned.blocks = vec![
        Block::new("u1", BlockRole::User, "plan it"),
        Block {
            plan: Some(PlanBlockMeta {
                key: None,
                status: PlanStatus::Ready,
                original_text: None,
                approved_text: None,
                edited: None,
                extra: Extra::new(),
            }),
            ..Block::new("p1", BlockRole::Plan, "1. One\n2. Two")
        },
    ];
    insert(planned, cx);
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.update_plan("s", "p1", "1. One\n2. Two\n3. Three", cx)
        })
    });
    let edited = session("s", cx);
    let meta = edited.blocks[1].plan.clone().unwrap();
    assert_eq!(meta.edited, Some(true));
    assert_eq!(meta.original_text.as_deref(), Some("1. One\n2. Two"));

    cx.update(|cx| {
        fixture
            .submit
            .update(cx, |submit, cx| submit.build_plan("s", "p1", None, cx))
    });
    cx.run_until_parked();
    let built = session("s", cx);
    let meta = built.blocks[1].plan.clone().unwrap();
    assert_eq!(meta.status, PlanStatus::Built);
    assert_eq!(
        meta.approved_text.as_deref(),
        Some("1. One\n2. Two\n3. Three")
    );
    assert_eq!(built.blocks[2].text, "Build approved plan");
    assert!(
        fixture.codex.calls.lock().sends[0]
            .text
            .contains("3. Three")
    );
}

#[gpui::test]
async fn an_edited_resend_rewinds_the_provider_then_replaces_the_turn(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let mut sent = chat("s", HarnessId::Codex);
    sent.blocks = vec![
        Block {
            provider_turn_id: Some("t1".into()),
            ..Block::new("u1", BlockRole::User, "first try")
        },
        Block::new("a1", BlockRole::Assistant, "old answer"),
    ];
    insert(sent, cx);
    let rejected: Rc<RefCell<Vec<EditedResendRejection>>> = Rc::default();
    let sink = rejected.clone();
    submit(
        &fixture,
        "s",
        "second try",
        SubmitOptions {
            resend_edited: true,
            on_resend_rejected: Some(Rc::new(move |rejection, _| {
                sink.borrow_mut().push(rejection)
            })),
            ..SubmitOptions::default()
        },
        cx,
    );
    let session = session("s", cx);
    assert_eq!(
        texts(&session),
        [
            (BlockRole::User, "second try".to_string()),
            (BlockRole::Assistant, "done".to_string())
        ]
    );
    let calls = fixture.codex.calls.lock();
    assert_eq!(calls.rewinds[0].provider_turn_id.as_deref(), Some("t1"));
    assert_eq!(calls.sends[0].text, "second try");
    assert!(rejected.borrow().is_empty());
}

struct MovedProject {
    applied: RefCell<Vec<(String, String)>>,
}

impl SubmitProjectsHooks for MovedProject {
    fn synchronize_project_location(
        &self,
        _cwd: &str,
        _cx: &mut App,
    ) -> Task<Result<Option<ProjectLocationSync>, String>> {
        Task::ready(Ok(Some(ProjectLocationSync {
            path: "/moved".into(),
            identity: "repo".into(),
            moved: true,
        })))
    }

    fn apply_project_location_change(
        &self,
        from: &str,
        to: &str,
        _cx: &mut App,
    ) -> Task<Result<(), String>> {
        self.applied
            .borrow_mut()
            .push((from.to_string(), to.to_string()));
        Task::ready(Ok(()))
    }

    fn create_worktree(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        _existing: bool,
        _cx: &mut App,
    ) -> Task<Result<WorktreeInfo, String>> {
        Task::ready(Ok(WorktreeInfo {
            path: format!("{cwd}/.worktrees/{}", branch.replace('/', "-")),
            branch: Some(format!("{branch}@{base}")),
        }))
    }
}

#[gpui::test]
async fn waits_for_a_moved_project_then_submits(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let projects = Rc::new(MovedProject {
        applied: RefCell::default(),
    });
    let hook = projects.clone();
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, _| {
            submit.set_peers(|peers| peers.projects = hook)
        })
    });
    insert(chat("s", HarnessId::Codex), cx);
    let acceptance = cx.update(|cx| {
        fixture.submit.update(cx, |submit, cx| {
            submit.submit("s", "hello", Vec::new(), SubmitOptions::default(), cx)
        })
    });
    assert!(matches!(
        acceptance,
        crate::submit::SubmissionAcceptance::Deferred(_)
    ));
    cx.run_until_parked();
    assert_eq!(acceptance.resolve().await, Ok(true));
    assert_eq!(
        *projects.applied.borrow(),
        [("/repo".to_string(), "/moved".to_string())]
    );
    assert_eq!(fixture.codex.calls.lock().sends.len(), 1);
}

#[gpui::test]
async fn creates_the_selected_worktree_on_the_first_send(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let hook = Rc::new(MovedProject {
        applied: RefCell::default(),
    });
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, _| {
            submit.set_peers(|peers| peers.projects = hook)
        })
    });
    insert(
        Session {
            workspace_mode: Some(monocode_core::session::WorkspaceMode::Worktree),
            ..chat("s", HarnessId::Codex)
        },
        cx,
    );
    submit(
        &fixture,
        "s",
        "hello",
        SubmitOptions {
            project_location_ready: true,
            ..SubmitOptions::default()
        },
        cx,
    );
    let session = session("s", cx);
    let worktree = session.worktree_cwd.clone().unwrap();
    assert!(worktree.starts_with("/repo/.worktrees/mc-"));
    assert_eq!(session.worktree_preparing, None);
    assert_eq!(session.workspace_mode, None);
    assert_eq!(fixture.codex.calls.lock().sends[0].session.cwd, worktree);
}

struct Lead;

#[cfg(feature = "orchestration")]
#[gpui::test]
async fn missing_worker_models_fail_before_sending_a_planning_turn(cx: &mut TestAppContext) {
    use crate::orchestration::testing::FakeStore;
    use crate::orchestration::{NoPeers, Orchestration, OrchestrationConfig};

    const ERROR: &str = "No worker models are available, or the model catalog is too large. Check your connected harnesses.";

    let fixture = setup(cx);
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, _| {
            submit.config.is_harness_available = Arc::new(|_| false);
        });
        Orchestration::init(
            OrchestrationConfig {
                storage: FakeStore::new(),
                control: None,
                harness_host: None,
                owner: "planning-qualification".into(),
                kv: fixture.kv.clone(),
                peers: Rc::new(NoPeers),
            },
            cx,
        );
    });
    insert(chat("s", HarnessId::Codex), cx);
    let (outcomes, on_settled) = recorder();
    assert!(submit(
        &fixture,
        "s",
        "Prepare the work",
        SubmitOptions {
            intent: Some(TurnIntent::Orchestrate),
            on_settled: Some(on_settled),
            ..SubmitOptions::default()
        },
        cx,
    ));
    assert!(
        fixture.codex.calls.lock().sends.is_empty(),
        "worker discovery must fail before calling the lead provider"
    );
    assert_eq!(outcomes.borrow().len(), 1);
    assert_eq!(outcomes.borrow()[0].status, ControlStatus::Failed);
    assert_eq!(outcomes.borrow()[0].error.as_deref(), Some(ERROR));
    let current = session("s", cx);
    assert_eq!(current.busy, Some(false));
    let proposal = current
        .blocks
        .iter()
        .find_map(|block| block.orchestration.as_ref())
        .expect("the failed proposal stays visible");
    assert_eq!(
        proposal.status,
        monocode_core::orchestration::OrchestrationProposalStatus::Invalid
    );
    assert_eq!(proposal.error.as_deref(), Some(ERROR));
}

impl SubmitOrchestrationHooks for Lead {
    fn submission_error(&self, session_id: &str, _managed: bool, _cx: &App) -> Option<String> {
        (session_id == "worker").then(|| "This worker is managed by its lead.".to_string())
    }
}

#[gpui::test]
async fn reports_orchestration_refusals_and_managed_unavailability(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, _| {
            submit.set_peers(|peers| peers.orchestration = Rc::new(Lead))
        })
    });
    insert(chat("worker", HarnessId::Codex), cx);
    assert!(!submit(
        &fixture,
        "worker",
        "go",
        SubmitOptions::default(),
        cx
    ));
    assert_eq!(
        session("worker", cx).blocks[0].text,
        "This worker is managed by its lead."
    );

    insert(
        Session {
            busy: Some(true),
            ..chat("busy", HarnessId::Codex)
        },
        cx,
    );
    let (outcomes, on_settled) = recorder();
    assert!(!submit(
        &fixture,
        "busy",
        "go",
        SubmitOptions {
            managed: true,
            on_settled: Some(on_settled),
            ..SubmitOptions::default()
        },
        cx,
    ));
    assert_eq!(
        *outcomes.borrow(),
        [ControlOutcome::failed(
            "Session is unavailable or already running"
        )]
    );
}

#[derive(Default)]
struct Notices {
    dismissed: RefCell<Vec<String>>,
    finished: RefCell<Vec<String>>,
}

impl SubmitAttentionHooks for Notices {
    fn dismiss_notices_for_continued_session(&self, session_id: &str, _cx: &mut App) {
        self.dismissed.borrow_mut().push(session_id.to_string());
    }

    fn announce_finished_later(&self, session_id: &str, _cx: &mut App) {
        self.finished.borrow_mut().push(session_id.to_string());
    }
}

#[gpui::test]
async fn dismisses_notices_and_announces_the_finished_turn(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    let notices = Rc::new(Notices::default());
    let hook = notices.clone();
    cx.update(|cx| {
        fixture.submit.update(cx, |submit, _| {
            submit.set_peers(|peers| peers.attention = hook)
        })
    });
    insert(chat("s", HarnessId::Codex), cx);
    submit(&fixture, "s", "hello", SubmitOptions::default(), cx);
    assert_eq!(*notices.dismissed.borrow(), ["s"]);
    assert_eq!(*notices.finished.borrow(), ["s"]);
}

#[gpui::test]
async fn sends_a_handoff_card_brief_with_the_message(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(
        Session {
            handoff_card: Some(HandoffComposerCard {
                from: HarnessId::Claude,
                to: HarnessId::Codex,
                brief: "## Session so far\nUser: footer".into(),
                request: Some("fix the footer".into()),
                files: Some(1),
            }),
            ..chat("s", HarnessId::Codex)
        },
        cx,
    );
    submit(&fixture, "s", "keep going", SubmitOptions::default(), cx);
    let session = session("s", cx);
    assert_eq!(session.handoff_card, None);
    let card = session.blocks[0].second_opinion.clone().unwrap();
    assert_eq!(card.from, HarnessId::Claude);
    assert_eq!(session.blocks[0].text, "keep going");
    let sent = &fixture.codex.calls.lock().sends[0].text;
    assert!(sent.contains("handed off from Claude Code"));
    assert!(sent.contains("User: footer"));
}

#[gpui::test]
async fn keeps_a_pending_switch_from_the_same_provider_out_of_the_way(cx: &mut TestAppContext) {
    let fixture = setup(cx);
    insert(
        Session {
            pending_switch: Some(PendingHarnessSwitch {
                from: HarnessId::Codex,
                from_model: "codex:default".into(),
                from_settings: ModelSettings::new(),
                from_provider_session_id: None,
                from_provider_account_id: None,
            }),
            ..chat("s", HarnessId::Codex)
        },
        cx,
    );
    submit(&fixture, "s", "hello", SubmitOptions::default(), cx);
    let session = session("s", cx);
    assert!(
        session
            .blocks
            .iter()
            .all(|block| block.role != BlockRole::Handoff)
    );
    assert_eq!(fixture.codex.calls.lock().sends[0].text, "hello");
}
