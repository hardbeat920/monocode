//! UI invariants around the host-owned conversation and the next local draft.
use super::*;
use gpui::TestAppContext;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Mutex;

fn conversation(cx: &mut TestAppContext) -> Entity<Conversation> {
    let store = monocode_core::store::Store::open(Path::new(":memory:")).unwrap();
    let mut summary = store.create("codex", "/test-project").unwrap();
    summary.cwd = "remote://remote-ui-tests/home/me/project".into();
    cx.update(|cx| {
        cx.new(|cx| {
            let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
            view.loaded = true;
            view.remote_state.online = true;
            view.remote_state.capabilities =
                ["sessions.draft", "sessions.plan", "attachments.upload"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
            view.input.update(cx, |input, cx| {
                input.set_text("Keep this next message".into(), cx)
            });
            view
        })
    })
}

#[gpui::test]
fn offline_pending_and_busy_guards_preserve_typed_message(cx: &mut TestAppContext) {
    let view = conversation(cx);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            for (loaded, online, pending, busy, expected) in [
                (
                    false,
                    true,
                    false,
                    false,
                    "Wait for this remote conversation",
                ),
                (true, false, false, false, "machine is offline"),
                (true, true, true, false, "command needs confirmation"),
                (true, true, false, true, "remote turn to finish"),
            ] {
                view.loaded = loaded;
                view.remote_state.online = online;
                view.remote_state.pending = pending;
                view.busy = busy;
                view.send(cx);
                assert!(view.status.contains(expected), "{}", view.status);
                assert_eq!(view.input.read(cx).content, "Keep this next message");
                assert!(view.follow_ups.state.messages.is_empty());
                assert!(view.transcript.read(cx).source_rows().is_empty());
            }
            view.busy = false;
        })
    });
}

#[gpui::test]
fn remote_capability_and_command_guards_keep_composer_and_attachments(cx: &mut TestAppContext) {
    let view = conversation(cx);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.remote_state.capabilities.clear();
            view.attachments = vec![PathBuf::from("/home/me/design.png")];
            assert!(view.remote_send_guard(cx));
            assert!(view.status.contains("attachments"));
            assert_eq!(view.attachments.len(), 1);
            view.attachments.clear();
            view.settings.plan = true;
            assert!(view.remote_send_guard(cx));
            assert!(view.status.contains("Plan mode"));
            view.settings.plan = false;
            view.follow_ups.draft_selected = true;
            assert!(view.remote_send_guard(cx));
            assert!(view.status.contains("save drafts"));
            view.follow_ups.draft_selected = false;
            for command in [
                "/operator delegate this",
                "/orchestrate a task",
                "/add-to-folder name",
            ] {
                view.input
                    .update(cx, |input, cx| input.set_text(command.into(), cx));
                assert!(view.remote_send_guard(cx));
                assert_eq!(view.input.read(cx).content.as_ref(), command);
            }
        })
    });
}

#[gpui::test]
fn restoring_host_saved_draft_preserves_the_next_typed_message(cx: &mut TestAppContext) {
    let view = conversation(cx);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.install_remote_draft(&json!({"session": {"blocks": [
            {"id": "saved-remote-draft", "role": "user", "draft": true, "text": "Saved on the host"}
        ]}}), cx);
            let draft = view.follow_ups.state.draft.as_ref().unwrap();
            assert_eq!(draft.id, "saved-remote-draft");
            assert_eq!(draft.text, "Saved on the host");
            assert_eq!(view.input.read(cx).content, "Keep this next message");
            view.install_remote_draft(&json!({"session": {"blocks": []}}), cx);
            assert!(view.follow_ups.state.draft.is_none());
            assert_eq!(view.input.read(cx).content, "Keep this next message");
        })
    });
}

#[gpui::test]
fn dropping_remote_conversation_does_not_cancel_its_host_turn(cx: &mut TestAppContext) {
    let view = conversation(cx);
    let cancel = cx.update(|cx| view.read(cx).cancel.clone());
    cx.update(|_| drop(view));
    cx.run_until_parked();
    assert!(!cancel.load(Ordering::Relaxed));

    let local = conversation(cx);
    let local_cancel = cx.update(|cx| {
        local.update(cx, |view, _| {
            view.summary.cwd = "/local/project".into();
            view.cancel.clone()
        })
    });
    cx.update(|_| drop(local));
    cx.run_until_parked();
    assert!(local_cancel.load(Ordering::Relaxed));
}

struct HostFixture {
    client: Arc<monocode_core::remote::Connections>,
    store: Database,
    summary: monocode_core::store::Session,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
    directory: PathBuf,
    /// A running host turn goes idle after this many syncs.
    idle_after: Arc<std::sync::atomic::AtomicUsize>,
    /// From this sync on (0: never) the host reports a title it chose itself.
    rename_at: Arc<std::sync::atomic::AtomicUsize>,
}
impl HostFixture {
    fn new(running: bool, saved_draft: bool) -> Self {
        use std::io::{BufRead, Read, Write};
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let environment = format!("remote-ui-{}-{nonce}", std::process::id());
        let directory = std::env::temp_dir().join(&environment);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let descriptor = json!({"protocolVersion": 1, "environmentId": environment, "name": "UI test host", "providers": ["codex"], "capabilities": ["sessions.draft", "sessions.plan", "attachments.upload", "sessions.sync"]});
        let snapshot = json!({"projectId": "project", "revision": 1, "status": if running {"running"} else {"idle"}, "runId": if running {json!("host-run")} else {serde_json::Value::Null}, "updatedAt": 42,
            "session": {"id": "host-session", "title": "Conversation on host", "harness": "codex", "model": "codex:gpt-test", "runtimeMode": "supervised", "modelSettings": {}, "blocks": if saved_draft { json!([{ "id": "host-saved-draft", "role": "user", "draft": true, "text": "Saved host draft" }]) } else {json!([])} }
        });
        let initial = snapshot.clone();
        let requests = Arc::new(Mutex::new(vec![]));
        let received = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let idle_after = Arc::new(std::sync::atomic::AtomicUsize::new(1));
        let idles = idle_after.clone();
        let rename_at = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let renames = rename_at.clone();
        let server = std::thread::spawn(move || {
            let mut snapshot = snapshot;
            let mut syncs = 0;
            while !done.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("host accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                received.lock().unwrap().push(request.clone());
                let result = match request["method"].as_str().unwrap() {
                    "environment.describe" => descriptor.clone(),
                    "sessions.sync" => {
                        syncs += 1;
                        let renamed = renames.load(Ordering::Relaxed);
                        if renamed > 0 && syncs >= renamed {
                            snapshot["session"]["title"] = json!("Named by the host");
                        }
                        if running && syncs > idles.load(Ordering::Relaxed) {
                            snapshot["status"] = json!("idle");
                            snapshot["runId"] = serde_json::Value::Null;
                            snapshot["revision"] = json!(2);
                        }
                        json!({"kind": "snapshot", "value": snapshot})
                    }
                    "commands.dispatch" => {
                        let command = &request["params"];
                        match host_rejects(command, &snapshot) {
                            Some(error) => json!({"error": error}),
                            None => {
                                // As a host does, a sent message becomes a transcript block.
                                if command["type"] == "send"
                                    && command["draftBlockId"].is_null()
                                    && let Some(blocks) =
                                        snapshot["session"]["blocks"].as_array_mut()
                                {
                                    blocks.push(json!({"id": command["commandId"], "role": "user", "text": command["text"]}));
                                }
                                json!({"commandId": command["commandId"], "sessionId": "host-session", "revision": 2})
                            }
                        }
                    }
                    _ => json!({}),
                };
                let body = match result.get("error") {
                    Some(_) => result.to_string(),
                    None => json!({"result": result}).to_string(),
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let client = Arc::new(monocode_core::remote::Connections::new(&directory));
        client.connect(String::new(), url, "a".repeat(43)).unwrap();
        let store = monocode_core::store::Store::open(Path::new(":memory:")).unwrap();
        let project = store
            .remember_remote_project(
                &environment,
                &json!({"id": "project", "cwd": "/home/me/project", "name": "project"}),
            )
            .unwrap();
        let summary = store.register_remote_summary(&project, &initial).unwrap();
        Self {
            client,
            store: Arc::new(Mutex::new(store)),
            summary,
            requests,
            stop,
            server: Some(server),
            directory,
            idle_after,
            rename_at,
        }
    }
    fn conversation(&self, cx: &mut TestAppContext) -> Entity<Conversation> {
        cx.update(|cx| {
            cx.new(|cx| {
                let mut view = Conversation::new(self.summary.clone(), self.store.clone(), cx);
                view.remote_state.client = Some(self.client.clone());
                view.input.update(cx, |input, cx| {
                    input.set_text("Keep this next message".into(), cx)
                });
                view.load_remote(cx);
                view
            })
        })
    }
    fn wait(&self, view: &Entity<Conversation>, cx: &mut TestAppContext, settled: bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if cx.update(|cx| {
                let view = view.read(cx);
                view.loaded && (!settled || (!view.busy && !view.remote_state.refreshing))
            }) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        cx.update(|cx| {
            panic!(
                "remote conversation did not settle: {}",
                view.read(cx).status
            )
        });
    }
}
/// The commands `host/engine.ts` refuses (`parseCommand`, and the engine's
/// draft lookups) that a desktop could plausibly send here.
fn host_rejects(command: &serde_json::Value, snapshot: &serde_json::Value) -> Option<&'static str> {
    let text = |key: &str| command[key].as_str().is_some_and(|v| !v.trim().is_empty());
    if !text("commandId") {
        return Some("Invalid command ID");
    }
    if command["type"] == "create" {
        // REMOTE_PROVIDERS and RUNTIME_MODES.
        let providers = [
            "codex",
            "claude",
            "cursor",
            "grok",
            "opencode",
            "pi",
            "omp",
            "fx",
            "hermes",
            "antigravity",
            "devin",
        ];
        let modes = ["supervised", "auto-accept-edits", "auto", "full-access"];
        if !command["harness"]
            .as_str()
            .is_some_and(|h| providers.contains(&h))
            || !command["runtimeMode"]
                .as_str()
                .is_some_and(|m| modes.contains(&m))
        {
            return Some("Invalid provider or permission mode");
        }
        if !text("projectId") {
            return Some("Invalid project ID");
        }
        if !text("model") {
            return Some("Invalid model");
        }
        return None;
    }
    if command["sessionId"] != "host-session" {
        return Some("Invalid session ID");
    }
    let has_draft = |id: &serde_json::Value| {
        snapshot["session"]["blocks"]
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|b| &b["id"] == id && b["draft"] == true))
    };
    match command["type"].as_str() {
        Some("send" | "draft") => {
            let attachments = command["attachments"]
                .as_array()
                .is_some_and(|a| !a.is_empty());
            let names_draft = command["type"] == "send" && !command["draftBlockId"].is_null();
            if !command["text"].is_string() || (!text("text") && !attachments && !names_draft) {
                return Some("Invalid prompt");
            }
            if names_draft && !has_draft(&command["draftBlockId"]) {
                return Some("Draft not found");
            }
            None
        }
        Some("removeDraft") if !has_draft(&command["draftBlockId"]) => Some("Draft not found"),
        _ => None,
    }
}

impl Drop for HostFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let result = server.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[gpui::test]
fn snapshot_load_restores_host_draft_without_replacing_unsent_composer(cx: &mut TestAppContext) {
    let host = HostFixture::new(false, true);
    let view = host.conversation(cx);
    host.wait(&view, cx, true);
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.remote_state.online);
        assert_eq!(view.summary.title, "Conversation on host");
        assert_eq!(
            view.follow_ups.state.draft.as_ref().unwrap().text,
            "Saved host draft"
        );
        assert_eq!(view.input.read(cx).content, "Keep this next message");
    });
    assert!(
        !host
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r["method"] == "commands.dispatch")
    );
}

/// The GPUI test executor runs the remote observer inline, so the host itself
/// scripts the turn: it renames the conversation on sync 3 and goes idle on 4.
#[gpui::test]
fn a_host_title_change_reaches_the_busy_view_without_a_rename_request(cx: &mut TestAppContext) {
    let host = HostFixture::new(true, false);
    host.idle_after.store(3, Ordering::Relaxed);
    host.rename_at.store(3, Ordering::Relaxed);
    let view = host.conversation(cx);
    let seen = Arc::new(Mutex::new(Vec::<(bool, String)>::new()));
    let log = seen.clone();
    let _changes = cx.update(|cx| {
        cx.subscribe(&view, move |view, _: &SessionChanged, cx| {
            let view = view.read(cx);
            log.lock()
                .unwrap()
                .push((view.busy, view.summary.title.to_string()));
        })
    });
    host.wait(&view, cx, true);
    cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(view.summary.title, "Named by the host");
        assert_eq!(view.display_title(), "Named by the host");
    });
    let seen = seen.lock().unwrap();
    assert!(
        seen.contains(&(true, "Named by the host".to_owned())),
        "chrome must be told while the turn is still running: {seen:?}"
    );
    let requests = host.requests.lock().unwrap();
    assert!(
        !requests.iter().any(|r| matches!(
            r["method"].as_str(),
            Some("sessions.update" | "commands.dispatch")
        )),
        "a title from the host must never be written back: {requests:?}"
    );
    drop(requests);
    assert_eq!(
        host.store
            .lock()
            .unwrap()
            .remote_session(host.summary.id)
            .unwrap()
            .unwrap()
            .summary
            .title,
        "Named by the host"
    );
}

#[gpui::test]
fn opening_running_host_reattaches_without_dispatching_another_turn(cx: &mut TestAppContext) {
    let host = HostFixture::new(true, false);
    let view = host.conversation(cx);
    host.wait(&view, cx, true);
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.remote_state.online);
        assert_eq!(view.input.read(cx).content, "Keep this next message");
        assert_eq!(
            view.remote_state.snapshot.as_ref().unwrap()["status"],
            "idle"
        );
        assert!(!view.cancel.load(Ordering::Relaxed));
    });
    let requests = host.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .filter(|r| r["method"] == "sessions.sync")
            .count()
            >= 2
    );
    assert!(!requests.iter().any(|r| r["method"] == "commands.dispatch"));
}

#[gpui::test]
fn explicit_retry_reuses_pending_command_and_preserves_new_composer_and_attachment(
    cx: &mut TestAppContext,
) {
    let host = HostFixture::new(false, false);
    let view = host.conversation(cx);
    host.wait(&view, cx, true);
    {
        let store = host.store.lock().unwrap();
        let mut remote = store.remote_session(host.summary.id).unwrap().unwrap();
        remote.outbox.push(json!({"type": "send", "commandId": "original-accepted-command", "sessionId": "host-session", "text": "Earlier message"}));
        store.save_remote_session(&remote).unwrap();
    }
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.remote_state.pending = true;
            view.attachments = vec![PathBuf::from("/unsent/new-attachment.png")];
            view.retry_remote(cx);
            assert_eq!(view.input.read(cx).content, "Keep this next message");
            assert_eq!(
                view.attachments,
                [PathBuf::from("/unsent/new-attachment.png")]
            );
        })
    });
    host.wait(&view, cx, true);
    let requests = host.requests.lock().unwrap();
    let dispatched: Vec<_> = requests
        .iter()
        .filter(|r| r["method"] == "commands.dispatch")
        .collect();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(
        dispatched[0]["params"]["commandId"],
        "original-accepted-command"
    );
    assert_eq!(dispatched[0]["params"]["text"], "Earlier message");
    assert!(!requests.iter().any(|r| r["method"] == "attachments.upload"));
    drop(requests);
    cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(view.input.read(cx).content, "Keep this next message");
        assert_eq!(view.attachments.len(), 1);
    });
}

#[gpui::test]
fn rejected_message_restore_preserves_new_draft_then_restores_original_text_and_files(
    cx: &mut TestAppContext,
) {
    let view = conversation(cx);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
        view.remote_state.rejected = Some(json!({
            "error": "Host rejected request: permission denied",
            "commands": [
                {"type": "configure", "commandId": "configure-original"},
                {"type": "send", "commandId": "send-original", "text": "Original rejected message"}
            ],
            "uploads": [{"commandId": "send-original", "path": "/original/rejected.png"}]
        }));
        view.attachments = vec![PathBuf::from("/new/unsent.png")];
        view.restore_remote_message(cx);
        assert_eq!(view.input.read(cx).content, "Keep this next message");
        assert_eq!(view.attachments, [PathBuf::from("/new/unsent.png")]);
        assert!(view.status.contains("Clear the composer"));

        view.input.update(cx, |input, cx| input.set_text(String::new(), cx));
        view.attachments.clear();
        view.restore_remote_message(cx);
        assert_eq!(view.input.read(cx).content, "Original rejected message");
        assert_eq!(view.attachments, [PathBuf::from("/original/rejected.png")]);
    })
    });
}

#[gpui::test]
fn ambiguous_legacy_requests_block_send_retry_and_restore_without_consuming_draft(
    cx: &mut TestAppContext,
) {
    let view = conversation(cx);
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.remote_state.rejected = Some(json!({
                "legacyRecovery": true,
                "error": "Open the original desktop to recover older pending requests.",
                "commands": [{"type": "send", "text": "Ambiguous original message"}]
            }));
            view.remote_state.pending = true;
            view.send(cx);
            view.retry_remote(cx);
            view.restore_remote_message(cx);
            assert_eq!(
                view.status,
                "Open the original desktop to recover older pending requests."
            );
            assert_eq!(view.input.read(cx).content, "Keep this next message");
            assert!(!view.busy);
            assert!(view.settings.remote_action.is_none());
            assert!(view.transcript.read(cx).source_rows().is_empty());
        })
    });
}

fn dispatched(host: &HostFixture) -> Vec<serde_json::Value> {
    host.requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request["method"] == "commands.dispatch")
        .map(|request| request["params"].clone())
        .collect()
}

#[gpui::test]
fn an_app_launch_sends_its_prompt_as_the_new_remote_conversations_first_turn(
    cx: &mut TestAppContext,
) {
    let host = HostFixture::new(false, false);
    // As sessions.start leaves it: a blank host handle whose prompt is a local
    // follow-up draft, recoverable if the app exits before the host has it.
    let (summary, settings) = {
        let store = locked(&host.store).unwrap();
        let mut summary = store.create("codex", &host.summary.cwd).unwrap();
        let settings = Settings {
            model: Some("gpt-test".into()),
            ..Default::default()
        };
        store
            .configure_agent(summary.id, "codex", &settings)
            .unwrap();
        store
            .prepare_app_prompt(summary.id, "request", "Report the current branch", false)
            .unwrap();
        summary.has_messages = true;
        (summary, settings)
    };
    assert!(summary.id < 0, "remote handles are negative");
    let view = cx.update(|cx| {
        cx.new(|cx| {
            let mut view = Conversation::new(summary.clone(), host.store.clone(), cx);
            view.remote_state.client = Some(host.client.clone());
            view.install_app_launch(settings, true, cx);
            view
        })
    });
    host.wait(&view, cx, true);
    let commands = dispatched(&host);
    let kinds: Vec<_> = commands
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["create", "send"], "{commands:?}");
    assert_eq!(commands[1]["text"], "Report the current branch");
    // The local draft's ID is not a host block; naming it would be refused.
    assert!(commands[1].get("draftBlockId").is_none(), "{commands:?}");
    cx.update(|cx| {
        let view = view.read(cx);
        // A Mono's completion check waits for a settled child with messages.
        assert!(!view.busy);
        assert!(view.summary.has_messages);
        assert!(view.input.read(cx).content.is_empty());
    });
    let store = locked(&host.store).unwrap();
    assert!(store.follow_ups(summary.id).unwrap().draft.is_none());
    assert_eq!(
        store
            .remote_session(summary.id)
            .unwrap()
            .unwrap()
            .session_id
            .as_deref(),
        Some("host-session")
    );
}

#[gpui::test]
fn a_mono_send_waits_for_the_remote_conversation_to_load(cx: &mut TestAppContext) {
    let host = HostFixture::new(false, false);
    let view = cx.update(|cx| {
        cx.new(|cx| {
            let mut view = Conversation::new(host.summary.clone(), host.store.clone(), cx);
            view.remote_state.client = Some(host.client.clone());
            view.mono_external_send("Check the logs".into(), false, cx)
                .unwrap();
            view
        })
    });
    host.wait(&view, cx, true);
    let commands = dispatched(&host);
    assert!(
        commands
            .iter()
            .any(|c| c["type"] == "send" && c["text"] == "Check the logs"),
        "{commands:?}"
    );
    cx.update(|cx| assert!(!view.read(cx).mono_external_pending()));
}

/// The app quit after `sessions.start` answered `accepted: true` and before
/// the prompt reached the host: the next load of the conversation sends it,
/// once, and a later load does not send it again.
#[gpui::test]
fn a_remote_launch_left_unsent_by_a_restart_is_sent_on_load(cx: &mut TestAppContext) {
    let host = HostFixture::new(false, false);
    let summary = {
        let store = locked(&host.store).unwrap();
        let summary = store.create("codex", &host.summary.cwd).unwrap();
        let settings = Settings {
            model: Some("gpt-test".into()),
            ..Default::default()
        };
        store
            .configure_agent(summary.id, "codex", &settings)
            .unwrap();
        store
            .prepare_app_prompt(summary.id, "request", "Report the current branch", false)
            .unwrap();
        summary
    };
    // No install_app_launch: the view that would have called it is gone.
    let open = |cx: &mut TestAppContext| {
        let view = cx.update(|cx| {
            cx.new(|cx| {
                let mut view = Conversation::new(summary.clone(), host.store.clone(), cx);
                view.remote_state.client = Some(host.client.clone());
                view.load_remote(cx);
                view
            })
        });
        host.wait(&view, cx, true);
        view
    };
    let view = open(cx);
    let commands = dispatched(&host);
    let kinds: Vec<_> = commands
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["create", "send"], "{commands:?}");
    assert_eq!(commands[1]["text"], "Report the current branch");
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.summary.has_messages, "{}", view.status);
        assert!(view.input.read(cx).content.is_empty());
    });
    {
        let store = locked(&host.store).unwrap();
        assert!(store.follow_ups(summary.id).unwrap().draft.is_none());
        assert!(!store.draft_sessions().unwrap().contains(&summary.id));
    }
    drop(view);
    open(cx);
    assert_eq!(dispatched(&host).len(), 2, "the launch was sent again");
}

/// A launch whose commands reached the outbox before the app quit is the
/// outbox's to retry. Its leftover local prompt is dropped, not sent again.
#[gpui::test]
fn a_remote_launch_already_in_the_outbox_is_not_sent_twice(cx: &mut TestAppContext) {
    let host = HostFixture::new(false, false);
    let summary = {
        let store = locked(&host.store).unwrap();
        let summary = store.create("codex", &host.summary.cwd).unwrap();
        store
            .prepare_app_prompt(summary.id, "request", "Report the current branch", false)
            .unwrap();
        let mut remote = store.remote_session(summary.id).unwrap().unwrap();
        remote
            .outbox
            .push(json!({"type":"create","commandId":"journaled"}));
        store.save_remote_session(&remote).unwrap();
        summary
    };
    let view = cx.update(|cx| {
        cx.new(|cx| {
            let mut view = Conversation::new(summary.clone(), host.store.clone(), cx);
            view.remote_state.client = Some(host.client.clone());
            view.load_remote(cx);
            view
        })
    });
    host.wait(&view, cx, true);
    assert!(dispatched(&host).is_empty());
    cx.update(|cx| assert!(view.read(cx).remote_state.pending));
    let store = locked(&host.store).unwrap();
    assert!(store.follow_ups(summary.id).unwrap().draft.is_none());
}

/// A draft launch (`draft: true`) resumed after a restart is saved as a host
/// draft, as it would have been, not sent.
#[gpui::test]
fn a_remote_draft_launch_left_unsent_is_saved_as_a_draft_on_load(cx: &mut TestAppContext) {
    let host = HostFixture::new(false, false);
    let summary = {
        let store = locked(&host.store).unwrap();
        let summary = store.create("codex", &host.summary.cwd).unwrap();
        store
            .configure_agent(
                summary.id,
                "codex",
                &Settings {
                    model: Some("gpt-test".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .prepare_app_prompt(summary.id, "request", "Draft the release notes", true)
            .unwrap();
        summary
    };
    let view = cx.update(|cx| {
        cx.new(|cx| {
            let mut view = Conversation::new(summary.clone(), host.store.clone(), cx);
            view.remote_state.client = Some(host.client.clone());
            view.load_remote(cx);
            view
        })
    });
    host.wait(&view, cx, true);
    let commands = dispatched(&host);
    let kinds: Vec<_> = commands
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["create", "draft"], "{commands:?}");
    assert_eq!(commands[1]["text"], "Draft the release notes");
}
