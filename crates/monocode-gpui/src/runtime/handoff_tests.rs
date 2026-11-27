use super::*;
use monocode_core::tool::{ToolCall, ToolState};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn transfer(rows: &[Row]) -> Option<ToolCall> {
    rows.iter()
        .filter(|row| row.role == "tool")
        .filter_map(|row| serde_json::from_str::<ToolCall>(&row.text).ok())
        .find(ToolCall::is_handoff)
}

#[test]
fn a_provider_switch_shows_its_transfer_before_the_new_provider_answers() {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "monocode-runtime-handoff-{}-{}",
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
        send({'id': value['id'], 'result': {'turn': {'id': 'turn'}}})
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'answer', 'delta': 'Answer'}})
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
                prompt: "Carry on".into(),
                settings: Settings {
                    text_only: true,
                    pending_provider_switch: Some(monocode_core::agent::ProviderSwitch {
                        from: "claude".into(),
                        model_label: "Opus 5.5".into(),
                        skip_outgoing_recap: true,
                        ..Default::default()
                    }),
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
    let mut running_seen = false;
    let final_status = loop {
        let update = receive
            .recv_blocking()
            .expect("Runtime closed before its final update");
        if let Some(tool) = transfer(&update.rows)
            && tool.state == ToolState::Running
        {
            assert_eq!(tool.summary().0, "Transferring from Opus 5.5");
            assert!(
                update.rows.iter().all(|row| row.text != "Answer"),
                "the transfer shows before the new provider answers"
            );
            running_seen = true;
        }
        if update.finished {
            break update.status;
        }
    };
    worker.join().unwrap();
    assert_eq!(final_status, "Ready");
    assert!(running_seen);
    let saved = locked(&store).unwrap().load(session).unwrap();
    let settled = transfer(&saved.rows).unwrap();
    assert_eq!(settled.state, ToolState::Completed);
    assert_eq!(settled.summary().0, "Transferred from Opus 5.5");
    assert!(saved.rows.iter().any(|row| row.text == "Answer"));
}
