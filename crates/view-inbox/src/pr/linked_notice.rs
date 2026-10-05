//! Port of src/features/inbox/ui/LinkedWorkItemUpdateNotice.tsx: the glass
//! card in a session's corner when its linked GitHub item has new comments,
//! reviews, or commits, with actions to open them, hand them to the agent,
//! or clean the session up once the item is merged or closed.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, ElementId, EventEmitter,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::color::with_alpha;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    DataTask, InboxServices, LinkedWorkItemActivityEntry, LinkedWorkItemActivityKind,
    LinkedWorkItemUpdateCard, LinkedWorkItemUpdateStatus, WorkItemKind,
};
use crate::model::{
    LinkedWorkItemTerminalState, format_relative_time, linked_work_item_terminal_state,
    linked_work_item_update_summary,
};
use crate::style::{loader, merged_ink, open_ink};

/// `entryKindLabel`.
pub fn entry_kind_label(entry: &LinkedWorkItemActivityEntry) -> String {
    match entry.kind {
        LinkedWorkItemActivityKind::Commit => {
            format!("Commit {}", entry.id.chars().take(7).collect::<String>())
        }
        LinkedWorkItemActivityKind::Review => "Review".into(),
        LinkedWorkItemActivityKind::ReviewComment => "Review comment".into(),
        LinkedWorkItemActivityKind::Comment => "Comment".into(),
    }
}

/// What the notice's buttons say and do, from the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeText {
    pub kind_label: &'static str,
    /// The latest entry is a comment or review: "Open" opens the discussion.
    pub discussion: bool,
    pub open_label: &'static str,
    pub agent_label: &'static str,
    pub terminal: Option<LinkedWorkItemTerminalState>,
    pub terminal_label: &'static str,
    pub summary: String,
}

impl NoticeText {
    pub fn new(card: &LinkedWorkItemUpdateCard) -> Self {
        let latest = card.entries.first();
        let discussion = latest.is_some_and(|entry| {
            matches!(
                entry.kind,
                LinkedWorkItemActivityKind::Comment
                    | LinkedWorkItemActivityKind::Review
                    | LinkedWorkItemActivityKind::ReviewComment
            )
        });
        let commit = latest.is_some_and(|entry| entry.kind == LinkedWorkItemActivityKind::Commit);
        let terminal = linked_work_item_terminal_state(card);
        Self {
            kind_label: if card.kind == WorkItemKind::Pr {
                "Pull request"
            } else {
                "Issue"
            },
            discussion,
            open_label: if commit {
                "Open commit"
            } else if card.kind == WorkItemKind::Pr {
                "Open PR"
            } else {
                "Open issue"
            },
            agent_label: if discussion {
                "Address with agent"
            } else if commit {
                "Review with agent"
            } else {
                "Continue with agent"
            },
            terminal,
            terminal_label: match terminal {
                Some(LinkedWorkItemTerminalState::PrMerged) => "Pull request merged",
                Some(LinkedWorkItemTerminalState::PrClosed) => "Pull request closed",
                Some(LinkedWorkItemTerminalState::IssueClosed) => "Issue closed",
                None => "",
            },
            summary: linked_work_item_update_summary(card),
        }
    }
}

type Action = Rc<dyn Fn(&mut Window, &mut App)>;
type CardAction = Rc<dyn Fn(&LinkedWorkItemUpdateCard, &mut Window, &mut App)>;
type Cleanup = Rc<dyn Fn(&mut App) -> DataTask<bool>>;
type AnnounceFn = Rc<dyn Fn(&str, &LinkedWorkItemUpdateCard, &mut App)>;

/// The notice's handlers.
#[derive(Clone)]
pub struct NoticeHandlers {
    pub on_acknowledge: Action,
    pub on_dismiss: Action,
    pub on_open_discussion: Action,
    /// "Address with agent": the host adds `linkedWorkItemActivityPrompt(card)`
    /// to the chat.
    pub on_add_to_chat: CardAction,
    /// `announceLinkedActivity(sessionId, card)`: the host plays the cue
    /// once per new activity.
    pub on_announce: Option<AnnounceFn>,
    pub on_archive_session: Option<Cleanup>,
    pub on_delete_session: Option<Cleanup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupAction {
    Archive,
    Delete,
}

/// A notice was dismissed or acted on; owners may drop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoticeDone;

pub struct LinkedWorkItemUpdateNotice {
    services: Rc<dyn InboxServices>,
    session_id: String,
    card: Option<LinkedWorkItemUpdateCard>,
    handlers: NoticeHandlers,
    cleanup: Option<CleanupAction>,
    animate: bool,
    _cleanup_task: Option<Task<()>>,
}

impl EventEmitter<NoticeDone> for LinkedWorkItemUpdateNotice {}

impl LinkedWorkItemUpdateNotice {
    pub fn new(
        services: Rc<dyn InboxServices>,
        session_id: String,
        card: Option<LinkedWorkItemUpdateCard>,
        handlers: NoticeHandlers,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut notice = Self {
            services,
            session_id: String::new(),
            card: None,
            handlers,
            cleanup: None,
            animate: true,
            _cleanup_task: None,
        };
        notice.set_card(session_id, card, cx);
        notice
    }

    /// New props. The host's announce runs on every change, as the React
    /// effect keyed on the session and card did.
    pub fn set_card(
        &mut self,
        session_id: String,
        card: Option<LinkedWorkItemUpdateCard>,
        cx: &mut Context<Self>,
    ) {
        if session_id == self.session_id && card == self.card {
            return;
        }
        self.session_id = session_id;
        self.card = card;
        if let (Some(announce), Some(card)) = (&self.handlers.on_announce, &self.card) {
            announce(&self.session_id, card, cx);
        }
        cx.notify();
    }

    /// Turns the slide-in off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    /// Whether the notice shows: a card that finished loading.
    pub fn shown(&self) -> bool {
        self.card
            .as_ref()
            .is_some_and(|card| card.status != LinkedWorkItemUpdateStatus::Loading)
    }

    /// The "Open" button: the discussion for comments, else the link.
    pub fn open_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.card.clone() else {
            return;
        };
        (self.handlers.on_acknowledge)(window, cx);
        let text = NoticeText::new(&card);
        if text.discussion {
            (self.handlers.on_open_discussion)(window, cx);
            return;
        }
        let url = card
            .entries
            .first()
            .map(|entry| entry.url.clone())
            .filter(|url| !url.is_empty())
            .unwrap_or(card.url);
        self.services.open_url(&url, cx);
    }

    /// The agent button.
    pub fn add_to_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.card.clone() else {
            return;
        };
        (self.handlers.on_acknowledge)(window, cx);
        (self.handlers.on_add_to_chat)(&card, window, cx);
    }

    /// The X button.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.handlers.on_acknowledge)(window, cx);
        (self.handlers.on_dismiss)(window, cx);
        cx.emit(NoticeDone);
    }

    /// "Archive session" (`archive`) or "Delete…".
    pub fn run_cleanup(&mut self, archive: bool, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        let handler = if archive {
            self.handlers.on_archive_session.clone()
        } else {
            self.handlers.on_delete_session.clone()
        };
        let Some(handler) = handler else {
            return;
        };
        if self.cleanup.is_some() {
            return;
        }
        self.cleanup = Some(if archive {
            CleanupAction::Archive
        } else {
            CleanupAction::Delete
        });
        cx.notify();
        let task = handler(cx);
        let acknowledge = self.handlers.on_acknowledge.clone();
        let handle = window.window_handle();
        self._cleanup_task = Some(cx.spawn(async move |this, cx| {
            let done = task.await.unwrap_or(false);
            let _ = handle.update(cx, |_, window, cx| {
                if done {
                    acknowledge(window, cx);
                }
                let _ = this.update(cx, |this, cx| {
                    this.cleanup = None;
                    cx.notify();
                });
            });
        }));
    }

    fn render_entry(
        &self,
        index: usize,
        entry: &LinkedWorkItemActivityEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let glyph = match entry.kind {
            LinkedWorkItemActivityKind::Commit => IconName::GitBranch,
            LinkedWorkItemActivityKind::Review => IconName::Check,
            _ => IconName::MessageSquare,
        };
        let now = self.services.now_ms();
        let url = entry.url.clone();
        let mut row = div()
            .id(("linked-entry", index))
            .flex()
            .w_full()
            .items_start()
            .gap(u(8.))
            .px(u(12.))
            .py(u(8.))
            .when(index > 0, |row| {
                row.border_t_1().border_color(theme.colors.stroke)
            })
            .child(
                div()
                    .mt(u(2.))
                    .child(icon(glyph).size(u(14.)).text_color(theme.content(0.45))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(6.))
                            .text_px(theme.text.caption)
                            .child(
                                div()
                                    .medium()
                                    .text_color(theme.content(0.70))
                                    .child(entry_kind_label(entry)),
                            )
                            .when(!entry.author.is_empty(), |line| {
                                line.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(theme.content(0.45))
                                        .child(format!("@{}", entry.author)),
                                )
                            })
                            .child(
                                div()
                                    .ml_auto()
                                    .flex_none()
                                    .text_color(theme.content(0.35))
                                    .child(format_relative_time(&entry.created_at, now)),
                            ),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .line_clamp(2)
                            .text_ellipsis()
                            .text_px(theme.text.label)
                            .leading(theme.leading.relaxed)
                            .text_color(theme.content(0.65))
                            .child(if entry.text.is_empty() {
                                "No message".to_string()
                            } else {
                                entry.text.clone()
                            }),
                    ),
            );
        if !url.is_empty() {
            let hover = theme.content(0.05);
            row = row.hover(move |s| s.bg(hover)).on_click(cx.listener(
                move |this, _, window, cx| {
                    (this.handlers.on_acknowledge)(window, cx);
                    this.services.open_url(&url, cx);
                },
            ));
        }
        row.into_any_element()
    }
}

impl Render for LinkedWorkItemUpdateNotice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(card) = self.card.clone().filter(|_| self.shown()) else {
            return div().into_any_element();
        };
        let theme = Theme::of(cx).clone();
        let text = NoticeText::new(&card);
        let kind_icon = if card.kind == WorkItemKind::Pr {
            IconName::GitPullRequest
        } else {
            IconName::CircleDot
        };
        let close_hover = theme.content(0.10);
        let close_ink = theme.colors.content;
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(u(2.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .py(u(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .child(
                        div()
                            .flex_none()
                            .size(u(8.))
                            .rounded_full()
                            .bg(theme.colors.accent),
                    )
                    .child(icon(kind_icon).size(u(14.)).text_color(theme.content(0.55)))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.label)
                            .semibold()
                            .child("GitHub activity"),
                    ),
            )
            .child(
                div()
                    .id("linked-notice-dismiss")
                    .group("linked-dismiss")
                    .flex()
                    .flex_none()
                    .size(u(24.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .hover(move |s| s.bg(close_hover))
                    .tooltip(tooltip("Dismiss"))
                    .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
                    .child(
                        icon(IconName::X)
                            .size(u(12.))
                            .text_color(theme.content(0.40))
                            .group_hover("linked-dismiss", move |s| s.text_color(close_ink)),
                    ),
            );
        let card_url = card.url.clone();
        let body = div()
            .px(u(12.))
            .py(u(10.))
            .child(
                div()
                    .id("linked-notice-open-card")
                    .group("linked-card")
                    .w_full()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        (this.handlers.on_acknowledge)(window, cx);
                        this.services.open_url(&card_url, cx);
                    }))
                    .child(
                        div()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.50))
                            .child(format!(
                                "{} #{} · {}",
                                text.kind_label, card.number, card.repo
                            )),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .truncate()
                            .text_px(theme.text.body)
                            .medium()
                            .group_hover("linked-card", |s| s.underline())
                            .child(card.title.clone()),
                    ),
            )
            .child(
                div()
                    .mt(u(6.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.55))
                    .child(text.summary.clone()),
            );
        let mut column = div().relative().flex().flex_col().child(header).child(body);
        if !card.entries.is_empty() {
            let mut entries = div()
                .id("linked-notice-entries")
                .max_h(u(208.))
                .overflow_y_scroll()
                .border_t_1()
                .border_color(theme.colors.stroke);
            for (index, entry) in card.entries.iter().take(3).enumerate() {
                entries = entries.child(self.render_entry(index, entry, cx));
            }
            column = column.child(entries);
        }
        if let Some(terminal) = text.terminal {
            let terminal_icon = match terminal {
                LinkedWorkItemTerminalState::PrMerged => IconName::GitMerge,
                LinkedWorkItemTerminalState::PrClosed => IconName::GitPullRequestClosed,
                LinkedWorkItemTerminalState::IssueClosed => IconName::Check,
            };
            let terminal_ink = if terminal == LinkedWorkItemTerminalState::PrMerged {
                merged_ink()
            } else {
                open_ink(&theme)
            };
            let busy = self.cleanup.is_some();
            let archive_enabled = !busy && self.handlers.on_archive_session.is_some();
            let delete_enabled = !busy && self.handlers.on_delete_session.is_some();
            let archive_hover = theme.content(0.15);
            let mut archive = div()
                .id("linked-notice-archive")
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .overflow_hidden()
                .rounded(u(theme.radius.md))
                .bg(theme.content(0.10))
                .px(u(8.))
                .py(u(4.))
                .text_px(theme.text.caption)
                .medium()
                .tooltip(tooltip("Archive session"))
                .child(if self.cleanup == Some(CleanupAction::Archive) {
                    loader("linked-archive-busy", 12., theme.colors.content)
                } else {
                    icon(IconName::Archive)
                        .size(u(12.))
                        .text_color(theme.colors.content)
                        .into_any_element()
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .whitespace_nowrap()
                        .child("Archive session"),
                );
            if archive_enabled {
                archive = archive.hover(move |s| s.bg(archive_hover)).on_click(
                    cx.listener(|this, _, window, cx| this.run_cleanup(true, window, cx)),
                );
            } else {
                archive = archive.opacity(0.4);
            }
            let red = with_alpha(theme.colors.danger_soft, 0.9);
            let red_hover = with_alpha(theme.colors.danger_fill, 0.15);
            let mut delete = div()
                .id("linked-notice-delete")
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .overflow_hidden()
                .rounded(u(theme.radius.md))
                .px(u(8.))
                .py(u(4.))
                .text_px(theme.text.caption)
                .text_color(red)
                .tooltip(tooltip("Delete session"))
                .child(if self.cleanup == Some(CleanupAction::Delete) {
                    loader("linked-delete-busy", 12., red)
                } else {
                    icon(IconName::Trash2)
                        .size(u(12.))
                        .text_color(red)
                        .into_any_element()
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .whitespace_nowrap()
                        .child("Delete…"),
                );
            if delete_enabled {
                delete = delete.hover(move |s| s.bg(red_hover)).on_click(
                    cx.listener(|this, _, window, cx| this.run_cleanup(false, window, cx)),
                );
            } else {
                delete = delete.opacity(0.4);
            }
            column = column.child(
                div()
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(8.))
                            .text_px(theme.text.caption)
                            .child(icon(terminal_icon).size(u(14.)).text_color(terminal_ink))
                            .child(
                                div()
                                    .medium()
                                    .text_color(theme.content(0.75))
                                    .child(text.terminal_label),
                            )
                            .child(
                                div()
                                    .text_color(theme.content(0.45))
                                    .child("Clean up this session"),
                            ),
                    )
                    .child(
                        div()
                            .mt(u(8.))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .child(archive)
                            .child(delete),
                    ),
            );
        }
        let c = theme.colors;
        let primary_hover = theme.content(0.90);
        let secondary_hover = theme.content(0.15);
        column = column.child(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .border_t_1()
                .border_color(theme.colors.stroke)
                .px(u(12.))
                .py(u(10.))
                .text_px(theme.text.caption)
                .child(
                    div()
                        .id("linked-notice-agent")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .whitespace_nowrap()
                        .rounded(u(theme.radius.md))
                        .bg(c.content)
                        .px(u(8.))
                        .py(u(4.))
                        .medium()
                        .text_center()
                        .text_color(c.background_base)
                        .hover(move |s| s.bg(primary_hover))
                        .tooltip(tooltip(text.agent_label))
                        .on_click(cx.listener(|this, _, window, cx| this.add_to_chat(window, cx)))
                        .child(text.agent_label),
                )
                .child(
                    div()
                        .id("linked-notice-open")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .whitespace_nowrap()
                        .rounded(u(theme.radius.md))
                        .bg(theme.content(0.10))
                        .px(u(8.))
                        .py(u(4.))
                        .medium()
                        .text_center()
                        .hover(move |s| s.bg(secondary_hover))
                        .tooltip(tooltip(text.open_label))
                        .on_click(cx.listener(|this, _, window, cx| this.open_activity(window, cx)))
                        .child(text.open_label),
                ),
        );
        let animated: AnyElement = if self.animate {
            let key: SharedString =
                format!("linked-notice-{}-{}", self.session_id, card.updated_at).into();
            let rise = theme.motion.popover_lift;
            column
                .with_animation(
                    ElementId::Name(key),
                    Animation::new(Duration::from_millis(180))
                        .with_easing(theme.motion.ease_out.easing()),
                    move |el, t| el.top(u(-rise * (1.0 - t))),
                )
                .into_any_element()
        } else {
            column.into_any_element()
        };
        div()
            .id("linked-notice")
            .occlude()
            .absolute()
            .top(u(12.))
            .right(u(12.))
            .w(u(320.))
            .max_w_full()
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.colors.popover_border)
            .shadow_xl()
            .text_color(theme.colors.content)
            .child(glass_backdrop(
                theme.radius.xl,
                24.,
                theme.colors.popover_backdrop,
            ))
            .child(animated)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::LinkedWorkItemActivityCounts;

    fn card() -> LinkedWorkItemUpdateCard {
        LinkedWorkItemUpdateCard {
            kind: WorkItemKind::Pr,
            repo: "acme/app".into(),
            number: 42,
            title: "Update sidebar activity".into(),
            url: "https://github.com/acme/app/pull/42".into(),
            state: "open".into(),
            since: 0,
            updated_at: 1,
            status: LinkedWorkItemUpdateStatus::Ready,
            counts: LinkedWorkItemActivityCounts {
                comments: 1,
                reviews: 0,
                commits: 0,
            },
            entries: vec![LinkedWorkItemActivityEntry {
                id: "comment-1".into(),
                kind: LinkedWorkItemActivityKind::Comment,
                author: "maya".into(),
                text: "Please cover the empty state".into(),
                created_at: "2026-09-13T11:00:00Z".into(),
                url: "https://github.com/acme/app/pull/42#issuecomment-1".into(),
            }],
            truncated: false,
        }
    }

    #[test]
    fn offers_discussion_and_agent_actions_for_a_new_comment() {
        let text = NoticeText::new(&card());
        assert_eq!(text.summary, "1 new comment");
        assert_eq!(text.open_label, "Open PR");
        assert_eq!(text.agent_label, "Address with agent");
        assert!(text.discussion);
    }

    #[test]
    fn opens_the_exact_commit_for_commit_activity() {
        let mut card = card();
        card.counts = LinkedWorkItemActivityCounts {
            comments: 0,
            reviews: 0,
            commits: 1,
        };
        card.entries = vec![LinkedWorkItemActivityEntry {
            id: "abcdef123456".into(),
            kind: LinkedWorkItemActivityKind::Commit,
            author: "nik".into(),
            text: "Handle linked activity".into(),
            created_at: "2026-09-13T11:30:00Z".into(),
            url: "https://github.com/acme/app/commit/abcdef123456".into(),
        }];
        let text = NoticeText::new(&card);
        assert_eq!(text.open_label, "Open commit");
        assert_eq!(text.agent_label, "Review with agent");
        assert!(!text.discussion);
        assert_eq!(entry_kind_label(&card.entries[0]), "Commit abcdef1");
    }

    #[test]
    fn offers_session_cleanup_when_the_item_ends() {
        let mut merged = card();
        merged.state = "merged".into();
        assert_eq!(
            NoticeText::new(&merged).terminal_label,
            "Pull request merged"
        );
        let mut closed = card();
        closed.kind = WorkItemKind::Issue;
        closed.state = "closed".into();
        assert_eq!(NoticeText::new(&closed).terminal_label, "Issue closed");
    }
}
