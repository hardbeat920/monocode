use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture(mode: &str) -> (tempfile::TempDir, Request) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider");
    let script = format!(
        r#"#!/usr/bin/env python3
import json, os, sys, time
mode = {mode:?}
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    value = json.loads(line)
    if value.get('id') == 1:
        send({{'id':1,'result':{{}}}})
    elif value.get('id') == 2:
        assert value['method'] == 'thread/start'
        assert value['params']['ephemeral'] is True
        assert value['params']['sandbox'] == 'read-only'
        assert value['params']['approvalPolicy'] == 'never'
        assert 'Do not call tools' in value['params']['developerInstructions']
        assert os.getenv('MONOCODE_TEST_CAPABILITY') is None
        send({{'id':2,'result':{{'thread':{{'id':'fresh-text-thread'}}}}}})
    elif value.get('id') == 3:
        assert value['params']['threadId'] == 'fresh-text-thread'
        assert len(value['params']['input']) == 1
        if mode == 'cancel':
            time.sleep(10)
        elif mode == 'tool':
            send({{'method':'item/started','params':{{'item':{{'type':'commandExecution','id':'tool','command':'never execute'}}}}}})
        else:
            send({{'method':'item/agentMessage/delta','params':{{'itemId':'answer','delta':'{{"title":"Improve session filters","workItem":null}}'}}}})
            send({{'method':'turn/completed','params':{{'turn':{{'status':'completed'}}}}}})
"#
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let request = Request {
        kind: Kind::Codex,
        binary: path,
        cwd: dir.path().into(),
        prompt: "Generate a session title".into(),
        provider_id: Some("user-thread-must-not-be-used".into()),
        model: None,
        settings: Settings {
            access: AccessMode::FullAccess,
            environment: [("MONOCODE_TEST_CAPABILITY".into(), "private".into())].into(),
            attachments: vec![dir.path().join("private-file")],
            ..Default::default()
        },
    };
    (dir, request)
}

#[test]
fn background_title_uses_an_isolated_read_only_thread_and_strips_capabilities() {
    let (_dir, request) = fixture("title");
    let output = run_text(request, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(
        crate::session_title::parse(&output, "").unwrap().title,
        "Improve session filters"
    );
}

#[test]
fn background_titles_reject_tools_and_reap_on_cancellation() {
    let (_dir, request) = fixture("tool");
    assert!(run_text(request, Arc::new(AtomicBool::new(false)))
        .unwrap_err()
        .contains("attempted to use tools"));
    let (_dir, request) = fixture("cancel");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let task = thread::spawn(move || run_text(request, flag));
    thread::sleep(Duration::from_millis(150));
    cancel.store(true, Ordering::Relaxed);
    assert!(task.join().unwrap().unwrap_err().contains("Stopped"));
}
