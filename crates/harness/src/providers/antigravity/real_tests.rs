//! Ports of antigravityReal.test.ts and antigravitySoak.test.ts: the adapter
//! against the installed `agy_acp_server.par` over stdio, through the real
//! process host. Both suites are `#[ignore]` and also skip unless their
//! switch is set and the binary exists:
//!
//! ```text
//! AGY_REAL=1 cargo test -p monocode-harness --no-default-features --features antigravity -- --ignored --nocapture real_
//! AGY_SOAK=1 cargo test -p monocode-harness --no-default-features --features antigravity -- --ignored --nocapture soak_
//! ```
//!
//! `AGY_BIN` overrides the binary path, as a configured binary path does.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent, SendTurnInput};
use monocode_process::harness::HarnessHost;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{
    BridgeLease, ChildBackend, ChildFuture, ChildRouter, Children, ExecRequest, HostChildBackend,
    HostChildOptions, HttpRequest, HttpResponse, ResolvedHarnessBinary, SpawnRequest,
};
use crate::core::registry::EventSink;
use crate::core::task::SmolSpawner;

use super::session::{AntigravityOptions, AntigravitySessions};

const LOW_MODEL: &str = "antigravity:gemini-3.8-flash-low";
const HIGH_MODEL: &str = "antigravity:gemini-3.8-flash-high";

/// `BIN`, when the suite's switch is on and the binary exists.
fn enabled(switch: &str) -> Option<String> {
    let bin = std::env::var("AGY_BIN").ok().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join(".local/bin/agy_acp_server.par")
            .to_string_lossy()
            .into_owned()
    });
    let on = std::env::var(switch).as_deref() == Ok("1") && PathBuf::from(&bin).exists();
    if !on {
        eprintln!("skipped: set {switch}=1 and install {bin}");
    }
    on.then_some(bin)
}

/// The host backend, recording writes and spawns, with `AGY_BIN` as the
/// configured Antigravity path.
struct Recorder {
    inner: HostChildBackend,
    bin: String,
    sent: Mutex<Vec<(String, Option<String>, String)>>,
    spawns: AtomicUsize,
    keys: Mutex<Vec<String>>,
}

impl ChildBackend for Recorder {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        self.spawns.fetch_add(1, Ordering::SeqCst);
        self.keys.lock().push(request.session_id.clone());
        self.inner.spawn(request)
    }
    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        let method = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|message| {
                message
                    .get("method")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        self.sent
            .lock()
            .push((session_id.clone(), method, line.clone()));
        self.inner.write(session_id, line)
    }
    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.inner.kill(session_id)
    }
    fn kill_all(&self) -> ChildFuture<()> {
        self.inner.kill_all()
    }
    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String> {
        (provider == HarnessId::Antigravity).then(|| self.bin.clone())
    }
    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        self.inner.resolve_default(provider)
    }
    fn resolve_configured(
        &self,
        provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        self.inner.resolve_configured(provider, binary_path)
    }
    fn exec(&self, request: ExecRequest) -> ChildFuture<String> {
        self.inner.exec(request)
    }
    fn free_port(&self) -> ChildFuture<u16> {
        self.inner.free_port()
    }
    fn http(&self, request: HttpRequest) -> ChildFuture<HttpResponse> {
        self.inner.http(request)
    }
    fn sse_open(
        &self,
        session_id: String,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        self.inner.sse_open(session_id, url, headers)
    }
    fn sse_close(&self, session_id: String) -> ChildFuture<()> {
        self.inner.sse_close(session_id)
    }
    fn read_text_file(&self, path: String) -> ChildFuture<String> {
        self.inner.read_text_file(path)
    }
    fn update_cli(
        &self,
        command: String,
        provider: HarnessId,
        binary_path: Option<String>,
    ) -> ChildFuture<()> {
        self.inner.update_cli(command, provider, binary_path)
    }
    fn home_dir(&self) -> ChildFuture<String> {
        self.inner.home_dir()
    }
}

struct Rig {
    sessions: Arc<AntigravitySessions>,
    backend: Arc<Recorder>,
    dir: PathBuf,
    _lease: BridgeLease,
}

impl Rig {
    fn new(bin: String) -> Self {
        let dir = std::env::temp_dir().join(format!("monocode-agy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let router = Arc::new(ChildRouter::new());
        let host = HarnessHost::new(router.clone());
        let inner = HostChildBackend::new(
            host,
            HostChildOptions {
                data_dir: dir.clone(),
                control: None,
                updater: None,
            },
        );
        let backend = Arc::new(Recorder {
            inner,
            bin,
            sent: Mutex::new(Vec::new()),
            spawns: AtomicUsize::new(0),
            keys: Mutex::new(Vec::new()),
        });
        let children = Children::new(backend.clone(), router, Arc::new(SmolSpawner));
        let lease = children.start_harness_bridge();
        let sessions = AntigravitySessions::new(
            children,
            Arc::new(SmolSpawner),
            SharedCatalog::new(),
            AntigravityOptions::default(),
        );
        Self {
            sessions,
            backend,
            dir,
            _lease: lease,
        }
    }

    fn methods(&self) -> Vec<String> {
        self.backend
            .sent
            .lock()
            .iter()
            .filter_map(|(_, method, _)| method.clone())
            .collect()
    }

    fn sent_contains(&self, method: &str) -> bool {
        self.methods().iter().any(|m| m == method)
    }

    fn prompts(&self) -> usize {
        self.methods()
            .iter()
            .filter(|m| *m == "session/prompt")
            .count()
    }

    fn clear_sent(&self) {
        self.backend.sent.lock().clear();
    }

    fn spawns(&self) -> usize {
        self.backend.spawns.load(Ordering::SeqCst)
    }

    /// `threadPromptSent`.
    fn thread_prompt_sent(&self, thread: &str) -> bool {
        let prefix = format!("{thread}#");
        self.backend.sent.lock().iter().any(|(key, method, _)| {
            method.as_deref() == Some("session/prompt") && key.starts_with(&prefix)
        })
    }

    /// `threadChild`: the newest generation key for a thread.
    fn thread_child(&self, thread: &str) -> Option<String> {
        let prefix = format!("{thread}#");
        self.backend
            .keys
            .lock()
            .iter()
            .rfind(|key| key.starts_with(&prefix))
            .cloned()
    }

    /// `child.kill("SIGKILL")`: kill the process behind the adapter's back.
    async fn kill(&self, key: &str) {
        let _ = self.backend.kill(key.to_string()).await;
    }

    fn turn(
        &self,
        thread: &str,
        text: &str,
        events: &Events,
        options: TurnOptions,
    ) -> smol::Task<anyhow::Result<()>> {
        let mut input = json!({
            "sessionId": thread,
            "cwd": options.cwd.unwrap_or("/tmp"),
            "model": options.model,
            "text": text,
            "runtimeMode": "supervised",
        });
        if let Some(settings) = options.settings {
            input["modelSettings"] = settings;
        }
        let input: SendTurnInput = serde_json::from_value(input).unwrap();
        let sink: EventSink = {
            let events = events.clone();
            let sessions = Arc::downgrade(&self.sessions);
            let thread = thread.to_string();
            let park = options.park_approvals;
            Arc::new(move |event: HarnessEvent| {
                let request = match &event {
                    HarnessEvent::ApprovalRequested { request_id, .. } if !park => {
                        Some(*request_id)
                    }
                    _ => None,
                };
                events.lock().push(event);
                // Unless a test parks on a permission request to prove a turn
                // is in flight, auto-approve so nothing can wedge.
                if let (Some(request_id), Some(sessions)) = (request, sessions.upgrade()) {
                    sessions.respond_antigravity_approval(
                        &thread,
                        request_id,
                        ApprovalDecision::Allow,
                    );
                }
            })
        };
        smol::spawn(self.sessions.send_antigravity_turn(input, sink))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Clone)]
struct TurnOptions {
    cwd: Option<&'static str>,
    model: &'static str,
    settings: Option<Value>,
    park_approvals: bool,
}

impl TurnOptions {
    /// The real suite's turns: the high model, no settings.
    fn real() -> Self {
        Self {
            cwd: None,
            model: HIGH_MODEL,
            settings: None,
            park_approvals: false,
        }
    }

    /// The soak suite's turns: low effort.
    fn soak() -> Self {
        Self {
            cwd: None,
            model: LOW_MODEL,
            settings: Some(json!({ "effort": "low" })),
            park_approvals: false,
        }
    }
}

type Events = Arc<Mutex<Vec<HarnessEvent>>>;

fn events() -> Events {
    Arc::default()
}

fn completed(events: &Events) -> usize {
    events
        .lock()
        .iter()
        .filter(|event| matches!(event, HarnessEvent::MessageCompleted))
        .count()
}

fn has(events: &Events, check: impl Fn(&HarnessEvent) -> bool) -> bool {
    events.lock().iter().any(check)
}

/// `expectCompleted`.
fn expect_completed(events: &Events, count: usize) {
    let errors: Vec<HarnessEvent> = events
        .lock()
        .iter()
        .filter(|event| matches!(event, HarnessEvent::SessionError { .. }))
        .cloned()
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(completed(events), count);
}

/// `vi.waitFor` with a two-minute budget.
async fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !predicate() {
        assert!(Instant::now() < deadline, "timed out waiting");
        smol::Timer::after(Duration::from_millis(200)).await;
    }
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn real_runs_a_turn_end_to_end_and_reuses_the_transport() {
    let Some(bin) = enabled("AGY_REAL") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let first = events();
        rig.turn(
            "real-thread",
            "Reply with the single word OK.",
            &first,
            TurnOptions::real(),
        )
        .await
        .unwrap();
        assert!(has(&first, |e| matches!(e, HarnessEvent::MessageCompleted)));
        assert_eq!(rig.spawns(), 1);
        assert!(rig.sent_contains("session/new"));
        let second = events();
        rig.turn(
            "real-thread",
            "Reply with the single word OK.",
            &second,
            TurnOptions::real(),
        )
        .await
        .unwrap();
        assert!(has(&second, |e| matches!(
            e,
            HarnessEvent::MessageCompleted
        )));
        // The second turn reuses the same child.
        assert_eq!(rig.spawns(), 1);
        rig.sessions
            .forget_antigravity_session("real-thread")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn real_cancels_a_mid_prompt_turn_fast_and_recovers_on_the_next_send() {
    let Some(bin) = enabled("AGY_REAL") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let parked = events();
        let pending = rig.turn(
            "real-thread",
            "Use your tools to run the shell command `ls -la /tmp` and report the output.",
            &parked,
            TurnOptions {
                park_approvals: true,
                ..TurnOptions::real()
            },
        );
        // A permission request parks the turn server-side until we answer.
        wait_for(|| {
            has(&parked, |e| {
                matches!(e, HarnessEvent::ApprovalRequested { .. })
            })
        })
        .await;
        let at = Instant::now();
        rig.sessions.cancel_antigravity_turn_now("real-thread");
        pending.await.unwrap();
        assert!(at.elapsed() < Duration::from_secs(60));
        wait_for(|| rig.sent_contains("session/cancel")).await;
        // The cancelled transport is stale: the next send recycles and resumes.
        let before = rig.spawns();
        let next = events();
        rig.turn(
            "real-thread",
            "Reply with the single word OK.",
            &next,
            TurnOptions::real(),
        )
        .await
        .unwrap();
        assert_eq!(rig.spawns(), before + 1);
        assert!(rig.sent_contains("session/resume"));
        assert!(has(&next, |e| matches!(e, HarnessEvent::MessageCompleted)));
        rig.sessions
            .forget_antigravity_session("real-thread")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn real_survives_the_provider_process_exiting_mid_turn_and_resumes_after_respawn() {
    let Some(bin) = enabled("AGY_REAL") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let first = events();
        let pending = rig.turn(
            "real-thread",
            "Write a long poem about the sea.",
            &first,
            TurnOptions::real(),
        );
        wait_for(|| rig.thread_prompt_sent("real-thread")).await;
        rig.kill(&rig.thread_child("real-thread").unwrap()).await;
        // The exit settles the turn instead of wedging it.
        let _ = pending.await;
        assert!(has(&first, |e| matches!(
            e,
            HarnessEvent::SessionEnded { .. }
        )));
        let next = events();
        rig.turn(
            "real-thread",
            "Reply with the single word OK.",
            &next,
            TurnOptions::real(),
        )
        .await
        .unwrap();
        assert!(rig.sent_contains("session/resume"));
        assert!(has(&next, |e| matches!(e, HarnessEvent::MessageCompleted)));
        rig.sessions
            .forget_antigravity_session("real-thread")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns a fleet of real agy_acp_server.par processes"]
fn soak_handles_a_6_thread_fleet_of_3_sequential_turns_each() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Arc::new(Rig::new(bin));
    smol::block_on(async {
        let fleets: Vec<(String, Events)> =
            (0..6).map(|i| (format!("fleet-{i}"), events())).collect();
        let tasks: Vec<_> = fleets
            .iter()
            .enumerate()
            .map(|(i, (thread, events))| {
                let rig = rig.clone();
                let thread = thread.clone();
                let events = events.clone();
                smol::spawn(async move {
                    // Stagger starts: a simultaneous session/new burst can trip
                    // the server's own rate limits.
                    smol::Timer::after(Duration::from_millis(i as u64 * 250)).await;
                    for turn_no in 0..3 {
                        rig.turn(
                            &thread,
                            &format!("Reply with the single word OK{i}{turn_no}."),
                            &events,
                            TurnOptions::soak(),
                        )
                        .await
                        .unwrap();
                    }
                })
            })
            .collect();
        for task in tasks {
            task.await;
        }
        for (_, events) in &fleets {
            expect_completed(events, 3);
        }
        // Each thread spawned exactly one child: no spawn storm.
        assert_eq!(rig.spawns(), 6);
        assert_eq!(rig.prompts(), 18);
        for (thread, _) in &fleets {
            let _ = rig.sessions.forget_antigravity_session(thread).await;
        }
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_serializes_4_rapid_fire_sends_on_one_thread_without_a_lost_turn() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let events = events();
        let sends: Vec<_> = ["one", "two", "three", "four"]
            .iter()
            .map(|word| {
                rig.turn(
                    "rapid",
                    &format!("Reply with the single word {word}."),
                    &events,
                    TurnOptions::soak(),
                )
            })
            .collect();
        for send in sends {
            send.await.unwrap();
        }
        expect_completed(&events, 4);
        assert_eq!(rig.spawns(), 1);
        assert_eq!(rig.prompts(), 4);
        rig.sessions
            .forget_antigravity_session("rapid")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_survives_5_cancel_cycles_every_resend_recycles_and_resumes() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let events = events();
        let mut mid_turn_cancels = 0;
        for _ in 0..5 {
            let approvals = |events: &Events| {
                events
                    .lock()
                    .iter()
                    .filter(|e| matches!(e, HarnessEvent::ApprovalRequested { .. }))
                    .count()
            };
            let at_start = approvals(&events);
            // `ls` with the default model reliably asks permission, which
            // parks the turn server-side, so the cancel lands mid-turn.
            let pending = rig.turn(
                "cancel-cycle",
                "Use your tools to run the shell command `ls -la /tmp` and report the output.",
                &events,
                TurnOptions {
                    model: HIGH_MODEL,
                    settings: None,
                    park_approvals: true,
                    ..TurnOptions::soak()
                },
            );
            wait_for(|| approvals(&events) > at_start || pending.is_finished()).await;
            let in_flight = !pending.is_finished();
            let at = Instant::now();
            rig.sessions.cancel_antigravity_turn_now("cancel-cycle");
            let _ = pending.await;
            assert!(at.elapsed() < Duration::from_secs(60));
            let spawns = rig.spawns();
            let done = Arc::default();
            rig.turn(
                "cancel-cycle",
                "Reply with the single word OK.",
                &done,
                TurnOptions::soak(),
            )
            .await
            .unwrap();
            assert!(has(&done, |e| matches!(e, HarnessEvent::MessageCompleted)));
            // A mid-turn cancel marks the transport stale, so the resend runs
            // on a fresh process. A turn that ended first may reuse it.
            if in_flight {
                mid_turn_cancels += 1;
                assert!(rig.spawns() > spawns);
            }
        }
        assert!(mid_turn_cancels >= 1);
        assert!(rig.sent_contains("session/resume"));
        rig.sessions
            .forget_antigravity_session("cancel-cycle")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_survives_3_stop_cycles_next_send_starts_a_fresh_process_with_resume() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let events = events();
        for _ in 0..3 {
            let pending = rig.turn(
                "stop-cycle",
                "Write a poem about the ocean, at least twenty lines.",
                &events,
                TurnOptions::soak(),
            );
            wait_for(|| rig.thread_prompt_sent("stop-cycle")).await;
            rig.sessions
                .stop_antigravity_session("stop-cycle")
                .await
                .unwrap();
            let _ = pending.await;
            let done = Arc::default();
            rig.turn(
                "stop-cycle",
                "Reply with the single word OK.",
                &done,
                TurnOptions::soak(),
            )
            .await
            .unwrap();
            assert!(has(&done, |e| matches!(e, HarnessEvent::MessageCompleted)));
            rig.clear_sent();
        }
        // The first process plus three respawns.
        assert_eq!(rig.spawns(), 4);
        rig.sessions
            .forget_antigravity_session("stop-cycle")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_survives_3_forget_cycles_next_send_starts_a_brand_new_session() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let events = events();
        for _ in 0..3 {
            let pending = rig.turn(
                "forget-cycle",
                "Write a poem about mountains, at least twenty lines.",
                &events,
                TurnOptions::soak(),
            );
            wait_for(|| rig.thread_prompt_sent("forget-cycle")).await;
            rig.sessions
                .forget_antigravity_session("forget-cycle")
                .await
                .unwrap();
            let _ = pending.await;
            rig.clear_sent();
            let done = Arc::default();
            rig.turn(
                "forget-cycle",
                "Reply with the single word OK.",
                &done,
                TurnOptions::soak(),
            )
            .await
            .unwrap();
            assert!(has(&done, |e| matches!(e, HarnessEvent::MessageCompleted)));
            // Forget drops the binding: a fresh session, never a resume.
            assert!(rig.sent_contains("session/new"));
            assert!(!rig.sent_contains("session/resume"));
        }
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_survives_3_mid_turn_process_kills_each_resend_resumes_cleanly() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        for _ in 0..3 {
            let events = events();
            let pending = rig.turn(
                "kill-cycle",
                "Write a poem about rivers, at least twenty lines.",
                &events,
                TurnOptions::soak(),
            );
            wait_for(|| {
                rig.thread_prompt_sent("kill-cycle") && rig.thread_child("kill-cycle").is_some()
            })
            .await;
            rig.kill(&rig.thread_child("kill-cycle").unwrap()).await;
            let _ = pending.await;
            assert!(has(&events, |e| matches!(
                e,
                HarnessEvent::SessionEnded { .. }
            )));
            let done = Arc::default();
            rig.turn(
                "kill-cycle",
                "Reply with the single word OK.",
                &done,
                TurnOptions::soak(),
            )
            .await
            .unwrap();
            assert!(has(&done, |e| matches!(e, HarnessEvent::MessageCompleted)));
        }
        assert!(rig.sent_contains("session/resume"));
        rig.sessions
            .forget_antigravity_session("kill-cycle")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_moves_a_thread_to_a_different_cwd_without_leaking_the_old_session() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let first = events();
        rig.turn(
            "drift",
            "Reply with the single word OK.",
            &first,
            TurnOptions::soak(),
        )
        .await
        .unwrap();
        expect_completed(&first, 1);
        rig.clear_sent();
        let second = events();
        rig.turn(
            "drift",
            "Reply with the single word OK.",
            &second,
            TurnOptions {
                cwd: Some("/private/tmp"),
                ..TurnOptions::soak()
            },
        )
        .await
        .unwrap();
        expect_completed(&second, 1);
        // A moved cwd drops the resume binding: fresh session, fresh process.
        assert!(rig.sent_contains("session/new"));
        assert!(!rig.sent_contains("session/resume"));
        assert_eq!(rig.spawns(), 2);
        rig.sessions
            .forget_antigravity_session("drift")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_suppresses_a_send_that_was_queued_when_the_running_turn_was_cancelled() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let events_a = events();
        let a = rig.turn(
            "suppress",
            "Use your tools to run the shell command `sleep 5 && echo done` and report the output.",
            &events_a,
            TurnOptions::soak(),
        );
        wait_for(|| {
            has(&events_a, |e| {
                matches!(e, HarnessEvent::ApprovalRequested { .. })
            }) || rig.thread_prompt_sent("suppress")
        })
        .await;
        let events_b = events();
        let b = rig.turn(
            "suppress",
            "PINEAPPLE-UNIQUE-MARKER",
            &events_b,
            TurnOptions::soak(),
        );
        rig.sessions.cancel_antigravity_turn_now("suppress");
        let _ = a.await;
        let _ = b.await;
        // B was queued at cancel time: its prompt must never reach the wire.
        assert!(
            !rig.backend
                .sent
                .lock()
                .iter()
                .any(|(_, _, raw)| raw.contains("PINEAPPLE-UNIQUE-MARKER"))
        );
        let events_c = events();
        rig.turn(
            "suppress",
            "Reply with the single word OK.",
            &events_c,
            TurnOptions::soak(),
        )
        .await
        .unwrap();
        expect_completed(&events_c, 1);
        rig.sessions
            .forget_antigravity_session("suppress")
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "spawns several real agy_acp_server.par processes"]
fn soak_kitchen_sink_4_concurrent_threads_each_a_different_abuse_pattern() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Arc::new(Rig::new(bin));
    smol::block_on(async {
        let steady = events();
        let cancels = events();
        let stops = events();
        let kills = events();
        let t1 = {
            let (rig, ev) = (rig.clone(), steady.clone());
            smol::spawn(async move {
                for _ in 0..3 {
                    rig.turn(
                        "steady",
                        "Reply with the single word OK.",
                        &ev,
                        TurnOptions::soak(),
                    )
                    .await
                    .unwrap();
                }
            })
        };
        let t2 = {
            let (rig, ev) = (rig.clone(), cancels.clone());
            smol::spawn(async move {
                let pending = rig.turn(
                    "cancels",
                    "Use your tools to run the shell command `sleep 4 && echo done` and report the output.",
                    &ev,
                    TurnOptions::soak(),
                );
                wait_for(|| {
                    has(&ev, |e| matches!(e, HarnessEvent::ApprovalRequested { .. }))
                        || rig.thread_prompt_sent("cancels")
                })
                .await;
                rig.sessions.cancel_antigravity_turn_now("cancels");
                let _ = pending.await;
                rig.turn(
                    "cancels",
                    "Reply with the single word OK.",
                    &ev,
                    TurnOptions::soak(),
                )
                .await
                .unwrap();
            })
        };
        let t3 = {
            let (rig, ev) = (rig.clone(), stops.clone());
            smol::spawn(async move {
                let pending = rig.turn(
                    "stops",
                    "Write a poem about forests, at least twenty lines.",
                    &ev,
                    TurnOptions::soak(),
                );
                wait_for(|| rig.thread_prompt_sent("stops")).await;
                rig.sessions
                    .stop_antigravity_session("stops")
                    .await
                    .unwrap();
                let _ = pending.await;
                rig.turn(
                    "stops",
                    "Reply with the single word OK.",
                    &ev,
                    TurnOptions::soak(),
                )
                .await
                .unwrap();
            })
        };
        let t4 = {
            let (rig, ev) = (rig.clone(), kills.clone());
            smol::spawn(async move {
                let pending = rig.turn(
                    "kills",
                    "Write a poem about deserts, at least twenty lines.",
                    &ev,
                    TurnOptions::soak(),
                );
                wait_for(|| rig.thread_prompt_sent("kills") && rig.thread_child("kills").is_some())
                    .await;
                rig.kill(&rig.thread_child("kills").unwrap()).await;
                let _ = pending.await;
                rig.turn(
                    "kills",
                    "Reply with the single word OK.",
                    &ev,
                    TurnOptions::soak(),
                )
                .await
                .unwrap();
            })
        };
        t1.await;
        t2.await;
        t3.await;
        t4.await;
        expect_completed(&steady, 3);
        assert_eq!(completed(&cancels), 1);
        assert_eq!(completed(&stops), 1);
        assert_eq!(completed(&kills), 1);
        for thread in ["steady", "cancels", "stops", "kills"] {
            let _ = rig.sessions.forget_antigravity_session(thread).await;
        }
    });
}

#[test]
#[ignore = "spawns the real agy_acp_server.par"]
fn soak_long_task_interrupt_and_continue_on_the_same_thread() {
    let Some(bin) = enabled("AGY_SOAK") else {
        return;
    };
    let rig = Rig::new(bin);
    smol::block_on(async {
        let events = events();
        let pending = rig.turn(
            "marathon",
            "Count from 1 to 60, one number per line.",
            &events,
            TurnOptions::soak(),
        );
        wait_for(|| {
            has(&events, |e| matches!(e, HarnessEvent::MessageDelta { .. }))
                || rig.thread_prompt_sent("marathon")
        })
        .await;
        rig.sessions.cancel_antigravity_turn_now("marathon");
        let _ = pending.await;
        let done = events_for_turn(&rig, "marathon", "Now reply with the single word OK.").await;
        expect_completed(&done, 1);
        assert!(rig.sent_contains("session/resume"));
        rig.sessions
            .forget_antigravity_session("marathon")
            .await
            .unwrap();
    });
}

async fn events_for_turn(rig: &Rig, thread: &str, text: &str) -> Events {
    let done = events();
    rig.turn(thread, text, &done, TurnOptions::soak())
        .await
        .unwrap();
    done
}
