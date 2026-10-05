//! End to end with real Claude Code: `serve` runs [`HostEngine`] on a
//! temporary data directory, a desktop pairs through `monocode_remote`'s
//! client over pinned TLS, creates a Claude session in a temporary project,
//! sends one short prompt, and syncs the reply.
//!
//! Run with `cargo test -p monocode-host --test live -- --ignored`. It needs
//! a signed-in `claude` CLI on this machine.

use std::sync::Arc;
use std::time::{Duration, Instant};

use monocode_host::{HOST_VERSION, HostEngine, HostEngineOptions};
use monocode_remote::host::network::{
    NetworkSettings, PairingOffer, pairing_link, write_network_settings,
};
use monocode_remote::host::owner::acquire_host_owner;
use monocode_remote::host::protocol::{
    HostSession, HostSessionStatus, SessionSync, apply_session_sync,
};
use monocode_remote::host::store::now_ms;
use monocode_remote::host::tls::load_host_identity;
use monocode_remote::host::{HostOptions, HostStore, serve};
use monocode_remote::remote::{Remote, remote_pair, remote_request};
use serde_json::{Value, json};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
#[ignore = "runs the real Claude Code CLI"]
fn a_paired_desktop_runs_a_claude_turn_on_the_rust_host() {
    let temporary = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temporary.path()).unwrap();
    let directory = root.join("host");
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("README.md"), "A scratch project.\n").unwrap();
    std::fs::create_dir(&directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    // Network access on loopback, so the front listener speaks TLS there.
    write_network_settings(
        &directory,
        &NetworkSettings {
            enabled: true,
            bind: "127.0.0.1".into(),
        },
    )
    .unwrap();
    let port = free_port();

    // The order `monocode-host serve` uses: own the directory, then build
    // the engine, then serve.
    let store = Arc::new(HostStore::open(&directory.join("host.db")).unwrap());
    let owner = acquire_host_owner(&directory).unwrap();
    let engine = HostEngine::start(store.clone(), HostEngineOptions::default()).unwrap();
    let handle = serve(
        HostOptions {
            directory: directory.clone(),
            port,
            version: HOST_VERSION.into(),
            owner,
        },
        engine.clone(),
    )
    .unwrap();
    assert!(
        handle
            .providers()
            .contains(&monocode_core::HarnessId::Claude),
        "claude is not installed here"
    );

    let identity = load_host_identity(&directory).unwrap();
    let link = pairing_link(&PairingOffer {
        name: "live host".into(),
        environment_id: store.environment_id.clone(),
        fingerprint: identity.fingerprint.clone(),
        code: store.issue_pairing(now_ms()).unwrap().code,
        endpoints: vec![format!("https://127.0.0.1:{port}")],
    });
    let desktop_directory = root.join("desktop");
    std::fs::create_dir(&desktop_directory).unwrap();
    let remote = Remote::new(desktop_directory);
    let machine = remote_pair(&remote, link, "Live desktop".into()).unwrap();
    let machine = serde_json::to_value(&machine).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let request = |method: &str, params: Value| {
        remote_request(&remote, machine.clone(), method.into(), params)
    };

    let described = request("environment.describe", json!({})).unwrap();
    assert_eq!(described["environmentId"], json!(store.environment_id));
    let opened = request("projects.open", json!({ "cwd": project })).unwrap();
    let project_id = opened["id"].as_str().unwrap().to_string();
    let created = request(
        "commands.dispatch",
        json!({
            "type": "create",
            "commandId": "live-create",
            "projectId": project_id,
            "harness": "claude",
            "model": "claude:haiku",
            "runtimeMode": "supervised",
        }),
    )
    .unwrap();
    let session_id = created["sessionId"].as_str().unwrap().to_string();
    request(
        "commands.dispatch",
        json!({
            "type": "send",
            "commandId": "live-send",
            "sessionId": session_id,
            "text": "Reply with the single word pong and nothing else. Do not use tools.",
        }),
    )
    .unwrap();

    // Follow the session the way an open tab does: deltas from the last
    // revision, a snapshot when a delta does not apply.
    let mut known: Option<HostSession> = None;
    let deadline = Instant::now() + Duration::from_secs(180);
    let settled = loop {
        let response = request(
            "sessions.sync",
            json!({ "sessionId": session_id, "revision": known.as_ref().map(|value| value.revision) }),
        )
        .unwrap();
        let sync: SessionSync = serde_json::from_value(response).unwrap();
        known = Some(match apply_session_sync(known.as_ref(), sync) {
            Ok(value) => value,
            Err(_) => {
                let snapshot =
                    request("sessions.sync", json!({ "sessionId": session_id })).unwrap();
                apply_session_sync(None, serde_json::from_value(snapshot).unwrap()).unwrap()
            }
        });
        let value = known.as_ref().unwrap();
        if value.status != HostSessionStatus::Running {
            break value.clone();
        }
        assert!(Instant::now() < deadline, "the turn did not finish in time");
        std::thread::sleep(Duration::from_millis(500));
    };

    let transcript: Vec<String> = settled
        .session
        .blocks
        .iter()
        .map(|block| format!("{:?}: {}", block.role, block.text))
        .collect();
    eprintln!("synced transcript: {transcript:#?}");
    assert_eq!(settled.status, HostSessionStatus::Idle, "{transcript:#?}");
    let reply = settled
        .session
        .blocks
        .iter()
        .filter(|block| block.role == monocode_core::BlockRole::Assistant)
        .map(|block| block.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(reply.contains("pong"), "{transcript:#?}");
    assert!(settled.session.provider_session_id.is_some());
    // The host's own copy matches what the desktop synced.
    assert_eq!(
        store.session(&session_id).unwrap().revision,
        settled.revision
    );

    handle.stop();
    handle.wait();
}
