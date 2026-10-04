//! Ports of child.test.ts, over a fake backend instead of mocked Tauri calls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::*;
use crate::core::task::{SmolSpawner, timeout};
use crate::core::testing::{Call, Fake, children};

fn drain(events: &ChildEvents) -> Vec<ChildEvent> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        out.push(event);
    }
    out
}

#[test]
fn is_current_child_exit_matches_only_the_live_childs_pid() {
    assert!(!is_current_child_exit(None, Some(41)));
    assert!(!is_current_child_exit(Some(42), Some(41)));
    assert!(is_current_child_exit(Some(42), Some(42)));
    assert!(!is_current_child_exit(Some(0), Some(0)));
    assert!(!is_current_child_exit(Some(42), None));
}

#[test]
fn reconciles_an_exit_that_arrives_before_spawn_returns_its_pid() {
    let (pid_tx, pid_rx) = async_channel::bounded(1);
    let (children, _fake) = children(Fake {
        pids: Mutex::new(Some(pid_rx)),
        ..Default::default()
    });
    let events = children.watch_child("probe");
    smol::block_on(async {
        let spawning = {
            let children = children.clone();
            smol::spawn(async move {
                children
                    .spawn_child(
                        "probe",
                        "pi",
                        vec!["--mode".into(), "rpc".into()],
                        "/repo",
                        None,
                        None,
                    )
                    .await
            })
        };
        // Let the spawn reach the backend before the exit lands.
        smol::Timer::after(Duration::from_millis(10)).await;
        children.router().on_exit("probe", Some(1), 42);
        assert!(drain(&events).is_empty());

        pid_tx.send(42).await.unwrap();
        spawning.await.unwrap();
        assert_eq!(drain(&events), vec![ChildEvent::Exit(Some(1))]);
    });
}

#[test]
fn passes_stored_overrides_through_resolution_and_command_validation() {
    let runtime_paths: HashMap<HarnessId, String> = [
        (HarnessId::Claude, "/opt/claude/bin/claude"),
        (HarnessId::Codex, "/opt/codex/bin/codex"),
        (HarnessId::Cursor, "/opt/cursor/bin/cursor-agent"),
        (HarnessId::Grok, "/opt/grok/bin/grok"),
        (HarnessId::Opencode, "/opt/opencode/bin/opencode"),
        (HarnessId::Pi, "/opt/pi/bin/pi"),
        (HarnessId::Omp, "/opt/omp/bin/omp"),
        (HarnessId::Fx, "/opt/fx/bin/fx"),
        (HarnessId::Hermes, "/opt/hermes/bin/hermes"),
        (
            HarnessId::Antigravity,
            "/opt/antigravity/bin/agy_acp_server.par",
        ),
    ]
    .into_iter()
    .map(|(id, path)| (id, path.to_string()))
    .collect();
    let (children, fake) = children(Fake {
        runtime_paths: runtime_paths.clone(),
        ..Default::default()
    });
    smol::block_on(async {
        children.resolve_codex_binary().await.unwrap();
        assert_eq!(
            fake.calls().last(),
            Some(&Call::ResolveConfigured(
                HarnessId::Codex,
                "/opt/codex/bin/codex".into()
            ))
        );
        for provider in [
            HarnessId::Claude,
            HarnessId::Cursor,
            HarnessId::Grok,
            HarnessId::Pi,
            HarnessId::Omp,
            HarnessId::Fx,
            HarnessId::Hermes,
            HarnessId::Antigravity,
        ] {
            children.resolve_binary(provider).await.unwrap();
            assert_eq!(
                fake.calls().last(),
                Some(&Call::ResolveConfigured(
                    provider,
                    runtime_paths[&provider].clone()
                ))
            );
        }
        // Droid has no stored override, so it uses its default resolver.
        children.resolve_droid_binary().await.unwrap();
        assert_eq!(
            fake.calls().last(),
            Some(&Call::ResolveDefault(HarnessId::Droid))
        );

        children
            .exec_child(
                "/resolved",
                vec!["--version".into()],
                None,
                Some(HarnessId::Opencode),
                BinaryPathChoice::Runtime,
            )
            .await
            .unwrap();
        assert_eq!(
            fake.calls().last(),
            Some(&Call::Exec(ExecRequest {
                command: "/resolved".into(),
                args: vec!["--version".into()],
                cwd: None,
                binary_provider: Some(HarnessId::Opencode),
                binary_path: Some("/opt/opencode/bin/opencode".into()),
            }))
        );

        children
            .exec_child(
                "/resolved",
                vec!["--version".into()],
                None,
                Some(HarnessId::Codex),
                BinaryPathChoice::Given(None),
            )
            .await
            .unwrap();
        assert_eq!(
            fake.calls().last(),
            Some(&Call::Exec(ExecRequest {
                command: "/resolved".into(),
                args: vec!["--version".into()],
                cwd: None,
                binary_provider: Some(HarnessId::Codex),
                binary_path: None,
            }))
        );

        // An explicit path is trimmed, and a blank one means the default.
        children
            .resolve_harness_binary(
                HarnessId::Pi,
                BinaryPathChoice::Given(Some("  /x/pi ".into())),
            )
            .await
            .unwrap();
        assert_eq!(
            fake.calls().last(),
            Some(&Call::ResolveConfigured(HarnessId::Pi, "/x/pi".into()))
        );
        children
            .resolve_harness_binary(HarnessId::Pi, BinaryPathChoice::Given(Some("  ".into())))
            .await
            .unwrap();
        assert_eq!(
            fake.calls().last(),
            Some(&Call::ResolveDefault(HarnessId::Pi))
        );
    });
}

#[test]
fn never_routes_a_retired_generations_stdout_or_exit_to_its_replacement() {
    let (children, _fake) = children(Fake::default());
    let router = children.router().clone();
    let old = children.watch_child("thread#0");
    router.on_stdout("thread#0", "gen0".into());
    assert_eq!(drain(&old), vec![ChildEvent::Stdout("gen0".into())]);

    // Retire the generation, then register its replacement under a new key.
    children.unwatch_child("thread#0");
    let new = children.watch_child("thread#1");

    // Late output from the killed process arrives under its own key and is
    // buffered there. It can never reach the replacement.
    router.on_stdout("thread#0", "gen0-late".into());
    router.on_exit("thread#0", Some(1), 42);
    assert!(drain(&old).is_empty());
    assert!(old.is_closed());
    assert!(drain(&new).is_empty());

    // The replacement still receives its own traffic.
    router.on_stdout("thread#1", "gen1".into());
    assert_eq!(drain(&new), vec![ChildEvent::Stdout("gen1".into())]);
}

#[test]
fn buffers_lines_until_a_watcher_attaches_and_caps_the_buffer() {
    let (children, _fake) = children(Fake::default());
    let router = children.router().clone();
    for index in 0..(MAX_BUFFERED + 5) {
        router.on_stdout("early", format!("line {index}"));
    }
    router.on_stderr("early", "dropped".into());
    let events = children.watch_child("early");
    let lines = drain(&events);
    assert_eq!(lines.len(), MAX_BUFFERED);
    assert_eq!(lines[0], ChildEvent::Stdout("line 5".into()));
    router.on_stderr("early", "kept".into());
    assert_eq!(drain(&events), vec![ChildEvent::Stderr("kept".into())]);
}

#[test]
fn spawn_kill_and_write_reach_the_backend() {
    let (children, fake) = children(Fake {
        runtime_paths: [(HarnessId::Codex, "/opt/codex".to_string())].into(),
        ..Default::default()
    });
    let events = children.watch_child("s1");
    smol::block_on(async {
        children
            .spawn_child(
                "s1",
                "/opt/codex",
                vec!["app-server".into()],
                "/repo",
                Some(ChildAccount {
                    provider: HarnessId::Codex,
                    id: "work".into(),
                }),
                Some(HarnessId::Codex),
            )
            .await
            .unwrap();
        children.write_child("s1", "{}").await.unwrap();
        // A stale pid is ignored; the live one is delivered once.
        children.router().on_exit("s1", Some(9), 3);
        children.router().on_exit("s1", Some(0), 7);
        children.router().on_exit("s1", Some(0), 7);
        assert_eq!(drain(&events), vec![ChildEvent::Exit(Some(0))]);
        children.kill_child("s1").await.unwrap();
    });
    assert!(events.is_closed());
    assert_eq!(
        fake.calls(),
        vec![
            Call::Spawn(SpawnRequest {
                session_id: "s1".into(),
                command: "/opt/codex".into(),
                args: vec!["app-server".into()],
                cwd: "/repo".into(),
                account: Some(ChildAccount {
                    provider: HarnessId::Codex,
                    id: "work".into()
                }),
                binary_provider: Some(HarnessId::Codex),
                binary_path: Some("/opt/codex".into()),
                environment: Default::default(),
            }),
            Call::Write("s1".into(), "{}".into()),
            Call::Kill("s1".into()),
        ]
    );
}

#[test]
fn watch_child_with_calls_the_handlers_in_order() {
    let (children, _fake) = children(Fake::default());
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let (lines, exits, errors) = (seen.clone(), seen.clone(), seen.clone());
    let (exited_tx, exited_rx) = async_channel::bounded(1);
    children.watch_child_with(
        "cb",
        ChildHandlers {
            on_line: Box::new(move |line| lines.lock().push(format!("out {line}"))),
            on_exit: Box::new(move |code| {
                exits.lock().push(format!("exit {code:?}"));
                let _ = exited_tx.try_send(());
            }),
            on_stderr: Some(Box::new(move |line| {
                errors.lock().push(format!("err {line}"))
            })),
        },
    );
    smol::block_on(async {
        children
            .spawn_child("cb", "x", vec![], "/", None, None)
            .await
            .unwrap();
        children.router().on_stdout("cb", "a".into());
        children.router().on_stderr("cb", "b".into());
        children.router().on_exit("cb", None, 7);
        timeout(Duration::from_secs(2), exited_rx.recv())
            .await
            .unwrap()
            .unwrap();
    });
    assert_eq!(*seen.lock(), vec!["out a", "err b", "exit None"]);
}

#[test]
fn routes_sse_frames_and_closes_the_stream() {
    let (children, fake) = children(Fake::default());
    let router = children.router().clone();
    router.on_sse("oc", "early".into());
    let events = children.watch_sse("oc");
    router.on_sse("oc", "next".into());
    router.on_sse_end("oc", Some("boom".into()));
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        seen.push(event);
    }
    assert_eq!(
        seen,
        vec![
            SseEvent::Data("early".into()),
            SseEvent::Data("next".into()),
            SseEvent::End(Some("boom".into())),
        ]
    );
    smol::block_on(children.close_harness_sse("oc")).unwrap();
    assert!(events.is_closed());
    assert_eq!(fake.calls(), vec![Call::SseClose("oc".into())]);
}

#[test]
fn the_last_bridge_lease_clears_routing_state() {
    let (children, _fake) = children(Fake::default());
    let app = children.start_harness_bridge();
    let probe = smol::block_on(children.acquire_harness_bridge()).unwrap();
    let events = children.watch_child("s");
    drop(probe);
    smol::block_on(smol::Timer::after(Duration::from_millis(20)));
    assert!(!events.is_closed(), "another lease is still held");
    drop(app);
    smol::block_on(timeout(Duration::from_secs(2), async {
        while !events.is_closed() {
            smol::Timer::after(Duration::from_millis(1)).await;
        }
    }))
    .expect("the router clears after the last release");
}

#[test]
fn inspects_a_binary_version() {
    let (children, _fake) = children(Fake::default());
    let inspection = smol::block_on(
        children.inspect_harness_binary(HarnessId::Codex, BinaryPathChoice::Runtime),
    )
    .unwrap();
    assert_eq!(
        inspection,
        HarnessBinaryInspection {
            path: "/resolved".into(),
            version: Some("tool 1.2.3".into()),
            error: None,
        }
    );
    let antigravity = smol::block_on(
        children.inspect_harness_binary(HarnessId::Antigravity, BinaryPathChoice::Runtime),
    )
    .unwrap();
    assert_eq!(antigravity.version.as_deref(), Some("ACP server"));
}

#[test]
fn host_backend_reads_text_files_and_rejects_binaries() {
    let dir = std::env::temp_dir().join(format!("monocode-harness-child-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = dir.join("a.txt");
    let binary = dir.join("b.bin");
    std::fs::write(&text, "hello").unwrap();
    std::fs::write(&binary, [0u8, 1, 2]).unwrap();
    assert_eq!(read_text_file(&text.to_string_lossy()).unwrap(), "hello");
    assert_eq!(
        read_text_file(&binary.to_string_lossy()).unwrap_err(),
        "Binary files cannot be edited."
    );
    assert_eq!(
        read_text_file(&dir.to_string_lossy()).unwrap_err(),
        "Not a file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn host_backend_spawns_and_routes_a_real_child() {
    if cfg!(windows) {
        return;
    }
    let (children, _host) = Children::for_host(HostChildOptions::default(), Arc::new(SmolSpawner));
    // `harness_spawn` only runs resolved harness CLIs, so a plain shell is
    // refused before it forks.
    let error =
        smol::block_on(children.spawn_child("sh", "/bin/sh", vec![], "/", None, None)).unwrap_err();
    assert!(
        error.to_string().contains("not a resolved harness CLI"),
        "{error}"
    );
}
