use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::store::Store;
use std::sync::mpsc;

fn setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
    let (window, other, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, cx| {
                v.install_transfer(
                    Settings {
                        operator_enabled: true,
                        ..Default::default()
                    },
                    None,
                    cx,
                );
            });
        })
        .unwrap();
    cx.run_until_parked();
    (window, other, caller)
}
fn call(
    window: WindowHandle<Shell>,
    caller: i64,
    request: &str,
    action: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> Result<Value, String> {
    let (reply, result) = mpsc::channel();
    window
        .update(cx, |shell, window, cx| {
            shell.handle_app_call(
                Call {
                    session: caller,
                    request_id: request.into(),
                    action: action.into(),
                    input,
                    reply,
                },
                window,
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    result
        .try_recv()
        .expect("the app action must settle after its writes")
}
fn launch(
    window: WindowHandle<Shell>,
    caller: i64,
    request: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> i64 {
    let mut input = input;
    input["draft"] = json!(true);
    input["prompt"] = json!("Review this project");
    call(window, caller, request, "sessions.start", input, cx).unwrap()["sessionId"]
        .as_i64()
        .unwrap()
}

#[gpui::test]
fn submitted_operator_access_survives_the_composer_reset(cx: &mut TestAppContext) {
    let (window, _, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |view, cx| {
                // Save the turn, then fail without starting a real provider.
                view.summary.provider = "fixture".into();
                view.quick_submit(
                    "/operator Start a test session".into(),
                    Settings::default(),
                    cx,
                );
                assert!(view.busy);
                assert!(!view.settings.operator);
                assert!(view.settings.operator_enabled);
            });
            let (reply, _) = mpsc::channel();
            shell
                .authorize_app_call(
                    &Call {
                        session: caller,
                        request_id: "after-submit".into(),
                        action: "sessions.list".into(),
                        input: json!({}),
                        reply,
                    },
                    cx,
                )
                .unwrap();
        })
        .unwrap();
    cx.run_until_parked();
    assert!(call(window, caller, "later", "sessions.list", json!({}), cx).is_ok());
    window
        .update(cx, |shell, _, cx| {
            let saved = locked(&shell.store)
                .unwrap()
                .agent_settings(caller)
                .unwrap()
                .unwrap();
            assert!(!saved.operator);
            assert!(saved.operator_enabled);
            shell.sessions[&caller].update(cx, |view, cx| {
                view.quick_submit("List my sessions".into(), saved, cx);
                assert!(!view.settings.operator);
                assert!(view.settings.operator_enabled);
            });
        })
        .unwrap();
    assert!(call(window, caller, "next-turn", "sessions.list", json!({}), cx).is_ok());
    let child = launch(
        window,
        caller,
        "after-reset",
        json!({"harness":"codex"}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.sessions[&child].read(cx).settings.operator_enabled);
        })
        .unwrap();
}

#[gpui::test]
fn unsent_operator_selection_does_not_grant_app_access(cx: &mut TestAppContext) {
    let (window, _, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |view, cx| view.enable_mode("operator", cx));
        })
        .unwrap();
    assert_eq!(
        call(window, caller, "unsent", "sessions.list", json!({}), cx).unwrap_err(),
        "Operator is not enabled for this thread."
    );
}

#[gpui::test]
fn operator_planning_turns_keep_read_only_app_access_after_submission(cx: &mut TestAppContext) {
    for mode in ["plan", "orchestrator"] {
        let (window, _, caller) = setup(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&caller].update(cx, |view, cx| {
                    view.summary.provider = "fixture".into();
                    view.quick_submit(
                        format!("/{mode} Inspect my sessions"),
                        view.settings.clone(),
                        cx,
                    );
                    assert!(view.busy);
                    assert!(!view.settings.plan && !view.settings.orchestrator);
                });
            })
            .unwrap();
        assert!(call(window, caller, "plan-read", "sessions.list", json!({}), cx).is_ok());
        assert!(
            call(
                window,
                caller,
                "plan-write",
                "notes.write",
                json!({"title":"Do not save", "body":"Plan only"}),
                cx,
            )
            .unwrap_err()
            .contains("read-only")
        );
    }
}

#[gpui::test]
fn operator_scopes_sessions_and_preserves_an_unsent_draft(cx: &mut TestAppContext) {
    let (window, other, caller) = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, cx| {
                v.input
                    .update(cx, |i, cx| i.set_text("keep my draft".into(), cx))
            })
        })
        .unwrap();
    let list = call(window, caller, "list", "sessions.list", json!({}), cx).unwrap();
    assert!(
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == caller)
    );
    assert!(
        !list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == other)
    );
    assert!(
        call(
            window,
            caller,
            "outside",
            "sessions.read",
            json!({"sessionId":other}),
            cx
        )
        .is_err()
    );
    assert!(
        call(
            window,
            caller,
            "self-draft",
            "sessions.draft",
            json!({"sessionId":caller,"prompt":"replace"}),
            cx
        )
        .is_err()
    );
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(
                shell.sessions[&caller].read(cx).input.read(cx).content,
                "keep my draft"
            );
            shell.sessions[&caller].update(cx, |v, _| {
                v.settings.access = monocode_core::agent::AccessMode::ReadOnly
            });
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "readonly",
            "notes.write",
            json!({"title":"Do not save","body":"Read only"}),
            cx
        )
        .unwrap_err()
        .contains("read-only")
    );
}

#[gpui::test]
fn operator_children_are_visible_durable_and_do_not_navigate_by_default(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(
        window,
        caller,
        "draft",
        json!({"runtimeMode":"read-only"}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.workspace.active, Some(caller));
            assert!(shell.sidebar_session_ids(cx).contains(&child));
            assert!(!shell.sidebar_hidden_sessions.contains(&child));
            assert!(!shell.monos.as_ref().unwrap().read(cx).hidden_session(child));
            let view = shell.sessions[&child].read(cx);
            assert_eq!(
                view.settings.access,
                monocode_core::agent::AccessMode::ReadOnly
            );
            // A draft launch opens with its prompt in the composer.
            assert!(view.has_unsent_draft());
            assert_eq!(view.input.read(cx).content.as_ref(), "Review this project");
            assert!(!view.busy);
            let db = locked(&shell.store).unwrap();
            assert_eq!(
                db.composer_draft(child).unwrap().unwrap().text,
                "Review this project"
            );
            assert_eq!(db.started_links().unwrap()[&child], caller);
            assert!(db.composer_draft_sessions().unwrap().contains(&child));
        })
        .unwrap();
    let opened = launch(window, caller, "visible", json!({"reveal":true}), cx);
    window
        .update(cx, |shell, _, _| {
            assert_eq!(shell.workspace.active, Some(opened))
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "hidden",
            "sessions.start",
            json!({"prompt":"Do not hide","draft":true,"sidebarHidden":true}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn operator_launches_can_build_nested_splits(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let right = launch(window, caller, "right", json!({"placement":"right"}), cx);
    let down = launch(
        window,
        caller,
        "down",
        json!({"placement":"down","besideSessionId":right}),
        cx,
    );
    window
        .update(cx, |shell, _, _| {
            let tab = shell.workspace.tab_of(caller).unwrap();
            let layout = shell.workspace.layout(tab);
            assert!(layout.contains(right) && layout.contains(down));
            assert_eq!(layout.leaf_ids().len(), 3);
            assert_eq!(shell.workspace.active, Some(down));
        })
        .unwrap();
}

#[gpui::test]
fn operator_archive_delete_and_folder_actions_settle_after_persistence(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let archived = launch(window, caller, "archive-child", json!({}), cx);
    let deleted = launch(window, caller, "delete-child", json!({"reveal":true}), cx);
    let folder = call(
        window,
        caller,
        "folder",
        "folders.move",
        json!({"sessionId":deleted,"newFolderName":"Review tasks"}),
        cx,
    )
    .unwrap();
    assert!(folder["folderId"].is_string());
    assert_eq!(
        call(
            window,
            caller,
            "archive",
            "sessions.archive",
            json!({"sessionId":archived}),
            cx
        )
        .unwrap()["archived"],
        true
    );
    assert_eq!(
        call(
            window,
            caller,
            "delete",
            "sessions.delete",
            json!({"sessionId":deleted}),
            cx
        )
        .unwrap()["deleted"],
        true
    );
    window
        .update(cx, |shell, _, _| {
            assert!(shell.session_summaries[&archived].archived);
            assert!(!shell.session_summaries.contains_key(&deleted));
            assert!(shell.workspace.tab_of(deleted).is_none());
            assert!(
                shell
                    .groups
                    .values()
                    .all(|g| g.folder_of(deleted).is_none())
            );
            let db = locked(&shell.store).unwrap();
            assert!(
                db.sessions()
                    .unwrap()
                    .iter()
                    .find(|s| s.id == archived)
                    .unwrap()
                    .archived
            );
            assert!(!db.sessions().unwrap().iter().any(|s| s.id == deleted));
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "self-delete",
            "sessions.delete",
            json!({"sessionId":caller}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn operator_retries_use_durable_receipts_and_reject_changed_input(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "repeat", json!({}), cx);
    window
        .update(cx, |shell, _, _| shell.app_results.clear())
        .unwrap();
    let retry = launch(window, caller, "repeat", json!({}), cx);
    assert_eq!(retry, child);
    assert!(
        call(
            window,
            caller,
            "repeat",
            "sessions.start",
            json!({"draft":true,"prompt":"Different input"}),
            cx
        )
        .unwrap_err()
        .contains("different input")
    );
    window
        .update(cx, |shell, _, _| {
            assert_eq!(
                shell
                    .started_by
                    .values()
                    .filter(|id| **id == caller)
                    .count(),
                1
            )
        })
        .unwrap();
}

#[gpui::test]
fn operator_notes_and_conversation_paging_use_the_project_api(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "read-child", json!({}), cx);
    let note = call(
        window,
        caller,
        "note",
        "notes.write",
        json!({"body":"# Review notes\n\nKeep this","tags":["work"]}),
        cx,
    )
    .unwrap();
    let changed = call(
        window,
        caller,
        "note-edit",
        "notes.write",
        json!({"id":note["id"],"tags":["done"]}),
        cx,
    )
    .unwrap();
    assert_eq!(changed["body"], note["body"]);
    assert_eq!(changed["tags"], json!(["done"]));
    let page = call(
        window,
        caller,
        "note-page",
        "notes.list",
        json!({"limit":1,"offset":0}),
        cx,
    )
    .unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["notes"][0]["id"], note["id"]);
    let read = call(
        window,
        caller,
        "read",
        "sessions.read",
        json!({"sessionId":child,"limit":1,"maxChars":200}),
        cx,
    )
    .unwrap();
    assert_eq!(read["hasDraft"], true);
    assert_eq!(read["turns"], json!([]));
    assert!(
        call(
            window,
            caller,
            "ignored-field",
            "sessions.read",
            json!({"sessionId":child,"typo":1}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn operator_creates_selects_and_validates_real_worktrees(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!(
        "operator-worktrees-{}",
        monocode_core::mono::new_id()
    ));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    std::fs::write(project.join("README.md"), "Fixture\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Operator Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-qm",
        "Fixture",
    ]);
    let project = project.to_string_lossy().into_owned();
    let store = Arc::new(Mutex::new(Store::open(&root.join("native.db")).unwrap()));
    let caller = locked(&store).unwrap().create("codex", &project).unwrap();
    let id = caller.id;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(store, vec![caller], project.clone(), None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            shell.ensure_session(id, cx).unwrap().update(cx, |v, cx| {
                v.install_transfer(
                    Settings {
                        operator_enabled: true,
                        ..Default::default()
                    },
                    None,
                    cx,
                );
            })
        })
        .unwrap();
    cx.run_until_parked();
    let tree = call(
        window,
        id,
        "create-tree",
        "worktrees.create",
        json!({"branch":"operator-review","base":"HEAD"}),
        cx,
    )
    .unwrap();
    let path = tree["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).join("README.md").is_file());
    let listed = call(window, id, "list-trees", "worktrees.list", json!({}), cx).unwrap();
    assert!(
        listed["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["path"] == path)
    );
    let selected = launch(window, id, "select-tree", json!({"worktreeCwd":path}), cx);
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(
                shell.sessions[&selected]
                    .read(cx)
                    .settings
                    .worktree_cwd
                    .as_deref(),
                Some(path)
            )
        })
        .unwrap();
    let fresh = launch(
        window,
        id,
        "new-tree",
        json!({"workspaceMode":"worktree","worktreeBase":"HEAD"}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            let path = shell.sessions[&fresh]
                .read(cx)
                .settings
                .worktree_cwd
                .as_deref()
                .unwrap();
            assert_ne!(path, project);
            assert!(std::path::Path::new(path).join("README.md").is_file());
        })
        .unwrap();
    let before = window
        .update(cx, |shell, _, _| {
            locked(&shell.store).unwrap().sessions().unwrap().len()
        })
        .unwrap();
    assert!(
        call(
            window,
            id,
            "invalid-checkout",
            "sessions.start",
            json!({"prompt":"Must not launch","draft":true,"worktreeCwd":"/unrelated-checkout"}),
            cx
        )
        .is_err()
    );
    window
        .update(cx, |shell, _, _| {
            assert_eq!(
                locked(&shell.store).unwrap().sessions().unwrap().len(),
                before
            )
        })
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn app_launch_submission_uses_the_saved_message_identity(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "queued-seed", json!({}), cx);
    let expected = window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&child].clone();
            // A draft launch hands its prompt to the composer. Record it again
            // as the saved prompt a launch that sends right away keeps.
            let saved = {
                let db = locked(&shell.store).unwrap();
                db.prepare_app_prompt(child, "queued-seed-send", "Review this project")
                    .unwrap();
                db.follow_ups(child).unwrap().draft.unwrap()
            };
            let expected = saved.id.clone();
            view.update(cx, |v, cx| {
                v.hold_saved_draft(saved);
                // Hold the provider boundary: verify queue acceptance without starting
                // a real CLI or making an inference request.
                v.busy = true;
                v.install_app_launch(v.settings.clone(), true, cx);
            });
            expected
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            let state = locked(&shell.store).unwrap().follow_ups(child).unwrap();
            assert!(state.draft.is_none());
            assert_eq!(state.messages.len(), 1);
            assert_eq!(state.messages[0].id, expected);
            assert_eq!(state.messages[0].text, "Review this project");
            shell.sessions[&child].update(cx, |v, _| v.busy = false);
        })
        .unwrap();
}
