//! Port of src/features/source-control/ui/GithubPrActions.test.ts.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, TestAppContext};

use super::support::{Recorded, setup};
use crate::git::GitHubWorkItem;
use crate::model::pr_actions::GithubPrAction;
use crate::ui::pr_actions::{GithubPrActions, PrActionsEvent, PrItem};

fn pr() -> PrItem {
    PrItem {
        project_path: "/tmp/web".into(),
        repo: "acme/web".into(),
        number: 42,
        title: "Ship the new inbox".into(),
        url: "https://github.com/acme/web/pull/42".into(),
        state: "open".into(),
        draft: false,
        updated_at: "2026-09-16T08:00:00Z".into(),
    }
}

#[gpui::test]
fn selects_a_merge_method_confirms_it_and_publishes_the_fresh_state(cx: &mut TestAppContext) {
    let setup = setup(cx, Recorded::default().hooks());
    let item = pr();
    setup.git.set_pr_action(GitHubWorkItem {
        kind: "pr".into(),
        number: item.number,
        title: item.title.clone(),
        url: item.url.clone(),
        state: "merged".into(),
        state_reason: String::new(),
        created_at: String::new(),
        updated_at: "2026-09-16T08:05:00Z".into(),
        labels: Vec::new(),
        assignees: Vec::new(),
        draft: false,
        repo: item.repo.clone(),
    });
    let scm = setup.scm.clone();
    let actions = cx.new(|_| GithubPrActions::new(scm, item.clone(), "main", "feature/inbox"));
    let changes = Rc::new(RefCell::new(Vec::new()));
    {
        let changes = changes.clone();
        cx.update(|cx| {
            cx.subscribe(&actions, move |_, event: &PrActionsEvent, _| {
                changes.borrow_mut().push(event.clone())
            })
            .detach()
        });
    }

    actions.update(cx, |actions, cx| actions.toggle_merge_menu(cx));
    assert!(actions.read_with(cx, |actions, _| actions.merge_menu_open()));
    actions.update(cx, |actions, cx| {
        actions.choose_merge(GithubPrAction::Squash, cx)
    });
    assert_eq!(
        actions.read_with(cx, |actions, _| actions.buttons()[0]),
        "Squash and merge"
    );

    let chosen = actions.read_with(cx, |actions, _| actions.merge_action());
    actions.update(cx, |actions, cx| actions.ask(chosen, cx));
    let copy = actions
        .read_with(cx, |actions, _| actions.confirmation_copy())
        .unwrap();
    assert_eq!(copy.title, "Squash and merge?");
    assert!(
        copy.detail
            .contains("from “feature/inbox” will be combined into one commit on “main”")
    );
    assert_eq!(copy.confirm, "Squash and merge");

    actions.update(cx, |actions, cx| actions.run(cx));
    cx.run_until_parked();
    assert_eq!(
        setup.git.calls("git_github_pr_action"),
        vec![vec![
            "/tmp/web".to_string(),
            "acme/web".to_string(),
            "42".to_string(),
            "squash".to_string()
        ]]
    );
    let expected = PrItem {
        state: "merged".into(),
        updated_at: "2026-09-16T08:05:00Z".into(),
        ..item
    };
    assert_eq!(*changes.borrow(), vec![PrActionsEvent::Changed(expected)]);
    assert_eq!(
        actions.read_with(cx, |actions, _| actions.confirmation()),
        None
    );
}

#[gpui::test]
fn keeps_a_failed_close_action_open_with_githubs_error(cx: &mut TestAppContext) {
    let setup = setup(cx, Recorded::default().hooks());
    setup.git.fail(
        "git_github_pr_action",
        Some("You do not have permission to close this pull request"),
    );
    let scm = setup.scm.clone();
    let actions = cx.new(|_| GithubPrActions::new(scm, pr(), "main", "feature/inbox"));
    assert!(actions.read_with(cx, |actions, _| {
        actions.buttons().contains(&"Close pull request")
    }));
    actions.update(cx, |actions, cx| actions.ask(GithubPrAction::Close, cx));
    let copy = actions
        .read_with(cx, |actions, _| actions.confirmation_copy())
        .unwrap();
    assert_eq!(copy.title, "Close this pull request?");
    actions.update(cx, |actions, cx| actions.run(cx));
    cx.run_until_parked();
    actions.read_with(cx, |actions, _| {
        assert_eq!(actions.confirmation(), Some(GithubPrAction::Close));
        assert!(
            actions
                .action_error()
                .unwrap()
                .contains("You do not have permission to close this pull request")
        );
    });
}
