//! Remote session recovery through pinned TLS, SQLite, and the real host engine.
//! A controlled provider supplies output without contacting a paid model.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{FutureExt as _, channel::oneshot};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent, SendTurnInput};
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{BlockRole, HarnessId};
use monocode_harness::core::{SharedCatalog, registry::EventSink};
use monocode_host::providers::ProviderFuture;
use monocode_host::runtime::HostRuntime;
use monocode_host::{HOST_VERSION, HostEngine, HostProvider, HostProviders};
use monocode_remote::host::HostStore;
use monocode_remote::host::http::HttpServer;
use monocode_remote::host::listener::{HostListener, HostListenerOptions, listen_host};
use monocode_remote::host::network::{PairingOffer, pairing_link};
use monocode_remote::host::protocol::{HostSession, HostSessionStatus, apply_session_sync};
use monocode_remote::host::server::{HostServerOptions, create_host_server};
use monocode_remote::host::store::now_ms;
use monocode_remote::host::tls::{HostIdentity, load_host_identity};
use monocode_remote::remote::{Remote, remote_pair, remote_request, remote_retry};
use parking_lot::Mutex;
use serde_json::{Value, json};

struct Turn {
    input: SendTurnInput,
    on_event: EventSink,
    finish: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct ControlledProvider {
    turns: Mutex<Vec<Turn>>,
    bindings: Mutex<Vec<(String, String, String)>>,
}

impl ControlledProvider {
    fn complete(&self, index: usize, text: &str) {
        let mut turns = self.turns.lock();
        let turn = &mut turns[index];
        (turn.on_event)(HarnessEvent::SessionProviderBound {
            provider_session_id: "recovery-provider-thread".into(),
        });
        (turn.on_event)(HarnessEvent::MessageDelta { text: text.into() });
        (turn.on_event)(HarnessEvent::MessageCompleted);
        turn.finish.take().unwrap().send(()).unwrap();
    }

    fn finish_session(&self, id: &str) {
        for turn in self.turns.lock().iter_mut() {
            if turn.input.session.session_id == id
                && let Some(finish) = turn.finish.take()
            {
                let _ = finish.send(());
            }
        }
    }
}

impl HostProvider for ControlledProvider {
    fn send(&self, input: SendTurnInput, on_event: EventSink) -> ProviderFuture<()> {
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

    fn cancel(&self, id: &str) -> ProviderFuture<()> {
        self.finish_session(id);
        async { Ok(()) }.boxed()
    }

    fn stop(&self, id: &str) -> ProviderFuture<()> {
        self.cancel(id)
    }

    fn bind(&self, id: &str, provider_id: &str, cwd: &str) {
        self.bindings
            .lock()
            .push((id.into(), provider_id.into(), cwd.into()));
    }

    fn approve(&self, _: &str, _: i64, _: ApprovalDecision) -> Result<(), String> {
        Ok(())
    }

    fn answer(&self, _: &str, _: i64, _: UserQuestionReply) -> Result<(), String> {
        Ok(())
    }
}

struct Host {
    store: Arc<HostStore>,
    provider: Arc<ControlledProvider>,
    engine: HostEngine,
    runtime: HostRuntime,
    http: Arc<HttpServer>,
    listener: HostListener,
    identity: HostIdentity,
}

impl Host {
    fn start(directory: &Path, port: u16) -> Self {
        let store = Arc::new(HostStore::open(&directory.join("host.db")).unwrap());
        let provider = Arc::new(ControlledProvider::default());
        let runtime = HostRuntime::new(2);
        let providers: HostProviders =
            [(HarnessId::Codex, provider.clone() as Arc<dyn HostProvider>)]
                .into_iter()
                .collect();
        let engine = HostEngine::new(
            store.clone(),
            providers,
            SharedCatalog::new(),
            runtime.spawner(),
        )
        .unwrap();
        let identity = load_host_identity(directory).unwrap();
        let http = create_host_server(
            Arc::new(engine.clone()),
            vec![HarnessId::Codex],
            HostServerOptions {
                version: HOST_VERSION.into(),
                ..Default::default()
            },
        );
        let listener = listen_host(
            http.clone(),
            HostListenerOptions {
                port,
                bind: "127.0.0.1".into(),
                identity: Some(identity.clone()),
                loopback: None,
            },
        )
        .unwrap();
        Self {
            store,
            provider,
            engine,
            runtime,
            http,
            listener,
            identity,
        }
    }

    fn pairing_link(&self) -> String {
        pairing_link(&PairingOffer {
            name: "Recovery fixture".into(),
            environment_id: self.store.environment_id.clone(),
            fingerprint: self.identity.fingerprint.clone(),
            code: self.store.issue_pairing(now_ms()).unwrap().code,
            endpoints: vec![format!("https://{}", self.listener.local_addr())],
        })
    }

    fn wait_idle(&self, id: &str) {
        wait_for("host turn settlement", || {
            self.store.session(id).unwrap().status == HostSessionStatus::Idle
        });
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.listener.close();
        self.http.close();
        self.http.close_all_connections();
        self.store.changes.close();
        self.engine.close();
        self.runtime.shutdown();
        self.store.close();
    }
}

fn wait_for(label: &str, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn request(remote: &Remote, machine: &str, method: &str, params: Value) -> Value {
    remote_request(remote, machine.into(), method.into(), params).unwrap()
}

fn sync(remote: &Remote, machine: &str, id: &str, known: Option<&HostSession>) -> HostSession {
    let response = request(
        remote,
        machine,
        "sessions.sync",
        json!({"sessionId": id, "revision": known.map(|session| session.revision)}),
    );
    apply_session_sync(known, serde_json::from_value(response).unwrap()).unwrap()
}

#[test]
fn a_saved_pairing_recovers_offline_work_and_resumes_after_a_host_restart_once() {
    let root = tempfile::tempdir().unwrap();
    let host_directory = root.path().join("host");
    let desktop_directory = root.path().join("desktop");
    let project_directory = root.path().join("project");
    std::fs::create_dir_all(&host_directory).unwrap();
    std::fs::create_dir_all(&project_directory).unwrap();
    let host = Host::start(&host_directory, 0);
    let port = host.listener.local_addr().port();
    let environment = host.store.environment_id.clone();
    let fingerprint = host.identity.fingerprint.clone();
    let remote = Remote::new(desktop_directory.clone());
    let machine = remote_pair(&remote, host.pairing_link(), "Recovery desktop".into()).unwrap();
    let public_machine = serde_json::to_value(machine).unwrap();
    assert!(public_machine.get("token").is_none());
    let machine = public_machine["id"].as_str().unwrap();
    let before = request(&remote, machine, "changes.wait", json!({}));
    let project = request(
        &remote,
        machine,
        "projects.open",
        json!({"cwd": project_directory}),
    );
    let created = request(
        &remote,
        machine,
        "commands.dispatch",
        json!({
            "type": "create", "commandId": "recovery-create", "projectId": project["id"],
            "harness": "codex", "model": "codex:recovery-model", "runtimeMode": "supervised"
        }),
    );
    let id = created["sessionId"].as_str().unwrap();
    let command = json!({
        "type": "send", "commandId": "recovery-send", "sessionId": id,
        "text": "Finish while this desktop is offline"
    });
    let receipt = request(&remote, machine, "commands.dispatch", command.clone());
    wait_for("first provider turn", || {
        host.provider.turns.lock().len() == 1
    });
    let running = sync(&remote, machine, id, None);
    assert_eq!(running.status, HostSessionStatus::Running);
    host.http.close_all_connections();
    drop(remote);
    host.provider.complete(0, "Completed while disconnected");
    host.wait_idle(id);

    let remote = Remote::new(desktop_directory);
    let changed = request(
        &remote,
        machine,
        "changes.wait",
        json!({"boot": before["boot"], "after": before["cursor"], "timeoutMs": 100}),
    );
    assert_eq!(changed["reset"], false);
    assert!(
        changed["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == id)
    );
    let recovered = sync(&remote, machine, id, Some(&running));
    assert_eq!(recovered.status, HostSessionStatus::Idle);
    assert_eq!(
        recovered.session.blocks.last().unwrap().text,
        "Completed while disconnected"
    );
    assert_eq!(
        request(&remote, machine, "commands.dispatch", command.clone()),
        receipt
    );
    assert_eq!(host.provider.turns.lock().len(), 1);
    drop(host);
    assert!(
        remote_request(
            &remote,
            machine.into(),
            "environment.describe".into(),
            json!({})
        )
        .is_err()
    );

    let host = Host::start(&host_directory, port);
    assert_eq!(host.identity.fingerprint, fingerprint);
    assert_eq!(host.store.environment_id, environment);
    assert_eq!(host.provider.bindings.lock().len(), 1);
    let binding = host.provider.bindings.lock()[0].clone();
    assert_eq!(
        (binding.0.as_str(), binding.1.as_str()),
        (id, "recovery-provider-thread")
    );
    assert_eq!(
        Path::new(&binding.2),
        dunce::canonicalize(project_directory).unwrap()
    );
    remote_retry(&remote, machine.into());
    let described = request(&remote, machine, "environment.describe", json!({}));
    assert_eq!(described["environmentId"], environment);
    let restarted = request(
        &remote,
        machine,
        "changes.wait",
        json!({"boot": changed["boot"], "after": changed["cursor"], "timeoutMs": 100}),
    );
    assert_eq!(restarted["reset"], true);
    assert_ne!(restarted["boot"], changed["boot"]);
    assert_eq!(sync(&remote, machine, id, Some(&recovered)), recovered);
    assert_eq!(
        request(&remote, machine, "commands.dispatch", command),
        receipt
    );
    assert!(host.provider.turns.lock().is_empty());

    let followup = json!({
        "type": "send", "commandId": "recovery-followup", "sessionId": id,
        "text": "Resume the same provider conversation"
    });
    let next_receipt = request(&remote, machine, "commands.dispatch", followup.clone());
    wait_for("resumed provider turn", || {
        host.provider.turns.lock().len() == 1
    });
    assert_eq!(
        host.provider.turns.lock()[0].input.text,
        "Resume the same provider conversation"
    );
    host.provider.complete(0, "Resumed after restart");
    host.wait_idle(id);
    let resumed = sync(&remote, machine, id, Some(&recovered));
    assert_eq!(resumed.status, HostSessionStatus::Idle);
    assert_eq!(resumed.session.model, "codex:recovery-model");
    assert_eq!(
        resumed.session.provider_session_id.as_deref(),
        Some("recovery-provider-thread")
    );
    assert_eq!(
        resumed
            .session
            .blocks
            .iter()
            .filter(|block| block.role == BlockRole::User)
            .count(),
        2
    );
    assert_eq!(
        resumed
            .session
            .blocks
            .iter()
            .filter(|block| block.role == BlockRole::Assistant)
            .count(),
        2
    );
    assert_eq!(
        request(&remote, machine, "commands.dispatch", followup),
        next_receipt
    );
    assert_eq!(host.provider.turns.lock().len(), 1);
}
