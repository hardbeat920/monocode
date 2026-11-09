use super::*;
use crate::{follow_up::State, notes::NoteUpsert};
use std::path::Path;
use std::time::Duration;

fn model() -> Model {
    Model {
        id: "test-model".into(),
        name: "Test model".into(),
        alias_target: None,
        efforts: vec!["low".into(), "high".into()],
        default_effort: Some("low".into()),
        fast: true,
        thinking: false,
        default: true,
        tiers: vec![("fast".into(), "Fast".into())],
        controls: vec![ModelControl {
            id: "custom".into(),
            name: "Custom".into(),
            current: "a".into(),
            choices: vec![("a".into(), "A".into()), ("b".into(), "B".into())],
        }],
    }
}

#[test]
fn operator_launch_applies_validated_overrides_without_transferring_app_access() {
    let mut inherited = Settings {
        model: Some("test-model".into()),
        effort: Some("low".into()),
        operator: true,
        operator_enabled: true,
        orchestrator: true,
        plan: true,
        worktree_cwd: Some("/existing".into()),
        ..Default::default()
    };
    inherited
        .environment
        .insert("MONOCODE_APP_TOKEN".into(), "private".into());
    let input = json!({"prompt":"Review","model":"test-model","runtimeMode":"read-only","effort":"high","modelSettings":{"fast":"true","custom":"b","serviceTier":"fast"}});
    let settings = launch_settings(&input, "codex", inherited, Kind::Codex, &[model()]).unwrap();
    assert_eq!(settings.access, AccessMode::ReadOnly);
    assert_eq!(settings.effort.as_deref(), Some("high"));
    assert!(settings.fast);
    assert_eq!(settings.service_tier.as_deref(), Some("fast"));
    assert_eq!(settings.model_settings["custom"], "b");
    assert_eq!(settings.worktree_cwd.as_deref(), Some("/existing"));
    assert!(!settings.operator && !settings.orchestrator && !settings.plan);
    assert!(!settings.operator_enabled);
    assert!(settings.environment.is_empty());
    for bad in [
        json!({"model":"unknown"}),
        json!({"modelSettings":{"custom":"invalid"}}),
        json!({"effort":"unknown"}),
        json!({"modelSettings":{"custom":true}}),
    ] {
        assert!(
            launch_settings(&bad, "codex", Settings::default(), Kind::Codex, &[model()]).is_err(),
            "{bad}"
        );
    }
    let descriptor = model_description(Kind::Codex, &model());
    assert!(descriptor["settings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "reasoningEffort"));
}

#[test]
fn operator_thread_access_survives_reopen_without_enabling_other_sessions() {
    let dir = tempfile::tempdir().unwrap();
    for shared in [false, true] {
        let path = dir
            .path()
            .join(if shared { "shared.db" } else { "native.db" });
        let open = || {
            if shared {
                Store::open_monocode(&path).unwrap()
            } else {
                Store::open(&path).unwrap()
            }
        };
        let store = open();
        let caller = store.create("codex", "/project").unwrap();
        let other = store.create("codex", "/project").unwrap();
        store
            .save_agent_settings(
                caller.id,
                &Settings {
                    operator_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        drop(store);
        let store = open();
        let restored = store.agent_settings(caller.id).unwrap().unwrap();
        assert!(restored.operator_enabled);
        assert!(!restored.operator);
        assert!(
            !store
                .agent_settings(other.id)
                .unwrap()
                .unwrap_or_default()
                .operator_enabled
        );
    }
}

#[test]
fn operator_rejects_unknown_fields_and_mono_only_capabilities() {
    for (action, input) in [
        ("sessions.start", json!({"prompt":"x","sidebarHidden":true})),
        ("sessions.start", json!({"prompt":"x","reveal":"false"})),
        (
            "sessions.start",
            json!({"prompt":"x","runtimeMode":"unsafe"}),
        ),
        ("sessions.start", json!({"prompt":"x","placement":"left"})),
        ("sessions.start", json!({"prompt":"x","besideSessionId":1})),
        (
            "sessions.send",
            json!({"sessionId":1,"prompt":"x","notifyOnComplete":true}),
        ),
        ("artifacts.write", json!({"body":"x"})),
    ] {
        assert!(
            validate_operator(action, &input).is_err(),
            "{action}: {input}"
        );
    }
    assert!(prompt(&json!({"prompt":"/operator do something"})).is_err());
    assert!(validate_operator("sessions.archive", &json!({"sessionId":"desktop-uuid"})).is_ok());
    assert!(validate_operator(
        "sessions.start",
        &json!({"prompt":"x","placement":"down","besideSessionId":"uuid","effort":"high"})
    )
    .is_ok());
}

#[test]
fn conversation_pages_keep_source_cursors_prose_boundaries_and_character_caps() {
    let blocks = vec![
        json!({"id":"u1","role":"user","text":"/operator First"}),
        json!({"id":"tool","role":"tool","text":"private tool output"}),
        json!({"id":"a1","role":"assistant","text":"Earlier answer"}),
        json!({"id":"a2","role":"assistant","text":"🙂".repeat(205)}),
        json!({"id":"private","role":"assistant","text":"hidden","internal":true}),
        json!({"id":"draft","role":"user","text":"unsent","draft":true}),
        json!({"id":"u2","role":"user","text":"Second"}),
        json!({"id":"a3","role":"assistant","text":"Second answer"}),
    ];
    let newest = conversation_page(&blocks, &json!({"limit":1,"maxChars":200})).unwrap();
    assert_eq!(newest["turns"][0]["turnId"], "u2");
    assert_eq!(newest["nextBefore"], "u2");
    let older = conversation_page(
        &blocks,
        &json!({"before":newest["nextBefore"],"limit":1,"maxChars":200}),
    )
    .unwrap();
    assert_eq!(older["turns"][0]["user"]["text"], "First");
    assert_eq!(older["turns"][0]["earlierAssistantMessages"], 1);
    assert_eq!(
        older["turns"][0]["assistant"]["text"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        200
    );
    assert_eq!(older["turns"][0]["assistant"]["truncated"], true);
    assert!(older["nextBefore"].is_null());
    assert!(conversation_page(&blocks, &json!({"before":"missing"})).is_err());
}

#[test]
fn operator_notes_support_desktop_ids_partial_updates_empty_bodies_and_pagination() {
    let store = Store::open_monocode(Path::new(":memory:")).unwrap();
    let caller = store.create("codex", "/project").unwrap();
    let original = store
        .upsert_note(&NoteUpsert {
            id: "desktop-note-uuid".into(),
            title: "Original".into(),
            body: "Original body".into(),
            tags: vec!["keep".into()],
            source_cwd: Some("/project".into()),
            ..Default::default()
        })
        .unwrap();
    let updated = notes_action(
        &store,
        caller.id,
        "update",
        "notes.write",
        &json!({"id":original.id,"body":""}),
        &["/project".into()],
    )
    .unwrap();
    assert_eq!(updated["title"], "Original");
    assert_eq!(updated["body"], "");
    assert_eq!(updated["tags"], json!(["keep"]));
    assert!(
        updated["sourceSessionId"].is_null(),
        "an update keeps the original note provenance"
    );
    let alias = crate::store::desktop_session_handle(&original.id) as u64;
    let restored = notes_action(
        &store,
        caller.id,
        "legacy-read",
        "notes.read",
        &json!({"id":alias}),
        &["/project".into()],
    )
    .unwrap();
    assert_eq!(restored["id"], original.id);
    let created = notes_action(
        &store,
        caller.id,
        "create",
        "notes.write",
        &json!({"body":"# Inferred title\n\nUseful preview","tags":["#Work"]}),
        &["/project".into()],
    )
    .unwrap();
    assert_eq!(created["title"], "Inferred title");
    assert_eq!(created["tags"], json!(["work"]));
    let again = notes_action(
        &store,
        caller.id,
        "create",
        "notes.write",
        &json!({"body":"# Inferred title\n\nUseful preview","tags":["#Work"]}),
        &["/project".into()],
    )
    .unwrap();
    assert_eq!(created["id"], again["id"]);
    let page = notes_action(
        &store,
        caller.id,
        "list",
        "notes.list",
        &json!({"limit":1,"offset":1}),
        &["/project".into()],
    )
    .unwrap();
    assert_eq!(page["total"], 2);
    assert_eq!(page["notes"].as_array().unwrap().len(), 1);
    let unscoped = store
        .upsert_note(&NoteUpsert {
            id: "global-note".into(),
            title: "Global".into(),
            body: "Keep global".into(),
            ..Default::default()
        })
        .unwrap();
    let edited = notes_action(
        &store,
        caller.id,
        "global-edit",
        "notes.write",
        &json!({"id":unscoped.id,"title":"Still global"}),
        &["/project".into()],
    )
    .unwrap();
    assert!(edited["sourceCwd"].is_null());
    assert!(notes_action(
        &store,
        caller.id,
        "read",
        "notes.read",
        &json!({"id":original.id}),
        &["/other".into()]
    )
    .is_err());
}

#[test]
fn launch_identity_and_unsent_prompt_survive_reopen_without_duplicate_sessions() {
    let dir = tempfile::tempdir().unwrap();
    for shared in [false, true] {
        let path = dir
            .path()
            .join(if shared { "shared.db" } else { "legacy.db" });
        let open = || {
            if shared {
                Store::open_monocode(&path).unwrap()
            } else {
                Store::open(&path).unwrap()
            }
        };
        let store = open();
        let caller = store.create("codex", "/project").unwrap();
        let (child, fresh) = store
            .create_app_session(caller.id, "start", "fingerprint", "codex", "/project")
            .unwrap();
        assert!(fresh);
        store.save_started_link(child.id, caller.id).unwrap();
        store
            .prepare_app_prompt(child.id, "start", "Saved request", false)
            .unwrap();
        assert!(store.draft_sessions().unwrap().contains(&child.id));
        assert!(!store
            .session_sidebar_metadata(&["/project".into()])
            .sidebar_hidden
            .contains(&child.id));
        drop(store);
        let store = open();
        let (retried, fresh) = store
            .create_app_session(caller.id, "start", "fingerprint", "codex", "/project")
            .unwrap();
        assert!(!fresh);
        assert_eq!(retried.id, child.id);
        assert_eq!(store.sessions().unwrap().len(), 2);
        assert_eq!(
            store.follow_ups(child.id).unwrap().draft.unwrap().text,
            "Saved request"
        );
        assert!(store
            .create_app_session(caller.id, "start", "different", "codex", "/project")
            .is_err());
        let page = store
            .app_conversation(child.id)
            .unwrap()
            .page(
                &no_machines(),
                Duration::from_secs(1),
                &json!({}),
                false,
                true,
            )
            .unwrap();
        assert_eq!(page["hasDraft"], true);
        store
            .save_follow_ups(
                child.id,
                &State {
                    revision: 2,
                    ..Default::default()
                },
            )
            .unwrap();
    }
}

#[test]
fn a_remote_session_reads_through_its_negative_handle_before_its_first_turn() {
    // Desktop stores read a session's blocks from the local sessions table,
    // where a remote handle has no row ("Query returned no rows").
    for store in [
        Store::open_monocode(Path::new(":memory:")).unwrap(),
        Store::open(Path::new(":memory:")).unwrap(),
    ] {
        let project = store
            .remember_remote_project("env", &json!({"id":"project","cwd":"/host/repo"}))
            .unwrap();
        let session = store.create("claude", &project.key).unwrap();
        assert!(session.id < 0);
        // No machine is connected: a session that has not run asks no host.
        let page = store
            .app_conversation(session.id)
            .unwrap()
            .page(
                &no_machines(),
                Duration::from_secs(1),
                &json!({}),
                false,
                false,
            )
            .unwrap();
        assert_eq!(page["turns"], json!([]));
        assert_eq!(page["sessionId"], session.id);
        assert_eq!(page["title"], session.title);
    }
}

fn no_machines() -> std::sync::Arc<crate::remote::Connections> {
    let dir = tempfile::tempdir().unwrap();
    std::sync::Arc::new(crate::remote::Connections::new(dir.path()))
}

#[test]
fn a_remote_read_asks_its_host_only_after_leaving_the_store_and_gives_up_in_time() {
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    // A host that accepts the request and never answers.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(crate::remote::Connections::new(dir.path()));
    client.add_test_machine("m", "env", &endpoint);
    let store = Arc::new(Mutex::new(Store::open(Path::new(":memory:")).unwrap()));
    let id = {
        let db = store.lock().unwrap();
        let project = db
            .remember_remote_project("env", &json!({"id":"project","cwd":"/host/repo"}))
            .unwrap();
        let session = db.create("claude", &project.key).unwrap();
        let mut remote = db.remote_session(session.id).unwrap().unwrap();
        remote.session_id = Some("host-session".into());
        db.save_remote_session(&remote).unwrap();
        session.id
    };
    listener.set_nonblocking(true).unwrap();
    let read = store.lock().unwrap().app_conversation(id).unwrap();
    // Taking the read from the store contacts no host.
    assert!(listener.accept().is_err());
    let started = Instant::now();
    let reader = std::thread::spawn({
        let client = client.clone();
        move || {
            read.page(
                &client,
                Duration::from_millis(500),
                &json!({}),
                false,
                false,
            )
        }
    });
    // The store stays free while the host is asked.
    let mut held = Vec::new();
    while held.is_empty() && started.elapsed() < Duration::from_secs(5) {
        if let Ok((stream, _)) = listener.accept() {
            held.push(stream);
        }
        assert!(store.try_lock().is_ok());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!held.is_empty(), "the host was never asked");
    let error = reader.join().unwrap().unwrap_err();
    assert!(error.contains("did not return this session"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn deleting_a_remote_handle_removes_its_settings() {
    // sessions.start writes these for the handle before a failure deletes it.
    for mut store in [
        Store::open_monocode(Path::new(":memory:")).unwrap(),
        Store::open(Path::new(":memory:")).unwrap(),
    ] {
        let project = store
            .remember_remote_project("env", &json!({"id":"project","cwd":"/host/repo"}))
            .unwrap();
        let caller = store.create("codex", "/project").unwrap();
        let mut handles = Vec::new();
        for request in ["first", "second"] {
            let session = store.create("claude", &project.key).unwrap();
            store
                .configure_agent(session.id, "claude", &Default::default())
                .unwrap();
            store.save_started_link(session.id, caller.id).unwrap();
            store
                .prepare_app_prompt(session.id, request, "Prompt", true)
                .unwrap();
            store
                .save_setting(&format!("mono-delivered:{}:message", session.id), "1")
                .unwrap();
            handles.push(session.id);
        }
        // A shared store answers `mono-delivered:` from the desktop's own
        // tables, which have no row for a remote handle; check it natively.
        let shared = store.uses_monocode_schema();
        let keys = |id: i64| {
            let mut keys = vec![
                format!("remote-agent:{id}"),
                format!("started-by:{id}"),
                format!("follow-up:{id}"),
                format!("app-launch-draft:{id}"),
            ];
            if !shared {
                keys.push(format!("mono-delivered:{id}:message"));
            }
            keys
        };
        for key in keys(handles[0]) {
            assert!(
                store.setting(&key).unwrap().is_some(),
                "{key} was not written"
            );
        }
        store.delete(handles[0]).unwrap();
        for key in keys(handles[0]) {
            assert_eq!(store.setting(&key).unwrap(), None, "{key} was left behind");
        }
        for key in keys(handles[1]) {
            assert!(
                store.setting(&key).unwrap().is_some(),
                "{key} of another handle was removed"
            );
        }
    }
}

#[test]
fn a_remote_launch_prompt_is_kept_until_the_outbox_holds_it() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let project = store
        .remember_remote_project("env", &json!({"id":"project","cwd":"/host/repo"}))
        .unwrap();
    let session = store.create("claude", &project.key).unwrap();
    store
        .prepare_app_prompt(session.id, "request", "Run the tests", false)
        .unwrap();
    // Unsent: still pending, however often it is asked.
    for _ in 0..2 {
        assert_eq!(
            store.pending_remote_app_launch(session.id).unwrap(),
            Some(("Run the tests".into(), true))
        );
    }
    // Another first turn does not take the launch's prompt.
    store
        .finish_remote_app_launch(session.id, "Something else")
        .unwrap();
    assert!(store.follow_ups(session.id).unwrap().draft.is_some());
    // Journaled: the outbox owns it, and the local copy goes.
    let mut remote = store.remote_session(session.id).unwrap().unwrap();
    remote.outbox.push(json!({"type":"create","commandId":"c"}));
    store.save_remote_session(&remote).unwrap();
    assert_eq!(store.pending_remote_app_launch(session.id).unwrap(), None);
    assert!(store.follow_ups(session.id).unwrap().draft.is_none());
}
