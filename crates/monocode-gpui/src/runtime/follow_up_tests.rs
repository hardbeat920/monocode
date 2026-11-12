use super::*;
use monocode_core::{agent::Decision, follow_up::Message};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn accepted_steer_publishes_its_working_receipt_before_provider_output() {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "monocode-runtime-steer-{}-{}",
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
    method = value.get('method')
    if method == 'initialize':
        send({'id': value['id'], 'result': {}})
    elif method == 'thread/start':
        send({'id': value['id'], 'result': {'thread': {'id': 'thread'}}})
    elif method == 'turn/start':
        send({'id': value['id'], 'result': {'turn': {'id': 'turn'}}})
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'before', 'delta': 'Before'}})
    elif method == 'turn/steer':
        send({'id': value['id'], 'result': {}})
        # Keep the provider silent until the UI has received the receipt.
        deadline = time.monotonic() + 4
        while not os.path.exists('receipt-observed'):
            assert time.monotonic() < deadline, 'Working receipt never arrived'
            time.sleep(0.01)
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'after', 'delta': 'After'}})
        send({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
        break
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_monocode(&fixture.0.join("sessions.db")).unwrap();
    let summary = store.create("codex", fixture.0.to_str().unwrap()).unwrap();
    let session = summary.id;
    let message = Message {
        id: "steer".into(),
        text: "Change direction\nwith more detail".into(),
        sent_at: Some(1_800_000_000_000),
        ..Default::default()
    };
    store
        .save_follow_ups(
            session,
            &monocode_core::follow_up::State {
                messages: vec![message.clone()],
                ..Default::default()
            },
        )
        .unwrap();
    let store = Arc::new(Mutex::new(store));
    let worker_store = store.clone();
    let (updates, receive) = async_channel::bounded(64);
    let (controls, control_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt: "Start working".into(),
                settings: Settings {
                    text_only: true,
                    turn_model: Some(monocode_core::agent::TurnInfo {
                        provider: "codex".into(),
                        model: "Chosen Model".into(),
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
    let mut receipt_seen = false;
    let final_status = loop {
        let update = receive
            .recv_blocking()
            .expect("Runtime closed before its final update");
        if update.turn_ready {
            controls
                .send(Control {
                    id: message.id.clone(),
                    decision: Decision::Steer(message.clone()),
                })
                .unwrap();
        }
        if !update.finished && update.follow_up_delivered.contains(&message.id) {
            let prompt = update.rows.iter().position(Row::is_prompt).unwrap();
            let response = update.rows.last().unwrap();
            assert_eq!(response.role, "assistant");
            assert!(response.first && response.text.is_empty());
            assert!(prompt < update.rows.len() - 1);
            assert_eq!(update.rows[prompt].sent_at, message.sent_at);
            let store = locked(&store).unwrap();
            assert!(store.follow_ups(session).unwrap().messages.is_empty());
            let saved = store.load(session).unwrap();
            assert_eq!(saved.rows.last().unwrap(), response);
            let turn = saved.rows.len() - 1;
            let settings = store.agent_settings(session).unwrap().unwrap();
            assert_eq!(settings.turns[&turn].provider, "codex");
            assert_eq!(settings.turns[&turn].model, "Chosen Model");
            assert!(!settings.turns[&turn].has_duration());
            assert_eq!(
                settings.turns[&turn].joined_turn,
                Some(1),
                "the accepted steer keeps its original run after reload"
            );
            receipt_seen = true;
            std::fs::write(fixture.0.join("receipt-observed"), "ready").unwrap();
        }
        if update.finished {
            break update.status;
        }
    };
    worker.join().unwrap();
    assert_eq!(final_status, "Ready");
    assert!(receipt_seen);
    let saved = locked(&store).unwrap().load(session).unwrap();
    let after = saved.rows.iter().find(|row| row.text == "After").unwrap();
    assert_eq!(after.role, "assistant");
    assert_eq!(saved.rows.last(), Some(after));
}

/// Makes every transcript write to `db` fail the way a full disk does, until
/// `room` runs.
struct DiskFull(rusqlite::Connection);
impl DiskFull {
    fn new(db: &std::path::Path) -> Self {
        let conn = rusqlite::Connection::open(db).unwrap();
        conn.execute_batch("CREATE TABLE test_disk_full(full INTEGER);")
            .unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('sessions','mono_blocks')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for table in tables {
            for event in ["INSERT", "UPDATE", "DELETE"] {
                conn.execute_batch(&format!(
                    "CREATE TRIGGER test_full_{table}_{event} BEFORE {event} ON {table}
                     WHEN EXISTS(SELECT 1 FROM test_disk_full)
                     BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;"
                ))
                .unwrap();
            }
        }
        Self(conn)
    }
    fn fill(&self) {
        self.0
            .execute("INSERT INTO test_disk_full VALUES(1)", [])
            .unwrap();
    }
    fn room(&self) {
        self.0.execute("DELETE FROM test_disk_full", []).unwrap();
    }
}

#[test]
fn a_reply_the_store_refused_is_saved_once_there_is_room() {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "monocode-runtime-full-{}-{}",
        std::process::id(),
        monocode_core::mono::new_id()
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
    method = value.get('method')
    if method == 'initialize':
        send({'id': value['id'], 'result': {}})
    elif method == 'thread/start':
        send({'id': value['id'], 'result': {'thread': {'id': 'thread'}}})
    elif method == 'turn/start':
        send({'id': value['id'], 'result': {'turn': {'id': 'turn'}}})
        # Answer only once the disk has filled up.
        deadline = time.monotonic() + 4
        while not os.path.exists('disk-full'):
            assert time.monotonic() < deadline, 'The disk never filled'
            time.sleep(0.01)
        send({'method': 'item/agentMessage/delta', 'params': {'itemId': 'answer', 'delta': 'The whole answer'}})
        send({'method': 'turn/completed', 'params': {'turn': {'status': 'completed'}}})
        break
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = fixture.0.join("sessions.db");
    let store = Store::open_monocode(&path).unwrap();
    let summary = store.create("codex", fixture.0.to_str().unwrap()).unwrap();
    let session = summary.id;
    let store = Arc::new(Mutex::new(store));
    let disk = DiskFull::new(&path);
    let turn = |summary: Session, prompt: &str, binary: Option<PathBuf>| {
        let (updates, receive) = async_channel::bounded(64);
        let (_controls, control_rx) = std::sync::mpsc::channel();
        let worker_store = store.clone();
        let prompt = prompt.to_owned();
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
                move |_| binary,
            );
        });
        (worker, receive)
    };
    let (worker, receive) = turn(summary.clone(), "Answer me", Some(binary));
    let mut filled = false;
    let last = loop {
        let update = receive.recv_blocking().expect("no final update");
        if !filled && update.turn_ready {
            // The prompt is saved; everything after it is refused.
            disk.fill();
            std::fs::write(fixture.0.join("disk-full"), "").unwrap();
            filled = true;
        }
        if update.finished {
            break update;
        }
    };
    worker.join().unwrap();
    assert!(
        last.status.contains("Saving failed") && last.status.contains("The reply is kept"),
        "{}",
        last.status
    );
    assert!(
        last.rows.iter().any(|row| row.text == "The whole answer"),
        "{} {:?}",
        last.status,
        last.rows
    );
    let saved = || locked(&store).unwrap().load(session).unwrap().rows;
    let before = saved();
    assert!(!before.iter().any(|row| row.text == "The whole answer"));
    assert_eq!(before[0].text, "Answer me", "the prompt was saved");
    let mut shown = before.clone();
    show_kept(&store, session, &mut shown);
    assert!(
        shown.iter().any(|row| row.text == "The whole answer"),
        "reopening the conversation still shows the reply"
    );
    let journal = fixture
        .0
        .join("sessions.db.gpui-unsaved")
        .join(format!("{session}.json"));
    assert!(journal.exists(), "the reply survives a restart");

    // The next turn can't start from the older history while it's full.
    let (worker, receive) = turn(summary.clone(), "Next", None);
    let next = receive.recv_blocking().unwrap();
    worker.join().unwrap();
    assert!(next.finished && next.rows.is_empty());
    assert!(
        next.status.contains("last reply isn't saved yet"),
        "{}",
        next.status
    );
    disk.room();
    assert_eq!(saved(), before, "the refused turn wrote nothing");

    persistence::save_kept(&store, session).unwrap();
    let after = saved();
    assert!(after.iter().any(|row| row.text == "The whole answer"));
    assert_eq!(after[..], shown[..]);
    assert!(!journal.exists());
}
