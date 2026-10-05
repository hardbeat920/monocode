use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::catalog;
use super::server::{Options, Server};
use crate::core::catalog::SharedCatalog;
use crate::core::child::{Children, HostChildOptions};
use crate::core::registry::{HarnessAdapter, event_sink};
use crate::core::task::{SharedSpawner, SmolSpawner, timeout};
use crate::providers::opencode::dispatch::OpenCodeAdapter;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("monocode-opencode-v2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn children(data: &Path, binary: &Path) -> (Children, SharedSpawner) {
    let spawner: SharedSpawner = Arc::new(SmolSpawner);
    let (children, host) = Children::for_host(
        HostChildOptions {
            data_dir: data.into(),
            ..Default::default()
        },
        spawner.clone(),
    );
    monocode_process::harness::harness_runtime_binary_paths(
        &host,
        HashMap::from([(
            HarnessId::Opencode.as_str().into(),
            binary.to_string_lossy().into_owned(),
        )]),
    );
    (children, spawner)
}

fn turn(cwd: &Path, model: &str) -> SendTurnInput {
    SendTurnInput {
        session: HarnessSessionInput {
            session_id: "native-v2-transport-owned".into(),
            cwd: cwd.to_string_lossy().into_owned(),
            model: model.into(),
            model_settings: None,
            provider_account_id: None,
            runtime_mode: RuntimeMode::Supervised,
            intent: None,
            controls_agents: None,
            app_access: None,
        },
        text: "Reply with exactly one word: pong. Do not use tools.".into(),
        attachments: None,
    }
}

fn response(mut stream: TcpStream, status: &str, content_type: &str, body: &str) {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}

struct Fixture {
    port: u16,
    stop: Arc<AtomicBool>,
    subscribers: Arc<Mutex<Vec<TcpStream>>>,
    requests: Arc<Mutex<Vec<(String, String, Value)>>>,
}

impl Fixture {
    fn start(password_path: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let subscribers: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let requests: Arc<Mutex<Vec<(String, String, Value)>>> = Arc::default();
        let worker_stop = stop.clone();
        let worker_subscribers = subscribers.clone();
        let worker_requests = requests.clone();
        std::thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                let password_path = password_path.clone();
                let subscribers = worker_subscribers.clone();
                let requests = worker_requests.clone();
                std::thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let method = line.split_whitespace().next().unwrap().to_string();
                    let target = line.split_whitespace().nth(1).unwrap().to_string();
                    let path = target.split('?').next().unwrap().to_string();
                    let mut size = 0;
                    let mut authorization = String::new();
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header.trim().is_empty() {
                            break;
                        }
                        if let Some((key, value)) = header.split_once(':') {
                            if key.eq_ignore_ascii_case("Content-Length") {
                                size = value.trim().parse().unwrap();
                            }
                            if key.eq_ignore_ascii_case("Authorization") {
                                authorization = value.trim().into();
                            }
                        }
                    }
                    let mut body = vec![0; size];
                    reader.read_exact(&mut body).unwrap();
                    let password = std::fs::read_to_string(password_path).unwrap();
                    let expected = format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD
                            .encode(format!("opencode:{password}"))
                    );
                    if authorization != expected {
                        response(stream, "401 Unauthorized", "application/json", "{}");
                        return;
                    }
                    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
                    requests
                        .lock()
                        .push((method.clone(), path.clone(), body.clone()));
                    if path == "/api/event" {
                        let mut stream = stream;
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {{\"type\":\"server.connected\",\"data\":{{}}}}\n\n").unwrap();
                        stream.flush().unwrap();
                        subscribers.lock().push(stream);
                    } else if path.starts_with("/api/experimental/session/") {
                        response(stream, "200 OK", "text/event-stream", "");
                    } else if path == "/api/session" && method == "POST" {
                        response(
                            stream,
                            "200 OK",
                            "application/json",
                            r#"{"data":{"id":"ses_transport"}}"#,
                        );
                    } else if path.ends_with("/prompt") {
                        if !matches!(body["delivery"].as_str(), Some("steer" | "queue")) {
                            response(
                                stream,
                                "400 Bad Request",
                                "application/json",
                                r#"{"_tag":"InvalidRequestError","message":"Expected Session.Inbox.Delivery"}"#,
                            );
                            return;
                        }
                        response(
                            stream,
                            "200 OK",
                            "application/json",
                            r#"{"data":{"id":"msg_fixture"}}"#,
                        );
                        std::thread::sleep(Duration::from_millis(20));
                        for subscriber in subscribers.lock().iter_mut() {
                            for event in [
                                json!({"type":"session.text.delta","data":{"sessionID":"ses_transport","assistantMessageID":"msg_response","ordinal":0,"delta":"pong"}}),
                                json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_transport"}}),
                            ] {
                                write!(subscriber, "data: {event}\n\n").unwrap();
                            }
                            subscriber.flush().unwrap();
                        }
                    } else {
                        response(stream, "204 No Content", "application/json", "");
                    }
                });
            }
        });
        Self {
            port,
            stop,
            subscribers,
            requests,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.subscribers.lock().clear();
    }
}

#[test]
fn version_two_runs_through_real_process_authenticated_http_and_sse() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let password_path = scratch.0.join("fixture-password");
    let fixture = Fixture::start(password_path.clone());
    let binary = scratch.0.join("opencode");
    std::fs::write(&binary, format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 2.0.20; exit 0; fi\numask 077\nprintf '%s' \"$OPENCODE_PASSWORD\" > '{}'\nprintf '%s' \"$XDG_DATA_HOME\" > '{}'/fixture-data-root\necho '{{\"url\":\"http://127.0.0.1:{}\"}}'\nexec sleep 600\n",
        password_path.display(), scratch.0.display(), fixture.port,
    )).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (children, spawner) = children(&scratch.0.join("host"), &binary);
    let options = Options::isolated(&scratch.0.join("profile")).unwrap();
    let adapter = OpenCodeAdapter::with_options(
        children,
        SharedCatalog::new(),
        spawner,
        None,
        options.clone(),
    );
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
    let sink = {
        let events = events.clone();
        event_sink(move |event| events.lock().push(event))
    };
    let result = smol::block_on(timeout(
        Duration::from_secs(8),
        adapter.send_turn(turn(&scratch.0, "opencode:fixture/free"), sink, None),
    ));
    smol::block_on(adapter.stop_session("native-v2-transport-owned".into())).unwrap();
    result.expect("transport turn timed out").unwrap();
    assert_eq!(
        std::fs::read_to_string(scratch.0.join("fixture-data-root")).unwrap(),
        options.environment["XDG_DATA_HOME"]
    );
    assert!(
        events
            .lock()
            .iter()
            .any(|event| matches!(event, HarnessEvent::MessageDelta { text } if text == "pong"))
    );
    assert!(events.lock().iter().any(|event| matches!(event, HarnessEvent::SessionProviderBound { provider_session_id } if provider_session_id == "ses_transport")));
    assert!(
        fixture
            .requests
            .lock()
            .iter()
            .any(|(method, path, body)| method == "POST"
                && path == "/api/session"
                && body["permissions"][0]["effect"] == "ask")
    );
}

#[test]
#[ignore = "starts an explicitly supplied OpenCode 2 binary with private data and one zero-cost model"]
fn isolated_live_free_model_turn() {
    let binary = PathBuf::from(
        std::env::var_os("MONOCODE_OPENCODE_V2_BIN")
            .expect("explicit owned OpenCode 2 binary required"),
    );
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        assert!(
            !credential_name(&name),
            "the owned runner must remove provider credential environment variable {name}"
        );
    }
    let scratch = Scratch::new();
    let options = Options::isolated(&scratch.0.join("profile")).unwrap();
    let (children, spawner) = children(&scratch.0.join("host"), &binary);
    let version = smol::block_on(children.exec_child(
        binary.to_str().unwrap(),
        vec!["--version".into()],
        None,
        Some(HarnessId::Opencode),
        Default::default(),
    ))
    .unwrap();
    assert_eq!(
        super::protocol::version(&version).unwrap(),
        super::protocol::MajorVersion::Two
    );
    eprintln!("OpenCode version {}", version.trim());
    let catalog = smol::block_on(async {
        let server = Server::start(children.clone(), scratch.0.to_str().unwrap(), &options).await?;
        let result = async {
            let settled = catalog::read(&server.client).await?;
            let raw = server.client.models().await?;
            Ok::<_, anyhow::Error>((settled, raw))
        }
        .await;
        server.stop().await;
        result
    })
    .unwrap();
    let selected = catalog
        .1
        .as_array()
        .unwrap()
        .iter()
        .find(|model| {
            model["enabled"] == true
                && model["providerID"] == "opencode"
                && model["cost"].as_array().is_some_and(|tiers| {
                    !tiers.is_empty()
                        && tiers.iter().all(|tier| {
                            ["/input", "/output", "/cache/read", "/cache/write"]
                                .iter()
                                .all(|path| tier.pointer(path).and_then(Value::as_f64) == Some(0.0))
                        })
                })
        })
        .expect("no enabled zero-cost OpenCode model in the isolated catalog");
    let model = format!("opencode:opencode/{}", selected["id"].as_str().unwrap());
    eprintln!(
        "Isolated enabled models {}, selected {model}, all advertised prices zero",
        catalog.0.len()
    );
    let shared = SharedCatalog::new();
    shared.set_harness_models(HarnessId::Opencode, catalog.0);
    let adapter = OpenCodeAdapter::with_options(children, shared, spawner, None, options);
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
    let sink = {
        let events = events.clone();
        event_sink(move |event| events.lock().push(event))
    };
    let result = smol::block_on(timeout(
        Duration::from_secs(90),
        adapter.send_turn(turn(&scratch.0, &model), sink, None),
    ));
    smol::block_on(adapter.stop_session("native-v2-transport-owned".into())).unwrap();
    result.expect("isolated free turn timed out").unwrap();
    let events = events.lock();
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            HarnessEvent::MessageDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        text.to_lowercase().contains("pong"),
        "free smoke did not return pong"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, HarnessEvent::MessageCompleted))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, HarnessEvent::SessionProviderBound { .. }))
    );
    eprintln!(
        "Free smoke passed with native session binding, streamed text, completion and owned server cleanup"
    );
}

fn credential_name(name: &str) -> bool {
    matches!(
        name,
        "OPENCODE_API_KEY"
            | "OPENCODE_PASSWORD"
            | "OPENCODE_AUTH"
            | "OPENCODE_AUTH_JSON"
            | "ANTHROPIC_API_KEY"
            | "ANTHROPIC_AUTH_TOKEN"
            | "OPENAI_API_KEY"
            | "OPENAI_ACCESS_TOKEN"
            | "GEMINI_API_KEY"
            | "GOOGLE_API_KEY"
            | "GOOGLE_APPLICATION_CREDENTIALS"
            | "OPENROUTER_API_KEY"
            | "GROQ_API_KEY"
            | "XAI_API_KEY"
            | "GROK_CODE_XAI_API_KEY"
            | "DEEPSEEK_API_KEY"
            | "MISTRAL_API_KEY"
            | "COHERE_API_KEY"
            | "TOGETHER_API_KEY"
            | "FIREWORKS_API_KEY"
            | "CEREBRAS_API_KEY"
            | "MOONSHOT_API_KEY"
            | "MINIMAX_API_KEY"
            | "AI_GATEWAY_API_KEY"
            | "FX_AI_GATEWAY_API_KEY"
            | "VERCEL_OIDC_TOKEN"
            | "AWS_ACCESS_KEY_ID"
            | "AWS_SECRET_ACCESS_KEY"
            | "AWS_SESSION_TOKEN"
            | "AWS_PROFILE"
            | "AZURE_OPENAI_API_KEY"
            | "AZURE_CLIENT_SECRET"
            | "GITHUB_TOKEN"
            | "GH_TOKEN"
    ) || name.ends_with("_API_KEY")
        || name.ends_with("_AUTH_TOKEN")
}
