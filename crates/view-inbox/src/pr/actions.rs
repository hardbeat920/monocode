//! Port of `GithubPrActions` from src/features/inbox/ui/InboxView.tsx: the
//! merge button with its method menu, the draft, ready, close, and reopen
//! buttons, and the confirmation popover each action opens.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_ui::color::with_alpha;
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{GithubPrAction, InboxItem};
use crate::style::{
    ActionKind, PopoverAlign, action_button_with_hover_ink, closed_ink, loader, palette,
    popover_below,
};

/// One merge method of the menu.
pub struct MergeOption {
    pub action: GithubPrAction,
    pub label: &'static str,
    pub description: &'static str,
}

/// `GITHUB_PR_MERGE_OPTIONS`.
pub const GITHUB_PR_MERGE_OPTIONS: [MergeOption; 3] = [
    MergeOption {
        action: GithubPrAction::Merge,
        label: "Create a merge commit",
        description: "Add every commit to the base branch.",
    },
    MergeOption {
        action: GithubPrAction::Squash,
        label: "Squash and merge",
        description: "Combine the commits into one.",
    },
    MergeOption {
        action: GithubPrAction::Rebase,
        label: "Rebase and merge",
        description: "Add the commits without a merge commit.",
    },
];

/// The copy of a confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrActionCopy {
    pub title: &'static str,
    pub detail: String,
    pub confirm: &'static str,
    pub progress: &'static str,
}

/// `githubPrActionCopy`.
pub fn github_pr_action_copy(
    action: GithubPrAction,
    base_ref: &str,
    head_ref: &str,
) -> PrActionCopy {
    let source = if head_ref.is_empty() {
        "this branch".to_string()
    } else {
        format!("“{head_ref}”")
    };
    let destination = if base_ref.is_empty() {
        "the base branch".to_string()
    } else {
        format!("“{base_ref}”")
    };
    let (title, detail, confirm, progress) = match action {
        GithubPrAction::Merge => (
            "Merge this pull request?",
            format!(
                "Every commit from {source} will be added to {destination} with a merge commit."
            ),
            "Merge pull request",
            "Merging…",
        ),
        GithubPrAction::Squash => (
            "Squash and merge?",
            format!("The commits from {source} will be combined into one commit on {destination}."),
            "Squash and merge",
            "Merging…",
        ),
        GithubPrAction::Rebase => (
            "Rebase and merge?",
            format!("The commits from {source} will be rebased individually onto {destination}."),
            "Rebase and merge",
            "Merging…",
        ),
        GithubPrAction::Draft => (
            "Convert to draft?",
            "Reviewers will see that this pull request is not ready to merge.".into(),
            "Convert to draft",
            "Converting…",
        ),
        GithubPrAction::Ready => (
            "Mark as ready for review?",
            "Reviewers will see that this pull request is ready for feedback.".into(),
            "Ready for review",
            "Updating…",
        ),
        GithubPrAction::Close => (
            "Close this pull request?",
            "The pull request will close without merging. You can reopen it later.".into(),
            "Close pull request",
            "Closing…",
        ),
        GithubPrAction::Reopen => (
            "Reopen this pull request?",
            "The pull request will return to the open state.".into(),
            "Reopen pull request",
            "Reopening…",
        ),
    };
    PrActionCopy {
        title,
        detail,
        confirm,
        progress,
    }
}

/// The lifecycle buttons a pull request shows, in order. `Merge` stands for
/// the merge group (button plus "Merge options").
pub fn pr_action_buttons(item: &InboxItem) -> Vec<GithubPrAction> {
    let state = item.state.trim().to_lowercase();
    let mut buttons = Vec::new();
    if state == "open" && !item.draft {
        buttons.push(GithubPrAction::Merge);
    }
    if state == "open" && item.draft {
        buttons.push(GithubPrAction::Ready);
    }
    if state == "open" && !item.draft {
        buttons.push(GithubPrAction::Draft);
    }
    if state == "open" {
        buttons.push(GithubPrAction::Close);
    }
    if state == "closed" {
        buttons.push(GithubPrAction::Reopen);
    }
    buttons
}

/// The label of a lifecycle button.
pub fn pr_action_label(action: GithubPrAction, merge_action: GithubPrAction) -> &'static str {
    match action {
        GithubPrAction::Merge | GithubPrAction::Squash | GithubPrAction::Rebase => {
            if merge_action == GithubPrAction::Merge {
                "Merge pull request"
            } else {
                GITHUB_PR_MERGE_OPTIONS
                    .iter()
                    .find(|option| option.action == merge_action)
                    .map(|option| option.label)
                    .unwrap_or("Merge pull request")
            }
        }
        GithubPrAction::Ready => "Ready for review",
        GithubPrAction::Draft => "Convert to draft",
        GithubPrAction::Close => "Close pull request",
        GithubPrAction::Reopen => "Reopen pull request",
    }
}

fn action_icon(action: GithubPrAction) -> IconName {
    match action {
        GithubPrAction::Merge | GithubPrAction::Squash | GithubPrAction::Rebase => {
            IconName::GitMerge
        }
        GithubPrAction::Ready | GithubPrAction::Reopen => IconName::GitPullRequest,
        GithubPrAction::Draft => IconName::GitPullRequestDraft,
        GithubPrAction::Close => IconName::GitPullRequestClosed,
    }
}

type Handler = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;
type ActionHandler = std::rc::Rc<dyn Fn(GithubPrAction, &mut Window, &mut App)>;

/// What `GithubPrActions` draws from.
pub struct PrActionsProps {
    pub item: InboxItem,
    pub base_ref: String,
    pub head_ref: String,
    pub merge_action: GithubPrAction,
    pub merge_menu_open: bool,
    /// The action being confirmed; its button anchors the popover.
    pub confirmation: Option<GithubPrAction>,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub animate: bool,
    /// A lifecycle button: open the confirmation.
    pub on_ask: ActionHandler,
    pub on_toggle_merge_menu: Handler,
    pub on_pick_merge: ActionHandler,
    pub on_dismiss: Handler,
    pub on_confirm: Handler,
}

/// `GithubPrActions`: the buttons, as children of the detail action row.
pub fn github_pr_actions(props: PrActionsProps, cx: &App) -> Vec<AnyElement> {
    let theme = Theme::of(cx);
    let c = theme.colors;
    let mut out = Vec::new();
    let confirming = props.confirmation;
    for action in pr_action_buttons(&props.item) {
        let confirm_here = match confirming {
            Some(confirming) if action == GithubPrAction::Merge => confirming.is_merge(),
            Some(confirming) => confirming == action,
            None => false,
        };
        let mut cell = div().relative().flex_none();
        if action == GithubPrAction::Merge {
            let on_ask = props.on_ask.clone();
            let toggle = props.on_toggle_merge_menu.clone();
            let press = with_alpha(c.background_base, 0.10);
            let mut main = div()
                .id("pr-merge")
                .flex()
                .items_center()
                .gap(u(6.))
                .px(u(12.))
                .text_px(theme.text.label)
                .medium()
                .child(
                    icon(IconName::GitMerge)
                        .size(u(14.))
                        .text_color(c.background_base),
                )
                .child(pr_action_label(GithubPrAction::Merge, props.merge_action));
            let mut chevron = div()
                .id("pr-merge-options")
                .flex()
                .w(u(28.))
                .items_center()
                .justify_center()
                .border_l_1()
                .border_color(with_alpha(c.background_base, 0.2))
                .tooltip(monocode_ui::widgets::tooltip("Merge options"))
                .child(
                    icon(IconName::ChevronDown)
                        .size(u(12.))
                        .text_color(c.background_base),
                );
            if props.busy {
                main = main.opacity(0.4);
                chevron = chevron.opacity(0.4);
            } else {
                let merge_action = props.merge_action;
                main = main
                    .hover(move |s| s.bg(press))
                    .on_click(move |_, window, cx| on_ask(merge_action, window, cx));
                chevron = chevron
                    .hover(move |s| s.bg(press))
                    .on_click(move |_, window, cx| toggle(window, cx));
            }
            cell = cell.child(
                div()
                    .flex()
                    .h(u(28.))
                    .overflow_hidden()
                    .rounded(u(theme.radius.md))
                    .bg(c.content)
                    .text_color(c.background_base)
                    .child(main)
                    .child(chevron),
            );
            if props.merge_menu_open {
                cell = cell.child(popover_below(
                    PopoverAlign::Start,
                    4.,
                    merge_menu(&props, cx),
                    cx,
                ));
            }
        } else {
            let on_ask = props.on_ask.clone();
            let mut button = action_button_with_hover_ink(
                ElementId::Name(format!("pr-action-{}", pr_action_label(action, action)).into()),
                ActionKind::Outline,
                Some(action_icon(action)),
                pr_action_label(action, props.merge_action),
                props.busy,
                (action == GithubPrAction::Close).then(palette::rose_400),
                cx,
            );
            if !props.busy {
                button = button.on_click(move |_, window, cx| on_ask(action, window, cx));
            }
            cell = cell.child(button);
        }
        if confirm_here && let Some(confirming) = confirming {
            cell = cell.child(popover_below(
                PopoverAlign::Start,
                5.,
                confirmation(&props, confirming, cx),
                cx,
            ));
        }
        out.push(cell.into_any_element());
    }
    if let Some(notice) = props.notice.clone() {
        out.push(
            div()
                .text_px(theme.text.caption)
                .text_color(theme.content(0.55))
                .child(notice)
                .into_any_element(),
        );
    }
    out
}

fn merge_menu(props: &PrActionsProps, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let mut list = div().flex().flex_col().p(u(4.));
    for option in GITHUB_PR_MERGE_OPTIONS.iter() {
        let selected = option.action == props.merge_action;
        let pick = props.on_pick_merge.clone();
        let action = option.action;
        let hover = theme.content(0.08);
        let mut row = div()
            .id(SharedString::from(format!("merge-option-{}", option.label)))
            .flex()
            .w_full()
            .items_start()
            .gap(u(8.))
            .rounded(u(theme.radius.lg))
            .px(u(8.))
            .py(u(8.))
            .hover(move |s| s.bg(hover))
            .on_click(move |_, window, cx| pick(action, window, cx))
            .child(
                div()
                    .mt(u(4.))
                    .flex_none()
                    .size(u(6.))
                    .rounded_full()
                    .bg(if selected {
                        theme.colors.success
                    } else {
                        theme.content(0.20)
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .text_px(theme.text.label)
                            .medium()
                            .leading(theme.leading.tight)
                            .child(option.label),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .text_px(theme.text.caption)
                            .leading(theme.leading.snug)
                            .text_color(theme.content(0.45))
                            .child(option.description),
                    ),
            );
        row = if selected {
            row.bg(theme.colors.selection)
                .text_color(theme.colors.content)
        } else {
            row.text_color(theme.content(0.75))
        };
        list = list.child(row);
    }
    let dismiss = props.on_toggle_merge_menu.clone();
    popover_frame("pr-merge-menu")
        .width(260.)
        .animate(props.animate)
        .child(
            div()
                .on_mouse_down_out(move |_, window, cx| dismiss(window, cx))
                .child(list),
        )
        .into_any_element()
}

fn confirmation(props: &PrActionsProps, action: GithubPrAction, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let c = theme.colors;
    let copy = github_pr_action_copy(action, &props.base_ref, &props.head_ref);
    let busy = props.busy;
    let (fill, ink, hover) = if action == GithubPrAction::Close {
        (
            with_alpha(palette::rose_500(), 0.20),
            if theme.is_dark() {
                palette::rose_300()
            } else {
                palette::rose_700()
            },
            with_alpha(palette::rose_500(), 0.30),
        )
    } else if action.is_merge() {
        (
            with_alpha(palette::emerald_500(), 0.20),
            if theme.is_dark() {
                palette::emerald_300()
            } else {
                palette::emerald_700()
            },
            with_alpha(palette::emerald_500(), 0.30),
        )
    } else {
        (c.content, c.background_base, theme.content(0.80))
    };
    let dismiss = props.on_dismiss.clone();
    let cancel_hover = theme.content(0.08);
    let cancel_ink = c.content;
    let mut cancel = div()
        .id("pr-confirm-cancel")
        .flex()
        .h(u(28.))
        .items_center()
        .rounded(u(theme.radius.md))
        .px(u(12.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.65))
        .child("Cancel");
    let mut confirm = div()
        .id("pr-confirm")
        .flex()
        .h(u(28.))
        .items_center()
        .gap(u(6.))
        .rounded(u(theme.radius.md))
        .px(u(12.))
        .text_px(theme.text.label)
        .medium()
        .bg(fill)
        .text_color(ink);
    if busy {
        cancel = cancel.opacity(0.4);
        confirm = confirm
            .opacity(0.6)
            .child(loader("pr-confirm-busy", 14., ink))
            .child(copy.progress);
    } else {
        let on_cancel = dismiss.clone();
        cancel = cancel
            .hover(move |s| s.bg(cancel_hover).text_color(cancel_ink))
            .on_click(move |_, window, cx| on_cancel(window, cx));
        let on_confirm = props.on_confirm.clone();
        confirm = confirm
            .hover(move |s| s.bg(hover))
            .on_click(move |_, window, cx| on_confirm(window, cx))
            .child(copy.confirm);
    }
    popover_frame("pr-confirmation")
        .width(320.)
        .animate(props.animate)
        .child(
            div()
                .when(!busy, |frame| {
                    frame.on_mouse_down_out(move |_, window, cx| dismiss(window, cx))
                })
                .p(u(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(u(4.))
                        .child(
                            div()
                                .text_px(theme.text.body)
                                .medium()
                                .text_color(c.content)
                                .child(copy.title),
                        )
                        .child(
                            div()
                                .text_px(theme.text.label)
                                .leading(theme.leading.snug)
                                .text_color(theme.content(0.55))
                                .child(copy.detail.clone()),
                        ),
                )
                .when_some(props.error.clone(), |frame, error| {
                    frame.child(
                        div()
                            .mt(u(8.))
                            .text_px(theme.text.caption)
                            .leading(theme.leading.snug)
                            .text_color(closed_ink())
                            .child(error),
                    )
                })
                .child(
                    div()
                        .mt(u(12.))
                        .flex()
                        .justify_end()
                        .gap(u(8.))
                        .child(cancel)
                        .child(confirm),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::InboxKind;

    fn pr(state: &str, draft: bool) -> InboxItem {
        let mut item = InboxItem::github(InboxKind::Pr, "acme/web", 157, "A pull request");
        item.state = state.into();
        item.draft = draft;
        item
    }

    #[test]
    fn offers_github_style_actions_for_an_open_pull_request() {
        assert_eq!(
            pr_action_buttons(&pr("open", false)),
            [
                GithubPrAction::Merge,
                GithubPrAction::Draft,
                GithubPrAction::Close
            ]
        );
        assert_eq!(
            pr_action_label(GithubPrAction::Merge, GithubPrAction::Merge),
            "Merge pull request"
        );
        assert_eq!(
            pr_action_label(GithubPrAction::Merge, GithubPrAction::Squash),
            "Squash and merge"
        );
    }

    #[test]
    fn adapts_pull_request_actions_to_draft_and_closed_states() {
        assert_eq!(
            pr_action_buttons(&pr("open", true)),
            [GithubPrAction::Ready, GithubPrAction::Close]
        );
        assert_eq!(
            pr_action_buttons(&pr("closed", false)),
            [GithubPrAction::Reopen]
        );
        assert!(pr_action_buttons(&pr("merged", false)).is_empty());
    }

    #[test]
    fn confirmation_copy_names_both_branches() {
        let copy = github_pr_action_copy(GithubPrAction::Squash, "main", "feature/x");
        assert_eq!(copy.title, "Squash and merge?");
        assert_eq!(
            copy.detail,
            "The commits from “feature/x” will be combined into one commit on “main”."
        );
        let copy = github_pr_action_copy(GithubPrAction::Merge, "", "");
        assert!(copy.detail.contains("this branch"));
        assert!(copy.detail.contains("the base branch"));
    }
}
