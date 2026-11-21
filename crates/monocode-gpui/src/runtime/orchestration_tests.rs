use super::*;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn settled_proposal(status: &str, cancelled: bool) -> Update {
    let fixture = Fixture(std::env::temp_dir().join(monocode_core::mono::new_id()));
    std::fs::create_dir(&fixture.0).unwrap();
    let binary = fixture.0.join("mock-provider");
    std::fs::write(
        &binary,
        r#"#!/usr/bin/env python3
import json, os, sys

def send(value):
    print(json.dumps(value), flush=True)

def event(method, params):
    send({'method': method, 'params': params})

for line in sys.stdin:
    value = json.loads(line)
    method = value.get('method')
    if method == 'initialize':
        send({'id': value['id'], 'result': {}})
    elif method == 'thread/start':
        send({'id': value['id'], 'result': {'thread': {'id': 'thread'}, 'model': 'fixture'}})
    elif method == 'turn/start':
        assert value['params']['collaborationMode']['mode'] == 'plan'
        assert value['params']['sandboxPolicy']['type'] == 'readOnly'
        send({'id': value['id'], 'result': {'turn': {'id': 'turn'}}})
        proposal = {
            'title': 'Verify and clean up',
            'summary': 'Investigate first, then fix',
            'tasks': [
                {'id': 'audit', 'title': 'Trace outcomes', 'prompt': 'Trace outcomes. ' * 210, 'files': ['data']},
                {'id': 'fix', 'title': 'Fix confirmed issues', 'prompt': 'Fix the confirmed issues', 'files': ['src'], 'depends_on': ['audit']},
            ],
        }
        text = '<monocode_orchestration>' + json.dumps(proposal) + '</monocode_orchestration>'
        event('item/agentMessage/delta', {'itemId': 'answer', 'delta': text[:2048]})
        event('item/plan/delta', {'itemId': 'plan', 'delta': text[:2048]})
        event('item/completed', {'item': {'type': 'plan', 'id': 'plan', 'text': text}})
        event('turn/completed', {'turn': {
            'status': os.environ['MONOCODE_FIXTURE_PLAN_STATUS'],
            'error': {'message': 'fixture provider diagnostic'},
        }})
        break
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_monocode(&fixture.0.join("sessions.db")).unwrap();
    let summary = store.create("codex", fixture.0.to_str().unwrap()).unwrap();
    let store = Arc::new(Mutex::new(store));
    let (updates, receive) = async_channel::bounded(64);
    let (_controls, control_rx) = std::sync::mpsc::channel();
    let environment = [("MONOCODE_FIXTURE_PLAN_STATUS".into(), status.into())].into();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt: "Prepare a plan".into(),
                settings: Settings {
                    text_only: true,
                    orchestrator: true,
                    plan: true,
                    environment,
                    ..Default::default()
                },
            },
            control_rx,
            store,
            Arc::new(AtomicBool::new(cancelled)),
            updates,
            |_| Some(binary),
        );
    });
    let settled = loop {
        let update = receive.recv_blocking().expect("Missing final update");
        if update.finished {
            break update;
        }
        assert!(update.orchestration.is_none());
    };
    worker.join().unwrap();
    settled
}

#[test]
fn a_completed_codex_plan_supplies_all_assignments_after_partial_assistant_text() {
    let settled = settled_proposal("completed", false);
    assert_eq!(settled.status, "Ready");
    let plan = settled.orchestration.unwrap().unwrap();
    assert_eq!(plan.tasks.len(), 2);
    assert!(plan.tasks[0].prompt.len() > monocode_core::transcript::ROW_BYTES);
    assert_eq!(plan.tasks[1].title, "Fix confirmed issues");
    assert_eq!(plan.tasks[1].depends_on, ["audit"]);
}

#[test]
fn unsuccessful_orchestration_proposals_only_offer_retry_after_provider_failure() {
    for status in ["failed", "interrupted"] {
        let settled = settled_proposal(status, false);
        assert_ne!(settled.status, "Ready");
        let error = settled.orchestration.unwrap().unwrap_err();
        assert!(error.contains("Retry plan"));
        assert!(!error.contains("fixture provider diagnostic"));
    }
    assert!(settled_proposal("completed", true).orchestration.is_none());
}
