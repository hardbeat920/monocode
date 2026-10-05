//! Tests for the remote package: remoteChanges.test.ts,
//! remoteAttachments.test.ts, and remoteCommands.test.ts, plus the entity
//! flows from RemoteSession.tsx and App.tsx against a scripted host.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use base64::Engine as _;
use futures::executor::block_on;
use gpui::{App, Entity, TestAppContext};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::{Attachment, AttachmentKind, Extra, HarnessId, RuntimeMode, Session};
use monocode_remote::host::protocol::HostProject;
use monocode_settings::Kv;
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use super::connections::{PendingScope, pending_remote_command, remember_remote_session};
use super::remote_commands::{NO_RUNNER, decode_remote_binary};
use super::remote_projects::{parse_remote_path, remember_remote_project, remote_path};
use super::testing::{FakeTransport, Reply, machine};
use super::*;
use crate::runtime::sessions::Sessions;
use crate::runtime::testing::init_test_engine;

fn args(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn client_with(transport: &FakeTransport) -> RemoteClient {
    transport.set_machines(vec![machine("machine", "env")]);
    RemoteClient::new(Arc::new(transport.clone()))
}

fn btoa(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

// remoteChanges.test.ts

#[gpui::test]
fn holds_one_wait_per_machine_and_reports_each_batch_of_session_writes(cx: &mut TestAppContext) {
    let transport = FakeTransport::new();
    transport.respond(
        "changes.wait",
        json!({ "boot": "b", "cursor": 4, "sessions": [], "reset": true }),
    );
    transport.respond(
        "changes.wait",
        json!({
            "boot": "b",
            "cursor": 6,
            "sessions": [{ "id": "s1", "projectId": "p", "revision": 3 }],
            "reset": false
        }),
    );
    transport.respond(
        "changes.wait",
        json!({ "boot": "c", "cursor": 0, "sessions": [], "reset": true }),
    );
    let s = setup_with(cx, transport.clone());
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let _listener = cx.update(|cx| {
        cx.subscribe(&s.connections, move |_, event: &RemoteEvent, _| {
            if let RemoteEvent::Changes(detail) = event {
                seen.borrow_mut().push(detail.clone());
            }
        })
    });
    let first = s.connections.update(cx, |connections, cx| {
        connections.watch_remote_changes("machine", cx)
    });
    let second = s.connections.update(cx, |connections, cx| {
        connections.watch_remote_changes("machine", cx)
    });
    cx.run_until_parked();
    let waits = transport.calls_for("changes.wait");
    assert_eq!(waits.len(), 4);
    // The first answer only sets the cursor; the host restart is a reset.
    assert_eq!(
        *events.borrow(),
        vec![
            RemoteChangesDetail {
                machine_id: "machine".into(),
                sessions: vec![
                    serde_json::from_value(json!({ "id": "s1", "projectId": "p", "revision": 3 }))
                        .unwrap()
                ],
                reset: false,
            },
            RemoteChangesDetail {
                machine_id: "machine".into(),
                sessions: Vec::new(),
                reset: true,
            },
        ]
    );
    assert_eq!(
        waits[..3],
        [
            json!({ "after": 0 }),
            json!({ "boot": "b", "after": 4 }),
            json!({ "boot": "b", "after": 6 }),
        ]
    );
    let live = |cx: &mut TestAppContext| {
        s.connections.read_with(cx, |connections, _| {
            connections.remote_changes_live("machine")
        })
    };
    assert!(live(cx));
    drop(first);
    assert!(live(cx));
    drop(second);
    assert!(!live(cx));
}

#[gpui::test]
fn stops_waiting_on_a_host_without_pushed_changes(cx: &mut TestAppContext) {
    let transport = FakeTransport::new();
    transport.set_handler(|_, _, _| {
        Some(Reply::Error(
            "Host rejected request: Unsupported host method".into(),
        ))
    });
    let s = setup_with(cx, transport.clone());
    let _watch = s.connections.update(cx, |connections, cx| {
        connections.watch_remote_changes("old-host", cx)
    });
    cx.run_until_parked();
    assert_eq!(transport.calls_for("changes.wait").len(), 1);
    s.advance(cx, 60_000);
    assert_eq!(transport.calls_for("changes.wait").len(), 1);
    assert!(!s.connections.read_with(cx, |connections, _| {
        connections.remote_changes_live("old-host")
    }));
}

#[gpui::test]
fn backs_off_and_retries_the_change_feed_after_a_failure(cx: &mut TestAppContext) {
    let transport = FakeTransport::new();
    transport.fail("changes.wait", "Machine is unreachable.");
    transport.hold("changes.wait");
    let s = setup_with(cx, transport.clone());
    let _watch = s.connections.update(cx, |connections, cx| {
        connections.watch_remote_changes("machine", cx)
    });
    cx.run_until_parked();
    assert_eq!(transport.calls_for("changes.wait").len(), 1);
    s.advance(cx, 1_999);
    assert_eq!(transport.calls_for("changes.wait").len(), 1);
    s.advance(cx, 1);
    assert_eq!(transport.calls_for("changes.wait").len(), 2);
}

// remoteAttachments.test.ts

#[test]
fn uploads_local_bytes_before_returning_host_attachment_references() {
    let transport = FakeTransport::new();
    transport.set_file("/laptop/sample.txt", &btoa("sample"));
    transport.respond("attachments.upload", json!({ "offset": 6 }));
    let client = client_with(&transport);
    let file = Attachment {
        copy_from_path: None,
        id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
        name: "sample.txt".into(),
        mime_type: "text/plain".into(),
        kind: AttachmentKind::File,
        size: 6,
        path: Some("/laptop/sample.txt".into()),
        data: None,
        preview_url: None,
        extra: Extra::new(),
    };
    let refs = block_on(client.upload_attachments("machine", vec![file.clone()])).unwrap();
    assert_eq!(
        serde_json::to_value(refs).unwrap(),
        json!([{ "id": file.id, "name": file.name, "mimeType": "text/plain", "kind": "file", "size": 6 }])
    );
    assert_eq!(
        transport.calls(),
        vec![(
            "machine".to_string(),
            "attachments.upload".to_string(),
            json!({ "id": file.id, "offset": 0, "size": 6, "data": btoa("sample") }),
        )]
    );
}

// remoteCommands.test.ts

#[test]
fn keeps_local_writes_local_when_their_content_mentions_a_remote_path() {
    let transport = FakeTransport::new();
    let client = client_with(&transport);
    assert!(
        client
            .invoke_workspace(
                "write_text_file",
                &args(
                    json!({ "path": "/home/me/note.txt", "content": "remote://env/home/me/repo" })
                ),
            )
            .is_none()
    );
    assert!(transport.calls().is_empty());
}

#[test]
fn runs_the_same_file_command_on_the_machine_with_host_paths() {
    let transport = FakeTransport::new();
    transport.respond(
        "workspace.run",
        json!([{ "name": "src", "path": "/home/me/repo/src", "isDir": true, "ignored": false }]),
    );
    let client = client_with(&transport);
    let listed = block_on(
        client
            .invoke_workspace(
                "list_dir",
                &args(json!({ "path": "remote://env/home/me/repo" })),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        listed,
        json!([{ "name": "src", "path": "remote://env/home/me/repo/src", "isDir": true, "ignored": false }])
    );
    assert_eq!(
        transport.calls_for("workspace.run"),
        vec![json!({ "command": "list_dir", "args": { "path": "/home/me/repo" } })]
    );
}

#[test]
fn maps_path_results_back_and_leaves_file_contents_alone() {
    let transport = FakeTransport::new();
    let client = client_with(&transport);
    let run = |command: &str, value: Value| {
        block_on(client.run_remote_command(command, args(value))).unwrap()
    };
    transport.respond("workspace.run", json!("remote://not-a-path"));
    assert_eq!(
        run(
            "read_text_file",
            json!({ "path": "remote://env/home/me/a.txt" })
        ),
        json!("remote://not-a-path")
    );
    transport.respond(
        "workspace.run",
        json!([{ "path": "/home/me/a.txt", "mtimeMs": 1 }]),
    );
    let stats = block_on(
        client.stat_files(vec!["remote://env/home/me/a.txt".into()], |_| {
            Box::pin(async { Ok(json!([])) })
        }),
    )
    .unwrap();
    assert_eq!(
        stats,
        vec![json!({ "path": "remote://env/home/me/a.txt", "mtimeMs": 1 })]
    );
    transport.respond("workspace.run", json!("/home/me/b.txt"));
    assert_eq!(
        run(
            "rename_path",
            json!({ "path": "remote://env/home/me/a.txt", "name": "b.txt" })
        ),
        json!("remote://env/home/me/b.txt")
    );
    transport.respond("workspace.run", json!("C:/work/x.ts"));
    assert_eq!(
        run(
            "create_path",
            json!({ "parent": "remote://env/C:/work", "name": "x.ts", "isDir": false })
        ),
        json!("remote://env/C:/work/x.ts")
    );
    assert_eq!(
        transport.calls_for("workspace.run").last(),
        Some(&json!({
            "command": "create_path",
            "args": { "parent": "C:/work", "name": "x.ts", "isDir": false }
        }))
    );
}

#[test]
fn stat_files_merges_local_and_remote_answers_in_the_order_asked() {
    let transport = FakeTransport::new();
    transport.respond(
        "workspace.run",
        json!([{ "path": "/home/me/a.txt", "mtimeMs": 7 }]),
    );
    let client = client_with(&transport);
    let stats = block_on(client.stat_files(
        vec![
            "/local/b.txt".into(),
            "remote://env/home/me/a.txt".into(),
            "/local/c.txt".into(),
        ],
        |paths| {
            assert_eq!(
                paths,
                vec!["/local/b.txt".to_string(), "/local/c.txt".to_string()]
            );
            Box::pin(async { Ok(json!([{ "path": "/local/b.txt", "mtimeMs": 2 }])) })
        },
    ))
    .unwrap();
    assert_eq!(
        stats,
        vec![
            json!({ "path": "/local/b.txt", "mtimeMs": 2 }),
            json!({ "path": "remote://env/home/me/a.txt", "mtimeMs": 7 }),
            json!({ "path": "/local/c.txt", "mtimeMs": null }),
        ]
    );
}

#[test]
fn decodes_remote_binary_reads() {
    let transport = FakeTransport::new();
    transport.respond("workspace.run", json!("AAEC/w=="));
    let client = client_with(&transport);
    let value = block_on(client.run_remote_command(
        "read_binary_file",
        args(json!({ "path": "remote://env/home/me/image.png" })),
    ))
    .unwrap();
    assert_eq!(decode_remote_binary(&value).unwrap(), vec![0, 1, 2, 255]);
}

#[test]
fn preserves_windows_drive_and_unc_paths() {
    assert_eq!(
        parse_remote_path(&remote_path("env", "C:\\work\\repo"))
            .unwrap()
            .host_path,
        "C:/work/repo"
    );
    assert_eq!(
        parse_remote_path(&remote_path("env", "\\\\server\\share\\repo"))
            .unwrap()
            .host_path,
        "//server/share/repo"
    );
}

#[test]
fn adds_remote_paths_to_git_index_entries() {
    let transport = FakeTransport::new();
    transport.respond(
        "workspace.run",
        json!({
            "branch": "main",
            "files": [{ "path": "src/app.ts", "relative": "src/app.ts", "status": "modified" }]
        }),
    );
    let client = client_with(&transport);
    let index = block_on(client.run_remote_command(
        "git_diff_index",
        args(json!({ "cwd": "remote://env/home/me/repo" })),
    ))
    .unwrap();
    assert_eq!(
        index["files"][0]["path"],
        "remote://env/home/me/repo/src/app.ts"
    );
}

#[test]
fn routes_project_search_through_the_host_and_maps_match_paths() {
    let transport = FakeTransport::new();
    transport.respond(
        "workspace.run",
        json!({
            "matches": [{ "path": "/home/me/repo/src/app.ts", "relative": "src/app.ts", "line": 4 }],
            "truncated": false
        }),
    );
    let client = client_with(&transport);
    let search = block_on(
        client
            .invoke_workspace(
                "search_project",
                &args(
                    json!({ "options": { "cwd": "remote://env/home/me/repo", "query": "hello" } }),
                ),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        search["matches"][0]["path"],
        "remote://env/home/me/repo/src/app.ts"
    );
    assert_eq!(search["matches"][0]["line"], 4);
    assert_eq!(
        transport.calls_for("workspace.run"),
        vec![json!({
            "command": "search_project",
            "args": { "options": { "cwd": "/home/me/repo", "query": "hello" } }
        })]
    );
}

#[test]
fn lists_the_skills_installed_on_the_machine_with_remote_paths() {
    let transport = FakeTransport::new();
    transport.respond(
        "workspace.run",
        json!([{
            "name": "quick-plan",
            "description": "Plan",
            "path": "/home/me/.claude/skills/quick-plan/SKILL.md",
            "scope": "user",
            "source": "claude"
        }]),
    );
    let client = client_with(&transport);
    let skills = block_on(client.run_remote_command(
        "list_skills",
        args(json!({ "cwd": "remote://env/home/me/repo", "disabledPaths": [] })),
    ))
    .unwrap();
    assert_eq!(skills[0]["name"], "quick-plan");
    assert_eq!(
        skills[0]["path"],
        "remote://env/home/me/.claude/skills/quick-plan/SKILL.md"
    );
    assert_eq!(
        transport.calls_for("workspace.run"),
        vec![
            json!({ "command": "list_skills", "args": { "cwd": "/home/me/repo", "disabledPaths": [] } })
        ]
    );
}

#[test]
fn refuses_what_the_host_cannot_do_and_explains_outdated_hosts() {
    let transport = FakeTransport::new();
    let client = client_with(&transport);
    let run = |command: &str, value: Value| {
        block_on(client.run_remote_command(command, args(value))).unwrap_err()
    };
    assert!(
        run("reveal_path", json!({ "path": "remote://env/home/me/a" }))
            .contains("isn’t available for projects on another machine")
    );
    assert!(
        run(
            "copy_path",
            json!({ "from": "/Users/me/local.txt", "destParent": "remote://env/home/me" })
        )
        .contains("within one machine")
    );
    assert!(
        run(
            "move_path",
            json!({ "from": "remote://env/home/me/a", "destParent": "remote://other/home/me" })
        )
        .contains("within one machine")
    );
    assert!(
        run("list_dir", json!({ "path": "remote://gone/home/me" })).contains("isn’t connected")
    );
    transport.fail("workspace.run", "Unsupported remote operation");
    assert!(
        run("list_dir", json!({ "path": "remote://env/home/me" })).contains("Update MonoCode Host")
    );
    assert_eq!(transport.calls_for("workspace.run").len(), 1);
}

#[gpui::test]
fn invoke_workspace_without_the_remote_package_explains_the_missing_machine(
    cx: &mut TestAppContext,
) {
    let local =
        cx.update(|cx| invoke_workspace("list_dir", &args(json!({ "path": "/home/me" })), cx));
    assert!(local.is_none());
    let remote = cx
        .update(|cx| {
            invoke_workspace(
                "list_dir",
                &args(json!({ "path": "remote://env/home/me" })),
                cx,
            )
        })
        .unwrap();
    assert_eq!(block_on(remote).unwrap_err(), NO_RUNNER);
}

// Entity flows.

struct Recorder(Rc<RefCell<Vec<String>>>);

impl RemotePeers for Recorder {
    fn announce_finished_later(&self, session_id: &str, _cx: &mut App) {
        self.0.borrow_mut().push(session_id.to_string());
    }
}

struct Setup {
    kv: Kv,
    transport: FakeTransport,
    connections: Entity<RemoteConnections>,
    remote: Entity<RemoteSessions>,
    sessions: Entity<Sessions>,
    clock: Arc<AtomicI64>,
    announced: Rc<RefCell<Vec<String>>>,
}

impl Setup {
    fn advance(&self, cx: &mut TestAppContext, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
        cx.executor()
            .advance_clock(Duration::from_millis(ms as u64));
        cx.run_until_parked();
    }

    fn session(&self, cx: &mut TestAppContext, id: &str) -> Session {
        self.sessions
            .read_with(cx, |sessions, _| sessions.get(id).cloned())
            .unwrap()
    }

    /// Open the tab in a remote project and wait for its first sync.
    fn open(&self, cx: &mut TestAppContext, shell_id: &str) -> Entity<RemoteSession> {
        let shell = self.session(cx, shell_id);
        let tab = self
            .remote
            .update(cx, |remote, cx| remote.open(&shell, true, cx));
        let RemoteTab::Connected(session) = tab else {
            panic!("the tab is not connected");
        };
        cx.run_until_parked();
        session
    }

    fn dispatched(&self) -> Vec<Value> {
        self.transport.calls_for("commands.dispatch")
    }
}

fn setup_with(cx: &mut TestAppContext, transport: FakeTransport) -> Setup {
    init_test_engine(cx);
    let kv = Kv::in_memory();
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let time = clock.clone();
    let announced = Rc::new(RefCell::new(Vec::new()));
    let peers = Rc::new(Recorder(announced.clone()));
    cx.update(|cx| {
        RemoteGlobal::init(
            RemoteConfig {
                transport: Arc::new(transport.clone()),
                kv: kv.clone(),
                clock: Arc::new(move || time.load(Ordering::SeqCst)),
            },
            cx,
        );
        RemoteGlobal::set_peers(cx, peers);
    });
    cx.run_until_parked();
    let (connections, remote) =
        cx.update(|cx| (RemoteGlobal::connections(cx), RemoteGlobal::sessions(cx)));
    let sessions = cx.update(|cx| crate::runtime::engine::Engine::sessions(cx));
    Setup {
        kv,
        transport,
        connections,
        remote,
        sessions,
        clock,
        announced,
    }
}

const PROJECT: &str = "remote://env/home/me/repo";

fn descriptor() -> Value {
    json!({
        "protocolVersion": 1,
        "environmentId": "env",
        "name": "mini",
        "providers": ["codex", "claude"],
        "capabilities": ["changes.wait", "attachments.upload", "sessions.plan", "sessions.draft"],
        "hostVersion": "0.6.0"
    })
}

fn catalog() -> Value {
    json!({
        "models": { "codex": [{ "id": "codex:gpt-5", "harness": "codex", "name": "GPT-5" }] },
        "errors": {}
    })
}

fn host_session(id: &str, revision: i64, busy: bool, blocks: Value) -> Value {
    json!({
        "projectId": "p1",
        "revision": revision,
        "status": if busy { "running" } else { "idle" },
        "runId": "run-1",
        "updatedAt": 1,
        "session": {
            "id": id,
            "harness": "codex",
            "model": "codex:gpt-5",
            "modelSettings": {},
            "runtimeMode": "supervised",
            "title": "Fix the build",
            "cwd": "/home/me/repo",
            "busy": busy,
            "blocks": blocks
        }
    })
}

/// A scripted host: one session per id, commands recorded, `changes.wait`
/// held until the test releases it.
#[derive(Default)]
struct Host {
    sessions: Map<String, Value>,
    next_session: String,
    dispatch_error: Option<String>,
    on_send: Option<fn(&mut Host, &Value)>,
}

fn host(transport: &FakeTransport) -> Arc<Mutex<Host>> {
    let host = Arc::new(Mutex::new(Host {
        next_session: "host-9".into(),
        ..Host::default()
    }));
    let state = host.clone();
    transport.set_handler(move |_, method, params| {
        let mut host = state.lock();
        let reply = match method {
            "environment.describe" => Reply::Value(descriptor()),
            "models.list" => Reply::Value(catalog()),
            "changes.wait" => Reply::Hold,
            "sessions.list" => Reply::Value(json!([])),
            "sessions.delete" => {
                let id = params["sessionId"].as_str().unwrap_or_default().to_string();
                host.sessions.remove(&id);
                Reply::Value(json!({}))
            }
            "sessions.sync" => {
                let id = params["sessionId"].as_str().unwrap_or_default();
                let snapshot = host.sessions.get(id)?.clone();
                if params.get("revision") == snapshot.get("revision") {
                    Reply::Value(json!({ "kind": "unchanged", "revision": snapshot["revision"] }))
                } else {
                    Reply::Value(json!({ "kind": "snapshot", "value": snapshot }))
                }
            }
            "commands.dispatch" => {
                if let Some(error) = host.dispatch_error.take() {
                    return Some(Reply::Error(error));
                }
                let session_id = if params["type"] == "create" {
                    let id = host.next_session.clone();
                    host.sessions
                        .insert(id.clone(), host_session(&id, 1, false, json!([])));
                    id
                } else {
                    params["sessionId"].as_str().unwrap_or_default().to_string()
                };
                if params["type"] == "send"
                    && let Some(on_send) = host.on_send
                {
                    on_send(&mut host, params);
                }
                Reply::Value(json!({
                    "commandId": params["commandId"],
                    "sessionId": session_id,
                    "revision": 2
                }))
            }
            _ => return None,
        };
        Some(reply)
    });
    host
}

/// Machines paired, the project remembered, and one tab open in it.
fn remote_setup(cx: &mut TestAppContext, bound: Option<&str>) -> (Setup, Arc<Mutex<Host>>) {
    let transport = FakeTransport::new();
    transport.set_machines(vec![machine("m1", "env")]);
    let host = host(&transport);
    let s = setup_with(cx, transport);
    remember_remote_project(
        &s.kv,
        "env",
        &HostProject {
            id: "p1".into(),
            cwd: "/home/me/repo".into(),
            name: "repo".into(),
        },
    );
    if let Some(bound) = bound {
        remember_remote_session(&s.kv, "tab-1", Some(bound));
    }
    let shell = Session::blank("tab-1", HarnessId::Codex, "codex:gpt-5", PROJECT);
    s.sessions
        .update(cx, |sessions, cx| sessions.insert(shell, cx));
    cx.run_until_parked();
    (s, host)
}

fn user_block(id: &str, text: &str) -> Value {
    json!({ "id": id, "role": "user", "text": text })
}

#[gpui::test]
fn a_remote_tab_shows_the_host_transcript_and_merges_it_into_sessions(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock().sessions.insert(
        "host-1".into(),
        host_session("host-1", 3, false, json!([user_block("turn", "Fix it")])),
    );
    let tab = s.open(cx, "tab-1");
    tab.read_with(cx, |tab, cx| {
        assert!(tab.online());
        assert!(tab.models_probed());
        assert_eq!(tab.models_for(HarnessId::Codex)[0].id, "codex:gpt-5");
        assert_eq!(
            tab.features(),
            RemoteFeatures {
                attachments: true,
                plan: true,
                draft: true
            }
        );
        let session = tab.session(cx);
        assert_eq!(session.id, "tab-1");
        assert_eq!(session.cwd, PROJECT);
        assert_eq!(session.blocks.len(), 1);
        assert_eq!(session.busy, Some(false));
        assert!(!tab.loading(cx));
    });
    let merged = s.session(cx, "tab-1");
    assert_eq!(merged.title, "Fix the build");
    assert_eq!(merged.cwd, PROJECT);
    assert_eq!(merged.worktree_cwd, None);
    assert_eq!(merged.blocks[0].text, "Fix it");
    assert_eq!(
        s.remote
            .read_with(cx, |remote, _| remote.host_revision("tab-1")),
        Some(3)
    );
    assert_eq!(
        s.connections
            .read_with(cx, |connections, _| connections.online("m1")),
        Some(true)
    );
    assert_eq!(
        s.transport.calls_for("environment.describe")[0],
        json!({ "supportedProviders": ["codex", "claude", "cursor", "grok", "opencode", "pi", "omp", "fx", "hermes", "droid", "antigravity"] })
    );
}

#[gpui::test]
fn pushed_changes_reload_at_once_and_a_finished_turn_is_announced(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock().sessions.insert(
        "host-1".into(),
        host_session("host-1", 3, true, json!([user_block("turn", "Fix it")])),
    );
    let _tab = s.open(cx, "tab-1");
    assert!(s.session(cx, "tab-1").is_busy());
    let syncs = s.transport.calls_for("sessions.sync").len();
    host.lock().sessions.insert(
        "host-1".into(),
        host_session(
            "host-1",
            4,
            false,
            json!([user_block("turn", "Fix it"), { "id": "reply", "role": "assistant", "text": "Done" }]),
        ),
    );
    // The first answer only sets the cursor.
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({ "boot": "b", "cursor": 1, "sessions": [], "reset": false }))
    ));
    cx.run_until_parked();
    assert_eq!(s.transport.calls_for("sessions.sync").len(), syncs);
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({
            "boot": "b",
            "cursor": 2,
            "sessions": [{ "id": "host-1", "projectId": "p1", "revision": 4, "status": "idle", "busy": false }],
            "reset": false
        }))
    ));
    cx.run_until_parked();
    let syncs_after = s.transport.calls_for("sessions.sync");
    assert!(syncs_after.len() > syncs);
    // Delta requests name the revision this desktop holds.
    assert_eq!(syncs_after[syncs]["revision"], 3);
    let merged = s.session(cx, "tab-1");
    assert!(!merged.is_busy());
    assert_eq!(merged.blocks.len(), 2);
    assert_eq!(*s.announced.borrow(), vec!["tab-1".to_string()]);
    assert_eq!(
        s.remote
            .read_with(cx, |remote, _| remote.host_revision("tab-1")),
        Some(4)
    );
}

#[gpui::test]
fn a_turn_that_started_elsewhere_marks_a_hidden_tab_busy(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, false, json!([])));
    cx.run_until_parked();
    // No tab view is open; the turn watch alone follows the machine.
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({ "boot": "b", "cursor": 1, "sessions": [], "reset": false }))
    ));
    cx.run_until_parked();
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({
            "boot": "b",
            "cursor": 2,
            "sessions": [{ "id": "host-1", "projectId": "p1", "revision": 4, "status": "running", "busy": true }],
            "reset": false
        }))
    ));
    cx.run_until_parked();
    assert!(s.session(cx, "tab-1").is_busy());
    host.lock().sessions.insert(
        "host-1".into(),
        host_session("host-1", 5, false, json!([user_block("t", "Hi")])),
    );
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({
            "boot": "b",
            "cursor": 3,
            "sessions": [{ "id": "host-1", "projectId": "p1", "revision": 5, "status": "idle", "busy": false }],
            "reset": false
        }))
    ));
    cx.run_until_parked();
    let session = s.session(cx, "tab-1");
    assert!(!session.is_busy());
    assert_eq!(session.blocks.len(), 1);
    assert_eq!(*s.announced.borrow(), vec!["tab-1".to_string()]);
}

#[gpui::test]
fn the_first_message_creates_the_session_on_the_host_then_sends_it(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, None);
    host.lock().on_send = Some(|host, params| {
        let id = params["sessionId"].as_str().unwrap().to_string();
        let block = user_block(
            params["commandId"].as_str().unwrap(),
            params["text"].as_str().unwrap(),
        );
        host.sessions
            .insert(id.clone(), host_session(&id, 2, true, json!([block])));
    });
    let tab = s.open(cx, "tab-1");
    tab.read_with(cx, |tab, _| {
        assert!(tab.online());
        assert_eq!(tab.draft_configuration().model, "codex:gpt-5");
        assert!(!tab.started());
    });
    let sent = tab.update(cx, |tab, cx| {
        tab.submit("Hello", Vec::new(), &RemoteTurnOptions::default(), cx)
    });
    assert!(sent);
    tab.read_with(cx, |tab, cx| {
        // The message shows at once, before the host confirms it.
        let session = tab.session(cx);
        assert_eq!(
            session.blocks.last().map(|block| block.text.as_str()),
            Some("Hello")
        );
        assert_eq!(session.busy, Some(true));
    });
    cx.run_until_parked();
    let dispatched = s.dispatched();
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[0]["type"], "create");
    assert_eq!(dispatched[0]["projectId"], "p1");
    assert_eq!(dispatched[0]["harness"], "codex");
    assert_eq!(dispatched[0]["model"], "codex:gpt-5");
    assert_eq!(dispatched[0]["runtimeMode"], "supervised");
    assert_eq!(dispatched[0]["modelSettings"], json!({}));
    assert!(dispatched[0].get("worktreeCwd").is_none());
    assert_eq!(dispatched[1]["type"], "send");
    assert_eq!(dispatched[1]["sessionId"], "host-9");
    assert_eq!(dispatched[1]["text"], "Hello");
    assert_eq!(dispatched[1]["attachments"], json!([]));
    assert_eq!(dispatched[1]["intent"], "default");
    assert_eq!(
        s.connections
            .read_with(cx, |c, _| c.remote_session_for("tab-1"))
            .as_deref(),
        Some("host-9")
    );
    assert_eq!(
        pending_remote_command(&s.kv, PROJECT, "env", PendingScope::Any, None),
        None
    );
    tab.read_with(cx, |tab, cx| {
        assert!(tab.started());
        assert_eq!(tab.session_id(), Some("host-9"));
        let session = tab.session(cx);
        assert_eq!(session.blocks.len(), 1);
        assert_eq!(session.blocks[0].text, "Hello");
        assert!(tab.notice().is_none());
    });
    assert_eq!(s.session(cx, "tab-1").blocks[0].text, "Hello");
}

#[gpui::test]
fn an_uncertain_dispatch_stays_in_the_outbox_and_retry_sends_the_same_command(
    cx: &mut TestAppContext,
) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, false, json!([])));
    let tab = s.open(cx, "tab-1");
    host.lock().dispatch_error =
        Some("The host request did not complete. Retry to confirm its result.".into());
    assert!(tab.update(cx, |tab, cx| tab.submit(
        "Ship it",
        Vec::new(),
        &RemoteTurnOptions::default(),
        cx
    )));
    cx.run_until_parked();
    let first = s.dispatched()[0].clone();
    assert_eq!(first["type"], "send");
    let pending = pending_remote_command(
        &s.kv,
        PROJECT,
        "env",
        PendingScope::Session("host-1"),
        Some("tab-1"),
    );
    assert_eq!(
        pending
            .as_ref()
            .map(|command| serde_json::to_value(command).unwrap()),
        Some(first.clone())
    );
    tab.read_with(cx, |tab, cx| {
        let notice = tab.notice().unwrap();
        assert_eq!(notice.text, "Waiting for the host to confirm your request.");
        assert_eq!(
            notice.detail.as_deref(),
            Some("The host request did not complete. Retry to confirm its result.")
        );
        assert_eq!(notice.action, NoticeAction::RetryPending);
        // The unconfirmed turn stays in the transcript.
        assert!(tab.busy());
        assert_eq!(tab.session(cx).blocks.last().unwrap().text, "Ship it");
        // Nothing else can be sent until the host confirms it.
    });
    assert!(!tab.update(cx, |tab, cx| tab.submit(
        "Another",
        Vec::new(),
        &RemoteTurnOptions::default(),
        cx
    )));
    tab.update(cx, |tab, cx| {
        tab.run_notice_action(NoticeAction::RetryPending, cx)
    });
    cx.run_until_parked();
    let dispatched = s.dispatched();
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[1], first);
    assert_eq!(
        pending_remote_command(&s.kv, PROJECT, "env", PendingScope::Any, None),
        None
    );
    tab.read_with(cx, |tab, _| assert!(tab.pending().is_none()));
}

#[gpui::test]
fn a_restarted_tab_finds_its_unconfirmed_command(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, false, json!([])));
    let tab = s.open(cx, "tab-1");
    host.lock().dispatch_error =
        Some("The host request did not complete. Retry to confirm its result.".into());
    tab.update(cx, |tab, cx| {
        tab.submit("Ship it", Vec::new(), &RemoteTurnOptions::default(), cx)
    });
    cx.run_until_parked();
    // The app restarts: the tab opens again with only the saved outbox.
    s.remote.update(cx, |remote, _| remote.close("tab-1"));
    drop(tab);
    cx.run_until_parked();
    let tab = s.open(cx, "tab-1");
    tab.read_with(cx, |tab, cx| {
        assert!(matches!(
            tab.pending(),
            Some(monocode_remote::host::protocol::HostCommand::Send { .. })
        ));
        assert_eq!(tab.session(cx).blocks.last().unwrap().text, "Ship it");
    });
}

#[gpui::test]
fn a_rejected_command_leaves_the_outbox_and_shows_the_error(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, false, json!([])));
    let tab = s.open(cx, "tab-1");
    host.lock().dispatch_error = Some("Host rejected request: Session is busy".into());
    tab.update(cx, |tab, cx| {
        tab.submit("Ship it", Vec::new(), &RemoteTurnOptions::default(), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        pending_remote_command(&s.kv, PROJECT, "env", PendingScope::Any, None),
        None
    );
    tab.read_with(cx, |tab, cx| {
        assert!(tab.pending().is_none());
        assert!(!tab.busy());
        let notice = tab.notice().unwrap();
        assert_eq!(notice.text, "Couldn’t send the message on Home.");
        assert_eq!(
            notice.detail.as_deref(),
            Some("Host rejected request: Session is busy")
        );
        assert_eq!(notice.action, NoticeAction::TryAgain);
        assert!(notice.alert);
        assert_eq!(
            tab.status(),
            RemoteSessionStatus {
                pending: false,
                sending: false,
                failed_draft: Some(false),
                error: "Host rejected request: Session is busy".into(),
                catalog_problem: String::new(),
            }
        );
        assert!(tab.session(cx).blocks.is_empty());
    });
    tab.update(cx, |tab, cx| {
        tab.run_notice_action(NoticeAction::TryAgain, cx)
    });
    cx.run_until_parked();
    let dispatched = s.dispatched();
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[1]["text"], "Ship it");
    tab.read_with(cx, |tab, _| assert!(tab.notice().is_none()));
}

#[gpui::test]
fn approvals_and_answers_go_to_the_host_with_the_run_id(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, true, json!([])));
    let _tab = s.open(cx, "tab-1");
    let session = s.session(cx, "tab-1");
    assert!(cx.update(|cx| RemoteGlobal::is_remote_session(&session, cx)));
    cx.update(|cx| RemoteGlobal::approve("tab-1", 7, ApprovalDecision::Allow, cx));
    cx.run_until_parked();
    let reply: monocode_core::user_question::UserQuestionReply =
        serde_json::from_value(json!({ "kind": "skipped" })).unwrap();
    cx.update(|cx| RemoteGlobal::answer("tab-1", 8, &reply, cx));
    cx.run_until_parked();
    cx.update(|cx| RemoteGlobal::stop("tab-1", cx));
    cx.run_until_parked();
    let dispatched = s.dispatched();
    assert_eq!(dispatched[0]["type"], "approve");
    assert_eq!(dispatched[0]["runId"], "run-1");
    assert_eq!(dispatched[0]["requestId"], 7);
    assert_eq!(dispatched[0]["decision"], "allow");
    assert_eq!(dispatched[1]["type"], "answer");
    assert_eq!(dispatched[1]["requestId"], 8);
    assert_eq!(
        dispatched[1]["reply"],
        serde_json::to_value(&reply).unwrap()
    );
    assert_eq!(dispatched[2]["type"], "cancel");
    assert_eq!(dispatched[2]["sessionId"], "host-1");
}

#[gpui::test]
fn configuration_changes_apply_once_the_session_is_idle(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, true, json!([])));
    let tab = s.open(cx, "tab-1");
    tab.update(cx, |tab, cx| {
        tab.set_runtime_mode(RuntimeMode::FullAccess, cx)
    });
    cx.run_until_parked();
    // A running turn keeps its settings.
    assert!(s.dispatched().is_empty());
    tab.read_with(cx, |tab, cx| {
        assert_eq!(tab.session(cx).runtime_mode, RuntimeMode::FullAccess)
    });
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 4, false, json!([])));
    s.advance(cx, 5_000);
    let dispatched = s.dispatched();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0]["type"], "configure");
    assert_eq!(dispatched[0]["runtimeMode"], "full-access");
    assert_eq!(dispatched[0]["model"], "codex:gpt-5");
}

#[gpui::test]
fn removing_the_only_draft_deletes_the_session_and_resets_the_tab(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock().sessions.insert(
        "host-1".into(),
        host_session(
            "host-1",
            3,
            false,
            json!([{ "id": "d1", "role": "user", "text": "Later", "draft": true }]),
        ),
    );
    let tab = s.open(cx, "tab-1");
    assert!(tab.update(cx, |tab, cx| tab.remove_draft("d1", cx)));
    cx.run_until_parked();
    assert_eq!(
        s.transport.calls_for("sessions.delete"),
        vec![json!({ "projectId": "p1", "sessionId": "host-1" })]
    );
    assert_eq!(
        s.connections
            .read_with(cx, |c, _| c.remote_session_for("tab-1")),
        None
    );
    let session = s.session(cx, "tab-1");
    assert_eq!(session.title, "New remote session");
    assert!(session.blocks.is_empty());
    tab.read_with(cx, |tab, _| assert!(!tab.started()));
}

#[gpui::test]
fn older_hosts_without_pushed_changes_keep_polling(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, false, json!([])));
    s.transport.queue(
        "changes.wait",
        Reply::Error("Host rejected request: Unsupported host method".into()),
    );
    let _tab = s.open(cx, "tab-1");
    let syncs = s.transport.calls_for("sessions.sync").len();
    // A visible idle tab polls every 3 seconds without pushed changes.
    s.advance(cx, 3_000);
    assert_eq!(s.transport.calls_for("sessions.sync").len(), syncs + 1);
    s.advance(cx, 3_000);
    assert_eq!(s.transport.calls_for("sessions.sync").len(), syncs + 2);
    assert_eq!(s.transport.calls_for("changes.wait").len(), 1);
}

#[gpui::test]
fn an_unreachable_machine_goes_offline_and_recovers(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    host.lock()
        .sessions
        .insert("host-1".into(), host_session("host-1", 3, false, json!([])));
    let tab = s.open(cx, "tab-1");
    s.transport.fail("sessions.sync", "Machine is unreachable.");
    // A visible idle tab polls every 3 seconds while changes are not pushed.
    s.advance(cx, 3_000);
    tab.read_with(cx, |tab, _| assert!(!tab.online()));
    assert_eq!(
        s.connections.read_with(cx, |c, _| c.online("m1")),
        Some(false)
    );
    assert!(!tab.update(cx, |tab, cx| tab.submit(
        "Hi",
        Vec::new(),
        &RemoteTurnOptions::default(),
        cx
    )));
    // It describes the host again before the next sync.
    let describes = s.transport.calls_for("environment.describe").len();
    s.advance(cx, 1_500);
    assert_eq!(
        s.transport.calls_for("environment.describe").len(),
        describes + 1
    );
    tab.read_with(cx, |tab, _| assert!(tab.online()));
    assert_eq!(
        s.connections.read_with(cx, |c, _| c.online("m1")),
        Some(true)
    );
}

#[gpui::test]
fn machine_status_follows_the_latest_check(cx: &mut TestAppContext) {
    let (s, _host) = remote_setup(cx, None);
    let _watch = s.connections.update(cx, |connections, cx| {
        connections.watch_machine_status("m1", cx)
    });
    cx.run_until_parked();
    assert_eq!(
        s.connections.read_with(cx, |c, _| c.online("m1")),
        Some(true)
    );
    s.transport
        .fail("environment.describe", "Machine is unreachable.");
    s.advance(cx, 15_000);
    assert_eq!(
        s.connections.read_with(cx, |c, _| c.online("m1")),
        Some(false)
    );
    // Back off: 6 seconds after one failure.
    let checks = s.transport.calls_for("environment.describe").len();
    s.advance(cx, 5_999);
    assert_eq!(s.transport.calls_for("environment.describe").len(), checks);
    s.advance(cx, 1);
    assert_eq!(
        s.connections.read_with(cx, |c, _| c.online("m1")),
        Some(true)
    );
}

#[gpui::test]
fn project_session_lists_load_cache_and_reload_on_pushed_changes(cx: &mut TestAppContext) {
    let (s, _host) = remote_setup(cx, None);
    let summary = json!([{
        "projectId": "p1", "revision": 1, "status": "idle", "updatedAt": 5,
        "id": "host-1", "title": "Fix the build", "harness": "codex", "providerSessionId": null
    }]);
    s.transport.respond("sessions.list", summary.clone());
    let _watch = s.connections.update(cx, |connections, cx| {
        connections.watch_project_sessions(PROJECT, cx)
    });
    cx.run_until_parked();
    let list = s
        .connections
        .read_with(cx, |c, _| c.project_sessions(PROJECT));
    assert!(list.loaded);
    assert_eq!(list.machine.map(|machine| machine.id), Some("m1".into()));
    assert_eq!(serde_json::to_value(&list.sessions).unwrap(), summary);
    assert_eq!(
        s.kv.get_item(&connections::history_key(PROJECT))
            .map(|raw| serde_json::from_str::<Value>(&raw).unwrap()),
        Some(summary)
    );
    assert_eq!(
        s.transport.calls_for("sessions.list"),
        vec![json!({ "projectId": "p1" })]
    );
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({ "boot": "b", "cursor": 1, "sessions": [], "reset": false }))
    ));
    cx.run_until_parked();
    assert!(s.transport.release(
        "changes.wait",
        Ok(json!({
            "boot": "b", "cursor": 2,
            "sessions": [{ "id": "host-2", "projectId": "p1", "revision": 1 }],
            "reset": false
        }))
    ));
    cx.run_until_parked();
    assert_eq!(s.transport.calls_for("sessions.list").len(), 2);
}

#[gpui::test]
fn pairing_and_disconnecting_update_the_machine_list(cx: &mut TestAppContext) {
    let transport = FakeTransport::new();
    let s = setup_with(cx, transport.clone());
    assert!(
        s.connections
            .read_with(cx, |c, _| c.loaded() && c.machines().is_empty())
    );
    transport.set_paired(machine("m2", "env-2"));
    let pair = s.connections.update(cx, |connections, cx| {
        connections.pair("monocode://pair?v=1".into(), String::new(), cx)
    });
    cx.run_until_parked();
    assert_eq!(block_on(pair).unwrap().id, "m2");
    assert_eq!(
        s.connections.read_with(cx, |c, _| c
            .machine_for_environment("env-2")
            .map(|m| m.id.clone())),
        Some("m2".into())
    );
    let disconnect = s
        .connections
        .update(cx, |connections, cx| connections.disconnect("m2", cx));
    cx.run_until_parked();
    block_on(disconnect).unwrap();
    assert!(s.connections.read_with(cx, |c, _| c.machines().is_empty()));
    assert!(
        transport
            .commands()
            .contains(&"remote_disconnect".to_string())
    );
}

#[gpui::test]
fn a_tab_without_a_machine_explains_why(cx: &mut TestAppContext) {
    let transport = FakeTransport::new();
    let s = setup_with(cx, transport);
    let shell = Session::blank("tab-1", HarnessId::Codex, "codex:gpt-5", PROJECT);
    s.sessions
        .update(cx, |sessions, cx| sessions.insert(shell.clone(), cx));
    let tab = s
        .remote
        .update(cx, |remote, cx| remote.open(&shell, true, cx));
    assert!(matches!(tab, RemoteTab::MissingProject));
    remember_remote_project(
        &s.kv,
        "env",
        &HostProject {
            id: "p1".into(),
            cwd: "/home/me/repo".into(),
            name: "repo".into(),
        },
    );
    let tab = s
        .remote
        .update(cx, |remote, cx| remote.open(&shell, true, cx));
    assert!(matches!(tab, RemoteTab::NotConnected));
    assert!(tab.offers_manage_machines());
    assert_eq!(
        tab.message(),
        Some("The machine for this project isn’t connected on this computer.")
    );
}

#[gpui::test]
fn a_usage_limit_resumes_once_it_resets(cx: &mut TestAppContext) {
    let (s, host) = remote_setup(cx, Some("host-1"));
    let resets_at = s.clock.load(Ordering::SeqCst) + 30_000;
    let mut snapshot = host_session("host-1", 3, false, json!([user_block("t", "Hi")]));
    snapshot["session"]["usageLimit"] = json!({ "resetsAt": resets_at });
    host.lock().sessions.insert("host-1".into(), snapshot);
    let tab = s.open(cx, "tab-1");
    tab.read_with(cx, |tab, cx| assert!(tab.usage_limit(cx).is_some()));
    tab.update(cx, |tab, cx| tab.set_resume_at_reset(true, cx));
    s.advance(cx, 29_000);
    assert!(s.dispatched().is_empty());
    s.advance(cx, 60_000);
    let dispatched = s.dispatched();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(
        dispatched[0]["text"],
        crate::runtime::in_flight::CONTINUE_PROMPT
    );
    tab.read_with(cx, |tab, cx| assert!(tab.usage_limit(cx).is_none()));
}

/// Checks the client against a real host, Node or Rust. Start one with
/// `node build/host/monocode-host.mjs connect --no-service --json --bind
/// 127.0.0.1 --port 38774 --data-dir "$(mktemp -d)"` and pass its link in
/// `MONOCODE_TEST_PAIRING_LINK`. The test pairs, creates a project and a
/// session in a temporary folder, saves and syncs a draft, follows the
/// change feed, lists the folder, deletes the session, and revokes its own
/// device credential. No provider runs.
#[test]
#[ignore = "needs a running host and MONOCODE_TEST_PAIRING_LINK"]
fn real_host_answers_in_the_shapes_the_client_reads() {
    use monocode_remote::host::protocol::{
        CommandReceipt, HostCommand, HostModelCatalog, HostSessionSummary, REMOTE_PROVIDERS,
        SessionChanges, provider_name, require_host_descriptor,
    };
    let Ok(link) = std::env::var("MONOCODE_TEST_PAIRING_LINK") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("monocode-remote-live-{}", uuid::Uuid::new_v4()));
    let folder = dir.join("repo");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("hello.txt"), "hi").unwrap();
    let client = RemoteClient::new(Arc::new(NativeTransport::new(dir.join("desktop"))));
    let machine = block_on(client.pair(link, "engine live test".into())).unwrap();
    assert_eq!(
        block_on(client.load_machines()).unwrap(),
        vec![machine.clone()]
    );
    let request =
        |method: &str, params: Value| block_on(client.request(&machine.id, method, params));

    let providers: Vec<&str> = REMOTE_PROVIDERS.iter().map(|p| provider_name(*p)).collect();
    let host = require_host_descriptor(
        &request(
            "environment.describe",
            json!({ "supportedProviders": providers }),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(host.environment_id, machine.environment_id);
    eprintln!(
        "host {:?} providers {:?}",
        host.host_version, host.providers
    );
    let Some(harness) = host.providers.first().copied() else {
        panic!("the host has no provider installed");
    };

    let project: HostProject =
        client::decode(request("projects.open", json!({ "cwd": folder })).unwrap()).unwrap();
    let catalog: HostModelCatalog =
        client::decode(request("models.list", json!({ "projectId": project.id })).unwrap())
            .unwrap();
    let model = catalog
        .models
        .get(&harness)
        .and_then(|models| models.first())
        .map(|model| model.id.clone())
        .unwrap_or_else(|| format!("{}:default", provider_name(harness)));

    let first: SessionChanges =
        client::decode(request("changes.wait", json!({ "after": 0 })).unwrap()).unwrap();
    assert!(first.reset);

    let create = HostCommand::Create {
        command_id: uuid::Uuid::new_v4().to_string(),
        project_id: project.id.clone(),
        worktree_cwd: None,
        auto_worktree_branch: None,
        harness,
        model,
        model_settings: Some(Default::default()),
        runtime_mode: RuntimeMode::Supervised,
    };
    let receipt: CommandReceipt = client::decode(
        request("commands.dispatch", serde_json::to_value(&create).unwrap()).unwrap(),
    )
    .unwrap();
    // The same command ID answers with the same receipt.
    let again: CommandReceipt = client::decode(
        request("commands.dispatch", serde_json::to_value(&create).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(again, receipt);
    let snapshot =
        block_on(client.load_remote_session(&machine.id, &receipt.session_id, None)).unwrap();
    assert_eq!(snapshot.project_id, project.id);
    let unchanged = block_on(client.load_remote_session(
        &machine.id,
        &receipt.session_id,
        Some(snapshot.clone()),
    ))
    .unwrap();
    assert!(Arc::ptr_eq(&unchanged, &snapshot));

    let draft = HostCommand::Draft {
        command_id: uuid::Uuid::new_v4().to_string(),
        session_id: receipt.session_id.clone(),
        text: "Later".into(),
        attachments: Some(Vec::new()),
    };
    request("commands.dispatch", serde_json::to_value(&draft).unwrap()).unwrap();
    let changes: SessionChanges = client::decode(
        request(
            "changes.wait",
            json!({ "boot": first.boot, "after": first.cursor }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        changes
            .sessions
            .iter()
            .any(|change| change.id == receipt.session_id)
    );
    let next = block_on(client.load_remote_session(
        &machine.id,
        &receipt.session_id,
        Some(snapshot.clone()),
    ))
    .unwrap();
    assert!(next.revision > snapshot.revision);
    assert_eq!(next.session.blocks.len(), 1);
    assert!(next.session.blocks[0].is_draft());

    let sessions: Vec<HostSessionSummary> =
        client::decode(request("sessions.list", json!({ "projectId": project.id })).unwrap())
            .unwrap();
    assert!(
        sessions
            .iter()
            .any(|session| session.id == receipt.session_id)
    );

    let root = remote_path(&machine.environment_id, &folder.to_string_lossy());
    let listed =
        block_on(client.run_remote_command("list_dir", args(json!({ "path": root })))).unwrap();
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == json!(format!("{root}/hello.txt")))
    );

    request(
        "sessions.delete",
        json!({ "projectId": project.id, "sessionId": receipt.session_id }),
    )
    .unwrap();
    request("devices.revokeSelf", json!({})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
