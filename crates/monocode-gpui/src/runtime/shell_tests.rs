use super::*;
use monocode_core::shell_run::ShellRun;
use std::os::unix::fs::PermissionsExt;

/// The provider must receive a `!command`'s output with the turn it starts.
#[test]
fn shell_output_reaches_the_provider_with_the_next_message() {
    let asked = received("What did that say?");
    assert!(asked.contains("$ git status"), "{asked}");
    assert!(asked.contains("working tree clean"), "{asked}");
    assert!(asked.contains("What did that say?"), "{asked}");
    assert!(asked.contains("context for it"), "{asked}");
    let bare = received("");
    assert!(bare.contains("working tree clean"), "{bare}");
    assert!(bare.contains("respond to what it shows"), "{bare}");
}

/// What a mock provider's `turn/start` got after one stored shell run.
fn received(prompt: &str) -> String {
    let prompt = prompt.to_owned();
    let dir = std::env::temp_dir().join(format!(
        "monocode-runtime-shell-{}-{}-{}",
        monocode_core::time::now_ms(),
        std::process::id(),
        prompt.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let binary = dir.join("mock-provider");
    std::fs::write(
        &binary,
        r#"#!/usr/bin/env python3
import json, sys
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    value = json.loads(line)
    method = value.get('method')
    if method == 'initialize':
        send({'id': value['id'], 'result': {}})
    elif method == 'thread/start':
        send({'id': value['id'], 'result': {'thread': {'id': 'thread'}}})
    elif method == 'turn/start':
        open('received.json', 'w').write(json.dumps(value['params']))
        send({'id': value['id'], 'result': {'turn': {'id': 'turn'}}})
        send({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
        break
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = Store::open_monocode(&dir.join("sessions.db")).unwrap();
    let summary = store.create("codex", dir.to_str().unwrap()).unwrap();
    let mut run = ShellRun::new("git status", dir.to_str().unwrap());
    run.output = "nothing to commit, working tree clean".into();
    run.exit = Some(0);
    run.state = monocode_core::shell_run::State::Completed;
    store.write_rows(summary.id, 0, &[run.row()]).unwrap();
    let store = Arc::new(Mutex::new(store));
    let (updates, receive) = async_channel::bounded(64);
    let (_controls, control_rx) = std::sync::mpsc::channel();
    let worker_store = store.clone();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt,
                settings: Settings {
                    text_only: true,
                    ..Default::default()
                },
            },
            control_rx,
            worker_store,
            Arc::new(AtomicBool::new(false)),
            updates,
            |_| Some(binary),
        );
    });
    let status = loop {
        let update = receive.recv_blocking().unwrap();
        if update.finished {
            break update.status;
        }
    };
    worker.join().unwrap();
    let received = std::fs::read_to_string(dir.join("received.json"))
        .unwrap_or_else(|_| panic!("the provider never started a turn: {status}"));
    let _ = std::fs::remove_dir_all(&dir);
    received
}
