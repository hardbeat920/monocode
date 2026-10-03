//! Port of `InboxCard` from src/features/inbox/ui/InboxView.tsx: one row of
//! the inbox list.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_layout::paths::project_name;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{InboxServices, ListedItem};
use crate::model::{format_relative_time, inbox_item_ref, item_attention_label, kind_label};
use crate::style::{inbox_status_mark, label_chip, provider_mark, status_ink};

/// The card's accessible name, as the React `aria-label` spelled it.
pub fn card_label(listed: &ListedItem) -> String {
    let item = &listed.item;
    let status = inbox_status_mark(item);
    let attention = item_attention_label(item);
    let related = listed.related_sessions.len();
    format!(
        "{} {} {}: {}{}{}{}",
        status.label,
        kind_label(item).to_lowercase(),
        inbox_item_ref(item),
        item.title,
        if attention.is_empty() {
            String::new()
        } else {
            format!(", {attention}")
        },
        if listed.unseen { ", new" } else { "" },
        if related > 0 {
            format!(
                ", {related} related {}",
                if related == 1 { "thread" } else { "threads" }
            )
        } else {
            String::new()
        }
    )
}

/// The source line of a card: a tracker's team, else the repo or project.
pub fn card_source(listed: &ListedItem) -> String {
    let item = &listed.item;
    if item.is_tracker() {
        item.team_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| item.repo.clone())
    } else if item.repo.is_empty() {
        project_name(&item.project_path)
    } else {
        item.repo.clone()
    }
}

type SelectFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// `InboxCard`.
#[derive(IntoElement)]
pub struct InboxCard {
    id: ElementId,
    listed: ListedItem,
    active: bool,
    now: i64,
    services: Rc<dyn InboxServices>,
    on_select: Option<SelectFn>,
}

pub fn inbox_card(
    id: impl Into<ElementId>,
    listed: ListedItem,
    active: bool,
    services: Rc<dyn InboxServices>,
) -> InboxCard {
    InboxCard {
        id: id.into(),
        now: services.now_ms(),
        listed,
        active,
        services,
        on_select: None,
    }
}

impl InboxCard {
    pub fn on_select(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for InboxCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let listed = &self.listed;
        let item = &listed.item;
        let status = inbox_status_mark(item);
        let status_color = status_ink(status.tone, &theme);
        let attention = item_attention_label(item);
        let time = format_relative_time(&item.updated_at, self.now);
        let related = listed.related_sessions.len();
        let source = card_source(listed);
        let mut kind_line = format!("{} · {}", kind_label(item), inbox_item_ref(item));
        if !attention.is_empty() {
            kind_line.push_str(&format!(" · {attention}"));
        }
        let mut right = div().flex().flex_none().items_center().gap(u(6.));
        if related > 0 {
            right = right.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(2.))
                    .text_px(theme.text.caption)
                    .tabular()
                    .text_color(theme.colors.accent)
                    .child(
                        icon(IconName::MessageMultiple)
                            .size(u(12.))
                            .text_color(theme.colors.accent),
                    )
                    .child(related.to_string()),
            );
        }
        if !time.is_empty() {
            right = right.child(
                div()
                    .text_px(theme.text.caption)
                    .tabular()
                    .text_color(theme.content(0.45))
                    .child(time.clone()),
            );
        }
        if listed.unseen {
            right = right.child(
                div()
                    .flex_none()
                    .size(u(6.))
                    .rounded_full()
                    .bg(theme.colors.accent),
            );
        }
        let show_right = related > 0 || !time.is_empty() || listed.unseen;
        let top = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .child(provider_mark(item.provider, 14., theme.content(0.80)))
                    .child(icon(status.icon).size(u(12.)).text_color(status_color))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.50))
                            .child(kind_line),
                    ),
            )
            .when(show_right, |row| row.child(right));
        let title = div()
            .mt(u(4.))
            .truncate()
            .text_px(theme.text.body)
            .semibold()
            .leading(theme.leading.snug)
            .text_color(theme.colors.content)
            .child(item.title.clone());
        let show_mark = !item.is_tracker() && !item.project_path.is_empty();
        let bottom = div()
            .mt(u(4.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .when(show_mark, |row| {
                        row.child(self.services.project_mark(&listed.project_mark, 12., cx))
                    })
                    .child(div().min_w_0().truncate().child(source)),
            )
            .when(!item.labels.is_empty(), |row| {
                row.child(
                    div()
                        .flex()
                        .flex_none()
                        .min_w_0()
                        .items_center()
                        .gap(u(4.))
                        .children(
                            item.labels
                                .iter()
                                .take(2)
                                .map(|label| label_chip(label, true, cx)),
                        ),
                )
            });
        let hover = theme.content(0.05);
        let ink = theme.colors.content;
        let mut card = div()
            .id(self.id)
            .flex()
            .flex_col()
            .w_full()
            .rounded(u(theme.radius.md))
            .px(u(10.))
            .py(u(8.))
            .tooltip(tooltip(item.title.clone()))
            .child(top)
            .child(title)
            .child(bottom);
        card = if self.active {
            card.bg(theme.colors.selection).text_color(ink)
        } else {
            card.text_color(theme.content(0.80))
                .hover(move |s| s.bg(hover).text_color(ink))
        };
        if let Some(handler) = self.on_select {
            card = card.on_click(move |event, window, cx| handler(event, window, cx));
        }
        card
    }
}

/// Exposed for the gallery: a card element without a click handler.
pub fn static_card(listed: ListedItem, services: Rc<dyn InboxServices>) -> AnyElement {
    inbox_card("static-card", listed, false, services).into_any_element()
}
