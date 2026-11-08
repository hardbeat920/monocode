use super::*;

fn completion_setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
    let (window, other, caller) = setup(cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, _| {
                v.summary.provider = "fixture".into();
                v.summary.has_messages = true;
                v.busy = true;
                v.mono_turn = 41;
            });
        })
        .unwrap();
    (window, other, caller)
}

fn completion_worker(
    window: WindowHandle<Shell>,
    caller: i64,
    name: &str,
    cx: &mut TestAppContext,
) -> i64 {
    completion_worker_in_group(window, caller, name, None, cx)
}

fn completion_worker_in_group(
    window: WindowHandle<Shell>,
    caller: i64,
    name: &str,
    label: Option<&str>,
    cx: &mut TestAppContext,
) -> i64 {
    let id = window
        .update(cx, |shell, _, cx| {
            let mut store = locked(&shell.store).unwrap();
            let mut summary = store
                .create("codex", &shell.sessions[&caller].read(cx).summary.cwd)
                .unwrap();
            store.title(summary.id, name).unwrap();
            summary.title = name.into();
            summary.provider = "fixture".into();
            summary.has_messages = true;
            store
                .write_rows(
                    summary.id,
                    0,
                    &[monocode_core::transcript::Row {
                        role: "assistant".into(),
                        text: format!("{name} response"),
                        first: true,
                        sent_at: None,
                    }],
                )
                .unwrap();
            let id = summary.id;
            drop(store);
            shell.add(summary, cx);
            shell.sessions[&id].update(cx, |v, cx| v.load(cx));
            id
        })
        .unwrap();
    cx.run_until_parked();
    let mut input = json!({"sessionId":id,"prompt":"Fixture work","notifyOnComplete":true});
    if let Some(label) = label {
        input["notifyGroup"] = json!(label);
    }
    call(window, caller, name, "sessions.send", input, cx).unwrap();
    // Hold the worker at the provider boundary after exercising the authorised send.
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |v, _| v.busy = true);
            let group = label
                .map(|label| OperatorCompletionGroup::Label(label.trim().into()))
                .unwrap_or(OperatorCompletionGroup::Turn(
                    shell.sessions[&caller].read(cx).mono_turn,
                ));
            assert_eq!(
                shell.operator_subscriptions[&caller]
                    .iter()
                    .find(|s| s.session == id)
                    .unwrap()
                    .group,
                group
            );
            shell
                .arm_operator_completion(caller, group, id, name.into())
                .unwrap();
        })
        .unwrap();
    id
}

fn finish_completion_worker(window: WindowHandle<Shell>, id: i64, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&id].update(cx, |v, cx| {
                v.busy = false;
                v.install_transfer(v.settings.clone(), None, cx);
                let rows = vec![monocode_core::transcript::Row {
                    role: "assistant".into(),
                    text: format!("Worker {id} final response"),
                    first: true,
                    sent_at: None,
                }];
                let start = v.transcript.read(cx).source_rows().len();
                v.transcript
                    .update(cx, |t, cx| t.patch(start, rows.clone(), cx));
                locked(&shell.store)
                    .unwrap()
                    .write_rows(id, start, &rows)
                    .unwrap();
                cx.emit(SessionChanged);
            });
        })
        .unwrap();
    cx.run_until_parked();
}

fn end_completion_origin(window: WindowHandle<Shell>, caller: i64, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, cx| {
                v.busy = false;
                cx.emit(SessionChanged);
            });
        })
        .unwrap();
    cx.run_until_parked();
}

fn completion_prompts(
    window: WindowHandle<Shell>,
    caller: i64,
    cx: &mut TestAppContext,
) -> Vec<String> {
    window
        .update(cx, |shell, _, _| {
            locked(&shell.store)
                .unwrap()
                .load(caller)
                .unwrap()
                .rows
                .into_iter()
                .filter(|row| {
                    row.role == "mono-system" && row.text.starts_with("[Delegated work completed]")
                })
                .map(|row| row.text)
                .collect()
        })
        .unwrap()
}

#[gpui::test]
fn operator_completion_groups_wait_for_every_worker_and_resume_only_the_origin(
    cx: &mut TestAppContext,
) {
    let (window, other, caller) = completion_setup(cx);
    let first = completion_worker(window, caller, "first", cx);
    let second = completion_worker(window, caller, "second", cx);
    end_completion_origin(window, caller, cx);
    finish_completion_worker(window, first, cx);
    assert!(completion_prompts(window, caller, cx).is_empty());
    finish_completion_worker(window, second, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!("Session {first}: first\nOutcome: completed")));
    assert!(prompts[0].contains(&format!("Session {second}: second\nOutcome: completed")));
    assert!(prompts[0].contains(&format!("Worker {second} final response")));
    assert!(prompts[0].contains("completion is not verification"));
    assert!(completion_prompts(window, other, cx).is_empty());
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.sessions[&first].read(cx).settings.operator_enabled);
            assert!(!shell.sessions[&second].read(cx).settings.operator_enabled);
            assert!(
                locked(&shell.store)
                    .unwrap()
                    .operator_subscriptions()
                    .unwrap()[&caller]
                    .iter()
                    .all(|s| s.delivered)
            );
        })
        .unwrap();
}

#[gpui::test]
fn operator_completion_waits_for_busy_origin_and_survives_duplicate_events_and_reload(
    cx: &mut TestAppContext,
) {
    let (window, _, caller) = completion_setup(cx);
    let child = completion_worker(window, caller, "busy-origin", cx);
    finish_completion_worker(window, child, cx);
    assert!(completion_prompts(window, caller, cx).is_empty());
    window
        .update(cx, |shell, _, cx| {
            shell.operator_subscriptions = locked(&shell.store)
                .unwrap()
                .session_sidebar_metadata(&[])
                .operator_subscriptions;
            shell.restore_operator_completions(cx);
        })
        .unwrap();
    end_completion_origin(window, caller, cx);
    assert_eq!(completion_prompts(window, caller, cx).len(), 1);
    window
        .update(cx, |shell, _, cx| {
            shell.operator_subscriptions = locked(&shell.store)
                .unwrap()
                .operator_subscriptions()
                .unwrap();
            shell.restore_operator_completions(cx);
            for _ in 0..4 {
                shell.sessions[&child].update(cx, |_, cx| cx.emit(SessionChanged));
                shell.sessions[&caller].update(cx, |_, cx| cx.emit(SessionChanged));
            }
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(completion_prompts(window, caller, cx).len(), 1);
}

#[gpui::test]
fn operator_completion_reports_failed_and_ui_stopped_workers(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let failed = completion_worker(window, caller, "failed-worker", cx);
    let stopped = completion_worker(window, caller, "stopped-worker", cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&failed].update(cx, |v, cx| {
                v.busy = false;
                v.quick_submit(
                    "Fail without starting a provider".into(),
                    v.settings.clone(),
                    cx,
                );
            });
            shell.sessions[&stopped].update(cx, |v, cx| v.stop_if_busy(cx));
        })
        .unwrap();
    cx.run_until_parked();
    end_completion_origin(window, caller, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!("Session {failed}: failed-worker\nOutcome: failed")));
    assert!(prompts[0].contains(&format!(
        "Session {stopped}: stopped-worker\nOutcome: stopped"
    )));
}

#[gpui::test]
fn operator_completion_preserves_drafts_and_loads_an_unopened_origin(cx: &mut TestAppContext) {
    for reopen in [false, true] {
        let (window, _, caller) = completion_setup(cx);
        let child = completion_worker(window, caller, "draft-origin", cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&caller].update(cx, |v, cx| {
                    v.busy = false;
                    v.input
                        .update(cx, |input, cx| input.set_text("My unsent text".into(), cx));
                });
            })
            .unwrap();
        finish_completion_worker(window, child, cx);
        assert!(completion_prompts(window, caller, cx).is_empty());
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&caller].update(cx, |v, cx| {
                    assert_eq!(v.input.read(cx).content, "My unsent text");
                    v.input
                        .update(cx, |input, cx| input.set_text(String::new(), cx));
                });
                if reopen {
                    shell.sessions.remove(&caller);
                    shell.session_summaries.get_mut(&caller).unwrap().provider = "fixture".into();
                    let settings = Settings {
                        operator_enabled: true,
                        ..Default::default()
                    };
                    locked(&shell.store)
                        .unwrap()
                        .configure_agent(caller, "codex", &settings)
                        .unwrap();
                    shell.check_operator_completions(cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(completion_prompts(window, caller, cx).len(), 1);
    }
}

#[gpui::test]
fn operator_completion_dismisses_only_the_callers_workers_and_reports_removal(
    cx: &mut TestAppContext,
) {
    let (window, _, caller) = completion_setup(cx);
    let dismissed = completion_worker(window, caller, "dismissed", cx);
    let removed = completion_worker(window, caller, "removed", cx);
    call(
        window,
        caller,
        "stop-dismissed",
        "sessions.stop",
        json!({"sessionId":dismissed}),
        cx,
    )
    .unwrap();
    window
        .update(cx, |shell, window, cx| {
            shell.delete_session(removed, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    end_completion_origin(window, caller, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    assert!(!prompts[0].contains("dismissed"));
    assert!(prompts[0].contains(&format!("Session {removed}: removed\nOutcome: removed")));
}

#[gpui::test]
fn operator_completion_drops_archived_and_deleted_origins(cx: &mut TestAppContext) {
    for delete in [false, true] {
        let (window, _, caller) = completion_setup(cx);
        let child = completion_worker(window, caller, "abandoned-origin", cx);
        window
            .update(cx, |shell, window, cx| {
                if delete {
                    shell.delete_session(caller, window, cx);
                } else {
                    shell.toggle_archive(caller, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        finish_completion_worker(window, child, cx);
        window
            .update(cx, |shell, _, _| {
                assert!(shell.error.is_none(), "{:?}", shell.error);
                assert!(
                    !shell.operator_subscriptions.contains_key(&caller),
                    "delete={delete}, error={:?}, summary={:?}, deleting={:?}",
                    shell.error,
                    shell.session_summaries.get(&caller),
                    shell.deleting
                );
                assert!(
                    locked(&shell.store)
                        .unwrap()
                        .operator_subscriptions()
                        .unwrap()
                        .get(&caller)
                        .is_none_or(Vec::is_empty)
                );
                if !delete {
                    assert!(
                        locked(&shell.store)
                            .unwrap()
                            .load(caller)
                            .unwrap()
                            .rows
                            .iter()
                            .all(|row| row.role != "mono-system")
                    );
                }
            })
            .unwrap();
    }
}

#[gpui::test]
fn operator_completion_keeps_project_self_and_read_only_boundaries(cx: &mut TestAppContext) {
    let (window, other, caller) = completion_setup(cx);
    for (request, source, input) in [
        (
            "self-callback",
            caller,
            json!({"sessionId":caller,"prompt":"No","notifyOnComplete":true}),
        ),
        (
            "other-project",
            caller,
            json!({"sessionId":other,"prompt":"No","notifyOnComplete":true}),
        ),
        (
            "unrelated-caller",
            other,
            json!({"sessionId":caller,"prompt":"No","notifyOnComplete":true}),
        ),
        (
            "wrong-project",
            caller,
            json!({"project":"/elsewhere","sessionId":other,"prompt":"No","notifyOnComplete":true}),
        ),
    ] {
        assert!(call(window, source, request, "sessions.send", input, cx).is_err());
    }
    for mode in ["plan", "read-only"] {
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&caller].update(cx, |v, _| {
                    v.settings.plan = mode == "plan";
                    v.settings.access = monocode_core::agent::AccessMode::ReadOnly;
                });
            })
            .unwrap();
        assert!(
            call(
                window,
                caller,
                mode,
                "sessions.start",
                json!({"prompt":"No","notifyOnComplete":true}),
                cx
            )
            .is_err()
        );
    }
    window
        .update(cx, |shell, _, _| {
            assert!(shell.operator_subscriptions.is_empty())
        })
        .unwrap();
}

#[gpui::test]
fn operator_completion_start_arms_the_dispatch_turn_and_defaults_to_off(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let missing = std::env::temp_dir()
        .join(monocode_core::mono::new_id())
        .join("missing-project");
    assert!(!missing.exists());
    window
        .update(cx, |shell, _, cx| {
            let project = missing.to_string_lossy().into_owned();
            shell.session_summaries.get_mut(&caller).unwrap().cwd = project.clone();
            shell.sessions[&caller].update(cx, |v, _| v.summary.cwd = project);
        })
        .unwrap();
    let first = call(window, caller, "notified-start", "sessions.start",
        json!({"prompt":"No provider can start in the missing fixture directory", "harness":"codex", "title":"Started worker", "notifyOnComplete":true}), cx).unwrap()["sessionId"].as_i64().unwrap();
    let second = call(window, caller, "default-start", "sessions.start",
        json!({"prompt":"No provider can start in the missing fixture directory", "harness":"codex", "title":"Unsubscribed worker"}), cx).unwrap()["sessionId"].as_i64().unwrap();
    window
        .update(cx, |shell, _, cx| {
            let subscriptions = &shell.operator_subscriptions[&caller];
            assert_eq!(subscriptions.len(), 1);
            assert_eq!(subscriptions[0].session, first);
            assert_eq!(subscriptions[0].group, OperatorCompletionGroup::Turn(41));
            assert!(!shell.sessions[&first].read(cx).settings.operator_enabled);
            assert!(!shell.sessions[&second].read(cx).settings.operator_enabled);
        })
        .unwrap();
    end_completion_origin(window, caller, cx);
    assert_eq!(completion_prompts(window, caller, cx).len(), 1);
}

#[gpui::test]
fn operator_completion_restart_reports_an_interrupted_unloaded_worker(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let child = completion_worker(window, caller, "interrupted", cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions.remove(&child);
            shell.sessions[&caller].update(cx, |v, _| v.busy = false);
            shell.operator_subscriptions = locked(&shell.store)
                .unwrap()
                .operator_subscriptions()
                .unwrap();
            shell.restore_operator_completions(cx);
        })
        .unwrap();
    cx.run_until_parked();
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!("Session {child}: interrupted\nOutcome: stopped")));
}

#[gpui::test]
fn operator_completion_app_archive_and_delete_dismiss_workers(cx: &mut TestAppContext) {
    for action in ["sessions.archive", "sessions.delete"] {
        let (window, _, caller) = completion_setup(cx);
        let child = completion_worker(window, caller, "dismiss-by-app", cx);
        call(
            window,
            caller,
            "dismiss",
            action,
            json!({"sessionId":child}),
            cx,
        )
        .unwrap();
        end_completion_origin(window, caller, cx);
        assert!(completion_prompts(window, caller, cx).is_empty());
        window
            .update(cx, |shell, _, _| {
                assert!(!shell.operator_subscriptions.contains_key(&caller));
            })
            .unwrap();
    }
}

#[gpui::test]
fn operator_completion_distinct_labels_report_each_worker_with_pending_context(
    cx: &mut TestAppContext,
) {
    let (window, _, caller) = completion_setup(cx);
    let first = completion_worker_in_group(window, caller, "first", Some("first"), cx);
    let second = completion_worker_in_group(window, caller, "second", Some("second"), cx);
    end_completion_origin(window, caller, cx);
    finish_completion_worker(window, first, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    let (report, pending) = prompts[0]
        .split_once("\n\nOther reports pending:\n")
        .unwrap();
    assert!(report.contains(&format!("Session {first}: first\nOutcome: completed")));
    assert!(report.contains(&format!("Worker {first} final response")));
    assert!(!report.contains(&format!("Session {second}:")));
    assert_eq!(
        pending,
        format!("Session {second}: second\nMore reports will follow.")
    );
    finish_completion_worker(window, second, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].contains(&format!("Session {second}: second\nOutcome: completed")));
    assert!(!prompts[1].contains(&format!("Session {first}:")));
    assert!(!prompts[1].contains("Still running:"));
    assert!(!prompts[1].contains("More reports will follow."));
}

#[gpui::test]
fn operator_completion_shared_label_groups_workers_across_turns(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let first = completion_worker_in_group(window, caller, "first", Some(" shared "), cx);
    end_completion_origin(window, caller, cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, _| {
                v.mono_turn = 42;
                v.busy = true;
            });
        })
        .unwrap();
    let second = completion_worker_in_group(window, caller, "second", Some("shared"), cx);
    finish_completion_worker(window, first, cx);
    end_completion_origin(window, caller, cx);
    assert!(completion_prompts(window, caller, cx).is_empty());
    finish_completion_worker(window, second, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!("Session {first}: first\nOutcome: completed")));
    assert!(prompts[0].contains(&format!("Session {second}: second\nOutcome: completed")));
    assert!(!prompts[0].contains("Still running:"));
}

#[gpui::test]
fn operator_completion_reusing_a_delivered_label_starts_a_fresh_group(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let first = completion_worker_in_group(window, caller, "first", Some("review"), cx);
    end_completion_origin(window, caller, cx);
    finish_completion_worker(window, first, cx);
    assert_eq!(completion_prompts(window, caller, cx).len(), 1);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&caller].update(cx, |v, _| v.busy = true);
        })
        .unwrap();
    let second = completion_worker_in_group(window, caller, "second", Some("review"), cx);
    end_completion_origin(window, caller, cx);
    assert_eq!(completion_prompts(window, caller, cx).len(), 1);
    finish_completion_worker(window, second, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].contains(&format!("Session {second}: second\nOutcome: completed")));
    assert!(!prompts[1].contains(&format!("Session {first}:")));
    assert!(!prompts[1].contains("Still running:"));
    window
        .update(cx, |shell, _, cx| {
            shell.operator_subscriptions = locked(&shell.store)
                .unwrap()
                .operator_subscriptions()
                .unwrap();
            shell.restore_operator_completions(cx);
            shell.check_operator_completions(cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(completion_prompts(window, caller, cx).len(), 2);
}

#[gpui::test]
fn operator_completion_notify_group_requires_completion_opt_in(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    for action in ["sessions.start", "sessions.send"] {
        for opt_in in [None, Some(false)] {
            let mut input =
                json!({"prompt":"Do not start", "sessionId":caller, "notifyGroup":"review"});
            if action == "sessions.start" {
                input.as_object_mut().unwrap().remove("sessionId");
            }
            if let Some(opt_in) = opt_in {
                input["notifyOnComplete"] = json!(opt_in);
            }
            let error = call(window, caller, "invalid-label", action, input, cx).unwrap_err();
            assert_eq!(error, "notifyGroup requires notifyOnComplete:true");
        }
    }
    window
        .update(cx, |shell, _, _| {
            assert!(shell.operator_subscriptions.is_empty());
            assert!(shell.started_by.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn operator_completion_start_uses_a_trimmed_label(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let missing = std::env::temp_dir()
        .join(monocode_core::mono::new_id())
        .join("missing-project");
    assert!(!missing.exists());
    window
        .update(cx, |shell, _, cx| {
            let project = missing.to_string_lossy().into_owned();
            shell.session_summaries.get_mut(&caller).unwrap().cwd = project.clone();
            shell.sessions[&caller].update(cx, |v, _| v.summary.cwd = project);
        })
        .unwrap();
    let child = call(
        window,
        caller,
        "labelled-start",
        "sessions.start",
        json!({
            "prompt":"No provider can start in the missing fixture directory", "harness":"codex",
            "title":"Labelled worker", "notifyOnComplete":true, "notifyGroup":" launch "
        }),
        cx,
    )
    .unwrap()["sessionId"]
        .as_i64()
        .unwrap();
    window
        .update(cx, |shell, _, _| {
            let subscriptions = &shell.operator_subscriptions[&caller];
            assert_eq!(subscriptions.len(), 1);
            assert_eq!(subscriptions[0].session, child);
            assert_eq!(
                subscriptions[0].group,
                OperatorCompletionGroup::Label("launch".into())
            );
        })
        .unwrap();
    end_completion_origin(window, caller, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!(
        "Session {child}: Labelled worker\nOutcome: failed"
    )));
}

#[gpui::test]
fn operator_completion_waits_for_parent_account_recovery(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    window
        .update(cx, |shell, _, cx| {
            crate::accounts::install(shell.store.clone(), cx);
            shell.sessions[&caller].update(cx, |v, _| {
                v.settings.account.id = "removed-account".into();
            });
        })
        .unwrap();
    let worker = completion_worker_in_group(window, caller, "first", Some("first"), cx);
    end_completion_origin(window, caller, cx);
    finish_completion_worker(window, worker, cx);
    assert!(completion_prompts(window, caller, cx).is_empty());
    window
        .update(cx, |shell, _, cx| {
            assert!(!shell.operator_subscriptions[&caller][0].delivered);
            shell.sessions[&caller].update(cx, |v, cx| {
                v.settings.account = Default::default();
                cx.emit(SessionChanged);
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(completion_prompts(window, caller, cx).len(), 1);
}

#[gpui::test]
fn operator_completion_terminal_sibling_is_pending_not_running(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let first = completion_worker_in_group(window, caller, "first", Some("first"), cx);
    let second = completion_worker_in_group(window, caller, "second", Some("second"), cx);
    finish_completion_worker(window, first, cx);
    finish_completion_worker(window, second, cx);
    assert!(completion_prompts(window, caller, cx).is_empty());
    end_completion_origin(window, caller, cx);
    let prompts = completion_prompts(window, caller, cx);
    assert_eq!(prompts.len(), 2);
    assert!(!prompts[0].contains("Still running:"));
    assert!(prompts[0].contains("Other reports pending:"));
}
