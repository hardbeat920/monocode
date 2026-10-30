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
                        if running && syncs > 1 {
                            snapshot["status"] = json!("idle");
                            snapshot["runId"] = serde_json::Value::Null;
                            snapshot["revision"] = json!(2);
                        }
                        json!({"kind": "snapshot", "value": snapshot})
                    }
                    "commands.dispatch" => {
                        let command = &request["params"];
                        json!({"commandId": command["commandId"], "sessionId": "host-session", "revision": 2})
                    }
                    _ => json!({}),
                };
                let body = json!({"result": result}).to_string();
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
        }
    }
    fn conversation(&self, cx: &mut TestAppContext) -> Entity<Conversation> {
        self.conversation_typing(cx, "Keep this next message")
    }
    fn conversation_typing(&self, cx: &mut TestAppContext, typed: &str) -> Entity<Conversation> {
        cx.update(|cx| {
            cx.new(|cx| {
                let mut view = Conversation::new(self.summary.clone(), self.store.clone(), cx);
                view.remote_state.client = Some(self.client.clone());
                view.input
                    .update(cx, |input, cx| input.set_text(typed.into(), cx));
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

#[gpui::test]
fn a_host_draft_moves_into_an_empty_composer_and_off_the_host(cx: &mut TestAppContext) {
    let host = HostFixture::new(false, true);
    let view = host.conversation_typing(cx, "");
    host.wait(&view, cx, true);
    cx.update(|cx| {
        assert_eq!(view.read(cx).input.read(cx).content, "Saved host draft");
    });
    assert!(
        host.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r["method"] == "commands.dispatch" && r.to_string().contains("removeDraft"))
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
