//! Port of `GithubPrActions` from src/features/inbox/ui/InboxView.tsx: merge
//! (with a method menu), draft, ready, close, and reopen for a GitHub pull
//! request, each behind a confirmation popover.

use gpui::{
    AnyElement, Context, EventEmitter, InteractiveElement as _, IntoElement, MouseDownEvent,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::GitHubWorkItem;
use crate::model::pr_actions::{
    ActionCopy, GITHUB_PR_MERGE_OPTIONS, GithubPrAction, github_pr_action_copy, merge_button_label,
    merge_notice,
};
use crate::scm::Scm;
use crate::ui::common::{
    BoundsCell, PopoverPlacement, anchored_popover, contains, palette, spin_icon, track_bounds,
    with_alpha,
};

/// The inbox item fields the actions read and update.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrItem {
    pub project_path: String,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub draft: bool,
    pub updated_at: String,
}

impl PrItem {
    /// `{ ...item, ...next, projectPath: item.projectPath, provider: "github" }`.
    pub fn merged_with(&self, next: &GitHubWorkItem) -> Self {
        Self {
            project_path: self.project_path.clone(),
            repo: next.repo.clone(),
            number: next.number,
            title: next.title.clone(),
            url: next.url.clone(),
            state: next.state.clone(),
            draft: next.draft,
            updated_at: next.updated_at.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrActionsEvent {
    /// `onChange`: GitHub's fresh state after an action.
    Changed(PrItem),
}

/// Which button a confirmation opened from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    Merge,
    Ready,
    Draft,
    Close,
    Reopen,
}

pub struct GithubPrActions {
    scm: Scm,
    item: PrItem,
    base_ref: String,
    head_ref: String,
    merge_action: GithubPrAction,
    merge_menu_open: bool,
    confirmation: Option<(GithubPrAction, Anchor)>,
    busy: bool,
    action_error: Option<String>,
    notice: Option<&'static str>,
    menu_toggle: BoundsCell,
    task: Option<Task<()>>,
}

impl EventEmitter<PrActionsEvent> for GithubPrActions {}

impl GithubPrActions {
    pub fn new(
        scm: Scm,
        item: PrItem,
        base_ref: impl Into<String>,
        head_ref: impl Into<String>,
    ) -> Self {
        Self {
            scm,
            item,
            base_ref: base_ref.into(),
            head_ref: head_ref.into(),
            merge_action: GithubPrAction::Merge,
            merge_menu_open: false,
            confirmation: None,
            busy: false,
            action_error: None,
            notice: None,
            menu_toggle: BoundsCell::default(),
            task: None,
        }
    }

    pub fn set_item(&mut self, item: PrItem, cx: &mut Context<Self>) {
        self.item = item;
        cx.notify();
    }

    fn state(&self) -> String {
        self.item.state.trim().to_lowercase()
    }

    // Reading, for tests.

    pub fn merge_action(&self) -> GithubPrAction {
        self.merge_action
    }

    pub fn merge_menu_open(&self) -> bool {
        self.merge_menu_open
    }

    pub fn confirmation(&self) -> Option<GithubPrAction> {
        self.confirmation.map(|(action, _)| action)
    }

    pub fn confirmation_copy(&self) -> Option<ActionCopy> {
        self.confirmation
            .map(|(action, _)| github_pr_action_copy(action, &self.base_ref, &self.head_ref))
    }

    pub fn action_error(&self) -> Option<&str> {
        self.action_error.as_deref()
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice
    }

    /// The labels of the buttons shown, in order.
    pub fn buttons(&self) -> Vec<&'static str> {
        let state = self.state();
        let mut out = Vec::new();
        if state == "open" && !self.item.draft {
            out.push(merge_button_label(self.merge_action));
        }
        if state == "open" && self.item.draft {
            out.push("Ready for review");
        }
        if state == "open" && !self.item.draft {
            out.push("Convert to draft");
        }
        if state == "open" {
            out.push("Close pull request");
        }
        if state == "closed" {
            out.push("Reopen pull request");
        }
        out
    }

    // Actions.

    pub fn toggle_merge_menu(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            self.merge_menu_open = !self.merge_menu_open;
            cx.notify();
        }
    }

    pub fn choose_merge(&mut self, action: GithubPrAction, cx: &mut Context<Self>) {
        self.merge_action = action;
        self.merge_menu_open = false;
        cx.notify();
    }

    /// `askToRun`.
    pub fn ask(&mut self, action: GithubPrAction, cx: &mut Context<Self>) {
        let anchor = match action {
            GithubPrAction::Merge | GithubPrAction::Squash | GithubPrAction::Rebase => {
                Anchor::Merge
            }
            GithubPrAction::Ready => Anchor::Ready,
            GithubPrAction::Draft => Anchor::Draft,
            GithubPrAction::Close => Anchor::Close,
            GithubPrAction::Reopen => Anchor::Reopen,
        };
        self.merge_menu_open = false;
        self.action_error = None;
        self.notice = None;
        self.confirmation = Some((action, anchor));
        cx.notify();
    }

    pub fn dismiss_confirmation(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.confirmation = None;
        self.action_error = None;
        cx.notify();
    }

    /// `runAction`.
    pub fn run(&mut self, cx: &mut Context<Self>) {
        let Some((action, _)) = self.confirmation else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.action_error = None;
        let (cwd, repo, number) = (
            self.item.project_path.clone(),
            self.item.repo.clone(),
            self.item.number,
        );
        let call = self.scm.run(cx, move |git| {
            git.git_github_pr_action(&cwd, &repo, number, action.as_str())
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = call.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(next) => {
                        this.confirmation = None;
                        this.notice = merge_notice(action, &next.state);
                        let merged = this.item.merged_with(&next);
                        this.item = merged.clone();
                        cx.emit(PrActionsEvent::Changed(merged));
                    }
                    Err(error) => this.action_error = Some(error),
                }
                this.busy = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn outline_button(
        &self,
        id: &'static str,
        glyph: IconName,
        label: &'static str,
        close: bool,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        let ink = theme.content(0.80);
        let mut el = div()
            .id(id)
            .flex()
            .h(u(28.))
            .items_center()
            .gap(u(6.))
            .rounded(u(6.))
            .border_1()
            .border_color(theme.content(0.15))
            .px(u(12.))
            .text_px(12.)
            .text_color(ink)
            .child(icon(glyph).size(u(14.)).text_color(ink))
            .child(label);
        if self.busy {
            el = el.opacity(0.4);
        } else {
            el = el.hover(move |s| {
                let s = s.bg(theme.content(0.05));
                if close {
                    s.text_color(palette::rose_400())
                } else {
                    s
                }
            });
        }
        el
    }

    fn render_confirmation(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some((action, _)) = self.confirmation else {
            return div().into_any_element();
        };
        let copy = github_pr_action_copy(action, &self.base_ref, &self.head_ref);
        let c = theme.colors;
        let dark = theme.is_dark();
        let (bg, hover, ink) = if action == GithubPrAction::Close {
            (
                with_alpha(palette::rose_500(), 0.20),
                with_alpha(palette::rose_500(), 0.30),
                if dark {
                    palette::rose_300()
                } else {
                    palette::rose_700()
                },
            )
        } else if action.is_merge() {
            (
                with_alpha(palette::emerald_500(), 0.20),
                with_alpha(palette::emerald_500(), 0.30),
                if dark {
                    palette::emerald_300()
                } else {
                    palette::emerald_700()
                },
            )
        } else {
            (c.content, theme.content(0.80), c.background_base)
        };
        let busy = self.busy;
        let mut confirm = div()
            .id("confirm-pr-action")
            .flex()
            .h(u(28.))
            .items_center()
            .gap(u(6.))
            .rounded(u(6.))
            .px(u(12.))
            .text_px(12.)
            .medium()
            .bg(bg)
            .text_color(ink);
        if busy {
            confirm = confirm
                .opacity(0.6)
                .child(spin_icon("pr-action-spin", 14., ink))
                .child(copy.progress);
        } else {
            confirm = confirm
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(|this, _, _, cx| this.run(cx)))
                .child(copy.confirm);
        }
        let mut cancel = div()
            .id("cancel-pr-action")
            .flex()
            .h(u(28.))
            .items_center()
            .rounded(u(6.))
            .px(u(12.))
            .text_px(12.)
            .text_color(theme.content(0.65))
            .child("Cancel");
        if busy {
            cancel = cancel.opacity(0.4);
        } else {
            cancel = cancel
                .hover(|s| s.bg(theme.content(0.08)).text_color(c.content))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_confirmation(cx)));
        }
        let mut body = div().p(u(12.)).child(
            div()
                .flex()
                .flex_col()
                .gap(u(4.))
                .child(
                    div()
                        .text_px(13.)
                        .medium()
                        .text_color(c.content)
                        .child(copy.title.clone()),
                )
                .child(
                    div()
                        .text_px(12.)
                        .text_color(theme.content(0.55))
                        .child(copy.detail.clone()),
                ),
        );
        if let Some(error) = &self.action_error {
            body = body.child(
                div()
                    .mt(u(8.))
                    .text_px(11.)
                    .text_color(palette::rose_400())
                    .child(error.clone()),
            );
        }
        body = body.child(
            div()
                .mt(u(12.))
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(cancel)
                .child(confirm),
        );
        div()
            .id("pr-confirmation")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_confirmation(cx)))
            .child(
                popover_frame(SharedString::from(format!(
                    "pr-confirm-{}",
                    action.as_str()
                )))
                .width(320.)
                .child(body),
            )
            .into_any_element()
    }

    fn render_merge_menu(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = theme.colors;
        let mut list = div().flex().flex_col().p(u(4.));
        for option in GITHUB_PR_MERGE_OPTIONS {
            let chosen = option.action == self.merge_action;
            let action = option.action;
            let mut row = div()
                .id(option.action.as_str())
                .flex()
                .w_full()
                .items_start()
                .gap(u(8.))
                .rounded(u(8.))
                .px(u(8.))
                .py(u(8.))
                .on_click(cx.listener(move |this, _, _, cx| this.choose_merge(action, cx)));
            row = if chosen {
                row.bg(c.selection).text_color(c.content)
            } else {
                row.text_color(theme.content(0.75))
                    .hover(|s| s.bg(theme.content(0.08)))
            };
            row = row
                .child(
                    div()
                        .mt(u(4.))
                        .flex_none()
                        .size(u(6.))
                        .rounded_full()
                        .bg(if chosen {
                            c.success
                        } else {
                            theme.content(0.20)
                        }),
                )
                .child(
                    div()
                        .min_w_0()
                        .child(div().text_px(12.).medium().child(option.label))
                        .child(
                            div()
                                .mt(u(2.))
                                .text_px(11.)
                                .text_color(theme.content(0.45))
                                .child(option.description),
                        ),
                );
            list = list.child(row);
        }
        div()
            .id("merge-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !contains(&this.menu_toggle, event.position) {
                    this.merge_menu_open = false;
                    cx.notify();
                }
            }))
            .child(popover_frame("merge-menu-frame").width(260.).child(list))
            .into_any_element()
    }
}

impl Render for GithubPrActions {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let state = self.state();
        let busy = self.busy;
        let anchor = self.confirmation.map(|(_, anchor)| anchor);
        let confirmation = if self.confirmation.is_some() {
            Some(self.render_confirmation(&theme, cx))
        } else {
            None
        };
        let mut confirmation = confirmation;
        let mut attach = |el: gpui::Div, here: Anchor, window: &Window| -> gpui::Div {
            if anchor == Some(here)
                && let Some(popover) = confirmation.take()
            {
                return el.child(anchored_popover(
                    PopoverPlacement::BottomStart,
                    5.,
                    theme.layer.popover,
                    popover,
                    window,
                ));
            }
            el
        };
        let mut row = div().flex().flex_wrap().items_center().gap(u(8.));
        if state == "open" && !self.item.draft {
            let ink = c.background_base;
            let action = self.merge_action;
            let mut primary = div()
                .id("merge-primary")
                .flex()
                .items_center()
                .gap(u(6.))
                .px(u(12.))
                .text_px(12.)
                .medium()
                .child(icon(IconName::GitMerge).size(u(14.)).text_color(ink))
                .child(merge_button_label(action));
            let mut options = div()
                .id("merge-options")
                .relative()
                .child(track_bounds(&self.menu_toggle))
                .flex()
                .w(u(28.))
                .items_center()
                .justify_center()
                .border_l_1()
                .border_color(with_alpha(c.background_base, 0.20))
                .child(icon(IconName::ChevronDown).size(u(12.)).text_color(ink));
            if busy {
                primary = primary.opacity(0.4);
                options = options.opacity(0.4);
            } else {
                primary = primary
                    .hover(|s| s.bg(with_alpha(c.background_base, 0.10)))
                    .on_click(cx.listener(move |this, _, _, cx| this.ask(action, cx)));
                options = options
                    .hover(|s| s.bg(with_alpha(c.background_base, 0.10)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_merge_menu(cx)));
            }
            let mut group = div()
                .relative()
                .flex()
                .h(u(28.))
                .overflow_hidden()
                .rounded(u(6.))
                .bg(c.content)
                .text_color(ink)
                .child(primary)
                .child(options);
            if self.merge_menu_open {
                let menu = self.render_merge_menu(&theme, cx);
                group = group.child(anchored_popover(
                    PopoverPlacement::BottomStart,
                    4.,
                    theme.layer.popover,
                    menu,
                    window,
                ));
            }
            row = row.child(attach(group, Anchor::Merge, window));
        }
        if state == "open" && self.item.draft {
            let button = self
                .outline_button(
                    "ready",
                    IconName::GitPullRequest,
                    "Ready for review",
                    false,
                    &theme,
                )
                .when(!busy, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| this.ask(GithubPrAction::Ready, cx)))
                });
            row = row.child(attach(
                div().relative().child(button),
                Anchor::Ready,
                window,
            ));
        }
        if state == "open" && !self.item.draft {
            let button = self
                .outline_button(
                    "draft",
                    IconName::GitPullRequestDraft,
                    "Convert to draft",
                    false,
                    &theme,
                )
                .when(!busy, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| this.ask(GithubPrAction::Draft, cx)))
                });
            row = row.child(attach(
                div().relative().child(button),
                Anchor::Draft,
                window,
            ));
        }
        if state == "open" {
            let button = self
                .outline_button(
                    "close",
                    IconName::GitPullRequestClosed,
                    "Close pull request",
                    true,
                    &theme,
                )
                .when(!busy, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| this.ask(GithubPrAction::Close, cx)))
                });
            row = row.child(attach(
                div().relative().child(button),
                Anchor::Close,
                window,
            ));
        }
        if state == "closed" {
            let button = self
                .outline_button(
                    "reopen",
                    IconName::GitPullRequest,
                    "Reopen pull request",
                    false,
                    &theme,
                )
                .when(!busy, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| this.ask(GithubPrAction::Reopen, cx)))
                });
            row = row.child(attach(
                div().relative().child(button),
                Anchor::Reopen,
                window,
            ));
        }
        if let Some(notice) = self.notice {
            row = row.child(
                div()
                    .text_px(11.)
                    .text_color(theme.content(0.55))
                    .child(notice),
            );
        }
        row
    }
}
