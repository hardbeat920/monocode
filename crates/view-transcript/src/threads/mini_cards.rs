//! Port of src/features/sessions/ui/SecondOpinionCard.tsx and
//! HandoffMiniCard.tsx: the compact second opinion and handoff cards.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_core::HarnessId;
use monocode_core::block::{SecondOpinionKind, SecondOpinionMeta};
use monocode_core::handoff::HandoffComposerCard;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::parts::{files_label, harness_icon};

/// `SecondOpinionCard`: the card inside a second opinion or split-pane
/// handoff turn's prompt.
#[derive(IntoElement)]
pub struct SecondOpinionCard {
    card: SecondOpinionMeta,
}

pub fn second_opinion_card(card: SecondOpinionMeta) -> SecondOpinionCard {
    SecondOpinionCard { card }
}

impl SecondOpinionCard {
    /// The card's title line.
    pub fn title(&self) -> &'static str {
        if self.card.kind == Some(SecondOpinionKind::Handoff) {
            "Handoff"
        } else {
            "Second opinion"
        }
    }
}

impl RenderOnce for SecondOpinionCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let title = self.title();
        let card = self.card;
        let files = files_label(card.files);
        div()
            .min_w_0()
            .font_family(theme.fonts.sans.clone())
            .child(
                div()
                    .text_px(13.)
                    .medium()
                    .leading(theme.leading.snug)
                    .text_color(theme.colors.content)
                    .child(title),
            )
            .child(
                div()
                    .mt(u(4.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .text_px(11.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.50))
                    .child(harness_icon(card.from, 12.))
                    .child(div().truncate().child(card.from.title()))
                    .child(
                        icon(IconName::ChevronRight)
                            .size(u(12.))
                            .text_color(theme.content(0.35)),
                    )
                    .child(harness_icon(card.to, 12.))
                    .child(div().truncate().child(card.to.title())),
            )
            .children(files.map(|files| {
                div()
                    .mt(u(4.))
                    .text_px(11.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.45))
                    .child(files)
            }))
    }
}

/// The handoff card's data: `HandoffComposerCard` without the brief.
#[derive(Clone, Debug, PartialEq)]
pub struct HandoffCard {
    pub from: HarnessId,
    pub to: HarnessId,
    pub request: Option<String>,
    pub files: Option<i64>,
}

impl From<&HandoffComposerCard> for HandoffCard {
    fn from(card: &HandoffComposerCard) -> Self {
        Self {
            from: card.from,
            to: card.to,
            request: card.request.clone(),
            files: card.files,
        }
    }
}

type DismissHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// `HandoffMiniCard`: the handoff chip inside the composer box.
#[derive(IntoElement)]
pub struct HandoffMiniCard {
    id: ElementId,
    card: HandoffCard,
    on_dismiss: Option<DismissHandler>,
}

pub fn handoff_mini_card(id: impl Into<ElementId>, card: HandoffCard) -> HandoffMiniCard {
    HandoffMiniCard {
        id: id.into(),
        card,
        on_dismiss: None,
    }
}

impl HandoffMiniCard {
    /// `onDismiss`: shows the remove button.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for HandoffMiniCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let card = self.card;
        let files = files_label(card.files);
        let dismissable = self.on_dismiss.is_some();
        let body = div()
            .flex()
            .w_full()
            .flex_col()
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .child(
                        icon(IconName::Replace)
                            .size(u(14.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_px(11.)
                            .text_color(theme.content(0.50))
                            .child("Handoff"),
                    ),
            )
            .child(
                div()
                    .mt(u(4.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .text_px(13.)
                    .semibold()
                    .leading(theme.leading.snug)
                    .text_color(theme.colors.content)
                    .child(harness_icon(card.from, 14.))
                    .child(div().min_w_0().truncate().child(card.from.title()))
                    .child(
                        icon(IconName::ChevronRight)
                            .size(u(12.))
                            .text_color(theme.content(0.35)),
                    )
                    .child(harness_icon(card.to, 14.))
                    .child(div().min_w_0().truncate().child(card.to.title())),
            )
            .children(
                card.request
                    .filter(|request| !request.is_empty())
                    .map(|request| {
                        div()
                            .mt(u(4.))
                            .truncate()
                            .text_px(11.)
                            .text_color(theme.content(0.45))
                            .child(SharedString::from(request))
                    }),
            )
            .children(files.map(|files| {
                div()
                    .mt(u(4.))
                    .text_px(11.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.45))
                    .child(files)
            }));
        let mut frame = div()
            .relative()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.06))
            .px(u(10.))
            .py(u(8.))
            .child(body);
        if dismissable {
            frame = frame.pr(u(32.));
        }
        if let Some(handler) = self.on_dismiss {
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            frame = frame.child(
                div()
                    .id(self.id)
                    .absolute()
                    .right(u(6.))
                    .top(u(6.))
                    .size(u(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.sm))
                    .group("handoff-dismiss")
                    .hover(move |style| style.bg(hover))
                    .tooltip(tooltip("Remove"))
                    .on_click(move |event, window, cx| handler(event, window, cx))
                    .child(
                        icon(IconName::X)
                            .size(u(12.))
                            .text_color(theme.content(0.40))
                            .group_hover("handoff-dismiss", move |style| style.text_color(ink)),
                    ),
            );
        }
        div().px(u(12.)).pt(u(8.)).child(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::Extra;

    #[test]
    fn titles_a_review_and_a_split_pane_handoff() {
        let review = SecondOpinionMeta {
            from: HarnessId::Claude,
            to: HarnessId::Codex,
            request: None,
            files: Some(2),
            kind: None,
            extra: Extra::new(),
        };
        assert_eq!(
            second_opinion_card(review.clone()).title(),
            "Second opinion"
        );
        let handoff = SecondOpinionMeta {
            kind: Some(SecondOpinionKind::Handoff),
            ..review
        };
        assert_eq!(second_opinion_card(handoff).title(), "Handoff");
    }
}
