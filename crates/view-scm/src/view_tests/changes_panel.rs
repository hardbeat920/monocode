//! Port of src/features/source-control/ui/GitChangesPanel.test.ts.

use std::sync::atomic::Ordering;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::support::{Recorded, index, pending_generator, setup};
use crate::git::{GitChangedFile, GitDiffIndex, GitRangeContext};
use crate::hooks::ScmHooks;
use crate::model::changes::{Busy, can_pull};
use crate::ui::changes_panel::GitChangesPanel;

fn render_panel<'a>(
    cx: &'a mut TestAppContext,
    cwd: &str,
    index: GitDiffIndex,
    hooks: ScmHooks,
) -> (
    Entity<GitChangesPanel>,
    &'a mut VisualTestContext,
    super::support::Setup,
) {
    let setup = setup(cx, hooks);
    setup.backend.set_diff_index(cwd, Ok(index));
    let scm = setup.scm.clone();
    let cwd = cwd.to_string();
    let (panel, cx) =
        cx.add_window_view(move |window, cx| GitChangesPanel::new(scm, cwd, true, window, cx));
    cx.run_until_parked();
    (panel, cx, setup)
}

fn staged_file() -> GitChangedFile {
    GitChangedFile {
        path: "/repo/change.ts".into(),
        relative: "change.ts".into(),
        status: "modified".into(),
        additions: 1,
        deletions: 0,
        staged: true,
        unstaged: false,
    }
}

// describe("GitChangesPanel commit message generation")

#[gpui::test]
fn cancels_promptly_and_ignores_a_late_result_after_a_retry(cx: &mut TestAppContext) {
    let (generator, sender, aborted, calls) = pending_generator("New message");
    let hooks = ScmHooks {
        generate_commit_message: Some(generator),
        ..Recorded::default().hooks()
    };
    let mut changed = index();
    changed.files.push(staged_file());
    let (panel, cx, _setup) = render_panel(cx, "/repo", changed, hooks);

    panel.update_in(cx, |panel, window, cx| panel.generate(window, cx));
    cx.run_until_parked();
    assert_eq!(*calls.borrow(), 1);
    assert!(!aborted.load(Ordering::SeqCst));
    assert!(panel.read_with(cx, |panel, _| panel.generating()));

    panel.update(cx, |panel, cx| panel.cancel_generate(cx));
    cx.run_until_parked();
    assert!(aborted.load(Ordering::SeqCst));
    panel.read_with(cx, |panel, cx| {
        let flags = panel.flags(cx);
        assert!(flags.can_generate, "the generate button is enabled again");
        assert!(flags.can_edit_message, "the message box is enabled again");
    });

    panel.update_in(cx, |panel, window, cx| panel.generate(window, cx));
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, cx| panel.message(cx)),
        "New message"
    );

    // The first request finishing late changes nothing.
    let late = sender
        .borrow_mut()
        .take()
        .unwrap()
        .send("Old message".into());
    assert!(late.is_err(), "the cancelled request is gone");
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, cx| panel.message(cx)),
        "New message"
    );
}

// describe("GitChangesPanel pull action")

#[gpui::test]
fn disables_pull_when_the_branch_has_no_upstream(cx: &mut TestAppContext) {
    let (panel, cx, setup) = render_panel(cx, "/repo", index(), Recorded::default().hooks());
    panel.update(cx, |panel, cx| panel.toggle_branch_menu(cx));
    assert!(panel.read_with(cx, |panel, _| panel.branch_menu_open()));
    assert!(!panel.read_with(cx, |panel, _| can_pull(panel.index())));
    panel.update_in(cx, |panel, window, cx| panel.pull(window, cx));
    cx.run_until_parked();
    assert!(setup.git.calls("git_pull").is_empty());
}

#[gpui::test]
fn disables_pull_when_the_repository_has_no_remote(cx: &mut TestAppContext) {
    let mut no_remote = index();
    no_remote.upstream = Some("origin/feature/pull".into());
    let (panel, cx, setup) = render_panel(cx, "/repo", no_remote, Recorded::default().hooks());
    panel.update(cx, |panel, cx| panel.toggle_branch_menu(cx));
    assert!(!panel.read_with(cx, |panel, _| can_pull(panel.index())));
    panel.update_in(cx, |panel, window, cx| panel.pull(window, cx));
    cx.run_until_parked();
    assert!(setup.git.calls("git_pull").is_empty());
}

#[gpui::test]
fn pulls_the_current_branch_and_reloads_watched_files(cx: &mut TestAppContext) {
    let recorded = Recorded::default();
    let mut ready = index();
    ready.remote = Some("origin".into());
    ready.upstream = Some("origin/feature/pull".into());
    let (panel, cx, setup) = render_panel(cx, "/repo", ready, recorded.hooks());
    panel.update(cx, |panel, cx| panel.toggle_branch_menu(cx));
    assert!(panel.read_with(cx, |panel, _| can_pull(panel.index())));

    *recorded.files_changed.borrow_mut() = 0;
    panel.update_in(cx, |panel, window, cx| panel.pull(window, cx));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.busy().cloned()),
        Some(Busy::Pull)
    );
    cx.run_until_parked();

    assert_eq!(setup.git.calls("git_pull"), vec![vec!["/repo".to_string()]]);
    assert!(*recorded.files_changed.borrow() > 0);
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.status_text(), Some("Pull complete"));
        assert!(!panel.branch_menu_open());
        assert_eq!(panel.busy(), None);
    });
}

// describe("GitChangesPanel remote pull request")

#[gpui::test]
fn creates_it_from_the_host_git_range_without_calling_a_local_harness(cx: &mut TestAppContext) {
    let cwd = "remote://machine/home/user/repo";
    let recorded = Recorded::default();
    let mut ahead = index();
    ahead.remote = Some("origin".into());
    ahead.upstream = Some("origin/feature/pull".into());
    ahead.ahead = 1;
    ahead.ahead_of_default = 1;
    let (panel, cx, setup) = render_panel(cx, cwd, ahead, recorded.hooks());
    setup.git.set_range(GitRangeContext {
        base: "main".into(),
        head: "feature/pull".into(),
        commit_summary: "abc123 Fix remote flow\ndef456 Add coverage".into(),
        diff_summary: "2 files changed, 4 insertions(+)\n".into(),
        diff_patch: String::new(),
    });
    setup.git.set_pr_url("https://example.test/pull/42");

    assert!(panel.read_with(cx, |panel, cx| panel.flags(cx).can_create_pr));
    panel.update_in(cx, |panel, window, cx| panel.create_pr(window, cx));
    cx.run_until_parked();

    assert_eq!(setup.git.calls("git_push"), vec![vec![cwd.to_string()]]);
    assert_eq!(
        setup.git.calls("git_range_context"),
        vec![vec![cwd.to_string()]]
    );
    assert_eq!(*recorded.pr_content_calls.borrow(), 0);
    let created = setup.git.calls("git_pr_create");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0][0], cwd);
    assert_eq!(created[0][1], "Fix remote flow");
    assert!(created[0][2].contains("## Changes\n\n2 files changed"));
    assert_eq!(
        (created[0][3].as_str(), created[0][4].as_str()),
        ("main", "feature/pull")
    );
    assert_eq!(
        *recorded.urls.borrow(),
        vec!["https://example.test/pull/42".to_string()]
    );
}

// Beyond the TypeScript tests: the actions around a commit.

#[gpui::test]
fn commits_the_message_and_clears_it(cx: &mut TestAppContext) {
    let mut changed = index();
    changed.files.push(staged_file());
    let (panel, cx, setup) = render_panel(cx, "/repo", changed, Recorded::default().hooks());
    let message = panel.read_with(cx, |panel, _| panel.message_input().clone());
    message.update_in(cx, |state, window, cx| {
        state.set_value("Fix the flow", window, cx)
    });
    assert!(panel.read_with(cx, |panel, cx| panel.flags(cx).can_commit));
    panel.update_in(cx, |panel, window, cx| {
        panel.commit(false, false, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        setup.git.calls("git_commit"),
        vec![vec![
            "/repo".to_string(),
            "Fix the flow".to_string(),
            String::new()
        ]]
    );
    assert!(setup.git.calls("git_push").is_empty());
    assert_eq!(panel.read_with(cx, |panel, cx| panel.message(cx)), "");
}

#[gpui::test]
fn stages_and_discards_files_after_confirming(cx: &mut TestAppContext) {
    let mut changed = index();
    let mut file = staged_file();
    file.staged = false;
    file.unstaged = true;
    changed.files.push(file.clone());
    let (panel, cx, setup) = render_panel(cx, "/repo", changed, Recorded::default().hooks());
    panel.update_in(cx, |panel, window, cx| {
        panel.run_file(
            file.clone(),
            crate::model::changes::FileAction::Stage,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    panel.update_in(cx, |panel, window, cx| {
        panel.run_all(crate::model::changes::FileAction::Discard, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        setup.git.calls("git_stage_file"),
        vec![vec!["/repo".to_string(), "change.ts".to_string()]]
    );
    assert_eq!(
        setup.git.calls("git_discard_all"),
        vec![vec!["/repo".to_string()]]
    );
}
