//! Port of host/engine.test.ts, and the engine cases of
//! host/git-worktrees.test.ts. A scripted provider stands in for the CLIs.
//!
//! Not ported here: the cases that only exercise `HostStore` (legacy
//! summaries and timestamps, rollback failure), which monocode-remote's
//! store tests cover.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::channel::oneshot;
use monocode_core::harness_event::ApprovalDecision as CoreDecision;
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{Attachment, HarnessId};
use monocode_harness::core::registry::EventSink;
use monocode_harness::core::session_title::GeneratedSessionTitle;
use monocode_remote::host::attachments::{read_attachment_chunk, write_attachment_chunk};
use monocode_remote::host::protocol::HostSession;
use serde_json::{Map, json};

use super::*;
use crate::providers::ProviderFuture;
use crate::runtime::HostRuntime;

/// One `send` the provider received.
struct Turn {
    input: SendTurnInput,
    on_event: EventSink,
    finish: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct Fake {
    turns: Mutex<Vec<Turn>>,
    sends: AtomicUsize,
    stops: AtomicUsize,
    compacts: AtomicUsize,
    compact: AtomicBool,
    binds: Mutex<Vec<(String, String, String)>>,
    approvals: Mutex<Vec<(String, i64, CoreDecision)>>,
    answers: AtomicUsize,
    titles: AtomicUsize,
    title: Mutex<Option<async_channel::Receiver<GeneratedSessionTitle>>>,
    branches: AtomicUsize,
    branch: Mutex<Option<async_channel::Receiver<String>>>,
}

impl Fake {
    fn push(&self, input: SendTurnInput, on_event: EventSink) -> ProviderFuture<()> {
        let (finish, finished) = oneshot::channel();
        self.turns.lock().push(Turn {
            input,
            on_event,
            finish: Some(finish),
        });
        async move {
            let _ = finished.await;
            Ok(())
        }
        .boxed()
    }

    fn finish_last(&self) {
        if let Some(turn) = self.turns.lock().last_mut()
            && let Some(finish) = turn.finish.take()
        {
            let _ = finish.send(());
        }
    }

    fn finish(&self, index: usize) {
        if let Some(finish) = self.turns.lock()[index].finish.take() {
            let _ = finish.send(());
        }
    }

    fn emit(&self, index: usize, event: Value) {
        let sink = self.turns.lock()[index].on_event.clone();
        sink(serde_json::from_value(event).unwrap());
    }

    fn input(&self, index: usize) -> SendTurnInput {
        self.turns.lock()[index].input.clone()
    }

    fn turn_count(&self) -> usize {
        self.turns.lock().len()
    }
}

impl HostProvider for Fake {
    fn send(&self, input: SendTurnInput, on_event: EventSink) -> ProviderFuture<()> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.push(input, on_event)
    }

    fn can_compact(&self) -> bool {
        self.compact.load(Ordering::SeqCst)
    }

    fn compact(
        &self,
        input: monocode_core::harness_event::CompactContextInput,
        on_event: EventSink,
    ) -> ProviderFuture<()> {
        self.compacts.fetch_add(1, Ordering::SeqCst);
        self.push(
            SendTurnInput {
                session: input,
                text: "/compact".into(),
                attachments: None,
            },
            on_event,
        )
    }

    fn cancel(&self, _id: &str) -> ProviderFuture<()> {
        self.finish_last();
        async { Ok(()) }.boxed()
    }

    fn stop(&self, _id: &str) -> ProviderFuture<()> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.finish_last();
        async { Ok(()) }.boxed()
    }

    fn bind(&self, id: &str, provider_id: &str, cwd: &str) {
        self.binds
            .lock()
            .push((id.into(), provider_id.into(), cwd.into()));
    }

    fn approve(&self, id: &str, request: i64, decision: CoreDecision) -> Result<(), String> {
        self.approvals.lock().push((id.into(), request, decision));
        Ok(())
    }

    fn answer(&self, _id: &str, _request: i64, _reply: UserQuestionReply) -> Result<(), String> {
        self.answers.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn can_generate_title(&self) -> bool {
        self.title.lock().is_some()
    }

    fn generate_title(&self, _input: TitleInput) -> ProviderFuture<Option<GeneratedSessionTitle>> {
        self.titles.fetch_add(1, Ordering::SeqCst);
        let receiver = self.title.lock().clone();
        async move {
            match receiver {
                Some(receiver) => Ok(receiver.recv().await.ok()),
                None => Ok(None),
            }
        }
        .boxed()
    }

    fn can_generate_branch_name(&self) -> bool {
        self.branch.lock().is_some()
    }

    fn generate_branch_name(&self, _cwd: &str, _message: &str) -> ProviderFuture<Option<String>> {
        self.branches.fetch_add(1, Ordering::SeqCst);
        let receiver = self.branch.lock().clone();
        async move {
            match receiver {
                Some(receiver) => Ok(receiver.recv().await.ok()),
                None => Ok(None),
            }
        }
        .boxed()
    }
}

/// `vi.waitFor`.
fn wait_for(what: &str, condition: impl Fn() -> bool) {
    wait_for_within(what, Duration::from_secs(4), condition);
}

fn wait_for_within(what: &str, timeout: Duration, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Setup {
    directory: tempfile::TempDir,
    store: Arc<HostStore>,
    provider: Arc<Fake>,
    project: HostProject,
    engine: HostEngine,
    id: String,
    runtime: HostRuntime,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.engine.close();
        self.runtime.shutdown();
        self.store.close();
    }
}

fn engine_over(
    store: Arc<HostStore>,
    provider: Arc<Fake>,
    harnesses: &[HarnessId],
    runtime: &HostRuntime,
) -> HostEngine {
    let providers: HostProviders = harnesses
        .iter()
        .map(|id| (*id, provider.clone() as Arc<dyn HostProvider>))
        .collect();
    HostEngine::new(store, providers, SharedCatalog::new(), runtime.spawner()).unwrap()
}

fn setup_with(harness: HarnessId, cwd: Option<&str>) -> Setup {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(HostStore::open(&directory.path().join("host.db")).unwrap());
    let project_cwd = cwd
        .map(str::to_string)
        .unwrap_or_else(|| directory.path().to_string_lossy().into_owned());
    let project = store.add_project(&project_cwd, "Test").unwrap();
    let provider = Arc::new(Fake::default());
    let runtime = HostRuntime::new(2);
    let engine = engine_over(
        store.clone(),
        provider.clone(),
        &[HarnessId::Codex, HarnessId::Claude],
        &runtime,
    );
    let created = engine
        .command(&json!({
            "type": "create",
            "commandId": "create",
            "projectId": project.id,
            "harness": harness.as_str(),
            "model": format!("{}:test", harness.as_str()),
            "runtimeMode": "supervised",
        }))
        .unwrap();
    Setup {
        directory,
        store,
        provider,
        project,
        engine,
        id: created.session_id,
        runtime,
    }
}

fn setup() -> Setup {
    setup_with(HarnessId::Codex, None)
}

impl Setup {
    fn session(&self) -> Arc<HostSession> {
        self.store.session(&self.id).unwrap()
    }

    fn send(&self, command_id: &str, text: &str) -> Result<CommandReceipt, String> {
        self.engine.command(&json!({
            "type": "send",
            "commandId": command_id,
            "sessionId": self.id,
            "text": text,
        }))
    }

    fn wait_for_turns(&self, count: usize) {
        wait_for("provider turns", || self.provider.turn_count() >= count);
    }

    fn wait_for_status(&self, status: HostSessionStatus) {
        wait_for("session status", || self.session().status == status);
    }

    fn upload(&self, id: &str, bytes: &[u8]) {
        let mut params = Map::new();
        params.insert("id".into(), json!(id));
        params.insert("offset".into(), json!(0));
        params.insert("size".into(), json!(bytes.len()));
        params.insert(
            "data".into(),
            json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
        );
        write_attachment_chunk(&self.store, &params).unwrap();
    }
}

#[test]
fn clears_the_old_draft_when_a_normal_send_or_compact_starts() {
    for kind in ["send", "compact"] {
        let setup = setup();
        setup.provider.compact.store(true, Ordering::SeqCst);
        setup
            .engine
            .command(&json!({ "type": "draft", "commandId": "draft", "sessionId": setup.id, "text": "Later" }))
            .unwrap();
        setup
            .engine
            .command(&json!({ "type": kind, "commandId": "next", "sessionId": setup.id, "text": "New work" }))
            .unwrap();
        assert!(!setup.session().session.blocks.iter().any(Block::is_draft));
        setup.wait_for_turns(1);
        setup.provider.finish(0);
    }
}

#[test]
fn contains_a_persistence_failure_while_requesting_approval() {
    let setup = setup();
    setup.send("approval-failure", "Work").unwrap();
    setup.wait_for_turns(1);
    let mut failed = false;
    setup.engine.set_save_fault(Some(Box::new(move |_| {
        !std::mem::replace(&mut failed, true)
    })));
    setup.provider.emit(
        0,
        json!({ "type": "approval.requested", "requestId": 1, "title": "Run?" }),
    );
    wait_for("stop", || setup.provider.stops.load(Ordering::SeqCst) > 0);
    setup.wait_for_status(HostSessionStatus::Interrupted);
}

#[test]
fn stores_a_remote_draft_with_an_uploaded_file_then_sends_it_in_plan_mode() {
    let setup = setup();
    let file_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    setup.upload(file_id, b"hello");
    let attachment = json!({
        "id": file_id, "name": "notes.txt", "mimeType": "text/plain", "kind": "file", "size": 5,
    });
    setup
        .engine
        .command(&json!({
            "type": "draft", "commandId": "draft-1", "sessionId": setup.id,
            "text": "Plan this", "attachments": [attachment],
        }))
        .unwrap();
    let first = setup.session().session.blocks[0].clone();
    assert_eq!(first.draft, Some(true));
    assert_eq!(first.attachments.unwrap()[0].name, "notes.txt");
    assert_eq!(
        setup.store.summaries(&setup.project.id).unwrap()[0].draft,
        Some(true)
    );
    setup
        .engine
        .command(&json!({
            "type": "send", "commandId": "send-draft", "sessionId": setup.id,
            "text": "Plan this", "intent": "plan", "draftBlockId": "draft-1",
        }))
        .unwrap();
    setup.wait_for_turns(1);
    let input = setup.provider.input(0);
    assert_eq!(input.session.intent, Some(TurnIntent::Plan));
    let sent: Vec<Attachment> = input.attachments.unwrap();
    assert_eq!((sent[0].name.as_str(), sent[0].size), ("notes.txt", 5));
    assert!(sent[0].path.as_deref().unwrap().contains(file_id));
    assert_ne!(
        setup.store.summaries(&setup.project.id).unwrap()[0].draft,
        Some(true)
    );
    setup.provider.finish(0);
}

#[test]
fn passes_an_uploaded_image_to_the_host_provider_on_an_attachment_only_turn() {
    let setup = setup_with(HarnessId::Claude, None);
    let file_id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    let image = b"image-bytes";
    setup.upload(file_id, image);
    setup
        .engine
        .command(&json!({
            "type": "send", "commandId": "image-turn", "sessionId": setup.id, "text": "",
            "attachments": [{
                "id": file_id, "name": "shot.png", "mimeType": "image/png", "kind": "image",
                "size": image.len(),
            }],
        }))
        .unwrap();
    setup.wait_for_turns(1);
    let encoded = base64::engine::general_purpose::STANDARD.encode(image);
    let sent = setup.provider.input(0).attachments.unwrap();
    assert_eq!(sent[0].name, "shot.png");
    assert_eq!(sent[0].data.as_deref(), Some(encoded.as_str()));
    let read = |session_id: &str| {
        let mut params = Map::new();
        params.insert("sessionId".into(), json!(session_id));
        params.insert("id".into(), json!(file_id));
        params.insert("offset".into(), json!(0));
        read_attachment_chunk(&setup.store, &params)
    };
    assert_eq!(
        serde_json::to_value(read(&setup.id).unwrap()).unwrap(),
        json!({ "offset": image.len(), "size": image.len(), "data": encoded })
    );
    let other = setup
        .engine
        .command(&json!({
            "type": "create", "commandId": "other-session", "projectId": setup.project.id,
            "harness": "claude", "model": "claude:test", "runtimeMode": "supervised",
        }))
        .unwrap();
    assert!(read(&other.session_id).is_err());
    setup.provider.finish(0);
}

#[test]
fn removes_a_remote_draft_without_starting_the_provider() {
    let setup = setup();
    setup
        .engine
        .command(&json!({ "type": "draft", "commandId": "draft-2", "sessionId": setup.id, "text": "Later" }))
        .unwrap();
    setup
        .engine
        .command(&json!({
            "type": "removeDraft", "commandId": "remove-2", "sessionId": setup.id, "draftBlockId": "draft-2",
        }))
        .unwrap();
    assert!(setup.session().session.blocks.is_empty());
    assert_eq!(setup.provider.sends.load(Ordering::SeqCst), 0);
}

#[test]
fn marks_a_reviewed_host_plan_as_built_after_its_build_turn() {
    let setup = setup();
    setup
        .engine
        .command(&json!({
            "type": "send", "commandId": "plan-turn", "sessionId": setup.id,
            "text": "Plan this", "intent": "plan",
        }))
        .unwrap();
    setup.wait_for_turns(1);
    setup.provider.emit(
        0,
        json!({ "type": "plan", "text": "# Steps\n\n1. Change code" }),
    );
    setup.provider.finish(0);
    setup.wait_for_status(HostSessionStatus::Idle);
    let plan = setup
        .session()
        .session
        .blocks
        .iter()
        .find(|block| block.role == BlockRole::Plan)
        .cloned()
        .unwrap();
    let status = |setup: &Setup| {
        setup
            .session()
            .session
            .blocks
            .iter()
            .find(|block| block.id == plan.id)
            .and_then(|block| block.plan.as_ref().map(|plan| plan.status))
    };
    setup
        .engine
        .command(&json!({
            "type": "send", "commandId": "build-turn", "sessionId": setup.id,
            "text": format!("Build the approved plan:\n\n{}", plan.text),
            "intent": "build", "planBlockId": plan.id,
        }))
        .unwrap();
    assert_eq!(status(&setup), Some(PlanStatus::Building));
    setup.wait_for_turns(2);
    setup.provider.finish(1);
    setup.wait_for_status(HostSessionStatus::Idle);
    assert_eq!(status(&setup), Some(PlanStatus::Built));
}

#[test]
fn keeps_a_manually_renamed_title_when_first_turn_generation_finishes_later() {
    let setup = setup();
    let (finish_title, title) = async_channel::bounded(1);
    *setup.provider.title.lock() = Some(title);
    setup
        .send("name-first-turn", "Fix remote project titles")
        .unwrap();
    setup.wait_for_turns(1);
    setup
        .engine
        .update_session(
            &setup.id,
            &SessionPatch {
                title: Some("codex · My own title".into()),
                ..Default::default()
            },
        )
        .unwrap();
    smol::block_on(finish_title.send(GeneratedSessionTitle {
        title: "Generated title".into(),
        work_item: None,
    }))
    .unwrap();
    wait_for("title request", || {
        setup.provider.titles.load(Ordering::SeqCst) == 1
    });
    setup.provider.finish(0);
    setup.wait_for_status(HostSessionStatus::Idle);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(setup.session().session.title, "codex · My own title");
}

#[test]
fn uses_one_creation_timestamp_and_advances_only_updated_at_on_later_commands() {
    let setup = setup();
    let initial = setup.session();
    assert_eq!(initial.created_at, Some(initial.updated_at));
    assert_eq!(initial.revision, 1);
    let listed = &setup.store.sessions(Some(&setup.project.id)).unwrap()[0];
    assert_eq!(
        (listed.created_at, listed.updated_at),
        (initial.created_at, initial.updated_at)
    );
    let summary = &setup.store.summaries(&setup.project.id).unwrap()[0];
    assert_eq!(
        (summary.created_at, summary.updated_at),
        (initial.created_at, initial.updated_at)
    );
    std::thread::sleep(Duration::from_millis(5));
    setup
        .engine
        .command(&json!({
            "type": "configure", "commandId": "configure-timestamps", "sessionId": setup.id,
            "model": "codex:updated", "modelSettings": {}, "runtimeMode": "supervised",
        }))
        .unwrap();
    let updated = setup.session();
    assert_eq!(updated.created_at, initial.created_at);
    assert!(updated.updated_at > initial.updated_at);
    assert_eq!(updated.revision, 2);
    let summary = &setup.store.summaries(&setup.project.id).unwrap()[0];
    assert_eq!(
        (summary.created_at, summary.updated_at, summary.revision),
        (updated.created_at, updated.updated_at, 2)
    );
}

#[test]
fn keeps_remote_card_changes_in_host_history_and_removes_deleted_sessions() {
    let setup = setup();
    let initial = setup.store.summaries(&setup.project.id).unwrap()[0].clone();
    assert_eq!(initial.model.as_deref(), Some("codex:test"));
    assert_eq!(initial.created_at, Some(initial.updated_at));
    let updated = setup
        .engine
        .update_session(
            &setup.id,
            &SessionPatch {
                title: Some("Codex · Renamed".into()),
                pinned: Some(true),
                archived: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.title, "Codex · Renamed");
    assert_eq!((updated.pinned, updated.archived), (Some(true), Some(true)));
    let sync =
        serde_json::to_value(setup.store.sync(&setup.id, Some(initial.revision)).unwrap()).unwrap();
    assert_eq!(sync["kind"], "delta");
    setup.store.delete_session(&setup.id).unwrap();
    assert!(setup.store.summaries(&setup.project.id).unwrap().is_empty());
    assert!(
        setup
            .store
            .session(&setup.id)
            .unwrap_err()
            .contains("Session not found")
    );
}

#[test]
fn keeps_the_checkout_idle_while_a_branch_switch_is_in_progress() {
    let setup = setup();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (entered, entering) = std::sync::mpsc::channel::<()>();
    let engine = setup.engine.clone();
    let project_id = setup.project.id.clone();
    let switching = std::thread::spawn(move || {
        engine.with_idle_project(&project_id, false, || {
            entered.send(()).unwrap();
            released.recv().unwrap();
            Ok(())
        })
    });
    entering.recv().unwrap();
    assert!(
        setup
            .send("during-switch", "Work")
            .unwrap_err()
            .contains("branch switch")
    );
    assert!(
        setup
            .engine
            .command(&json!({
                "type": "create", "commandId": "new-during-switch", "projectId": setup.project.id,
                "harness": "codex", "model": "codex:test", "runtimeMode": "supervised",
            }))
            .unwrap_err()
            .contains("branch switch")
    );
    assert!(
        setup
            .engine
            .with_idle_project(&setup.project.id, false, || Ok(()))
            .unwrap_err()
            .contains("already in progress")
    );
    release.send(()).unwrap();
    switching.join().unwrap().unwrap();
    setup.send("after-switch", "Work").unwrap();
    setup.wait_for_turns(1);
    setup.provider.finish(0);
}

#[test]
fn clears_a_usage_limit_when_the_next_turn_starts() {
    let setup = setup();
    let mut value = (*setup.session()).clone();
    value.revision += 1;
    value.session.usage_limit = Some(serde_json::from_value(json!({ "resetsAt": 1 })).unwrap());
    setup.store.save(value, &json!({ "type": "test" })).unwrap();
    setup.send("retry", "Go on").unwrap();
    assert!(setup.session().session.usage_limit.is_none());
    setup.wait_for_turns(1);
    setup.provider.finish(0);
}

#[test]
fn names_running_sessions_before_a_branch_switch_and_switches_when_forced() {
    let setup = setup();
    setup.send("running", "Work").unwrap();
    setup.wait_for_turns(1);
    let title = setup.session().session.title.clone();
    assert_eq!(
        setup
            .engine
            .with_idle_project(&setup.project.id, false, || Ok("switched"))
            .unwrap_err(),
        format!(
            "\"{title}\" is running on the host. Switching branches changes the files it is working on."
        )
    );
    assert_eq!(
        setup
            .engine
            .with_idle_project(&setup.project.id, true, || Ok("switched"))
            .unwrap(),
        "switched"
    );
    setup.provider.finish(0);
}

#[test]
fn names_up_to_three_running_sessions() {
    let summaries: Vec<HostSessionSummary> = (1..=4)
        .map(|index| {
            serde_json::from_value(json!({
                "projectId": "p", "revision": 1, "status": "running", "updatedAt": 1,
                "id": format!("s{index}"), "title": format!("T{index}"), "harness": "codex",
            }))
            .unwrap()
        })
        .collect();
    assert_eq!(
        running_sessions_message(&summaries),
        "4 sessions are running on the host: \"T1\", \"T2\", \"T3\" and 1 more. Switching branches changes the files they are working on."
    );
}

#[test]
fn runs_provider_context_compaction_once_and_persists_its_transcript_marker() {
    let setup = setup();
    setup.provider.compact.store(true, Ordering::SeqCst);
    let command = json!({ "type": "compact", "commandId": "compact", "sessionId": setup.id });
    let receipt = setup.engine.command(&command).unwrap();
    assert_eq!(setup.engine.command(&command).unwrap(), receipt);
    wait_for("compaction", || {
        setup.provider.compacts.load(Ordering::SeqCst) == 1
    });
    assert!(
        setup
            .session()
            .session
            .blocks
            .iter()
            .any(|block| block.text == "/compact")
    );
    setup.provider.finish(0);
    setup.wait_for_status(HostSessionStatus::Idle);
    assert_eq!(setup.provider.compacts.load(Ordering::SeqCst), 1);
}

#[test]
fn refuses_compaction_for_a_provider_without_it() {
    let setup = setup();
    assert_eq!(
        setup
            .engine
            .command(&json!({ "type": "compact", "commandId": "compact", "sessionId": setup.id }))
            .unwrap_err(),
        "Context compaction is unavailable for this provider"
    );
}

#[test]
fn persists_model_and_permission_changes_for_the_next_turn_and_rejects_changes_mid_turn() {
    let setup = setup();
    let change = json!({
        "type": "configure", "commandId": "settings", "sessionId": setup.id, "model": "codex:new",
        "modelSettings": { "reasoningEffort": "high" }, "runtimeMode": "full-access",
    });
    let receipt = setup.engine.command(&change).unwrap();
    assert_eq!(setup.engine.command(&change).unwrap(), receipt);
    let session = setup.session().session.clone();
    assert_eq!(session.model, "codex:new");
    assert_eq!(session.model_settings["reasoningEffort"], "high");
    assert_eq!(session.runtime_mode, monocode_core::RuntimeMode::FullAccess);
    setup.send("turn", "Continue").unwrap();
    setup.wait_for_turns(1);
    let input = setup.provider.input(0);
    assert_eq!(input.session.model, "codex:new");
    assert_eq!(
        input.session.model_settings.unwrap()["reasoningEffort"],
        "high"
    );
    assert_eq!(
        input.session.runtime_mode,
        monocode_core::RuntimeMode::FullAccess
    );
    let mut later = change.clone();
    later["commandId"] = json!("later");
    assert!(
        setup
            .engine
            .command(&later)
            .unwrap_err()
            .contains("current turn")
    );
    setup.provider.finish(0);
}

#[test]
fn keeps_working_with_no_client_persists_output_and_deduplicates_a_lost_acknowledgement() {
    let setup = setup();
    let command = json!({ "type": "send", "commandId": "send-once", "sessionId": setup.id, "text": "Do the work" });
    let receipt = setup.engine.command(&command).unwrap();
    assert_eq!(setup.engine.command(&command).unwrap(), receipt);
    setup.wait_for_turns(1);
    assert_eq!(setup.provider.sends.load(Ordering::SeqCst), 1);
    let before = setup.session().revision;
    setup.provider.emit(
        0,
        json!({ "type": "session.providerBound", "providerSessionId": "provider-thread" }),
    );
    setup.provider.emit(
        0,
        json!({ "type": "message.delta", "text": "still working while disconnected" }),
    );
    setup.provider.finish(0);
    setup.wait_for_status(HostSessionStatus::Idle);
    assert!(
        setup
            .session()
            .session
            .blocks
            .iter()
            .any(|block| block.text.contains("still working"))
    );
    let events = setup.store.events(&setup.id, before).unwrap();
    assert!(events.events.unwrap().len() > 1);
    assert_eq!(setup.engine.command(&command).unwrap(), receipt);
    assert_eq!(setup.provider.sends.load(Ordering::SeqCst), 1);
    wait_for("bind", || !setup.provider.binds.lock().is_empty());
    let bound = setup.provider.binds.lock()[0].clone();
    assert_eq!(
        (bound.0.as_str(), bound.1.as_str()),
        (setup.id.as_str(), "provider-thread")
    );
    let mut changed = command.clone();
    changed["text"] = json!("Changed payload");
    assert!(
        setup
            .engine
            .command(&changed)
            .unwrap_err()
            .contains("different payload")
    );
}

#[test]
fn keeps_turn_timing_and_model_provenance_after_settlement_and_reconnect() {
    for harness in [HarnessId::Codex, HarnessId::Claude] {
        let setup = setup_with(harness, None);
        setup.send("first-turn", "Inspect the project").unwrap();
        setup.wait_for_turns(1);
        let running = setup.session();
        let first = &running.session.blocks[0];
        assert_eq!(first.id, "first-turn");
        assert!(first.started_at.is_some());
        let model = first.turn_model.as_ref().unwrap();
        assert_eq!(
            (model.harness, model.id.clone()),
            (harness, format!("{}:test", harness.as_str()))
        );
        setup
            .provider
            .emit(0, json!({ "type": "message.delta", "text": "Found it" }));
        setup.provider.finish(0);
        setup.wait_for_status(HostSessionStatus::Idle);

        let reconnected = serde_json::to_value(setup.store.sync(&setup.id, None).unwrap()).unwrap();
        assert_eq!(reconnected["kind"], "snapshot");
        let blocks = &reconnected["value"]["session"]["blocks"];
        assert_eq!(blocks[0]["id"], "first-turn");
        assert!(blocks[0]["startedAt"].is_i64());
        assert!(blocks[0]["durationMs"].as_i64().unwrap() >= 0);
        assert_eq!(
            blocks[0]["turnModel"]["id"],
            format!("{}:test", harness.as_str())
        );
        assert_eq!(blocks[1]["text"], "Found it");
        let delta =
            serde_json::to_value(setup.store.sync(&setup.id, Some(running.revision)).unwrap())
                .unwrap();
        assert_eq!(delta["kind"], "delta");
        assert!(
            delta["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| block["id"] == "first-turn" && block["durationMs"].is_i64())
        );
    }
}

#[test]
fn serializes_concurrent_sends_and_accepts_only_one_approval_decision_for_a_run() {
    let setup = setup();
    setup.send("send", "Work").unwrap();
    assert!(
        setup
            .send("other-send", "More work")
            .unwrap_err()
            .contains("already running")
    );
    setup.wait_for_turns(1);
    setup.provider.emit(
        0,
        json!({ "type": "approval.requested", "requestId": 7, "title": "Run a command?" }),
    );
    let run_id = setup.session().run_id.clone().unwrap();
    let approval = json!({
        "type": "approve", "commandId": "approval-1", "sessionId": setup.id, "runId": run_id,
        "requestId": 7, "decision": "allow",
    });
    let mut stale = approval.clone();
    stale["runId"] = json!("stale");
    assert!(
        setup
            .engine
            .command(&stale)
            .unwrap_err()
            .contains("finished or replaced")
    );
    setup.engine.command(&approval).unwrap();
    setup.engine.command(&approval).unwrap();
    let mut other = approval.clone();
    other["commandId"] = json!("approval-2");
    other["decision"] = json!("deny");
    assert!(
        setup
            .engine
            .command(&other)
            .unwrap_err()
            .contains("already resolved")
    );
    assert_eq!(
        *setup.provider.approvals.lock(),
        vec![(setup.id.clone(), 7, CoreDecision::Allow)]
    );
}

#[test]
fn stores_pending_questions_and_rejects_a_second_devices_stale_answer() {
    let setup = setup();
    setup.send("send", "Work").unwrap();
    setup.wait_for_turns(1);
    setup.provider.emit(
        0,
        json!({
            "type": "question.asked", "requestId": 3,
            "questions": [{
                "id": "q1", "prompt": "Choose", "multiSelect": false, "allowCustom": false,
                "options": [{ "id": "yes", "label": "Yes" }],
            }],
        }),
    );
    assert!(setup.session().session.pending_question.is_some());
    let reply = json!({
        "type": "answer", "commandId": "answer", "sessionId": setup.id,
        "runId": setup.session().run_id, "requestId": 3,
        "reply": { "kind": "answered", "answers": { "q1": ["yes"] } },
    });
    setup.engine.command(&reply).unwrap();
    assert!(setup.session().session.pending_question.is_none());
    let mut other = reply.clone();
    other["commandId"] = json!("other-answer");
    assert!(
        setup
            .engine
            .command(&other)
            .unwrap_err()
            .contains("already resolved")
    );
    assert_eq!(setup.provider.answers.load(Ordering::SeqCst), 1);
}

#[test]
fn recovers_interrupted_durable_state_without_replaying_an_uncertain_provider_send() {
    let setup = setup();
    let value = setup.session();
    let mut running = (*value).clone();
    running.revision += 1;
    running.status = HostSessionStatus::Running;
    running.run_id = Some("old-run".into());
    running.session.busy = Some(true);
    running.session.provider_session_id = Some("retained".into());
    let mut block = Block::new("interrupted-turn", BlockRole::User, "Work");
    block.started_at = Some(value.updated_at - 2_000);
    running.session.blocks = vec![block];
    setup
        .store
        .transaction(|| setup.store.save(running, &json!({ "type": "accepted" })))
        .unwrap();
    let provider = Arc::new(Fake::default());
    let recovered = engine_over(
        setup.store.clone(),
        provider.clone(),
        &[HarnessId::Codex],
        &setup.runtime,
    );
    let session = setup.session();
    assert_eq!(session.status, HostSessionStatus::Interrupted);
    assert_eq!(session.session.busy, Some(false));
    assert_eq!(session.session.blocks[0].duration_ms, Some(2_000));
    assert_eq!(provider.sends.load(Ordering::SeqCst), 0);
    assert_eq!(
        provider.binds.lock()[0],
        (
            setup.id.clone(),
            "retained".to_string(),
            value.session.cwd.clone()
        )
    );
    recovered.close();
}

#[test]
fn retries_a_failed_event_write_and_settles_the_stopped_turn() {
    let setup = setup();
    setup.send("send", "Work").unwrap();
    setup.wait_for_turns(1);
    let mut failed = false;
    setup
        .engine
        .set_save_fault(Some(Box::new(move |event: &Value| {
            if !failed && event["type"] == "events" {
                failed = true;
                return true;
            }
            false
        })));
    setup.provider.emit(
        0,
        json!({ "type": "message.delta", "text": "Retained output" }),
    );
    setup.provider.finish(0);
    wait_for_within("interrupted", Duration::from_secs(4), || {
        setup.session().status == HostSessionStatus::Interrupted
    });
    let session = setup.session();
    assert!(
        session
            .session
            .blocks
            .iter()
            .any(|block| block.text == "Retained output")
    );
    assert_eq!(session.session.busy, Some(false));
}

#[test]
fn retries_a_failed_final_settlement() {
    let setup = setup();
    setup.send("send", "Work").unwrap();
    setup.wait_for_turns(1);
    let mut failed = false;
    setup
        .engine
        .set_save_fault(Some(Box::new(move |event: &Value| {
            if !failed && event["type"] == "settled" {
                failed = true;
                return true;
            }
            false
        })));
    setup.provider.finish(0);
    wait_for_within("interrupted", Duration::from_secs(4), || {
        setup.session().status == HostSessionStatus::Interrupted
    });
    assert_eq!(setup.session().session.busy, Some(false));
    wait_for("settled bookkeeping", || {
        !setup.engine.is_running(&setup.id)
    });
}

#[test]
fn batches_streamed_output_and_syncs_only_changed_blocks() {
    let setup = setup();
    setup.send("send", "Work").unwrap();
    setup.wait_for_turns(1);
    let started = setup.session().revision;
    for index in 0..50 {
        setup.provider.emit(
            0,
            json!({ "type": "message.delta", "text": format!("chunk {index} ") }),
        );
    }
    assert_eq!(setup.session().revision, started);
    wait_for("batched write", || setup.session().revision == started + 1);
    let sync = serde_json::to_value(setup.store.sync(&setup.id, Some(started)).unwrap()).unwrap();
    assert_eq!(sync["kind"], "delta");
    assert_eq!(sync["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(sync["blocks"][0]["role"], "assistant");
    assert_eq!(sync["blockIds"].as_array().unwrap().len(), 2);

    setup.provider.emit(
        0,
        json!({ "type": "approval.requested", "requestId": 1, "title": "Run a command?" }),
    );
    assert_eq!(setup.session().revision, started + 2);
    let streamed = setup.session().revision;
    setup.provider.finish(0);
    setup.wait_for_status(HostSessionStatus::Idle);
    let settled =
        serde_json::to_value(setup.store.sync(&setup.id, Some(streamed)).unwrap()).unwrap();
    assert_eq!(settled["kind"], "delta");
    assert!(
        settled["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["role"] == "user" && block["durationMs"].is_i64())
    );
    let revision = setup.session().revision;
    let unchanged =
        serde_json::to_value(setup.store.sync(&setup.id, Some(revision)).unwrap()).unwrap();
    assert_eq!(unchanged["kind"], "unchanged");
    let snapshot = serde_json::to_value(setup.store.sync(&setup.id, None).unwrap()).unwrap();
    assert_eq!(snapshot["kind"], "snapshot");
    let summary = &setup.store.summaries(&setup.project.id).unwrap()[0];
    assert_eq!(summary.id, setup.id);
    assert_eq!(summary.status, HostSessionStatus::Idle);
    assert_eq!(summary.title, "codex · Work");
}

#[test]
fn requires_snapshot_recovery_when_the_clients_event_cursor_is_invalid() {
    let setup = setup();
    assert_eq!(
        setup
            .store
            .events(&setup.id, 100_000)
            .unwrap()
            .snapshot
            .unwrap()
            .session
            .id,
        setup.id
    );
}

#[test]
fn reports_a_cancelled_turn_and_rejects_commands_after_close() {
    let setup = setup();
    setup.send("send", "Work").unwrap();
    setup.wait_for_turns(1);
    let run_id = setup.session().run_id.clone().unwrap();
    setup
        .engine
        .command(&json!({ "type": "cancel", "commandId": "cancel", "sessionId": setup.id, "runId": run_id }))
        .unwrap();
    setup.wait_for_status(HostSessionStatus::Idle);
    let last = setup.session().session.blocks.last().cloned().unwrap();
    assert_eq!(
        (last.role, last.text.as_str()),
        (BlockRole::System, "Stopped by you.")
    );
    setup.engine.close();
    assert_eq!(setup.send("after", "Work").unwrap_err(), "Host is stopping");
}

mod worktrees {
    use super::*;
    use crate::git_branches::tests::run_git;
    use crate::git_worktrees::{create_host_worktree, host_worktrees};

    fn repository() -> (tempfile::TempDir, String) {
        let directory = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(directory.path()).unwrap();
        let git = |args: &[&str]| run_git(&root, args);
        git(&["init", "-q"]);
        git(&["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join("file.txt"), "initial\n").unwrap();
        git(&["add", "file.txt"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "initial",
        ]);
        (directory, root.to_string_lossy().into_owned())
    }

    /// git-worktrees.test.ts: "applies generated worktree names only to
    /// retained sessions".
    #[test]
    fn applies_generated_worktree_names_only_to_retained_sessions() {
        for deleted in [false, true] {
            let (_repository, cwd) = repository();
            let setup = setup_with(HarnessId::Codex, Some(&cwd));
            let (finish_title, title) = async_channel::bounded(1);
            let (finish_branch, branch) = async_channel::bounded(1);
            *setup.provider.title.lock() = Some(title);
            *setup.provider.branch.lock() = Some(branch);
            let root = host_worktrees(&cwd).unwrap().default_root;
            let tree = create_host_worktree(
                &cwd,
                Some(&json!("mc/12345678")),
                Some(&json!("HEAD")),
                Some(&json!(false)),
                None,
            )
            .unwrap();
            let created = setup
                .engine
                .command(&json!({
                    "type": "create", "commandId": "create-named", "projectId": setup.project.id,
                    "worktreeCwd": tree.path, "autoWorktreeBranch": tree.branch,
                    "harness": "codex", "model": "codex:test", "runtimeMode": "supervised",
                }))
                .unwrap();
            let id = created.session_id;
            let session = |id: &str| setup.store.session(id).unwrap();
            setup
                .engine
                .command(&json!({
                    "type": "send", "commandId": "first", "sessionId": id,
                    "text": "Fix remote session and worktree naming",
                }))
                .unwrap();
            wait_for("branch request", || {
                setup.provider.branches.load(Ordering::SeqCst) == 1
            });
            setup.wait_for_turns(1);
            let branch_of = || {
                host_worktrees(&cwd)
                    .unwrap()
                    .worktrees
                    .into_iter()
                    .find(|item| item.path == tree.path)
                    .and_then(|item| item.branch)
            };
            if deleted {
                setup.provider.finish(0);
                wait_for("idle", || session(&id).status == HostSessionStatus::Idle);
                setup.store.delete_session(&id).unwrap();
                smol::block_on(finish_branch.send("remote-naming".into())).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                assert_eq!(branch_of().as_deref(), Some("mc/12345678"));
                drop(finish_title);
                std::fs::remove_dir_all(&root).unwrap();
                continue;
            }
            smol::block_on(finish_title.send(GeneratedSessionTitle {
                title: "Fix remote naming".into(),
                work_item: None,
            }))
            .unwrap();
            smol::block_on(finish_branch.send("remote-naming".into())).unwrap();
            wait_for("generated names", || {
                let value = session(&id);
                value.session.title == "codex · Fix remote naming"
                    && value.session.branch.as_deref() == Some("mc/remote-naming")
            });
            assert_eq!(
                session(&id).session.worktree_cwd.as_deref(),
                Some(tree.path.as_str())
            );
            assert!(session(&id).auto_worktree_branch.is_none());
            assert_eq!(branch_of().as_deref(), Some("mc/remote-naming"));
            assert_eq!(setup.provider.titles.load(Ordering::SeqCst), 1);
            setup.provider.finish(0);
            wait_for("idle", || session(&id).status == HostSessionStatus::Idle);
            setup
                .engine
                .command(&json!({ "type": "send", "commandId": "second", "sessionId": id, "text": "More work" }))
                .unwrap();
            wait_for("second turn", || setup.provider.turn_count() == 2);
            setup.provider.finish(1);
            wait_for("idle", || session(&id).status == HostSessionStatus::Idle);
            assert_eq!(setup.provider.titles.load(Ordering::SeqCst), 1);
            assert_eq!(setup.provider.branches.load(Ordering::SeqCst), 1);
            assert!(
                rename_host_worktree_branch(&cwd, &tree.path, "mc/12345678", "mc/other", &|| true)
                    .unwrap_err()
                    .contains("changed")
            );
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    /// git-worktrees.test.ts: "creates registered host worktrees and binds
    /// new sessions to the selected checkout", the session half.
    #[test]
    fn binds_new_sessions_to_the_selected_checkout() {
        let (_repository, cwd) = repository();
        let setup = setup_with(HarnessId::Codex, Some(&cwd));
        let root = host_worktrees(&cwd).unwrap().default_root;
        let tree = create_host_worktree(
            &cwd,
            Some(&json!("feature/task")),
            Some(&json!("HEAD")),
            Some(&json!(false)),
            None,
        )
        .unwrap();
        let receipt = setup
            .engine
            .command(&json!({
                "type": "create", "commandId": "worktree-session", "projectId": setup.project.id,
                "worktreeCwd": tree.path, "harness": "codex", "model": "codex:test",
                "runtimeMode": "supervised",
            }))
            .unwrap();
        assert_eq!(
            setup
                .store
                .session(&receipt.session_id)
                .unwrap()
                .session
                .cwd,
            tree.path
        );
        assert!(
            setup
                .engine
                .command(&json!({
                    "type": "create", "commandId": "outside", "projectId": setup.project.id,
                    "worktreeCwd": std::env::temp_dir(), "harness": "codex", "model": "codex:test",
                    "runtimeMode": "supervised",
                }))
                .unwrap_err()
                .contains("Choose an available worktree")
        );
        let _ = setup.directory.path();
        std::fs::remove_dir_all(root).unwrap();
    }
}
