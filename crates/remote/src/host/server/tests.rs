//! Port of host/server.test.ts. The engine and workspace parts run on
//! [`TestBackend`]; their own tests belong to the engine port.

use super::*;
use crate::host::http::HttpServer;
use crate::host::listener::{HostListener, HostListenerOptions, listen_host};
use crate::host::protocol::REMOTE_PROVIDERS;
use crate::host::store::{HostStore, IssuedDevice};
use crate::host::test_backend::TestBackend;
use monocode_core::{AgentModel, HarnessId};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

pub(crate) struct Setup {
    pub directory: tempfile::TempDir,
    pub backend: Arc<TestBackend>,
    pub store: Arc<HostStore>,
    pub project: HostProject,
    pub url: String,
    pub first: IssuedDevice,
    pub second: IssuedDevice,
    pub clock: Arc<AtomicI64>,
    pub http: Arc<HttpServer>,
    pub listener: HostListener,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.listener.close();
        self.http.close();
        self.http.close_all_connections();
        self.store.changes.close();
    }
}

pub(crate) fn setup(providers: &[RemoteProvider]) -> Setup {
    let directory = crate::host::store::tests::temporary("monocode-server-test-");
    let store = Arc::new(HostStore::open(&directory.path().join("host.db")).unwrap());
    let backend = Arc::new(TestBackend::new(store.clone(), providers.to_vec()));
    // Follow production's canonicalization, such as /var to /private/var.
    let project = backend
        .open_project(directory.path().to_str().unwrap())
        .unwrap();
    let clock = Arc::new(AtomicI64::new(0));
    let offset = clock.clone();
    let http = create_host_server(
        backend.clone(),
        providers.to_vec(),
        HostServerOptions {
            endpoints: Some(Arc::new(|| vec!["https://10.0.0.5:3774".into()])),
            clock: Arc::new(move || now_ms() + offset.load(Ordering::SeqCst)),
            ..Default::default()
        },
    );
    let listener = listen_host(
        http.clone(),
        HostListenerOptions {
            port: 0,
            bind: "127.0.0.1".into(),
            identity: None,
            loopback: None,
        },
    )
    .unwrap();
    let url = format!("http://127.0.0.1:{}/rpc", listener.local_addr().port());
    let first = store.issue_device("Laptop").unwrap();
    let second = store.issue_device("Other computer").unwrap();
    Setup {
        directory,
        backend,
        store,
        project,
        url,
        first,
        second,
        clock,
        http,
        listener,
    }
}

pub(crate) fn post(url: &str, headers: &[(&str, &str)], body: &str) -> (u16, Value) {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(40))
        .build();
    let mut request = agent.post(url);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match request.send_string(body) {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("{error}"),
    };
    let status = response.status();
    let text = response.into_string().unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

impl Setup {
    pub fn call_with(
        &self,
        method: &str,
        params: Value,
        token: &str,
        overrides: Value,
        headers: &[(&str, &str)],
    ) -> (u16, Value) {
        let mut payload = json!({
            "version": 1,
            "environmentId": self.store.environment_id,
            "method": method,
            "params": params,
        });
        for (key, value) in overrides.as_object().cloned().unwrap_or_default() {
            payload[key] = value;
        }
        let authorization = format!("Bearer {token}");
        let mut all = vec![("Authorization", authorization.as_str())];
        all.extend_from_slice(headers);
        post(&self.url, &all, &payload.to_string())
    }

    pub fn call(&self, method: &str, params: Value) -> (u16, Value) {
        self.call_with(method, params, &self.first.token, json!({}), &[])
    }

    pub fn call_as(&self, method: &str, params: Value, token: &str) -> (u16, Value) {
        self.call_with(method, params, token, json!({}), &[])
    }

    pub fn pair(&self, params: Value, method: &str) -> (u16, Value) {
        post(
            &self.url,
            &[],
            &json!({ "version": 1, "method": method, "params": params }).to_string(),
        )
    }

    pub fn create(&self, command_id: &str) -> String {
        let (status, created) = self.call(
            "commands.dispatch",
            json!({
                "type": "create",
                "commandId": command_id,
                "projectId": self.project.id,
                "harness": "codex",
                "model": "codex:test",
                "runtimeMode": "supervised",
            }),
        );
        assert_eq!(status, 200, "{created}");
        created["result"]["sessionId"].as_str().unwrap().to_string()
    }
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn rejects_a_credential_revoked_while_its_request_body_is_arriving() {
    let s = setup(&[HarnessId::Codex]);
    let body = json!({
        "version": 1,
        "environmentId": s.store.environment_id,
        "method": "commands.dispatch",
        "params": {
            "type": "create", "commandId": "revoked-create", "projectId": s.project.id,
            "harness": "codex", "model": "codex:test", "runtimeMode": "supervised"
        }
    })
    .to_string();
    let mut stream = TcpStream::connect(s.listener.local_addr()).unwrap();
    write!(
        stream,
        "POST /rpc HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\n\r\n{}",
        s.first.token,
        body.len(),
        &body[..1]
    )
    .unwrap();
    wait_until(|| s.store.auth_checks.load(Ordering::SeqCst) == 1);
    s.store.revoke_token(&s.first.token).unwrap();
    stream.write_all(&body.as_bytes()[1..]).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
    assert!(s.store.summaries(&s.project.id).unwrap().is_empty());
}

#[test]
fn uploads_an_authenticated_attachment_and_sends_its_host_path_to_the_provider() {
    let s = setup(&[HarnessId::Codex]);
    let id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    let upload = json!({ "id": id, "offset": 0, "size": 5, "data": "aGVsbG8=" });
    assert_eq!(
        s.call_as("attachments.upload", upload.clone(), "invalid").0,
        401
    );
    assert_eq!(
        s.call("attachments.upload", upload).1["result"],
        json!({ "offset": 5 })
    );
    let session_id = s.create("upload-create");
    let (status, sent) = s.call(
        "commands.dispatch",
        json!({
            "type": "send", "commandId": "upload-send", "sessionId": session_id,
            "text": "Read this",
            "attachments": [{ "id": id, "name": "notes.txt", "mimeType": "text/plain", "kind": "file", "size": 5 }]
        }),
    );
    assert_eq!(status, 200, "{sent}");
    let turns = s.backend.wait_for_turns(1, Duration::from_secs(5));
    assert!(
        turns[0].attachments[0]
            .path
            .as_deref()
            .unwrap()
            .contains(id)
    );
}

#[test]
fn applies_card_actions_to_the_owning_project_and_lists_their_saved_state() {
    let s = setup(&[HarnessId::Codex]);
    let session_id = s.create("card-session");
    let (status, _) = s.call(
        "sessions.update",
        json!({ "projectId": s.project.id, "sessionId": session_id, "title": "Codex · Card title", "pinned": true }),
    );
    assert_eq!(status, 200);
    let listed = s
        .call("sessions.list", json!({ "projectId": s.project.id }))
        .1;
    let first = &listed["result"][0];
    assert_eq!(first["id"], session_id.as_str());
    assert_eq!(first["title"], "Codex · Card title");
    assert_eq!(first["pinned"], true);
    assert_eq!(first["model"], "codex:test");
    assert_eq!(first["repo"], s.project.name.as_str());
    assert_ne!(
        s.call(
            "sessions.update",
            json!({ "projectId": "wrong-project", "sessionId": session_id, "archived": true })
        )
        .0,
        200
    );
    assert_ne!(
        s.call(
            "sessions.delete",
            json!({ "projectId": "wrong-project", "sessionId": session_id })
        )
        .0,
        200
    );
    for (patch, error) in [
        (json!({ "title": 3 }), "Invalid session title"),
        (json!({ "archived": "yes" }), "Invalid archive value"),
        (json!({}), "No session changes supplied"),
        (
            json!({ "linkedWorkItem": { "kind": "pr", "repo": "a/b", "number": 2, "url": "https://github.com/a/b/pull/3" } }),
            "Invalid linked work item",
        ),
    ] {
        let mut params = json!({ "projectId": s.project.id, "sessionId": session_id });
        for (key, value) in patch.as_object().unwrap() {
            params[key] = value.clone();
        }
        assert_eq!(s.call("sessions.update", params).1["error"], error);
    }
    let linked = s.call(
        "sessions.update",
        json!({
            "projectId": s.project.id, "sessionId": session_id,
            "linkedWorkItem": { "kind": "pr", "repo": "a/b", "number": 3, "url": "https://github.com/a/b/pull/3" }
        }),
    );
    assert_eq!(linked.1["result"]["linkedWorkItem"]["number"], 3);
    let cleared = s.call(
        "sessions.update",
        json!({ "projectId": s.project.id, "sessionId": session_id, "linkedWorkItem": null }),
    );
    assert!(cleared.1["result"].get("linkedWorkItem").is_none());
    assert_eq!(
        s.call(
            "sessions.delete",
            json!({ "projectId": s.project.id, "sessionId": session_id })
        )
        .1,
        json!({ "result": { "deleted": true } })
    );
    assert_eq!(
        s.call("sessions.list", json!({ "projectId": s.project.id }))
            .1,
        json!({ "result": [] })
    );
}

#[test]
fn lists_session_branches_and_diffs_the_selected_working_copy() {
    let s = setup(&[HarnessId::Codex]);
    let cwd = s.project.cwd.clone();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "core.autocrlf", "false"]);
    git(&["checkout", "-q", "-b", "main"]);
    std::fs::write(s.directory.path().join(".gitignore"), "host.db*\n").unwrap();
    std::fs::write(s.directory.path().join("file.txt"), "initial\n").unwrap();
    git(&["add", ".gitignore", "file.txt"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-q",
        "-m",
        "initial",
    ]);
    let session_id = s.create("in-checkout");
    let first = s
        .call("sessions.list", json!({ "projectId": s.project.id }))
        .1["result"][0]
        .clone();
    assert_eq!(first["id"], session_id.as_str());
    assert_eq!(first["branch"], "main");
    assert_eq!(first["repo"], s.project.name.as_str());
    assert!(first.get("worktreeCwd").is_none());
    let outside = s.call(
        "commands.dispatch",
        json!({
            "type": "create", "commandId": "outside-worktree", "projectId": s.project.id,
            "worktreeCwd": std::env::temp_dir(), "harness": "codex", "model": "codex:test",
            "runtimeMode": "supervised"
        }),
    );
    assert!(
        outside.1["error"]
            .as_str()
            .unwrap()
            .contains("available worktree")
    );
    std::fs::write(s.directory.path().join("file.txt"), "changed\n").unwrap();
    let diff = s.call("git.diff", json!({ "projectId": s.project.id, "cwd": cwd }));
    assert!(
        diff.1["result"].as_str().unwrap().contains("+changed"),
        "{}",
        diff.1
    );
}

#[test]
fn retries_model_discovery_after_a_provider_becomes_available() {
    let s = setup(&[HarnessId::Codex]);
    s.backend.models.lock().unwrap().extend([
        Err("Login required".to_string()),
        Ok(vec![AgentModel::new(
            "codex:test",
            HarnessId::Codex,
            "Test",
        )]),
    ]);
    let first = s
        .call("models.list", json!({ "projectId": s.project.id }))
        .1;
    assert_eq!(first["result"]["errors"]["codex"], "Login required");
    let second = s
        .call("models.list", json!({ "projectId": s.project.id }))
        .1;
    assert_eq!(
        second["result"]["models"]["codex"],
        json!([{ "id": "codex:test", "harness": "codex", "name": "Test" }])
    );
    assert_eq!(s.backend.probes.load(Ordering::SeqCst), 2);
}

#[test]
fn advertises_newer_providers_only_to_desktops_that_request_them() {
    let s = setup(&[HarnessId::Codex, HarnessId::Cursor]);
    assert_eq!(
        s.call("environment.describe", json!({})).1["result"]["providers"],
        json!(["codex"])
    );
    assert_eq!(
        s.call(
            "environment.describe",
            json!({ "supportedProviders": ["codex", "cursor"] })
        )
        .1["result"]["providers"],
        json!(["codex", "cursor"])
    );
}

#[test]
fn re_probes_models_after_the_provider_cli_is_updated() {
    let s = setup(&[HarnessId::Codex]);
    let one = s.directory.path().join("codex-1");
    let two = s.directory.path().join("codex-2");
    std::fs::write(&one, "").unwrap();
    std::fs::write(&two, "").unwrap();
    s.backend
        .binaries
        .lock()
        .unwrap()
        .insert(HarnessId::Codex, one);
    s.backend.models.lock().unwrap().extend([
        Ok(vec![AgentModel::new("codex:old", HarnessId::Codex, "Old")]),
        Ok(vec![AgentModel::new("codex:new", HarnessId::Codex, "New")]),
    ]);
    let list = || {
        s.call("models.list", json!({ "projectId": s.project.id }))
            .1["result"]["models"]["codex"][0]["id"]
            .clone()
    };
    assert_eq!(list(), "codex:old");
    assert_eq!(list(), "codex:old");
    s.backend
        .binaries
        .lock()
        .unwrap()
        .insert(HarnessId::Codex, two);
    assert_eq!(list(), "codex:new");
    assert_eq!(s.backend.probes.load(Ordering::SeqCst), 2);
}

#[test]
fn re_probes_models_once_the_catalog_is_five_minutes_old() {
    let s = setup(&[HarnessId::Codex]);
    s.backend.models.lock().unwrap().extend([
        Ok(vec![AgentModel::new("codex:old", HarnessId::Codex, "Old")]),
        Ok(vec![AgentModel::new("codex:new", HarnessId::Codex, "New")]),
    ]);
    let list = || {
        s.call("models.list", json!({ "projectId": s.project.id }))
            .1["result"]["models"]["codex"][0]["id"]
            .clone()
    };
    assert_eq!(list(), "codex:old");
    s.clock.fetch_add(4 * 60_000, Ordering::SeqCst);
    assert_eq!(list(), "codex:old");
    s.clock.fetch_add(60_000, Ordering::SeqCst);
    assert_eq!(list(), "codex:new");
    assert_eq!(s.backend.probes.load(Ordering::SeqCst), 2);
}

#[test]
fn lets_an_authenticated_desktop_browse_host_folders_without_reading_files() {
    let s = setup(&[HarnessId::Codex]);
    let root = &s.project.cwd;
    std::fs::create_dir(Path::new(root).join("checkout")).unwrap();
    std::fs::write(Path::new(root).join("private.txt"), "secret").unwrap();
    let (status, listed) = s.call("projects.browse", json!({ "path": root }));
    assert_eq!(status, 200);
    assert_eq!(
        listed["result"]["entries"],
        json!([{ "name": "checkout", "path": Path::new(root).join("checkout") }])
    );
    assert_eq!(
        s.call_as("projects.browse", json!({ "path": root }), "invalid")
            .0,
        401
    );
}

#[test]
fn allows_a_different_client_to_recover_work_completed_while_the_laptop_was_disconnected() {
    let s = setup(&[HarnessId::Codex]);
    let id = s.create("create");
    let command = json!({ "type": "send", "commandId": "send", "sessionId": id, "text": "Work without this client" });
    assert_eq!(s.call("commands.dispatch", command.clone()).0, 200);
    s.backend.wait_for_turns(1, Duration::from_secs(5));
    s.backend.deliver(&id, "Finished on the host").unwrap();
    s.backend.finish(&id).unwrap();
    let recovered = s
        .call_as("sessions.get", json!({ "sessionId": id }), &s.second.token)
        .1;
    let blocks = recovered["result"]["session"]["blocks"].as_array().unwrap();
    assert_eq!(blocks.last().unwrap()["text"], "Finished on the host");
    assert!(recovered["result"].get("blockRevisions").is_some());
    let revision = recovered["result"]["revision"].clone();
    assert_eq!(
        s.call(
            "sessions.get",
            json!({ "sessionId": id, "revision": revision })
        )
        .1,
        json!({ "result": null })
    );
    s.call_as("commands.dispatch", command, &s.second.token);
    assert_eq!(s.backend.turns().len(), 1);
}

#[test]
fn rejects_revoked_devices_browser_origins_and_changed_host_identities() {
    let s = setup(&[HarnessId::Codex]);
    assert_eq!(
        s.call_as("environment.describe", json!({}), "invalid").0,
        401
    );
    assert_eq!(
        s.call_with(
            "environment.describe",
            json!({}),
            &s.first.token,
            json!({}),
            &[("Origin", "https://untrusted.example")]
        )
        .0,
        403
    );
    let changed = s.call_with(
        "projects.list",
        json!({}),
        &s.first.token,
        json!({ "environmentId": "different-host" }),
        &[],
    );
    assert!(
        changed.1["error"]
            .as_str()
            .unwrap()
            .contains("identity changed")
    );
    assert_eq!(
        s.call_with(
            "projects.list",
            json!({}),
            &s.first.token,
            json!({ "version": 2 }),
            &[]
        )
        .1["error"],
        "Incompatible protocol version"
    );
    s.store.revoke_device(&s.first.id).unwrap();
    assert_eq!(s.call("environment.describe", json!({})).0, 401);
    assert_eq!(
        s.call_as("environment.describe", json!({}), &s.second.token)
            .0,
        200
    );
}

#[test]
fn exchanges_a_pairing_code_for_a_device_credential_once() {
    let s = setup(&[HarnessId::Codex]);
    let code = s.store.issue_pairing(now_ms()).unwrap().code;
    let (status, paired) = s.pair(json!({ "code": code, "name": "Studio" }), "pair.exchange");
    assert_eq!(status, 200, "{paired}");
    assert_eq!(
        paired["result"]["environmentId"],
        s.store.environment_id.as_str()
    );
    assert_eq!(paired["result"]["name"], hostname().as_str());
    let token = paired["result"]["token"].as_str().unwrap().to_string();
    assert!(
        s.store
            .devices()
            .unwrap()
            .iter()
            .any(|device| device.name == "Studio")
    );
    let described = s.call_as("environment.describe", json!({}), &token).1["result"].clone();
    assert_eq!(described["hostVersion"], HOST_VERSION);
    assert_eq!(described["endpoints"], json!(["https://10.0.0.5:3774"]));
    assert_eq!(described["protocolVersion"], 1);
    assert_eq!(described["platform"], node_platform());
    assert!(
        described["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "changes.wait")
    );
    assert_eq!(
        s.pair(json!({ "code": code, "name": "Again" }), "pair.exchange")
            .0,
        401
    );
    // Without a credential, nothing but pairing is reachable.
    assert_eq!(s.pair(json!({}), "projects.list").0, 401);
}

#[test]
fn slows_repeated_failed_pairing_attempts() {
    let s = setup(&[HarnessId::Codex]);
    for _ in 0..30 {
        assert_eq!(
            s.pair(json!({ "code": "x".repeat(43) }), "pair.exchange").0,
            401
        );
    }
    let code = s.store.issue_pairing(now_ms()).unwrap().code;
    assert_eq!(s.pair(json!({ "code": code }), "pair.exchange").0, 429);
    // The throttle forgets failures after a minute.
    s.clock.fetch_add(61_000, Ordering::SeqCst);
    assert_eq!(s.pair(json!({ "code": code }), "pair.exchange").0, 200);
}

#[test]
fn answers_a_waiting_desktop_when_a_session_changes() {
    let s = setup(&[HarnessId::Codex]);
    let first = s.call("changes.wait", json!({})).1["result"].clone();
    assert_eq!(first["reset"], true);
    assert_eq!(first["sessions"], json!([]));
    let (boot, cursor) = (first["boot"].clone(), first["cursor"].clone());
    let waiting = std::thread::scope(|scope| {
        let waiter =
            scope.spawn(|| s.call("changes.wait", json!({ "boot": boot, "after": cursor })));
        std::thread::sleep(Duration::from_millis(50));
        let id = s.create("create");
        (waiter.join().unwrap(), id)
    });
    let ((_, changed), id) = waiting;
    let changed = changed["result"].clone();
    assert_eq!(changed["reset"], false);
    assert_eq!(changed["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(changed["sessions"][0]["id"], id.as_str());
    assert_eq!(changed["sessions"][0]["projectId"], s.project.id.as_str());
    let started = Instant::now();
    let idle = s
        .call(
            "changes.wait",
            json!({ "boot": boot, "after": changed["cursor"], "timeoutMs": 20 }),
        )
        .1;
    assert_eq!(idle["result"]["sessions"], json!([]));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn lets_a_desktop_revoke_only_its_own_credential_keeping_sessions() {
    let s = setup(&[HarnessId::Codex]);
    let id = s.create("create");
    assert_eq!(
        s.call("devices.revokeSelf", json!({})).1,
        json!({ "result": { "revoked": true } })
    );
    assert_eq!(s.call("environment.describe", json!({})).0, 401);
    let other = s
        .call_as("sessions.sync", json!({ "sessionId": id }), &s.second.token)
        .1;
    assert_eq!(other["result"]["kind"], "snapshot");
    assert_eq!(other["result"]["value"]["session"]["id"], id.as_str());
    let revision = other["result"]["value"]["revision"].clone();
    assert_eq!(
        s.call_as(
            "sessions.sync",
            json!({ "sessionId": id, "revision": revision }),
            &s.second.token
        )
        .1["result"],
        json!({ "kind": "unchanged", "revision": revision })
    );
}

#[test]
fn reads_host_files_through_the_backend_in_the_selected_working_copy() {
    let s = setup(&[HarnessId::Codex]);
    std::fs::write(Path::new(&s.project.cwd).join("hello.txt"), "from host").unwrap();
    assert_eq!(
        s.call(
            "files.read",
            json!({ "projectId": s.project.id, "cwd": s.project.cwd, "path": "hello.txt" })
        ),
        (200, json!({ "result": "from host" }))
    );
    assert_eq!(
        s.call("files.write", json!({ "projectId": s.project.id, "path": "hello.txt", "expected": "from host", "content": "edited" })),
        (200, json!({ "result": null }))
    );
    assert!(
        s.call(
            "files.read",
            json!({ "projectId": s.project.id, "path": ".." })
        )
        .1["error"]
            .as_str()
            .unwrap()
            .contains("outside")
    );
    assert!(
        s.call("files.list", json!({ "projectId": s.project.id, "cwd": s.directory.path().parent().unwrap(), "path": "" })).1["error"]
            .as_str()
            .unwrap()
            .contains("worktree")
    );
    assert_eq!(
        s.call(
            "files.read",
            json!({ "projectId": "missing", "path": "hello.txt" })
        )
        .1["error"],
        "Project is not registered on this machine"
    );
}

#[test]
fn routes_workspace_and_git_methods_to_the_backend() {
    let s = setup(&[HarnessId::Codex]);
    let project = json!(s.project.id);
    let cwd = json!(s.project.cwd);
    let call = |method: &str, extra: Value| {
        let mut params = json!({ "projectId": project });
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        s.call(method, params).1
    };
    let echoed = |method: &str, extra: Value| call(method, extra)["result"]["arguments"].clone();
    assert_eq!(echoed("git.branches", json!({})), json!({ "cwd": cwd }));
    assert_eq!(
        echoed("git.worktrees", json!({})),
        json!({ "projectCwd": cwd })
    );
    assert_eq!(
        echoed("files.list", json!({ "path": "src" })),
        json!({ "cwd": cwd, "path": "src" })
    );
    assert_eq!(echoed("files.index", json!({})), json!({ "cwd": cwd }));
    assert_eq!(
        echoed("files.search", json!({ "query": "app" })),
        json!({ "cwd": cwd, "query": "app" })
    );
    assert_eq!(
        echoed("files.searchContent", json!({ "query": "x" })),
        json!({ "cwd": cwd, "query": "x" })
    );
    assert_eq!(
        echoed(
            "files.create",
            json!({ "parent": "", "name": "a.md", "isDir": false })
        ),
        json!({ "cwd": cwd, "parent": "", "name": "a.md", "isDir": false })
    );
    assert_eq!(echoed("git.index", json!({})), json!({ "cwd": cwd }));
    assert_eq!(
        echoed("git.fileDiff", json!({ "path": "a", "staged": "yes" })),
        json!({ "cwd": cwd, "path": "a", "staged": false })
    );
    assert_eq!(
        call("git.action", json!({ "action": "stageAll" })),
        json!({ "result": null })
    );
    assert_eq!(
        s.call("workspace.run", json!({ "command": "nothing" })).1,
        json!({ "result": null })
    );
    assert_eq!(
        s.call(
            "workspace.run",
            json!({ "command": "list_dir", "args": { "path": cwd } })
        )
        .1["result"]["arguments"],
        json!({ "command": "list_dir", "args": { "path": cwd } })
    );
    assert_eq!(
        echoed(
            "git.worktreeCreate",
            json!({ "branch": "feature", "base": "HEAD", "existing": true })
        ),
        json!({ "projectCwd": cwd, "branch": "feature", "base": "HEAD", "existing": true, "cwd": cwd })
    );
    assert!(
        s.backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(method, _)| method == "invalidate_workspace_roots")
    );
    // A running session blocks branch changes until the desktop forces them.
    let id = s.create("running");
    s.call(
        "commands.dispatch",
        json!({ "type": "send", "commandId": "turn", "sessionId": id, "text": "Go" }),
    );
    let blocked = call("git.switch", json!({ "branch": "main" }));
    assert_eq!(
        blocked["error"],
        "\"New remote session\" is running on the host. Switching branches changes the files it is working on."
    );
    assert_eq!(
        echoed(
            "git.switch",
            json!({ "branch": "main", "remote": false, "force": true })
        ),
        json!({ "cwd": cwd, "branch": "main", "remote": false })
    );
    assert_eq!(
        echoed(
            "git.createBranch",
            json!({ "branch": "next", "force": true })
        ),
        json!({ "cwd": cwd, "branch": "next" })
    );
    assert_eq!(
        s.call("git.unknown", json!({})).1["error"],
        "Unsupported host method"
    );
}

#[test]
fn reads_events_after_a_cursor_and_rejects_bad_cursors() {
    let s = setup(&[HarnessId::Codex]);
    let id = s.create("create");
    let events = s
        .call("events.read", json!({ "sessionId": id, "after": 0 }))
        .1;
    assert_eq!(events["result"]["revision"], 1);
    assert_eq!(events["result"]["events"][0]["revision"], 1);
    assert_eq!(
        s.call("events.read", json!({ "sessionId": id, "after": -1 }))
            .1["error"],
        "Invalid event cursor"
    );
    assert!(
        s.call("events.read", json!({ "sessionId": id, "after": 9 }))
            .1["result"]["snapshot"]
            .is_object()
    );
}

#[test]
fn answers_lifecycle_only_from_loopback_and_refuses_other_routes() {
    let s = setup(&[HarnessId::Codex]);
    let port = s.listener.local_addr().port();
    let agent = ureq::AgentBuilder::new().build();
    let status = |result: Result<ureq::Response, ureq::Error>| match result {
        Ok(response) => response.status(),
        Err(ureq::Error::Status(status, _)) => status,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(
        status(agent.get(&format!("http://127.0.0.1:{port}/rpc")).call()),
        403
    );
    assert_eq!(
        status(
            agent
                .post(&format!("http://127.0.0.1:{port}/other"))
                .send_string("{}")
        ),
        403
    );
    // Without a lifecycle handler, /lifecycle is an unknown route.
    assert_eq!(
        status(
            agent
                .post(&format!("http://127.0.0.1:{port}/lifecycle"))
                .send_string("{}")
        ),
        403
    );
    let lifecycle: Lifecycle = Arc::new(|_| Response::new(200).body("ok"));
    let server = HostServer::new(
        s.backend.clone(),
        REMOTE_PROVIDERS.to_vec(),
        HostServerOptions {
            lifecycle: Some(lifecycle),
            ..Default::default()
        },
    );
    let local = listen_host(
        HttpServer::new(Arc::new(server)),
        HostListenerOptions {
            port: 0,
            bind: "127.0.0.1".into(),
            identity: None,
            loopback: None,
        },
    )
    .unwrap();
    let response = agent
        .post(&format!(
            "http://127.0.0.1:{}/lifecycle",
            local.local_addr().port()
        ))
        .send_string("{}")
        .unwrap();
    assert_eq!(response.into_string().unwrap(), "ok");
}

#[test]
fn parses_github_work_item_urls() {
    assert_eq!(
        parse_github_work_item_url("see https://GitHub.com/a/b/PULL/12."),
        Some((
            "pr".into(),
            "a/b".into(),
            12,
            "https://github.com/a/b/pull/12".into()
        ))
    );
    assert_eq!(
        parse_github_work_item_url("https://github.com/a/b/issues/0"),
        None
    );
    assert_eq!(
        parse_github_work_item_url("https://github.com/a/b/issues/12x"),
        None
    );
}
