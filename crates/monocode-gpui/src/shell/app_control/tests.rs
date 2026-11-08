use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::agent::{AccessMode, Settings};
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
                    request_id: monocode_core::mono::new_id(),
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
    result.recv().unwrap()
}

/// A second thread in the caller's project.
fn sibling(window: WindowHandle<Shell>, title: &str, cx: &mut TestAppContext) -> i64 {
    let id = window
        .update(cx, |shell, _, cx| {
            let mut summary = locked(&shell.store)
                .unwrap()
                .create("claude", "/work/monocode")
                .unwrap();
            summary.title = title.into();
            let id = summary.id;
            shell.add(summary, cx);
            id
        })
        .unwrap();
    cx.run_until_parked();
    id
}

#[gpui::test]
fn panels_open_close_and_repeat_safely(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let set = |open, cx: &mut TestAppContext| {
        call(
            window,
            caller,
            "control.panel",
            json!({"panel":"sidebar","open":open}),
            cx,
        )
    };
    assert_eq!(set(false, cx).unwrap()["sidebar"], false);
    assert_eq!(
        set(false, cx).unwrap()["sidebar"],
        false,
        "repeat is a no-op"
    );
    assert_eq!(set(true, cx).unwrap()["sidebar"], true);

    let state = call(
        window,
        caller,
        "control.panel",
        json!({"panel":"right","open":true,"tab":"graph"}),
        cx,
    )
    .unwrap();
    assert_eq!(
        (&state["right"], &state["rightTab"]),
        (&json!(true), &json!("graph"))
    );
    let state = call(
        window,
        caller,
        "control.panel",
        json!({"panel":"right","open":false}),
        cx,
    )
    .unwrap();
    assert_eq!(state["right"], false);

    let state = call(
        window,
        caller,
        "control.panel",
        json!({"panel":"notes","open":true}),
        cx,
    )
    .unwrap();
    assert_eq!(state["view"], "Notes");
    let state = call(
        window,
        caller,
        "control.panel",
        json!({"panel":"notes","open":false}),
        cx,
    )
    .unwrap();
    assert_eq!(state["view"], Value::Null);
}

#[gpui::test]
fn a_named_action_runs_like_its_shortcut(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let before = call(window, caller, "control.state", json!({}), cx).unwrap();
    let after = call(
        window,
        caller,
        "control.run",
        json!({"action":"workspace::ToggleSidebar"}),
        cx,
    )
    .unwrap();
    assert_ne!(before["sidebar"], after["sidebar"]);
    let error = call(
        window,
        caller,
        "control.run",
        json!({"action":"workspace::NoSuchAction"}),
        cx,
    )
    .unwrap_err();
    assert!(!error.is_empty());
}

#[gpui::test]
fn actions_can_be_discovered(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let listed = call(window, caller, "control.actions", json!({}), cx).unwrap();
    let names = listed["actions"].as_array().unwrap();
    assert!(names.contains(&json!("workspace::ToggleSidebar")));
    assert!(names.iter().all(|name| {
        let name = name.as_str().unwrap();
        name.starts_with("workspace::") || name.starts_with("application::")
    }));
    let listed = call(
        window,
        caller,
        "control.actions",
        json!({"filter":"terminal"}),
        cx,
    )
    .unwrap();
    assert!(
        listed["actions"]
            .as_array()
            .unwrap()
            .contains(&json!("workspace::ToggleTerminal"))
    );
}

#[gpui::test]
fn read_only_threads_can_look_but_not_touch(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, _| v.settings.access = AccessMode::ReadOnly);
        })
        .unwrap();
    assert!(call(window, caller, "control.state", json!({}), cx).is_ok());
    assert!(call(window, caller, "control.threads", json!({}), cx).is_ok());
    let denied = call(
        window,
        caller,
        "control.run",
        json!({"action":"workspace::ToggleSidebar"}),
        cx,
    );
    assert!(denied.unwrap_err().contains("access policy"));
    let denied = call(
        window,
        caller,
        "control.panel",
        json!({"panel":"sidebar","open":false}),
        cx,
    );
    assert!(denied.unwrap_err().contains("access policy"));
}

#[gpui::test]
fn threads_stay_inside_the_callers_project(cx: &mut TestAppContext) {
    let (window, other, caller) = setup(cx);
    let peer = sibling(window, "Peer work", cx);
    let listed = call(window, caller, "control.threads", json!({}), cx).unwrap();
    let ids: Vec<_> = listed["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|thread| thread["threadId"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [peer], "not itself, not another project");
    assert!(listed["threads"][0].get("reply").is_none());

    assert!(call(window, caller, "control.read", json!({"threadId":peer}), cx).is_ok());
    let denied = call(
        window,
        caller,
        "control.read",
        json!({"threadId":other}),
        cx,
    );
    assert_eq!(denied.unwrap_err(), "Thread is not in this project.");
    let missing = call(
        window,
        caller,
        "control.read",
        json!({"threadId":999_999}),
        cx,
    );
    assert_eq!(missing.unwrap_err(), "Thread is not in this project.");
}

#[gpui::test]
fn thread_lists_filter_and_limit(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    sibling(window, "Alpha review", cx);
    sibling(window, "Beta review", cx);
    sibling(window, "Gamma", cx);
    let listed = call(
        window,
        caller,
        "control.threads",
        json!({"query":"REVIEW","limit":1}),
        cx,
    )
    .unwrap();
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["threads"].as_array().unwrap().len(), 1);
}

#[gpui::test]
fn mono_conversations_are_not_readable(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let private = sibling(window, "Private", cx);
    window
        .update(cx, |shell, _, cx| {
            let monos = shell.monos.clone().expect("monos");
            monos.update(cx, |monos, _| monos.roster.hidden_sessions.push(private));
        })
        .unwrap();
    let listed = call(window, caller, "control.threads", json!({}), cx).unwrap();
    assert_eq!(listed["total"], 0);
    let denied = call(
        window,
        caller,
        "control.read",
        json!({"threadId":private}),
        cx,
    );
    assert_eq!(denied.unwrap_err(), "Thread is not in this project.");
}

#[gpui::test]
fn the_action_list_hides_what_app_run_cannot_build(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let names = call(window, caller, "control.actions", json!({}), cx).unwrap();
    let names = names["actions"].as_array().unwrap();
    assert!(names.iter().any(|name| name == "workspace::ToggleSidebar"));
    assert!(!names.iter().any(|name| name == "workspace::ActivateTab"));
    assert!(!names.iter().any(|name| name == "application::OpenUrl"));
    let error = call(
        window,
        caller,
        "control.run",
        json!({"action":"application::OpenUrl"}),
        cx,
    );
    assert!(error.unwrap_err().contains("cannot be built"));
}

#[gpui::test]
fn a_thread_opens_beside_the_caller_and_closes_without_stopping(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    window
        .update(cx, |shell, window, cx| shell.open(caller, window, cx))
        .unwrap();
    cx.run_until_parked();

    let input = json!({"threadId":target,"placement":"right"});
    let state = call(window, caller, "control.open", input.clone(), cx).unwrap();
    assert_eq!(state["activeThread"], target);
    assert_eq!(state["panes"].as_array().unwrap().len(), 2);
    // Repeating it keeps the one pane.
    let state = call(window, caller, "control.open", input, cx).unwrap();
    assert_eq!(state["panes"].as_array().unwrap().len(), 2);

    let state = call(
        window,
        caller,
        "control.close",
        json!({"threadId":target}),
        cx,
    )
    .unwrap();
    assert_eq!(state["panes"], json!([caller]));
    window
        .update(cx, |shell, _, _| {
            assert!(shell.workspace.tab_of(target).is_none());
            assert!(shell.session_summaries.contains_key(&target));
            assert!(!shell.session_summaries[&target].archived);
        })
        .unwrap();
    // Closing twice is harmless.
    assert!(
        call(
            window,
            caller,
            "control.close",
            json!({"threadId":target}),
            cx
        )
        .is_ok()
    );
}

#[gpui::test]
fn pane_tools_stay_in_the_project_and_out_of_read_only_threads(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    let elsewhere = window
        .update(cx, |shell, _, cx| {
            let summary = locked(&shell.store)
                .unwrap()
                .create("claude", "/work/other")
                .unwrap();
            let id = summary.id;
            shell.add(summary, cx);
            id
        })
        .unwrap();
    cx.run_until_parked();
    for action in ["control.open", "control.close"] {
        let error = call(window, caller, action, json!({"threadId":elsewhere}), cx).unwrap_err();
        assert_eq!(error, NOT_IN_PROJECT);
    }
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |view, _| view.app_read_only = true);
        })
        .unwrap();
    assert!(
        call(
            window,
            caller,
            "control.open",
            json!({"threadId":target}),
            cx
        )
        .is_err()
    );
    assert!(call(window, caller, "control.files", json!({}), cx).is_ok());
}

#[gpui::test]
fn reading_a_thread_leaves_a_capped_record(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    for _ in 0..25 {
        call(
            window,
            caller,
            "control.read",
            json!({"threadId":target}),
            cx,
        )
        .unwrap();
    }
    window
        .update(cx, |shell, _, _| {
            let reads = locked(&shell.store).unwrap().thread_reads(target).unwrap();
            assert_eq!(reads.len(), 20);
            assert_eq!(reads[19]["reader"], caller);
        })
        .unwrap();
}

#[test]
fn changed_files_line_up_across_checkouts() {
    let root = Path::new("/wt/one");
    let edited = [
        "/wt/one/src/a.rs".to_owned(),
        "src/b.rs".to_owned(),
        "/elsewhere/c.rs".to_owned(),
    ];
    assert_eq!(
        changed_files(&edited, root, &[]),
        ["/elsewhere/c.rs", "src/b.rs", "src/a.rs"]
    );
    let own = Path::new("/repo");
    let file = checkout_path("/repo/src/a.rs", own);
    assert_eq!(changed_files(&edited, root, &[file]), ["src/a.rs"]);
    let dir = checkout_path("src", own);
    assert_eq!(
        changed_files(&edited, root, &[dir]),
        ["src/b.rs", "src/a.rs"]
    );
    let near = checkout_path("src/a", own);
    assert!(changed_files(&edited, root, &[near]).is_empty());
}

#[gpui::test]
fn every_chat_reaches_project_actions_but_not_what_only_monos_may_do(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    assert!(call(window, caller, "folders.list", json!({}), cx).is_ok());
    assert!(call(window, caller, "notes.list", json!({}), cx).is_ok());
    let input = json!({"sessionId":target});
    assert_eq!(
        call(window, caller, "sessions.stop", input.clone(), cx).unwrap()["stopped"],
        true
    );
    for (action, input) in [
        ("sessions.delete", json!({"sessionId":target})),
        ("sessions.send", json!({"sessionId":target,"prompt":"x"})),
        ("sessions.start", json!({"prompt":"x"})),
    ] {
        let error = call(window, caller, action, input, cx).unwrap_err();
        assert!(error.contains("Unknown app action"), "{action}: {error}");
    }
    // Managing yourself stays refused.
    let error = call(
        window,
        caller,
        "sessions.archive",
        json!({"sessionId":caller}),
        cx,
    );
    assert!(error.unwrap_err().contains("Only other"));
}

#[gpui::test]
fn staging_adds_to_the_composer_and_never_replaces_it(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    let stage = |text: &str, cx: &mut TestAppContext| {
        call(
            window,
            caller,
            "control.compose",
            json!({"threadId":target,"text":text}),
            cx,
        )
    };
    assert_eq!(stage("First", cx).unwrap()["text"], "First");
    assert_eq!(stage("Second", cx).unwrap()["text"], "First\n\nSecond");
    let read = call(
        window,
        caller,
        "control.composer",
        json!({"threadId":target}),
        cx,
    );
    assert_eq!(read.unwrap()["text"], "First\n\nSecond");
    window
        .update(cx, |shell, _, cx| {
            let view = shell.sessions[&target].read(cx);
            assert_eq!(view.input.read(cx).content.as_ref(), "First\n\nSecond");
            assert!(!view.busy);
        })
        .unwrap();
}

fn set(
    window: WindowHandle<Shell>,
    caller: i64,
    input: Value,
    cx: &mut TestAppContext,
) -> mpsc::Receiver<Result<Value, String>> {
    let (reply, result) = mpsc::channel();
    window
        .update(cx, |shell, window, cx| {
            shell.handle_app_call(
                Call {
                    session: caller,
                    request_id: "set".into(),
                    action: "control.set".into(),
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
}

fn access(window: WindowHandle<Shell>, id: i64, cx: &mut TestAppContext) -> AccessMode {
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].read(cx).settings.access
        })
        .unwrap()
}

#[gpui::test]
fn only_the_person_can_raise_a_threads_access(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    let input = json!({"threadId":target,"access":"full-access"});

    let declined = set(window, caller, input.clone(), cx);
    assert!(cx.has_pending_prompt());
    assert!(declined.try_recv().is_err());
    assert_eq!(access(window, target, cx), AccessMode::Supervised);
    cx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();
    assert!(declined.recv().unwrap().unwrap_err().contains("declined"));
    assert_eq!(access(window, target, cx), AccessMode::Supervised);

    let allowed = set(window, caller, input, cx);
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert_eq!(allowed.recv().unwrap().unwrap()["access"], "full-access");
    assert_eq!(access(window, target, cx), AccessMode::FullAccess);
    // A person's default for new chats doesn't follow an agent's change.
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(
                crate::conversation::default_access(cx),
                AccessMode::Supervised
            );
            let _ = shell;
        })
        .unwrap();

    // Lowering asks nothing.
    let lowered = set(
        window,
        caller,
        json!({"threadId":target,"access":"read-only"}),
        cx,
    );
    assert!(!cx.has_pending_prompt());
    assert_eq!(lowered.recv().unwrap().unwrap()["access"], "read-only");
}

#[gpui::test]
fn a_read_only_thread_cannot_ask_for_access(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let target = sibling(window, "Target", cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |view, _| view.app_read_only = true);
        })
        .unwrap();
    let input = json!({"threadId":target,"access":"full-access"});
    assert!(set(window, caller, input, cx).recv().unwrap().is_err());
    assert!(!cx.has_pending_prompt());
    let change = json!({"set":{"theme":"light"}});
    assert!(call(window, caller, "control.configure", change, cx).is_err());
    assert!(call(window, caller, "control.settings", json!({}), cx).is_ok());
}

#[gpui::test]
fn a_changed_setting_reaches_the_app_and_settings_page(cx: &mut TestAppContext) {
    let (window, _, caller) = setup(cx);
    let change = json!({"set":{"theme":"light"}});
    let state = call(window, caller, "control.configure", change, cx).unwrap();
    assert_eq!(state["settings"]["theme"], "light");
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.workspace.preferences.theme, "light");
            assert_eq!(cx.global::<crate::settings::Preferences>().theme, "light");
        })
        .unwrap();
}
