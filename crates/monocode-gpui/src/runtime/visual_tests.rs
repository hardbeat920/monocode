use super::*;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn silent_provider_visual_is_saved_and_acknowledged_before_streaming_resumes() {
    visual_turn(None);
}

#[test]
fn visualize_command_sends_required_workflow_and_keeps_the_visible_request() {
    visual_turn(Some(""));
    visual_turn(Some("Compare the two approaches"));
}

fn visual_turn(request: Option<&str>) {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "monocode-runtime-visual-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    std::fs::create_dir_all(&fixture.0).unwrap();
    let prompt = match request {
        Some("") => "/visualize".to_owned(),
        Some(request) => format!("/visualize {request}"),
        None => "Show a native visual".to_owned(),
    };
    let prepared_prompt = request.map(monocode_core::visual_bridge::requested_prompt);
    std::fs::write(
        fixture.0.join("expected-prompt.txt"),
        prepared_prompt.as_ref().unwrap_or(&prompt),
    )
    .unwrap();
    let binary = fixture.0.join("mock-provider");
    std::fs::write(&binary, r#"#!/usr/bin/env python3
import json, os, socket, sys, time
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    value = json.loads(line)
    if value.get('id') == 1:
        send({'id': 1, 'result': {}})
    elif value.get('id') == 2:
        assert value['method'] == 'thread/start'
        send({'id': 2, 'result': {'thread': {'id': 'native-visual-test'}}})
    elif value.get('id') == 3:
        assert value['method'] == 'turn/start'
        with open('expected-prompt.txt') as expected:
            assert value['params']['input'] == [{'type': 'text', 'text': expected.read()}], 'The provider must receive the prepared prompt'
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'before', 'delta': 'Before the visual.'}})
        time.sleep(0.05)
        address, port = os.environ['MONOCODE_VISUAL_ENDPOINT'].rsplit(':', 1)
        with socket.create_connection((address, int(port)), timeout=4) as connection:
            connection.settimeout(4)
            request = {
                'session': int(os.environ['MONOCODE_VISUAL_SESSION']),
                'token': os.environ['MONOCODE_VISUAL_TOKEN'],
                    'request_id': 'runtime-integration-request-' + str(os.getpid()),
                'document': {
                    'id': 'runtime-native', 'title': 'Native result',
                    'source': 'return () => visual.text(' + json.dumps('a' * 3000) + ');'
                }
            }
            connection.sendall((json.dumps(request) + '\n').encode())
            # No stdout events are emitted while awaiting publication. Without
            # the provider heartbeat, neither the save nor this reply can occur.
            response = json.loads(connection.makefile('rb').readline())
            assert response['ok'] is True, response
            assert response['result']['saved'] is True, response
            with open('acknowledged.json', 'w') as output:
                json.dump(response, output)
        deadline = time.monotonic() + 4
        while not os.path.exists('saved-observed'):
            assert time.monotonic() < deadline, 'Live visual update never arrived'
            time.sleep(0.01)
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'after', 'delta': 'After the visual.'}})
        send({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
        break
"#).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_monocode(&fixture.0.join("sessions.db")).unwrap();
    let summary = store.create("codex", fixture.0.to_str().unwrap()).unwrap();
    let session = summary.id;
    let store = Arc::new(Mutex::new(store));
    let worker_store = store.clone();
    let (updates, receive) = async_channel::bounded(64);
    let (_controls, control_rx) = std::sync::mpsc::channel();
    let visible_prompt = prompt.clone();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt,
                settings: Settings {
                    prepared_prompt,
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
    let mut live_visual = false;
    let final_status = loop {
        let update = receive
            .recv_blocking()
            .expect("Runtime closed before its final update");
        if !update.finished && update.rows.iter().any(|row| row.role == "visual") {
            let saved = locked(&store).unwrap().load(session).unwrap();
            let visual_rows = saved
                .rows
                .iter()
                .filter(|row| row.role == "visual")
                .collect::<Vec<_>>();
            assert!(
                visual_rows.len() >= 2,
                "The large visual must use bounded transcript chunks"
            );
            assert!(
                visual_rows
                    .iter()
                    .all(|row| row.text.len() <= monocode_core::transcript::ROW_BYTES)
            );
            let text = visual_rows
                .iter()
                .map(|row| row.text.as_str())
                .collect::<String>();
            let document: monocode_core::visual::VisualDocument =
                serde_json::from_str(&text).unwrap();
            assert_eq!(document.id, "runtime-native");
            live_visual = true;
            std::fs::write(fixture.0.join("saved-observed"), "saved").unwrap();
        }
        if update.finished {
            break update.status;
        }
    };
    worker.join().unwrap();
    assert_eq!(final_status, "Ready");
    assert!(live_visual, "The visual must arrive before the turn ends");
    let acknowledgment: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("acknowledged.json")).unwrap())
            .unwrap();
    assert_eq!(acknowledgment["result"]["saved"], true);
    let saved = locked(&store).unwrap().load(session).unwrap();
    assert_eq!(
        saved
            .rows
            .iter()
            .filter(|row| row.is_prompt())
            .map(|row| row.text.as_str())
            .collect::<String>(),
        visible_prompt
    );
    let before = saved
        .rows
        .iter()
        .position(|row| row.text.contains("Before the visual."))
        .unwrap();
    let visual = saved
        .rows
        .iter()
        .position(|row| row.role == "visual")
        .unwrap();
    let after = saved
        .rows
        .iter()
        .position(|row| row.text.contains("After the visual."))
        .unwrap();
    assert!(before < visual && visual < after);
    let settings = locked(&store)
        .unwrap()
        .agent_settings(session)
        .unwrap()
        .unwrap();
    assert!(
        settings.environment.is_empty(),
        "Transport capabilities must not be persisted"
    );
}
