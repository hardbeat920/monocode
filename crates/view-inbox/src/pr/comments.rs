//! Port of src/features/inbox/ui/InboxComments.tsx: the comment thread
//! under an item's description, its replies, and the comment form.
//!
//! GitHub threads read as an activity rail: comments, review events, and
//! pushes in time order, each on a stop with an avatar, an icon, or a dot.
//! Other providers keep the plain list of comment cards.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AbsoluteLength, AnyElement, App, AppContext as _, Div, EdgesRefinement, ElementId, Entity,
    EntityId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, linear_color_stop,
    linear_gradient, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Textarea, TextareaState};
use monocode_markdown::{BlockMargins, MarkdownView};
use monocode_ui::color::{mix, with_alpha};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    InboxProvider, InboxReplyTarget, InboxServices, Loadable, WorkItemComment, WorkItemCommit,
    WorkItemThread,
};
use crate::model::{
    date_parse, format_relative_time, github_review_state_label, inbox_person_avatar_url,
    open_on_label, provider_name,
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

/// `CLAMPED_BODY_PX`: how tall a long timeline comment starts.
pub const CLAMPED_BODY_PX: f32 = 180.;

/// A body this much past the clamp just shows in full.
const CLAMP_SLACK_PX: f32 = 48.;

#[derive(Default)]
struct ClampState {
    /// Each body's natural height in CSS px, from its last paint.
    heights: HashMap<String, f32>,
    expanded: HashSet<String>,
}

/// What `CollapsibleBody` keeps across renders: each comment body's measured
/// height and the bodies the reader expanded. Bodies measure themselves when
/// they paint, and a body that crosses the limit redraws its owner view.
#[derive(Clone)]
pub struct BodyClamps {
    owner: EntityId,
    state: Rc<RefCell<ClampState>>,
}

impl BodyClamps {
    /// Clamps that redraw the view `owner` when they change.
    pub fn new(owner: EntityId) -> Self {
        Self {
            owner,
            state: Rc::default(),
        }
    }

    /// Whether the body at `key` is long enough to start clamped.
    pub fn overflows(&self, key: &str) -> bool {
        self.state
            .borrow()
            .heights
            .get(key)
            .is_some_and(|height| *height > CLAMPED_BODY_PX + CLAMP_SLACK_PX)
    }

    pub fn expanded(&self, key: &str) -> bool {
        self.state.borrow().expanded.contains(key)
    }

    /// "Show more" or "Show less".
    pub fn toggle(&self, key: &str, cx: &mut App) {
        {
            let mut state = self.state.borrow_mut();
            if !state.expanded.remove(key) {
                state.expanded.insert(key.to_string());
            }
        }
        cx.notify(self.owner);
    }

    /// Another item: forget every height and expansion.
    pub fn clear(&self) {
        *self.state.borrow_mut() = ClampState::default();
    }

    fn measure(&self, key: &str, height: f32, window: &mut Window, cx: &mut App) {
        let before = self.overflows(key);
        self.state
            .borrow_mut()
            .heights
            .insert(key.to_string(), height);
        if self.overflows(key) != before {
            let owner = self.owner;
            window.defer(cx, move |_, cx| cx.notify(owner));
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
    /// The long-body clamps of the timeline.
    pub clamps: BodyClamps,
}

impl CommentsProps {
    /// The TypeScript turned the timeline on when `thread.commits` was
    /// present, and only the GitHub thread command returns that field. The
    /// native thread always has a `commits` list, so the provider decides.
    pub fn timeline(&self) -> bool {
        self.provider == InboxProvider::Github
    }
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

/// `isReviewEvent`: a review without a note is an event, not conversation.
pub fn is_review_event(comment: &WorkItemComment) -> bool {
    comment.kind == "review" && comment.body.trim().is_empty() && comment.replies.is_empty()
}

/// The "N comments" label. The timeline leaves review events out of the
/// count, since they show as rail events.
pub fn comment_count_label(thread: &WorkItemThread, timeline: bool) -> String {
    let count: usize = thread
        .comments
        .iter()
        .map(|comment| {
            if timeline && is_review_event(comment) {
                0
            } else {
                1 + comment.replies.len()
            }
        })
        .sum();
    if count == 1 {
        "1 comment".into()
    } else {
        format!("{count} comments")
    }
}

/// The timeline heading's detail: "N comments · M commits", without the
/// commits when there are none.
pub fn activity_count_label(thread: &WorkItemThread) -> String {
    let comments = comment_count_label(thread, true);
    match thread.commits.len() {
        0 => comments,
        1 => format!("{comments} · 1 commit"),
        count => format!("{comments} · {count} commits"),
    }
}

/// One comment or commit in the activity stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityEntry<'a> {
    Comment(&'a WorkItemComment),
    Commit(&'a WorkItemCommit),
}

/// `activityTimeline`: comments and commits as one stream, oldest first.
/// The sort is stable, so ties keep comments before commits.
pub fn activity_timeline(thread: &WorkItemThread) -> Vec<ActivityEntry<'_>> {
    let at = |iso: &str| date_parse(iso).unwrap_or(0);
    let mut entries: Vec<(i64, ActivityEntry<'_>)> = thread
        .comments
        .iter()
        .map(|comment| (at(&comment.created_at), ActivityEntry::Comment(comment)))
        .chain(
            thread
                .commits
                .iter()
                .map(|commit| (at(&commit.committed_date), ActivityEntry::Commit(commit))),
        )
        .collect();
    entries.sort_by_key(|(at, _)| *at);
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// One stop group on the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineItem<'a> {
    Comment(&'a WorkItemComment),
    Commits {
        author: &'a str,
        commits: Vec<&'a WorkItemCommit>,
    },
}

/// `timelineItems`: back-to-back commits by one author read as a single
/// push on the rail.
pub fn timeline_items<'a>(entries: &[ActivityEntry<'a>]) -> Vec<TimelineItem<'a>> {
    let mut items: Vec<TimelineItem<'a>> = Vec::new();
    for entry in entries.iter().copied() {
        match entry {
            ActivityEntry::Comment(comment) => items.push(TimelineItem::Comment(comment)),
            ActivityEntry::Commit(commit) => {
                if let Some(TimelineItem::Commits { author, commits }) = items.last_mut()
                    && *author == commit.author
                {
                    commits.push(commit);
                } else {
                    items.push(TimelineItem::Commits {
                        author: &commit.author,
                        commits: vec![commit],
                    });
                }
            }
        }
    }
    items
}

/// What sits on a rail stop's node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailNode {
    Avatar { name: String, url: String },
    Approved,
    ChangesRequested,
    Reviewed,
}

/// One row of the activity rail, in drawing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailRow<'a> {
    /// A comment card beside its author's avatar.
    Comment {
        comment: &'a WorkItemComment,
        first: bool,
        last: bool,
    },
    /// `TimelineEventLine`: "name action · time" beside an avatar or a
    /// review icon. Pushes and bare reviews.
    Event {
        node: RailNode,
        name: String,
        action: String,
        time: String,
        first: bool,
        last: bool,
    },
    /// `InboxCommitStop`: one commit of a push, on a dot.
    Commit {
        commit: &'a WorkItemCommit,
        last: bool,
    },
}

/// The rows the rail draws for `thread`: comments, review events, and each
/// push as its event line followed by its commits.
pub fn rail_rows(thread: &WorkItemThread, provider: InboxProvider, now: i64) -> Vec<RailRow<'_>> {
    let entries = activity_timeline(thread);
    let items = timeline_items(&entries);
    let count = items.len();
    let mut rows = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let first = index == 0;
        let last = index + 1 == count;
        match item {
            TimelineItem::Comment(comment) if is_review_event(comment) => {
                let state = comment.state.trim().to_uppercase();
                let (node, action) = match state.as_str() {
                    "APPROVED" => (RailNode::Approved, "approved"),
                    "CHANGES_REQUESTED" => (RailNode::ChangesRequested, "requested changes"),
                    "DISMISSED" => (RailNode::Reviewed, "had a review dismissed"),
                    _ => (RailNode::Reviewed, "reviewed"),
                };
                rows.push(RailRow::Event {
                    node,
                    name: author_name(&comment.author),
                    action: action.into(),
                    time: format_relative_time(&comment.created_at, now),
                    first,
                    last,
                });
            }
            TimelineItem::Comment(comment) => rows.push(RailRow::Comment {
                comment,
                first,
                last,
            }),
            TimelineItem::Commits { author, commits } => {
                let name = author_name(author);
                let newest = commits.last().map(|commit| commit.committed_date.as_str());
                rows.push(RailRow::Event {
                    node: RailNode::Avatar {
                        name: name.clone(),
                        url: inbox_person_avatar_url(provider, author, None),
                    },
                    name,
                    action: if commits.len() == 1 {
                        "added a commit".into()
                    } else {
                        format!("added {} commits", commits.len())
                    },
                    time: format_relative_time(newest.unwrap_or(""), now),
                    first,
                    last: false,
                });
                let pushed = commits.len();
                rows.extend(commits.into_iter().enumerate().map(|(index, commit)| {
                    RailRow::Commit {
                        commit,
                        last: last && index + 1 == pushed,
                    }
                }));
            }
        }
    }
    rows
}

fn author_name(author: &str) -> String {
    if author.is_empty() {
        "ghost".into()
    } else {
        author.to_string()
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
    let timeline = props.timeline();
    let commit_count = if timeline { value.commits.len() } else { 0 };
    if value.comments.is_empty() && commit_count == 0 && !value.truncated {
        return thread.loading.then(|| comments_pending(cx));
    }
    let theme = Theme::of(cx).clone();
    let mut header = div()
        .flex()
        .items_center()
        .gap(u(8.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50));
    header = if timeline {
        header
            .child(div().text_color(theme.content(0.70)).child("Activity"))
            .child(activity_count_label(value))
    } else {
        header.child(
            div()
                .text_color(theme.content(0.70))
                .child(comment_count_label(value, false)),
        )
    };
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
    if timeline {
        let mut rail = div().flex().flex_col();
        for row in rail_rows(value, props.provider, props.now) {
            rail = rail.child(rail_row(row, props, markdown, cx));
        }
        return Some(section.child(rail).into_any_element());
    }
    let mut list = div().flex().flex_col().gap(u(8.));
    for comment in &value.comments {
        list = list.child(inbox_comment(comment, props, false, false, markdown, cx));
    }
    Some(section.child(list).into_any_element())
}

/// The space between any two rail stops: headings, commits, reviews, and
/// comments.
const TIMELINE_GAP: f32 = 12.;

fn rail_line(theme: &Theme) -> Div {
    div().w(px(1.)).flex_none().bg(theme.content(0.10))
}

/// `TimelineRail`: the rail column every stop shares. The lead segment sets
/// where the node sits, so avatars, review icons, and commit dots all land
/// on their row's center.
fn timeline_rail(first: bool, last: bool, lead: f32, node: AnyElement, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(u(20.))
        .items_center()
        .child(
            div()
                .w(px(1.))
                .flex_none()
                .h(u(lead))
                .when(!first, |line| line.bg(theme.content(0.10))),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .size(u(20.))
                .items_center()
                .justify_center()
                .child(node),
        )
        .when(!last, |rail| rail.child(rail_line(theme).flex_1()))
}

/// `TimelineStop`: a node on the rail and the row beside it. `card` centers
/// the node on a comment card's header instead of a 20px row.
fn timeline_stop(
    node: AnyElement,
    first: bool,
    last: bool,
    card: bool,
    content: AnyElement,
    theme: &Theme,
) -> AnyElement {
    div()
        .flex()
        .gap(u(12.))
        .child(timeline_rail(
            first,
            last,
            if card { 8. } else { 0. },
            node,
            theme,
        ))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .when(!last, |row| row.pb(u(TIMELINE_GAP)))
                .child(content),
        )
        .into_any_element()
}

/// `TimelineEventLine`: "name action · time" on the 20px row every rail
/// row shares.
fn timeline_event_line(name: String, action: String, time: String, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .h(u(20.))
        .min_w_0()
        .items_center()
        .gap(u(6.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50))
        .child(
            div()
                .min_w_0()
                .truncate()
                .medium()
                .text_color(theme.colors.content)
                .child(name),
        )
        .child(div().flex_none().child(action))
        .when(!time.is_empty(), |line| {
            line.child(div().flex_none().child("·"))
                .child(div().flex_none().child(time))
        })
        .into_any_element()
}

fn rail_node(node: RailNode, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let review =
        |name: IconName, ink: Hsla| icon(name).size(u(16.)).text_color(ink).into_any_element();
    match node {
        RailNode::Avatar { name, url } => avatar(&name, &url, 20., cx),
        RailNode::Approved => review(IconName::CheckCircle, open_ink(theme)),
        RailNode::ChangesRequested => review(IconName::CircleX, closed_ink()),
        RailNode::Reviewed => review(IconName::MessageSquare, theme.content(0.45)),
    }
}

fn rail_row(
    row: RailRow<'_>,
    props: &CommentsProps,
    markdown: &mut MarkdownCache,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::of(cx).clone();
    match row {
        RailRow::Event {
            node,
            name,
            action,
            time,
            first,
            last,
        } => {
            let node = rail_node(node, cx);
            timeline_stop(
                node,
                first,
                last,
                false,
                timeline_event_line(name, action, time, &theme),
                &theme,
            )
        }
        RailRow::Comment {
            comment,
            first,
            last,
        } => {
            let url = inbox_person_avatar_url(
                props.provider,
                &comment.author,
                comment.author_avatar_url.as_deref(),
            );
            let node = avatar(&author_name(&comment.author), &url, 20., cx);
            let card = inbox_comment(comment, props, false, true, markdown, cx);
            timeline_stop(node, first, last, true, card, &theme)
        }
        RailRow::Commit { commit, last } => commit_stop(commit, last, markdown, &theme),
    }
}

/// `InboxCommitStop`: the rail runs through commit dots, so a push reads as
/// one stretch.
fn commit_stop(
    commit: &WorkItemCommit,
    last: bool,
    markdown: &MarkdownCache,
    theme: &Theme,
) -> AnyElement {
    let ring = u(1.5).into();
    let mut dot = div()
        .flex_none()
        .size(u(8.))
        .rounded_full()
        .border_color(theme.content(0.35));
    dot.style().border_widths = EdgesRefinement::<AbsoluteLength> {
        top: Some(ring),
        right: Some(ring),
        bottom: Some(ring),
        left: Some(ring),
    };
    let rail = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(u(20.))
        .items_center()
        .child(rail_line(theme).h(u(6.)))
        .child(dot)
        .when(!last, |rail| rail.child(rail_line(theme).flex_1()));
    let group: SharedString = format!("commit:{}", commit.oid).into();
    let short: String = commit.oid.chars().take(7).collect();
    let headline = theme.content(0.70);
    let ink = theme.colors.content;
    let oid_hover = theme.content(0.60);
    let linked = !commit.url.is_empty();
    let mut button = div()
        .id(ElementId::Name(group.clone()))
        .group(group.clone())
        .flex()
        .h(u(20.))
        .w_full()
        .min_w_0()
        .items_center()
        .gap(u(12.))
        .text_px(theme.text.label)
        .tooltip(tooltip(if linked {
            "Open commit".to_string()
        } else {
            commit.message_headline.clone()
        }))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_color(headline)
                .when(linked, |text| {
                    text.group_hover(group.clone(), move |s| s.text_color(ink))
                })
                .child(commit.message_headline.clone()),
        )
        .child(
            div()
                .flex_none()
                .font_family(theme.fonts.mono.clone())
                .text_px(theme.text.caption)
                .text_color(theme.content(0.35))
                .when(linked, |text| {
                    text.group_hover(group.clone(), move |s| s.text_color(oid_hover))
                })
                .child(short),
        );
    if linked {
        let url = commit.url.clone();
        let services = markdown.services.clone();
        button = button.on_click(move |_, _, cx| services.open_url(&url, cx));
    }
    div()
        .flex()
        .gap(u(12.))
        .child(rail)
        .child(
            div()
                .min_w_0()
                .flex_1()
                .when(!last, |row| row.pb(u(TIMELINE_GAP)))
                .child(button),
        )
        .into_any_element()
}

/// `CollapsibleBody`: bot reviews and long write-ups start clamped at
/// `CLAMPED_BODY_PX` so the timeline stays scannable. CSS faded the cut with
/// a mask; GPUI has no masks, so a gradient to the card's fill covers the
/// last 40% instead.
fn collapsible_body(
    key: &str,
    content: AnyElement,
    clamps: &BodyClamps,
    theme: &Theme,
) -> AnyElement {
    let overflows = clamps.overflows(key);
    let expanded = clamps.expanded(key);
    let clamped = overflows && !expanded;
    let measure = {
        let clamps = clamps.clone();
        let key = key.to_string();
        canvas(
            move |bounds, window, cx| {
                let height = bounds.size.height / window.rem_size() * 16.;
                clamps.measure(&key, height, window, cx);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };
    let mut frame = div()
        .relative()
        .child(div().relative().flex_none().child(content).child(measure));
    if clamped {
        let fill = mix(theme.colors.content, theme.colors.background_base, 0.05);
        frame = frame.max_h(u(CLAMPED_BODY_PX)).overflow_hidden().child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(u(CLAMPED_BODY_PX * 0.4))
                .bg(linear_gradient(
                    0.,
                    linear_color_stop(fill, 0.),
                    linear_color_stop(with_alpha(fill, 0.), 1.),
                )),
        );
    }
    let mut body = div().flex().flex_col().child(frame);
    if overflows {
        let clamps = clamps.clone();
        let toggle_key = key.to_string();
        let hover = theme.colors.content;
        body = body.child(
            div()
                .id(ElementId::Name(format!("show-more:{key}").into()))
                .mt(u(6.))
                .self_start()
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .hover(move |s| s.text_color(hover))
                .on_click(move |_, _, cx| clamps.toggle(&toggle_key, cx))
                .child(if expanded { "Show less" } else { "Show more" }),
        );
    }
    body.into_any_element()
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

/// `InboxComment`. On the timeline the rail already shows the avatar, so the
/// header shows only the name, and a long body starts clamped.
fn inbox_comment(
    comment: &WorkItemComment,
    props: &CommentsProps,
    nested: bool,
    timeline: bool,
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
    let author = author_name(&comment.author);
    let name = div()
        .min_w_0()
        .truncate()
        .medium()
        .text_color(theme.colors.content)
        .child(author.clone());
    let person: AnyElement = if timeline {
        name.into_any_element()
    } else {
        let avatar_url = inbox_person_avatar_url(
            props.provider,
            &comment.author,
            comment.author_avatar_url.as_deref(),
        );
        div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .child(avatar(&author, &avatar_url, 20., cx))
            .child(name)
            .into_any_element()
    };
    let mut header = div()
        .flex()
        .min_w_0()
        .flex_wrap()
        .items_center()
        .gap_x(u(8.))
        .gap_y(u(4.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50))
        .child(person);
    if !nested {
        header = if timeline {
            header.min_h(u(36.)).px(u(12.)).py(u(6.))
        } else {
            header.px(u(12.)).py(u(8.))
        };
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
                span.text_color(with_alpha(theme.colors.success, 0.8))
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
        let key = format!("comment:{}", comment.id);
        let view = markdown.view(&key, &comment.body, true, cx);
        let content = div().child(view);
        let body = if timeline {
            div().child(collapsible_body(
                &key,
                content.into_any_element(),
                &props.clamps,
                &theme,
            ))
        } else {
            content
        };
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
                    .child(inbox_comment(reply, props, true, false, markdown, cx)),
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

#[cfg(test)]
mod tests {
    use super::*;

    // InboxComments.test.ts

    fn comment(id: &str, created_at: &str) -> WorkItemComment {
        WorkItemComment {
            id: id.into(),
            kind: "comment".into(),
            author: "maya".into(),
            body: id.into(),
            created_at: created_at.into(),
            ..Default::default()
        }
    }

    fn commit(oid: &str, author: &str, committed_date: &str) -> WorkItemCommit {
        WorkItemCommit {
            oid: oid.into(),
            message_headline: oid.into(),
            author: author.into(),
            committed_date: committed_date.into(),
            url: String::new(),
        }
    }

    #[test]
    fn interleaves_comments_and_commits_oldest_first() {
        let thread = WorkItemThread {
            comments: vec![
                comment("review", "2026-10-03T12:00:00Z"),
                comment("opened", "2026-10-03T09:00:00Z"),
            ],
            commits: vec![
                commit("c1", "maya", "2026-10-03T10:00:00Z"),
                commit("c2", "maya", "2026-10-03T10:05:00Z"),
                commit("c3", "jonas", "2026-10-03T10:10:00Z"),
                commit("c4", "maya", "2026-10-03T13:00:00Z"),
            ],
            ..Default::default()
        };
        let ids: Vec<&str> = activity_timeline(&thread)
            .into_iter()
            .map(|entry| match entry {
                ActivityEntry::Comment(comment) => comment.id.as_str(),
                ActivityEntry::Commit(commit) => commit.oid.as_str(),
            })
            .collect();
        assert_eq!(ids, ["opened", "c1", "c2", "c3", "review", "c4"]);

        // Back-to-back commits by one author group into one push.
        let pushes: Vec<usize> = timeline_items(&activity_timeline(&thread))
            .into_iter()
            .filter_map(|item| match item {
                TimelineItem::Commits { commits, .. } => Some(commits.len()),
                TimelineItem::Comment(_) => None,
            })
            .collect();
        assert_eq!(pushes, [2, 1, 1]);
    }

    /// The text each rail row draws, as `textContent` read it.
    fn rail_text(rows: &[RailRow<'_>]) -> String {
        rows.iter()
            .map(|row| match row {
                RailRow::Comment { comment, .. } => format!("{}{}", comment.author, comment.body),
                RailRow::Event {
                    name, action, time, ..
                } => format!("{name}{action}{time}"),
                RailRow::Commit { commit, .. } => format!(
                    "{}{}",
                    commit.message_headline,
                    commit.oid.chars().take(7).collect::<String>()
                ),
            })
            .collect()
    }

    #[test]
    fn puts_a_push_and_a_bare_approval_on_the_rail_between_comments() {
        let thread = WorkItemThread {
            comments: vec![
                comment("Can we handle concurrent retries?", "2026-10-03T09:00:00Z"),
                WorkItemComment {
                    id: "approval".into(),
                    kind: "review".into(),
                    author: "priya".into(),
                    state: "APPROVED".into(),
                    ..comment("", "2026-10-03T12:00:00Z")
                },
            ],
            commits: vec![
                commit("abc1234def", "maya", "2026-10-03T10:00:00Z"),
                commit("def5678abc", "maya", "2026-10-03T10:05:00Z"),
            ],
            ..Default::default()
        };
        assert_eq!(activity_count_label(&thread), "1 comment · 2 commits");
        let rows = rail_rows(&thread, InboxProvider::Github, 0);
        let text = rail_text(&rows);
        let at = |needle: &str| text.find(needle).expect(needle);
        assert!(at("concurrent retries") < at("added 2 commits"));
        assert!(at("abc1234") < at("priya"));
        assert!(text.contains("priyaapproved"));

        // The rail starts at the first stop and ends at the last.
        assert!(matches!(
            rows.first(),
            Some(RailRow::Comment { first: true, .. })
        ));
        assert!(matches!(
            rows.last(),
            Some(RailRow::Event {
                node: RailNode::Approved,
                last: true,
                ..
            })
        ));
        assert!(
            rows.iter()
                .all(|row| !matches!(row, RailRow::Commit { last: true, .. }))
        );
    }

    #[test]
    fn only_github_threads_read_as_a_timeline() {
        let props = |provider| CommentsProps {
            provider,
            reply_mode: None,
            on_reply: None,
            now: 0,
            clamps: BodyClamps::new(gpui::EntityId::from(1u64)),
        };
        assert!(props(InboxProvider::Github).timeline());
        for provider in [
            InboxProvider::Gitlab,
            InboxProvider::AzureDevops,
            InboxProvider::Linear,
            InboxProvider::Jira,
        ] {
            assert!(!props(provider).timeline());
        }
    }

    #[test]
    fn counts_review_events_only_outside_the_timeline() {
        let thread = WorkItemThread {
            comments: vec![
                comment("a", "2026-10-03T09:00:00Z"),
                WorkItemComment {
                    kind: "review".into(),
                    state: "APPROVED".into(),
                    ..comment("", "2026-10-03T10:00:00Z")
                },
            ],
            ..Default::default()
        };
        assert_eq!(comment_count_label(&thread, false), "2 comments");
        assert_eq!(comment_count_label(&thread, true), "1 comment");
        assert_eq!(activity_count_label(&thread), "1 comment");
    }
}
