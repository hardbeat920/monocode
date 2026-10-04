//! Port of host/provider-transport.test.ts and
//! host/opencode-transport.test.ts: the real provider adapters over this
//! host's process supervisor, against fake provider executables, through
//! the engine. Nothing contacts a paid model.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use monocode_core::HarnessId;
use monocode_remote::host::HostStore;
use monocode_remote::host::protocol::{HostSessionStatus, RemoteProvider};
use serde_json::{Value, json};

use crate::backend::HostEngineOptions;
use crate::engine::HostEngine;
use crate::testing::{fake_provider_script, fake_providers};

const FIXTURE: &str = r#"const readline = require('node:readline');
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
// Records what each turn actually received, so tests can prove that settings
// applied between turns reach the provider.
const record = value => require('node:fs').appendFileSync(require('node:path').join(__dirname, 'calls.log'), JSON.stringify(value) + '\n');
if (!process.argv.includes('app-server')) record({claudeArgs: process.argv.slice(2)});
readline.createInterface({input: process.stdin}).on('line', line => {
  const request = JSON.parse(line);
  if (request.jsonrpc === '2.0') {
    if (request.id == null) return;
    if (request.method === 'session/prompt') {
      send({jsonrpc: '2.0', method: 'session/update', params: {sessionId: 'fixture_acp', update: {sessionUpdate: 'agent_message_chunk', content: {type: 'text', text: 'Headless ACP completed'}}}});
      setTimeout(() => send({jsonrpc: '2.0', id: request.id, result: {stopReason: 'end_turn'}}), 30);
    } else {
      send({jsonrpc: '2.0', id: request.id, result: request.method === 'session/new' || request.method === 'session/load' || request.method === 'session/resume' ? {sessionId: 'fixture_acp', configOptions: []} : {}});
    }
    return;
  }
  if (request.method === 'initialize') send({id: request.id, result: {}});
  if (request.method === 'account/read') send({id: request.id, result: {account: {type: 'fixture'}, requiresOpenaiAuth: false}});
  if (request.method === 'model/list') send({id: request.id, result: {data: [{model: 'fixture-model', displayName: 'Fixture model', supportedReasoningEfforts: ['low', 'high']}], nextCursor: null}});
  if (request.method === 'thread/start' || request.method === 'thread/resume') send({id: request.id, result: {thread: {id: 'fixture-thread'}}});
  if (request.method === 'turn/start') {
    record({codexEffort: request.params.effort ?? null, codexText: (request.params.input || []).map(item => item.text || '').join('')});
    send({id: request.id, result: {turn: {id: 'fixture-turn'}}});
    setTimeout(() => {
      send({method: 'item/agentMessage/delta', params: {threadId: 'fixture-thread', turnId: 'fixture-turn', itemId: 'message', delta: 'Headless Codex completed'}});
      send({method: 'turn/completed', params: {threadId: 'fixture-thread', turn: {id: 'fixture-turn', status: 'completed'}}});
    }, 30);
  }
  if (request.type === 'control_request' && request.request.subtype === 'initialize') {
    send({type: 'system', subtype: 'init', session_id: 'fixture-claude'});
    send({type: 'control_response', response: {subtype: 'success', request_id: request.request_id}});
  }
  if (request.type === 'control_request' && request.request.subtype === 'list_models') send({type: 'control_response', response: {subtype: 'success', request_id: request.request_id, response: {models: [{value: 'claude-fixture-model', resolvedModel: 'claude-fixture-model', displayName: 'Fixture Claude'}]}}});
  if (request.type === 'user') setTimeout(() => {
    send({type: 'assistant', session_id: 'fixture-claude', message: {content: [{type: 'text', text: 'Headless Claude completed'}]}});
    send({type: 'result', subtype: 'success', session_id: 'fixture-claude'});
  }, 30);
  if (request.type === 'get_state') send({type: 'response', id: request.id, command: 'get_state', success: true, data: {sessionId: 'fixture_pi', model: {provider: 'openai', id: 'fixture-model', contextWindow: 100000}}});
  if (request.type === 'get_session_stats') send({type: 'response', id: request.id, command: 'get_session_stats', success: true, data: {contextWindow: 100000}});
  if (request.type === 'get_available_models') send({type: 'response', id: request.id, command: 'get_available_models', success: true, data: {models: [{provider: 'openai', id: 'fixture-model', name: 'Fixture model'}]}});
  if (request.type === 'prompt') {
    send({type: 'response', id: request.id, command: 'prompt', success: true, data: {}});
    setTimeout(() => {
      send({type: 'message_update', assistantMessageEvent: {type: 'text_delta', delta: 'Headless Pi completed'}});
      send({type: 'agent_settled'});
    }, 30);
  }
});
"#;

struct Host {
    directory: tempfile::TempDir,
    root: PathBuf,
    store: Arc<HostStore>,
    engine: HostEngine,
    binaries: std::collections::HashMap<RemoteProvider, PathBuf>,
}

impl Drop for Host {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Closing waits for running turns; a failed test may have left
            // one stuck, so only stop the processes.
            if let Some(harness) = self.engine.harness() {
                harness.close();
            }
        } else {
            self.engine.close();
        }
        self.store.close();
    }
}

impl Host {
    fn start(
        binaries: impl FnOnce(&Path) -> std::collections::HashMap<RemoteProvider, PathBuf>,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(directory.path()).unwrap();
        let binaries = binaries(&root);
        let store = Arc::new(HostStore::open(&root.join("host.db")).unwrap());
        let engine = HostEngine::start(
            store.clone(),
            HostEngineOptions {
                binaries: binaries.clone(),
                threads: 4,
            },
        )
        .unwrap();
        Self {
            directory,
            root,
            store,
            engine,
            binaries,
        }
    }

    fn create(
        &self,
        command_id: &str,
        harness: HarnessId,
        model: &str,
        settings: Option<Value>,
    ) -> String {
        let project = self
            .engine
            .open_project(&self.root.to_string_lossy())
            .unwrap();
        let mut command = json!({
            "type": "create",
            "commandId": command_id,
            "projectId": project.id,
            "harness": harness.as_str(),
            "model": model,
            "runtimeMode": "supervised",
        });
        if let Some(settings) = settings {
            command["modelSettings"] = settings;
        }
        self.engine.command(&command).unwrap().session_id
    }

    fn send(&self, session_id: &str, command_id: &str) {
        self.engine
            .command(&json!({
                "type": "send",
                "commandId": command_id,
                "sessionId": session_id,
                "text": "hello",
            }))
            .unwrap();
    }

    fn wait_idle(&self, session_id: &str, timeout: Duration) -> monocode_core::Session {
        let deadline = std::time::Instant::now() + timeout;
        while self.store.session(session_id).unwrap().status == HostSessionStatus::Running {
            if std::time::Instant::now() > deadline {
                let value = self.store.session(session_id).unwrap();
                panic!(
                    "{} is still running: {}",
                    value.session.harness,
                    serde_json::to_string_pretty(&value.session.blocks).unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let value = self.store.session(session_id).unwrap();
        assert_eq!(
            value.status,
            HostSessionStatus::Idle,
            "{:?}",
            value.session.blocks.last().map(|block| block.text.clone())
        );
        value.session.clone()
    }
}

fn assistant_count(session: &monocode_core::Session) -> usize {
    session
        .blocks
        .iter()
        .filter(|block| block.role == monocode_core::BlockRole::Assistant)
        .count()
}

fn last_text(session: &monocode_core::Session) -> String {
    session
        .blocks
        .last()
        .map(|block| block.text.clone())
        .unwrap_or_default()
}

/// provider-transport.test.ts: "discovers host models in parallel without
/// probe process collisions".
#[test]
fn discovers_host_models_in_parallel_without_probe_process_collisions() {
    let host = Host::start(|root| fake_providers(root, FIXTURE));
    let harness = host.engine.harness().unwrap().clone();
    let cwd = host.root.to_string_lossy().into_owned();
    let probes: Vec<_> = [
        HarnessId::Codex,
        HarnessId::Codex,
        HarnessId::Claude,
        HarnessId::Claude,
        HarnessId::Pi,
        HarnessId::Pi,
        HarnessId::Omp,
        HarnessId::Omp,
    ]
    .into_iter()
    .map(|provider| {
        let (harness, cwd) = (harness.clone(), cwd.clone());
        std::thread::spawn(move || harness.discover_models(provider, &cwd).unwrap())
    })
    .collect();
    let found: Vec<Vec<monocode_core::AgentModel>> = probes
        .into_iter()
        .map(|probe| probe.join().unwrap())
        .collect();
    for pair in found.chunks(2) {
        assert_eq!(pair[0], pair[1]);
    }
    assert_eq!(found[0][0].id, "codex:fixture-model");
    assert_eq!(
        found[2][0].native_id.as_deref(),
        Some("claude-fixture-model")
    );
    assert_eq!(found[4][0].id, "pi:openai/fixture-model");
    assert_eq!(found[6][0].id, "omp:openai/fixture-model");
    assert!(host.engine.catalog().has_live_catalog(HarnessId::Codex));
}

/// provider-transport.test.ts: "completes and resumes %s with no React or
/// Tauri process", for codex and claude.
#[test]
fn completes_and_resumes_codex_and_claude_with_no_app_process() {
    let host = Host::start(|root| fake_providers(root, FIXTURE));
    for harness in [HarnessId::Codex, HarnessId::Claude] {
        let model = format!("{}:test", harness.as_str());
        let id = host.create(
            &format!("create-{}", harness.as_str()),
            harness,
            &model,
            None,
        );
        for turn in 0..2 {
            host.send(&id, &format!("{}-{turn}", harness.as_str()));
            let state = host.wait_idle(&id, Duration::from_secs(20));
            assert_eq!(assistant_count(&state), turn + 1);
            assert!(
                last_text(&state).contains("completed"),
                "{}",
                last_text(&state)
            );
            assert!(state.provider_session_id.is_some());
        }
    }
}

/// provider-transport.test.ts: "completes and resumes %s over the host RPC
/// transport", for pi and omp.
#[test]
fn completes_and_resumes_pi_and_omp_over_the_host_rpc_transport() {
    let host = Host::start(|root| fake_providers(root, FIXTURE));
    for harness in [HarnessId::Pi, HarnessId::Omp] {
        let model = format!("{}:default", harness.as_str());
        let id = host.create(
            &format!("create-{}", harness.as_str()),
            harness,
            &model,
            None,
        );
        for turn in 0..2 {
            host.send(&id, &format!("{}-send-{turn}", harness.as_str()));
            let state = host.wait_idle(&id, Duration::from_secs(20));
            assert_eq!(assistant_count(&state), turn + 1);
            assert!(last_text(&state).contains("Headless Pi completed"));
            assert_eq!(state.provider_session_id.as_deref(), Some("fixture_pi"));
        }
    }
}

/// provider-transport.test.ts: "completes a %s turn over the headless ACP
/// transport".
#[test]
fn completes_acp_turns_over_the_headless_transport() {
    let host = Host::start(|root| fake_providers(root, FIXTURE));
    for harness in [
        HarnessId::Cursor,
        HarnessId::Grok,
        HarnessId::Fx,
        HarnessId::Hermes,
        HarnessId::Droid,
        HarnessId::Antigravity,
    ] {
        let model = format!("{}:default", harness.as_str());
        let id = host.create(
            &format!("create-{}", harness.as_str()),
            harness,
            &model,
            None,
        );
        host.send(&id, &format!("{}-send", harness.as_str()));
        let state = host.wait_idle(&id, Duration::from_secs(20));
        if cfg!(windows) && harness == HarnessId::Antigravity {
            assert!(
                last_text(&state)
                    .contains("Antigravity ACP server overrides are not supported on Windows.")
            );
            assert_eq!(assistant_count(&state), 0);
            assert_eq!(state.provider_session_id, None);
            continue;
        }
        assert!(
            last_text(&state).contains("Headless ACP completed"),
            "{harness}: {}",
            last_text(&state)
        );
        assert_eq!(state.provider_session_id.as_deref(), Some("fixture_acp"));
    }
}

/// provider-transport.test.ts: "uses %s reasoning effort applied between
/// turns on the next turn".
#[test]
fn uses_reasoning_effort_applied_between_turns_on_the_next_turn() {
    let host = Host::start(|root| fake_providers(root, FIXTURE));
    for (harness, setting) in [
        (HarnessId::Codex, "reasoningEffort"),
        (HarnessId::Claude, "effort"),
    ] {
        let log = host.binaries[&harness].parent().unwrap().join("calls.log");
        let model = format!("{}:test", harness.as_str());
        let id = host.create(
            &format!("effort-create-{}", harness.as_str()),
            harness,
            &model,
            Some(json!({ setting: "low" })),
        );
        let mut efforts: Vec<Option<String>> = Vec::new();
        for (turn, effort) in ["low", "high"].into_iter().enumerate() {
            if turn > 0 {
                host.engine
                    .command(&json!({
                        "type": "configure",
                        "commandId": format!("effort-configure-{}", harness.as_str()),
                        "sessionId": id,
                        "model": model,
                        "modelSettings": { setting: effort },
                        "runtimeMode": "supervised",
                    }))
                    .unwrap();
            }
            std::fs::write(&log, "").unwrap();
            host.send(&id, &format!("effort-{}-{turn}", harness.as_str()));
            host.wait_idle(&id, Duration::from_secs(20));
            let calls: Vec<Value> = std::fs::read_to_string(&log)
                .unwrap()
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if harness == HarnessId::Codex {
                // The first turn also starts an isolated title helper with
                // its own effort; the turn is the call that sent "hello".
                let call = calls
                    .iter()
                    .find(|call| call.get("codexText") == Some(&json!("hello")))
                    .unwrap();
                efforts.push(call["codexEffort"].as_str().map(str::to_string));
            } else {
                // The first turn also starts an isolated title helper; the
                // turn is the call without `--no-session-persistence`.
                let args: Vec<String> = calls
                    .iter()
                    .filter_map(|call| {
                        serde_json::from_value::<Vec<String>>(call.get("claudeArgs")?.clone()).ok()
                    })
                    .find(|args| !args.iter().any(|arg| arg == "--no-session-persistence"))
                    .unwrap();
                let at = args.iter().position(|arg| arg == "--effort");
                efforts.push(at.and_then(|at| args.get(at + 1).cloned()));
            }
        }
        assert_eq!(
            efforts,
            [Some("low".to_string()), Some("high".to_string())],
            "{harness}: {}",
            std::fs::read_to_string(&log).unwrap()
        );
        assert_eq!(
            host.store.session(&id).unwrap().session.model_settings[setting],
            "high"
        );
    }
    let _ = host.directory.path();
}

const OPENCODE: &str = r#"#!/usr/bin/env node
const http = require('node:http');
const args = process.argv.slice(2);
if (args.includes('--version')) { console.log('1.20.0'); process.exit(0); }
const port = Number(args.find(arg => arg.startsWith('--port='))?.slice(7));
const subscribers = new Set();
const session = {id: 'fixture_open', directory: process.cwd()};
const server = http.createServer((req, res) => {
  if (req.url?.startsWith('/event')) {
    res.writeHead(200, {'Content-Type': 'text/event-stream'});
    res.flushHeaders();
    subscribers.add(res);
    req.on('close', () => subscribers.delete(res));
    return;
  }
  const path = new URL(req.url, 'http://127.0.0.1').pathname;
  if (path === '/session' || path === '/session/fixture_open') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end(JSON.stringify(session));
    return;
  }
  if (path === '/session/fixture_open/message') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end('[]');
    return;
  }
  if (path === '/session/fixture_open/prompt_async') {
    res.writeHead(204); res.end();
    setTimeout(() => {
      const events = [
        {type: 'message.updated', properties: {info: {sessionID: 'fixture_open', id: 'msg', role: 'assistant'}}},
        {type: 'message.part.updated', properties: {part: {sessionID: 'fixture_open', id: 'part', messageID: 'msg', type: 'text', text: 'Headless OpenCode completed'}}},
        {type: 'session.status', properties: {sessionID: 'fixture_open', status: {type: 'idle'}}},
      ];
      for (const subscriber of subscribers) for (const event of events)
        subscriber.write('data: ' + JSON.stringify(event) + '\n\n');
    }, 30);
    return;
  }
  res.writeHead(204); res.end();
});
server.listen(port, '127.0.0.1', () => console.log('opencode server listening on http://127.0.0.1:' + port));
"#;

/// opencode-transport.test.ts: "runs an OpenCode session through the host
/// HTTP and SSE bridge".
#[test]
fn runs_an_opencode_session_through_the_host_http_and_sse_bridge() {
    let host = Host::start(|root| {
        let path = fake_provider_script(root, HarnessId::Opencode, OPENCODE);
        std::collections::HashMap::from([(HarnessId::Opencode, path)])
    });
    let id = host.create(
        "create-opencode",
        HarnessId::Opencode,
        "opencode:openai/fixture-model",
        None,
    );
    host.send(&id, "send-opencode");
    let state = host.wait_idle(&id, Duration::from_secs(20));
    assert!(
        last_text(&state).contains("Headless OpenCode completed"),
        "{}",
        last_text(&state)
    );
    assert_eq!(state.provider_session_id.as_deref(), Some("fixture_open"));
}

/// A provider that never finishes its turn: closing the host stops it, and
/// the turn settles as interrupted instead of keeping the host alive.
#[test]
fn close_interrupts_turns_that_never_finish() {
    const SILENT: &str = r#"const readline = require('node:readline');
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
readline.createInterface({input: process.stdin}).on('line', line => {
  const request = JSON.parse(line);
  if (request.method === 'initialize') send({id: request.id, result: {}});
  if (request.method === 'account/read') send({id: request.id, result: {account: {type: 'fixture'}, requiresOpenaiAuth: false}});
  if (request.method === 'thread/start' || request.method === 'thread/resume') send({id: request.id, result: {thread: {id: 'fixture-thread'}}});
  const started = () => require('node:fs').writeFileSync(require('node:path').join(__dirname, 'turn'), '');
  if (request.method === 'turn/start') { send({id: request.id, result: {turn: {id: 'fixture-turn'}}}); started(); }
  if (request.type === 'user') started();
  if (request.type === 'control_request' && request.request.subtype === 'initialize') {
    send({type: 'system', subtype: 'init', session_id: 'fixture-claude'});
    send({type: 'control_response', response: {subtype: 'success', request_id: request.request_id}});
  }
});
"#;
    for harness in [HarnessId::Claude, HarnessId::Codex] {
        let host = Host::start(|root| fake_providers(root, SILENT));
        let model = format!("{}:test", harness.as_str());
        let id = host.create("create", harness, &model, None);
        host.send(&id, "send");
        // Stop only once the provider is in the turn.
        let marker = host.binaries[&harness].parent().unwrap().join("turn");
        crate::testing::wait_for("the turn to start", Duration::from_secs(20), || {
            marker.exists()
        });
        // Let the adapter read the provider's answer to the turn request. A
        // stop while that request is still pending hits the gap that
        // `close_returns_when_providers_never_answer` covers.
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            host.store.session(&id).unwrap().status,
            HostSessionStatus::Running
        );
        let (closed, done) = std::sync::mpsc::channel();
        let engine = host.engine.clone();
        std::thread::spawn(move || {
            engine.close();
            let _ = closed.send(());
        });
        done.recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("{harness}: close did not return"));
        let value = host.store.session(&id).unwrap();
        assert_eq!(value.status, HostSessionStatus::Interrupted, "{harness}");
        assert_eq!(
            value.session.blocks.last().unwrap().text,
            "Host stopped. This turn was interrupted."
        );
    }
}

/// Providers that never answer at all: close still returns.
#[test]
fn close_returns_when_providers_never_answer() {
    const MUTE: &str = "setInterval(() => {}, 1000);\n";
    for harness in [
        HarnessId::Claude,
        HarnessId::Codex,
        HarnessId::Pi,
        HarnessId::Cursor,
    ] {
        let host = Host::start(|root| fake_providers(root, MUTE));
        let model = format!("{}:test", harness.as_str());
        let id = host.create("create", harness, &model, None);
        host.send(&id, "send");
        std::thread::sleep(Duration::from_millis(500));
        let (closed, done) = std::sync::mpsc::channel();
        let engine = host.engine.clone();
        std::thread::spawn(move || {
            engine.close();
            let _ = closed.send(());
        });
        done.recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("{harness}: close did not return"));
        let value = host.store.session(&id).unwrap();
        if matches!(harness, HarnessId::Claude | HarnessId::Pi) {
            assert_eq!(value.status, HostSessionStatus::Interrupted, "{harness}");
            continue;
        }
        // Codex and the ACP providers keep waiting on their first request
        // after the session is forgotten, so close gives up on them; the
        // next start marks the turn interrupted.
        assert_eq!(value.status, HostSessionStatus::Running, "{harness}");
        let restarted = HostEngine::new(
            host.store.clone(),
            Default::default(),
            Default::default(),
            std::sync::Arc::new(|_: futures::future::BoxFuture<'static, ()>| {}),
        )
        .unwrap();
        assert_eq!(
            host.store.session(&id).unwrap().status,
            HostSessionStatus::Interrupted
        );
        restarted.close();
    }
}
