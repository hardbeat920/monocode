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

#[test]
fn mono_goal_keeps_one_delivery_and_the_final_continuation_output() {
    run_mono_goal("complete", None);
}

#[test]
fn mono_goal_settles_idle_without_losing_a_racing_continuation() {
    run_mono_goal("idle", None);
    run_mono_goal("racing-read", None);
}

#[test]
fn mono_goal_stop_requires_a_confirmed_pause_between_turns() {
    for (mode, error) in [
        ("paused", None),
        ("in-flight", None),
        ("turn-rejected", Some("interrupt rejected")),
        ("ack-only", Some("timed out")),
        ("rpc-error", Some("pause rejected")),
        ("timeout", Some("timed out")),
        ("disconnect", Some("disconnected")),
        ("write-error", Some("Broken pipe")),
        ("wrong-thread", Some("for this thread")),
        ("still-active", Some("paused goal")),
    ] {
        run_mono_goal(mode, error);
    }
}

fn run_mono_goal(mode: &str, error: Option<&str>) {
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "monocode-runtime-goal-{}",
        monocode_core::mono::new_id()
    )));
    std::fs::create_dir_all(&fixture.0).unwrap();
    let binary = fixture.0.join("codex");
    std::fs::write(fixture.0.join("mode"), mode).unwrap();
    std::fs::write(&binary, r#"#!/usr/bin/env python3
import json, os, signal, sys, time
signal.alarm(20)
mode = open("mode").read()
def send(frame):
    print(json.dumps(frame), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    if method == 'initialize':
        send({'id': request['id'], 'result': {}})
    elif method == 'thread/start':
        send({'id': request['id'], 'result': {'thread': {'id': 'main'}}})
    elif method == 'thread/read':
        assert mode in ('idle','racing-read') and request['params'] == {'threadId':'main'}
        if mode == 'racing-read':
            send({'method':'turn/started','params':{'threadId':'main','turn':{'id':'second'}}})
        send({'id':request['id'],'result':{'thread':{'id':'main','status':{'type':'idle'}}}})
        if mode == 'racing-read':
            send({'method':'thread/goal/updated','params':{'threadId':'main','goal':{'status':'complete','objective':'fix build'}}})
            send({'method':'item/agentMessage/delta','params':{'threadId':'main','turnId':'second','itemId':'tail','delta':'second'}})
            send({'method':'turn/completed','params':{'threadId':'main','turn':{'id':'second','status':'completed'}}})
        break
    elif method == 'turn/interrupt':
        assert request['params'] == {'threadId':'main','turnId':'continuation'}
        if mode == 'turn-rejected':
            send({'id':request['id'],'error':{'message':'interrupt rejected'}})
        else:
            send({'id':request['id'],'result':{}})
            send({'method':'turn/completed','params':{'threadId':'main','turn':{'id':'wrong','status':'completed'}}})
            send({'method':'item/agentMessage/delta','params':{'threadId':'main','turnId':'continuation','itemId':'tail','delta':'post-ack output'}})
            send({'method':'item/completed','params':{'threadId':'main','turnId':'continuation','item':{'id':'late-tool','type':'commandExecution','command':'pwd','status':'completed','aggregatedOutput':'post-ack tool'}}})
            if mode == 'ack-only':
                time.sleep(10)
            send({'method':'turn/completed','params':{'threadId':'main','turn':{'id':'continuation','status':'interrupted'}}})
        break
    elif method == 'thread/goal/set':
        if request['params'].get('status') == 'paused':
            assert request['params'] == {'origin':'user','status':'paused','threadId':'main'}
            reply = {'id':request['id'],'result':{'goal':{'threadId':'main','status':'paused'}}}
            if mode == 'rpc-error':
                reply = {'id':request['id'],'error':{'message':'pause rejected'}}
            elif mode == 'disconnect':
                break
            elif mode == 'timeout':
                send({**reply, 'id':'unrelated'})
                time.sleep(10)
            elif mode == 'wrong-thread':
                reply['result']['goal']['threadId'] = 'another'
            elif mode == 'still-active':
                reply['result']['goal']['status'] = 'active'
            if mode in ('in-flight', 'turn-rejected', 'ack-only'):
                send({'method':'turn/started','params':{'threadId':'main','turn':{'id':'continuation'}}})
                send(reply)
                continue
            send(reply)
            break
        assert request['params']['objective'] == 'fix build'
        goal = {'threadId':'main','objective':'fix build','status':'active'}
        send({'id': request['id'], 'result': {'goal': goal}})
        if mode == 'write-error':
            os.close(0)
        for turn in (['first', 'second'] if mode == 'complete' else ['first']):
            send({'method':'turn/started','params':{'threadId':'main','turn':{'id':turn}}})
            if turn == 'second':
                goal['status'] = 'complete'
                send({'method':'thread/goal/updated','params':{'threadId':'main','goal':goal}})
            send({'method':'item/agentMessage/delta','params':{'itemId':turn,'delta':turn}})
            send({'method':'turn/completed','params':{'threadId':'main','turn':{'id':turn,'status':'completed'}}})
        if mode == 'complete':
            break
        # A post-completion usage update tells the test the first turn has drained.
        send({'method':'thread/tokenUsage/updated','params':{'tokenUsage':{'last':{'totalTokens':123},'modelContextWindow':1024}}})
        if mode == 'write-error':
            time.sleep(10)
    else:
        assert method == 'initialized', method
"#).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open_monocode(&fixture.0.join("sessions.db")).unwrap();
    let summary = store.create("codex", fixture.0.to_str().unwrap()).unwrap();
    let session = summary.id;
    let store = Arc::new(Mutex::new(store));
    let (updates, received) = async_channel::unbounded();
    let (_controls, control_rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let worker_store = store.clone();
    let root = fixture.0.clone();
    let worker = std::thread::spawn(move || {
        run_turn_using(
            TurnInput {
                summary,
                prompt: "/goal fix build".into(),
                settings: Settings {
                    mono: Some(monocode_core::agent::MonoContext {
                        mono: Default::default(),
                        root,
                        executable: binary.clone(),
                    }),
                    mono_message_id: Some("goal-message".into()),
                    prepared_prompt: Some("expanded skill".into()),
                    ..Default::default()
                },
            },
            control_rx,
            worker_store,
            cancel,
            updates,
            |_| Some(binary),
        )
    });
    let mut final_update = None;
    while let Ok(update) = received.recv_blocking() {
        if !matches!(mode, "complete" | "idle" | "racing-read")
            && update.context.is_some_and(|usage| usage.used == 123)
        {
            stop.store(true, Ordering::Relaxed);
        }
        final_update = Some(update);
    }
    worker.join().unwrap();
    let finished = final_update.unwrap();
    assert!(finished.finished);
    assert_eq!(
        finished.goal_pause_unconfirmed,
        error.is_some(),
        "{mode}: {}",
        finished.status
    );
    if let Some(error) = error {
        assert!(
            finished.status.contains(error),
            "{mode}: {}",
            finished.status
        );
    } else {
        assert_eq!(finished.status, "Ready");
    }
    assert_eq!(finished.delivered.as_deref(), Some("goal-message"));
    let reopened = Store::open_monocode(&fixture.0.join("sessions.db")).unwrap();
    let mut delivered = monocode_core::mono::Message {
        id: "goal-message".into(),
        text: "/goal fix build".into(),
        internal: false,
        sent_at: None,
        attachments: Vec::new(),
        error: None,
    };
    assert!(delivered.reconcile_delivery(&reopened, session).unwrap());
    let saved = locked(&store).unwrap().load(session).unwrap();
    assert_eq!(saved.rows.iter().filter(|row| row.is_prompt()).count(), 1);
    assert!(saved.rows.iter().any(|row| row.text == "first"));
    if matches!(mode, "in-flight" | "ack-only") {
        assert!(saved.rows.iter().any(|r| r.text == "post-ack output"));
        assert!(
            saved
                .rows
                .iter()
                .any(|r| r.role == "tool" && r.text.contains("post-ack tool"))
        );
    }
    if mode == "idle" {
        assert!(saved.rows.last().unwrap().text.contains("still active"));
        assert_eq!(finished.status, "Ready");
    }
    if error.is_some() {
        let last = saved.rows.last().unwrap();
        assert_eq!(last.role, "error");
        assert_eq!(last.text, finished.status);
    } else if matches!(mode, "complete" | "racing-read") {
        assert_eq!(saved.rows.last().unwrap().text, "second");
    }
    let settings = locked(&store)
        .unwrap()
        .agent_settings(session)
        .unwrap()
        .unwrap();
    assert_eq!(
        settings.turns.values().next().unwrap().stopped,
        matches!(mode, "paused" | "in-flight")
    );
}

#[test]
fn mono_goal_restart_before_provider_request_preserves_the_command_for_review() {
    use monocode_core::mono::{Message as OutboxMessage, Mono, Roster};
    const CHILD: &str = "MONOCODE_TEST_GOAL_CRASH_DIR";
    if let Some(dir) = std::env::var_os(CHILD) {
        let root = PathBuf::from(dir);
        let store = Store::open_monocode(&root.join("sessions.db")).unwrap();
        let summary = store.sessions().unwrap().remove(0);
        let mono = Roster::load(&store).unwrap().monos.remove(0);
        let message = &mono.outbox[0];
        let (tx, _rx) = async_channel::unbounded();
        let (_controls, control_rx) = std::sync::mpsc::channel();
        run_turn_using(
            TurnInput {
                summary,
                prompt: message.text.clone(),
                settings: Settings {
                    mono_message_id: Some(message.id.clone()),
                    mono: Some(monocode_core::agent::MonoContext {
                        mono,
                        root,
                        executable: "unused".into(),
                    }),
                    ..Default::default()
                },
            },
            control_rx,
            Arc::new(Mutex::new(store)),
            Arc::new(AtomicBool::new(false)),
            tx,
            |_| std::process::exit(73),
        );
        panic!("never reached provider resolution");
    }
    for command in ["/goal repair build", "/goal pause"] {
        let dir = Fixture(std::env::temp_dir().join(monocode_core::mono::new_id()));
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("sessions.db");
        let store = Store::open_monocode(&path).unwrap();
        let session = store.create("codex", dir.0.to_str().unwrap()).unwrap().id;
        let message = OutboxMessage {
            id: "operation".into(),
            text: command.into(),
            internal: false,
            sent_at: None,
            attachments: Vec::new(),
            error: None,
        };
        let later = OutboxMessage {
            id: "later".into(),
            text: "next request".into(),
            ..message.clone()
        };
        let mut roster = Roster::default();
        roster.monos.push(Mono {
            id: "test".into(),
            session_id: Some(session),
            outbox: vec![message, later],
            ..Default::default()
        });
        roster.save(&store).unwrap();
        drop(store);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","runtime::follow_up_tests::mono_goal_restart_before_provider_request_preserves_the_command_for_review"])
            .env(CHILD,&dir.0).status().unwrap();
        assert_eq!(status.code(), Some(73));
        let reopened = Store::open_monocode(&path).unwrap();
        assert!(
            reopened
                .load(session)
                .unwrap()
                .rows
                .iter()
                .any(|row| row.text == command)
        );
        let mut recovered = Roster::load(&reopened).unwrap().monos.remove(0).outbox;
        assert!(!recovered[0].reconcile_delivery(&reopened, session).unwrap());
        assert_eq!(recovered[0].text, command);
        assert!(recovered[0].error.as_ref().unwrap().contains("interrupted"));
        assert!(!recovered[1].reconcile_delivery(&reopened, session).unwrap());
        assert!(recovered[1].error.is_none());
    }
}
