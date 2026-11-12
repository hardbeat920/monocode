use super::*;
use std::os::unix::fs::PermissionsExt;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (Fixture, std::path::PathBuf, std::path::PathBuf) {
    let dir = Fixture(std::env::temp_dir().join(format!(
            "monocode-folders-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(dir.path()).unwrap();
    let binary = dir.path().join("provider");
    let log = dir.path().join("frames.jsonl");
    std::fs::write(&binary, format!(r#"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    v = json.loads(line)
    with open({log:?}, 'a') as f: f.write(json.dumps(v) + '\n')
    if v.get('id') == 1: print(json.dumps({{'id':1,'result':{{'userAgent':'codex/0.167.1'}}}}), flush=True)
    if v.get('id') == 2: print(json.dumps({{'id':2,'result':{{'thread':{{'id':'same-thread'}}}}}}), flush=True)
    if v.get('method') == 'turn/start':
        print(json.dumps({{'method':'item/agentMessage/delta','params':{{'itemId':'answer','delta':'Done'}}}}), flush=True)
        print(json.dumps({{'method':'turn/completed','params':{{'turn':{{'status':'completed'}}}}}}), flush=True)
        break
"#, log=log.to_str().unwrap())).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    (dir, binary, log)
}

#[test]
fn next_turn_reads_project_store_and_resumes_same_thread_after_removal() {
    let (dir, binary, log) = fixture();
    let extra = dir.path().join("extra");
    let worktree = dir.path().join("worktree");
    std::fs::create_dir(&extra).unwrap();
    std::fs::create_dir(&worktree).unwrap();
    let store = Store::open(&dir.path().join("store.db")).unwrap();
    let mut summary = store.create("codex", dir.path().to_str().unwrap()).unwrap();
    let folders = store.add_project_folder(&summary.cwd, &extra).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut settings = Settings {
        access: monocode_core::agent::AccessMode::AutoAcceptEdits,
        worktree_cwd: Some(worktree.to_str().unwrap().into()),
        ..Default::default()
    };
    for turn in 0..2 {
        if turn == 1 {
            locked(&store)
                .unwrap()
                .remove_project_folder(&summary.cwd, &folders.additional[0])
                .unwrap();
            settings.additional_folders = Some(folders.additional.clone());
            summary.provider_id = Some("same-thread".into());
        }
        let (tx, rx) = async_channel::bounded(64);
        let (_control, control_rx) = std::sync::mpsc::channel();
        run_turn_using(
            TurnInput {
                summary: summary.clone(),
                prompt: "Inspect".into(),
                settings: settings.clone(),
            },
            control_rx,
            store.clone(),
            Arc::new(AtomicBool::new(false)),
            tx,
            |_| Some(binary.clone()),
        );
        let mut updates = Vec::new();
        while let Ok(update) = rx.try_recv() {
            updates.push(update);
        }
        let final_update = updates.last().unwrap();
        assert!(final_update.finished);
        assert!(
            !final_update.status.contains("Error"),
            "{}",
            final_update.status
        );
    }
    let frames: Vec<serde_json::Value> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let starts: Vec<_> = frames
        .iter()
        .filter(|v| matches!(v["method"].as_str(), Some("thread/start" | "thread/resume")))
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["params"]["cwd"], worktree.to_str().unwrap());
    assert_eq!(starts[1]["method"], "thread/resume");
    assert_eq!(starts[1]["params"]["threadId"], "same-thread");
    assert_eq!(
        starts[1]["params"]["runtimeWorkspaceRoots"],
        serde_json::json!([worktree])
    );
    let turns: Vec<_> = frames
        .iter()
        .filter(|v| v["method"] == "turn/start")
        .collect();
    assert_eq!(
        turns[0]["params"]["sandboxPolicy"]["writableRoots"],
        serde_json::json!([worktree, folders.additional[0]])
    );
    assert_eq!(
        turns[1]["params"]["sandboxPolicy"]["writableRoots"],
        serde_json::json!([worktree])
    );
    let persisted = locked(&store)
        .unwrap()
        .agent_settings(summary.id)
        .unwrap()
        .unwrap();
    assert!(persisted.additional_folders.is_none());
}

fn assert_turns_keep_folder_snapshot(initial: Option<&str>, updated: Option<&str>) {
    let (dir, binary, log) = fixture();
    let worktree = dir.path().join("worktree");
    let original = dir.path().join("original");
    let added = dir.path().join("added");
    for path in [&worktree, &original, &added] {
        std::fs::create_dir(path).unwrap();
    }
    let store = Store::open_monocode(&dir.path().join("store.db")).unwrap();
    let mut summary = store.create("codex", dir.path().to_str().unwrap()).unwrap();
    if let Some(initial) = initial {
        store
            .add_project_folder(&summary.cwd, &dir.path().join(initial))
            .unwrap();
    }
    let store = Arc::new(Mutex::new(store));
    let settings = Settings {
        access: monocode_core::agent::AccessMode::AutoAcceptEdits,
        worktree_cwd: Some(worktree.to_str().unwrap().into()),
        ..Default::default()
    };
    let mut statuses = Vec::new();
    for turn in 0..2 {
        let (tx, rx) = async_channel::bounded(64);
        let (_control, control_rx) = std::sync::mpsc::channel();
        run_turn_using(
            TurnInput {
                summary: summary.clone(),
                prompt: "Inspect".into(),
                settings: settings.clone(),
            },
            control_rx,
            store.clone(),
            Arc::new(AtomicBool::new(false)),
            tx,
            |_| {
                if turn == 0 {
                    let store = locked(&store).unwrap();
                    if let Some(updated) = updated {
                        store
                            .add_project_folder(&summary.cwd, &dir.path().join(updated))
                            .unwrap();
                    } else if let Some(initial) = initial {
                        let initial = dir.path().join(initial).canonicalize().unwrap();
                        store.remove_project_folder(&summary.cwd, &initial).unwrap();
                    }
                }
                Some(binary.clone())
            },
        );
        let updates: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(updates.last().unwrap().finished);
        statuses.push(updates.last().unwrap().status.clone());
        summary.provider_id = Some("same-thread".into());
    }
    let frames: Vec<serde_json::Value> = std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let turns: Vec<_> = frames
        .iter()
        .filter(|v| v["method"] == "turn/start")
        .collect();
    assert_eq!(turns.len(), 2, "{statuses:?}");
    let mut first = vec![worktree.clone()];
    if let Some(initial) = initial {
        first.push(dir.path().join(initial).canonicalize().unwrap());
    }
    let first = if initial.is_some() {
        serde_json::json!(first)
    } else {
        serde_json::Value::Null
    };
    let mut next = vec![worktree];
    if let Some(updated) = updated {
        if let Some(initial) = initial {
            next.push(dir.path().join(initial).canonicalize().unwrap());
        }
        next.push(dir.path().join(updated).canonicalize().unwrap());
    }
    let expected = vec![first, serde_json::json!(next)];
    let writable: Vec<_> = turns
        .iter()
        .map(|v| v["params"]["sandboxPolicy"]["writableRoots"].clone())
        .collect();
    assert_eq!(writable, expected);
    let runtime: Vec<_> = turns
        .iter()
        .map(|v| v["params"]["runtimeWorkspaceRoots"].clone())
        .collect();
    assert_eq!(runtime, expected);
}

#[test]
fn folder_added_while_connecting_applies_to_next_turn() {
    assert_turns_keep_folder_snapshot(Some("original"), Some("added"));
}

#[test]
fn folder_removed_while_connecting_applies_to_next_turn() {
    assert_turns_keep_folder_snapshot(Some("original"), None);
}

#[test]
fn first_folder_added_while_connecting_applies_to_next_turn() {
    assert_turns_keep_folder_snapshot(None, Some("added"));
}

#[test]
fn additional_folder_matching_worktree_executes_with_one_writable_root() {
    let (dir, binary, log) = fixture();
    let worktree = dir.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let worktree = worktree.canonicalize().unwrap();
    let store = Store::open_monocode(&dir.path().join("store.db")).unwrap();
    let summary = store.create("codex", dir.path().to_str().unwrap()).unwrap();
    store.add_project_folder(&summary.cwd, &worktree).unwrap();
    let store = Arc::new(Mutex::new(store));
    let settings = Settings {
        access: monocode_core::agent::AccessMode::AutoAcceptEdits,
        worktree_cwd: Some(worktree.to_str().unwrap().into()),
        ..Default::default()
    };
    let (tx, rx) = async_channel::bounded(64);
    let (_control, control_rx) = std::sync::mpsc::channel();
    run_turn_using(
        TurnInput {
            summary,
            prompt: "Inspect".into(),
            settings,
        },
        control_rx,
        store,
        Arc::new(AtomicBool::new(false)),
        tx,
        |_| Some(binary),
    );
    let updates: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let status = &updates.last().unwrap().status;
    assert!(updates.last().unwrap().finished);
    let frames: Vec<serde_json::Value> = std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let turns: Vec<_> = frames
        .iter()
        .filter(|v| v["method"] == "turn/start")
        .collect();
    assert_eq!(turns.len(), 1, "{status}");
    assert_eq!(
        turns[0]["params"]["sandboxPolicy"]["writableRoots"],
        serde_json::json!([worktree])
    );
    assert_eq!(
        turns[0]["params"]["runtimeWorkspaceRoots"],
        serde_json::json!([worktree])
    );
}
