//! Control executor tests: request routing and app receipts (App.tsx 8998-
//! 9221), and an end-to-end run of the real `app` CLI against the loopback
//! control server.

use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

use gpui::TestAppContext;
use monocode_core::{HarnessId, Session};
use monocode_process::control::{ControlEvents, ControlHost, ControlRequest};
use monocode_settings::Kv;
use serde_json::{Value, json};

use super::executor::{ControlExecutor, run_request, serve_control_requests};
use super::package::{Orchestration, OrchestrationConfig};
use super::peers::NoPeers;
use super::testing::{FakeStore, finish};
use crate::runtime::Engine;
use crate::runtime::testing::init_test_engine;

fn init(cx: &mut TestAppContext, control: Option<Arc<ControlHost>>) {
    init_test_engine(cx);
    cx.update(|cx| {
        Orchestration::init(
            OrchestrationConfig {
                storage: FakeStore::new(),
                control,
                harness_host: None,
                owner: "main".into(),
                kv: Kv::in_memory(),
                peers: Rc::new(NoPeers),
            },
            cx,
        );
    });
}

fn open(cx: &mut TestAppContext, session: Session) {
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| sessions.upsert(session, cx));
    });
}

fn request(
    namespace: &str,
    session_id: &str,
    request_id: &str,
    action: &str,
    input: Value,
) -> ControlRequest {
    ControlRequest {
        id: format!("reply-{request_id}"),
        namespace: namespace.into(),
        session_id: session_id.into(),
        request_id: request_id.into(),
        action: action.into(),
        input,
    }
}

fn run(cx: &mut TestAppContext, request: ControlRequest) -> Result<Value, String> {
    let task = cx.spawn(|mut cx| async move { run_request("main", request, &mut cx).await });
    finish(cx, task)
}

#[gpui::test]
fn routes_control_requests_to_the_orchestrator_and_refuses_unknown_namespaces(
    cx: &mut TestAppContext,
) {
    init(cx, None);
    assert_eq!(
        run(cx, request("control", "lead", "r1", "list", json!({}))).unwrap_err(),
        "No orchestration run was found for this lead"
    );
    assert!(
        run(
            cx,
            request("control", "lead", "r2", "list", json!({ "x": 1 }))
        )
        .unwrap_err()
        .contains("list takes no input")
    );
    assert_eq!(
        run(cx, request("other", "lead", "r3", "list", json!({}))).unwrap_err(),
        "Unknown CLI namespace"
    );
}

#[gpui::test]
fn refuses_app_calls_from_workers_inbox_asks_and_unknown_sessions(cx: &mut TestAppContext) {
    init(cx, None);
    let mut worker = Session::blank("worker", HarnessId::Codex, "codex:test", "/tmp/project");
    worker.orchestration_lead_id = Some("lead".into());
    open(cx, worker);
    for id in ["worker", "missing"] {
        assert_eq!(
            run(cx, request("app", id, "r1", "models.list", json!({}))).unwrap_err(),
            "This session cannot use the MonoCode app CLI"
        );
    }
}

#[gpui::test]
fn answers_a_repeated_app_request_once_and_rejects_changed_input(cx: &mut TestAppContext) {
    init(cx, None);
    open(
        cx,
        Session::blank("lead", HarnessId::Codex, "codex:test", "/tmp/project"),
    );
    let first = run(
        cx,
        request("app", "lead", "same", "folders.list", json!({})),
    )
    .unwrap();
    assert_eq!(first, json!({ "cwd": "/tmp/project", "folders": [] }));
    let again = run(
        cx,
        request("app", "lead", "same", "folders.list", json!({})),
    )
    .unwrap();
    assert_eq!(again, first);
    assert_eq!(
        run(cx, request("app", "lead", "same", "notes.list", json!({}))).unwrap_err(),
        "Request ID was already used with different input"
    );
    // A failed call frees its id for a corrected retry.
    assert!(
        run(
            cx,
            request("app", "lead", "retry", "sessions.read", json!({}))
        )
        .is_err()
    );
    assert!(
        run(
            cx,
            request("app", "lead", "retry", "folders.list", json!({}))
        )
        .is_ok()
    );
}

#[gpui::test]
fn lists_open_and_stored_project_sessions_but_hides_workers(cx: &mut TestAppContext) {
    init(cx, None);
    let mut lead = Session::blank("lead", HarnessId::Codex, "codex:test", "/tmp/project");
    lead.title = "Lead".into();
    lead.busy = Some(true);
    open(cx, lead);
    let mut worker = Session::blank("worker", HarnessId::Codex, "codex:test", "/tmp/project");
    worker.orchestration_lead_id = Some("other-lead".into());
    open(cx, worker);
    let mut elsewhere = Session::blank("elsewhere", HarnessId::Codex, "codex:test", "/tmp/other");
    elsewhere.title = "Elsewhere".into();
    open(cx, elsewhere);
    let result = run(
        cx,
        request("app", "lead", "list", "sessions.list", json!({})),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({
            "cwd": "/tmp/project",
            "sessions": [{
                "id": "lead", "title": "Lead", "harness": "codex", "model": "codex:test",
                "busy": true, "hasDraft": false
            }]
        })
    );
}

/// Runs the real `app` CLI when the end-to-end test spawns this binary.
#[test]
#[ignore]
fn cli_child() {
    let Ok(args) = std::env::var("MONOCODE_E2E_CLI_ARGS") else {
        return;
    };
    let args: Vec<String> = serde_json::from_str(&args).unwrap();
    std::process::exit(monocode_process::control_cli::run_app(args));
}

/// The control server's threads may not wake the deterministic test
/// scheduler, so the test thread moves each request into the real
/// `ControlExecutor`.
struct Forwarder(std::sync::Mutex<std::sync::mpsc::Sender<(String, ControlRequest)>>);

impl ControlEvents for Forwarder {
    fn request(&self, owner: &str, request: ControlRequest) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "forwarder poisoned".to_string())?
            .send((owner.to_string(), request))
            .map_err(|error| error.to_string())
    }
}

struct Server {
    host: Arc<ControlHost>,
    executor: Arc<ControlExecutor>,
    forwarded: std::sync::mpsc::Receiver<(String, ControlRequest)>,
}

/// Spawn this test binary as the `app` CLI with the session's credentials,
/// drive the engine until it exits, and return its JSON line and exit code.
fn cli(cx: &mut TestAppContext, server: &Server, session_id: &str, args: &[&str]) -> (Value, i32) {
    let host = &server.host;
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "orchestration::executor_tests::cli_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
            "-q",
        ])
        .env(
            "MONOCODE_E2E_CLI_ARGS",
            serde_json::to_string(args).unwrap(),
        );
    monocode_process::control::configure_child(Some(host), session_id, &mut command);
    let child = std::thread::spawn(move || command.output().unwrap());
    while !child.is_finished() {
        while let Ok((owner, request)) = server.forwarded.try_recv() {
            server.executor.request(&owner, request).unwrap();
        }
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let output = child.join().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|line| line.starts_with("{\"ok\""))
        .unwrap_or_else(|| panic!("no JSON line in {stdout}"));
    (
        serde_json::from_str(line).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[gpui::test]
#[ignore]
fn app_cli_lists_models_and_sessions_through_the_control_server(cx: &mut TestAppContext) {
    init_test_engine(cx);
    let (forward, forwarded) = std::sync::mpsc::channel();
    let host = Arc::new(
        monocode_process::control::init(Arc::new(Forwarder(std::sync::Mutex::new(forward))))
            .unwrap(),
    );
    let (executor, requests) = ControlExecutor::channel();
    cx.update(|cx| serve_control_requests(requests, host.clone(), cx).detach());
    let server = Server {
        host: host.clone(),
        executor,
        forwarded,
    };
    cx.update(|cx| {
        Orchestration::init(
            OrchestrationConfig {
                storage: FakeStore::new(),
                control: Some(host.clone()),
                harness_host: None,
                owner: "main".into(),
                kv: Kv::in_memory(),
                peers: Rc::new(NoPeers),
            },
            cx,
        );
    });
    let project = std::env::temp_dir().join(format!("monocode-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&project).unwrap();
    let cwd = project.to_string_lossy().into_owned();
    let mut lead = Session::blank("lead", HarnessId::Codex, "codex:test", &cwd);
    lead.title = "Lead".into();
    open(cx, lead);
    let mut other = Session::blank("other", HarnessId::Claude, "claude:test", &cwd);
    other.title = "Other".into();
    open(cx, other);

    // Without an opted-in turn the token exists but is refused.
    monocode_process::control::control_authorize_turn(
        &host,
        "main",
        "lead".into(),
        cwd.clone(),
        false,
    )
    .unwrap();
    let (denied, code) = cli(cx, &server, "lead", &["models.list"]);
    assert_eq!(code, 1);
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["retryable"], false);

    monocode_process::control::control_authorize_turn(
        &host,
        "main",
        "lead".into(),
        cwd.clone(),
        true,
    )
    .unwrap();
    let (models, code) = cli(cx, &server, "lead", &["models.list"]);
    assert_eq!(code, 0, "{models}");
    assert_eq!(models["ok"], true);
    let result = &models["result"];
    assert_eq!(
        result["runtimeModes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mode| mode["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["supervised", "auto-accept-edits", "auto", "full-access"]
    );
    let harnesses = result["harnesses"].as_array().unwrap();
    assert_eq!(harnesses.len(), monocode_core::HARNESSES.len());
    let claude = harnesses
        .iter()
        .find(|entry| entry["id"] == "claude")
        .unwrap();
    assert!(!claude["models"].as_array().unwrap().is_empty());
    assert!(
        claude["models"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("claude:")
    );

    let (sessions, code) = cli(
        cx,
        &server,
        "lead",
        &["sessions.list", "--request-id", "list-1"],
    );
    assert_eq!(code, 0, "{sessions}");
    assert_eq!(
        sessions,
        json!({
            "ok": true,
            "result": {
                "cwd": cwd,
                "sessions": [
                    { "id": "lead", "title": "Lead", "harness": "codex", "model": "codex:test", "busy": false, "hasDraft": false },
                    { "id": "other", "title": "Other", "harness": "claude", "model": "claude:test", "busy": false, "hasDraft": false }
                ]
            }
        })
    );
    let (unknown, code) = cli(
        cx,
        &server,
        "lead",
        &["sessions.read", "--json", "{\"sessionId\":\"nope\"}"],
    );
    assert_eq!(code, 1);
    assert_eq!(unknown["error"], "Session was not found in this project");
    assert!(
        unknown["retryWith"]
            .as_str()
            .unwrap()
            .starts_with("--request-id ")
    );
    let _ = std::fs::remove_dir_all(project);
}
