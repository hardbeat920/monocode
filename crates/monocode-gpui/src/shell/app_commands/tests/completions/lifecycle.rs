use super::*;

fn empty_session(window: WindowHandle<Shell>, caller: i64, cx: &mut TestAppContext) -> i64 {
    let id = window
        .update(cx, |shell, _, cx| {
            let mut summary = locked(&shell.store)
                .unwrap()
                .create("codex", &shell.sessions[&caller].read(cx).summary.cwd)
                .unwrap();
            summary.provider = "fixture".into();
            let id = summary.id;
            shell.add(summary, cx);
            shell.sessions[&id].update(cx, |v, cx| v.load(cx));
            id
        })
        .unwrap();
    cx.run_until_parked();
    id
}

fn reload_shell(window: WindowHandle<Shell>, cx: &mut TestAppContext) {
    window
        .update(cx, |shell, _, cx| {
            let store = shell.store.clone();
            let summaries = shell
                .session_summaries
                .values()
                .map(|s| {
                    shell
                        .sessions
                        .get(&s.id)
                        .map(|v| v.read(cx).summary.clone())
                        .unwrap_or_else(|| s.clone())
                })
                .collect();
            let cwd = shell.workspace.project.clone();
            let saved = Some(serde_json::to_string(&shell.workspace).unwrap());
            *shell = Shell::new(store, summaries, cwd, saved, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn operator_completion_empty_failure_does_not_block_other_workers(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let empty = empty_session(window, caller, cx);
    let other = completion_worker(window, caller, "other", cx);
    window
        .update(cx, |shell, _, cx| {
            assert!(
                shell.sessions[&empty].read(cx).worker_outcome().is_none(),
                "a fresh empty chat is not a completed worker"
            );
            shell
                .arm_operator_completion(
                    caller,
                    OperatorCompletionGroup::Turn(41),
                    empty,
                    "empty".into(),
                )
                .unwrap();
            shell.sessions[&empty].update(cx, |v, cx| {
                let summary = v.summary.clone();
                let settings = Settings {
                    worktree_removed: true,
                    ..v.settings.clone()
                };
                v.worktree_deleted(summary, settings, cx);
                assert!(!v.summary.has_messages);
            });
        })
        .unwrap();
    end_completion_origin(window, caller, cx);
    assert!(completion_prompts(window, caller, cx).is_empty());
    finish_completion_worker(window, other, cx);
    let reports = completion_prompts(window, caller, cx);
    assert_eq!(reports.len(), 1);
    assert!(reports[0].contains(&format!("Session {empty}: empty\nOutcome: failed")));
    assert!(reports[0].contains(&format!("Session {other}: other\nOutcome: completed")));
}

#[gpui::test]
fn operator_completion_restart_reports_empty_interrupted_worker(cx: &mut TestAppContext) {
    let (window, _, caller) = completion_setup(cx);
    let empty = empty_session(window, caller, cx);
    window
        .update(cx, |shell, _, cx| {
            shell
                .arm_operator_completion(
                    caller,
                    OperatorCompletionGroup::Turn(41),
                    empty,
                    "empty".into(),
                )
                .unwrap();
            shell.sessions[&caller].update(cx, |v, _| v.busy = false);
            shell.sessions.remove(&empty);
            shell.restore_operator_completions(cx);
        })
        .unwrap();
    cx.run_until_parked();
    let reports = completion_prompts(window, caller, cx);
    assert_eq!(reports.len(), 1);
    assert!(reports[0].contains(&format!("Session {empty}: empty\nOutcome: stopped")));
}

#[gpui::test]
fn operator_completion_send_rejects_removed_worktree_without_a_subscription(
    cx: &mut TestAppContext,
) {
    let (window, _, caller) = completion_setup(cx);
    let other = completion_worker(window, caller, "other", cx);
    let empty = empty_session(window, caller, cx);
    window
        .update(cx, |shell, _, cx| {
            shell.sessions[&empty].update(cx, |v, cx| {
                v.worktree_deleted(
                    v.summary.clone(),
                    Settings {
                        worktree_removed: true,
                        ..v.settings.clone()
                    },
                    cx,
                );
            });
        })
        .unwrap();
    let error = call(
        window,
        caller,
        "removed-worktree",
        "sessions.send",
        json!({
            "sessionId":empty,"prompt":"Do not accept this prompt","notifyOnComplete":true
        }),
        cx,
    )
    .unwrap_err();
    assert!(error.to_lowercase().contains("worktree"));
    window
        .read_with(cx, |shell, cx| {
            assert_eq!(shell.operator_subscriptions[&caller].len(), 1);
            assert_eq!(shell.operator_subscriptions[&caller][0].session, other);
            assert!(
                shell.sessions[&empty]
                    .read(cx)
                    .input
                    .read(cx)
                    .content
                    .is_empty()
            );
            assert!(!shell.sessions[&empty].read(cx).summary.has_messages);
        })
        .unwrap();
}

#[gpui::test]
fn operator_completion_load_error_preserves_saved_state_until_recovery(cx: &mut TestAppContext) {
    let (window, other, caller) = completion_setup(cx);
    let worker = completion_worker(window, caller, "saved-worker", cx);
    let key = format!("operator-completions:{caller}");
    let broken = format!("operator-completions:{other}");
    let saved = window
        .update(cx, |shell, _, _| {
            let store = locked(&shell.store).unwrap();
            let saved = store.setting(&key).unwrap().unwrap();
            store
                .save_setting(&broken, "invalid subscription JSON")
                .unwrap();
            saved
        })
        .unwrap();
    reload_shell(window, cx);
    window
        .update(cx, |shell, _, cx| {
            let error = shell
                .arm_operator_completion(
                    caller,
                    OperatorCompletionGroup::Label("new".into()),
                    worker,
                    "new-worker".into(),
                )
                .unwrap_err();
            assert!(error.to_lowercase().contains("completion"));
            assert!(
                shell
                    .error
                    .as_deref()
                    .unwrap()
                    .to_lowercase()
                    .contains("completion")
            );
            shell.check_operator_completions(cx);
            let store = locked(&shell.store).unwrap();
            assert_eq!(
                store.setting(&key).unwrap().as_deref(),
                Some(saved.as_str())
            );
            assert_eq!(
                store.setting(&broken).unwrap().as_deref(),
                Some("invalid subscription JSON")
            );
            store.save_setting(&broken, "[]").unwrap();
        })
        .unwrap();
    reload_shell(window, cx);
    window
        .update(cx, |shell, _, _| {
            shell
                .arm_operator_completion(
                    caller,
                    OperatorCompletionGroup::Label("recovered".into()),
                    worker,
                    "recovered-worker".into(),
                )
                .unwrap();
        })
        .unwrap();
}
