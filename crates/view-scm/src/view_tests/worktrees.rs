//! Port of src/features/source-control/ui/Worktrees.test.ts: the worktree
//! picker, the worktrees page, and the create and delete dialogs.
//!
//! Not ported here: the branch name helpers and `assertWorktreeFilesClosed`
//! (model/worktrees.ts, outside this crate), the workspace picker and the
//! delete session dialog (other features), the popover height check (CSS),
//! and the tests of in-flight reads, which the engine's `GitStatus` owns and
//! tests.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Entity, Task, TestAppContext, VisualTestContext};

use super::support::{Recorded, Setup, main_tree, setup, tree, worktrees};
use crate::git::{GitBranchEntry, GitBranches, Worktree};
use crate::model::worktrees::NO_BRANCH_LABEL;
use crate::ui::branch_picker::BranchPicker;
use crate::ui::dialogs::create_worktree::CreateWorktreeDialog;
use crate::ui::dialogs::delete_worktree::{
    DeleteWorktreeDialog, DeleteWorktreeEvent, RemoveRequest,
};
use crate::ui::worktree_picker::WorktreePicker;
use crate::ui::worktrees_page::{WorktreeRemoveCall, WorktreesPage};

fn main_branches() -> GitBranches {
    GitBranches {
        current: Some("main".into()),
        detached: false,
        branches: vec![GitBranchEntry {
            name: "main".into(),
            current: true,
            remote: None,
        }],
    }
}

fn result(branch: &str) -> crate::git::Worktrees {
    let mut feature = tree();
    feature.branch = Some(branch.into());
    worktrees(vec![feature])
}

type Selected = Rc<RefCell<Vec<Worktree>>>;

struct PickerOptions {
    cwd: &'static str,
    execution_cwd: String,
    opens_new_session: bool,
    worktree_removed: bool,
}

fn options(cwd: &'static str, execution_cwd: &str) -> PickerOptions {
    PickerOptions {
        cwd,
        execution_cwd: execution_cwd.into(),
        opens_new_session: false,
        worktree_removed: false,
    }
}

fn render_picker(
    cx: &mut TestAppContext,
    opts: PickerOptions,
    configure: impl FnOnce(&Setup),
) -> (
    Entity<WorktreePicker>,
    &mut VisualTestContext,
    Setup,
    Selected,
) {
    let setup = setup(cx, Recorded::default().hooks());
    for cwd in [opts.cwd, opts.execution_cwd.as_str()] {
        setup.backend.set_branches(cwd, Ok(main_branches()));
    }
    setup
        .backend
        .set_worktrees(opts.cwd, Ok(worktrees(vec![main_tree("/repo"), tree()])));
    configure(&setup);
    let selected: Selected = Rc::default();
    let on_select = {
        let selected = selected.clone();
        Rc::new(
            move |tree: Worktree, _: &mut gpui::Window, _: &mut gpui::App| {
                selected.borrow_mut().push(tree);
                Task::ready(Ok(()))
            },
        )
    };
    let scm = setup.scm.clone();
    let (picker, cx) = cx.add_window_view(move |window, cx| {
        WorktreePicker::new(
            scm,
            opts.cwd,
            opts.execution_cwd.clone(),
            opts.opens_new_session,
            opts.worktree_removed,
            on_select,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    (picker, cx, setup, selected)
}

fn open(picker: &Entity<WorktreePicker>, cx: &mut VisualTestContext) {
    picker.update_in(cx, |picker, window, cx| picker.toggle(window, cx));
    cx.run_until_parked();
}

fn highlighted(picker: &Entity<WorktreePicker>, cx: &mut VisualTestContext) -> String {
    picker.read_with(cx, |picker, cx| {
        let rows = picker.rows(cx);
        rows[picker.active(cx)].path.clone()
    })
}

fn search(picker: &Entity<WorktreePicker>, text: &str, cx: &mut VisualTestContext) {
    let input = picker.read_with(cx, |picker, _| picker.query_input().clone());
    let text = text.to_string();
    input.update_in(cx, |state, window, cx| state.set_value(text, window, cx));
    cx.run_until_parked();
}

type Removed = Rc<RefCell<Vec<WorktreeRemoveCall>>>;

fn render_page<'a>(
    cx: &'a mut TestAppContext,
    cwd: &'static str,
    list: crate::git::Worktrees,
    remove: Result<(), String>,
) -> (
    Entity<WorktreesPage>,
    &'a mut VisualTestContext,
    Setup,
    Removed,
) {
    let setup = setup(cx, Recorded::default().hooks());
    setup.backend.set_worktrees(cwd, Ok(list));
    let removed: Removed = Rc::default();
    let on_remove = {
        let removed = removed.clone();
        Rc::new(move |call: WorktreeRemoveCall, _: &mut gpui::App| {
            removed.borrow_mut().push(call);
            Task::ready(remove.clone())
        })
    };
    let scm = setup.scm.clone();
    let (page, cx) =
        cx.add_window_view(move |window, cx| WorktreesPage::new(scm, cwd, on_remove, window, cx));
    cx.run_until_parked();
    (page, cx, setup, removed)
}

fn branches_of(page: &Entity<WorktreesPage>, cx: &mut VisualTestContext) -> Vec<String> {
    page.read_with(cx, |page, cx| {
        page.worktrees(cx)
            .iter()
            .map(|tree| tree.branch.clone().unwrap_or_default())
            .collect()
    })
}

fn delete_dialog(
    page: &Entity<WorktreesPage>,
    cx: &mut VisualTestContext,
) -> Entity<DeleteWorktreeDialog> {
    let tree = page.read_with(cx, |page, cx| page.worktrees(cx)[0].clone());
    page.update(cx, |page, cx| page.start_delete(tree, cx));
    page.read_with(cx, |page, _| page.deleting().cloned())
        .expect("delete dialog")
}

fn refresh(page: &Entity<WorktreesPage>, cx: &mut VisualTestContext) {
    page.update(cx, |page, cx| page.refresh(cx).detach());
    cx.run_until_parked();
}

type Deleted = Rc<RefCell<Vec<Vec<String>>>>;

fn delete_sessions(
    page: &Entity<WorktreesPage>,
    answer: Result<bool, String>,
    cx: &mut VisualTestContext,
) -> Deleted {
    let deleted: Deleted = Rc::default();
    let handler = {
        let deleted = deleted.clone();
        Rc::new(move |ids: Vec<String>, _: &mut gpui::App| {
            deleted.borrow_mut().push(ids);
            Task::ready(answer.clone())
        })
    };
    page.update(cx, |page, _| page.set_delete_sessions(Some(handler)));
    deleted
}

#[gpui::test]
fn keeps_the_settings_rows_mounted_through_refreshes_and_failures(cx: &mut TestAppContext) {
    let (page, cx, setup, _) = render_page(cx, "/settings-refresh", result("feature"), Ok(()));
    assert_eq!(branches_of(&page, cx), vec!["feature"]);
    setup
        .backend
        .set_failing("git_worktrees", Some("Git unavailable"));
    refresh(&page, cx);
    assert_eq!(branches_of(&page, cx), vec!["feature"], "the rows stay");
    assert!(
        page.read_with(cx, |page, cx| page.refresh_title(cx))
            .contains("Git unavailable")
    );
    setup.backend.set_failing("git_worktrees", None);
    setup
        .backend
        .set_worktrees("/settings-refresh", Ok(result("updated")));
    refresh(&page, cx);
    assert_eq!(branches_of(&page, cx), vec!["updated"]);
}

#[gpui::test]
fn reopens_the_picker_with_cached_rows_while_revalidating(cx: &mut TestAppContext) {
    let (picker, cx, setup, _) =
        render_picker(cx, options("/picker-cache", "/picker-cache"), |setup| {
            setup.backend.set_worktrees(
                "/picker-cache",
                Ok(worktrees(vec![main_tree("/picker-cache"), tree()])),
            );
        });
    open(&picker, cx);
    assert_eq!(picker.read_with(cx, |picker, cx| picker.rows(cx).len()), 2);
    setup
        .backend
        .set_worktrees("/picker-cache", Ok(result("updated")));
    picker.update_in(cx, |picker, window, cx| picker.toggle(window, cx));
    picker.update_in(cx, |picker, window, cx| picker.toggle(window, cx));
    // Before the refresh lands, the cached rows show.
    assert_eq!(picker.read_with(cx, |picker, cx| picker.rows(cx).len()), 2);
    cx.run_until_parked();
    let rows = picker.read_with(cx, |picker, cx| picker.rows(cx));
    assert_eq!(rows[0].branch.as_deref(), Some("updated"));
}

fn highlights_the_current_working_copy(execution_cwd: &str, cx: &mut TestAppContext) {
    let (picker, cx, _, selected) = render_picker(cx, options("/repo", execution_cwd), |_| {});
    open(&picker, cx);
    assert_eq!(highlighted(&picker, cx), execution_cwd);
    let current = picker.read_with(cx, |picker, cx| {
        picker
            .rows(cx)
            .iter()
            .filter(|tree| picker.is_current(tree))
            .map(|tree| tree.path.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(current, vec![execution_cwd.to_string()]);
    let delta = if execution_cwd == "/repo" { 1 } else { -1 };
    picker.update(cx, |picker, cx| picker.move_active(delta, cx));
    assert_ne!(highlighted(&picker, cx), execution_cwd);
    picker.update_in(cx, |picker, window, cx| picker.toggle(window, cx));
    open(&picker, cx);
    assert_eq!(
        highlighted(&picker, cx),
        execution_cwd,
        "reopening highlights the current copy"
    );
    picker.update_in(cx, |picker, window, cx| picker.enter(window, cx));
    cx.run_until_parked();
    assert_eq!(selected.borrow()[0].path, execution_cwd);
}

#[gpui::test]
fn highlights_the_current_working_copy_repo_each_time_the_picker_opens(cx: &mut TestAppContext) {
    highlights_the_current_working_copy("/repo", cx);
}

#[gpui::test]
fn highlights_the_current_working_copy_worktree_each_time_the_picker_opens(
    cx: &mut TestAppContext,
) {
    highlights_the_current_working_copy("/repo-worktrees/feature", cx);
}

#[gpui::test]
fn highlights_the_current_worktree_after_a_cold_load_and_keeps_navigation_through_refreshes(
    cx: &mut TestAppContext,
) {
    let main = main_tree("/cold-highlight");
    let (picker, cx, setup, _) =
        render_picker(cx, options("/cold-highlight", &tree().path), |setup| {
            setup.backend.set_worktrees(
                "/cold-highlight",
                Ok(worktrees(vec![main_tree("/cold-highlight"), tree()])),
            );
        });
    open(&picker, cx);
    assert_eq!(highlighted(&picker, cx), tree().path);
    picker.update(cx, |picker, cx| picker.move_active(-1, cx));
    assert_eq!(highlighted(&picker, cx), main.path);
    setup
        .backend
        .set_worktrees("/cold-highlight", Ok(worktrees(vec![tree(), main.clone()])));
    picker.update(cx, |picker, cx| picker.refresh(cx));
    cx.run_until_parked();
    assert_eq!(
        highlighted(&picker, cx),
        main.path,
        "navigation follows the path, not the index"
    );
}

#[gpui::test]
fn highlights_search_results_and_restores_the_current_worktree_when_search_is_cleared(
    cx: &mut TestAppContext,
) {
    let (picker, cx, _, _) = render_picker(cx, options("/repo", &tree().path), |_| {});
    open(&picker, cx);
    search(&picker, "main", cx);
    assert_eq!(highlighted(&picker, cx), "/repo");
    search(&picker, "no matches", cx);
    assert!(picker.read_with(cx, |picker, cx| picker.rows(cx).is_empty()));
    search(&picker, "", cx);
    assert_eq!(highlighted(&picker, cx), tree().path);
}

#[gpui::test]
fn switches_to_branch_mode_and_back(cx: &mut TestAppContext) {
    let (picker, cx, _, _) = render_picker(cx, options("/repo", &tree().path), |_| {});
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.trigger_label(cx)),
        "main"
    );
    open(&picker, cx);
    assert!(picker.read_with(cx, |picker, _| picker.can_switch_branch()));
    picker.update_in(cx, |picker, window, cx| picker.switch_branch(window, cx));
    cx.run_until_parked();
    let branch: Entity<BranchPicker> = picker
        .read_with(cx, |picker, _| picker.branch_picker().cloned())
        .unwrap();
    assert!(
        branch.read_with(cx, |branch, _| branch.is_open()),
        "the branch popover opens at once"
    );
    branch.update_in(cx, |branch, window, cx| branch.dismiss(true, window, cx));
    cx.run_until_parked();
    assert!(picker.read_with(cx, |picker, _| picker.branch_picker().is_none()));
}

#[gpui::test]
fn uses_the_project_picker_and_shows_the_chosen_project(cx: &mut TestAppContext) {
    let (page, cx, setup, _) = render_page(cx, "/first-project", result("first"), Ok(()));
    setup
        .backend
        .set_worktrees("/second-project", Ok(result("second")));
    page.update(cx, |page, cx| {
        page.set_projects(vec!["/second-project".into()], Vec::new(), cx)
    });
    let select = page.read_with(cx, |page, _| page.project_select().clone());
    assert!(
        select
            .read_with(cx, |select, _| select.trigger_label())
            .starts_with("Switch project")
    );
    select.update_in(cx, |select, window, cx| select.open_menu(window, cx));
    let options = select.read_with(cx, |select, cx| select.filtered(cx));
    assert_eq!(options.len(), 2);
    select.update_in(cx, |select, window, cx| {
        select.pick("/second-project".into(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        page.read_with(cx, |page, _| page.project().to_string()),
        "/second-project"
    );
    assert_eq!(branches_of(&page, cx), vec!["second"]);
}

#[gpui::test]
fn selects_an_existing_working_copy_without_checking_out_a_branch(cx: &mut TestAppContext) {
    let (picker, cx, setup, selected) = render_picker(cx, options("/repo", "/repo"), |_| {});
    open(&picker, cx);
    assert_eq!(picker.read_with(cx, |picker, _| picker.notice()), None);
    let second = picker.read_with(cx, |picker, cx| picker.rows(cx)[1].clone());
    picker.update_in(cx, |picker, window, cx| picker.select(second, window, cx));
    cx.run_until_parked();
    assert_eq!(*selected.borrow(), vec![tree()]);
    assert!(setup.git.calls("git_checkout").is_empty());
    assert!(!picker.read_with(cx, |picker, _| picker.is_open()));
}

#[gpui::test]
fn explains_that_a_started_session_opens_another_working_copy_in_a_new_session(
    cx: &mut TestAppContext,
) {
    let mut opts = options("/repo", &tree().path);
    opts.opens_new_session = true;
    let (picker, cx, setup, selected) = render_picker(cx, opts, |_| {});
    open(&picker, cx);
    assert_eq!(
        picker.read_with(cx, |picker, _| picker.notice()),
        Some("Another working copy opens a new session.")
    );
    let first = picker.read_with(cx, |picker, cx| picker.rows(cx)[0].clone());
    picker.update_in(cx, |picker, window, cx| picker.select(first, window, cx));
    cx.run_until_parked();
    assert_eq!(selected.borrow()[0].path, "/repo");
    assert!(selected.borrow()[0].is_main);
    assert!(setup.git.calls("git_checkout").is_empty());
}

#[gpui::test]
fn can_return_to_the_main_working_copy_after_a_worktree_is_deleted_externally(
    cx: &mut TestAppContext,
) {
    let (picker, cx, _, selected) = render_picker(cx, options("/repo", &tree().path), |setup| {
        setup
            .backend
            .set_branches(&tree().path, Err("not a repository".into()));
    });
    assert!(picker.read_with(cx, |picker, cx| picker.trigger_enabled(cx)));
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.trigger_label(cx)),
        "Worktree unavailable"
    );
    open(&picker, cx);
    let first = picker.read_with(cx, |picker, cx| picker.rows(cx)[0].clone());
    picker.update_in(cx, |picker, window, cx| picker.select(first, window, cx));
    cx.run_until_parked();
    assert_eq!(selected.borrow()[0].path, "/repo");
}

type Requests = Rc<RefCell<Vec<RemoveRequest>>>;
type DeleteDialogSetup<'a> = (
    Entity<DeleteWorktreeDialog>,
    &'a mut VisualTestContext,
    Requests,
    Rc<RefCell<usize>>,
);

fn render_delete_dialog(cx: &mut TestAppContext, session_count: usize) -> DeleteDialogSetup<'_> {
    let _ = setup(cx, Recorded::default().hooks());
    let removed: Rc<RefCell<Vec<RemoveRequest>>> = Rc::default();
    let on_remove = {
        let removed = removed.clone();
        Rc::new(
            move |request: RemoveRequest, _: &mut gpui::Window, _: &mut gpui::App| {
                removed.borrow_mut().push(request);
                Task::ready(Ok(()))
            },
        )
    };
    let (dialog, cx) = cx.add_window_view(move |_, _| {
        DeleteWorktreeDialog::new("/repo", tree(), session_count, on_remove)
    });
    let deleted: Rc<RefCell<usize>> = Rc::default();
    {
        let deleted = deleted.clone();
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &DeleteWorktreeEvent, _| {
                if *event == DeleteWorktreeEvent::Deleted {
                    *deleted.borrow_mut() += 1;
                }
            })
            .detach()
        });
    }
    (dialog, cx, removed, deleted)
}

#[gpui::test]
fn shows_session_and_local_change_consequences_before_removing_a_worktree(cx: &mut TestAppContext) {
    let (dialog, cx, removed, deleted) = render_delete_dialog(cx, 2);
    assert!(!dialog.read_with(cx, |dialog, _| dialog.delete_sessions()));
    let lines = dialog.read_with(cx, |dialog, _| dialog.consequences());
    assert!(lines.iter().any(|line| line == "2 sessions using this worktree are kept. Select a branch or worktree to continue them."));
    dialog.update(cx, |dialog, cx| dialog.toggle_delete_sessions(cx));
    let lines = dialog.read_with(cx, |dialog, _| dialog.consequences());
    assert!(
        lines
            .iter()
            .any(|line| line == "2 sessions using this worktree are permanently deleted.")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("All uncommitted and untracked changes"))
    );
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.button_label()),
        "Delete worktree and sessions"
    );
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert_eq!(
        *removed.borrow(),
        vec![RemoveRequest {
            cwd: "/repo".into(),
            path: tree().path,
            force: true,
            delete_sessions: true,
        }]
    );
    assert_eq!(*deleted.borrow(), 1);
}

#[gpui::test]
fn offers_cascade_deletion_for_a_session_linked_worktree_but_not_the_main_copy(
    cx: &mut TestAppContext,
) {
    let mut linked = tree();
    linked.session_ids = vec!["session-1".into()];
    let (page, cx, setup, removed) = render_page(
        cx,
        "/repo",
        worktrees(vec![main_tree("/repo"), linked]),
        Ok(()),
    );
    assert_eq!(
        branches_of(&page, cx),
        vec!["feature"],
        "the main copy is not listed"
    );
    let order: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let deleted: Deleted = Rc::default();
    {
        let (order, deleted, git) = (order.clone(), deleted.clone(), setup.git.clone());
        page.update(cx, |page, _| {
            page.set_delete_sessions(Some(Rc::new(move |ids: Vec<String>, _: &mut gpui::App| {
                assert_eq!(
                    git.calls("git_worktree_check_remove").len(),
                    1,
                    "the preflight ran first"
                );
                order.borrow_mut().push("delete");
                deleted.borrow_mut().push(ids);
                Task::ready(Ok(true))
            })))
        });
    }
    let dialog = delete_dialog(&page, cx);
    dialog.update(cx, |dialog, cx| dialog.toggle_delete_sessions(cx));
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.button_label()),
        "Delete worktree and session"
    );
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert_eq!(*deleted.borrow(), vec![vec!["session-1".to_string()]]);
    assert_eq!(
        setup.git.calls("git_worktree_check_remove"),
        vec![vec!["/repo".to_string(), tree().path, "force".to_string()]]
    );
    assert_eq!(
        *removed.borrow(),
        vec![WorktreeRemoveCall {
            cwd: "/repo".into(),
            path: tree().path,
            force: true,
            keep_sessions: Some(false),
        }]
    );
    assert_eq!(*order.borrow(), vec!["delete"]);
}

fn blocked_deletion(native: bool, cx: &mut TestAppContext) {
    let mut linked = tree();
    linked.session_ids = vec!["session-1".into(), "session-2".into()];
    let cwd = if native {
        "/blocked-native"
    } else {
        "/blocked-file"
    };
    let (page, cx, setup, removed) = render_page(cx, cwd, worktrees(vec![linked]), Ok(()));
    if native {
        setup
            .git
            .fail("git_worktree_check_remove", Some("This worktree is locked"));
    } else {
        page.update(cx, |page, _| {
            page.set_check_remove(Some(Rc::new(|_, _: &mut gpui::App| {
                Task::ready(Err(
                    "Close the files and terminals open in this worktree first.".into(),
                ))
            })))
        });
    }
    let deleted = delete_sessions(&page, Ok(true), cx);
    let dialog = delete_dialog(&page, cx);
    dialog.update(cx, |dialog, cx| dialog.toggle_delete_sessions(cx));
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert!(deleted.borrow().is_empty());
    assert!(removed.borrow().is_empty());
    let error = dialog
        .read_with(cx, |dialog, _| dialog.error().map(str::to_string))
        .unwrap();
    assert!(error.contains(if native {
        "locked"
    } else {
        "Close the files and terminals"
    }));
}

#[gpui::test]
fn keeps_every_session_when_a_file_blocker_prevents_worktree_deletion(cx: &mut TestAppContext) {
    blocked_deletion(false, cx);
}

#[gpui::test]
fn keeps_every_session_when_a_native_blocker_prevents_worktree_deletion(cx: &mut TestAppContext) {
    blocked_deletion(true, cx);
}

#[gpui::test]
fn reports_partial_completion_if_worktree_removal_fails_after_successful_preflight(
    cx: &mut TestAppContext,
) {
    let mut linked = tree();
    linked.session_ids = vec!["session-1".into()];
    let (page, cx, _, _) = render_page(
        cx,
        "/late-removal-failure",
        worktrees(vec![linked]),
        Err("Disk unavailable".into()),
    );
    let deleted = delete_sessions(&page, Ok(true), cx);
    let dialog = delete_dialog(&page, cx);
    dialog.update(cx, |dialog, cx| dialog.toggle_delete_sessions(cx));
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert_eq!(deleted.borrow().len(), 1);
    let error = page
        .read_with(cx, |page, _| page.error().map(str::to_string))
        .unwrap();
    assert!(error.contains("The sessions were deleted, but the worktree was kept."));
    assert!(error.contains("Disk unavailable"));
}

#[gpui::test]
fn refreshes_the_session_ids_before_retrying_a_partial_deletion_failure(cx: &mut TestAppContext) {
    let mut linked = tree();
    linked.session_ids = vec!["deleted".into(), "remaining".into()];
    let (page, cx, setup, removed) =
        render_page(cx, "/retry-partial", worktrees(vec![linked]), Ok(()));
    let deleted = delete_sessions(&page, Ok(false), cx);
    let mut remaining = tree();
    remaining.session_ids = vec!["remaining".into()];
    setup
        .backend
        .set_worktrees("/retry-partial", Ok(worktrees(vec![remaining])));
    let dialog = delete_dialog(&page, cx);
    dialog.update(cx, |dialog, cx| dialog.toggle_delete_sessions(cx));
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert!(
        page.read_with(cx, |page, _| page.deleting().is_none()),
        "the dialog closes"
    );
    assert_eq!(
        *deleted.borrow(),
        vec![vec!["deleted".to_string(), "remaining".to_string()]]
    );
    assert!(removed.borrow().is_empty());
    assert!(
        page.read_with(cx, |page, _| page.error().map(str::to_string))
            .unwrap()
            .contains("Some sessions could not be deleted")
    );

    let deleted = delete_sessions(&page, Ok(true), cx);
    let dialog = delete_dialog(&page, cx);
    let lines = dialog.read_with(cx, |dialog, _| dialog.consequences());
    assert!(lines[0].starts_with("1 session using this worktree is kept."));
    dialog.update(cx, |dialog, cx| dialog.toggle_delete_sessions(cx));
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert_eq!(*deleted.borrow(), vec![vec!["remaining".to_string()]]);
    assert!(page.read_with(cx, |page, _| page.deleting().is_none()));
}

#[gpui::test]
fn uses_searchable_custom_selects_in_the_create_worktree_dialog(cx: &mut TestAppContext) {
    let setup = setup(cx, Recorded::default().hooks());
    setup.backend.set_branches(
        "/repo",
        Ok(GitBranches {
            current: Some("main".into()),
            detached: false,
            branches: vec![
                GitBranchEntry {
                    name: "main".into(),
                    current: true,
                    remote: None,
                },
                GitBranchEntry {
                    name: "feature/search".into(),
                    current: false,
                    remote: None,
                },
                GitBranchEntry {
                    name: "release".into(),
                    current: false,
                    remote: Some("origin".into()),
                },
            ],
        }),
    );
    let scm = setup.scm.clone();
    let (dialog, cx) = cx.add_window_view(move |window, cx| {
        CreateWorktreeDialog::new(scm, "/repo", "/repo", None, window, cx)
    });
    cx.run_until_parked();

    let branch_type = dialog.read_with(cx, |dialog, _| dialog.branch_type().clone());
    assert!(
        branch_type
            .read_with(cx, |select, _| select.trigger_label())
            .starts_with("Branch type:")
    );
    branch_type.update_in(cx, |select, window, cx| select.open_menu(window, cx));
    let query = branch_type.read_with(cx, |select, _| select.query_input().clone());
    query.update_in(cx, |state, window, cx| {
        state.set_value("existing", window, cx)
    });
    assert_eq!(
        branch_type.read_with(cx, |select, cx| select.filtered(cx).len()),
        1
    );
    branch_type.update_in(cx, |select, window, cx| select.enter(window, cx));
    cx.run_until_parked();

    let existing = dialog.read_with(cx, |dialog, _| dialog.existing_branch().clone());
    assert!(
        existing
            .read_with(cx, |select, _| select.trigger_label())
            .contains("Choose a branch…")
    );
    existing.update_in(cx, |select, window, cx| select.open_menu(window, cx));
    let query = existing.read_with(cx, |select, _| select.query_input().clone());
    query.update_in(cx, |state, window, cx| {
        state.set_value("feature", window, cx)
    });
    let matches = existing.read_with(cx, |select, cx| select.filtered(cx));
    assert_eq!(matches.len(), 1);
    assert!(
        !matches
            .iter()
            .any(|option| option.label.contains("origin/release"))
    );
    existing.update_in(cx, |select, window, cx| select.enter(window, cx));
    cx.run_until_parked();
    assert!(
        existing
            .read_with(cx, |select, _| select.trigger_label())
            .contains("feature/search")
    );

    branch_type.update_in(cx, |select, window, cx| select.open_menu(window, cx));
    let query = branch_type.read_with(cx, |select, _| select.query_input().clone());
    query.update_in(cx, |state, window, cx| state.set_value("new", window, cx));
    branch_type.update_in(cx, |select, window, cx| select.enter(window, cx));
    cx.run_until_parked();

    let start_from = dialog.read_with(cx, |dialog, _| dialog.start_from().clone());
    start_from.update_in(cx, |select, window, cx| select.open_menu(window, cx));
    assert_eq!(
        start_from.read_with(cx, |select, _| select.search_placeholder().to_string()),
        "Search branches and refs…"
    );
    let query = start_from.read_with(cx, |select, _| select.query_input().clone());
    query.update_in(cx, |state, window, cx| {
        state.set_value("no-matching-branch", window, cx)
    });
    start_from.update_in(cx, |select, window, cx| select.enter(window, cx));
    assert!(
        start_from.read_with(cx, |select, _| select.is_open()),
        "Enter with no match keeps the menu"
    );
    start_from.update_in(cx, |select, window, cx| select.close(window, cx));
    assert!(!start_from.read_with(cx, |select, _| select.is_open()));
    assert!(setup.git.calls("git_worktree_create").is_empty());
}

#[gpui::test]
fn keeps_associated_sessions_by_default_when_deleting_a_worktree(cx: &mut TestAppContext) {
    let mut linked = tree();
    linked.session_ids = vec!["saved".into(), "archived".into()];
    let (page, cx, _, removed) = render_page(cx, "/keep-sessions", worktrees(vec![linked]), Ok(()));
    let deleted = delete_sessions(&page, Ok(true), cx);
    let dialog = delete_dialog(&page, cx);
    assert_eq!(
        dialog.read_with(cx, |dialog, _| dialog.button_label()),
        "Delete worktree"
    );
    dialog.update_in(cx, |dialog, window, cx| dialog.submit(window, cx));
    cx.run_until_parked();
    assert!(deleted.borrow().is_empty());
    assert_eq!(
        *removed.borrow(),
        vec![WorktreeRemoveCall {
            cwd: "/keep-sessions".into(),
            path: tree().path,
            force: true,
            keep_sessions: Some(true),
        }]
    );
}

#[gpui::test]
fn shows_no_selected_branch_after_removal_even_when_the_old_path_has_a_branch_again(
    cx: &mut TestAppContext,
) {
    let mut opts = options("/repo", &tree().path);
    opts.worktree_removed = true;
    opts.opens_new_session = true;
    let (picker, cx, _, selected) = render_picker(cx, opts, |_| {});
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.trigger_label(cx)),
        NO_BRANCH_LABEL
    );
    assert!(picker.read_with(cx, |picker, cx| picker.trigger_enabled(cx)));
    open(&picker, cx);
    let current = picker.read_with(cx, |picker, cx| {
        picker.rows(cx).iter().any(|tree| picker.is_current(tree))
    });
    assert!(!current);
    assert_ne!(
        picker.read_with(cx, |picker, _| picker.notice()),
        Some("Another working copy opens a new session.")
    );
    assert!(!picker.read_with(cx, |picker, _| picker.can_switch_branch()));
    let first = picker.read_with(cx, |picker, cx| picker.rows(cx)[0].clone());
    picker.update_in(cx, |picker, window, cx| picker.select(first, window, cx));
    cx.run_until_parked();
    assert_eq!(selected.borrow()[0].path, "/repo");
}

#[gpui::test]
fn creates_a_worktree_and_selects_it_from_the_picker(cx: &mut TestAppContext) {
    let (picker, cx, setup, selected) = render_picker(cx, options("/repo", "/repo"), |_| {});
    open(&picker, cx);
    picker.update_in(cx, |picker, window, cx| picker.start_create(window, cx));
    let dialog = picker
        .read_with(cx, |picker, _| picker.creating().cloned())
        .unwrap();
    let input = dialog.read_with(cx, |dialog, _| dialog.name_input().clone());
    input.update_in(cx, |state, window, cx| {
        state.set_value("feature/new", window, cx)
    });
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    cx.run_until_parked();
    assert_eq!(
        setup.git.calls("git_worktree_create"),
        vec![vec![
            "/repo".to_string(),
            "feature/new".to_string(),
            "HEAD".to_string(),
            String::new()
        ]]
    );
    assert_eq!(selected.borrow()[0].branch.as_deref(), Some("feature/new"));
}
