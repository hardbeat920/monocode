use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::store::Store;
use std::sync::mpsc;

fn setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
    let (window, other, caller) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, cx| {
                v.install_transfer(Settings::default(), None, cx);
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
    call(window, caller, request, "threads.new", input, cx).unwrap()["sessionId"]
        .as_i64()
        .unwrap()
}

#[gpui::test]
fn planning_turns_keep_read_only_app_access_after_submission(cx: &mut TestAppContext) {
    let mode = "plan";
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
                assert!(!view.settings.plan);
            });
        })
        .unwrap();
    assert!(call(window, caller, "plan-read", "folders.list", json!({}), cx).is_ok());
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

#[gpui::test]
fn app_actions_stay_in_the_project_and_respect_a_read_only_thread(cx: &mut TestAppContext) {
    let (window, other, caller) = setup(cx);
    assert!(
        call(
            window,
            caller,
            "outside",
            "folders.move",
            json!({"sessionId":other,"newFolderName":"Elsewhere"}),
            cx
        )
        .is_err()
    );
    window
        .update(cx, |shell, _, cx| {
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
fn started_threads_are_visible_durable_and_do_not_navigate_by_default(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let child = launch(window, caller, "draft", json!({}), cx);
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.workspace.active, Some(caller));
            assert!(shell.sidebar_session_ids(cx).contains(&child));
            assert!(!shell.sidebar_hidden_sessions.contains(&child));
            assert!(!shell.monos.as_ref().unwrap().read(cx).hidden_session(child));
            let view = shell.sessions[&child].read(cx);
            assert!(view.has_saved_draft());
            assert!(!view.busy);
            let db = locked(&shell.store).unwrap();
            assert_eq!(
                db.follow_ups(child).unwrap().draft.unwrap().text,
                "Review this project"
            );
            assert_eq!(db.started_links().unwrap()[&child], caller);
            assert!(db.draft_sessions().unwrap().contains(&child));
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
            "threads.new",
            json!({"prompt":"Do not hide","draft":true,"sidebarHidden":true}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn opened_threads_can_build_nested_splits(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let right = launch(window, caller, "first", json!({}), cx);
    let down = launch(window, caller, "second", json!({}), cx);
    let input = json!({"threadId":right,"placement":"right"});
    call(window, caller, "open-right", "control.open", input, cx).unwrap();
    let input = json!({"threadId":down,"placement":"down","besideSessionId":right});
    call(window, caller, "open-down", "control.open", input, cx).unwrap();
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
fn archive_and_folder_actions_settle_after_persistence(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let archived = launch(window, caller, "archive-child", json!({}), cx);
    let filed = launch(window, caller, "folder-child", json!({"reveal":true}), cx);
    let folder = call(
        window,
        caller,
        "folder",
        "folders.move",
        json!({"sessionId":filed,"newFolderName":"Review tasks"}),
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
    window
        .update(cx, |shell, _, _| {
            assert!(shell.session_summaries[&archived].archived);
            let db = locked(&shell.store).unwrap();
            assert!(
                db.sessions()
                    .unwrap()
                    .iter()
                    .find(|s| s.id == archived)
                    .unwrap()
                    .archived
            );
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "self-archive",
            "sessions.archive",
            json!({"sessionId":caller}),
            cx
        )
        .is_err()
    );
}

#[gpui::test]
fn retries_use_durable_receipts_and_reject_changed_input(cx: &mut TestAppContext) {
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
            "threads.new",
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
fn notes_use_the_project_api(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
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
}

#[gpui::test]
fn worktrees_are_created_and_listed_for_real(cx: &mut TestAppContext) {
    let root =
        std::env::temp_dir().join(format!("app-worktrees-{}", monocode_core::mono::new_id()));
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
        "user.name=Worktree Test",
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
                v.install_transfer(Settings::default(), None, cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    let tree = call(
        window,
        id,
        "create-tree",
        "worktrees.create",
        json!({"branch":"app-review","base":"HEAD"}),
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
    std::fs::remove_dir_all(root).unwrap();
}
