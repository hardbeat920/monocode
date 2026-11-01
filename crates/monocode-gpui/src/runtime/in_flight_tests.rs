use super::*;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_running_turn_records_its_policy_until_it_finishes() {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "monocode-runtime-in-flight-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    std::fs::create_dir_all(&fixture.0).unwrap();
    let binary = fixture.0.join("mock-provider");
    std::fs::write(
        &binary,
        r#"#!/usr/bin/env python3
import json, os, sys, time
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    value = json.loads(line)
    if value.get('id') == 1:
        send({'id': 1, 'result': {}})
    elif value.get('id') == 2:
        send({'id': 2, 'result': {'thread': {'id': 'in-flight-test'}}})
    elif value.get('id') == 3:
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'a', 'delta': 'Working'}})
        deadline = time.monotonic() + 4
        while not os.path.exists('checked'):
            assert time.monotonic() < deadline, 'The test never inspected the marker'
            time.sleep(0.01)
        send({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
        break
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_monocode(&fixture.0.join("sessions.db")).unwrap();
    let summary = store.create("codex", fixture.0.to_str().unwrap()).unwrap();
    let session = summary.id;
    let store = Arc::new(Mutex::new(store));
    let worker_store = store.clone();
    let (updates, receive) = async_channel::bounded(64);
    let (_controls, control_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt: "Plan the change".into(),
                settings: Settings {
                    plan: true,
                    access: monocode_core::agent::AccessMode::ReadOnly,
                    resume_after_restart: true,
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
    let mut checked = false;
    loop {
        let update = receive
            .recv_blocking()
            .expect("Runtime closed before its final update");
        if !checked && !update.finished && update.rows.iter().any(|row| row.role == "assistant") {
            let turns = locked(&store).unwrap().in_flight().unwrap();
            assert_eq!(turns.len(), 1);
            let (id, turn) = &turns[0];
            assert_eq!(*id, session);
            assert!(turn.plan && turn.resume);
            assert_eq!(turn.access, monocode_core::agent::AccessMode::ReadOnly);
            std::fs::write(fixture.0.join("checked"), "").unwrap();
            checked = true;
        }
        if update.finished {
            assert_eq!(update.status, "Ready");
            break;
        }
    }
    worker.join().unwrap();
    assert!(checked);
    assert!(locked(&store).unwrap().in_flight().unwrap().is_empty());
}
