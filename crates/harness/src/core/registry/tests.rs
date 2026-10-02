//! Ports of registry.test.ts. The support-matrix cases that need every
//! provider live in `register.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::FutureExt;
use parking_lot::Mutex;

use super::*;
use crate::core::catalog::SharedCatalog;
use crate::core::task::SmolSpawner;
use monocode_core::block::{TaskListItem, TaskListItemStatus};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::HarnessSessionInput;
use monocode_core::models::AgentModel;

type Hook<T> = Arc<dyn Fn(T) -> BoxFuture<'static, Result<()>> + Send + Sync>;
type Calls<T> = Arc<Mutex<Vec<T>>>;
type BindCall = (String, String, String, Option<String>);

#[derive(Default, Clone)]
struct Hooks {
    compact: Option<Hook<String>>,
    run_text_prompt: Option<Arc<dyn Fn() -> BoxFuture<'static, Result<String>> + Send + Sync>>,
    stop_text_prompt: Option<Arc<AtomicUsize>>,
    refresh_catalog: Option<Hook<()>>,
    stop_session: Option<Calls<String>>,
    bind_session: Option<Calls<BindCall>>,
    restore_task_lists: Option<Calls<(String, Vec<TaskListMeta>)>>,
}

struct Stub {
    id: HarnessId,
    hooks: Hooks,
}

fn stub(id: HarnessId, hooks: Hooks) -> Arc<dyn HarnessAdapter> {
    Arc::new(Stub { id, hooks })
}

impl HarnessAdapter for Stub {
    fn id(&self) -> HarnessId {
        self.id
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: self.hooks.compact.is_some(),
            run_text_prompt: self.hooks.run_text_prompt.is_some(),
            stop_text_prompt: self.hooks.stop_text_prompt.is_some(),
            refresh_catalog: self.hooks.refresh_catalog.is_some(),
            restore_task_lists: self.hooks.restore_task_lists.is_some(),
            ..Default::default()
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

    fn compact_context(
        &self,
        input: CompactContextInput,
        _on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        match &self.hooks.compact {
            Some(hook) => hook(input.session_id),
            None => unsupported(self.id, "manual compaction"),
        }
    }

    fn steer_turn(&self, _input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn cancel_turn(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn respond_approval(&self, _session_id: &str, _request_id: i64, _decision: ApprovalDecision) {}

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        if let Some(calls) = &self.hooks.stop_session {
            calls.lock().push(session_id);
        }
        async { Ok(()) }.boxed()
    }

    fn forget_session(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed()
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        provider_account_id: Option<&str>,
    ) {
        if let Some(calls) = &self.hooks.bind_session {
            calls.lock().push((
                thread_id.into(),
                provider_session_id.into(),
                cwd.into(),
                provider_account_id.map(str::to_string),
            ));
        }
    }

    fn restore_task_lists(&self, thread_id: &str, lists: Vec<TaskListMeta>) {
        if let Some(calls) = &self.hooks.restore_task_lists {
            calls.lock().push((thread_id.into(), lists));
        }
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        match &self.hooks.refresh_catalog {
            Some(hook) => hook(()),
            None => ok(()),
        }
    }

    fn run_text_prompt(&self, _input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        match &self.hooks.run_text_prompt {
            Some(hook) => hook(),
            None => unsupported(self.id, "isolated text prompts"),
        }
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        if let Some(calls) = &self.hooks.stop_text_prompt {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        ok(())
    }
}

fn registry() -> HarnessRegistry {
    HarnessRegistry::new(Arc::new(SmolSpawner), RegistryOptions::default())
}

fn session_input(session_id: &str, model: &str) -> HarnessSessionInput {
    HarnessSessionInput {
        session_id: session_id.into(),
        cwd: "/tmp".into(),
        model: model.into(),
        model_settings: None,
        provider_account_id: None,
        runtime_mode: RuntimeMode::Supervised,
        intent: None,
        controls_agents: None,
        app_access: None,
    }
}

fn counting_hook(calls: &Arc<AtomicUsize>) -> Hook<()> {
    let calls = calls.clone();
    Arc::new(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }.boxed()
    })
}

#[test]
fn tracks_live_adapters() {
    let registry = registry();
    registry.register_harness(stub(HarnessId::Cursor, Hooks::default()));
    registry.register_harness(stub(HarnessId::Codex, Hooks::default()));
    registry.register_harness(stub(HarnessId::Claude, Hooks::default()));
    assert!(registry.is_live_harness(HarnessId::Cursor));
    assert!(registry.is_live_harness(HarnessId::Codex));
    assert!(registry.is_live_harness(HarnessId::Claude));
    let mut ids: Vec<HarnessId> = registry.list_harnesses().iter().map(|a| a.id()).collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![HarnessId::Claude, HarnessId::Codex, HarnessId::Cursor]
    );
    assert!(
        registry.require_harness(HarnessId::Pi).is_err_and(|error| {
            error.to_string() == "No harness adapter registered for \"pi\""
        })
    );
}

#[test]
fn advertises_and_dispatches_compaction_only_when_an_adapter_supports_it() {
    let registry = registry();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    registry.register_harness(stub(
        HarnessId::Codex,
        Hooks {
            compact: Some(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }.boxed()
            })),
            ..Default::default()
        },
    ));
    registry.register_harness(stub(HarnessId::Claude, Hooks::default()));

    assert!(registry.can_compact_harness_context(HarnessId::Codex));
    assert!(!registry.can_compact_harness_context(HarnessId::Claude));

    smol::block_on(async {
        registry
            .compact_harness_context(
                HarnessId::Codex,
                session_input("compact-1", "codex:gpt-5.4"),
                ignore_events(),
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let error = registry
            .compact_harness_context(
                HarnessId::Claude,
                session_input("compact-2", "claude:sonnet"),
                ignore_events(),
            )
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not support manual compaction")
        );
    });
}

#[test]
fn cancels_an_isolated_text_prompt_through_the_adapter_lifecycle() {
    let registry = registry();
    let runs = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    registry.register_harness(stub(
        HarnessId::Claude,
        Hooks {
            run_text_prompt: Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                futures::future::pending().boxed()
            })),
            stop_text_prompt: Some(stops.clone()),
            ..Default::default()
        },
    ));
    let signal = AbortSignal::new();
    let request = registry.run_harness_text_prompt(
        HarnessId::Claude,
        TextPromptInput {
            cwd: "/tmp".into(),
            prompt: "read-only question".into(),
            signal: Some(signal.clone()),
            ..Default::default()
        },
    );
    signal.abort();
    let error = smol::block_on(request).unwrap_err();
    assert_eq!(error.to_string(), "By-the-way request cancelled");
    // The run started at call time on the spawner; give that task a moment.
    smol::block_on(crate::core::task::timeout(Duration::from_secs(2), async {
        while runs.load(Ordering::SeqCst) == 0 {
            smol::Timer::after(Duration::from_millis(1)).await;
        }
    }));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(stops.load(Ordering::SeqCst), 0);
}

#[test]
fn does_not_stop_the_shared_text_backend_while_another_prompt_is_active() {
    let registry = registry();
    let stops = Arc::new(AtomicUsize::new(0));
    registry.register_harness(stub(
        HarnessId::Claude,
        Hooks {
            run_text_prompt: Some(Arc::new(|| futures::future::pending().boxed())),
            stop_text_prompt: Some(stops.clone()),
            ..Default::default()
        },
    ));
    let first = AbortSignal::new();
    let second = AbortSignal::new();
    let prompt = |prompt: &str, signal: &AbortSignal| TextPromptInput {
        cwd: "/tmp".into(),
        prompt: prompt.into(),
        signal: Some(signal.clone()),
        ..Default::default()
    };
    smol::block_on(async {
        let spawn_prompt = |input: TextPromptInput| {
            let registry = registry.clone();
            smol::spawn(async move {
                registry
                    .run_harness_text_prompt(HarnessId::Claude, input)
                    .await
            })
        };
        let first_request = spawn_prompt(prompt("first", &first));
        let second_request = spawn_prompt(prompt("second", &second));
        first.abort();
        assert_eq!(
            first_request.await.unwrap_err().to_string(),
            "By-the-way request cancelled"
        );
        assert_eq!(stops.load(Ordering::SeqCst), 0);
        second.abort();
        assert_eq!(
            second_request.await.unwrap_err().to_string(),
            "By-the-way request cancelled"
        );
        assert_eq!(stops.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn refreshes_only_the_requested_catalogs() {
    let registry = registry();
    let pi = Arc::new(AtomicUsize::new(0));
    let claude = Arc::new(AtomicUsize::new(0));
    registry.register_harness(stub(
        HarnessId::Pi,
        Hooks {
            refresh_catalog: Some(counting_hook(&pi)),
            ..Default::default()
        },
    ));
    registry.register_harness(stub(
        HarnessId::Claude,
        Hooks {
            refresh_catalog: Some(counting_hook(&claude)),
            ..Default::default()
        },
    ));
    smol::block_on(registry.refresh_harness_catalogs([HarnessId::Claude], false, |_| false));
    assert_eq!(claude.load(Ordering::SeqCst), 1);
    assert_eq!(pi.load(Ordering::SeqCst), 0);
}

#[test]
fn does_not_spawn_a_catalog_probe_twice_after_a_live_list_lands() {
    let registry = registry();
    let catalog = SharedCatalog::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let writer = catalog.clone();
    registry.register_harness(stub(
        HarnessId::Pi,
        Hooks {
            refresh_catalog: Some(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                writer.set_harness_models(
                    HarnessId::Pi,
                    vec![
                        AgentModel::new("pi:opus", HarnessId::Pi, "Opus")
                            .with_native_id("anthropic/opus"),
                    ],
                );
                async { Ok(()) }.boxed()
            })),
            ..Default::default()
        },
    ));
    smol::block_on(async {
        let live = |id| catalog.has_live_catalog(id);
        registry
            .refresh_harness_catalogs([HarnessId::Pi], false, live)
            .await;
        registry
            .refresh_harness_catalogs([HarnessId::Pi], false, live)
            .await;
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn skips_catalog_refresh_when_no_harness_is_in_use() {
    let registry = registry();
    let pi = Arc::new(AtomicUsize::new(0));
    registry.register_harness(stub(
        HarnessId::Pi,
        Hooks {
            refresh_catalog: Some(counting_hook(&pi)),
            ..Default::default()
        },
    ));
    smol::block_on(registry.refresh_harness_catalogs([], false, |_| false));
    assert_eq!(pi.load(Ordering::SeqCst), 0);
}

#[test]
fn parks_a_live_child_a_few_minutes_after_the_turn_settles() {
    let park = Duration::from_millis(150);
    let registry = HarnessRegistry::new(
        Arc::new(SmolSpawner),
        RegistryOptions {
            idle_park: park,
            ..Default::default()
        },
    );
    let stops = Arc::new(Mutex::new(Vec::new()));
    registry.register_harness(stub(
        HarnessId::Cursor,
        Hooks {
            stop_session: Some(stops.clone()),
            ..Default::default()
        },
    ));
    smol::block_on(async {
        registry
            .send_harness_turn(
                HarnessId::Cursor,
                SendTurnInput {
                    session: session_input("s1", "cursor:composer-2.5"),
                    text: "hi".into(),
                    attachments: None,
                },
                ignore_events(),
                None,
            )
            .await
            .unwrap();
        assert!(stops.lock().is_empty());
        smol::Timer::after(park / 3).await;
        assert!(stops.lock().is_empty());
        smol::Timer::after(park).await;
        assert_eq!(*stops.lock(), vec!["s1".to_string()]);
    });
}

#[test]
fn a_new_turn_cancels_the_pending_park() {
    let park = Duration::from_millis(80);
    let registry = HarnessRegistry::new(
        Arc::new(SmolSpawner),
        RegistryOptions {
            idle_park: park,
            ..Default::default()
        },
    );
    let stops = Arc::new(Mutex::new(Vec::new()));
    registry.register_harness(stub(
        HarnessId::Cursor,
        Hooks {
            stop_session: Some(stops.clone()),
            ..Default::default()
        },
    ));
    smol::block_on(async {
        let send = || {
            registry.send_harness_turn(
                HarnessId::Cursor,
                SendTurnInput {
                    session: session_input("s1", "cursor:composer-2.5"),
                    text: "hi".into(),
                    attachments: None,
                },
                ignore_events(),
                None,
            )
        };
        send().await.unwrap();
        registry.reset_harness_idle_park();
        smol::Timer::after(park * 2).await;
        assert!(stops.lock().is_empty());
        send().await.unwrap();
        registry
            .stop_harness_session(HarnessId::Cursor, "s1")
            .await
            .unwrap();
        smol::Timer::after(park * 2).await;
        // Only the explicit stop ran; it also cancelled the park.
        assert_eq!(*stops.lock(), vec!["s1".to_string()]);
    });
}

#[test]
fn serializes_provider_state_operations_per_session() {
    let registry = registry();
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let (release_first, first_gate) = async_channel::bounded::<()>(1);
    let (mark_first_started, first_started) = async_channel::bounded::<()>(1);
    let log = order.clone();
    registry.register_harness(stub(
        HarnessId::Codex,
        Hooks {
            compact: Some(Arc::new(move |session_id: String| {
                let log = log.clone();
                let first_gate = first_gate.clone();
                let mark_first_started = mark_first_started.clone();
                async move {
                    log.lock().push(format!("start:{session_id}"));
                    if session_id == "s1" {
                        let _ = mark_first_started.try_send(());
                        let _ = first_gate.recv().await;
                    }
                    log.lock().push(format!("end:{session_id}"));
                    Ok(())
                }
                .boxed()
            })),
            ..Default::default()
        },
    ));
    let compact = |session_id: &str| {
        registry.compact_harness_context(
            HarnessId::Codex,
            session_input(session_id, "codex:gpt-5.4"),
            ignore_events(),
        )
    };
    smol::block_on(async {
        let compact1 = compact("s1");
        first_started.recv().await.unwrap();
        let compact2 = compact("s1");
        let independent = compact("s2");

        independent.await.unwrap();
        assert_eq!(*order.lock(), vec!["start:s1", "start:s2", "end:s2"]);
        release_first.send(()).await.unwrap();
        release_first.send(()).await.unwrap();
        let (first, second) = futures::join!(compact1, compact2);
        first.unwrap();
        second.unwrap();
        assert_eq!(
            *order.lock(),
            vec![
                "start:s1", "start:s2", "end:s2", "end:s1", "start:s1", "end:s1"
            ]
        );
    });
}

#[test]
fn a_failed_operation_does_not_block_the_queue() {
    let registry = registry();
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    registry.register_harness(stub(
        HarnessId::Codex,
        Hooks {
            compact: Some(Arc::new(move |_| {
                let attempt = counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        Err(anyhow!("first fails"))
                    } else {
                        Ok(())
                    }
                }
                .boxed()
            })),
            ..Default::default()
        },
    ));
    smol::block_on(async {
        let first = registry.compact_harness_context(
            HarnessId::Codex,
            session_input("s1", "codex:gpt-5.4"),
            ignore_events(),
        );
        let second = registry.compact_harness_context(
            HarnessId::Codex,
            session_input("s1", "codex:gpt-5.4"),
            ignore_events(),
        );
        assert!(first.await.is_err());
        second.await.unwrap();
    });
    assert!(registry.inner.state.lock().operation_tails.is_empty());
}

#[test]
fn binds_a_restored_session_and_forwards_its_task_panels() {
    let registry = registry();
    let binds = Arc::new(Mutex::new(Vec::new()));
    let restores = Arc::new(Mutex::new(Vec::new()));
    registry.register_harness(stub(
        HarnessId::Claude,
        Hooks {
            bind_session: Some(binds.clone()),
            restore_task_lists: Some(restores.clone()),
            ..Default::default()
        },
    ));
    let task_list = TaskListMeta {
        key: Some("claude-tasks".into()),
        items: vec![TaskListItem {
            id: Some("1".into()),
            text: "Write tests".into(),
            status: TaskListItemStatus::Pending,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut tasks = Block::new("b2", BlockRole::Tasks, "Write tests");
    tasks.task_list = Some(task_list.clone());
    let blocks = vec![Block::new("b1", BlockRole::User, "go"), tasks];

    registry.bind_harness_session(
        HarnessId::Claude,
        "s1",
        "sess_1",
        "/repo",
        Some("work"),
        Some(&blocks),
    );
    registry.bind_harness_session(HarnessId::Claude, "s2", "sess_2", "/repo", None, None);

    assert_eq!(
        *binds.lock(),
        vec![
            (
                "s1".into(),
                "sess_1".into(),
                "/repo".into(),
                Some("work".into())
            ),
            ("s2".into(), "sess_2".into(), "/repo".into(), None),
        ]
    );
    assert_eq!(*restores.lock(), vec![("s1".to_string(), vec![task_list])]);
}

#[test]
fn steers_a_live_turn_while_its_send_is_pending() {
    struct Slow {
        release: async_channel::Receiver<()>,
        steered: Arc<AtomicUsize>,
    }
    impl HarnessAdapter for Slow {
        fn id(&self) -> HarnessId {
            HarnessId::Claude
        }
        fn send_turn(
            &self,
            _input: SendTurnInput,
            _on_event: EventSink,
            _on_accepted: Option<AcceptedHook>,
        ) -> BoxFuture<'_, Result<()>> {
            async move {
                let _ = self.release.recv().await;
                Ok(())
            }
            .boxed()
        }
        fn steer_turn(&self, _input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
            self.steered.fetch_add(1, Ordering::SeqCst);
            ok(())
        }
        fn cancel_turn(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
            ok(())
        }
        fn respond_approval(&self, _: &str, _: i64, _: ApprovalDecision) {}
        fn stop_session(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
            ok(())
        }
        fn forget_session(&self, _session_id: String) -> BoxFuture<'_, Result<()>> {
            ok(())
        }
        fn bind_session(&self, _: &str, _: &str, _: &str, _: Option<&str>) {}
    }

    let registry = registry();
    let (release, gate) = async_channel::bounded(1);
    let steered = Arc::new(AtomicUsize::new(0));
    registry.register_harness(Arc::new(Slow {
        release: gate,
        steered: steered.clone(),
    }));
    smol::block_on(async {
        let send = registry.send_harness_turn(
            HarnessId::Claude,
            SendTurnInput {
                session: session_input("s1", "claude:sonnet"),
                text: "go".into(),
                attachments: None,
            },
            ignore_events(),
            None,
        );
        while !registry.inner.state.lock().active_turns.contains("s1") {
            smol::future::yield_now().await;
        }
        registry
            .steer_harness_turn(
                HarnessId::Claude,
                SteerTurnInput {
                    session_id: "s1".into(),
                    cwd: "/tmp".into(),
                    model: "claude:sonnet".into(),
                    model_settings: None,
                    text: "also this".into(),
                    attachments: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(steered.load(Ordering::SeqCst), 1);
        release.send(()).await.unwrap();
        send.await.unwrap();
    });
    assert!(registry.can_steer_harness(HarnessId::Claude));
    assert!(!registry.can_rewind_harness_last_turn(HarnessId::Claude));
}

#[test]
fn reports_turn_control_around_a_send() {
    struct Recorder(Mutex<Vec<String>>);
    impl TurnControl for Recorder {
        fn authorize_turn(
            &self,
            session_id: &str,
            cwd: &str,
            app_access: bool,
        ) -> Result<(), String> {
            self.0
                .lock()
                .push(format!("authorize {session_id} {cwd} {app_access}"));
            if session_id == "denied" {
                return Err("This checkout is controlled by an orchestrator.".into());
            }
            Ok(())
        }
        fn turn_finished(&self, session_id: &str) {
            self.0.lock().push(format!("finished {session_id}"));
        }
    }
    let control = Arc::new(Recorder(Mutex::new(Vec::new())));
    let registry = HarnessRegistry::new(
        Arc::new(SmolSpawner),
        RegistryOptions {
            turn_control: Some(control.clone()),
            ..Default::default()
        },
    );
    registry.register_harness(stub(HarnessId::Codex, Hooks::default()));
    let turn = |session_id: &str| SendTurnInput {
        session: HarnessSessionInput {
            app_access: Some(true),
            ..session_input(session_id, "codex:gpt-5.4")
        },
        text: "go".into(),
        attachments: None,
    };
    smol::block_on(async {
        registry
            .send_harness_turn(HarnessId::Codex, turn("s1"), ignore_events(), None)
            .await
            .unwrap();
        let error = registry
            .send_harness_turn(HarnessId::Codex, turn("denied"), ignore_events(), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("orchestrator"));
    });
    assert_eq!(
        *control.0.lock(),
        vec![
            "authorize s1 /tmp true".to_string(),
            "finished s1".to_string(),
            "authorize denied /tmp true".to_string(),
        ]
    );
    registry.reset_harness_idle_park();
}
