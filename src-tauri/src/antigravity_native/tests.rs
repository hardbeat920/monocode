use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn oauth_refusal_timeout_cancel_and_port_release() {
    let pkce = auth::Pkce::new();
    assert!(pkce
        .callback("/oauth-callback?state=wrong&code=bad")
        .is_err());
    assert!(pkce
        .callback(&format!("/wrong?state={}&code=bad", pkce.state))
        .is_err());
    let url = url::Url::parse(&pkce.url).unwrap();
    let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
    use base64::Engine;
    use sha2::Digest;
    assert_eq!(
        pairs["code_challenge"],
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sha2::Sha256::digest(pkce.verifier.as_bytes()))
    );
    assert_eq!(pairs["redirect_uri"], transport::REDIRECT);
    for mode in ["refusal", "timeout", "cancel", "success"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = Cancel::default();
        let pkce = auth::Pkce::new();
        let state = pkce.state.clone();
        let flow = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                auth::wait_callback(listener, &pkce, &cancel, Duration::from_millis(200)).await
            }
        });
        if mode == "cancel" {
            cancel.cancel();
        }
        if mode == "refusal" || mode == "success" {
            // A stray callback must leave the legitimate flow alive.
            let mut stray = TcpStream::connect(address).await.unwrap();
            stray.write_all(b"GET /oauth-callback?state=wrong&error=access_denied HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
            let mut response = Vec::new();
            stray.read_to_end(&mut response).await.unwrap();
            assert!(!flow.is_finished());
            let mut socket = TcpStream::connect(address).await.unwrap();
            let query = if mode == "refusal" {
                "error=access_denied"
            } else {
                "code=fake-code"
            };
            socket.write_all(format!("GET /oauth-callback?state={state}&{query} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
        }
        let result = flow.await.unwrap();
        match mode {
            "refusal" => assert_eq!(result.unwrap_err().code, "oauth_declined"),
            "cancel" => assert_eq!(result.unwrap_err().code, "cancelled"),
            "timeout" => assert_eq!(result.unwrap_err().code, "timeout"),
            _ => assert_eq!(result.unwrap(), "fake-code"),
        }
        let rebound = TcpListener::bind(address)
            .await
            .expect("OAuth terminal path frees port");
        drop(rebound);
    }
}

#[test]
fn permission_modes_are_fail_closed() {
    use tools::{permission, Permission::*};
    for mode in ["supervised", "auto", "auto-accept-edits", "full-access"] {
        assert_eq!(permission("read_file", mode, true), Allow);
        assert_eq!(permission("edit_file", mode, true), Deny);
        assert_eq!(permission("powershell", mode, true), Deny);
        assert_eq!(permission("unknown", mode, false), Deny);
    }
    assert_eq!(permission("write_file", "supervised", false), Ask);
    assert_eq!(permission("powershell", "auto", false), Ask);
    assert_eq!(permission("write_file", "auto-accept-edits", false), Allow);
    assert_eq!(permission("powershell", "auto-accept-edits", false), Ask);
    assert_eq!(permission("powershell", "full-access", false), Allow);
}

#[test]
fn user_attachments_outside_workspace_are_embedded_without_granting_tool_access() {
    use base64::Engine;

    let dir = TestDir::new();
    let workspace = dir.0.join("workspace");
    let downloads = dir.0.join("Downloads");
    let temporary = dir.0.join("monocode-attachments");
    for path in [&workspace, &downloads, &temporary] {
        std::fs::create_dir(path).unwrap();
    }
    let files = [
        (
            downloads.join("notes.txt"),
            "text/plain",
            b"outside notes".as_slice(),
        ),
        (
            downloads.join("document.pdf"),
            "application/pdf",
            b"%PDF-1.7".as_slice(),
        ),
        (
            temporary.join("screenshot.png"),
            "image/png",
            b"image bytes".as_slice(),
        ),
        (
            workspace.join("local.txt"),
            "text/plain",
            b"local notes".as_slice(),
        ),
    ];
    let mut request = input(&workspace);
    for (index, (path, mime_type, bytes)) in files.iter().enumerate() {
        std::fs::write(path, bytes).unwrap();
        request.attachments.push(Attachment {
            name: path.file_name().unwrap().to_string_lossy().into(),
            mime_type: (*mime_type).into(),
            data: None,
            path: Some(if index == 3 {
                "local.txt".into()
            } else {
                path.to_string_lossy().into()
            }),
        });
    }
    let parts = user_parts(&workspace, &request, &Cancel::default()).unwrap();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[1]["text"], "Attached file: notes.txt\noutside notes");
    for index in [1, 2] {
        assert_eq!(parts[index + 1]["inlineData"]["mimeType"], files[index].1);
        assert_eq!(
            parts[index + 1]["inlineData"]["data"],
            base64::engine::general_purpose::STANDARD.encode(files[index].2)
        );
    }
    assert_eq!(parts[4]["text"], "Attached file: local.txt\nlocal notes");
    assert!(tools::workspace_path(&workspace, &files[0].0.to_string_lossy(), false).is_err());
}

#[test]
fn user_attachment_paths_keep_size_file_and_cancellation_checks() {
    let dir = TestDir::new();
    let workspace = dir.0.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let large = dir.0.join("large.txt");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(10 * 1024 * 1024 + 1)
        .unwrap();
    let mut request = input(&workspace);
    request.attachments.push(Attachment {
        name: "large.txt".into(),
        mime_type: "text/plain".into(),
        data: None,
        path: Some(large.to_string_lossy().into()),
    });
    assert_eq!(
        user_parts(&workspace, &request, &Cancel::default())
            .unwrap_err()
            .message,
        "Attachment exceeds 10 MiB."
    );
    request.attachments[0].path = Some(dir.0.to_string_lossy().into());
    assert!(user_parts(&workspace, &request, &Cancel::default()).is_err());
    request.attachments[0].path = Some(dir.0.join("missing.txt").to_string_lossy().into());
    assert!(user_parts(&workspace, &request, &Cancel::default()).is_err());
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(
        user_parts(&workspace, &request, &cancel).unwrap_err().code,
        "cancelled"
    );
}

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target")
            .join(format!("antigravity-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
    #[cfg(windows)]
    fn store(&self) -> Store {
        Store(self.0.join("storage"))
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let target = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target")
            .canonicalize()
            .unwrap();
        assert!(self.0.starts_with(target));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
fn account(email: &str) -> auth::Account {
    auth::Account {
        access_token: "fake-access-secret".into(),
        refresh_token: "fake-refresh-secret".into(),
        expires_at: now_ms() + 3600_000,
        email: email.into(),
        project: Some("test-project".into()),
        project_checked_at: now_ms(),
    }
}
fn input(root: &Path) -> TurnInput {
    TurnInput {
        session_id: "thread".into(),
        turn_id: uuid::Uuid::new_v4().to_string(),
        cwd: root.to_string_lossy().into(),
        model: "antigravity:gemini-3.8-flash-high".into(),
        model_settings: HashMap::new(),
        runtime_mode: "supervised".into(),
        intent: None,
        text: "test".into(),
        attachments: vec![],
    }
}
fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: name.into(),
        args,
    }
}
fn frame(parts: Value, finish: bool) -> Value {
    let mut candidate = json!({"content":{"parts":parts}});
    if finish {
        candidate["finishReason"] = json!("STOP");
    }
    json!({"response":{"candidates":[candidate]}})
}

struct Reply {
    status: u16,
    body: String,
    extra: String,
    delay: Duration,
}
impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            body: value.to_string(),
            extra: "Content-Type: application/json\r\n".into(),
            delay: Duration::ZERO,
        }
    }
    fn sse(value: Value) -> Self {
        Self {
            body: format!("data: {value}\n\ndata: [DONE]\n\n"),
            extra: "Content-Type: text/event-stream\r\n".into(),
            ..Self::json(Value::Null)
        }
    }
}
struct Captured {
    header: String,
    body: String,
}
async fn mock_http(
    replies: Vec<Reply>,
) -> (
    Transport,
    tokio::sync::mpsc::UnboundedReceiver<Captured>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut byte = [0u8; 1];
            while !bytes.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                bytes.push(byte[0]);
            }
            let header = String::from_utf8(bytes).unwrap();
            let lower = header.to_lowercase();
            let mut body = Vec::new();
            if lower.contains("transfer-encoding: chunked") {
                loop {
                    let mut size = Vec::new();
                    while !size.ends_with(b"\r\n") {
                        socket.read_exact(&mut byte).await.unwrap();
                        size.push(byte[0]);
                    }
                    let n =
                        usize::from_str_radix(String::from_utf8(size).unwrap().trim(), 16).unwrap();
                    let mut chunk = vec![0; n + 2];
                    socket.read_exact(&mut chunk).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..n]);
                }
            } else if let Some(line) = lower.lines().find(|l| l.starts_with("content-length:")) {
                body.resize(line.split(':').nth(1).unwrap().trim().parse().unwrap(), 0);
                socket.read_exact(&mut body).await.unwrap();
            }
            let _ = tx.send(Captured {
                header,
                body: String::from_utf8(body).unwrap(),
            });
            tokio::time::sleep(reply.delay).await;
            let response = format!(
                "HTTP/1.1 {} Test\r\nConnection: close\r\nContent-Length: {}\r\n{}\r\n{}",
                reply.status,
                reply.body.len(),
                reply.extra,
                reply.body
            );
            // Deliberately split UTF-8 and JSON across writes.
            for fragment in response.as_bytes().chunks(7) {
                if socket.write_all(fragment).await.is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        }
    });
    let transport = Transport {
        endpoints: vec![base.clone()],
        token_url: format!("{base}/token"),
        userinfo_url: format!("{base}/userinfo"),
        ..Transport::default()
    };
    (transport, rx, handle)
}

#[tokio::test]
async fn explicit_oauth_exchanges_pkce_without_provider_process() {
    let occupied = TcpListener::bind("127.0.0.1:51121").await.unwrap();
    assert_eq!(
        auth::login(&Transport::default(), &Cancel::default(), |_| panic!(
            "Occupied port must not open the browser"
        ))
        .await
        .err()
        .unwrap()
        .code,
        "oauth"
    );
    drop(occupied);
    let (transport, mut requests, server) = mock_http(vec![Reply::json(json!({"access_token":"fake-access-secret","refresh_token":"fake-refresh-secret","expires_in":3600})), Reply::json(json!({"email":"one@example.test"}))]).await;
    let connected = auth::login(&transport, &Cancel::default(), |url| {
        let query: HashMap<_, _> = url::Url::parse(url).unwrap().query_pairs().into_owned().collect();
        let state = query["state"].clone();
        tokio::spawn(async move {
            let mut socket = TcpStream::connect("127.0.0.1:51121").await.unwrap();
            socket.write_all(format!("GET /oauth-callback?state={state}&code=fake-code HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
        });
        Ok(())
    }).await.unwrap();
    assert_eq!(connected.email, "one@example.test");
    let token = requests.recv().await.unwrap();
    let fields: HashMap<_, _> = url::form_urlencoded::parse(token.body.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(fields["code"], "fake-code");
    assert_eq!(fields["grant_type"], "authorization_code");
    assert_eq!(fields["redirect_uri"], transport::REDIRECT);
    assert!(fields["code_verifier"].len() >= 43);
    assert!(requests
        .recv()
        .await
        .unwrap()
        .header
        .contains("Bearer fake-access-secret"));
    server.await.unwrap();
    drop(TcpListener::bind("127.0.0.1:51121").await.unwrap());
}

#[tokio::test]
async fn project_onboarding_and_quota_are_structured() {
    let (transport, mut requests, server) = mock_http(vec![
        Reply::json(json!({"allowedTiers":[{"id":"paid"},{"id":"free-tier","isDefault":true}]})),
        Reply::json(
            json!({"done":true,"response":{"cloudaicompanionProject":{"id":"project-object"}}}),
        ),
    ])
    .await;
    assert_eq!(
        auth::discover_project(&transport, "secret", &Cancel::default())
            .await
            .unwrap(),
        "project-object"
    );
    let load = requests.recv().await.unwrap();
    assert!(load.header.starts_with("POST /v1internal:loadCodeAssist "));
    assert_eq!(
        serde_json::from_str::<Value>(&load.body).unwrap(),
        json!({"metadata":{"ideType":"ANTIGRAVITY"}})
    );
    let onboard = requests.recv().await.unwrap();
    assert!(onboard.header.starts_with("POST /v1internal:onboardUser "));
    assert_eq!(
        serde_json::from_str::<Value>(&onboard.body).unwrap(),
        json!({"tierId":"free-tier"})
    );
    server.await.unwrap();
    for (status, code) in [
        (401, "auth"),
        (403, "ineligible"),
        (429, "quota"),
        (503, "network"),
    ] {
        let reply = Reply {
            status,
            extra: "Retry-After: 37\r\n".into(),
            ..Reply::json(json!({"error":"fake-access-secret"}))
        };
        let (transport, _, server) = mock_http(vec![reply]).await;
        let error = transport
            .stream("secret", &json!({}), &Cancel::default(), |_| Ok(()))
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!serde_json::to_string(&error)
            .unwrap()
            .contains("fake-access-secret"));
        if status == 429 {
            assert_eq!(error.retry_after, Some(37));
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn signed_fragmented_tool_roundtrip_and_resume_both_families() {
    let dir = TestDir::new();
    for id in ["gemini-3.8-flash-high", "claude-sonnet-4-6-thinking"] {
        let model = protocol::resolve_model(id, None).unwrap();
        let mut decoder = transport::SseDecoder::default();
        let values = [
            frame(json!([{"thought":true,"text":"I "}]), false),
            frame(json!([{"thought":true,"text":"think"}]), false),
            frame(
                json!([{"thought":true,"text":"","thoughtSignature":"before-call"}]),
                false,
            ),
            frame(
                json!([{"functionCall":{"id":"a","name":"read_file","args":{"path":"a"}}},{"functionCall":{"id":"b","name":"read_file","args":{"path":"b"}}}]),
                true,
            ),
        ];
        let wire = values
            .iter()
            .map(|v| format!("data: {v}\r\n\r\n"))
            .collect::<String>();
        let mut generation = Generation::default();
        let mut events = vec![];
        for byte in wire.as_bytes().chunks(1) {
            for value in decoder.push(byte).unwrap() {
                generation
                    .chunk(value, &mut |event| events.push(event))
                    .unwrap();
            }
        }
        generation.complete().unwrap();
        assert_eq!(generation.parts.len(), 3);
        assert_eq!(generation.parts[0]["text"], "I think");
        assert!(generation.parts[0].get("thoughtSignature").is_none());
        assert_eq!(generation.parts[1]["thoughtSignature"], "before-call");
        assert!(generation.parts[2].get("thoughtSignature").is_none());
        let mut history = History::new(&dir.0, "one@example.test".into());
        history.contents.push(HistoryItem {
            role: "model".into(),
            parts: generation.parts,
            model: Some(model.wire.clone()),
        });
        for id in ["a", "b"] {
            history.tool_results.insert(id.into(), json!({"output":id}));
        }
        recover_tools(&mut history);
        recover_tools(&mut history);
        let same = history.contents_for(&model);
        assert_eq!(same.len(), 2);
        assert_eq!(same[1]["role"], "model");
        assert_eq!(same[1]["parts"].as_array().unwrap().len(), 2);
        assert_eq!(same[0]["parts"][1]["thoughtSignature"], "before-call");
        #[cfg(windows)]
        let restored: History = {
            dir.store().write(&history.filename(), &history).unwrap();
            dir.store().read(&history.filename()).unwrap().unwrap()
        };
        #[cfg(not(windows))]
        let restored: History =
            serde_json::from_value(serde_json::to_value(&history).unwrap()).unwrap();
        assert_eq!(restored.contents_for(&model), same);
        let other = protocol::resolve_model(
            if model.claude {
                "gemini-3.8-flash-high"
            } else {
                "claude-sonnet-4-6-thinking"
            },
            None,
        )
        .unwrap();
        let cross = restored.contents_for(&other);
        assert_eq!(cross[1]["role"], "user");
        assert!(!cross[0]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["thought"] == true));
        assert_ne!(cross[0]["parts"][0]["thoughtSignature"], "before-call");
        for before in [true, false] {
            let mut text = Generation::default();
            if before {
                text.chunk(
                    frame(json!([{"thoughtSignature":"text-signature"}]), false),
                    &mut |_| {},
                )
                .unwrap();
            }
            text.chunk(
                frame(json!([{"text":"hé"},{"text":"llo"}]), false),
                &mut |_| {},
            )
            .unwrap();
            text.chunk(
                frame(
                    if before {
                        json!([])
                    } else {
                        json!([{"thoughtSignature":"text-signature"}])
                    },
                    true,
                ),
                &mut |_| {},
            )
            .unwrap();
            assert_eq!(
                text.parts,
                vec![json!({"text":"héllo","thoughtSignature":"text-signature"})]
            );
        }
        let mut trailing = Generation::default();
        trailing.chunk(frame(json!([{"text":"answer"},{"thoughtSignature":"after-text"},{"functionCall":{"id":"c","name":"read_file","args":{"path":"c"}}}]),true),&mut |_| {}).unwrap();
        assert_eq!(trailing.parts[0]["thoughtSignature"], "after-text");
        assert!(trailing.parts[1].get("thoughtSignature").is_none());
        let mut after_call = Generation::default();
        after_call
            .chunk(
                frame(
                    json!([{"functionCall":{"id":"d","name":"read_file","args":{"path":"d"}}}]),
                    false,
                ),
                &mut |_| {},
            )
            .unwrap();
        after_call
            .chunk(
                frame(json!([{"thoughtSignature":"after-call"}]), true),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(after_call.parts[0]["thoughtSignature"], "after-call");
    }
}

#[test]
fn registry_variants_and_family_request_contract() {
    let dir = TestDir::new();
    for entry in protocol::catalog() {
        let id = entry["id"].as_str().unwrap();
        let model = protocol::resolve_model(id, None).unwrap();
        let mut history = History::new(&dir.0, "one@example.test".into());
        history.contents.push(HistoryItem {
            role: "user".into(),
            parts: vec![json!({"text":"hello"})],
            model: None,
        });
        let request = history.request(&model, "test-project", tools::definitions(false), false);
        assert_eq!(request["requestType"], "agent");
        assert_eq!(request["model"], model.wire);
        assert!(request["requestId"].as_str().unwrap().starts_with("agent/"));
        assert!(request["request"]["sessionId"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .is_ok());
        if model.claude {
            assert_eq!(
                request["request"]["generationConfig"]["thinkingConfig"]["thinking_budget"],
                1024
            );
            assert!(entry.get("settings").is_none());
            assert!(protocol::resolve_model(id, Some("low")).is_err());
            assert!(protocol::resolve_model(id, Some("high")).is_err());
        } else if !model.image && id.contains("gemini") {
            for option in entry["settings"][0]["options"].as_array().unwrap() {
                assert!(protocol::resolve_model(id, option["value"].as_str()).is_ok());
            }
            assert!(protocol::resolve_model(id, Some("medium")).is_err());
        }
        if model.image {
            assert!(request["request"].get("tools").is_none());
        }
    }
    assert_eq!(
        protocol::resolve_model("antigravity:gemini-3.8-flash-medium", None)
            .unwrap()
            .wire,
        "gemini-3.8-flash-medium"
    );
}

#[tokio::test]
async fn http_stream_is_chunked_fragmented_and_cancellable() {
    let (transport, mut requests, server) =
        mock_http(vec![Reply::sse(frame(json!([{"text":"héllo"}]), true))]).await;
    let mut generation = Generation::default();
    transport
        .stream(
            "fake-access-secret",
            &json!({"project":"test"}),
            &Cancel::default(),
            |v| generation.chunk(v, &mut |_| {}),
        )
        .await
        .unwrap();
    generation.complete().unwrap();
    assert_eq!(generation.parts[0]["text"], "héllo");
    let captured = requests.recv().await.unwrap();
    assert!(captured
        .header
        .starts_with("POST /v1internal:streamGenerateContent?alt=sse HTTP/1.1"));
    assert!(captured
        .header
        .to_lowercase()
        .contains("transfer-encoding: chunked"));
    assert!(captured.header.contains(&transport::user_agent()));
    assert_eq!(
        serde_json::from_str::<Value>(&captured.body).unwrap(),
        json!({"project":"test"})
    );
    server.await.unwrap();
    let (transport, mut requests, server) = mock_http(vec![Reply {
        delay: Duration::from_secs(60),
        ..Reply::json(Value::Null)
    }])
    .await;
    let cancel = Cancel::default();
    let future = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            transport
                .stream("secret", &json!({}), &cancel, |_| Ok(()))
                .await
        }
    });
    requests.recv().await.unwrap();
    cancel.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), future)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        "cancelled"
    );
    server.abort();
}

#[cfg(windows)]
#[tokio::test]
async fn refresh_is_shared_and_storage_is_encrypted() {
    let dir = TestDir::new();
    let auth = Arc::new(Auth::default());
    let mut old = account("one@example.test");
    old.expires_at = 0;
    auth.save(&dir.store(), old, &Cancel::default())
        .await
        .unwrap();
    let (transport, mut requests, server) = mock_http(vec![Reply::json(json!({"access_token":"refreshed-secret","refresh_token":"rotated-secret","expires_in":3600}))]).await;
    let cancel = Cancel::default();
    let store = dir.store();
    let (one, two) = tokio::join!(
        auth.account(&store, &transport, &cancel),
        auth.account(&store, &transport, &cancel)
    );
    assert_eq!(one.unwrap().access_token, "refreshed-secret");
    assert_eq!(two.unwrap().refresh_token, "rotated-secret");
    let fields = requests.recv().await.unwrap().body;
    assert!(fields.contains("grant_type=refresh_token"));
    server.await.unwrap();
    assert!(requests.try_recv().is_err());
    let encrypted = std::fs::read(store.0.join("account.dpapi")).unwrap();
    assert!(!String::from_utf8_lossy(&encrypted).contains("secret"));
    auth.logout(&store).await.unwrap();
    assert!(auth.summary(&store).await.unwrap().is_none());
    assert!(!store.0.join("account.dpapi").exists());
}

#[cfg(windows)]
#[tokio::test]
async fn account_change_rebind_and_corrupt_credentials_keep_backend_available() {
    let dir = TestDir::new();
    let store = dir.store();
    let host = NativeHost::default();
    let one = account("one@example.test");
    let two = account("two@example.test");
    let cancel = Cancel::default();
    host.auth.save(&store, one.clone(), &cancel).await.unwrap();
    let session = host.session("thread", "window", &dir.0).unwrap();
    let first = session
        .load_history(&host, "thread", &one.email, &store, &cancel)
        .await
        .unwrap()
        .as_ref()
        .unwrap()
        .id
        .clone();
    host.auth.logout(&store).await.unwrap();
    host.auth.save(&store, two.clone(), &cancel).await.unwrap();
    assert_eq!(
        session
            .load_history(&host, "thread", &two.email, &store, &cancel)
            .await
            .err()
            .unwrap()
            .code,
        "session"
    );
    let replacement = History::new(&dir.0, two.email.clone());
    store.write(&replacement.filename(), &replacement).unwrap();
    host.bind_owned("window", "thread", &replacement.id, dir.0.clone())
        .unwrap();
    assert!(session.retired.load(Ordering::SeqCst));
    let rebound = host.session("thread", "window", &dir.0).unwrap();
    let loaded = rebound
        .load_history(&host, "thread", &two.email, &store, &cancel)
        .await
        .unwrap();
    assert_eq!(loaded.as_ref().unwrap().id, replacement.id);
    assert_ne!(replacement.id, first);
    drop(loaded);
    assert!(host
        .bind_owned("other-window", "thread", &replacement.id, dir.0.clone())
        .is_err());
    host.stop_owned("window", "thread", true, &store)
        .await
        .unwrap();
    assert!(!store.0.join(replacement.filename()).exists());
    std::fs::write(store.0.join("account.dpapi"), b"corrupt-dpapi").unwrap();
    let status = account_status(Auth::default().summary(&store).await, false);
    assert!(status.backend_available);
    assert!(!status.authenticated);
    assert!(status.auth_error.is_some());
}

#[cfg(windows)]
fn saved_conversation(dir: &TestDir) -> History {
    let mut history = History::new(&dir.0, "one@example.test".into());
    history.contents = vec![
        HistoryItem {
            role: "user".into(),
            parts: vec![json!({"text":"previous question"})],
            model: None,
        },
        HistoryItem {
            role: "model".into(),
            parts: vec![json!({"text":"previous answer","thoughtSignature":"saved-signature"})],
            model: Some("gemini-3.8-flash-high".into()),
        },
    ];
    dir.store().write(&history.filename(), &history).unwrap();
    history
}

#[cfg(windows)]
#[tokio::test]
async fn stopped_session_releases_owner_and_resumes_signed_history_in_another_window() {
    let dir = TestDir::new();
    let store = dir.store();
    let history = saved_conversation(&dir);
    let encrypted = std::fs::read(store.0.join(history.filename())).unwrap();
    let (transport, mut requests, server) =
        mock_http(vec![Reply::sse(frame(json!([{"text":"resumed"}]), true))]).await;
    let host = NativeHost {
        transport,
        ..NativeHost::default()
    };
    let account = account("one@example.test");
    let cancel = Cancel::default();
    host.auth
        .save(&store, account.clone(), &cancel)
        .await
        .unwrap();
    host.bind_owned("window-a", "thread", &history.id, dir.0.clone())
        .unwrap();
    let old = host.session("thread", "window-a", &dir.0).unwrap();
    drop(
        old.load_history(&host, "thread", &account.email, &store, &cancel)
            .await
            .unwrap(),
    );
    host.stop_owned("window-a", "thread", false, &store)
        .await
        .unwrap();
    assert!(old.retired.load(Ordering::SeqCst));
    assert!(host.owned_session("thread", "window-a").unwrap().is_none());
    assert_eq!(host.bindings.lock().unwrap()["thread"].0, None);
    assert_eq!(
        std::fs::read(store.0.join(history.filename())).unwrap(),
        encrypted
    );
    // Idle parking must also keep automatic resume in the original window.
    let parked = host.session("thread", "window-a", &dir.0).unwrap();
    assert_eq!(
        parked
            .load_history(&host, "thread", &account.email, &store, &cancel)
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .id,
        history.id
    );
    host.stop_owned("window-a", "thread", false, &store)
        .await
        .unwrap();
    host.bind_owned("window-b", "thread", &history.id, dir.0.clone())
        .unwrap();
    let resumed = host.session("thread", "window-b", &dir.0).unwrap();
    assert!(!Arc::ptr_eq(&old, &resumed));
    let mut loaded = resumed
        .load_history(&host, "thread", &account.email, &store, &cancel)
        .await
        .unwrap();
    let restored = loaded.as_mut().unwrap();
    assert_eq!(restored.id, history.id);
    let input = input(&dir.0);
    let model = protocol::resolve_model(&input.model, None).unwrap();
    assert_eq!(restored.contents_for(&model), history.contents_for(&model));
    restored.contents.push(HistoryItem {
        role: "user".into(),
        parts: vec![json!({"text":"continue"})],
        model: None,
    });
    agent_loop(
        &host,
        &resumed,
        restored,
        &input,
        &model,
        &account,
        &store,
        &cancel,
        &mut |_| {},
    )
    .await
    .unwrap();
    let request: Value = serde_json::from_str(&requests.recv().await.unwrap().body).unwrap();
    assert_eq!(
        request["request"]["contents"][1]["parts"][0]["thoughtSignature"],
        "saved-signature"
    );
    server.await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn stopping_active_turn_prevents_transfer_until_the_turn_gate_is_released() {
    let dir = TestDir::new();
    let store = dir.store();
    let history = saved_conversation(&dir);
    let host = Arc::new(NativeHost::default());
    host.bind_owned("window-a", "thread", &history.id, dir.0.clone())
        .unwrap();
    let old = host.session("thread", "window-a", &dir.0).unwrap();
    let turn = old.turn_gate.lock().await;
    let cancel = Cancel::default();
    *old.active.lock().unwrap() = Some(Active {
        turn_id: "active-turn".into(),
        cancel: cancel.clone(),
    });
    assert!(host
        .bind_owned("window-b", "thread", &history.id, dir.0.clone())
        .is_err());
    assert!(host.session("thread", "window-b", &dir.0).is_err());
    assert!(host
        .stop_owned("window-b", "thread", false, &store)
        .await
        .is_err());
    let stop = tokio::spawn({
        let host = host.clone();
        let store = store.clone();
        async move { host.stop_owned("window-a", "thread", false, &store).await }
    });
    tokio::time::timeout(Duration::from_secs(1), cancel.cancelled())
        .await
        .unwrap();
    assert!(!stop.is_finished());
    *old.active.lock().unwrap() = None;
    host.cancel_window("window-a"); // Closure must not bypass explicit stop's cleanup.
    assert!(!old.closed.load(Ordering::SeqCst));
    assert!(host
        .bind_owned("window-b", "thread", &history.id, dir.0.clone())
        .is_err());
    assert!(host
        .bind_owned("window-a", "thread", &history.id, dir.0.clone())
        .is_err());
    drop(turn);
    tokio::time::timeout(Duration::from_secs(1), stop)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    host.bind_owned("window-b", "thread", &history.id, dir.0.clone())
        .unwrap();
    let new = host.session("thread", "window-b", &dir.0).unwrap();
    assert_eq!(
        new.load_history(&host, "thread", &history.email, &store, &Cancel::default())
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .id,
        history.id
    );
    assert!(host
        .stop_owned("window-a", "thread", true, &store)
        .await
        .is_err());
    assert!(store.0.join(history.filename()).exists());
}

#[cfg(windows)]
#[tokio::test]
async fn closed_window_cancels_then_releases_ownership_without_deleting_history() {
    let dir = TestDir::new();
    let store = dir.store();
    let history = saved_conversation(&dir);
    let encrypted = std::fs::read(store.0.join(history.filename())).unwrap();
    let host = NativeHost::default();
    // Cover an idle session, a binding whose session has not yet loaded, and
    // an active turn whose writes may still be unwinding after cancellation.
    for state in ["idle", "bound", "active", "lazy"] {
        let thread = format!("closed-{state}");
        host.bind_owned("window-a", &thread, &history.id, dir.0.clone())
            .unwrap();
        let session = if state != "bound" {
            Some(host.session(&thread, "window-a", &dir.0).unwrap())
        } else {
            None
        };
        let gate = if state == "active" || state == "lazy" {
            Some(session.as_ref().unwrap().turn_gate.lock().await)
        } else {
            None
        };
        let cancel = Cancel::default();
        if gate.is_some() {
            *session.as_ref().unwrap().active.lock().unwrap() = Some(Active {
                turn_id: state.into(),
                cancel: cancel.clone(),
            });
            assert!(host
                .bind_owned("window-b", &thread, &history.id, dir.0.clone())
                .is_err());
        }
        host.cancel_window("window-a");
        if gate.is_some() {
            assert!(cancel.is_cancelled());
            assert!(host
                .bind_owned("window-b", &thread, &history.id, dir.0.clone())
                .is_err());
            host.release_closed(&thread, session.as_ref().unwrap());
            assert!(host.owned_session(&thread, "window-a").unwrap().is_some());
            *session.as_ref().unwrap().active.lock().unwrap() = None;
            assert!(host
                .bind_owned("window-b", &thread, &history.id, dir.0.clone())
                .is_err());
        }
        drop(gate);
        if state != "lazy" {
            if let Some(session) = &session {
                host.release_closed(&thread, session);
            }
            assert!(host.owned_session(&thread, "window-a").unwrap().is_none());
            assert_eq!(host.bindings.lock().unwrap()[&thread].0, None);
        }
        assert_eq!(
            std::fs::read(store.0.join(history.filename())).unwrap(),
            encrypted
        );
        host.bind_owned("window-b", &thread, &history.id, dir.0.clone())
            .unwrap();
        let resumed = host.session(&thread, "window-b", &dir.0).unwrap();
        if let Some(old) = &session {
            host.release_closed(&thread, old);
        }
        assert_eq!(
            host.bindings.lock().unwrap()[&thread].0.as_deref(),
            Some("window-b")
        );
        assert_eq!(
            resumed
                .load_history(&host, &thread, &history.email, &store, &Cancel::default())
                .await
                .unwrap()
                .as_ref()
                .unwrap()
                .contents[1]
                .parts[0]["thoughtSignature"],
            "saved-signature"
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn closing_before_active_releases_owner_on_early_turn_exits() {
    let dir = TestDir::new();
    let store = dir.store();
    let history = saved_conversation(&dir);
    let encrypted = std::fs::read(store.0.join(history.filename())).unwrap();
    for reason in ["retired", "cancelled-turn"] {
        let host = NativeHost::default();
        let input = input(&dir.0);
        let cancel = Cancel::default();
        host.bind_owned("window-a", &input.session_id, &history.id, dir.0.clone())
            .unwrap();
        let session = host.session(&input.session_id, "window-a", &dir.0).unwrap();
        if reason == "cancelled-turn" {
            host.cancelled_turns.lock().unwrap().push((
                "window-a".into(),
                input.session_id.clone(),
                input.turn_id.clone(),
            ));
        }
        let result: Result<()> = async {
            let _turn = TurnGuard::lock(&host, &input.session_id, &session).await;
            assert!(session.active.lock().unwrap().is_none());
            if reason == "retired" {
                host.cancel_window("window-a");
            }
            let started = session.begin_turn(&host, &input, &cancel);
            if reason == "cancelled-turn" {
                assert_eq!(started.as_ref().unwrap(), &false);
                // Closure races the cancelled-turn return while the gate is held.
                host.cancel_window("window-a");
            }
            assert!(session.active.lock().unwrap().is_none());
            assert!(host
                .owned_session(&input.session_id, "window-a")
                .unwrap()
                .is_some());
            assert!(host
                .bind_owned("window-b", &input.session_id, &history.id, dir.0.clone())
                .is_err());
            if !started? {
                return Ok(());
            }
            panic!("A closed or cancelled turn must not register active");
        }
        .await;
        if reason == "retired" {
            assert_eq!(result.unwrap_err().code, "cancelled");
        } else {
            result.unwrap();
        }
        assert!(session.turn_gate.try_lock().is_ok());
        assert!(host
            .owned_session(&input.session_id, "window-a")
            .unwrap()
            .is_none());
        assert_eq!(host.bindings.lock().unwrap()[&input.session_id].0, None);
        assert_eq!(
            std::fs::read(store.0.join(history.filename())).unwrap(),
            encrypted
        );
        host.bind_owned("window-b", &input.session_id, &history.id, dir.0.clone())
            .unwrap();
        let resumed = host.session(&input.session_id, "window-b", &dir.0).unwrap();
        assert_eq!(
            resumed
                .load_history(
                    &host,
                    &input.session_id,
                    &history.email,
                    &store,
                    &Cancel::default()
                )
                .await
                .unwrap()
                .as_ref()
                .unwrap()
                .id,
            history.id
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn forget_cancels_auth_refresh_and_waits_for_turn_before_deletion() {
    let dir = TestDir::new();
    let store = dir.store();
    let (transport, mut requests, server) = mock_http(vec![Reply {
        delay: Duration::from_secs(60),
        ..Reply::json(Value::Null)
    }])
    .await;
    let host = Arc::new(NativeHost {
        transport,
        ..NativeHost::default()
    });
    let mut old = account("one@example.test");
    old.expires_at = 0;
    host.auth
        .save(&store, old, &Cancel::default())
        .await
        .unwrap();
    let history = History::new(&dir.0, "one@example.test".into());
    store.write(&history.filename(), &history).unwrap();
    host.bind_owned("window", "thread", &history.id, dir.0.clone())
        .unwrap();
    let session = host.session("thread", "window", &dir.0).unwrap();
    let cancel = Cancel::default();
    let turn = tokio::spawn({
        let host = host.clone();
        let session = session.clone();
        let store = store.clone();
        let cancel = cancel.clone();
        async move {
            let _gate = session.turn_gate.lock().await;
            *session.active.lock().unwrap() = Some(Active {
                turn_id: "turn".into(),
                cancel: cancel.clone(),
            });
            let result = host.auth.account(&store, &host.transport, &cancel).await;
            if let Ok(account) = result {
                let _history = session
                    .load_history(&host, "thread", &account.email, &store, &cancel)
                    .await?;
            }
            *session.active.lock().unwrap() = None;
            if cancel.is_cancelled() {
                Err(NativeError::cancelled())
            } else {
                Ok(())
            }
        }
    });
    requests.recv().await.unwrap();
    // A login that already completed network work is waiting behind refresh.
    let login_cancel = Cancel::default();
    let save = tokio::spawn({
        let host = host.clone();
        let store = store.clone();
        let cancel = login_cancel.clone();
        async move {
            host.auth
                .save(&store, account("late@example.test"), &cancel)
                .await
        }
    });
    login_cancel.cancel();
    tokio::time::timeout(
        Duration::from_secs(1),
        host.stop_owned("window", "thread", true, &store),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(turn.await.unwrap().unwrap_err().code, "cancelled");
    host.auth.logout(&store).await.unwrap();
    assert_eq!(save.await.unwrap().unwrap_err().code, "cancelled");
    assert!(!store.0.join(history.filename()).exists());
    assert!(!store.0.join("account.dpapi").exists());
    assert!(session
        .load_history(&host, "thread", "one@example.test", &store, &cancel)
        .await
        .is_err());
    server.abort();
}

#[tokio::test]
async fn approval_previews_and_native_decision_are_readable() {
    let dir = TestDir::new();
    let host = NativeHost::default();
    let session = host.session("thread", "window", &dir.0).unwrap();
    let input = input(&dir.0);
    let cancel = Cancel::default();
    let edit = call(
        "edit_file",
        json!({"path":"file.txt","oldText":"old\nkeep","newText":"new\nkeep"}),
    );
    let preview = tool_preview(&edit, "Edit file.txt");
    assert_eq!(preview["kind"], "write");
    assert_eq!(preview["lines"][0]["kind"], "del");
    assert_eq!(preview["lines"][0]["text"], "old");
    assert_eq!(preview["lines"][2]["kind"], "add");
    assert_eq!(preview["lines"][2]["text"], "new");
    let shell = call("powershell", json!({"command":"Get-ChildItem"}));
    assert_eq!(tool_preview(&shell, "Run")["output"], "Get-ChildItem");
    for decision in [true, false] {
        let mut events = vec![];
        let allowed = authorize_tool(&host, &session, &input, &edit, &cancel, &mut |event| {
            if event["type"] == "approval.requested" {
                assert_eq!(event["preview"], preview_with_title(&edit));
                let (_, sender) = session
                    .approvals
                    .lock()
                    .unwrap()
                    .remove(&event["requestId"].as_u64().unwrap())
                    .unwrap();
                sender.send(decision).unwrap();
            }
            events.push(event);
        })
        .await
        .unwrap();
        assert_eq!(allowed, decision);
        assert_eq!(events.last().unwrap()["type"], "approval.resolved");
        assert!(session.approvals.lock().unwrap().is_empty());
    }
    let cancel = Cancel::default();
    assert_eq!(
        authorize_tool(&host, &session, &input, &edit, &cancel, &mut |event| {
            if event["type"] == "approval.requested" {
                cancel.cancel();
            }
        })
        .await
        .unwrap_err()
        .code,
        "cancelled"
    );
    assert!(session.approvals.lock().unwrap().is_empty());
}

#[cfg(windows)]
#[tokio::test]
async fn forget_while_waiting_for_shared_auth_mutex_never_loads_history() {
    let dir = TestDir::new();
    let store = dir.store();
    let (transport, mut requests, server) = mock_http(vec![Reply {
        delay: Duration::from_secs(60),
        ..Reply::json(Value::Null)
    }])
    .await;
    let host = Arc::new(NativeHost {
        transport,
        ..NativeHost::default()
    });
    let mut old = account("one@example.test");
    old.expires_at = 0;
    host.auth
        .save(&store, old, &Cancel::default())
        .await
        .unwrap();
    let refresh_cancel = Cancel::default();
    let refresh = tokio::spawn({
        let host = host.clone();
        let store = store.clone();
        let cancel = refresh_cancel.clone();
        async move { host.auth.account(&store, &host.transport, &cancel).await }
    });
    requests.recv().await.unwrap();
    let history = History::new(&dir.0, "one@example.test".into());
    store.write(&history.filename(), &history).unwrap();
    host.bind_owned("window", "thread", &history.id, dir.0.clone())
        .unwrap();
    let session = host.session("thread", "window", &dir.0).unwrap();
    let cancel = Cancel::default();
    let (started, waiting) = oneshot::channel();
    let turn = tokio::spawn({
        let host = host.clone();
        let session = session.clone();
        let store = store.clone();
        let cancel = cancel.clone();
        async move {
            let _gate = session.turn_gate.lock().await;
            *session.active.lock().unwrap() = Some(Active {
                turn_id: "waiting-turn".into(),
                cancel: cancel.clone(),
            });
            started.send(()).unwrap();
            let account = host.auth.account(&store, &host.transport, &cancel).await?;
            let _history = session
                .load_history(&host, "thread", &account.email, &store, &cancel)
                .await?;
            Ok::<_, NativeError>(())
        }
    });
    waiting.await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        host.stop_owned("window", "thread", true, &store),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(turn.await.unwrap().unwrap_err().code, "cancelled");
    assert!(session.history.lock().await.is_none());
    assert!(!store.0.join(history.filename()).exists());
    refresh_cancel.cancel();
    assert_eq!(refresh.await.unwrap().err().unwrap().code, "cancelled");
    server.abort();
}
fn preview_with_title(edit: &ToolCall) -> Value {
    tool_preview(edit, "edit_file file.txt")
}

#[test]
fn canonical_workspace_search_and_edits_reject_escape() {
    let dir = TestDir::new();
    let cancel = Cancel::default();
    tools::file_tool(
        &dir.0,
        &call("write_file", json!({"path":"a.txt","content":"needle old"})),
        &cancel,
    )
    .unwrap();
    // Pass a noncanonical root to exercise Windows extended path normalization.
    let root = dir.0.join(".");
    let result = tools::file_tool(
        &root,
        &call("search_files", json!({"path":".","query":"needle"})),
        &cancel,
    )
    .unwrap();
    assert!(result["output"].as_str().unwrap().contains("a.txt:1:"));
    tools::file_tool(
        &root,
        &call(
            "edit_file",
            json!({"path":"a.txt","oldText":"old","newText":"new"}),
        ),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.0.join("a.txt")).unwrap(),
        "needle new"
    );
    for path in ["../escape.txt", "a.txt:secret", "missing/child.txt"] {
        assert!(tools::file_tool(
            &root,
            &call("write_file", json!({"path":path,"content":"bad"})),
            &cancel
        )
        .is_err());
    }
    cancel.cancel();
    assert_eq!(
        tools::file_tool(&root, &call("read_file", json!({"path":"a.txt"})), &cancel)
            .unwrap_err()
            .code,
        "cancelled"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn powershell_descendant_inheriting_pipes_and_cancellation_are_bounded() {
    let dir = TestDir::new();
    let command="$p = New-Object System.Diagnostics.ProcessStartInfo; $p.FileName = $env:ComSpec; $p.Arguments = '/c ping -n 60 127.0.0.1'; $p.UseShellExecute = $false; $p.CreateNoWindow = $true; $child = [System.Diagnostics.Process]::Start($p); Write-Output ('descendant:' + $child.Id); exit 0";
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        tools::execute(
            dir.0.clone(),
            call("powershell", json!({"command":command})),
            Cancel::default(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result["output"].as_str().unwrap().contains("descendant:"));
    assert!(start.elapsed() < Duration::from_secs(8));
    let pid: u32 = result["output"]
        .as_str()
        .unwrap()
        .split("descendant:")
        .nth(1)
        .unwrap()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap();
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
        let process = OpenProcess(0x0010_0000, 0, pid); // SYNCHRONIZE
        if !process.is_null() {
            let status = WaitForSingleObject(process, 1000);
            CloseHandle(process);
            assert_eq!(
                status, 0,
                "Invocation job must kill the orphaned descendant"
            );
        }
    }
    let cancel = Cancel::default();
    let task = tokio::spawn(tools::execute(
        dir.0.clone(),
        call(
            "powershell",
            json!({"command":"Write-Output started; Start-Sleep -Seconds 60"}),
        ),
        cancel.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        "cancelled"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn agent_tool_is_not_reexecuted_after_network_failure_or_resume() {
    let dir = TestDir::new();
    let store = dir.store();
    std::fs::write(dir.0.join("a.txt"), "old").unwrap();
    let tool = frame(
        json!([{"functionCall":{"id":"edit-once","name":"edit_file","args":{"path":"a.txt","oldText":"old","newText":"new"}},"thoughtSignature":"signed"}]),
        true,
    );
    let (transport, mut requests, server) = mock_http(vec![
        Reply::sse(tool.clone()),
        Reply {
            status: 503,
            ..Reply::json(Value::Null)
        },
        Reply::sse(tool),
        Reply::sse(frame(json!([{"text":"done"}]), true)),
    ])
    .await;
    let host = NativeHost {
        transport,
        ..NativeHost::default()
    };
    let account = account("one@example.test");
    host.auth
        .save(&store, account.clone(), &Cancel::default())
        .await
        .unwrap();
    let session = host.session("thread", "window", &dir.0).unwrap();
    let mut input = input(&dir.0);
    input.runtime_mode = "auto-accept-edits".into();
    let model = protocol::resolve_model(&input.model, None).unwrap();
    let mut history = History::new(&dir.0, account.email.clone());
    assert_eq!(
        agent_loop(
            &host,
            &session,
            &mut history,
            &input,
            &model,
            &account,
            &store,
            &Cancel::default(),
            &mut |_| {}
        )
        .await
        .unwrap_err()
        .code,
        "network"
    );
    assert_eq!(std::fs::read_to_string(dir.0.join("a.txt")).unwrap(), "new");
    let mut restored: History = store.read(&history.filename()).unwrap().unwrap();
    recover_tools(&mut restored);
    let mut events = vec![];
    agent_loop(
        &host,
        &session,
        &mut restored,
        &input,
        &model,
        &account,
        &store,
        &Cancel::default(),
        &mut |event| events.push(event),
    )
    .await
    .unwrap();
    assert!(!events.iter().any(|e| e["type"] == "tool.started"));
    assert_eq!(restored.tool_results["edit-once"]["output"], "File edited.");
    assert_eq!(events.last().unwrap()["type"], "turn.ready");
    server.await.unwrap();
    let mut count = 0;
    while requests.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 4);
    assert!(!serde_json::to_string(&restored)
        .unwrap()
        .contains("fake-access-secret"));
}
