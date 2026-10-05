//! Port of src/features/inbox/ui/InboxMiniCard.tsx: the inbox item chip
//! above the composer after "Send to agent".

use std::rc::Rc;

use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{InboxComposerCard, InboxKind, InboxProvider};
use crate::style::{label_chip, provider_mark};

/// The provider name the card's link names.
pub fn mini_card_provider_label(provider: InboxProvider) -> &'static str {
    match provider {
        InboxProvider::Linear => "Linear",
        InboxProvider::Jira => "Jira",
        InboxProvider::Gitlab => "GitLab",
        InboxProvider::AzureDevops => "ADO",
        InboxProvider::Github => "GitHub",
    }
}

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
type OpenFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// `InboxMiniCard`.
#[derive(IntoElement)]
pub struct InboxMiniCard {
    card: InboxComposerCard,
    on_open: Option<OpenFn>,
    on_dismiss: Option<Handler>,
}

pub fn inbox_mini_card(card: InboxComposerCard) -> InboxMiniCard {
    InboxMiniCard {
        card,
        on_open: None,
        on_dismiss: None,
    }
}

impl InboxMiniCard {
    /// Opens the card's URL.
    pub fn on_open(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(handler));
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for InboxMiniCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let card = self.card;
        let kind_icon = if card.kind == InboxKind::Pr {
            IconName::GitPullRequest
        } else {
            IconName::CircleDot
        };
        let kind_label = if card.kind == InboxKind::Pr {
            if card.provider == InboxProvider::Gitlab {
                "Merge request"
            } else {
                "Pull request"
            }
        } else {
            "Issue"
        };
        let provider_label = mini_card_provider_label(card.provider);
        let url = card.url.clone();
        let mut body =
            div()
                .id("inbox-mini-card-open")
                .flex()
                .flex_col()
                .w_full()
                .tooltip(tooltip(format!("Open in {provider_label}")))
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(u(6.))
                        .child(provider_mark(card.provider, 14., theme.content(0.80)))
                        .child(icon(kind_icon).size(u(12.)).text_color(theme.content(0.45)))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_px(theme.text.caption)
                                .text_color(theme.content(0.50))
                                .child(format!("{kind_label} · {}", card.identifier)),
                        ),
                )
                .child(
                    div()
                        .mt(u(4.))
                        .truncate()
                        .text_px(theme.text.body)
                        .semibold()
                        .leading(theme.leading.snug)
                        .text_color(theme.colors.content)
                        .child(card.title.clone()),
                )
                .child(
                    div()
                        .mt(u(4.))
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(u(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_px(theme.text.caption)
                                .text_color(theme.content(0.45))
                                .child(card.source.clone()),
                        )
                        .when(!card.labels.is_empty(), |row| {
                            row.child(div().flex().flex_none().items_center().gap(u(4.)).children(
                                card.labels.iter().map(|label| label_chip(label, true, cx)),
                            ))
                        }),
                );
        if let (Some(open), false) = (self.on_open, url.is_empty()) {
            body = body.on_click(move |_, window, cx| open(&url, window, cx));
        }
        let mut frame = div()
            .relative()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.06))
            .px(u(10.))
            .py(u(8.))
            .pr(u(32.))
            .child(body);
        if let Some(dismiss) = self.on_dismiss {
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            frame = frame.child(
                div()
                    .id("inbox-mini-card-remove")
                    .group("mini-remove")
                    .absolute()
                    .right(u(6.))
                    .top(u(6.))
                    .flex()
                    .size(u(20.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.sm))
                    .hover(move |s| s.bg(hover))
                    .tooltip(tooltip("Remove"))
                    .on_click(move |_, window, cx| dismiss(window, cx))
                    .child(
                        icon(IconName::X)
                            .size(u(12.))
                            .text_color(theme.content(0.40))
                            .group_hover("mini-remove", move |s| s.text_color(ink)),
                    ),
            );
        }
        div().px(u(12.)).pt(u(8.)).child(frame)
    }
}
