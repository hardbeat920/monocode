use super::*;
use crate::popover::{Align, Anchor, popover};
use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, CursorStyle, Div, FontWeight, Hsla,
    MouseButton, Pixels, Role, SharedString, Stateful, Window, canvas, div, list, prelude::*, px,
    rgb,
};

fn action(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    plain_action(id, label).hover(|s| s.bg(content(0.07)).text_color(content(1.)))
}
/// An [`action`] without its hover, for callers that set their own.
fn plain_action(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    let id: SharedString = id.into();
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(content(0.7))
        .cursor_pointer()
        .child(label)
}
/// The green of the tick that confirms a copy.
const COPIED: Hsla = Hsla {
    h: 0.396,
    s: 0.72,
    l: 0.56,
    a: 1.,
};
const COPIED_POP: Duration = Duration::from_millis(260);

fn icon_action(id: impl Into<SharedString>, label: &str, name: &'static str) -> Stateful<Div> {
    icon_action_ink(id, label, name, content(0.7))
}
fn icon_action_ink(
    id: impl Into<SharedString>,
    label: &str,
    name: &'static str,
    ink: Hsla,
) -> Stateful<Div> {
    icon_action_with(id, label, action_icon(name, ink), ink)
}
fn icon_action_with(
    id: impl Into<SharedString>,
    label: &str,
    glyph: impl IntoElement,
    ink: Hsla,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.to_owned())
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(content(0.7))
        .cursor_pointer()
        .hover(move |s| {
            if ink == shell_fill() {
                s.bg(shell_fill().opacity(0.1)).text_color(ink)
            } else {
                s.bg(content(0.07)).text_color(content(1.))
            }
        })
        .child(glyph)
        .child(label.to_owned())
}
fn action_icon(name: &'static str, ink: Hsla) -> gpui::Svg {
    icon(name, 14.).text_color(ink)
}
pub(crate) fn relative_time(iso: &str) -> String {
    if model::timestamp_ms(iso) == 0 {
        return String::new();
    }
    let mut value =
        (((model::timestamp_ms(iso) - chrono::Utc::now().timestamp_millis()) as f64 / 1000.) + 0.5)
            .floor();
    let mut unit = "second";
    for (step, name) in [
        (60., "second"),
        (60., "minute"),
        (24., "hour"),
        (7., "day"),
        (4.34524, "week"),
        (12., "month"),
        (f64::INFINITY, "year"),
    ] {
        unit = name;
        if value.abs() < step {
            break;
        }
        value = ((value / step) + 0.5).floor();
    }
    let amount = value as i64;
    match (unit, amount) {
        ("second", 0) => "now".into(),
        ("day", -1) => "yesterday".into(),
        ("day", 1) => "tomorrow".into(),
        ("week" | "month" | "year", -1) => format!("last {unit}"),
        ("week" | "month" | "year", 1) => format!("next {unit}"),
        _ => {
            let count = amount.unsigned_abs();
            let label = format!("{count} {unit}{}", if count == 1 { "" } else { "s" });
            if amount > 0 {
                format!("in {label}")
            } else {
                format!("{label} ago")
            }
        }
    }
}
fn review_label(state: &str) -> String {
    match state.trim().to_uppercase().as_str() {
        "APPROVED" => "Approved".into(),
        "CHANGES_REQUESTED" => "Changes requested".into(),
        "COMMENTED" => "Commented".into(),
        "DISMISSED" => "Dismissed".into(),
        "PENDING" => "Pending".into(),
        "REVIEW_REQUIRED" => "Review required".into(),
        _ => state.replace('_', " ").to_lowercase(),
    }
}
fn failed_check(state: &str) -> bool {
    matches!(
        state.to_lowercase().as_str(),
        "failure" | "failed" | "error" | "timed_out" | "cancelled" | "action_required"
    )
}
fn review_event(comment: &model::Comment) -> bool {
    comment.kind == "review" && comment.body.trim().is_empty() && comment.replies.is_empty()
}
#[derive(Clone)]
pub(super) enum Activity {
    Comment(std::sync::Arc<model::Comment>),
    Commits(Vec<model::Commit>),
}
#[derive(Default)]
pub(super) struct SummaryRows {
    revision: u64,
    discussion: bool,
    keys: Vec<SharedString>,
}
fn timeline_event(name: &str, action: &str, time: &str) -> Div {
    div()
        .h(px(20.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(content(0.5))
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .text_color(content(1.))
                .child(if name.is_empty() {
                    "ghost".to_owned()
                } else {
                    name.to_owned()
                }),
        )
        .child(action.to_owned())
        .when(!time.is_empty(), |d| {
            d.child("·").child(relative_time(time))
        })
}
fn menu_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: bool,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let label = label.into();
    div()
        .id(id)
        .role(Role::MenuItemCheckBox)
        .aria_label(label.clone())
        .h(px(28.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(content(0.8))
        .cursor_pointer()
        .hover(|s| s.bg(content(0.06)))
        .child(
            div()
                .size(px(14.))
                .when(checked, |d| d.child(icon("check", 14.))),
        )
        .child(div().flex_1().min_w_0().truncate().child(label))
}
fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .px(px(8.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(content(0.4))
        .child(text.into())
}
fn message(text: impl Into<SharedString>) -> Div {
    div()
        .p(px(12.))
        .text_size(px(12.))
        .line_height(px(19.))
        .text_color(content(0.5))
        .child(text.into())
}
fn error(text: impl Into<SharedString>) -> Div {
    message(text).text_color(rgb(0xf87171))
}
fn provider_mark(provider: &str) -> gpui::Svg {
    let (path, color) = match provider {
        "linear" => ("inbox-linear", content(0.8)),
        "jira" => ("inbox-jira", rgb(0x2684ff).into()),
        "gitlab" => ("inbox-gitlab", rgb(0xfc6d26).into()),
        "azuredevops" => ("inbox-azuredevops", content(0.8)),
        _ => ("inbox-github", content(0.8)),
    };
    icon(path, 14.).text_color(color)
}
fn status_color(item: &Item) -> Hsla {
    match item.status() {
        "Merged" => rgb(0xa78bfa).into(),
        "Closed" => rgb(0xf87171).into(),
        "Draft" => content(0.4),
        _ => rgb(0x4ade80).into(),
    }
}
fn status_icon(item: &Item) -> &'static str {
    if item.kind == "pr" {
        "git-pull-request"
    } else if item.status() == "Closed" {
        "check-circle"
    } else {
        "circle-dot"
    }
}
fn attention_label(reason: &str) -> String {
    let reason = reason.trim().to_lowercase();
    match reason.as_str() {
        "assigned" => "Assigned to you",
        "mentioned" | "directly_addressed" => "Mentioned you",
        "review_requested" => "Review requested",
        "review_submitted" => "Review submitted",
        "approval_required" => "Approval required",
        "build_failed" => "Pipeline failed",
        "unmergeable" => "Cannot be merged",
        "merge_train_removed" => "Removed from merge train",
        "member_access_requested" => "Access requested",
        "marked" => "Added to your to-dos",
        _ => {
            return reason
                .split('_')
                .filter(|word| !word.is_empty())
                .enumerate()
                .map(|(ix, word)| {
                    if ix == 0 {
                        let mut chars = word.chars();
                        chars
                            .next()
                            .map(|c| c.to_uppercase().to_string() + chars.as_str())
                            .unwrap_or_default()
                    } else {
                        word.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    .into()
}
fn toggle(list: &mut Vec<String>, key: String) {
    if list.contains(&key) {
        list.retain(|value| value != &key);
    } else {
        list.push(key);
    }
}
fn project_name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn label_badge(label: &model::Label, compact: bool) -> Div {
    let color = u32::from_str_radix(label.color.trim().trim_start_matches('#'), 16)
        .ok()
        .filter(|_| label.color.trim().trim_start_matches('#').len() == 6);
    div()
        .max_w(px(if compact { 80. } else { 200. }))
        .rounded(px(3.))
        .px(px(6.))
        .py(px(1.))
        .flex()
        .items_center()
        .gap(px(4.))
        .bg(content(0.08))
        .text_size(px(if compact { 10. } else { 11. }))
        .text_color(content(0.5))
        .when_some(color, |d, color| {
            d.child(div().size(px(6.)).rounded_full().bg(rgb(color)))
        })
        .child(div().min_w_0().truncate().child(label.name.clone()))
}

#[derive(Clone, Copy)]
enum CommentStyle {
    Plain,
    Timeline,
    Reply,
}

impl Inbox {
    fn project_label(&self, path: &str) -> String {
        self.appearances.get(path).map_or_else(
            || project_name(std::path::Path::new(path)),
            |appearance| appearance.label(path),
        )
    }
    fn project_mark(&self, path: &str, side: f32) -> AnyElement {
        crate::project_settings::project_icon(
            &self.appearances.get(path).cloned().unwrap_or_default(),
            path,
            side,
        )
    }

    fn list_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let connected = model::PROVIDERS
            .into_iter()
            .filter(|provider| {
                self.snapshot
                    .connections
                    .get(*provider)
                    .is_some_and(|s| s.connected)
            })
            .collect::<Vec<_>>();
        let mut items = self.visible_items(cx);
        // An item opened from a link leads the rail until another row is picked.
        if let Some(pinned) = self
            .pinned
            .as_ref()
            .filter(|pinned| items.iter().all(|item| item.key() != pinned.key()))
        {
            items.insert(0, pinned.clone());
        }
        let sources = div()
            .h(px(36.))
            .px(px(8.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(1.))
            .border_b_1()
            .border_color(content(0.07))
            .children(connected.iter().map(|provider| {
                let source = provider.to_string();
                div()
                    .id(SharedString::from(format!("inbox-source-{provider}")))
                    .role(Role::Tab)
                    .aria_label(model::provider_label(provider))
                    .flex_1()
                    .min_w_0()
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .text_color(content(if self.state.source == *provider {
                        1.
                    } else {
                        0.5
                    }))
                    .when(self.state.source == *provider, |d| d.bg(content(0.1)))
                    .hover(|s| s.bg(content(0.05)))
                    .child(provider_mark(provider))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(model::provider_label(provider)),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.change_source(source.clone(), cx)),
                    )
            }))
            .child(
                div()
                    .id("inbox-add-connection")
                    .role(Role::Button)
                    .aria_label("Add connection")
                    .relative()
                    .h(px(24.))
                    .flex_1()
                    .min_w_0()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.4))
                    .hover(|s| s.bg(content(0.05)))
                    .child(self.connection_anchor.probe())
                    .child(icon("plus", 14.))
                    .when(connected.len() < 2, |d| {
                        d.child(div().truncate().child("Add connection"))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.connections_open = !this.connections_open;
                        this.filters_open = false;
                        cx.notify();
                    })),
            );
        let tools = div()
            .h(px(36.))
            .px(px(8.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(1.))
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(28.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(icon("search", 12.))
                    .child(self.search.clone()),
            )
            .child(
                icon_button("inbox-filter", "Filter inbox", "filter")
                    .relative()
                    .child(self.filter_anchor.probe())
                    .when(self.filters_open || self.filters_active(), |d| {
                        d.bg(content(0.1))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filters_open = !this.filters_open;
                        this.connections_open = false;
                        cx.notify();
                    })),
            )
            .child(
                icon_button("inbox-read-all", "Mark all as read", "check-2").on_click(cx.listener(
                    |this, _, _, cx| {
                        for item in &this.snapshot.items {
                            if item.provider == this.state.source {
                                this.state.seen.mark(item);
                            }
                        }
                        this.save();
                        cx.notify();
                    },
                )),
            )
            .child(
                icon_button(
                    "inbox-refresh",
                    "Refresh",
                    if self.loading { "loader" } else { "refresh-cw" },
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.refresh(cx);
                    this.load_detail(false, cx);
                })),
            );
        let empty = if connected.is_empty() && !self.loading {
            "Add a connection to start using the Inbox.".into()
        } else if self.loading && self.snapshot.items.is_empty() {
            "Loading inbox…".into()
        } else if let Some(error) = self.snapshot.errors.get(&self.state.source) {
            error.clone()
        } else if self.projects.is_empty() && !model::is_tracker(&self.state.source) {
            "Open a project to fill the inbox".into()
        } else if self.filters_active() || !self.search.read(cx).content.is_empty() {
            "No items match these filters".into()
        } else if model::is_tracker(&self.state.source) {
            format!("No {} issues", model::provider_label(&self.state.source))
        } else {
            "No matching issues or pull requests".into()
        };
        let rows = items
            .into_iter()
            .map(|item| {
                let key = item.key();
                let active = self.selected.as_ref() == Some(&key);
                let unread = self.state.seen.unseen(&item);
                let sessions = self.related_sessions(&item).len();
                let tracker = model::is_tracker(&item.provider);
                let kind = if item.kind == "pr" {
                    if item.provider == "gitlab" {
                        "Merge request"
                    } else {
                        "Pull request"
                    }
                } else {
                    "Issue"
                };
                let source = if tracker && !item.team_name.is_empty() {
                    item.team_name.clone()
                } else if !item.repo.is_empty() {
                    item.repo.clone()
                } else {
                    self.project_label(&item.project_path)
                };
                let attention = attention_label(&item.attention_reason);
                div()
                    .id(SharedString::from(format!("inbox-item-{key}")))
                    .role(Role::Button)
                    .aria_label(item.title.clone())
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(content(0.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(content(0.1)))
                    .when(!active, |d| d.hover(|s| s.bg(content(0.05))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .text_color(content(0.5))
                            .child(provider_mark(&item.provider))
                            .child(icon(status_icon(&item), 12.).text_color(status_color(&item)))
                            .child(div().min_w_0().flex_1().truncate().child(format!(
                                "{kind} · {}{}",
                                item.reference(),
                                if attention.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · {attention}")
                                }
                            )))
                            .when(sessions > 0, |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(2.))
                                        .text_color(accent_color())
                                        .child(
                                            icon("message-multiple", 12.)
                                                .text_color(accent_color()),
                                        )
                                        .child(sessions.to_string()),
                                )
                            })
                            .child(
                                div()
                                    .text_color(content(0.45))
                                    .child(relative_time(&item.updated_at)),
                            )
                            .when(unread, |d| {
                                d.child(div().size(px(6.)).rounded_full().bg(accent_color()))
                            }),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(1.))
                            .child(item.title.clone()),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .when(!tracker && !item.project_path.is_empty(), |d| {
                                        d.child(self.project_mark(&item.project_path, 12.))
                                    })
                                    .child(div().min_w_0().truncate().child(source)),
                            )
                            .children(
                                item.labels
                                    .iter()
                                    .take(2)
                                    .map(|label| label_badge(label, true)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select(key.clone(), cx)))
            })
            .collect::<Vec<_>>();
        div()
            .relative()
            .w(px(self.state.list_width.clamp(240., 420.)))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(content(0.07))
            .flex()
            .flex_col()
            .child(sources)
            .when(!connected.is_empty(), |d| d.child(tools))
            .child(
                div()
                    .id("inbox-list-scroll")
                    .role(Role::ListBox)
                    .aria_label("Inbox items")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .p(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when(rows.is_empty(), |d| d.child(message(empty)))
                    .children(rows),
            )
            .child(
                div()
                    .id("inbox-list-resize")
                    .role(Role::Splitter)
                    .aria_label("Resize inbox list")
                    .absolute()
                    .right(px(-3.))
                    .top_0()
                    .bottom_0()
                    .w(px(6.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            this.resize_start =
                                Some((f32::from(event.position.x), this.state.list_width));
                            cx.stop_propagation();
                        }),
                    ),
            )
            .into_any_element()
    }

    fn related_sessions(&self, item: &Item) -> Vec<Session> {
        let work_item = item.work_item();
        self.sessions
            .iter()
            .filter(|s| {
                s.work_item.as_ref().is_some_and(|work| {
                    (!work.url.is_empty() && work.url == work_item.url)
                        || (work.kind == work_item.kind
                            && work.number == work_item.number
                            && work.repo.eq_ignore_ascii_case(&work_item.repo))
                })
            })
            .cloned()
            .collect()
    }

    fn detail_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.item() else {
            // A link can be loading or have failed with no row to show yet.
            return div()
                .size_full()
                .flex()
                .flex_col()
                .when_some(self.link_notice_view(cx), |d, notice| d.child(notice))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(12.))
                        .child(icon("inbox", 24.).text_color(content(0.3)))
                        .child(message("Select an inbox item").text_size(px(13.))),
                )
                .into_any_element();
        };
        let is_pr = item.kind == "pr";
        let details = self.details.clone().unwrap_or_default();
        let assignees = item
            .assignees
            .iter()
            .filter(|person| {
                !person
                    .login
                    .trim()
                    .eq_ignore_ascii_case(details.author.trim())
            })
            .collect::<Vec<_>>();
        let base_ref = if details.base_ref_name.is_empty() {
            self.thread
                .as_ref()
                .map(|t| t.base_ref_name.as_str())
                .unwrap_or("")
        } else {
            &details.base_ref_name
        };
        let head_ref = if details.head_ref_name.is_empty() {
            self.thread
                .as_ref()
                .map(|t| t.head_ref_name.as_str())
                .unwrap_or("")
        } else {
            &details.head_ref_name
        };
        let title = div()
            .w_full()
            .flex_shrink_0()
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .w_full()
                    .px(px(12.))
                    .pt(px(20.))
                    .pb(px(if is_pr { 0. } else { 20. }))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .text_color(content(0.5))
                            .when(!self.back.is_empty(), |d| {
                                d.child(
                                    icon_action("inbox-back", "Back", "chevron-left")
                                        .h(px(20.))
                                        .px(px(4.))
                                        .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                                )
                            })
                            .child(provider_mark(&item.provider))
                            .child(if is_pr {
                                if item.provider == "gitlab" {
                                    "Merge request"
                                } else {
                                    "Pull request"
                                }
                            } else {
                                "Issue"
                            })
                            .child(item.reference())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_color(status_color(&item))
                                    .child(
                                        icon(status_icon(&item), 14.)
                                            .text_color(status_color(&item)),
                                    )
                                    .child(item.status()),
                            )
                            .when(!item.attention_reason.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_color(accent_color())
                                        .child(attention_label(&item.attention_reason)),
                                )
                            })
                            .child(div().min_w_0().truncate().child(
                                if item.team_name.is_empty() {
                                    item.repo.clone()
                                } else {
                                    item.team_name.clone()
                                },
                            )),
                    )
                    .child(
                        div()
                            .line_clamp(2)
                            .text_size(px(20.))
                            .line_height(px(25.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(1.))
                            .child(item.title.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .overflow_hidden()
                            .text_size(px(12.))
                            .text_color(content(0.5))
                            .when(!details.author.is_empty(), |d| {
                                d.child(avatar::person(
                                    &item.provider,
                                    &details.author,
                                    &details.author_avatar_url,
                                    16.,
                                    window,
                                    cx,
                                ))
                            })
                            .when(!assignees.is_empty(), |d| {
                                d.child("·").children(assignees.iter().map(|person| {
                                    avatar::person(
                                        &item.provider,
                                        &person.login,
                                        &person.avatar_url,
                                        16.,
                                        window,
                                        cx,
                                    )
                                }))
                            })
                            .when(
                                item.assignees.is_empty()
                                    && matches!(
                                        item.provider.as_str(),
                                        "github" | "gitlab" | "azuredevops"
                                    ),
                                |d| d.child("·").child("Unassigned"),
                            )
                            .when(!item.created_at.is_empty(), |d| {
                                d.child("·")
                                    .child(format!("Created {}", relative_time(&item.created_at)))
                            })
                            .when(!item.updated_at.is_empty(), |d| {
                                d.child("·")
                                    .child(format!("Updated {}", relative_time(&item.updated_at)))
                            })
                            .when(!base_ref.is_empty() && !head_ref.is_empty(), |d| {
                                let branch = head_ref.to_owned();
                                d.child("·").child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(icon("git-compare", 12.).text_color(content(0.5)))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(format!("{base_ref} ← {head_ref}")),
                                        )
                                        .child(
                                            icon_button(
                                                "inbox-copy-branch",
                                                "Copy branch name",
                                                "response-copy",
                                            )
                                            .size(px(18.))
                                            .on_click(
                                                move |_, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(branch.clone()),
                                                    )
                                                },
                                            ),
                                        ),
                                )
                            })
                            .when(!details.review_decision.is_empty(), |d| {
                                d.child("·").child(review_label(&details.review_decision))
                            }),
                    )
                    .when(!self.related_sessions(&item).is_empty(), |d| {
                        d.child(
                            div()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .overflow_hidden()
                                .child(icon("message-multiple", 14.).text_color(content(0.45)))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(content(0.45))
                                        .child("Related threads"),
                                )
                                .children(self.related_sessions(&item).into_iter().map(
                                    |session| {
                                        action(
                                            format!("inbox-session-{}", session.id),
                                            session.title,
                                        )
                                        .h(px(24.))
                                        .bg(content(0.05))
                                        .max_w(px(256.))
                                        .truncate()
                                        .on_click(
                                            cx.listener(move |_, _, _, cx| {
                                                cx.emit(Event::OpenSession(session.id))
                                            }),
                                        )
                                    },
                                )),
                        )
                    })
                    .child(
                        div()
                            .pt(px(2.))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .when(!is_pr, |d| {
                                d.child(
                                    action("inbox-send-agent", "Open in new session")
                                        .px(px(12.))
                                        .bg(content(1.))
                                        .text_color(shell_fill())
                                        .font_weight(FontWeight::MEDIUM)
                                        .when(self.detail_loading || self.details.is_none(), |d| {
                                            d.opacity(0.4)
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if !this.detail_loading && this.details.is_some() {
                                                this.start(cx);
                                            }
                                        })),
                                )
                            })
                            .when(!is_pr && item.project_path.is_empty(), |d| {
                                d.child(
                                    icon_action(
                                        "inbox-choose-project",
                                        &self.project_label(&self.start_project.to_string_lossy()),
                                        "folder",
                                    )
                                    .relative()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .child(icon("chevron-down", 12.))
                                    .child(self.project_anchor.probe())
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.project_picker = !this.project_picker;
                                            cx.notify();
                                        },
                                    )),
                                )
                            })
                            .when(is_pr && item.provider == "github", |d| {
                                d.child(self.pr_actions(&item, cx))
                            })
                            .child(self.copy_link_button(&item, cx))
                            .child(
                                icon_action(
                                    "inbox-open-external",
                                    &format!(
                                        "{} {}",
                                        if is_pr { "Review on" } else { "Open in" },
                                        model::provider_label(&item.provider)
                                    ),
                                    "external-link",
                                )
                                .when(item.url.is_empty(), |d| d.opacity(0.4))
                                .on_click({
                                    let url = item.url.clone();
                                    move |_, _, cx| {
                                        if !url.is_empty() {
                                            cx.open_url(&url);
                                        }
                                    }
                                }),
                            ),
                    )
                    .when(is_pr, |d| {
                        d.child(
                            div()
                                .id("inbox-pr-tabs")
                                .role(Role::TabList)
                                .aria_label("Pull request details")
                                .h(px(36.))
                                .flex()
                                .items_stretch()
                                .gap(px(16.))
                                .children(
                                    [
                                        (Tab::Summary, "Summary"),
                                        (Tab::Code, "Code"),
                                        (Tab::Checks, "Checks"),
                                    ]
                                    .into_iter()
                                    .filter(|(tab, _)| {
                                        *tab != Tab::Checks || item.provider == "github"
                                    })
                                    .map(|(tab, label)| {
                                        plain_action(format!("inbox-tab-{label}"), label)
                                            .role(Role::Tab)
                                            .h_full()
                                            .px_0()
                                            .rounded_none()
                                            .hover(|s| s.text_color(content(1.)))
                                            .when(self.tab == tab, |d| {
                                                d.border_b_2()
                                                    .border_color(content(1.))
                                                    .text_color(content(1.))
                                            })
                                            .when(tab == Tab::Checks, |d| {
                                                let failed = self
                                                    .checks
                                                    .as_ref()
                                                    .map(|checks| {
                                                        checks
                                                            .checks
                                                            .iter()
                                                            .filter(|check| {
                                                                failed_check(&check.state)
                                                            })
                                                            .count()
                                                    })
                                                    .unwrap_or(0);
                                                d.when(failed > 0, |d| {
                                                    d.child(
                                                        icon("circle-x", 13.)
                                                            .text_color(rgb(0xf87171)),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_color(rgb(0xf87171))
                                                            .child(failed.to_string()),
                                                    )
                                                })
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_tab(tab, cx)
                                            }))
                                    }),
                                ),
                        )
                    }),
            );
        let body = match self.tab {
            Tab::Summary => self.summary_view(&item, window, cx),
            Tab::Code => self.diff_view(),
            Tab::Checks => self.checks_view(cx),
        };
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(title)
            .when_some(self.link_notice_view(cx), |d, notice| d.child(notice))
            .when_some(self.detail_error.clone(), |d, text| d.child(error(text)))
            .child(body)
            .into_any_element()
    }

    fn link_notice_view(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.link_notice.as_ref()? {
            LinkNotice::Loading(label) => message(format!("Opening {label}…")).into_any_element(),
            LinkNotice::Failed {
                url,
                message: reason,
            } => {
                let url = url.clone();
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w_0().child(error(reason.clone())))
                    .child(
                        action("inbox-link-browser", "Open in browser")
                            .debug_selector(|| "inbox-link-browser".into())
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .child(
                        action("inbox-link-dismiss", "Dismiss").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.link_notice = None;
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element()
            }
        })
    }

    /// Copy link, which turns into a green tick reading "Copied link" for a
    /// moment once the address is on the clipboard.
    fn copy_link_button(&self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let copied = self
            .copied_link
            .as_ref()
            .filter(|(url, _)| *url == item.url)
            .map(|(_, press)| *press);
        let url = item.url.clone();
        let on_click = cx.listener(move |this, _, _, cx| this.copy_link(url.clone(), cx));
        let button = match copied {
            Some(press) => icon_action_with(
                "inbox-copy-link",
                "Copied link",
                action_icon("check", COPIED).with_animation(
                    ("inbox-copied-tick", press),
                    Animation::new(COPIED_POP).with_easing(|t| 1. - (1. - t).powi(3)),
                    |svg, t| svg.size(px(14. * (0.5 + 0.5 * t))).opacity(t),
                ),
                COPIED,
            )
            .text_color(content(1.)),
            None => icon_action("inbox-copy-link", "Copy link", "response-copy")
                .when(item.url.is_empty(), |d| d.opacity(0.4)),
        };
        button.on_click(on_click).into_any_element()
    }

    fn pr_actions(&self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let open = item.state.eq_ignore_ascii_case("open");
        let selected = self.merge_action.clone();
        let merge_label = match selected.as_str() {
            "squash" => "Squash and merge",
            "rebase" => "Rebase and merge",
            _ => "Merge pull request",
        };
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .when(open && !item.draft, |d| {
                d.child(
                    div()
                        .relative()
                        .h(px(28.))
                        .flex()
                        .items_stretch()
                        .rounded(px(6.))
                        .bg(content(1.))
                        .text_color(shell_fill())
                        .child(self.action_anchor.probe())
                        .child(
                            icon_action_ink(
                                "inbox-pr-merge",
                                merge_label,
                                "git-merge",
                                shell_fill(),
                            )
                            .px(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(shell_fill())
                            .when(self.acting, |d| d.opacity(0.4))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.acting {
                                        this.confirm_action = Some(selected.clone());
                                        this.action_error = None;
                                        this.action_notice = None;
                                        cx.notify();
                                    }
                                },
                            )),
                        )
                        .child(
                            div()
                                .id("inbox-merge-options")
                                .role(Role::Button)
                                .aria_label("Merge options")
                                .h_full()
                                .w(px(28.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_l_1()
                                .border_color(shell_fill().opacity(0.2))
                                .cursor_pointer()
                                .child(icon("chevron-down", 12.).text_color(shell_fill()))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.acting {
                                        this.action_menu = !this.action_menu;
                                        cx.notify();
                                    }
                                })),
                        ),
                )
            })
            .children(
                [
                    (open && item.draft, "ready", "Ready for review"),
                    (open && !item.draft, "draft", "Convert to draft"),
                    (open, "close", "Close pull request"),
                    (
                        item.state.eq_ignore_ascii_case("closed"),
                        "reopen",
                        "Reopen pull request",
                    ),
                ]
                .into_iter()
                .filter(|(show, _, _)| *show)
                .map(|(_, action_name, label)| {
                    let anchor_ix = match action_name {
                        "ready" => 0,
                        "draft" => 1,
                        "close" => 2,
                        _ => 3,
                    };
                    icon_action(
                        format!("inbox-pr-{action_name}"),
                        label,
                        match action_name {
                            "draft" => "git-pull-request-draft",
                            "close" => "git-pull-request-closed",
                            _ => "git-pull-request",
                        },
                    )
                    .relative()
                    .child(self.pr_action_anchors[anchor_ix].probe())
                    .border_1()
                    .border_color(content(0.15))
                    .px(px(12.))
                    .when(self.acting, |d| d.opacity(0.4))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.acting {
                            this.confirm_action = Some(action_name.into());
                            this.action_error = None;
                            this.action_notice = None;
                            cx.notify();
                        }
                    }))
                }),
            )
            .when_some(self.action_notice.clone(), |d, notice| {
                d.child(
                    div()
                        .text_size(px(11.))
                        .text_color(content(0.55))
                        .child(notice),
                )
            })
            .into_any_element()
    }

    fn comment_card(
        &self,
        item: &Item,
        comment: &model::Comment,
        style: CommentStyle,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nested = matches!(style, CommentStyle::Reply);
        let timeline = matches!(style, CommentStyle::Timeline);
        let has_body = !comment.body.trim().is_empty();
        let has_replies = !nested && !comment.replies.is_empty();
        let author = if comment.author.is_empty() {
            "ghost"
        } else {
            &comment.author
        };
        let reply_id = if item.provider == "linear" {
            comment.id.clone()
        } else if item.provider == "github" {
            comment.thread_id.clone()
        } else {
            String::new()
        };
        let state = comment.state.trim().to_uppercase();
        let review_color = if state == "APPROVED" {
            rgb(0x4ade80).into()
        } else if state == "CHANGES_REQUESTED" {
            rgb(0xf87171).into()
        } else {
            content(0.5)
        };
        let document = self
            .comments
            .iter()
            .find(|(cached, _)| cached.id == comment.id)
            .map(|(_, document)| document.clone());
        let clamped = timeline
            && !self.expanded_comments.contains(&comment.id)
            && (comment.body.len() > 1200 || comment.body.lines().count() > 12);
        let expandable =
            timeline && (comment.body.len() > 1200 || comment.body.lines().count() > 12);
        let header = div()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(content(0.5))
            .when(!nested, |d| {
                d.px(px(12.))
                    .py(px(if timeline { 6. } else { 8. }))
                    .min_h(px(if timeline { 36. } else { 0. }))
            })
            .when(!nested && (has_body || has_replies), |d| {
                d.border_b_1().border_color(content(0.07))
            })
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .child(if timeline {
                        div().child(author.to_string()).into_any_element()
                    } else {
                        avatar::person(
                            &item.provider,
                            author,
                            &comment.author_avatar_url,
                            20.,
                            window,
                            cx,
                        )
                    }),
            )
            .when(!state.is_empty(), |d| {
                d.child("·")
                    .child(div().text_color(review_color).child(review_label(&state)))
            })
            .when(!comment.path.is_empty(), |d| {
                d.child("·").child(div().min_w_0().truncate().child(format!(
                        "{}{}",
                        comment.path,
                        comment
                            .line
                            .map(|line| format!(":{line}"))
                            .unwrap_or_default()
                    )))
            })
            .when(comment.resolved, |d| {
                d.child("·")
                    .child(div().text_color(rgb(0x4ade80)).child("Resolved"))
            })
            .when(!comment.created_at.is_empty(), |d| {
                d.child("·").child(
                    action(
                        format!("inbox-comment-time-{}", comment.id),
                        relative_time(&comment.created_at),
                    )
                    .h_auto()
                    .px_0()
                    .text_color(content(0.5))
                    .on_click({
                        let url = comment.url.clone();
                        move |_, _, cx| {
                            if !url.is_empty() {
                                cx.open_url(&url);
                            }
                        }
                    }),
                )
            })
            .when(!reply_id.is_empty(), |d| {
                let name = author.to_string();
                d.child("·").child(
                    action(format!("inbox-reply-{}", comment.id), "Reply")
                        .h_auto()
                        .px_0()
                        .text_color(content(0.5))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.reply = Some((reply_id.clone(), name.clone()));
                            this.summary_scroll.scroll_to_end();
                            window.focus(&this.comment.focus_handle(cx), cx);
                            cx.notify();
                        })),
                )
            });
        div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .when(!nested, |d| {
                d.rounded(px(6.))
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.05))
                    .overflow_hidden()
            })
            .child(header)
            .when(has_body, |d| {
                d.child(
                    div()
                        .when(nested, |d| d.mt(px(8.)))
                        .when(!nested, |d| d.px(px(12.)).py(px(10.)))
                        .child(
                            div()
                                .id(format!("inbox-comment-body-{}", comment.id))
                                .w_full()
                                .when(clamped, |d| d.max_h(px(180.)).overflow_hidden())
                                .when_some(document, |d, document| {
                                    d.child(document::DocumentElement::new(
                                        document,
                                        if nested {
                                            width
                                        } else {
                                            (width - px(26.)).max(px(1.))
                                        },
                                        clamped.then_some(px(180.)),
                                    ))
                                }),
                        )
                        .when(expandable, |d| {
                            let id = comment.id.clone();
                            d.child(
                                action(
                                    format!("inbox-expand-comment-{}", comment.id),
                                    if clamped { "Show more" } else { "Show less" },
                                )
                                .mt(px(6.))
                                .h_auto()
                                .px_0()
                                .text_color(content(0.5))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if !this.expanded_comments.remove(&id) {
                                            this.expanded_comments.insert(id.clone());
                                        }
                                        this.summary_scroll.remeasure();
                                        cx.notify();
                                    },
                                )),
                            )
                        }),
                )
            })
            .when(has_replies, |d| {
                d.child(
                    div()
                        .border_t_1()
                        .border_color(content(0.07))
                        .px(px(12.))
                        .children(comment.replies.iter().enumerate().map(|(ix, reply)| {
                            div()
                                .py(px(10.))
                                .when(ix > 0, |d| d.border_t_1().border_color(content(0.07)))
                                .child(self.comment_card(
                                    item,
                                    reply,
                                    CommentStyle::Reply,
                                    (width - px(26.)).max(px(1.)),
                                    window,
                                    cx,
                                ))
                        })),
                )
            })
            .into_any_element()
    }

    fn timeline_rail(&self, node: AnyElement, first: bool, last: bool, card: bool) -> Div {
        div()
            .w(px(20.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w(px(1.))
                    .h(px(if card { 8. } else { 0. }))
                    .flex_shrink_0()
                    .when(!first, |d| d.bg(content(0.1))),
            )
            .child(
                div()
                    .size(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(node),
            )
            .when(!last, |d| {
                d.child(div().w(px(1.)).flex_1().bg(content(0.1)))
            })
    }

    pub(super) fn rebuild_activity(&mut self) {
        self.activity.clear();
        self.activity_revision += 1;
        let Some(thread) = &self.thread else {
            return;
        };
        let timeline = self
            .item()
            .is_some_and(|item| item.kind == "pr" && item.provider == "github");
        if timeline {
            let mut entries = thread
                .comments
                .iter()
                .map(|comment| {
                    (
                        model::timestamp_ms(&comment.created_at),
                        Activity::Comment(std::sync::Arc::new(comment.clone())),
                    )
                })
                .chain(thread.commits.iter().map(|commit| {
                    (
                        model::timestamp_ms(&commit.committed_date),
                        Activity::Commits(vec![commit.clone()]),
                    )
                }))
                .collect::<Vec<_>>();
            entries.sort_by_key(|(at, _)| *at);
            let mut groups: Vec<Activity> = Vec::new();
            for (_, entry) in entries {
                match entry {
                    Activity::Commits(mut commits) => {
                        if let Some(Activity::Commits(previous)) = groups.last_mut()
                            && previous
                                .last()
                                .is_some_and(|c| c.author == commits[0].author)
                        {
                            previous.append(&mut commits);
                            continue;
                        }
                        groups.push(Activity::Commits(commits));
                    }
                    entry => groups.push(entry),
                }
            }
            self.activity = groups;
        } else {
            self.activity = thread
                .comments
                .iter()
                .cloned()
                .map(std::sync::Arc::new)
                .map(Activity::Comment)
                .collect();
        }
        self.summary_scroll.remeasure();
    }

    fn discussion_row(
        &self,
        item: &Item,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = self.activity[ix].clone();
        let timeline = item.kind == "pr" && item.provider == "github";
        let width = self.summary_body_width();
        if !timeline {
            let Activity::Comment(comment) = entry else {
                unreachable!()
            };
            return div()
                .pb(px(if ix + 1 == self.activity.len() {
                    0.
                } else {
                    8.
                }))
                .child(self.comment_card(item, &comment, CommentStyle::Plain, width, window, cx))
                .into_any_element();
        }
        let first = ix == 0;
        let last = ix + 1 == self.activity.len();
        let mut activity = Vec::new();
        match entry {
            Activity::Comment(comment) => {
                let event = review_event(&comment);
                let state = comment.state.to_uppercase();
                let color = if state == "APPROVED" {
                    rgb(0x4ade80).into()
                } else if state == "CHANGES_REQUESTED" {
                    rgb(0xf87171).into()
                } else {
                    content(0.45)
                };
                let node = if event {
                    icon(
                        if state == "APPROVED" {
                            "check-circle"
                        } else if state == "CHANGES_REQUESTED" {
                            "circle-x"
                        } else {
                            "message-multiple"
                        },
                        16.,
                    )
                    .text_color(color)
                    .into_any_element()
                } else {
                    avatar::image(
                        &item.provider,
                        &comment.author,
                        &comment.author_avatar_url,
                        20.,
                        window,
                        cx,
                    )
                };
                let body = if event {
                    let verb = match state.as_str() {
                        "APPROVED" => "approved",
                        "CHANGES_REQUESTED" => "requested changes",
                        "DISMISSED" => "had a review dismissed",
                        _ => "reviewed",
                    };
                    timeline_event(&comment.author, verb, &comment.created_at).into_any_element()
                } else {
                    self.comment_card(
                        item,
                        &comment,
                        CommentStyle::Timeline,
                        (width - px(32.)).max(px(1.)),
                        window,
                        cx,
                    )
                };
                activity.push(
                    div()
                        .min_w_0()
                        .flex()
                        .gap(px(12.))
                        .child(self.timeline_rail(node, first, last, !event))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .when(!last, |d| d.pb(px(12.)))
                                .child(body),
                        )
                        .into_any_element(),
                );
            }
            Activity::Commits(commits) => {
                let author = commits[0].author.clone();
                let time = commits
                    .last()
                    .map(|commit| commit.committed_date.clone())
                    .unwrap_or_default();
                let node = avatar::image(&item.provider, &author, "", 20., window, cx);
                activity.push(
                    div()
                        .min_w_0()
                        .flex()
                        .gap(px(12.))
                        .child(self.timeline_rail(node, first, false, false))
                        .child(div().min_w_0().flex_1().pb(px(12.)).child(timeline_event(
                            &author,
                            &format!(
                                "added {}",
                                if commits.len() == 1 {
                                    "a commit".into()
                                } else {
                                    format!("{} commits", commits.len())
                                }
                            ),
                            &time,
                        )))
                        .into_any_element(),
                );
                let commit_count = commits.len();
                for (commit_ix, commit) in commits.into_iter().enumerate() {
                    let final_commit = last && commit_ix + 1 == commit_count;
                    let url = commit.url.clone();
                    activity.push(
                        div()
                            .min_w_0()
                            .flex()
                            .gap(px(12.))
                            .child(
                                div()
                                    .w(px(20.))
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(div().h(px(6.)).w(px(1.)).bg(content(0.1)))
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .border_1()
                                            .border_color(content(0.35)),
                                    )
                                    .when(!final_commit, |d| {
                                        d.child(div().w(px(1.)).flex_1().bg(content(0.1)))
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .when(!final_commit, |d| d.pb(px(12.)))
                                    .child(
                                        div()
                                            .id(format!("inbox-commit-{}", commit.oid))
                                            .role(Role::Button)
                                            .aria_label("Open commit")
                                            .h(px(20.))
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .text_size(px(12.))
                                            .cursor_pointer()
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .flex_1()
                                                    .truncate()
                                                    .text_color(content(0.7))
                                                    .child(commit.message_headline),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .font_family("monospace")
                                                    .text_color(content(0.35))
                                                    .child(
                                                        commit
                                                            .oid
                                                            .chars()
                                                            .take(7)
                                                            .collect::<String>(),
                                                    ),
                                            )
                                            .on_click(move |_, _, cx| {
                                                if !url.is_empty() {
                                                    cx.open_url(&url);
                                                }
                                            }),
                                    ),
                            )
                            .into_any_element(),
                    );
                }
            }
        }
        div()
            .min_w_0()
            .flex()
            .flex_col()
            .children(activity)
            .into_any_element()
    }

    fn discussion_heading(
        &self,
        item: &Item,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let thread = self.thread.as_ref()?;
        if thread.comments.is_empty() && thread.commits.is_empty() && !thread.truncated {
            return None;
        }
        let timeline = item.kind == "pr" && item.provider == "github";
        let count = thread
            .comments
            .iter()
            .filter(|comment| !(timeline && review_event(comment)))
            .map(|comment| 1 + comment.replies.len())
            .sum::<usize>();
        Some(
            div()
                .w_full()
                .flex_shrink_0()
                .border_t_1()
                .border_color(content(0.07))
                .pt(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .text_color(content(0.5))
                        .child(div().text_color(content(0.7)).child(if timeline {
                            "Activity".into()
                        } else {
                            format!(
                                "{count} {}",
                                if count == 1 { "comment" } else { "comments" }
                            )
                        }))
                        .when(timeline, |d| {
                            d.child(format!(
                                "{count} {}{}",
                                if count == 1 { "comment" } else { "comments" },
                                if thread.commits.is_empty() {
                                    String::new()
                                } else {
                                    format!(
                                        " · {} {}",
                                        thread.commits.len(),
                                        if thread.commits.len() == 1 {
                                            "commit"
                                        } else {
                                            "commits"
                                        }
                                    )
                                }
                            ))
                        })
                        .when(thread.truncated, |d| {
                            d.child(format!(
                                "Latest comments · more on {}",
                                model::provider_label(&item.provider)
                            ))
                        }),
                )
                .when_some(self.thread_error.clone(), |d, text| d.child(error(text)))
                .into_any_element(),
        )
    }

    fn summary_body_width(&self) -> Pixels {
        (self.summary_width.get().min(px(1024.)) - px(24.)).max(px(1.))
    }

    fn summary_view(&self, item: &Item, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let discussion = self.thread.as_ref().is_some_and(|thread| {
            !thread.comments.is_empty() || !thread.commits.is_empty() || thread.truncated
        });
        let count = 2 + if discussion {
            1 + self.activity.len()
        } else {
            0
        };
        let mut rows = self.summary_rows.borrow_mut();
        if rows.keys.is_empty()
            || rows.revision != self.activity_revision
            || rows.discussion != discussion
        {
            let mut keys: Vec<SharedString> = vec!["intro".into()];
            if discussion {
                keys.push("activity".into());
                keys.extend(self.activity.iter().map(|activity| match activity {
                    Activity::Comment(comment) => format!("comment:{}", comment.id).into(),
                    Activity::Commits(commits) => format!("push:{}", commits[0].oid).into(),
                }));
            }
            keys.push("composer".into());
            // Retain unchanged prefix/suffix rows so refreshes preserve the
            // reader's item and pixel offset, including a focused composer.
            let prefix = rows
                .keys
                .iter()
                .zip(&keys)
                .take_while(|(old, new)| old == new)
                .count();
            let suffix = rows.keys[prefix..]
                .iter()
                .rev()
                .zip(keys[prefix..].iter().rev())
                .take_while(|(old, new)| old == new)
                .count();
            self.summary_scroll.splice_focusable(
                prefix..rows.keys.len() - suffix,
                (prefix..keys.len() - suffix)
                    .map(|ix| (ix + 1 == count).then(|| self.comment.focus_handle(cx))),
            );
            *rows = SummaryRows {
                revision: self.activity_revision,
                discussion,
                keys,
            };
        }
        drop(rows);
        let width = self.summary_width.clone();
        let entity = cx.entity();
        let item = item.clone();
        div()
            .id(format!("inbox-summary-{}", item.key()))
            .relative()
            .min_h_0()
            .min_w_0()
            .flex_1()
            .child(
                canvas(
                    move |bounds, _, _| {
                        width.set(bounds.size.width);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                list(self.summary_scroll.clone(), move |ix, window, cx| {
                    entity.update(cx, |this, cx| {
                        this.summary_row(&item, ix, count, discussion, window, cx)
                    })
                })
                .size_full(),
            )
            .into_any_element()
    }

    fn summary_row(
        &self,
        item: &Item,
        ix: usize,
        count: usize,
        discussion: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let can_post = !self.posting && !self.comment.read(cx).content.trim().is_empty();
        let body = if ix == 0 {
            div()
                .pt(px(20.))
                .pb(px(20.))
                .flex()
                .flex_col()
                .gap(px(20.))
                .when(!item.labels.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .children(item.labels.iter().map(|label| label_badge(label, false))),
                    )
                })
                .when(self.detail_loading && self.details.is_none(), |d| {
                    d.child(message("Loading description…"))
                })
                .when(
                    self.details
                        .as_ref()
                        .is_some_and(|d| d.body.trim().is_empty()),
                    |d| {
                        d.child(
                            div()
                                .text_size(px(13.))
                                .text_color(content(0.45))
                                .child("No description"),
                        )
                    },
                )
                .when_some(self.document.clone(), |d, document| {
                    d.child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .child(document::DocumentElement::new(
                                document,
                                self.summary_body_width(),
                                None,
                            )),
                    )
                })
                .when(self.thread.is_none(), |d| {
                    d.when_some(self.thread_error.clone(), |d, text| d.child(error(text)))
                })
                .into_any_element()
        } else if ix + 1 == count {
            div()
                .pb(px(20.))
                .child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .border_t_1()
                        .border_color(content(0.07))
                        .pt(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .when_some(self.reply.clone(), |d, (_, author)| {
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(12.))
                                    .text_color(content(0.5))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .child(format!("Replying to {author}")),
                                    )
                                    .child(
                                        icon_button("inbox-cancel-reply", "Cancel reply", "close")
                                            .size(px(20.))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.reply = None;
                                                this.post_error = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(content(0.1))
                                .bg(content(0.05))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .px(px(12.))
                                        .py(px(8.))
                                        .when(self.posting, |d| d.opacity(0.4))
                                        .child(self.comment.clone()),
                                )
                                .child(
                                    div().px(px(8.)).pb(px(8.)).flex().justify_end().child(
                                        action(
                                            "inbox-post-comment",
                                            if self.posting {
                                                "Posting..."
                                            } else if self.reply.is_some() {
                                                "Reply"
                                            } else {
                                                "Comment"
                                            },
                                        )
                                        .px(px(12.))
                                        .bg(content(1.))
                                        .text_color(shell_fill())
                                        .when(!can_post, |d| d.opacity(0.4))
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.post_comment(cx)),
                                        ),
                                    ),
                                ),
                        )
                        .when_some(self.post_error.clone(), |d, text| d.child(error(text))),
                )
                .into_any_element()
        } else if discussion && ix == 1 {
            div()
                .pb(px(if self.activity.is_empty() { 20. } else { 12. }))
                .children(self.discussion_heading(item, window, cx))
                .into_any_element()
        } else {
            div()
                .pb(px(if ix + 2 == count { 20. } else { 0. }))
                .child(self.discussion_row(item, ix - 2, window, cx))
                .into_any_element()
        };
        div()
            .id(("inbox-summary-row", ix))
            .w_full()
            .px(px(12.))
            .child(body)
            .into_any_element()
    }

    fn diff_view(&self) -> AnyElement {
        div()
            .min_h_0()
            .flex_1()
            .flex()
            .flex_col()
            .when(self.extra_loading, |d| d.child(message("Loading changes…")))
            .when_some(self.extra_error.clone(), |d, text| d.child(error(text)))
            .when(self.diff.is_some(), |d| {
                d.child(div().flex_1().min_h_0().child(self.diff_view.clone()))
            })
            .when(self.diff.as_ref().is_some_and(|diff| diff.truncated), |d| {
                d.child(message(
                    "This diff is truncated. Open the pull request to see all changes.",
                ))
            })
            .into_any_element()
    }

    fn checks_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div().id("inbox-checks-scroll").min_h_0().flex_1().overflow_y_scroll().px(px(12.)).py(px(20.))
            .when(self.extra_loading, |d| d.child(message("Loading checks…")))
            .when_some(self.extra_error.clone(), |d, text| d.child(error(text)))
            .when_some(self.checks.clone(), |d, checks| {
                let failed = checks.checks.iter().filter(|c| matches!(c.state.to_lowercase().as_str(), "failure" | "failed" | "error" | "timed_out" | "cancelled" | "action_required")).cloned().collect::<Vec<_>>();
                d.when(checks.checks.is_empty(), |d| d.child(message("No checks reported")))
                    .when(!failed.is_empty(), |d| d.child(div().mb(px(12.)).flex().items_center().gap(px(8.))
                        .child(div().flex_1().text_size(px(12.)).text_color(content(0.7)).child(format!("{} failing checks", failed.len())))
                        .child(action("inbox-fix-checks", "Fix with agent").bg(content(0.1)).on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(mut item) = this.item() {
                                if item.project_path.is_empty() { item.project_path = this.start_project.to_string_lossy().into_owned(); }
                                let evidence = failed.iter().map(|check| format!("{}: {} {}", check.name, check.state, check.url.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("\n");
                                let prompt = format!("Investigate and repair the failing CI checks for {} {}: {}\n{}\n\n{}\n\nRead the check logs, reproduce the failures, implement the fix, and run relevant checks.", item.repo, item.reference(), item.title, item.url, evidence);
                                cx.emit(Event::StartSession { item: Box::new(item), prompt });
                            }
                        })))))
                    .children(checks.checks.iter().enumerate().map(|(ix, check)| {
                        let state = check.state.to_lowercase();
                        let color: Hsla = if matches!(state.as_str(), "success" | "neutral" | "skipped") { rgb(0x4ade80).into() }
                            else if matches!(state.as_str(), "failure" | "failed" | "error" | "timed_out" | "cancelled" | "action_required") { rgb(0xf87171).into() }
                            else { rgb(0xfbbf24).into() };
                        let url = check.url.clone();
                        div().id(("inbox-check", ix)).py(px(12.)).border_b_1().border_color(content(0.07)).flex().items_center().gap(px(10.))
                            .child(icon(if state == "success" { "check-circle" } else { "circle-dot" }, 15.).text_color(color))
                            .child(div().flex_1().min_w_0().child(div().text_size(px(12.)).text_color(content(0.85)).child(check.name.clone()))
                                .child(div().text_size(px(11.)).text_color(content(0.4)).child(check.workflow.clone())))
                            .child(div().text_size(px(11.)).text_color(color).child(check.state.replace('_', " ").to_lowercase()))
                            .when(url.is_some(), |d| d.child(action(format!("inbox-check-link-{ix}"), "Details ↗").on_click(move |_, _, cx| { if let Some(url) = &url { cx.open_url(url); } })))
                    }))
            }).into_any_element()
    }

    fn filters_active(&self) -> bool {
        let f = &self.state.filters;
        f.assigned_to_me
            || !f.hidden_projects.is_empty()
            || !f.hidden_linear_projects.is_empty()
            || !f.hidden_kinds.is_empty()
            || f.time != "all"
            || f.status.open
            || f.status.draft
            || f.status.closed
            || f.status.merged
            || (self.state.source == "linear" && !self.state.hidden_linear_teams.is_empty())
            || (self.state.source == "jira" && !self.state.hidden_jira_projects.is_empty())
    }
    fn filters_changed(&mut self, refetch: bool, cx: &mut Context<Self>) {
        self.save();
        if let Some(item) = self.visible_items(cx).first() {
            self.select(item.key(), cx);
        } else {
            self.selected = None;
        }
        if refetch {
            self.refresh(cx);
        }
        cx.notify();
    }
    fn filters_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let f = &self.state.filters;
        let tracker = model::is_tracker(&self.state.source);
        let mut rows = vec![
            caption("FILTER INBOX").into_any_element(),
            menu_row(
                "inbox-assigned",
                if matches!(self.state.source.as_str(), "gitlab" | "azuredevops") {
                    "Needs my attention"
                } else {
                    "Assigned to me"
                },
                f.assigned_to_me,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.filters.assigned_to_me = !this.state.filters.assigned_to_me;
                this.filters_changed(true, cx);
            }))
            .into_any_element(),
            caption("UPDATED").into_any_element(),
        ];
        for (time, label) in [
            ("all", "Any time"),
            ("today", "Today"),
            ("7d", "Last 7 days"),
            ("30d", "Last 30 days"),
        ] {
            rows.push(
                menu_row(format!("inbox-time-{time}"), label, f.time == time)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.filters.time = time.into();
                        this.filters_changed(false, cx);
                    }))
                    .into_any_element(),
            );
        }
        rows.push(caption("STATUS").into_any_element());
        for status in ["Open", "Draft", "Closed", "Merged"]
            .into_iter()
            .filter(|s| !tracker || matches!(*s, "Open" | "Closed"))
        {
            let checked = match status {
                "Open" => f.status.open,
                "Draft" => f.status.draft,
                "Closed" => f.status.closed,
                _ => f.status.merged,
            };
            rows.push(
                menu_row(format!("inbox-status-{status}"), status, checked)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let s = &mut this.state.filters.status;
                        match status {
                            "Open" => s.open = !s.open,
                            "Draft" => s.draft = !s.draft,
                            "Closed" => s.closed = !s.closed,
                            _ => s.merged = !s.merged,
                        }
                        this.filters_changed(true, cx);
                    }))
                    .into_any_element(),
            );
        }
        if !tracker {
            rows.push(caption("TYPE").into_any_element());
            for (kind, label) in [
                ("issue", "Issues"),
                (
                    "pr",
                    if self.state.source == "gitlab" {
                        "Merge requests"
                    } else {
                        "Pull requests"
                    },
                ),
            ] {
                rows.push(
                    menu_row(
                        format!("inbox-kind-{kind}"),
                        label,
                        !f.hidden_kinds.iter().any(|k| k == kind),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(&mut this.state.filters.hidden_kinds, kind.into());
                        this.filters_changed(false, cx);
                    }))
                    .into_any_element(),
                );
            }
            rows.push(caption("PROJECTS").into_any_element());
            for (ix, project) in self.projects.iter().enumerate() {
                let key = project.to_string_lossy().into_owned();
                rows.push(
                    menu_row(
                        format!("inbox-project-filter-{ix}"),
                        self.project_label(&key),
                        !f.hidden_projects.contains(&key),
                    )
                    .child(self.project_mark(&key, 12.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(&mut this.state.filters.hidden_projects, key.clone());
                        this.filters_changed(false, cx);
                    }))
                    .into_any_element(),
                );
            }
        }
        let provider_options = if self.state.source == "linear" {
            &self.snapshot.linear_teams
        } else {
            &self.snapshot.jira_projects
        };
        if tracker && !provider_options.is_empty() {
            rows.push(
                caption(if self.state.source == "linear" {
                    "TEAMS"
                } else {
                    "PROJECTS"
                })
                .into_any_element(),
            );
            for (ix, option) in provider_options.iter().enumerate() {
                let id = option.id.clone();
                let hidden = if self.state.source == "linear" {
                    &self.state.hidden_linear_teams
                } else {
                    &self.state.hidden_jira_projects
                };
                rows.push(
                    menu_row(
                        format!("inbox-tracker-filter-{ix}"),
                        option.name.clone(),
                        !hidden.contains(&id),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.state.source == "linear" {
                            toggle(&mut this.state.hidden_linear_teams, id.clone());
                        } else {
                            toggle(&mut this.state.hidden_jira_projects, id.clone());
                        }
                        this.filters_changed(true, cx);
                    }))
                    .into_any_element(),
                );
            }
        }
        if self.state.source == "linear" {
            let mut projects = std::collections::BTreeMap::new();
            for item in self
                .snapshot
                .items
                .iter()
                .filter(|i| i.provider == "linear")
            {
                projects
                    .entry(if item.project_id.is_empty() {
                        "~none".into()
                    } else {
                        item.project_id.clone()
                    })
                    .or_insert_with(|| {
                        if item.project_name.is_empty() {
                            "No project".into()
                        } else {
                            item.project_name.clone()
                        }
                    });
            }
            rows.push(caption("LINEAR PROJECTS").into_any_element());
            for (ix, (id, name)) in projects.into_iter().enumerate() {
                rows.push(
                    menu_row(
                        format!("inbox-linear-project-{ix}"),
                        name,
                        !f.hidden_linear_projects.contains(&id),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(&mut this.state.filters.hidden_linear_projects, id.clone());
                        this.filters_changed(false, cx);
                    }))
                    .into_any_element(),
                );
            }
        }
        rows.push(
            action("inbox-clear-filters", "Clear filters")
                .w_full()
                .justify_start()
                .mt(px(8.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.state.filters = model::Filters::default();
                    this.state.hidden_linear_teams.clear();
                    this.state.hidden_jira_projects.clear();
                    this.filters_changed(true, cx);
                }))
                .into_any_element(),
        );
        popover(
            "inbox-filter-menu",
            Anchor::Slot(self.filter_anchor.clone()),
        )
        .align(Align::End)
        .width(252.)
        .max_height(620.)
        .padding(4.)
        .on_dismiss(move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.filters_open = false;
                cx.notify();
            });
        })
        .child(
            div()
                .id("inbox-filter-scroll")
                .overflow_y_scroll()
                .children(rows),
        )
        .into_any_element()
    }

    fn connection_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        popover(
            "inbox-connections-menu",
            Anchor::Slot(self.connection_anchor.clone()),
        )
        .width(272.)
        .padding(4.)
        .on_dismiss(move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.connections_open = false;
                cx.notify();
            });
        })
        .child(caption("CONNECTIONS"))
        .children(model::PROVIDERS.into_iter().map(|provider| {
            let connected = self
                .snapshot
                .connections
                .get(provider)
                .is_some_and(|s| s.connected);
            let label = format!(
                "{} {}",
                if connected { "Manage" } else { "Connect" },
                model::provider_label(provider)
            );
            div()
                .id(SharedString::from(format!("inbox-connect-{provider}")))
                .role(Role::Button)
                .aria_label(label.clone())
                .w_full()
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(6.))
                .text_size(px(12.))
                .text_color(content(0.7))
                .cursor_pointer()
                .hover(|s| s.bg(content(0.07)).text_color(content(1.)))
                .child(provider_mark(provider))
                .child(label)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.open_connection(provider.into(), cx)),
                )
        }))
        .into_any_element()
    }

    fn connection_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let provider = self.connect_provider.clone().unwrap_or_default();
        let connected = self
            .snapshot
            .connections
            .get(&provider)
            .is_some_and(|s| s.connected);
        let title = format!(
            "{} {}",
            if connected { "Manage" } else { "Connect" },
            model::provider_label(&provider)
        );
        let hint = match provider.as_str() {
            "github" => {
                "Install GitHub CLI, then run gh auth login in a terminal. Check again when signed in."
            }
            "linear" => "Create a personal API key in Linear → Settings → Security & Access.",
            "jira" => "Connect your Jira Cloud site with your Atlassian email and an API token.",
            "gitlab" => {
                "Connect GitLab.com or your GitLab instance with a personal access token with API access."
            }
            _ => {
                "Connect your ADO organization using a personal access token with Boards and Repos read & write access."
            }
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(crate::modal::scrim("inbox-connect-scrim"))
            .child(crate::modal::enter(
                "inbox-connect-card",
                div()
                    .relative()
                    .w(px(440.))
                    .max_w_full()
                    .p(px(24.))
                    .rounded(px(crate::modal::RADIUS))
                    .border_1()
                    .border_color(content(crate::modal::BORDER))
                    .child(crate::modal::surface(crate::modal::RADIUS - 1.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(provider_mark(&provider))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(content(1.))
                                    .child(title),
                            )
                            .child(
                                icon_button("inbox-close-connect", "Close connection", "close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.connecting {
                                            this.connect_provider = None;
                                            this.connect_token.update(cx, |input, cx| {
                                                input.set_text(String::new(), cx)
                                            });
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(19.))
                            .text_color(content(0.5))
                            .child(hint),
                    )
                    .when(provider != "github" && provider != "linear", |d| {
                        d.child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(content(0.1))
                                .child(self.connect_url.clone()),
                        )
                    })
                    .when(provider == "jira", |d| {
                        d.child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(content(0.1))
                                .child(self.connect_email.clone()),
                        )
                    })
                    .when(provider != "github", |d| {
                        d.child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(content(0.1))
                                .child(self.connect_token.clone()),
                        )
                    })
                    .when_some(self.connect_error.clone(), |d, text| d.child(error(text)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(connected && provider != "github", |d| {
                                d.child(
                                    action("inbox-disconnect", "Disconnect")
                                        .text_color(rgb(0xf87171))
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.connect(true, cx)),
                                        ),
                                )
                            })
                            .when(provider == "github", |d| {
                                d.child(
                                    action("inbox-gh-guide", "Installation guide").on_click(
                                        |_, _, cx| cx.open_url("https://cli.github.com/"),
                                    ),
                                )
                            })
                            .child(div().flex_1())
                            .child(
                                crate::modal::disabled(
                                    crate::modal::primary_button(
                                        "inbox-connect-submit",
                                        if self.connecting {
                                            "Connecting…"
                                        } else if provider == "github" {
                                            "Check again"
                                        } else {
                                            "Connect"
                                        },
                                    ),
                                    self.connecting,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.connect(false, cx))),
                            ),
                    ),
            ))
            .into_any_element()
    }

    fn project_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        popover(
            "inbox-project-menu",
            Anchor::Slot(self.project_anchor.clone()),
        )
        .width(240.)
        .padding(4.)
        .on_dismiss(move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.project_picker = false;
                cx.notify();
            });
        })
        .children(self.projects.iter().enumerate().map(|(ix, project)| {
            let path = project.clone();
            menu_row(
                format!("inbox-start-project-{ix}"),
                self.project_label(&project.to_string_lossy()),
                *project == self.start_project,
            )
            .child(self.project_mark(&project.to_string_lossy(), 14.))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.start_project = path.clone();
                this.project_picker = false;
                cx.notify();
            }))
        }))
        .into_any_element()
    }

    fn action_menu_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        popover(
            "inbox-action-menu",
            Anchor::Slot(self.action_anchor.clone()),
        )
        .width(272.)
        .padding(4.)
        .on_dismiss(move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.action_menu = false;
                cx.notify();
            });
        })
        .children(
            [
                (
                    "merge",
                    "Create a merge commit",
                    "Add every commit to the base branch.",
                ),
                (
                    "squash",
                    "Squash and merge",
                    "Combine the commits into one.",
                ),
                (
                    "rebase",
                    "Rebase and merge",
                    "Add the commits without a merge commit.",
                ),
            ]
            .into_iter()
            .map(|(name, label, description)| {
                div()
                    .id(format!("inbox-merge-{name}"))
                    .role(Role::MenuItemRadio)
                    .aria_label(label)
                    .w_full()
                    .px(px(8.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.08)))
                    .child(div().mt(px(4.)).size(px(6.)).rounded_full().bg(
                        if self.merge_action == name {
                            accent_color()
                        } else {
                            content(0.2)
                        },
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(content(0.9))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(11.))
                                    .line_height(px(15.))
                                    .text_color(content(0.45))
                                    .child(description),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.merge_action = name.into();
                        this.action_menu = false;
                        cx.notify();
                    }))
            }),
        )
        .into_any_element()
    }
    fn action_confirmation(&self, cx: &mut Context<Self>) -> AnyElement {
        let action_name = self.confirm_action.clone().unwrap_or_default();
        let details = self.details.clone().unwrap_or_default();
        let source = if details.head_ref_name.is_empty() {
            "this branch".into()
        } else {
            format!("“{}”", details.head_ref_name)
        };
        let destination = if details.base_ref_name.is_empty() {
            "the base branch".into()
        } else {
            format!("“{}”", details.base_ref_name)
        };
        let (title, detail, label, progress) = match action_name.as_str() {
            "merge" => (
                "Merge this pull request?",
                format!(
                    "Every commit from {source} will be added to {destination} with a merge commit."
                ),
                "Merge pull request",
                "Merging…",
            ),
            "squash" => (
                "Squash and merge?",
                format!(
                    "The commits from {source} will be combined into one commit on {destination}."
                ),
                "Squash and merge",
                "Merging…",
            ),
            "rebase" => (
                "Rebase and merge?",
                format!(
                    "The commits from {source} will be rebased individually onto {destination}."
                ),
                "Rebase and merge",
                "Merging…",
            ),
            "close" => (
                "Close this pull request?",
                "The pull request will close without merging. You can reopen it later.".into(),
                "Close pull request",
                "Closing…",
            ),
            "reopen" => (
                "Reopen this pull request?",
                "The pull request will return to the open state.".into(),
                "Reopen pull request",
                "Reopening…",
            ),
            "ready" => (
                "Mark as ready for review?",
                "Reviewers will see that this pull request is ready for feedback.".into(),
                "Ready for review",
                "Updating…",
            ),
            _ => (
                "Convert to draft?",
                "Reviewers will see that this pull request is not ready to merge.".into(),
                "Convert to draft",
                "Converting…",
            ),
        };
        let anchor = match action_name.as_str() {
            "ready" => self.pr_action_anchors[0].clone(),
            "draft" => self.pr_action_anchors[1].clone(),
            "close" => self.pr_action_anchors[2].clone(),
            "reopen" => self.pr_action_anchors[3].clone(),
            _ => self.action_anchor.clone(),
        };
        let weak = cx.entity().downgrade();
        popover("inbox-pr-confirmation", Anchor::Slot(anchor))
            .width(320.)
            .gap(5.)
            .padding(12.)
            .on_dismiss(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if !this.acting {
                        this.confirm_action = None;
                        this.action_error = None;
                        cx.notify();
                    }
                });
            })
            .child(
                div()
                    .id("inbox-pr-confirmation-dialog")
                    .role(Role::Dialog)
                    .aria_label(title)
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(1.))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(content(0.55))
                            .child(detail),
                    ),
            )
            .when_some(self.action_error.clone(), |d, text| {
                d.child(
                    div()
                        .mt(px(8.))
                        .text_size(px(11.))
                        .text_color(rgb(0xf87171))
                        .child(text),
                )
            })
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        action("inbox-action-cancel", "Cancel")
                            .when(self.acting, |d| d.opacity(0.4))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.acting {
                                    this.confirm_action = None;
                                    this.action_error = None;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        action(
                            "inbox-action-confirm",
                            if self.acting { progress } else { label },
                        )
                        .font_weight(FontWeight::MEDIUM)
                        .bg(if action_name == "close" {
                            Hsla::from(rgb(0xf43f5e)).opacity(0.2)
                        } else if matches!(action_name.as_str(), "merge" | "squash" | "rebase") {
                            Hsla::from(rgb(0x10b981)).opacity(0.2)
                        } else {
                            content(1.)
                        })
                        .text_color(if action_name == "close" {
                            rgb(0xfda4af).into()
                        } else if matches!(action_name.as_str(), "merge" | "squash" | "rebase") {
                            rgb(0x6ee7b7).into()
                        } else {
                            shell_fill()
                        })
                        .when(self.acting, |d| d.opacity(0.6))
                        .on_click(cx.listener(|this, _, _, cx| this.run_action(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl gpui::Render for Inbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_comment {
            self.focus_comment = false;
            window.focus(&self.comment.focus_handle(cx), cx);
        }
        let list = (!self.detail_only).then(|| self.list_view(cx));
        let detail = match self.link_notice_view(cx) {
            Some(notice) if self.detail_only && self.item().is_none() => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .px(px(16.))
                .child(notice)
                .into_any_element(),
            _ => self.detail_view(window, cx),
        };
        div()
            .id("inbox-view")
            .role(Role::Group)
            .aria_label("Inbox")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    if this.connect_provider.is_some() && !this.connecting {
                        this.connect_provider = None;
                    } else if this.confirm_action.is_some() && !this.acting {
                        this.confirm_action = None;
                    } else if this.filters_open
                        || this.connections_open
                        || this.project_picker
                        || this.action_menu
                    {
                        this.filters_open = false;
                        this.connections_open = false;
                        this.project_picker = false;
                        this.action_menu = false;
                    } else {
                        cx.emit(Event::Close);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some((start, width)) = this.resize_start {
                        let max = (f32::from(window.viewport_size().width) * 0.5).clamp(240., 420.);
                        this.state.list_width =
                            (width + f32::from(event.position.x) - start).clamp(240., max);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resize_start.take().is_some() {
                        this.save();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resize_start.take().is_some() {
                        this.save();
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .children(list)
                    .child(div().flex_1().min_w_0().min_h_0().child(detail)),
            )
            .when_some(self.error.clone(), |d, text| d.child(error(text)))
            .when(self.filters_open, |d| d.child(self.filters_menu(cx)))
            .when(self.connections_open, |d| d.child(self.connection_menu(cx)))
            .when(self.project_picker, |d| d.child(self.project_menu(cx)))
            .when(self.action_menu, |d| d.child(self.action_menu_view(cx)))
            .when(self.connect_provider.is_some(), |d| {
                d.child(crate::modal::window_overlay(
                    window,
                    self.connection_dialog(cx),
                ))
            })
            .when(self.confirm_action.is_some(), |d| {
                d.child(self.action_confirmation(cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        AssetSource, Bounds, Render, TestAppContext, WindowBounds, WindowHandle, WindowOptions,
        point, size,
    };
    use std::sync::{Arc, Mutex};

    struct Fixture {
        inbox: Entity<Inbox>,
        item: Item,
    }
    impl Render for Fixture {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.inbox.update(cx, |inbox, cx| {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(inbox.pr_actions(&self.item, cx))
                    .child(inbox.summary_view(&self.item, window, cx))
            })
        }
    }
    fn fixture(cx: &mut TestAppContext) -> WindowHandle<Fixture> {
        let window = cx.update(|cx| cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(1100.), px(700.))))),
            ..Default::default()
        }, |_, cx| cx.new(|cx| {
            let item = Item { provider: "github".into(), kind: "pr".into(), state: "open".into(), number: 7, ..Default::default() };
            let inbox = cx.new(|cx| {
                let store = Arc::new(Mutex::new(monocode_core::store::Store::open(std::path::Path::new(":memory:")).unwrap()));
                let mut view = Inbox::new_inner(store, vec![], PathBuf::from("/private/tmp/inbox-regression"), false, cx);
                view.snapshot.items = vec![item.clone()]; view.selected = Some(item.key());
                view.details = Some(Details { body: "Description".into(), ..Default::default() });
                let comments = (0..100).map(|ix| model::Comment {
                    id: format!("comment-{ix}"), body: (0..30).map(|line| format!("### Finding {line}\n\nReview **retry handling** in `src/main.rs` before returning.\n\n")).collect(),
                    created_at: format!("2026-10-22T08:{:02}:00Z", ix % 60), ..Default::default()
                }).collect::<Vec<_>>();
                view.comments = comments.iter().map(|comment| (comment.clone(), cx.new(|cx| document::InboxDocument::new(comment.body.clone(), PathBuf::from("/repo"), cx)))).collect();
                view.thread = Some(Thread { comments, ..Default::default() });
                view.rebuild_activity(); view
            });
            Fixture { inbox, item }
        })).unwrap());
        draw(window, cx);
        window
    }
    fn draw(window: WindowHandle<Fixture>, cx: &mut TestAppContext) {
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
    }
    #[gpui::test]
    fn the_inbox_opens_scoped_to_its_project_and_labels_projects_by_appearance(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, _| {
                    let home = PathBuf::from("/work/home");
                    let other = PathBuf::from("/work/other");
                    assert_eq!(
                        super::super::other_projects(&[home.clone(), other.clone()], &home),
                        vec!["/work/other".to_string()]
                    );
                    assert_eq!(inbox.project_label("/work/other"), "other");
                    inbox.appearances.insert(
                        "/work/other".into(),
                        monocode_core::project_appearance::ProjectAppearance {
                            name: "Other app".into(),
                            ..Default::default()
                        },
                    );
                    assert_eq!(inbox.project_label("/work/other"), "Other app");
                });
            })
            .unwrap();
    }
    #[gpui::test]
    fn switching_projects_drops_the_previous_projects_selection_and_detail(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let (a, b) = (PathBuf::from("/work/a"), PathBuf::from("/work/b"));
                    let item = |number, project: &PathBuf| Item {
                        provider: "github".into(),
                        kind: "pr".into(),
                        state: "open".into(),
                        repo: format!("acme/app-{number}"),
                        number,
                        project_path: project.to_string_lossy().into_owned(),
                        ..Default::default()
                    };
                    // Keep the list fetch queued instead of reaching real providers.
                    inbox.loading = true;
                    inbox.snapshot.items = vec![item(1, &a), item(2, &b)];
                    inbox.set_context(vec![a.clone(), b.clone()], a.clone(), cx);
                    inbox.selected = Some(item(1, &a).key());
                    inbox.details = Some(Details {
                        body: "From project A".into(),
                        ..Default::default()
                    });
                    inbox.detail_loading = true;
                    let request = inbox.detail_generation;
                    let scope = inbox.fetch_scope();

                    inbox.set_context(vec![a.clone(), b.clone()], b.clone(), cx);

                    assert_eq!(inbox.selected, None);
                    assert!(inbox.details.is_none() && inbox.thread.is_none());
                    assert!(inbox.document.is_none() && inbox.comments.is_empty());
                    assert!(!inbox.detail_loading);
                    assert_ne!(
                        inbox.detail_generation, request,
                        "project A's detail request must be discarded"
                    );
                    assert_ne!(inbox.fetch_scope(), scope, "a fetch made for A is stale");
                    assert_eq!(
                        inbox
                            .visible_items(cx)
                            .iter()
                            .map(Item::key)
                            .collect::<Vec<_>>(),
                        vec![item(2, &b).key()]
                    );
                });
            })
            .unwrap();
    }
    #[gpui::test]
    fn long_discussion_materializes_only_viewport_comments_and_reply_focus_reaches_composer(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let measured = inbox
                        .comments
                        .iter()
                        .filter(|(_, doc)| doc.read(cx).measured_rows() > 0)
                        .count();
                    assert!(
                        measured > 0 && measured < 10,
                        "only viewport/overscan cards should be laid out, got {measured}"
                    );
                    assert_eq!(inbox.summary_scroll.item_count(), 103);
                    inbox.summary_scroll.scroll_by(px(2500.));
                })
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |fixture, window, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    assert!(inbox.summary_scroll.logical_scroll_top().item_ix > 3);
                    inbox.reply = Some(("thread-1".into(), "Reviewer".into()));
                    inbox.summary_scroll.remeasure_items(102..103);
                    inbox.summary_scroll.scroll_to_end();
                    window.focus(&inbox.comment.focus_handle(cx), cx);
                })
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |fixture, window, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    assert!(inbox.summary_scroll.logical_scroll_top().item_ix > 90);
                    assert!(inbox.comment.focus_handle(cx).is_focused(window));
                    inbox
                        .comment
                        .update(cx, |input, cx| input.set_text("A reply\n".repeat(8), cx));
                })
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |fixture, window, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    assert!(inbox.comment.focus_handle(cx).is_focused(window));
                    assert_eq!(inbox.summary_scroll.item_count(), 103);
                })
            })
            .unwrap();
    }
    #[gpui::test]
    fn expanding_comment_invalidates_its_height_and_preserves_timeline_order(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx);
        let (id, previous) = window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let Activity::Comment(comment) = &inbox.activity[0] else {
                        panic!()
                    };
                    let id = comment.id.clone();
                    let doc = &inbox
                        .comments
                        .iter()
                        .find(|(comment, _)| comment.id == id)
                        .unwrap()
                        .1;
                    let previous = doc.read(cx).measured_rows();
                    inbox.expanded_comments.insert(id.clone());
                    inbox.summary_scroll.remeasure();
                    (id, previous)
                })
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let doc = &inbox
                        .comments
                        .iter()
                        .find(|(comment, _)| comment.id == id)
                        .unwrap()
                        .1;
                    assert!(doc.read(cx).measured_rows() > previous);
                    let Activity::Comment(first) = &inbox.activity[0] else {
                        panic!()
                    };
                    assert_eq!(first.id, id);
                    assert_eq!(inbox.summary_scroll.logical_scroll_top().item_ix, 0);
                })
            })
            .unwrap();
    }
    #[gpui::test]
    fn linear_team_filters_hide_cached_issues_before_refresh(cx: &mut TestAppContext) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    inbox.state.source = "linear".into();
                    inbox.state.filters = model::Filters::default();
                    inbox.snapshot.items = ["web", "mobile"]
                        .into_iter()
                        .map(|team| Item {
                            provider: "linear".into(),
                            identifier: format!("{team}-1"),
                            team_id: team.into(),
                            ..Default::default()
                        })
                        .collect();
                    let visible_teams = |inbox: &Inbox, cx: &App| {
                        inbox
                            .visible_items(cx)
                            .into_iter()
                            .map(|item| item.team_id)
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(visible_teams(inbox, cx), ["web", "mobile"]);
                    toggle(&mut inbox.state.hidden_linear_teams, "web".into());
                    assert_eq!(visible_teams(inbox, cx), ["mobile"]);
                    let mobile = inbox.visible_items(cx).remove(0);
                    inbox.selected = Some(mobile.key());
                    inbox.filters_changed(false, cx);
                    assert_eq!(inbox.item().map(|item| item.team_id), Some("mobile".into()));
                    toggle(&mut inbox.state.hidden_linear_teams, "mobile".into());
                    assert!(inbox.visible_items(cx).is_empty());
                    inbox.filters_changed(false, cx);
                    assert!(inbox.item().is_none());
                    toggle(&mut inbox.state.hidden_linear_teams, "web".into());
                    assert_eq!(visible_teams(inbox, cx), ["web"]);
                    inbox.state.hidden_linear_teams.clear();
                    assert_eq!(visible_teams(inbox, cx), ["web", "mobile"]);
                });
            })
            .unwrap();
    }
    #[gpui::test]
    fn arriving_comment_preserves_the_readers_item_and_offset(cx: &mut TestAppContext) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture
                    .inbox
                    .update(cx, |inbox, _| inbox.summary_scroll.scroll_by(px(2500.)))
            })
            .unwrap();
        draw(window, cx);
        let before = window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let before = inbox.summary_scroll.logical_scroll_top();
                    let comment = model::Comment {
                        id: "new-comment".into(),
                        body: "A new reply".into(),
                        created_at: "2026-10-22T09:00:00Z".into(),
                        ..Default::default()
                    };
                    let doc = cx.new(|cx| {
                        document::InboxDocument::new(
                            comment.body.clone(),
                            PathBuf::from("/repo"),
                            cx,
                        )
                    });
                    inbox.comments.push((comment.clone(), doc));
                    inbox.thread.as_mut().unwrap().comments.push(comment);
                    inbox.rebuild_activity();
                    before
                })
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, _| {
                    let after = inbox.summary_scroll.logical_scroll_top();
                    assert_eq!(after.item_ix, before.item_ix);
                    assert!((after.offset_in_item - before.offset_in_item).abs() < px(1.));
                    assert_eq!(inbox.summary_scroll.item_count(), 104);
                })
            })
            .unwrap();
    }

    /// Renders the real inbox offscreen with Metal and writes PNGs, for looking
    /// at the link navigation states. Run explicitly:
    /// `cargo test inbox_link_screenshots -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Requires a Metal device; run explicitly on macOS"]
    fn inbox_link_screenshots() {
        use gpui::HeadlessAppContext;
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(crate::assets::Assets),
            || {
                Ok(Some(Box::new(
                    gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
                )))
            },
        );
        let pr = |number: i64, title: &str, state: &str| Item {
            provider: "github".into(),
            kind: "pr".into(),
            repo: "acme/app".into(),
            number,
            title: title.into(),
            state: state.into(),
            url: format!("https://github.com/acme/app/pull/{number}"),
            updated_at: "2026-10-25T09:00:00Z".into(),
            ..Default::default()
        };
        let (first, second, outside) = (
            pr(12, "Add retry handling to the sync client", "open"),
            pr(9, "Fix flaky token refresh", "open"),
            pr(41, "Move the retry policy into a shared crate", "open"),
        );
        let body = "Retries failed syncs with backoff.\n\nDepends on https://github.com/acme/app/pull/9 and the follow-up in https://github.com/acme/app/pull/41.\n\nSee also [the design note](https://example.com/design).";
        let window = cx
            .open_window(size(px(1100.), px(700.)), |_, cx| {
                cx.new(|cx| {
                    let store = Arc::new(Mutex::new(
                        monocode_core::store::Store::open(std::path::Path::new(":memory:"))
                            .unwrap(),
                    ));
                    let mut view = Inbox::new_inner(
                        store,
                        vec![],
                        PathBuf::from("/private/tmp/inbox-screens"),
                        false,
                        cx,
                    );
                    view.snapshot.items = vec![first.clone(), second.clone()];
                    view.snapshot.connections.insert(
                        "github".into(),
                        model::Status {
                            connected: true,
                            ..Default::default()
                        },
                    );
                    view.state.source = "github".into();
                    view.selected = Some(first.key());
                    view
                })
            })
            .unwrap();
        let shoot = |cx: &mut HeadlessAppContext, name: &str| {
            let _ = cx.capture_screenshot(window.into());
            let image = cx.capture_screenshot(window.into()).unwrap();
            image.save(format!("/tmp/inbox-link-{name}.png")).unwrap();
        };
        // The detail a real fetch would fill in, without running `gh`.
        let show = |cx: &mut HeadlessAppContext, body: &str| {
            let body = body.to_string();
            window
                .update(cx, move |view, _, cx| {
                    let weak = cx.weak_entity();
                    view.details = Some(Details {
                        body: body.clone(),
                        author: "mira".into(),
                        base_ref_name: "main".into(),
                        head_ref_name: "retry-handling".into(),
                        ..Default::default()
                    });
                    view.document = Some(cx.new(|cx| {
                        document::InboxDocument::linked(body, PathBuf::from("/repo"), weak, cx)
                    }));
                    view.detail_loading = false;
                    cx.notify();
                })
                .unwrap();
        };
        show(&mut cx, body);
        shoot(&mut cx, "1-start");
        window
            .update(&mut cx, |view, _, cx| {
                assert!(view.open_url("https://github.com/acme/app/pull/9", cx));
            })
            .unwrap();
        show(&mut cx, "Refreshes the token before it expires.");
        shoot(&mut cx, "2-listed-link-with-back");
        window
            .update(&mut cx, |view, _, cx| {
                view.go_to(outside.clone(), Tab::Summary, cx)
            })
            .unwrap();
        show(&mut cx, "Moves the retry policy into its own crate.");
        shoot(&mut cx, "3-unlisted-pinned-row");
        window
            .update(&mut cx, |view, _, cx| {
                view.link_notice = Some(LinkNotice::Failed {
                    url: "https://github.com/acme/private/pull/3".into(),
                    message:
                        "GraphQL: Could not resolve to a Repository with the name 'acme/private'."
                            .into(),
                });
                cx.notify();
            })
            .unwrap();
        shoot(&mut cx, "4-failed-notice");
        window
            .update(&mut cx, |view, _, cx| {
                view.link_notice = Some(LinkNotice::Loading("acme/app#77".into()));
                cx.notify();
            })
            .unwrap();
        shoot(&mut cx, "5-loading-notice");
    }

    struct Shell {
        opened: Vec<String>,
        focus: gpui::FocusHandle,
    }
    impl Render for Shell {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.focus)
                .on_action(
                    cx.listener(|this, event: &crate::inbox::OpenWorkLink, _, cx| {
                        this.opened.push(event.0.clone());
                        cx.notify();
                    }),
                )
                .child("shell")
        }
    }

    #[gpui::test]
    fn work_links_reach_the_shell(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|cx| Shell {
                    opened: vec![],
                    focus: cx.focus_handle(),
                })
            })
            .unwrap()
        });
        window
            .update(cx, |shell, window, cx| window.focus(&shell.focus, cx))
            .unwrap();
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let shown = window
            .update(cx, |_, window, cx| {
                crate::inbox::open_in_inbox("https://github.com/acme/app/pull/1", window, cx)
            })
            .unwrap();
        assert!(shown, "the shell's handler makes the action available");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.opened, ["https://github.com/acme/app/pull/1"])
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_window_without_a_shell_leaves_work_links_to_the_browser(cx: &mut TestAppContext) {
        struct Bare;
        impl Render for Bare {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child("bare")
            }
        }
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Bare))
                .unwrap()
        });
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let shown = window
            .update(cx, |_, window, cx| {
                crate::inbox::open_in_inbox("https://github.com/acme/app/pull/1", window, cx)
            })
            .unwrap();
        assert!(!shown);
    }

    #[gpui::test]
    fn following_a_listed_link_selects_it_and_back_returns(cx: &mut TestAppContext) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let item = |number| Item {
                        provider: "github".into(),
                        kind: "pr".into(),
                        repo: "acme/app".into(),
                        number,
                        ..Default::default()
                    };
                    let (first, second) = (item(1), item(2));
                    inbox.snapshot.items = vec![first.clone(), second.clone()];
                    inbox.selected = Some(first.key());
                    assert!(inbox.open_url("https://github.com/Acme/App/pull/2", cx));
                    assert_eq!(inbox.selected, Some(second.key()));
                    assert!(inbox.pinned.is_none(), "listed items are not pinned");
                    assert_eq!(inbox.back.len(), 1);
                    inbox.go_back(cx);
                    assert_eq!(inbox.selected, Some(first.key()));
                    assert!(inbox.back.is_empty());
                    assert!(inbox.open_url("https://github.com/acme/app/pull/2", cx));
                    inbox.select(first.key(), cx);
                    assert!(inbox.back.is_empty(), "a rail pick starts a new trail");
                });
            })
            .unwrap();
    }

    #[gpui::test]
    fn linear_links_open_listed_issues_and_other_urls_stay_with_the_browser(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let issue = Item {
                        provider: "linear".into(),
                        kind: "linear".into(),
                        repo: "ENG".into(),
                        identifier: "ENG-42".into(),
                        number: 42,
                        url: "https://linear.app/acme/issue/ENG-42/fix".into(),
                        ..Default::default()
                    };
                    inbox.snapshot.items = vec![issue.clone()];
                    assert!(inbox.can_open("https://linear.app/acme/issue/eng-42"));
                    assert!(!inbox.can_open("https://example.com/page"));
                    assert!(!inbox.open_url("https://example.com/page", cx));
                    assert!(inbox.open_url("https://linear.app/acme/issue/eng-42", cx));
                    assert_eq!(inbox.selected, Some(issue.key()));
                    assert_eq!(inbox.state.source, "linear");
                    assert!(
                        inbox.back.is_empty(),
                        "the fixture's selection was not an item"
                    );
                });
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_followed_item_outside_the_list_leads_the_rail_until_another_row_is_picked(
        cx: &mut TestAppContext,
    ) {
        let window = fixture(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.inbox.update(cx, |inbox, cx| {
                    let listed = Item {
                        provider: "github".into(),
                        kind: "pr".into(),
                        repo: "acme/app".into(),
                        number: 1,
                        ..Default::default()
                    };
                    let linked = Item {
                        number: 9,
                        ..listed.clone()
                    };
                    inbox.snapshot.items = vec![listed.clone()];
                    inbox.selected = Some(listed.key());
                    inbox.go_to(linked.clone(), Tab::Summary, cx);
                    assert_eq!(inbox.item().map(|item| item.number), Some(9));
                    assert_eq!(inbox.pinned.as_ref().map(Item::key), Some(linked.key()));
                    inbox.select(listed.key(), cx);
                    assert!(inbox.pinned.is_none());
                    assert_eq!(inbox.item().map(|item| item.number), Some(1));
                });
            })
            .unwrap();
    }

    fn github_pr(number: i64) -> Item {
        Item {
            provider: "github".into(),
            kind: "pr".into(),
            repo: "acme/app".into(),
            number,
            ..Default::default()
        }
    }

    /// Starts a lookup the test answers later, as a slow `gh` or Linear call.
    fn pending_lookup(
        inbox: &mut Inbox,
        cx: &mut Context<Inbox>,
    ) -> async_channel::Sender<Result<Item, String>> {
        let (send, receive) = async_channel::bounded(1);
        let task = cx.background_executor().spawn(async move {
            receive
                .recv()
                .await
                .unwrap_or_else(|error| Err(error.to_string()))
        });
        inbox.cancel_link();
        inbox.await_link(
            "https://github.com/acme/app/pull/99".into(),
            "acme/app#99".into(),
            Tab::Summary,
            task,
            cx,
        );
        send
    }

    /// The selection after a lookup for #99 arrives late, once `act` has run.
    fn selection_after_a_late_lookup(
        cx: &mut TestAppContext,
        act: impl FnOnce(&mut Inbox, &mut Context<Inbox>),
    ) -> Option<String> {
        let window = fixture(cx);
        let inbox = window.update(cx, |f, _, _| f.inbox.clone()).unwrap();
        let send = inbox.update(cx, |inbox, cx| {
            // Queues refreshes instead of running `gh`.
            inbox.loading = true;
            inbox.snapshot.items = vec![github_pr(1), github_pr(2)];
            inbox.selected = Some(github_pr(1).key());
            assert!(inbox.open_url("https://github.com/acme/app/pull/2", cx));
            let send = pending_lookup(inbox, cx);
            act(inbox, cx);
            send
        });
        send.try_send(Ok(github_pr(99))).unwrap();
        cx.run_until_parked();
        inbox.read_with(cx, |inbox, _| inbox.selected.clone())
    }

    #[gpui::test]
    fn a_late_link_lookup_does_not_replace_a_newer_selection(cx: &mut TestAppContext) {
        let linked = Some(github_pr(99).key());
        assert_eq!(selection_after_a_late_lookup(cx, |_, _| {}), linked);
        let picked =
            selection_after_a_late_lookup(cx, |inbox, cx| inbox.select(github_pr(1).key(), cx));
        assert_eq!(picked, Some(github_pr(1).key()), "a rail pick");
        let reselected =
            selection_after_a_late_lookup(cx, |inbox, cx| inbox.select(github_pr(2).key(), cx));
        assert_eq!(
            reselected,
            Some(github_pr(2).key()),
            "the row already shown"
        );
        let known = selection_after_a_late_lookup(cx, |inbox, cx| {
            assert!(inbox.open_url("https://github.com/acme/app/pull/1", cx));
        });
        assert_eq!(known, Some(github_pr(1).key()), "a listed link");
        let back = selection_after_a_late_lookup(cx, |inbox, cx| inbox.go_back(cx));
        assert_eq!(back, Some(github_pr(1).key()), "back");
        let source =
            selection_after_a_late_lookup(cx, |inbox, cx| inbox.change_source("linear".into(), cx));
        assert_ne!(source, linked, "another source");
        let project = selection_after_a_late_lookup(cx, |inbox, cx| {
            inbox.set_context(vec![], PathBuf::from("/private/tmp/inbox-other"), cx)
        });
        assert_ne!(project, linked, "another project");
    }

    #[gpui::test]
    fn an_automatic_row_pick_leaves_a_loading_link_to_arrive(cx: &mut TestAppContext) {
        let linked = selection_after_a_late_lookup(cx, |inbox, cx| {
            inbox.selected = None;
            inbox.auto_select(github_pr(1).key(), cx);
        });
        assert_eq!(linked, Some(github_pr(99).key()));
    }

    struct Detail {
        inbox: Entity<Inbox>,
    }
    impl Render for Detail {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.inbox
                .update(cx, |inbox, cx| inbox.detail_view(window, cx))
        }
    }

    #[gpui::test]
    fn a_failed_link_lookup_shows_its_notice_on_an_empty_inbox(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                let inbox = cx.new(|cx| {
                    let store = Arc::new(Mutex::new(
                        monocode_core::store::Store::open(std::path::Path::new(":memory:"))
                            .unwrap(),
                    ));
                    Inbox::new_inner(
                        store,
                        vec![],
                        PathBuf::from("/private/tmp/inbox-regression"),
                        false,
                        cx,
                    )
                });
                cx.new(|_| Detail { inbox })
            })
            .unwrap()
        });
        let inbox = window.update(cx, |d, _, _| d.inbox.clone()).unwrap();
        let send = inbox.update(cx, |inbox, cx| {
            assert!(inbox.snapshot.items.is_empty() && inbox.selected.is_none());
            pending_lookup(inbox, cx)
        });
        send.try_send(Err("Could not resolve acme/app#99".into()))
            .unwrap();
        cx.run_until_parked();
        inbox.read_with(cx, |inbox, _| {
            assert!(matches!(inbox.link_notice, Some(LinkNotice::Failed { .. })))
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(
            visual.debug_bounds("inbox-link-browser").is_some(),
            "the browser fallback is drawn"
        );
    }

    #[test]
    fn toolbar_icons_have_explicit_ink_and_the_original_distinct_assets() {
        for (name, ink) in [
            ("git-merge", shell_fill()),
            ("git-pull-request-draft", content(0.7)),
            ("git-pull-request-closed", content(0.7)),
            ("external-link", content(0.7)),
        ] {
            let mut icon = action_icon(name, ink);
            assert_eq!(icon.style().text.color, Some(ink));
            let bytes = crate::assets::Assets.load(name).unwrap().unwrap();
            assert!(std::str::from_utf8(&bytes).unwrap().contains("<svg"));
        }
    }
}
