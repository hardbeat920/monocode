use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::{agent::Settings, transcript::Row};
use std::sync::mpsc;

fn setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
    let (window, child, lead) = crate::shell::tests::window(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.thread_parents.insert(child, lead);
            shell.started_by.insert(child, lead);
            locked(&shell.store)
                .unwrap()
                .save_thread_parent(child, lead)
                .unwrap();
            shell.sessions[&lead].update(cx, |v, cx| {
                v.install_transfer(Settings::default(), None, cx)
            });
            shell.sessions[&child].update(cx, |v, cx| {
                v.install_transfer(Settings::default(), None, cx);
                v.driven_by = Some(lead);
                v.managed = true;
                v.busy = true;
            });
            shell.thread_session_changed(child, cx);
        })
        .unwrap();
    cx.run_until_parked();
    (window, child, lead)
}

fn call(
    window: WindowHandle<Shell>,
    caller: i64,
    request: &str,
    action: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> mpsc::Receiver<Result<Value, String>> {
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
}

#[gpui::test]
fn threads_are_top_level_owned_and_available_without_operator(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    let denied = call(window, child, "child-list", "threads.list", json!({}), cx)
        .try_recv()
        .unwrap()
        .unwrap_err();
    assert!(denied.contains("top-level"));
    let denied = call(
        window,
        lead,
        "not-child",
        "threads.read",
        json!({"threadId":lead}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap_err();
    assert!(denied.contains("own children"));
    let value = call(window, lead, "list", "threads.list", json!({}), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(value["threads"][0]["threadId"], child);
    assert_eq!(value["threads"][0]["status"], "working");
    let denied = call(
        window,
        lead,
        "busy",
        "threads.send",
        json!({"threadId":child,"prompt":"Explain"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap_err();
    assert!(denied.contains("busy"));
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&lead].update(cx, |v, _| {
                v.settings.access = monocode_core::agent::AccessMode::ReadOnly
            })
        })
        .unwrap();
    assert!(
        call(
            window,
            lead,
            "read-only-list",
            "threads.list",
            json!({}),
            cx
        )
        .try_recv()
        .unwrap()
        .is_ok()
    );
    assert!(
        call(
            window,
            lead,
            "read-only-start",
            "threads.start",
            json!({"prompt":"Review"}),
            cx
        )
        .try_recv()
        .unwrap()
        .unwrap_err()
        .contains("read-only")
    );
}

#[gpui::test]
fn threads_wait_times_out_then_resolves_with_the_child_reply(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    let result = call(
        window,
        lead,
        "timeout",
        "threads.wait",
        json!({"threadId":child,"timeoutSeconds":2}),
        cx,
    );
    assert!(result.try_recv().is_err());
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(result.try_recv().unwrap().unwrap()["status"], "working");
    window
        .update(cx, |shell, _, _| {
            shell
                .orchestration_summaries
                .get_mut(&lead)
                .unwrap()
                .tasks
                .push(SummaryTask {
                    session: Some(-1),
                    title: "Earlier worker".into(),
                    status: "running".into(),
                    ..Default::default()
                });
        })
        .unwrap();
    let result = call(
        window,
        lead,
        "reply",
        "threads.wait",
        json!({"threadId":child}),
        cx,
    );
    assert!(result.try_recv().is_err());
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&child].update(cx, |v, cx| {
                v.busy = false;
                v.transcript.update(cx, |t, cx| {
                    t.replace_history(
                        vec![
                            Row {
                                role: "user".into(),
                                text: "Review".into(),
                                first: true,
                                sent_at: None,
                            },
                            Row {
                                role: "assistant".into(),
                                text: "Ownership is enforced".into(),
                                first: true,
                                sent_at: None,
                            },
                        ],
                        cx,
                    )
                });
                cx.emit(SessionChanged);
            });
        })
        .unwrap();
    cx.run_until_parked();
    let reply = result.try_recv().unwrap().unwrap();
    assert_eq!(reply["status"], "idle");
    assert_eq!(reply["success"], true);
    assert_eq!(reply["reply"], "Ownership is enforced");
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.sessions[&child].read(cx).driven_by, Some(lead));
            assert!(!shell.orchestration_summaries[&lead].live);
        })
        .unwrap();
}

#[gpui::test]
fn threads_stopping_lead_stops_child_and_releases_waiters(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    let result = call(
        window,
        lead,
        "wait-stop",
        "threads.wait",
        json!({"threadId":child}),
        cx,
    );
    assert!(result.try_recv().is_err());
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&lead].update(cx, |v, cx| {
                v.busy = true;
                v.stop_if_busy(cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        result
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("Lead thread was stopped")
    );
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.sessions[&child].read(cx).busy);
            assert_eq!(shell.sessions[&child].read(cx).driven_by, Some(lead));
            assert!(!shell.thread_waiters.contains_key(&child));
        })
        .unwrap();
}

#[gpui::test]
fn restored_children_name_their_lead_as_the_sender(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions.remove(&child);
            shell.thread_parents = locked(&shell.store).unwrap().thread_parents().unwrap();
            let child_view = shell.ensure_session(child, cx).unwrap();
            assert_eq!(child_view.read(cx).driven_by, Some(lead));
            assert!(child_view.read(cx).managed);
            assert_eq!(shell.sessions[&lead].read(cx).driven_by, None);
            assert!(
                child_view
                    .read(cx)
                    .transcript
                    .read(cx)
                    .senders
                    .contains_key(&lead)
            );
            assert!(
                shell.sessions[&lead]
                    .read(cx)
                    .transcript
                    .read(cx)
                    .senders
                    .is_empty()
            );
        })
        .unwrap();
}

#[gpui::test]
fn threads_start_is_idempotent_and_marks_the_child_before_dispatch(cx: &mut TestAppContext) {
    let (window, _, lead) = crate::shell::tests::window(cx);
    let missing = std::env::temp_dir()
        .join(monocode_core::mono::new_id())
        .join("missing-project");
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&lead].update(cx, |v, cx| {
                v.install_transfer(Settings::default(), None, cx);
                v.summary.cwd = missing.to_string_lossy().into_owned();
            });
        })
        .unwrap();
    let input = json!({"prompt":"Review ownership","harness":"codex","title":"Ownership review"});
    let first = call(
        window,
        lead,
        "start-once",
        "threads.start",
        input.clone(),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let second = call(window, lead, "start-once", "threads.start", input, cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    let child = first["threadId"].as_i64().unwrap();
    window
        .update(cx, |shell, _, cx| {
            assert_eq!(shell.thread_parents.get(&child), Some(&lead));
            assert_eq!(shell.started_by.get(&child), Some(&lead));
            assert_eq!(shell.sessions[&child].read(cx).driven_by, Some(lead));
            assert!(shell.sessions[&child].read(cx).managed);
            assert_eq!(shell.subagent_lead(child), Some(lead));
            assert_eq!(
                locked(&shell.store)
                    .unwrap()
                    .thread_parents()
                    .unwrap()
                    .get(&child),
                Some(&lead)
            );
            assert!(
                shell.orchestration_summaries[&lead]
                    .tasks
                    .iter()
                    .any(|t| t.session == Some(child))
            );
        })
        .unwrap();
}

#[gpui::test]
fn threads_new_opens_a_top_level_thread_the_lead_does_not_own(cx: &mut TestAppContext) {
    let (window, _, lead) = crate::shell::tests::window(cx);
    let missing = std::env::temp_dir()
        .join(monocode_core::mono::new_id())
        .join("missing-project");
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&lead].update(cx, |v, cx| {
                v.install_transfer(Settings::default(), None, cx);
                v.summary.cwd = missing.to_string_lossy().into_owned();
            });
        })
        .unwrap();
    // No Operator grant: a lead opens a new thread through its thread tools.
    let value = call(
        window,
        lead,
        "new-once",
        "threads.new",
        json!({"prompt":"Plan the drafts","title":"Drafts","draft":true}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert!(value["threadId"].is_null());
    let id = value["sessionId"].as_i64().unwrap();
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.thread_parents.contains_key(&id));
            assert!(!shell.sessions[&id].read(cx).managed);
            assert!(
                !locked(&shell.store)
                    .unwrap()
                    .thread_parents()
                    .unwrap()
                    .contains_key(&id)
            );
            assert!(
                shell
                    .orchestration_summaries
                    .get(&lead)
                    .is_none_or(|s| s.tasks.iter().all(|t| t.session != Some(id)))
            );
        })
        .unwrap();
    let denied = call(
        window,
        lead,
        "not-child",
        "threads.send",
        json!({"threadId":id,"prompt":"Go"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap_err();
    assert!(denied.contains("own children"));
}

#[gpui::test]
fn a_subagent_opens_read_only_beside_its_lead(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    let other = window
        .update(cx, |shell, window, cx| {
            shell.sessions[&child].update(cx, |v, _| v.busy = false);
            shell.thread_session_changed(child, cx);
            let other = locked(&shell.store)
                .unwrap()
                .create("codex", &shell.workspace.project)
                .unwrap();
            let other_id = other.id;
            shell.thread_parents.insert(other_id, lead);
            shell.started_by.insert(other_id, lead);
            shell.add(other, cx);
            shell.thread_session_changed(other_id, cx);
            shell.open(lead, window, cx);
            shell.open(child, window, cx);
            other_id
        })
        .unwrap();
    cx.run_until_parked();
    cx.refresh().unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    window
        .update(visual, |shell, _, _| {
            let tab = shell.workspace.tab_of(lead).unwrap();
            assert_eq!(
                shell.workspace.layout(tab).leaf_ids(),
                [lead, child],
                "beside its lead, in the lead's tab"
            );
            assert_eq!(shell.workspace.active, Some(child));
        })
        .unwrap();
    // The lead keeps its composer; the subagent only has its lock and Stop.
    assert!(visual.debug_bounds("thread-locked-composer").is_some());
    assert!(visual.debug_bounds("composer-box").is_some());
    assert!(visual.debug_bounds("subagent-strip").is_some());
    let chip: &'static str = Box::leak(format!("subagent-chip-{other}").into_boxed_str());
    let chip = visual
        .debug_bounds(chip)
        .expect("the lead's strip lists it");
    visual.simulate_click(chip.center(), Default::default());
    visual.run_until_parked();
    window
        .update(visual, |shell, _, cx| {
            let tab = shell.workspace.tab_of(lead).unwrap();
            assert_eq!(
                shell.workspace.layout(tab).leaf_ids(),
                [lead, other],
                "one subagent at a time takes the pane"
            );
            assert!(shell.workspace.tab_of(child).is_none());
            let strip = shell.subagent_strip(lead, cx);
            assert_eq!(
                strip.iter().map(|s| s.id).collect::<Vec<_>>(),
                [child, other]
            );
        })
        .unwrap();
}

#[gpui::test]
fn archiving_a_lead_archives_its_subagents_and_restoring_brings_them_back(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&child].update(cx, |v, _| v.busy = false);
            shell.thread_session_changed(child, cx);
            shell.toggle_archive(lead, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.session_summaries[&lead].archived);
            assert!(
                shell.session_summaries[&child].archived,
                "it goes with its lead"
            );
            shell.toggle_archive(lead, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .read_with(cx, |shell, _| {
            assert!(!shell.session_summaries[&lead].archived);
            assert!(
                !shell.session_summaries[&child].archived,
                "and comes back with it"
            );
        })
        .unwrap();
}

#[gpui::test]
fn threads_start_does_not_launch_after_the_lead_is_stopped(cx: &mut TestAppContext) {
    let (window, _, lead) = crate::shell::tests::window(cx);
    let (reply, result) = mpsc::channel();
    window
        .update(cx, |shell, window, cx| {
            shell.sessions[&lead].update(cx, |v, cx| {
                v.install_transfer(Settings::default(), None, cx)
            });
            shell.handle_app_call(
                Call {
                    session: lead,
                    request_id: "cancel-launch".into(),
                    action: "threads.start".into(),
                    input: json!({"prompt":"Review","harness":"codex"}),
                    reply,
                },
                window,
                cx,
            );
            shell.sessions[&lead].update(cx, |v, cx| {
                v.busy = true;
                v.stop_if_busy(cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert!(result.try_recv().unwrap().unwrap_err().contains("stopped"));
    window
        .update(cx, |shell, _, _| {
            assert!(!shell.thread_parents.values().any(|parent| *parent == lead))
        })
        .unwrap();
}

#[gpui::test]
fn threads_stopping_an_idle_lead_stops_its_busy_children(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    let result = call(
        window,
        lead,
        "idle-lead-wait",
        "threads.wait",
        json!({"threadId":child}),
        cx,
    );
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&lead].update(cx, |v, cx| {
                assert!(!v.busy);
                v.stop_if_busy(cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    assert!(result.try_recv().unwrap().unwrap_err().contains("stopped"));
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.sessions[&child].read(cx).busy)
        })
        .unwrap();
}

#[gpui::test]
fn threads_pending_send_cannot_outlive_stop_or_a_new_lead_turn(cx: &mut TestAppContext) {
    for stop in [true, false] {
        let (window, child, lead) = setup(cx);
        let (reply, result) = mpsc::channel();
        window
            .update(cx, |shell, window, cx| {
                shell.sessions[&child].update(cx, |v, _| v.busy = false);
                shell.handle_app_call(
                    Call {
                        session: lead,
                        request_id: "stale-send".into(),
                        action: "threads.send".into(),
                        input: json!({"threadId":child,"prompt":"Do not deliver"}),
                        reply,
                    },
                    window,
                    cx,
                );
                shell.sessions[&lead].update(cx, |v, cx| {
                    if stop {
                        v.stop_if_busy(cx)
                    } else {
                        v.submission_generation += 1
                    }
                });
            })
            .unwrap();
        cx.run_until_parked();
        assert!(
            result
                .try_recv()
                .unwrap()
                .unwrap_err()
                .contains("stopped or started another turn")
        );
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&child].read(cx).busy)
            })
            .unwrap();
    }
}

#[gpui::test]
fn threads_close_archives_a_finished_child_and_takes_it_off_the_rail(cx: &mut TestAppContext) {
    let (window, child, lead) = setup(cx);
    let close = |request: &str, cx: &mut TestAppContext| {
        call(
            window,
            lead,
            request,
            "threads.close",
            json!({"threadId":child}),
            cx,
        )
        .try_recv()
        .unwrap()
    };
    assert!(close("busy", cx).unwrap_err().contains("stop it first"));
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&child].update(cx, |v, cx| {
                v.busy = false;
                cx.emit(SessionChanged);
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(close("close", cx).unwrap()["closed"], true);
    cx.run_until_parked();
    assert_eq!(close("again", cx).unwrap()["closed"], true);
    let listed = call(window, lead, "list", "threads.list", json!({}), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(listed["threads"], json!([]));
    let send = call(
        window,
        lead,
        "send",
        "threads.send",
        json!({"threadId":child,"prompt":"More"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap_err();
    assert!(send.contains("closed"));
    window
        .update(cx, |shell, _, cx| {
            let session = locked(&shell.store)
                .unwrap()
                .sessions()
                .unwrap()
                .into_iter()
                .find(|s| s.id == child)
                .unwrap();
            assert!(session.archived);
            assert!(
                !shell.orchestration_summaries[&lead]
                    .tasks
                    .iter()
                    .any(|t| t.session == Some(child))
            );
            let transcript = shell.sessions[&lead].read(cx).transcript.clone();
            assert!(transcript.read(cx).thread_hidden(child));
            // Restoring it from the archive puts it back on the lead's rail.
            shell.toggle_archive(child, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            assert!(
                shell.orchestration_summaries[&lead]
                    .tasks
                    .iter()
                    .any(|t| t.session == Some(child))
            );
            // Removing it from the lead's spawn tree closes it too.
            let transcript = shell.sessions[&lead].read(cx).transcript.clone();
            transcript.update(cx, |t, cx| t.dismiss_threads(&[child], cx));
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |shell, _, cx| {
            assert!(shell.session_metadata(child, cx).summary.archived);
        })
        .unwrap();
}
