use super::*;
use crate::{follow_up::State, notes::NoteUpsert};
use std::path::Path;

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
            .prepare_app_prompt(child.id, "start", "Saved request")
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
            .app_conversation(child.id, &json!({}), false, true)
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
