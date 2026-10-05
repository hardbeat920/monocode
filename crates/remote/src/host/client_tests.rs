//! The desktop client in `remote.rs` against this host server, over pinned
//! TLS, through the client's public functions. Includes the port of
//! host/large-sync.test.ts, which moved tens of megabytes through the
//! desktop's 16 MiB response cap.

use std::sync::Arc;
use std::time::{Duration, Instant};

use monocode_core::{Block, BlockRole};
use serde_json::{Value, json};

use super::backend::HostBackend;
use super::http::HttpServer;
use super::listener::{HostListener, HostListenerOptions, listen_host};
use super::network::{PairingOffer, pairing_link};
use super::protocol::{HostProject, HostSession, SessionSync, apply_session_sync};
use super::server::{HostServerOptions, create_host_server};
use super::store::{HostStore, now_ms};
use super::test_backend::TestBackend;
use super::tls::{HostIdentity, load_host_identity};
use crate::remote::{Remote, remote_pair, remote_request};
use monocode_core::HarnessId;

/// The desktop's cap on one host response.
const DESKTOP_LIMIT: usize = 16 * 1024 * 1024;

struct TlsHost {
    _directory: tempfile::TempDir,
    backend: Arc<TestBackend>,
    store: Arc<HostStore>,
    http: Arc<HttpServer>,
    listener: HostListener,
    identity: HostIdentity,
    project: HostProject,
}

impl Drop for TlsHost {
    fn drop(&mut self) {
        self.listener.close();
        self.http.close();
        self.http.close_all_connections();
        self.store.changes.close();
    }
}

impl TlsHost {
    fn start() -> Self {
        let directory = super::store::tests::temporary("monocode-client-host-");
        let identity = load_host_identity(directory.path()).unwrap();
        let store = Arc::new(HostStore::open(&directory.path().join("host.db")).unwrap());
        let backend = Arc::new(TestBackend::new(
            store.clone(),
            vec![HarnessId::Codex, HarnessId::Claude],
        ));
        let project = backend
            .open_project(directory.path().to_str().unwrap())
            .unwrap();
        let port = Arc::new(std::sync::atomic::AtomicU16::new(0));
        let advertised = port.clone();
        let http = create_host_server(
            backend.clone(),
            vec![HarnessId::Codex, HarnessId::Claude],
            HostServerOptions {
                endpoints: Some(Arc::new(move || {
                    vec![format!(
                        "https://127.0.0.1:{}",
                        advertised.load(std::sync::atomic::Ordering::SeqCst)
                    )]
                })),
                ..Default::default()
            },
        );
        let listener = listen_host(
            http.clone(),
            HostListenerOptions {
                port: 0,
                bind: "127.0.0.1".into(),
                identity: Some(identity.clone()),
                loopback: None,
            },
        )
        .unwrap();
        port.store(
            listener.local_addr().port(),
            std::sync::atomic::Ordering::SeqCst,
        );
        Self {
            _directory: directory,
            backend,
            store,
            http,
            listener,
            identity,
            project,
        }
    }

    fn link(&self, fingerprint: Option<&str>) -> String {
        pairing_link(&PairingOffer {
            name: "test host".into(),
            environment_id: self.store.environment_id.clone(),
            fingerprint: fingerprint.unwrap_or(&self.identity.fingerprint).into(),
            code: self.store.issue_pairing(now_ms()).unwrap().code,
            endpoints: vec![format!(
                "https://127.0.0.1:{}",
                self.listener.local_addr().port()
            )],
        })
    }
}

struct Desktop {
    _directory: tempfile::TempDir,
    remote: Remote,
    machine: String,
    methods: Vec<String>,
    largest: usize,
}

impl Desktop {
    fn pair(host: &TlsHost) -> Self {
        let directory = super::store::tests::temporary("monocode-desktop-");
        let remote = Remote::new(directory.path().to_path_buf());
        let machine = remote_pair(&remote, host.link(None), "Test desktop".into()).unwrap();
        let machine = serde_json::to_value(&machine).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        Self {
            _directory: directory,
            remote,
            machine,
            methods: Vec::new(),
            largest: 0,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.methods.push(method.into());
        let result = remote_request(&self.remote, self.machine.clone(), method.into(), params)?;
        self.largest = self
            .largest
            .max(serde_json::to_string(&result).unwrap().len());
        Ok(result)
    }

    /// `syncRemoteSession` from src/features/connections/model/connections.ts.
    fn sync(&mut self, session_id: &str, revision: Option<i64>) -> Result<SessionSync, String> {
        let response = self.request(
            "sessions.sync",
            json!({ "sessionId": session_id, "revision": revision }),
        )?;
        if response["kind"] != "chunked" {
            return serde_json::from_value(response).map_err(|error| error.to_string());
        }
        let length = response["length"].as_u64().unwrap() as usize;
        let mut pieces = String::new();
        let mut offset = 0;
        while offset < length {
            let chunk = self.request(
                "sessions.syncChunk",
                json!({ "sessionId": session_id, "transfer": response["transfer"], "offset": offset }),
            )?;
            let data = chunk["data"].as_str().unwrap_or("");
            if data.is_empty() {
                return Err("Session transfer ended early".into());
            }
            offset += monocode_core::js::len(data);
            pieces.push_str(data);
        }
        if offset != length {
            return Err("Session transfer has an unexpected length".into());
        }
        serde_json::from_str(&pieces).map_err(|error| error.to_string())
    }

    /// `loadRemoteSession`, without attachment previews.
    fn load(&mut self, session_id: &str, known: Option<&HostSession>) -> HostSession {
        let update = self
            .sync(session_id, known.map(|known| known.revision))
            .unwrap();
        match apply_session_sync(known, update) {
            Ok(session) => session,
            Err(_) => apply_session_sync(None, self.sync(session_id, None).unwrap()).unwrap(),
        }
    }
}

fn visible(value: &HostSession) -> HostSession {
    let mut value = value.clone();
    value.block_revisions = None;
    value
}

#[test]
fn the_desktop_client_pairs_over_pinned_tls_and_drives_the_rust_host() {
    let host = TlsHost::start();
    let mut desktop = Desktop::pair(&host);
    let described = desktop.request("environment.describe", json!({})).unwrap();
    assert_eq!(
        described["environmentId"],
        host.store.environment_id.as_str()
    );
    assert_eq!(described["providers"], json!(["codex", "claude"]));
    assert!(
        described["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "changes.wait")
    );
    assert_eq!(
        desktop.request("projects.list", json!({})).unwrap(),
        json!([{ "id": host.project.id, "cwd": host.project.cwd, "name": host.project.name }])
    );
    let first = desktop.request("changes.wait", json!({})).unwrap();
    assert_eq!(first["reset"], true);
    let created = desktop
        .request(
            "commands.dispatch",
            json!({
                "type": "create", "commandId": "c1", "projectId": host.project.id,
                "harness": "claude", "model": "claude:test", "runtimeMode": "supervised"
            }),
        )
        .unwrap();
    let session_id = created["sessionId"].as_str().unwrap().to_string();
    let changed = desktop
        .request(
            "changes.wait",
            json!({ "boot": first["boot"], "after": first["cursor"] }),
        )
        .unwrap();
    assert_eq!(changed["sessions"][0]["id"], session_id.as_str());
    assert_eq!(changed["sessions"][0]["status"], "idle");
    let started = Instant::now();
    let idle = desktop
        .request(
            "changes.wait",
            json!({ "boot": changed["boot"], "after": changed["cursor"], "timeoutMs": 300 }),
        )
        .unwrap();
    assert_eq!(idle["sessions"], json!([]));
    assert!(started.elapsed() >= Duration::from_millis(250));
    let listed = desktop
        .request("sessions.list", json!({ "projectId": host.project.id }))
        .unwrap();
    assert_eq!(listed[0]["harness"], "claude");
    let loaded = desktop.load(&session_id, None);
    assert_eq!(loaded, visible(&host.store.session(&session_id).unwrap()));
    let error = desktop
        .request("sessions.sync", json!({ "sessionId": "missing" }))
        .unwrap_err();
    assert_eq!(
        error,
        "Host rejected request: Session not found on this machine"
    );
    // The client forwards only the methods it knows.
    assert!(desktop.request("pair.exchange", json!({})).is_err());

    // A pairing code works once, and a link with another pin is refused
    // before any request is sent.
    let link = host.link(None);
    let other = super::store::tests::temporary("monocode-desktop-");
    let second = Remote::new(other.path().to_path_buf());
    remote_pair(&second, link.clone(), String::new()).unwrap();
    let reused = remote_pair(&second, link, String::new()).err().unwrap();
    assert!(reused.contains("already used"), "{reused}");
    let mismatch = remote_pair(&second, host.link(Some(&"A".repeat(43))), String::new())
        .err()
        .unwrap();
    assert!(mismatch.contains("different certificate"), "{mismatch}");

    assert_eq!(
        desktop.request("devices.revokeSelf", json!({})).unwrap(),
        json!({ "revoked": true })
    );
    let revoked = desktop.request("projects.list", json!({})).unwrap_err();
    assert!(revoked.contains("invalid or revoked"), "{revoked}");
}

/// Quotes, backslashes, newlines and control characters expand when a piece
/// is embedded as a JSON string; emoji must never be split between pieces.
fn text(size: usize, seed: &str) -> String {
    let unit = format!("{seed} \"quoted\" \\path\\ \u{1} naïve 🚀\n");
    let copies = size.div_ceil(monocode_core::js::len(&unit));
    monocode_core::js::slice_prefix(&unit.repeat(copies), size).to_string()
}

fn save_blocks(host: &TlsHost, session_id: &str, blocks: Vec<Block>) {
    host.store
        .transaction(|| {
            let mut next = (*host.store.session(session_id)?).clone();
            next.revision += 1;
            next.session.blocks = blocks;
            host.store.save(next, &json!({ "type": "fixture" }))?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn reopens_and_updates_a_session_whose_transcript_exceeds_16_mib() {
    let host = TlsHost::start();
    let session_id = host
        .backend
        .command(
            json!({
                "type": "create", "commandId": "create", "projectId": host.project.id,
                "harness": "codex", "model": "codex:test", "runtimeMode": "supervised"
            })
            .as_object()
            .unwrap(),
        )
        .unwrap()
        .session_id;
    let mut blocks: Vec<Block> = (0..24)
        .map(|index| {
            Block::new(
                format!("history-{index}"),
                BlockRole::Assistant,
                text(600_000, &format!("turn {index}")),
            )
        })
        .collect();
    // One block alone is larger than the desktop's response cap.
    blocks.push(Block::new("huge", BlockRole::Tool, text(17_500_000, "log")));
    save_blocks(&host, &session_id, blocks);
    let full = host.store.session(&session_id).unwrap();
    assert!(serde_json::to_string(&visible(&full)).unwrap().len() > DESKTOP_LIMIT * 2);

    let mut desktop = Desktop::pair(&host);
    desktop.methods.clear();
    let reopened = desktop.load(&session_id, None);
    assert!(reopened == visible(&full));
    assert!(
        desktop
            .methods
            .iter()
            .filter(|method| *method == "sessions.syncChunk")
            .count()
            > 4
    );
    assert!(desktop.largest <= DESKTOP_LIMIT);

    // A follow-up turn streams into the large session; only new blocks move.
    desktop.methods.clear();
    desktop.largest = 0;
    host.backend
        .command(
            json!({ "type": "send", "commandId": "follow-up", "sessionId": session_id, "text": "Summarize the log" })
                .as_object()
                .unwrap(),
        )
        .unwrap();
    host.backend.wait_for_turns(1, Duration::from_secs(5));
    host.backend.deliver(&session_id, "Summary").unwrap();
    host.backend.finish(&session_id).unwrap();
    let updated = desktop.load(&session_id, Some(&reopened));
    assert!(updated == visible(&host.store.session(&session_id).unwrap()));
    assert_eq!(updated.session.blocks.last().unwrap().text, "Summary");
    assert_eq!(desktop.methods, ["sessions.sync"]);
    assert!(desktop.largest < 64 * 1024, "{}", desktop.largest);
}

#[test]
fn streams_a_changed_block_larger_than_16_mib_as_a_bounded_delta() {
    let host = TlsHost::start();
    let session_id = host
        .backend
        .command(
            json!({
                "type": "create", "commandId": "create", "projectId": host.project.id,
                "harness": "codex", "model": "codex:test", "runtimeMode": "supervised"
            })
            .as_object()
            .unwrap(),
        )
        .unwrap()
        .session_id;
    let mut desktop = Desktop::pair(&host);
    let base = desktop.load(&session_id, None);
    save_blocks(
        &host,
        &session_id,
        vec![Block::new("huge", BlockRole::Tool, text(17_000_000, "out"))],
    );
    desktop.methods.clear();
    desktop.largest = 0;
    let next = desktop.load(&session_id, Some(&base));
    assert!(next == visible(&host.store.session(&session_id).unwrap()));
    assert_eq!(desktop.methods[0], "sessions.sync");
    assert!(
        desktop
            .methods
            .iter()
            .any(|method| method == "sessions.syncChunk")
    );
    assert!(desktop.largest <= DESKTOP_LIMIT);
}
