//! Port of src/features/sessions/model/sessionFolders.test.ts.

use super::*;
use monocode_core::HarnessId;
use monocode_core::harness::RuntimeMode;

pub(crate) fn summary(id: &str) -> SessionSummary {
    SessionSummary {
        model: "gpt-5".into(),
        runtime_mode: RuntimeMode::Supervised,
        title: format!("cursor · {id}"),
        created_at: 1,
        updated_at: 1,
        additions: Some(0),
        deletions: Some(0),
        ..SessionSummary::new(id, "/tmp/project", HarnessId::Cursor)
    }
}

fn pinned(id: &str) -> SessionSummary {
    SessionSummary {
        pinned: Some(true),
        ..summary(id)
    }
}

fn updated(id: &str, updated_at: i64) -> SessionSummary {
    SessionSummary {
        updated_at,
        ..summary(id)
    }
}

fn folder(id: &str, session_ids: &[&str]) -> SessionFolder {
    SessionFolder::new(
        id,
        id,
        session_ids.iter().map(|id| id.to_string()).collect(),
    )
}

fn named(id: &str, session_ids: &[&str], name: &str) -> SessionFolder {
    SessionFolder {
        name: name.into(),
        ..folder(id, session_ids)
    }
}

fn collapsed(mut folder: SessionFolder) -> SessionFolder {
    folder.collapsed = true;
    folder
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn entry_kinds(entries: &[SessionListEntry]) -> Vec<&'static str> {
    entries
        .iter()
        .map(|entry| match entry {
            SessionListEntry::Folder { .. } => "folder",
            SessionListEntry::Pinned { .. } => "pinned",
            SessionListEntry::Reminders { .. } => "reminders",
            SessionListEntry::Session { .. } => "session",
        })
        .collect()
}

#[test]
fn unique_folder_name_uses_new_folder_then_numbers() {
    assert_eq!(unique_folder_name(&[]), "New folder");
    assert_eq!(
        unique_folder_name(&[named("a", &["s"], "New folder")]),
        "New folder 2"
    );
    assert_eq!(
        unique_folder_name(&[
            named("a", &["s1"], "New folder"),
            named("b", &["s2"], "New folder 2"),
        ]),
        "New folder 3"
    );
}

#[test]
fn puts_reminders_ahead_of_folders_and_pins_without_duplicate_cards_or_changing_membership() {
    let sessions = vec![
        pinned("pin"),
        summary("folder-member"),
        summary("loose"),
        pinned("other-pin"),
    ];
    let folders = vec![folder("work", &["folder-member", "loose"])];
    let loose = ungrouped_sessions(&sessions, &folders);
    let entries = build_session_list(
        &sessions,
        &folders,
        &loose,
        false,
        Some(&ReminderGroup {
            session_ids: strings(&["folder-member", "pin"]),
            collapsed: false,
        }),
    );
    assert_eq!(entry_kinds(&entries), vec!["reminders", "folder", "pinned"]);
    assert_eq!(
        session_list_navigation_ids(&entries, false),
        strings(&["folder-member", "pin", "loose", "other-pin"])
    );
    assert_eq!(folders[0].session_ids, strings(&["folder-member", "loose"]));
    let restored = build_session_list(&sessions, &folders, &loose, false, None);
    assert_eq!(
        session_list_navigation_ids(&restored, false),
        strings(&["folder-member", "loose", "pin", "other-pin"])
    );
}

#[test]
fn keeps_reminder_grouping_ahead_of_paginated_sessions_and_respects_collapse_and_search() {
    let sessions = vec![summary("loose"), summary("reminded")];
    let entries = build_session_list(
        &sessions,
        &[],
        &sessions[..1],
        false,
        Some(&ReminderGroup {
            session_ids: strings(&["reminded", "not-visible"]),
            collapsed: true,
        }),
    );
    assert_eq!(
        entries[0],
        SessionListEntry::Reminders {
            sessions: vec![sessions[1].clone()],
            collapsed: true,
        }
    );
    assert_eq!(
        session_list_navigation_ids(&entries, false),
        strings(&["loose"])
    );
    assert_eq!(
        session_list_navigation_ids(&entries, true),
        strings(&["reminded", "loose"])
    );
}

#[test]
fn places_folders_above_pinned_ungrouped_sessions() {
    let sessions = vec![
        SessionSummary {
            pinned: Some(true),
            ..updated("pin", 1)
        },
        updated("new", 9),
        updated("in-folder", 5),
    ];
    let folders = vec![named("work", &["in-folder"], "Work")];
    let entries = build_session_list(
        &sessions,
        &folders,
        &ungrouped_sessions(&sessions, &folders),
        false,
        None,
    );
    let labels: Vec<String> = entries
        .iter()
        .map(|entry| match entry {
            SessionListEntry::Folder { folder, .. } => folder.name.clone(),
            SessionListEntry::Pinned { sessions, .. } => format!(
                "Pinned:{}",
                sessions
                    .iter()
                    .map(|session| session.id.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            SessionListEntry::Session { session } => session.id.clone(),
            SessionListEntry::Reminders { .. } => "reminders".into(),
        })
        .collect();
    assert_eq!(labels, strings(&["Work", "Pinned:pin", "new"]));
}

#[test]
fn hides_folders_whose_members_are_not_in_the_visible_set() {
    let sessions = vec![summary("a")];
    let folders = vec![folder("hidden", &["gone"])];
    assert_eq!(
        build_session_list(&sessions, &folders, &sessions, false, None),
        vec![SessionListEntry::Session {
            session: Box::new(sessions[0].clone())
        }]
    );
}

#[test]
fn surfaces_an_open_folder_member_that_is_not_in_history_yet() {
    let visible = vec![summary("a")];
    let extra = summary("blank");
    let folders = vec![folder("work", &["a", "blank"])];
    let merged = merge_folder_session_summaries(&visible, std::slice::from_ref(&extra), &folders);
    let ids: Vec<&str> = merged.iter().map(|session| session.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "blank"]);
    assert_eq!(
        merge_folder_session_summaries(&visible, &[extra], &[]),
        visible
    );
}

#[test]
fn sorts_members_inside_a_folder_by_pin_then_recency() {
    let sessions = vec![
        updated("old", 1),
        SessionSummary {
            pinned: Some(true),
            ..updated("pinned", 2)
        },
        updated("new", 9),
    ];
    let folders = vec![folder("g", &["old", "new", "pinned"])];
    let entries = build_session_list(&sessions, &folders, &[], false, None);
    let SessionListEntry::Folder { sessions, .. } = &entries[0] else {
        panic!("expected a folder");
    };
    let ids: Vec<&str> = sessions.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, vec!["pinned", "new", "old"]);
}

#[test]
fn groups_pinned_sessions_without_a_divider() {
    let sessions = vec![pinned("pin"), summary("rest")];
    let entries = build_session_list(&sessions, &[], &sessions, false, None);
    assert_eq!(
        entries,
        vec![
            SessionListEntry::Pinned {
                collapsed: false,
                sessions: vec![sessions[0].clone()],
            },
            SessionListEntry::Session {
                session: Box::new(sessions[1].clone())
            },
        ]
    );
}

#[test]
fn omits_collapsed_pinned_sessions_from_keyboard_navigation() {
    let sessions = vec![pinned("pin"), summary("rest")];
    let entries = build_session_list(&sessions, &[], &sessions, true, None);
    assert_eq!(
        session_list_navigation_ids(&entries, false),
        strings(&["rest"])
    );
    assert_eq!(
        session_list_navigation_ids(&entries, true),
        strings(&["pin", "rest"])
    );
}

#[test]
fn exposes_the_full_visible_navigation_order_without_pagination() {
    let sessions = vec![summary("folder-a"), summary("folder-b"), summary("loose")];
    let folders = vec![collapsed(folder("work", &["folder-a", "folder-b"]))];
    let entries = build_session_list(
        &sessions,
        &folders,
        &ungrouped_sessions(&sessions, &folders),
        false,
        None,
    );
    assert_eq!(
        session_list_navigation_ids(&entries, false),
        strings(&["loose"])
    );
    assert_eq!(
        session_list_navigation_ids(&entries, true),
        strings(&["folder-a", "folder-b", "loose"])
    );
}

#[test]
fn creates_a_folder_at_the_top_and_pulls_members_out_of_other_folders() {
    let existing = vec![named("old", &["a", "c"], "Old")];
    let (folders, id) = create_folder_with_sessions(&existing, &strings(&["a", "b"]), None);
    assert!(!id.is_empty());
    assert_eq!(folders[0].id, id);
    assert_eq!(folders[0].name, "New folder");
    assert_eq!(folders[0].session_ids, strings(&["a", "b"]));
    assert!(!folders[0].collapsed);
    assert_eq!(folders[1].id, "old");
    assert_eq!(folders[1].session_ids, strings(&["c"]));
}

#[test]
fn adds_a_session_to_a_folder_and_drops_an_emptied_source_folder() {
    let folders = vec![folder("src", &["a"]), folder("dst", &["b"])];
    let next = add_session_to_folder(&folders, "dst", "a");
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].id, "dst");
    assert_eq!(next[0].session_ids, strings(&["b", "a"]));
}

#[test]
fn places_the_current_session_in_an_existing_expanded_folder() {
    let folders = vec![collapsed(folder("work", &["a"]))];
    assert_eq!(
        place_session_in_folder(
            &folders,
            "b",
            &SessionFolderTarget::Existing {
                folder_id: "work".into()
            }
        ),
        vec![folder("work", &["a", "b"])]
    );
}

#[test]
fn creates_a_named_folder_for_the_current_session() {
    let folders = place_session_in_folder(
        &[],
        "a",
        &SessionFolderTarget::New {
            name: "  Launch work  ".into(),
        },
    );
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "Launch work");
    assert_eq!(folders[0].session_ids, strings(&["a"]));
    assert!(!folders[0].collapsed);
}

#[test]
fn is_a_no_op_when_adding_a_session_already_in_that_folder() {
    let folders = vec![folder("g", &["a"])];
    assert_eq!(add_session_to_folder(&folders, "g", "a"), folders);
}

#[test]
fn removes_a_session_and_deletes_the_folder_when_it_would_be_empty() {
    let folders = vec![folder("g", &["a", "b"])];
    let next = remove_session_from_folder(&folders, "a");
    assert_eq!(next[0].session_ids, strings(&["b"]));
    assert!(remove_session_from_folder(&next, "b").is_empty());
}

#[test]
fn dissolves_a_folder_without_touching_the_others() {
    let folders = vec![folder("a", &["s1"]), folder("b", &["s2"])];
    let ids: Vec<String> = dissolve_folder(&folders, "a")
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, strings(&["b"]));
}

#[test]
fn renames_and_ignores_a_blank_name() {
    let folders = vec![named("g", &["a"], "Work")];
    assert_eq!(rename_folder(&folders, "g", "  Sprint  ")[0].name, "Sprint");
    assert_eq!(rename_folder(&folders, "g", "   "), folders);
}

#[test]
fn toggles_collapsed_without_rewriting_unchanged_folders() {
    let folders = vec![folder("g", &["a"])];
    assert_eq!(set_folder_collapsed(&folders, "g", false), folders);
    assert!(set_folder_collapsed(&folders, "g", true)[0].collapsed);
}

#[test]
fn stores_a_palette_color_and_clears_it_back_to_the_default_wash() {
    let folders = vec![folder("g", &["a"])];
    let tinted = set_folder_color(&folders, "g", Some(2));
    assert_eq!(tinted[0].color_index, Some(2));
    assert_eq!(set_folder_color(&tinted, "g", Some(2)), tinted);
    assert_eq!(set_folder_color(&tinted, "g", Some(0))[0].color_index, None);
    assert_eq!(set_folder_color(&tinted, "g", None)[0].color_index, None);
    assert!(folder_accent(Some(2), None).is_some());
    assert_eq!(folder_accent(Some(0), None), None);
    assert!(
        folder_shell_fill(Some(2), None)
            .unwrap()
            .starts_with("color-mix(")
    );
    assert_eq!(folder_shell_fill(None, None), None);
}

#[test]
fn stores_a_custom_hex_and_prefers_it_over_a_palette_index() {
    let folders = vec![SessionFolder {
        color_index: Some(2),
        ..folder("g", &["a"])
    }];
    let custom = set_folder_custom_color(&folders, "g", Some("#3B82F6"));
    assert_eq!(custom[0].custom_color.as_deref(), Some("#3b82f6"));
    assert_eq!(custom[0].color_index, None);
    assert_eq!(
        set_folder_custom_color(&custom, "g", Some("#3b82f6")),
        custom
    );
    assert_eq!(
        set_folder_custom_color(&custom, "g", Some("not-a-color")),
        custom
    );
    assert_eq!(
        set_folder_custom_color(&custom, "g", None)[0].custom_color,
        None
    );
    assert_eq!(
        folder_accent(Some(2), Some("#3b82f6")).as_deref(),
        Some("#3b82f6")
    );
    assert!(
        folder_shell_fill(None, Some("#3b82f6"))
            .unwrap()
            .contains("color-mix(in srgb, #3b82f6 18%")
    );
    let preset = set_folder_color(&custom, "g", Some(3));
    assert_eq!(preset[0].color_index, Some(3));
    assert_eq!(preset[0].custom_color, None);
    assert_eq!(set_folder_color(&custom, "g", None)[0].custom_color, None);
}

#[test]
fn reorders_named_folders_and_leaves_others_in_place() {
    let folders = vec![
        folder("a", &["s1"]),
        folder("hidden", &["gone"]),
        folder("b", &["s2"]),
        folder("c", &["s3"]),
    ];
    let next = reorder_session_folders(&folders, &strings(&["c", "a", "b"]));
    let ids: Vec<&str> = next.iter().map(|folder| folder.id.as_str()).collect();
    assert_eq!(ids, vec!["c", "hidden", "a", "b"]);
    assert_eq!(
        reorder_session_folders(&folders, &strings(&["a", "b", "c"])),
        folders
    );
}

#[test]
fn makes_a_folder_when_one_ungrouped_session_is_dropped_on_another() {
    let (folders, created) =
        apply_session_list_drop(&[], "a", &SessionListDropTarget::Session { id: "b".into() });
    assert!(created.is_some());
    assert_eq!(folders[0].session_ids, strings(&["a", "b"]));
}

#[test]
fn joins_the_target_folder_when_dropping_on_a_session_already_in_one() {
    let folders = vec![folder("work", &["b"])];
    let (next, created) = apply_session_list_drop(
        &folders,
        "a",
        &SessionListDropTarget::Session { id: "b".into() },
    );
    assert_eq!(created, None);
    assert_eq!(next[0].session_ids, strings(&["b", "a"]));
}

#[test]
fn expands_a_collapsed_folder_you_drop_onto() {
    let folders = vec![collapsed(folder("work", &["b"]))];
    let (next, _) = apply_session_list_drop(
        &folders,
        "a",
        &SessionListDropTarget::Folder { id: "work".into() },
    );
    assert!(!next[0].collapsed);
    assert_eq!(next[0].session_ids, strings(&["b", "a"]));
}

#[test]
fn ignores_a_drop_onto_itself_or_a_sibling_in_the_same_folder() {
    let folders = vec![folder("work", &["a", "b"])];
    let target = |id: &str| SessionListDropTarget::Session { id: id.into() };
    assert_eq!(
        apply_session_list_drop(&folders, "a", &target("a")).0,
        folders
    );
    assert_eq!(
        apply_session_list_drop(&folders, "a", &target("b")).0,
        folders
    );
}

#[test]
fn moves_a_session_from_one_folder_into_another() {
    let folders = vec![folder("src", &["a"]), folder("dst", &["b"])];
    let (next, _) = apply_session_list_drop(
        &folders,
        "a",
        &SessionListDropTarget::Folder { id: "dst".into() },
    );
    let ids: Vec<&str> = next.iter().map(|folder| folder.id.as_str()).collect();
    assert_eq!(ids, vec!["dst"]);
    assert_eq!(next[0].session_ids, strings(&["b", "a"]));
}

#[test]
fn drops_unknown_members_and_empty_folders_keeping_the_same_list_when_nothing_changed() {
    let folders = vec![
        folder("keep", &["a", "gone"]),
        folder("empty", &["missing"]),
    ];
    let known: HashSet<String> = ["a".to_string()].into_iter().collect();
    let next = prune_session_folders(&folders, &known);
    assert_eq!(next, vec![folder("keep", &["a"])]);
    assert_eq!(prune_session_folders(&next, &known), next);
}

#[test]
fn finds_the_folder_a_session_belongs_to() {
    let folders = vec![folder("g", &["a"])];
    assert_eq!(
        folder_containing(&folders, "a").map(|f| f.id.as_str()),
        Some("g")
    );
    assert!(folder_containing(&folders, "b").is_none());
}

#[test]
fn round_trips_folders_for_a_project_and_ignores_another_cwd() {
    let kv = Kv::in_memory();
    let folders = vec![collapsed(named("g", &["a"], "Work"))];
    save_session_folders(&kv, "/tmp/project/", &folders);
    assert_eq!(load_session_folders(&kv, "/tmp/project"), folders);
    assert!(load_session_folders(&kv, "/tmp/other").is_empty());
}

#[test]
fn does_not_persist_the_home_cwd() {
    let kv = Kv::in_memory();
    save_session_folders(&kv, "~", &[folder("g", &["a"])]);
    assert!(load_session_folders(&kv, "~").is_empty());
}

#[test]
fn round_trips_a_folder_color() {
    let kv = Kv::in_memory();
    let folders = vec![SessionFolder {
        color_index: Some(4),
        ..named("g", &["a"], "Work")
    }];
    save_session_folders(&kv, "/tmp/project", &folders);
    assert_eq!(
        load_session_folders(&kv, "/tmp/project")[0].color_index,
        Some(4)
    );
}

#[test]
fn round_trips_a_custom_folder_color_and_prefers_it_over_a_palette_index() {
    let kv = Kv::in_memory();
    save_session_folders(
        &kv,
        "/tmp/project",
        &[SessionFolder {
            custom_color: Some("#AABBCC".into()),
            ..named("g", &["a"], "Work")
        }],
    );
    let loaded = load_session_folders(&kv, "/tmp/project");
    assert_eq!(loaded[0].custom_color.as_deref(), Some("#aabbcc"));
    assert_eq!(loaded[0].color_index, None);

    save_session_folders(
        &kv,
        "/tmp/project",
        &[SessionFolder {
            color_index: Some(4),
            custom_color: Some("#ff00aa".into()),
            ..named("g", &["a"], "Work")
        }],
    );
    let loaded = load_session_folders(&kv, "/tmp/project");
    assert_eq!(loaded[0].custom_color.as_deref(), Some("#ff00aa"));
    assert_eq!(loaded[0].color_index, None);
}

#[test]
fn drops_an_invalid_custom_folder_color_on_load() {
    let kv = Kv::in_memory();
    save_session_folders(
        &kv,
        "/tmp/project",
        &[SessionFolder {
            custom_color: Some("red".into()),
            ..named("g", &["a"], "Work")
        }],
    );
    assert_eq!(
        load_session_folders(&kv, "/tmp/project")[0].custom_color,
        None
    );
}

#[test]
fn drops_a_project_key_when_the_last_folder_is_gone() {
    let kv = Kv::in_memory();
    save_session_folders(&kv, "/tmp/project", &[folder("g", &["a"])]);
    save_session_folders(&kv, "/tmp/project", &[]);
    assert_eq!(kv.get_item(SESSION_FOLDERS_KEY).as_deref(), Some("{}"));
}

#[test]
fn round_trips_the_pinned_group_collapsed_state_per_project() {
    let kv = Kv::in_memory();
    save_pinned_sessions_collapsed(&kv, "/tmp/project/", true);
    assert!(load_pinned_sessions_collapsed(&kv, "/tmp/project"));
    assert!(!load_pinned_sessions_collapsed(&kv, "/tmp/other"));

    save_pinned_sessions_collapsed(&kv, "/tmp/project", false);
    assert!(!load_pinned_sessions_collapsed(&kv, "/tmp/project"));
    assert_eq!(kv.get_item(PINNED_COLLAPSED_KEY).as_deref(), Some("{}"));
}

#[test]
fn writes_the_typescript_json_shape() {
    let kv = Kv::in_memory();
    save_session_folders(
        &kv,
        "/tmp/project",
        &[SessionFolder {
            color_index: Some(4),
            ..named("g", &["a"], "Work")
        }],
    );
    assert_eq!(
        kv.get_item(SESSION_FOLDERS_KEY).as_deref(),
        Some(
            r#"{"/tmp/project":[{"id":"g","name":"Work","sessionIds":["a"],"collapsed":false,"colorIndex":4}]}"#
        )
    );
}

#[test]
fn rebases_folders_and_collapsed_groups_to_a_new_path() {
    let kv = Kv::in_memory();
    save_session_folders(&kv, "/tmp/old", &[folder("g", &["a"])]);
    save_pinned_sessions_collapsed(&kv, "/tmp/old", true);
    save_reminder_sessions_collapsed(&kv, "/tmp/old", true);
    rebase_session_folder_settings(&kv, "/tmp/old", "/tmp/new/");
    assert!(load_session_folders(&kv, "/tmp/old").is_empty());
    assert_eq!(
        load_session_folders(&kv, "/tmp/new"),
        vec![folder("g", &["a"])]
    );
    assert!(load_pinned_sessions_collapsed(&kv, "/tmp/new"));
    assert!(load_reminder_sessions_collapsed(&kv, "/tmp/new"));
    assert!(!load_pinned_sessions_collapsed(&kv, "/tmp/old"));
}

#[test]
fn hears_folder_changes_for_its_own_project_only() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let kv = Kv::in_memory();
    let heard = Arc::new(AtomicUsize::new(0));
    let counter = heard.clone();
    let _subscription = subscribe_session_folders(&kv, "/tmp/project", move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    save_session_folders(&kv, "/tmp/other", &[folder("o", &["x"])]);
    assert_eq!(heard.load(Ordering::SeqCst), 0);
    save_session_folders(&kv, "/tmp/project", &[folder("g", &["a"])]);
    assert_eq!(heard.load(Ordering::SeqCst), 1);
    assert!(subscribe_session_folders(&kv, "~", || {}).is_none());
}
