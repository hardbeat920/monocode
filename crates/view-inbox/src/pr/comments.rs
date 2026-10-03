//! Port of src/features/inbox/ui/InboxComments.tsx: the comment thread
//! under an item's description, its replies, and the comment form.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Textarea, TextareaState};
use monocode_markdown::{BlockMargins, MarkdownView};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    InboxProvider, InboxReplyTarget, InboxServices, Loadable, WorkItemComment, WorkItemThread,
};
use crate::model::{
    format_relative_time, github_review_state_label, inbox_person_avatar_url, open_on_label,
    provider_name,
};
use crate::style::{avatar, closed_ink, loader, markdown_style, open_ink};

/// Markdown views for an item's body and comments, kept across renders so
/// selection and highlighting survive. Links open through the services.
pub struct MarkdownCache {
    services: Rc<dyn InboxServices>,
    views: HashMap<String, (String, Entity<MarkdownView>)>,
    used: HashSet<String>,
}

impl MarkdownCache {
    pub fn new(services: Rc<dyn InboxServices>) -> Self {
        Self {
            services,
            views: HashMap::new(),
            used: HashSet::new(),
        }
    }

    /// The view for `key` showing `text`. `comment` uses the tighter
    /// `.inbox-comment-md` spacing.
    pub fn view(
        &mut self,
        key: &str,
        text: &str,
        comment: bool,
        cx: &mut App,
    ) -> Entity<MarkdownView> {
        self.used.insert(key.to_string());
        if let Some((current, view)) = self.views.get_mut(key) {
            if current != text {
                *current = text.to_string();
                let text = text.to_string();
                view.update(cx, |view, cx| view.set_text(&text, cx));
            }
            return view.clone();
        }
        let mut style = markdown_style(Theme::of(cx));
        if comment {
            style.paragraph_margins = BlockMargins::new(8., 0.);
        }
        let services = self.services.clone();
        let view = cx.new(|cx| {
            let mut view = MarkdownView::with_text(text.to_string(), cx);
            view.set_style(style, cx);
            view.on_link_click(move |link, _, cx| services.open_url(&link.url, cx));
            view
        });
        self.views
            .insert(key.to_string(), (text.to_string(), view.clone()));
        view
    }

    /// Drops the views no render asked for since the last sweep.
    pub fn sweep(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.views.retain(|key, _| used.contains(key));
    }

    /// Restyles every view after a theme change.
    pub fn restyle(&mut self, cx: &mut App) {
        for (key, (_, view)) in &self.views {
            let mut style = markdown_style(Theme::of(cx));
            if key.starts_with("comment:") {
                style.paragraph_margins = BlockMargins::new(8., 0.);
            }
            view.update(cx, |view, cx| view.set_style(style, cx));
        }
    }
}

/// How replies attach: GitHub review threads, Linear parent comments, or
/// none (Jira, GitLab, ADO).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyMode {
    Thread,
    Parent,
}

type ReplyFn = Rc<dyn Fn(InboxReplyTarget, &mut Window, &mut App)>;

/// What `InboxComments` needs besides the thread.
pub struct CommentsProps {
    pub provider: InboxProvider,
    pub reply_mode: Option<ReplyMode>,
    pub on_reply: Option<ReplyFn>,
    pub now: i64,
}

fn comments_pending(cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    div()
        .flex()
        .items_center()
        .gap(u(8.))
        .border_t_1()
        .border_color(theme.colors.stroke)
        .pt(u(20.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.45))
        .child(loader("comments-pending", 14., theme.content(0.45)))
        .child("Loading comments")
        .into_any_element()
}

/// The "N comments" label.
pub fn comment_count_label(thread: &WorkItemThread) -> String {
    let count: usize = thread
        .comments
        .iter()
        .map(|comment| 1 + comment.replies.len())
        .sum();
    if count == 1 {
        "1 comment".into()
    } else {
        format!("{count} comments")
    }
}

/// `InboxComments`. `None` when there is nothing to show.
pub fn inbox_comments(
    thread: &Loadable<WorkItemThread>,
    props: &CommentsProps,
    markdown: &mut MarkdownCache,
    cx: &mut App,
) -> Option<AnyElement> {
    let Some(value) = thread.value.as_ref() else {
        if let Some(error) = thread.error.clone() {
            let theme = Theme::of(cx);
            return Some(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child(error)
                    .into_any_element(),
            );
        }
        return thread.loading.then(|| comments_pending(cx));
    };
    if value.comments.is_empty() && !value.truncated {
        return thread.loading.then(|| comments_pending(cx));
    }
    let theme = Theme::of(cx).clone();
    let mut header = div()
        .flex()
        .items_center()
        .gap(u(8.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50))
        .child(
            div()
                .text_color(theme.content(0.70))
                .child(comment_count_label(value)),
        );
    if value.truncated {
        header = header.child(format!(
            "Latest comments · more on {}",
            provider_name(props.provider)
        ));
    }
    if thread.loading {
        header = header.child(loader("comments-refreshing", 12., theme.content(0.35)));
    }
    let mut section = div()
        .flex()
        .flex_col()
        .gap(u(12.))
        .border_t_1()
        .border_color(theme.colors.stroke)
        .pt(u(20.))
        .child(header);
    if let Some(error) = thread.error.clone() {
        section = section.child(
            div()
                .text_px(theme.text.label)
                .text_color(theme.content(0.45))
                .child(error),
        );
    }
    let mut list = div().flex().flex_col().gap(u(8.));
    for comment in &value.comments {
        list = list.child(inbox_comment(comment, props, false, markdown, cx));
    }
    Some(section.child(list).into_any_element())
}

fn comment_location(comment: &WorkItemComment) -> String {
    let path = comment.path.trim();
    if path.is_empty() {
        return String::new();
    }
    match comment.line {
        Some(line) if line > 0 => format!("{path}:{line}"),
        _ => path.to_string(),
    }
}

fn inbox_comment(
    comment: &WorkItemComment,
    props: &CommentsProps,
    nested: bool,
    markdown: &mut MarkdownCache,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::of(cx).clone();
    let time = format_relative_time(&comment.created_at, props.now);
    let review = github_review_state_label(&comment.state);
    let location = comment_location(comment);
    let meta: Vec<String> = [
        review.to_string(),
        location,
        if comment.resolved {
            "Resolved".into()
        } else {
            String::new()
        },
        time.clone(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();
    let has_body = !comment.body.trim().is_empty();
    let has_replies = !nested && !comment.replies.is_empty();
    let can_reply = props.on_reply.is_some()
        && match props.reply_mode {
            Some(ReplyMode::Parent) => true,
            Some(ReplyMode::Thread) => !comment.thread_id.trim().is_empty(),
            None => false,
        };
    let author = if comment.author.is_empty() {
        "ghost".to_string()
    } else {
        comment.author.clone()
    };
    let avatar_url = inbox_person_avatar_url(
        props.provider,
        &comment.author,
        comment.author_avatar_url.as_deref(),
    );
    let mut header = div()
        .flex()
        .min_w_0()
        .flex_wrap()
        .items_center()
        .gap_x(u(8.))
        .gap_y(u(4.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50))
        .child(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .child(avatar(&author, &avatar_url, 20., cx))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .medium()
                        .text_color(theme.colors.content)
                        .child(author.clone()),
                ),
        );
    if !nested {
        header = header.px(u(12.)).py(u(8.));
        if has_body || has_replies {
            header = header.border_b_1().border_color(theme.colors.stroke);
        }
    }
    let state = comment.state.as_str();
    for (index, part) in meta.iter().enumerate() {
        let item = div().flex().min_w_0().items_center().gap(u(8.)).child("·");
        let content: AnyElement = if !comment.url.is_empty() && *part == time {
            let url = comment.url.clone();
            let services = markdown.services.clone();
            let ink = theme.colors.content;
            div()
                .id(ElementId::Name(
                    format!("comment-time:{}:{index}", comment.id).into(),
                ))
                .hover(move |s| s.text_color(ink))
                .tooltip(tooltip(open_on_label(props.provider)))
                .on_click(move |_, _, cx| services.open_url(&url, cx))
                .child(part.clone())
                .into_any_element()
        } else {
            let span = div().child(part.clone());
            let span = if state == "APPROVED" {
                span.text_color(open_ink(&theme))
            } else if state == "CHANGES_REQUESTED" {
                span.text_color(closed_ink())
            } else if comment.resolved && part == "Resolved" {
                span.text_color(monocode_ui::color::with_alpha(theme.colors.success, 0.8))
            } else {
                span.min_w_0().truncate()
            };
            span.into_any_element()
        };
        header = header.child(item.child(content));
    }
    if can_reply && let Some(on_reply) = props.on_reply.clone() {
        let target = InboxReplyTarget {
            id: comment.id.clone(),
            author: author.clone(),
            thread_id: comment.thread_id.trim().to_string(),
        };
        let ink = theme.colors.content;
        header = header.child(
            div().flex().items_center().gap(u(8.)).child("·").child(
                div()
                    .id(ElementId::Name(
                        format!("comment-reply:{}", comment.id).into(),
                    ))
                    .hover(move |s| s.text_color(ink))
                    .on_click(move |_, window, cx| on_reply(target.clone(), window, cx))
                    .child("Reply"),
            ),
        );
    }
    let mut article = div().flex().flex_col().child(header);
    if has_body {
        let view = markdown.view(&format!("comment:{}", comment.id), &comment.body, true, cx);
        let body = div().child(view);
        article = article.child(if nested {
            body.mt(u(8.))
        } else {
            body.px(u(12.)).py(u(10.))
        });
    }
    if has_replies {
        let mut replies = div()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .px(u(12.));
        for (index, reply) in comment.replies.iter().enumerate() {
            replies = replies.child(
                div()
                    .py(u(10.))
                    .when(index > 0, |row| {
                        row.border_t_1().border_color(theme.colors.stroke)
                    })
                    .child(inbox_comment(reply, props, true, markdown, cx)),
            );
        }
        article = article.child(replies);
    }
    if nested {
        return article.into_any_element();
    }
    article
        .overflow_hidden()
        .rounded(u(theme.radius.md))
        .border_1()
        .border_color(theme.content(0.10))
        .bg(theme.content(0.05))
        .into_any_element()
}

/// The platform's modifier glyph in the comment placeholder (`MOD`).
pub fn mod_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

/// The comment field's placeholder.
pub fn comment_placeholder(replying: bool) -> String {
    if replying {
        format!("Write a reply ({}↩)", mod_key())
    } else {
        format!("Leave a comment ({}↩)", mod_key())
    }
}

/// The comment button's label.
pub fn comment_button_label(posting: bool, replying: bool) -> &'static str {
    if posting {
        "Posting..."
    } else if replying {
        "Reply"
    } else {
        "Comment"
    }
}

type FormFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// `InboxCommentForm`. The detail view owns the field and the draft.
pub struct CommentForm<'a> {
    pub field: &'a Entity<TextareaState>,
    pub draft: &'a str,
    pub reply_to: Option<&'a InboxReplyTarget>,
    pub posting: bool,
    pub error: Option<&'a str>,
    pub on_cancel_reply: FormFn,
    pub on_submit: FormFn,
}

pub fn comment_form(form: CommentForm<'_>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let can_post = !form.draft.trim().is_empty() && !form.posting;
    let mut root = div()
        .flex()
        .flex_col()
        .gap(u(8.))
        .border_t_1()
        .border_color(theme.colors.stroke)
        .pt(u(20.));
    if let Some(reply_to) = form.reply_to {
        let author = if reply_to.author.is_empty() {
            "comment".to_string()
        } else {
            reply_to.author.clone()
        };
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let cancel = form.on_cancel_reply.clone();
        root = root.child(
            div()
                .flex()
                .items_center()
                .gap(u(8.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(format!("Replying to {author}")),
                )
                .child(
                    div()
                        .id("comment-cancel-reply")
                        .group("comment-cancel")
                        .flex()
                        .flex_none()
                        .size(u(20.))
                        .items_center()
                        .justify_center()
                        .rounded(u(theme.radius.md))
                        .hover(move |s| s.bg(hover))
                        .tooltip(tooltip("Cancel reply"))
                        .on_click(move |_, window, cx| cancel(window, cx))
                        .child(
                            icon(IconName::X)
                                .size(u(12.))
                                .text_color(theme.content(0.45))
                                .group_hover("comment-cancel", move |s| s.text_color(ink)),
                        ),
                ),
        );
    }
    let label: SharedString = comment_button_label(form.posting, form.reply_to.is_some()).into();
    let c = theme.colors;
    let mut button = div()
        .id("comment-submit")
        .flex()
        .h(u(28.))
        .items_center()
        .rounded(u(theme.radius.md))
        .bg(c.content)
        .px(u(12.))
        .text_px(theme.text.label)
        .text_color(c.background_base)
        .child(label);
    if can_post {
        let submit = form.on_submit.clone();
        let hover = theme.content(0.80);
        button = button
            .hover(move |s| s.bg(hover))
            .on_click(move |_, window, cx| submit(window, cx));
    } else {
        button = button.opacity(0.4);
    }
    root = root.child(
        div()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.05))
            .child(
                div()
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(theme.text.body)
                    .line_height(u(20.))
                    .text_color(c.content)
                    .when(form.posting, |field| field.opacity(0.4))
                    .child(
                        Textarea::new(form.field)
                            .appearance(false)
                            .bordered(false)
                            .disabled(form.posting)
                            .p_0()
                            .text_px(theme.text.body),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px(u(8.))
                    .pb(u(8.))
                    .child(button),
            ),
    );
    if let Some(error) = form.error {
        root = root.child(
            div()
                .text_px(theme.text.label)
                .text_color(monocode_ui::color::with_alpha(theme.colors.danger, 0.9))
                .child(error.to_string()),
        );
    }
    root.into_any_element()
}
