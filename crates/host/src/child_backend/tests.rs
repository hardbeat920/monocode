//! Port of host/child-backend.test.ts.
//!
//! The unexpected-host-exit fixture uses the actual native supervisor and
//! checks its provider group, a stubborn descendant, and an unrelated child.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use monocode_core::HarnessId;
#[cfg(unix)]
use monocode_harness::core::child::ChildEvent;
use monocode_harness::core::child::SseEvent;
use monocode_harness::core::task::SharedSpawner;
use monocode_remote::host::protocol::REMOTE_PROVIDERS;

use super::*;
use crate::runtime::HostRuntime;
#[cfg(unix)]
use crate::testing::wait_for;
use crate::testing::{fake_provider, fake_providers};

#[cfg(any(unix, windows))]
#[path = "provider_guard_tests.rs"]
mod provider_guard_tests;

const ECHO_ARGS: &str = "console.log(JSON.stringify(process.argv.slice(2)));\n";

fn children_over(
    binaries: HashMap<RemoteProvider, PathBuf>,
) -> (
    HostRuntime,
    tempfile::TempDir,
    monocode_harness::Children,
    Arc<HeadlessChildBackend>,
) {
    let runtime = HostRuntime::new(2);
    let spawner: SharedSpawner = runtime.spawner();
    let data = tempfile::tempdir().unwrap();
    let (children, backend) = host_children(data.path().to_path_buf(), binaries, spawner);
    (runtime, data, children, backend)
}

/// child-backend.test.ts: "resolves every provider and runs only allowed
/// catalog commands".
#[test]
fn resolves_every_provider_and_runs_only_allowed_catalog_commands() {
    let directory = tempfile::tempdir().unwrap();
    let binaries = fake_providers(directory.path(), ECHO_ARGS);
    let (runtime, _data, _children, backend) = children_over(binaries.clone());
    for provider in REMOTE_PROVIDERS {
        let resolved = smol::block_on(backend.resolve_default(provider)).unwrap();
        assert_eq!(resolved.path, binaries[&provider].to_string_lossy());
        if provider == HarnessId::Antigravity {
            let expected: Vec<String> = if cfg!(target_os = "linux") {
                vec!["--uid=".into()]
            } else {
                Vec::new()
            };
            assert_eq!(resolved.args, Some(expected));
        }
    }
    let fx = binaries[&HarnessId::Fx].to_string_lossy().into_owned();
    let output = smol::block_on(backend.exec(ExecRequest {
        command: fx.clone(),
        args: vec!["models".into(), "--json".into()],
        cwd: Some(directory.path().to_string_lossy().into_owned()),
        binary_provider: Some(HarnessId::Fx),
        binary_path: backend.runtime_binary_path(HarnessId::Fx),
    }))
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(output.trim()).unwrap(),
        ["models", "--json"]
    );
    let unsafe_exec = smol::block_on(backend.exec(ExecRequest {
        command: fx,
        args: vec!["-e".into(), "console.log('unsafe')".into()],
        cwd: None,
        binary_provider: Some(HarnessId::Fx),
        binary_path: None,
    }));
    assert_eq!(
        unsafe_exec.unwrap_err(),
        "Unsupported headless catalog command"
    );
    let note = directory.path().join("note.txt");
    std::fs::write(&note, "host-owned transcript").unwrap();
    assert_eq!(
        smol::block_on(backend.read_text_file(note.to_string_lossy().into_owned())).unwrap(),
        "host-owned transcript"
    );
    let transcript = "x".repeat(1024 * 1024 + 1);
    let large = directory.path().join("large-transcript.txt");
    std::fs::write(&large, &transcript).unwrap();
    assert_eq!(
        smol::block_on(backend.read_text_file(large.to_string_lossy().into_owned())).unwrap(),
        transcript
    );
    backend.close();
    runtime.shutdown();
}

/// A loopback server: JSON at `/`, one event and an open stream at `/event`.
fn serve_loopback(stop: Arc<AtomicBool>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                if request.contains(" /event") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {\"type\":\"ready\"}\n\n",
                    );
                    let _ = stream.flush();
                    while !stop.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
                    );
                }
            });
        }
    });
    port
}

/// child-backend.test.ts: "bridges OpenCode HTTP and event streams on
/// loopback".
#[test]
fn bridges_opencode_http_and_event_streams_on_loopback() {
    let stop = Arc::new(AtomicBool::new(false));
    let port = serve_loopback(stop.clone());
    let (runtime, _data, children, backend) = children_over(HashMap::new());
    let base = format!("http://127.0.0.1:{port}");
    let response = smol::block_on(backend.http(HttpRequest {
        url: base.clone(),
        method: "GET".into(),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(
        response,
        HttpResponse {
            status: 200,
            body: "{\"ok\":true}".into(),
        }
    );
    let events = children.watch_sse("fixture");
    smol::block_on(children.open_harness_sse("fixture", &format!("{base}/event"), None)).unwrap();
    let first = smol::block_on(monocode_harness::core::task::timeout(
        Duration::from_secs(5),
        events.recv(),
    ))
    .map(Result::ok);
    assert_eq!(
        first,
        Some(Some(SseEvent::Data("{\"type\":\"ready\"}".into())))
    );
    smol::block_on(children.close_harness_sse("fixture")).unwrap();
    stop.store(true, Ordering::SeqCst);
    let refused = smol::block_on(backend.http(HttpRequest {
        url: "https://example.com/".into(),
        method: "GET".into(),
        ..Default::default()
    }));
    assert!(refused.unwrap_err().contains("localhost"));
    assert!(loopback_url("http://user:secret@127.0.0.1:1/").is_err());
    assert!(loopback_url("http://localhost:4096/session").is_ok());
    backend.close();
    runtime.shutdown();
}

/// child-backend.test.ts: "runs the resolved Claude version fallback in
/// headless mode".
#[test]
fn runs_the_resolved_claude_version_fallback_in_headless_mode() {
    let directory = tempfile::tempdir().unwrap();
    let claude = fake_provider(directory.path(), HarnessId::Claude, ECHO_ARGS);
    let (runtime, _data, children, backend) =
        children_over(HashMap::from([(HarnessId::Claude, claude.clone())]));
    let version = smol::block_on(children.exec_child(
        &claude.to_string_lossy(),
        vec!["--version".into()],
        Some(&directory.path().to_string_lossy()),
        Some(HarnessId::Claude),
        monocode_harness::core::child::BinaryPathChoice::Runtime,
    ))
    .unwrap();
    assert_eq!(version.trim(), "1.0.0 claude codex hermes fixture");
    let refused = smol::block_on(backend.exec(ExecRequest {
        command: claude.to_string_lossy().into_owned(),
        args: vec!["-e".into(), "console.log('unsafe')".into()],
        cwd: None,
        binary_provider: Some(HarnessId::Claude),
        binary_path: None,
    }));
    assert_eq!(refused.unwrap_err(), "Unsupported headless catalog command");
    backend.close();
    runtime.shutdown();
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// child-backend.test.ts: "stops a provider tree (ignores SIGTERM: %s)".
#[cfg(unix)]
#[test]
fn stops_a_provider_tree() {
    for stubborn in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let grandchild = format!(
            "{}console.log('ready'); setInterval(() => {{}}, 1000)",
            if stubborn {
                "process.on('SIGTERM', () => {});"
            } else {
                ""
            }
        );
        let body = format!(
            "const {{ spawn }} = require('node:child_process');
const child = spawn(process.execPath, ['-e', {}], {{ stdio: ['ignore', 'pipe', 'ignore'] }});
child.stdout.once('data', () => console.log(JSON.stringify({{ child: child.pid }})));
setInterval(() => {{}}, 1000);
",
            serde_json::to_string(&grandchild).unwrap()
        );
        let claude = fake_provider(directory.path(), HarnessId::Claude, &body);
        let (runtime, _data, children, backend) =
            children_over(HashMap::from([(HarnessId::Claude, claude.clone())]));
        let events = children.watch_child("tree");
        smol::block_on(children.spawn_child(
            "tree",
            &claude.to_string_lossy(),
            Vec::new(),
            &directory.path().to_string_lossy(),
            None,
            Some(HarnessId::Claude),
        ))
        .unwrap();
        let descendant = smol::block_on(async {
            loop {
                match events.recv().await {
                    Ok(ChildEvent::Stdout(line)) => {
                        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
                        break value["child"].as_u64().unwrap() as u32;
                    }
                    Ok(_) => continue,
                    Err(_) => panic!("the provider ended before reporting its child"),
                }
            }
        });
        assert!(alive(descendant));
        smol::block_on(children.kill_child("tree")).unwrap();
        wait_for("the provider tree to stop", Duration::from_secs(8), || {
            !alive(descendant)
        });
        backend.close();
        runtime.shutdown();
    }
}

#[test]
fn refuses_named_accounts_and_spawns_after_close() {
    let (runtime, _data, _children, backend) = children_over(HashMap::new());
    let named = smol::block_on(backend.spawn(SpawnRequest {
        session_id: "named".into(),
        command: "/bin/true".into(),
        account: Some(monocode_harness::core::child::ChildAccount {
            provider: HarnessId::Claude,
            id: "work".into(),
        }),
        ..Default::default()
    }));
    assert_eq!(
        named.unwrap_err(),
        "Named provider accounts are not supported by this host yet"
    );
    backend.close();
    let closed = smol::block_on(backend.spawn(SpawnRequest::default()));
    assert_eq!(closed.unwrap_err(), "Host is stopping");
    runtime.shutdown();
}
