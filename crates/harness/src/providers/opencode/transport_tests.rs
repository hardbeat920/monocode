//! Port of host/opencode-transport.test.ts, and the live test.
//!
//! The transport test drives one turn through the real process supervisor
//! (`HostChildBackend` over `HarnessHost`), real loopback HTTP, and a real
//! SSE stream. The TypeScript fixture was a Node script that served the API
//! itself. Here the API runs on a thread in the test, and the fixture
//! `opencode` is a shell script that prints that server's URL and stays
//! alive, so the test needs only `/bin/sh`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;

use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};

use super::adapter::OpenCodeAdapter;
use super::catalog::discover_open_code_models;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{Children, HostChildOptions};
use crate::core::registry::{HarnessAdapter, TextPromptInput, event_sink};
use crate::core::task::{SharedSpawner, SmolSpawner, timeout};

/// The fixture API: the routes the TypeScript fixture served.
struct FixtureServer {
    port: u16,
}

impl FixtureServer {
    fn start(directory: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let subscribers: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let directory = directory.to_string_lossy().into_owned();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let subscribers = subscribers.clone();
                let directory = directory.clone();
                thread::spawn(move || serve(stream, &subscribers, &directory));
            }
        });
        Self { port }
    }
}

fn respond(mut stream: TcpStream, status: &str, body: &str) {
    let content_type = if body.is_empty() {
        ""
    } else {
        "Content-Type: application/json\r\n"
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn serve(stream: TcpStream, subscribers: &Mutex<Vec<TcpStream>>, directory: &str) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut content_length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; content_length];
    let _ = reader.read_exact(&mut body);
    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or(target);

    if path.starts_with("/event") {
        let mut stream = stream;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n"
        );
        let _ = stream.flush();
        subscribers.lock().push(stream);
        return;
    }
    let session = json!({ "id": "fixture_open", "directory": directory }).to_string();
    match path {
        "/session" | "/session/fixture_open" => respond(stream, "200 OK", &session),
        "/session/fixture_open/message" => respond(stream, "200 OK", "[]"),
        "/session/fixture_open/prompt_async" => {
            respond(stream, "204 No Content", "");
            thread::sleep(Duration::from_millis(30));
            let events = [
                json!({ "type": "message.updated", "properties": { "info": { "sessionID": "fixture_open", "id": "msg", "role": "assistant" } } }),
                json!({ "type": "message.part.updated", "properties": { "part": {
                    "sessionID": "fixture_open", "id": "part", "messageID": "msg", "type": "text", "text": "Headless OpenCode completed",
                } } }),
                json!({ "type": "session.status", "properties": { "sessionID": "fixture_open", "status": { "type": "idle" } } }),
            ];
            for subscriber in subscribers.lock().iter_mut() {
                for event in &events {
                    let _ = write!(subscriber, "data: {event}\n\n");
                }
                let _ = subscriber.flush();
            }
        }
        _ => respond(stream, "204 No Content", ""),
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("monocode-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fixture_binary(dir: &Path, port: u16) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let binary = dir.join("opencode");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 1.20.0; exit 0; fi\n\
             echo 'opencode server listening on http://127.0.0.1:{port}'\nexec sleep 600\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    binary
}

/// Children over a real `HarnessHost`, with `binary` as the configured
/// OpenCode when given.
fn host_children(data_dir: &Path, binary: Option<&Path>) -> (Children, SharedSpawner) {
    let spawner: SharedSpawner = Arc::new(SmolSpawner);
    let (children, host) = Children::for_host(
        HostChildOptions {
            data_dir: data_dir.to_path_buf(),
            ..Default::default()
        },
        spawner.clone(),
    );
    let paths: HashMap<String, String> = binary
        .map(|binary| {
            HashMap::from([(
                "opencode".to_string(),
                binary.to_string_lossy().into_owned(),
            )])
        })
        .unwrap_or_default();
    monocode_process::harness::harness_runtime_binary_paths(&host, paths);
    (children, spawner)
}

fn turn(session_id: &str, cwd: &Path, model: &str, text: &str) -> SendTurnInput {
    SendTurnInput {
        session: HarnessSessionInput {
            session_id: session_id.into(),
            cwd: cwd.to_string_lossy().into_owned(),
            model: model.into(),
            model_settings: None,
            provider_account_id: None,
            runtime_mode: RuntimeMode::Supervised,
            intent: None,
            controls_agents: None,
            app_access: None,
        },
        text: text.into(),
        attachments: None,
    }
}

#[test]
fn runs_an_opencode_session_through_the_host_http_and_sse_bridge() {
    let directory = temp_dir("opencode-transport");
    let server = FixtureServer::start(&directory);
    let binary = write_fixture_binary(&directory, server.port);
    let (children, spawner) = host_children(&directory, Some(&binary));
    let adapter = OpenCodeAdapter::new(children, SharedCatalog::new(), spawner, None);
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
    let sink = {
        let events = events.clone();
        event_sink(move |event| events.lock().push(event))
    };

    let result = smol::block_on(timeout(
        Duration::from_secs(8),
        adapter.send_turn(
            turn(
                "opencode-transport",
                &directory,
                "opencode:openai/fixture-model",
                "hello",
            ),
            sink,
            None,
        ),
    ));
    smol::block_on(adapter.stop_session("opencode-transport".into())).unwrap();
    let _ = std::fs::remove_dir_all(&directory);

    result.expect("turn timed out").unwrap();
    let events = events.lock().clone();
    assert!(events.contains(&HarnessEvent::SessionProviderBound {
        provider_session_id: "fixture_open".into()
    }));
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            HarnessEvent::MessageDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains("Headless OpenCode completed"), "{events:?}");
    assert!(events.contains(&HarnessEvent::MessageCompleted));
}

/// One short turn against a real OpenCode 1.x. Set `MONOCODE_OPENCODE_BIN`
/// to an absolute path to use a binary other than the one MonoCode finds,
/// and `MONOCODE_OPENCODE_MODEL` (`opencode:provider/model`) to pick the
/// model; otherwise the first catalog model runs.
#[test]
#[ignore = "runs the real OpenCode CLI and a model"]
fn live_turn_with_a_real_opencode() {
    let directory = temp_dir("opencode-live");
    let binary = std::env::var_os("MONOCODE_OPENCODE_BIN").map(PathBuf::from);
    let (children, spawner) = host_children(&directory, binary.as_deref());
    let catalog = SharedCatalog::new();
    let models = smol::block_on(discover_open_code_models(
        &children,
        Some(&directory.to_string_lossy()),
    ))
    .expect("opencode models");
    assert!(!models.is_empty(), "OpenCode reported no models");
    catalog.set_harness_models(HarnessId::Opencode, models.clone());
    let model = std::env::var("MONOCODE_OPENCODE_MODEL").unwrap_or_else(|_| models[0].id.clone());
    eprintln!("{} models, using {model}", models.len());

    let adapter = OpenCodeAdapter::new(children, catalog, spawner, None);
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
    let sink = {
        let events = events.clone();
        event_sink(move |event| events.lock().push(event))
    };
    let result = smol::block_on(timeout(
        Duration::from_secs(180),
        adapter.send_turn(
            turn(
                "opencode-live",
                &directory,
                &model,
                "Reply with exactly one word: pong. Do not use any tools.",
            ),
            sink,
            None,
        ),
    ));
    smol::block_on(adapter.stop_session("opencode-live".into())).unwrap();
    let _ = std::fs::remove_dir_all(&directory);

    let events = events.lock().clone();
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            HarnessEvent::MessageDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    eprintln!("reply: {text:?}");
    for event in &events {
        eprintln!("{}", serde_json::to_string(event).unwrap());
    }
    result.expect("turn timed out").unwrap();
    assert!(events.contains(&HarnessEvent::MessageCompleted));
    assert!(text.to_lowercase().contains("pong"), "{text:?}");
}

/// One isolated text prompt (the path titles and commit messages use)
/// against a real OpenCode 1.x. Same environment variables as above.
#[test]
#[ignore = "runs the real OpenCode CLI and a model"]
fn live_text_prompt_with_a_real_opencode() {
    let directory = temp_dir("opencode-live-text");
    let binary = std::env::var_os("MONOCODE_OPENCODE_BIN").map(PathBuf::from);
    let (children, spawner) = host_children(&directory, binary.as_deref());
    let adapter = OpenCodeAdapter::new(children, SharedCatalog::new(), spawner, None);
    let result = smol::block_on(timeout(
        Duration::from_secs(180),
        adapter.run_text_prompt(TextPromptInput {
            cwd: directory.to_string_lossy().into_owned(),
            model: std::env::var("MONOCODE_OPENCODE_MODEL").ok(),
            prompt: "Reply with exactly one word: pong.".into(),
            ..Default::default()
        }),
    ));
    let _ = std::fs::remove_dir_all(&directory);
    let text = result.expect("prompt timed out").unwrap();
    eprintln!("reply: {text:?}");
    assert!(text.to_lowercase().contains("pong"), "{text:?}");
}
