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
