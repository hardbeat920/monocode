//! Compares this host with the TypeScript host in `build/host`, on one data
//! directory: the Node host creates it, pairs a desktop, and creates a
//! session; this host then serves the same directory and port, and must
//! answer the same requests the same way, keep the desktop's pairing
//! working, and leave data the Node host can read again.
//!
//! Needs Node and `npm run host:build`. Run it with
//! `cargo test -p monocode-remote node_compat -- --ignored --nocapture`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::cli::{HostOptions, serve};
use super::exec::{ExecOptions, exec};
use super::owner::acquire_host_owner;
use super::protocol::parse_provider;
use super::server::tests::post;
use super::store::HostStore;
use super::test_backend::TestBackend;
use super::tls::load_host_identity;
use crate::remote::{Remote, remote_pair, remote_request};

fn node_cli() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/host/monocode-host.mjs")
}

fn node(data: &Path, port: u16, args: &[&str]) -> String {
    let cli = node_cli();
    let data = data.to_string_lossy();
    let port = port.to_string();
    let mut all = vec![cli.to_str().unwrap()];
    all.extend_from_slice(args);
    all.extend(["--data-dir", &data, "--port", &port]);
    match exec(
        "node",
        &all,
        ExecOptions {
            timeout: Duration::from_secs(60),
            ..Default::default()
        },
    ) {
        Ok(output) => output.stdout,
        Err(error) => panic!("node {args:?} failed: {}\n{}", error.message, error.stdout),
    }
}

fn wait_stopped(data: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while data.join("running.json").exists() {
        assert!(Instant::now() < deadline, "the host did not stop");
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct Probe {
    url: String,
    token: String,
    environment_id: String,
}

impl Probe {
    fn call(&self, method: &str, params: Value) -> (u16, Value) {
        post(
            &self.url,
            &[("Authorization", &format!("Bearer {}", self.token))],
            &json!({ "version": 1, "environmentId": self.environment_id, "method": method, "params": params })
                .to_string(),
        )
    }

    /// Requests whose answers must match between the hosts.
    fn reads(&self, project: &Value, session: &str, cwd: &str) -> Vec<(String, (u16, Value))> {
        let mut answers = vec![
            ("describe".into(), self.call("environment.describe", json!({}))),
            (
                "describe all".into(),
                self.call(
                    "environment.describe",
                    json!({ "supportedProviders": ["codex", "claude", "cursor", "grok", "opencode", "pi", "omp", "fx", "hermes", "droid", "antigravity"] }),
                ),
            ),
            ("projects.list".into(), self.call("projects.list", json!({}))),
            ("projects.open".into(), self.call("projects.open", json!({ "cwd": cwd }))),
            ("sessions.list".into(), self.call("sessions.list", json!({ "projectId": project }))),
            ("sessions.sync".into(), self.call("sessions.sync", json!({ "sessionId": session }))),
            ("sessions.get".into(), self.call("sessions.get", json!({ "sessionId": session }))),
            ("events.read".into(), self.call("events.read", json!({ "sessionId": session, "after": 0 }))),
            (
                "attachments.upload retry".into(),
                self.call(
                    "attachments.upload",
                    json!({ "id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "offset": 0, "size": 5, "data": "aGVsbG8=" }),
                ),
            ),
            ("missing session".into(), self.call("sessions.sync", json!({ "sessionId": "missing" }))),
            ("missing project".into(), self.call("sessions.list", json!({ "projectId": "missing" }))),
            ("no changes".into(), self.call("sessions.update", json!({ "projectId": project, "sessionId": session }))),
            ("bad cursor".into(), self.call("events.read", json!({ "sessionId": session, "after": -1 }))),
            ("bad method".into(), self.call("git.arbitrary", json!({}))),
            (
                "bad version".into(),
                post(
                    &self.url,
                    &[("Authorization", &format!("Bearer {}", self.token))],
                    &json!({ "version": 2, "method": "projects.list" }).to_string(),
                ),
            ),
            (
                "other host".into(),
                post(
                    &self.url,
                    &[("Authorization", &format!("Bearer {}", self.token))],
                    &json!({ "version": 1, "environmentId": "other", "method": "projects.list" }).to_string(),
                ),
            ),
            (
                "revoked".into(),
                post(
                    &self.url,
                    &[("Authorization", &format!("Bearer {}", "x".repeat(43)))],
                    &json!({ "version": 1, "method": "environment.describe" }).to_string(),
                ),
            ),
            (
                "origin".into(),
                post(
                    &self.url,
                    &[("Authorization", format!("Bearer {}", self.token).as_str()), ("Origin", "https://example.com")],
                    &json!({ "version": 1, "method": "environment.describe" }).to_string(),
                ),
            ),
            (
                "bad pairing".into(),
                post(
                    &self.url,
                    &[],
                    &json!({ "version": 1, "method": "pair.exchange", "params": { "code": "x".repeat(43) } }).to_string(),
                ),
            ),
        ];
        let first = self.call("changes.wait", json!({}));
        let shape = |value: &Value| {
            value
                .get("result")
                .and_then(Value::as_object)
                .map(|fields| fields.keys().cloned().collect::<Vec<_>>())
        };
        answers.push((
            "changes.wait keys".into(),
            (first.0, json!(shape(&first.1))),
        ));
        answers
    }
}

#[test]
#[ignore = "needs Node and build/host; compares with the TypeScript host"]
fn node_compat_the_rust_host_takes_over_a_node_host_data_directory() {
    if !node_cli().exists() || exec("node", &["--version"], ExecOptions::default()).is_err() {
        eprintln!("Skipping: run npm run host:build first");
        return;
    }
    let root = super::store::tests::temporary("monocode-node-compat-");
    let data = root.path().join("data");
    let project_dir = root.path().join("project");
    std::fs::create_dir(&data).unwrap();
    std::fs::create_dir(&project_dir).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    // The Node host sets up the directory and prints a pairing link.
    let connected: Value = serde_json::from_str(
        node(
            &data,
            port,
            &["connect", "--no-service", "--json", "--bind", "127.0.0.1"],
        )
        .lines()
        .last()
        .unwrap(),
    )
    .unwrap();
    let device: Value =
        serde_json::from_str(&node(&data, port, &["pair", "--name", "Raw", "--json"])).unwrap();
    let desktop_dir = super::store::tests::temporary("monocode-node-compat-desktop-");
    let remote = Remote::new(desktop_dir.path().to_path_buf());
    let machine = remote_pair(
        &remote,
        connected["link"].as_str().unwrap().into(),
        String::new(),
    )
    .unwrap();
    let machine = serde_json::to_value(&machine).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let desktop = |method: &str, params: Value| {
        let remote = Remote::new(desktop_dir.path().to_path_buf());
        remote_request(&remote, machine.clone(), method.into(), params)
    };
    let probe = Probe {
        url: format!("http://127.0.0.1:{port}/rpc"),
        token: device["token"].as_str().unwrap().into(),
        environment_id: device["environmentId"].as_str().unwrap().into(),
    };
    let described = probe.call("environment.describe", json!({ "supportedProviders": ["codex", "claude", "cursor", "grok", "opencode", "pi", "omp", "fx", "hermes", "droid", "antigravity"] })).1;
    let providers: Vec<_> = described["result"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|provider| provider.as_str().and_then(parse_provider))
        .collect();
    assert!(!providers.is_empty(), "the Node host found no provider CLI");
    let harness = described["result"]["providers"][0]
        .as_str()
        .unwrap()
        .to_string();

    let cwd = std::fs::canonicalize(&project_dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let project = desktop("projects.open", json!({ "cwd": cwd })).unwrap()["id"].clone();
    let created = desktop(
        "commands.dispatch",
        json!({
            "type": "create", "commandId": "compat-create", "projectId": project,
            "harness": harness, "model": format!("{harness}:test"), "runtimeMode": "supervised"
        }),
    )
    .unwrap();
    let session = created["sessionId"].as_str().unwrap().to_string();
    desktop(
        "sessions.update",
        json!({
            "projectId": project, "sessionId": session, "title": "Made by Node", "pinned": true,
            "linkedWorkItem": { "kind": "issue", "repo": "a/b", "number": 7, "url": "https://github.com/a/b/issues/7" }
        }),
    )
    .unwrap();
    let node_answers = probe.reads(&project, &session, &cwd);
    let node_fingerprint = connected["fingerprint"].as_str().unwrap().to_string();
    node(&data, port, &["stop"]);
    wait_stopped(&data);

    // This host takes over the same directory and port.
    let owner = acquire_host_owner(&data).unwrap();
    let store = Arc::new(HostStore::open(&data.join("host.db")).unwrap());
    assert_eq!(store.environment_id, probe.environment_id);
    assert_eq!(
        load_host_identity(&data).unwrap().fingerprint,
        node_fingerprint
    );
    let handle = serve(
        HostOptions {
            directory: data.clone(),
            port,
            version: super::server::HOST_VERSION.into(),
            owner,
        },
        TestBackend::new(store, providers),
    )
    .unwrap();
    let rust_answers = probe.reads(&project, &session, &cwd);
    let mut differences = Vec::new();
    for ((name, node), (_, rust)) in node_answers.iter().zip(&rust_answers) {
        if node == rust {
            eprintln!("same: {name}");
        } else {
            differences.push(format!("{name}\n  node: {node:?}\n  rust: {rust:?}"));
        }
    }
    // The desktop paired with the Node host keeps working: same pin, same
    // credential, same environment.
    let listed = desktop("sessions.list", json!({ "projectId": project })).unwrap();
    assert_eq!(listed[0]["title"], "Made by Node");
    desktop(
        "sessions.update",
        json!({ "projectId": project, "sessionId": session, "title": "Changed by Rust" }),
    )
    .unwrap();
    let state = super::control::read_running(&data).unwrap();
    assert_eq!(state.port, port);
    // The Node CLI stops this host through its lifecycle endpoint.
    assert!(node(&data, port, &["status"]).contains("is running"));
    node(&data, port, &["stop"]);
    handle.wait();
    wait_stopped(&data);

    // The Node host reads what this host wrote.
    node(&data, port, &["start"]);
    let synced = probe
        .call("sessions.sync", json!({ "sessionId": session }))
        .1;
    assert_eq!(
        synced["result"]["value"]["session"]["title"],
        "Changed by Rust"
    );
    assert_eq!(
        desktop("sessions.list", json!({ "projectId": project })).unwrap()[0]["title"],
        "Changed by Rust"
    );
    node(&data, port, &["stop"]);
    wait_stopped(&data);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
