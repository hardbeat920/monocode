use super::*;
use monocode_core::remote::{Connections, Machine};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::atomic::AtomicUsize,
    thread,
    time::Instant,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "monocode-remote-runtime-{}",
            monocode_core::mono::new_id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> Database {
        Arc::new(Mutex::new(Store::open(&self.0.join("native.db")).unwrap()))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Host {
    client: Arc<Connections>,
    machine: Machine,
    stopped: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
    _directory: Directory,
}
impl Host {
    fn new(mut handler: impl FnMut(Value) -> Option<Value> + Send + 'static) -> Self {
        let directory = Directory::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let done = stopped.clone();
        let server = thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut size = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        size = value.trim().parse().unwrap();
                    }
                }
                assert!(size <= 4 * 1024 * 1024);
                let mut bytes = vec![0; size];
                reader.read_exact(&mut bytes).unwrap();
                let payload: Value = serde_json::from_slice(&bytes).unwrap();
                let result = if payload["method"] == "environment.describe" {
                    assert!(
                        payload["params"]["supportedProviders"]
                            .as_array()
                            .unwrap()
                            .contains(&json!("devin"))
                    );
                    Some(
                        json!({"protocolVersion":1,"environmentId":"env","name":"Fixture","providers":["codex"]}),
                    )
                } else {
                    assert_eq!(payload["environmentId"], "env");
                    handler(payload)
                };
                // Dropping a socket after handling a command models a lost HTTP
                // response, so the next attempt must retain its original ID.
                if let Some(result) = result {
                    let body = if result.get("error").is_some() {
                        result
                    } else {
                        json!({"result":result})
                    }
                    .to_string();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                }
            }
        });
        let client = Arc::new(Connections::new(&directory.0));
        let machine = client.connect("".into(), url, "a".repeat(43)).unwrap();
        Self {
            client,
            machine,
            stopped,
            server: Some(server),
            _directory: directory,
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.server.take().unwrap().join().unwrap();
    }
}
fn binding(store: &Database) -> RemoteSession {
    let store = locked(store).unwrap();
    let project = store
        .remember_remote_project("env", &json!({"id":"project","cwd":"/host/repo"}))
        .unwrap();
    let summary = store.create("codex", &project.key).unwrap();
    store.remote_session(summary.id).unwrap().unwrap()
}
fn receipt(command: &Value) -> Value {
    json!({"commandId":command["commandId"],"sessionId":"host-session","revision":1})
}
fn snapshot(status: &str, revision: u64) -> Value {
    json!({"session":{"id":"host-session","harness":"codex","title":"Fixture","model":"codex:fixture","modelSettings":{},"runtimeMode":"supervised","blocks":[{"id":"u","role":"user","text":"Original message"}]},"projectId":"project","revision":revision,"status":status,"runId":"host-run","updatedAt":20})
}
fn receive(rx: &async_channel::Receiver<Update>) -> Update {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match rx.try_recv() {
            Ok(value) => return value,
            Err(async_channel::TryRecvError::Closed) => {
                panic!("Remote worker closed without an update")
            }
            Err(_) => {}
        }
        assert!(
            Instant::now() < deadline,
            "Remote worker did not publish an update"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn lost_create_receipt_retries_original_ids_from_durable_outbox() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    binding.outbox = vec![
        json!({"type":"create","commandId":"create-original"}),
        json!({"type":"send","commandId":"send-original","text":"hello"}),
    ];
    journal(&store, &binding).unwrap();
    let saved = store.clone();
    let desktop_id = binding.summary.id;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let mut creates = 0;
    let host = Host::new(move |request| {
        let command = &request["params"];
        let pending = locked(&saved)
            .unwrap()
            .remote_session(desktop_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            pending.outbox[0]["commandId"], command["commandId"],
            "journal must precede network I/O"
        );
        recorded.lock().unwrap().push(command.clone());
        if command["type"] == "create" {
            creates += 1;
            if creates == 1 {
                return None;
            }
        } else {
            assert_eq!(command["sessionId"], "host-session");
        }
        Some(receipt(command))
    });
    assert!(dispatch_outbox(&host.client, &host.machine.id, &store, &mut binding).is_err());
    assert_eq!(binding.session_id, None);
    let mut restored = locked(&store)
        .unwrap()
        .remote_session(binding.summary.id)
        .unwrap()
        .unwrap();
    dispatch_outbox(&host.client, &host.machine.id, &store, &mut restored).unwrap();
    assert!(restored.outbox.is_empty());
    assert_eq!(restored.session_id.as_deref(), Some("host-session"));
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[2]["commandId"], "send-original");
}

#[test]
fn mismatched_receipt_never_commits_a_session_or_discards_pending_command() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    binding
        .outbox
        .push(json!({"type":"create","commandId":"original"}));
    journal(&store, &binding).unwrap();
    let host =
        Host::new(|_| Some(json!({"commandId":"wrong","sessionId":"unrelated","revision":1})));
    assert!(dispatch_outbox(&host.client, &host.machine.id, &store, &mut binding).is_err());
    assert!(binding.session_id.is_none());
    let durable = locked(&store)
        .unwrap()
        .remote_session(binding.summary.id)
        .unwrap()
        .unwrap();
    assert!(durable.session_id.is_none());
    assert_eq!(durable.outbox[0]["commandId"], "original");
}

#[test]
fn attachment_upload_is_bounded_journaled_and_retries_the_same_uuid() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    let file = dir.0.join("image.png");
    std::fs::write(&file, vec![b'x'; ATTACHMENT_CHUNK_BYTES + 13]).unwrap();
    let (references, uploads) = prepare_attachments(std::slice::from_ref(&file), "send").unwrap();
    let id = references[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "4");
    binding
        .outbox
        .push(json!({"type":"send","commandId":"send","attachments":references}));
    binding.uploads = uploads;
    journal(&store, &binding).unwrap();
    let bytes = Arc::new(Mutex::new(Vec::<u8>::new()));
    let received = bytes.clone();
    let saved = store.clone();
    let desktop_id = binding.summary.id;
    let mut lost = true;
    let host = Host::new(move |request| {
        assert_eq!(request["method"], "attachments.upload");
        let params = &request["params"];
        let durable = locked(&saved)
            .unwrap()
            .remote_session(desktop_id)
            .unwrap()
            .unwrap();
        assert_eq!(params["id"], durable.uploads[0].id);
        let data = STANDARD.decode(params["data"].as_str().unwrap()).unwrap();
        assert!(data.len() <= ATTACHMENT_CHUNK_BYTES);
        let offset = params["offset"].as_u64().unwrap() as usize;
        let mut bytes = received.lock().unwrap();
        if offset == bytes.len() {
            bytes.extend_from_slice(&data);
        } else {
            assert_eq!(&bytes[offset..offset + data.len()], data);
        }
        if lost {
            lost = false;
            return None;
        }
        Some(json!({"offset":offset+data.len()}))
    });
    assert!(upload_pending(&host.client, &host.machine.id, &store, &mut binding).is_err());
    assert_eq!(binding.uploads[0].id, id);
    let mut restored = locked(&store)
        .unwrap()
        .remote_session(binding.summary.id)
        .unwrap()
        .unwrap();
    upload_pending(&host.client, &host.machine.id, &store, &mut restored).unwrap();
    assert!(restored.uploads.is_empty());
    assert_eq!(bytes.lock().unwrap().len(), ATTACHMENT_CHUNK_BYTES + 13);
    let huge = dir.0.join("huge.bin");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_ATTACHMENT_BYTES + 1)
        .unwrap();
    assert!(prepare_attachments(&[huge], "too-big").is_err());
    assert!(prepare_attachments(&vec![file; 21], "too-many").is_err());
}

#[test]
fn question_labels_map_to_host_ids_and_resolved_approvals_disappear() {
    let value = json!({"session":{"pendingQuestion":{"requestId":7,"questions":[
        {"id":"q","prompt":"Choose","multiSelect":true,"allowCustom":true,"options":[{"id":"stable-id","label":"Visible label"}]}]},
        "blocks":[{"id":"live","text":"Write file","tool":{"title":"Edit","preview":{"title":"/repo/file"}},"approval":{"requestId":2}},
        {"approval":{"requestId":3,"decided":"allow"}},{"approval":{"requestId":4,"decided":"deny"}},{"approval":{"requestId":5,"decided":"cancelled"}}]}});
    let question = question(&value).unwrap();
    assert_eq!(question.id, "7");
    assert!(question.items[0].multiple);
    let answer = reply(
        &question,
        vec![vec!["Visible label".into(), "custom text".into()]],
    );
    assert_eq!(answer["answers"]["q"], json!(["stable-id"]));
    assert_eq!(answer["custom"]["q"], "custom text");
    let approvals = approvals(&value);
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].id, "2");
    assert_eq!(approvals[0].detail, "/repo/file");
}

#[test]
fn model_ids_and_codex_controls_match_host_wire_schema() {
    assert_eq!(
        host_model("codex", Some("gpt-6.1-sol")).unwrap(),
        "codex:gpt-6.1-sol"
    );
    assert_eq!(
        host_model("opencode", Some("opencode:openai/gpt")).unwrap(),
        "opencode:openai/gpt"
    );
    assert!(host_model("codex", None).is_err());
    let settings = Settings {
        effort: Some("xhigh".into()),
        fast: true,
        service_tier: Some("priority".into()),
        ..Default::default()
    };
    let values = model_settings("codex", &settings);
    assert_eq!(values["reasoningEffort"], "xhigh");
    assert_eq!(values["serviceTier"], "priority");
    assert!(!values.contains_key("effort"));
    assert!(!values.contains_key("fast"));
    let values = model_settings("claude", &settings);
    assert_eq!(values["effort"], "xhigh");
    assert_eq!(values["fast"], "true");
}

#[test]
fn attaching_never_replays_pending_sends_or_stops_a_run_when_receiver_closes() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    binding.session_id = Some("host-session".into());
    binding.outbox.push(json!({"type":"send","commandId":"pending-original","sessionId":"host-session","text":"not yet confirmed"}));
    journal(&store, &binding).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let host = Host::new(move |request| {
        recorded.lock().unwrap().push(request["method"].clone());
        Some(json!({"kind":"snapshot","value":snapshot("running",1)}))
    });
    let (tx, rx) = async_channel::unbounded();
    let (_controls_tx, controls) = std::sync::mpsc::channel();
    let settings = Settings {
        remote_action: Some(json!({"type":"attach"})),
        ..Default::default()
    };
    let client = host.client.clone();
    let summary = binding.summary.clone();
    let worker_store = store.clone();
    let worker = thread::spawn(move || {
        run_with_client(
            client,
            summary,
            String::new(),
            settings,
            controls,
            worker_store,
            Arc::new(AtomicBool::new(false)),
            tx,
        )
    });
    let next = receive(&rx);
    assert!(!next.finished);
    assert_eq!(next.rows[0].text, "Original message");
    drop(rx);
    worker.join().unwrap();
    assert!(
        requests
            .lock()
            .unwrap()
            .iter()
            .all(|method| method == "sessions.sync")
    );
    assert_eq!(
        locked(&store)
            .unwrap()
            .remote_session(binding.summary.id)
            .unwrap()
            .unwrap()
            .outbox[0]["commandId"],
        "pending-original"
    );
}

#[test]
fn explicit_stop_is_journaled_once_for_the_observed_run() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    binding.session_id = Some("host-session".into());
    journal(&store, &binding).unwrap();
    let stops = Arc::new(AtomicUsize::new(0));
    let sent = stops.clone();
    let mut polls = 0;
    let host = Host::new(move |request| {
        if request["method"] == "commands.dispatch" {
            let command = &request["params"];
            assert_eq!(command["type"], "cancel");
            assert_eq!(command["runId"], "host-run");
            sent.fetch_add(1, Ordering::Relaxed);
            Some(receipt(command))
        } else {
            polls += 1;
            Some(
                json!({"kind":"snapshot","value":snapshot(if polls<4 {"running"}else{"idle"},polls)}),
            )
        }
    });
    let (tx, rx) = async_channel::unbounded();
    let (_controls_tx, controls) = std::sync::mpsc::channel();
    let settings = Settings {
        remote_action: Some(json!({"type":"attach"})),
        ..Default::default()
    };
    let client = host.client.clone();
    let summary = binding.summary.clone();
    let worker = thread::spawn(move || {
        run_with_client(
            client,
            summary,
            String::new(),
            settings,
            controls,
            store,
            Arc::new(AtomicBool::new(true)),
            tx,
        )
    });
    loop {
        if receive(&rx).finished {
            break;
        }
    }
    worker.join().unwrap();
    assert_eq!(stops.load(Ordering::Relaxed), 1);
}

/// This crosses the production Rust client, Node HTTP server, SQLite engine,
/// attachment parser and provider adapter. The fixture provider performs no
/// model requests; only its local JSON-lines protocol is exercised.
#[cfg(unix)]
#[test]
#[ignore = "requires npm run host:build and loopback networking"]
fn built_node_host_round_trip_preserves_native_wire_commands() {
    use std::{
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    let directory = Directory::new();
    let cli = std::env::var_os("MONOCODE_TEST_HOST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/host/monocode-host.mjs")
        });
    assert!(cli.is_file(), "Run npm run host:build first");
    let node = std::env::var("MONOCODE_TEST_NODE").unwrap_or_else(|_| "node".into());
    let bin = directory.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let provider = bin.join("codex");
    std::fs::write(&provider,r#"#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const send = value => process.stdout.write(JSON.stringify(value) + '\n');
if (process.argv.includes('--version')) { console.log('codex fixture'); process.exit(0); }
require('node:readline').createInterface({input:process.stdin}).on('line', line => {
  const request = JSON.parse(line);
  if (request.id == null) return;
  if (request.method === 'initialize') send({id:request.id,result:{}});
  else if (request.method === 'account/read') send({id:request.id,result:{account:{type:'fixture'},requiresOpenaiAuth:false}});
  else if (request.method === 'model/list') send({id:request.id,result:{data:[{model:'fixture-model',displayName:'Fixture',supportedReasoningEfforts:[{reasoningEffort:'high'}]}],nextCursor:null}});
  else if (request.method === 'thread/start' || request.method === 'thread/resume') send({id:request.id,result:{thread:{id:'fixture-thread'}}});
  else if (request.method === 'turn/start') {
    fs.appendFileSync(path.join(__dirname,'calls.log'),JSON.stringify(request.params)+'\n');
    send({id:request.id,result:{turn:{id:'fixture-turn'}}});
    setTimeout(() => {
      send({method:'item/agentMessage/delta',params:{threadId:'fixture-thread',turnId:'fixture-turn',itemId:'message',delta:'Native fixture completed'}});
      send({method:'turn/completed',params:{threadId:'fixture-thread',turn:{id:'fixture-turn',status:'completed'}}});
    },30);
  } else send({id:request.id,result:{}});
});
"#).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o700)).unwrap();
    let data = directory.0.join("host");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let log_path = directory.0.join("host.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let fake_path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = Command::new(&node)
        .arg(&cli)
        .args(["serve", "--data-dir"])
        .arg(&data)
        .args(["--port", &port.to_string()])
        .env("PATH", fake_path)
        .env("NODE_OPTIONS", "--no-experimental-webstorage")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    struct RunningHost {
        child: std::process::Child,
        node: String,
        cli: PathBuf,
        data: PathBuf,
    }
    impl Drop for RunningHost {
        fn drop(&mut self) {
            let _ = Command::new(&self.node)
                .arg(&self.cli)
                .args(["stop", "--data-dir"])
                .arg(&self.data)
                .env("NODE_OPTIONS", "--no-experimental-webstorage")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    let mut host = RunningHost {
        child,
        node: node.clone(),
        cli: cli.clone(),
        data: data.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(12);
    while !data.join("running.json").is_file() {
        assert!(
            host.child.try_wait().unwrap().is_none(),
            "{}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );
        assert!(
            Instant::now() < deadline,
            "Host startup timed out: {}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(50));
    }
    let pair = Command::new(&node)
        .arg(&cli)
        .args(["pair", "--name", "Native fixture", "--json", "--data-dir"])
        .arg(&data)
        .env("NODE_OPTIONS", "--no-experimental-webstorage")
        .output()
        .unwrap();
    assert!(
        pair.status.success(),
        "{}",
        String::from_utf8_lossy(&pair.stderr)
    );
    let pair: Value = serde_json::from_slice(&pair.stdout).unwrap();
    let client = Arc::new(Connections::new(directory.0.join("connections")));
    let machine = client
        .connect(
            "Fixture".into(),
            format!("http://127.0.0.1:{port}"),
            pair["token"].as_str().unwrap().into(),
        )
        .unwrap();
    let repo = directory.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let opened = client
        .request(&machine.id, "projects.open", json!({"cwd":repo}))
        .unwrap();
    let store = directory.store();
    let project = locked(&store)
        .unwrap()
        .remember_remote_project(&machine.environment_id, &opened)
        .unwrap();
    let summary = locked(&store)
        .unwrap()
        .create("codex", &project.key)
        .unwrap();
    let attachment = directory.0.join("empty.txt");
    std::fs::write(&attachment, []).unwrap();
    let settings = Settings {
        model: Some("codex:fixture-model".into()),
        effort: Some("high".into()),
        attachments: vec![attachment],
        ..Default::default()
    };
    let run_action = |settings: Settings, text: &str| {
        let (tx, rx) = async_channel::unbounded();
        let (_controls_tx, controls) = std::sync::mpsc::channel();
        let worker_client = client.clone();
        let worker_store = store.clone();
        let worker_summary = summary.clone();
        let text = text.to_owned();
        let worker = thread::spawn(move || {
            run_with_client(
                worker_client,
                worker_summary,
                text,
                settings,
                controls,
                worker_store,
                Arc::new(AtomicBool::new(false)),
                tx,
            )
        });
        let mut rows = Vec::new();
        loop {
            let update = receive(&rx);
            if !update.rows.is_empty() {
                rows = update.rows;
            }
            if update.finished {
                assert_eq!(update.status, "Ready");
                break;
            }
        }
        worker.join().unwrap();
        rows
    };
    let rows = run_action(settings, "Hello fixture");
    assert!(
        rows.iter()
            .any(|row| row.role == "assistant" && row.text.contains("Native fixture completed"))
    );
    let binding = locked(&store)
        .unwrap()
        .remote_session(summary.id)
        .unwrap()
        .unwrap();
    assert!(binding.outbox.is_empty() && binding.uploads.is_empty());
    let session_id = binding.session_id.as_deref().unwrap();
    let snapshot = client.load_session(&machine.id, session_id, None).unwrap();
    assert_eq!(
        snapshot["session"]["modelSettings"]["reasoningEffort"],
        "high"
    );
    assert_eq!(
        snapshot["session"]["blocks"][0]["attachments"][0]["size"],
        0
    );
    let calls = std::fs::read_to_string(bin.join("calls.log")).unwrap();
    assert!(
        calls
            .lines()
            .any(|line| serde_json::from_str::<Value>(line).unwrap()["effort"] == "high")
    );
    let draft = Settings {
        model: Some("codex:fixture-model".into()),
        remote_action: Some(json!({"type":"draft"})),
        ..Default::default()
    };
    run_action(draft, "Keep this draft");
    let snapshot = client.load_session(&machine.id, session_id, None).unwrap();
    let draft = snapshot["session"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["draft"] == true)
        .unwrap();
    let remove = Settings {
        model: Some("codex:fixture-model".into()),
        remote_action: Some(json!({"type":"removeDraft","draftBlockId":draft["id"]})),
        ..Default::default()
    };
    run_action(remove, "");
    let snapshot = client.load_session(&machine.id, session_id, None).unwrap();
    assert!(
        !snapshot["session"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["draft"] == true)
    );
}

#[test]
fn definite_rejection_clears_dependent_commands_but_keeps_original_prompt_for_recovery() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    binding.outbox = vec![
        json!({"type":"configure","commandId":"configuration","sessionId":"host-session"}),
        json!({"type":"send","commandId":"send-original","text":"recover this prompt"}),
    ];
    binding.session_id = Some("host-session".into());
    journal(&store, &binding).unwrap();
    let host = Host::new(|_| Some(json!({"error":"Invalid model"})));
    assert!(
        dispatch_outbox(&host.client, &host.machine.id, &store, &mut binding)
            .unwrap_err()
            .starts_with("Host rejected request:")
    );
    assert!(binding.outbox.is_empty() && binding.uploads.is_empty());
    let durable = locked(&store)
        .unwrap()
        .remote_session(binding.summary.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        durable.rejected.unwrap()["commands"][1]["text"],
        "recover this prompt"
    );
}

#[test]
fn ambiguous_legacy_requests_block_direct_send_and_retry_before_transport() {
    let dir = Directory::new();
    let store = dir.store();
    let mut binding = binding(&store);
    binding.rejected = Some(json!({"legacyRecovery":true,"error":"Recover the original request"}));
    binding.outbox = vec![json!({"type":"create","commandId":"original-create"})];
    journal(&store, &binding).unwrap();
    let client = Arc::new(monocode_core::remote::Connections::new(&dir.0));
    for action in ["send", "retry"] {
        let (tx, rx) = async_channel::unbounded();
        let (_controls_tx, controls) = std::sync::mpsc::channel();
        run_with_client(
            client.clone(),
            binding.summary.clone(),
            "new message".into(),
            Settings {
                remote_action: Some(json!({"type":action})),
                ..Default::default()
            },
            controls,
            store.clone(),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let result = receive(&rx);
        assert!(result.finished);
        assert_eq!(result.status, "Recover the original request");
        assert_eq!(
            locked(&store)
                .unwrap()
                .remote_session(binding.summary.id)
                .unwrap()
                .unwrap()
                .outbox,
            binding.outbox
        );
    }
}

#[test]
fn host_context_and_latest_turn_metrics_use_actual_session_fields() {
    let value = json!({"session":{"context":{"used":1200,"window":128000},"blocks":[
        {"role":"user","turnMetrics":{"inputTokens":1}},
        {"role":"user","turnMetrics":{"inputTokens":100,"outputTokens":25,"cacheReadTokens":80,"cacheWriteTokens":10,"cacheHitPercent":80.0}},
        {"role":"assistant","turnMetrics":{"inputTokens":999}},
        {"role":"user","draft":true,"turnMetrics":{"inputTokens":999}}
    ]}});
    let (context, metrics) = accounting(&value);
    assert_eq!(
        context,
        Some(monocode_core::context::Usage {
            used: 1200,
            window: Some(128000),
            cache_ttl_ms: None,
            cost_total_usd: None,
        })
    );
    let metrics = metrics.unwrap();
    assert_eq!(metrics.input_tokens, 100);
    assert_eq!(metrics.output_tokens, 25);
    assert_eq!(metrics.cache_read_tokens, 80);
    assert_eq!(metrics.cache_hit_percent, Some(80.0));
    let value = json!({"session":{"context":{"used":0},"blocks":[{"role":"user"}]}});
    let (context, metrics) = accounting(&value);
    assert_eq!(context.unwrap().window, None);
    assert!(metrics.is_none());
}
