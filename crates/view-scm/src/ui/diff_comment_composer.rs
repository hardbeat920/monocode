//! Port of src/features/source-control/ui/DiffCommentComposer.tsx: a small
//! popover to comment on one diff line and add the comment to the chat.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div,
};
use gpui_component::input::{Enter, Escape, InputEvent, TextareaState};
use monocode_editor::unified_diff::DiffCommentTarget;
use monocode_ui::widgets::{popover_at, popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::model::diff_comment::diff_comment_context;
use crate::paths::MOD;
use crate::scm::Scm;
use crate::ui::common::{plain_textarea, with_alpha};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffCommentEvent {
    Dismiss,
}

pub struct DiffCommentComposer {
    scm: Scm,
    target: DiffCommentTarget,
    position: Point<Pixels>,
    comment: Entity<TextareaState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DiffCommentEvent> for DiffCommentComposer {}

impl DiffCommentComposer {
    /// A composer for `target`, opening to the right of `position`.
    pub fn new(
        scm: Scm,
        target: DiffCommentTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let comment = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("Leave a comment…")
        });
        comment.update(cx, |state, cx| state.focus(window, cx));
        let subscriptions = vec![cx.subscribe(&comment, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        Self {
            scm,
            target,
            position,
            comment,
            _subscriptions: subscriptions,
        }
    }

    pub fn location(&self) -> String {
        self.target.location()
    }

    pub fn comment_input(&self) -> &Entity<TextareaState> {
        &self.comment
    }

    fn text(&self, cx: &gpui::App) -> String {
        self.comment.read(cx).value().to_string()
    }

    /// `addToChat`.
    pub fn add_to_chat(&mut self, cx: &mut Context<Self>) {
        let Some(item) = diff_comment_context(&self.target, &self.text(cx)) else {
            return;
        };
        self.scm.hooks.add_to_chat(item, cx);
        cx.emit(DiffCommentEvent::Dismiss);
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(DiffCommentEvent::Dismiss);
    }
}

impl Render for DiffCommentComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let location = self.location();
        let empty = self.text(cx).trim().is_empty();
        let focused = gpui::Focusable::focus_handle(self.comment.read(cx), cx).is_focused(window);
        let mut add = div()
            .id("add-to-chat")
            .flex()
            .h(u(28.))
            .items_center()
            .gap(u(6.))
            .rounded(u(6.))
            .bg(c.content)
            .px(u(10.))
            .text_px(12.)
            .medium()
            .text_color(c.background_base)
            .child(
                icon(IconName::MessageSquarePlus)
                    .size(u(14.))
                    .text_color(c.background_base),
            )
            .child("Add to chat");
        if empty {
            add = add.opacity(0.4);
        } else {
            add = add
                .hover(|s| s.opacity(0.8))
                .on_click(cx.listener(|this, _, _, cx| this.add_to_chat(cx)));
        }
        let body = div()
            .id("diff-comment")
            .p(u(8.))
            .capture_action(cx.listener(|this, action: &Enter, _, cx| {
                if action.secondary && !this.text(cx).trim().is_empty() {
                    this.add_to_chat(cx);
                } else {
                    cx.propagate();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, _, cx| this.dismiss(cx)))
            .child(
                div()
                    .mb(u(6.))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .px(u(2.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(11.)
                            .text_color(theme.content(0.55))
                            .child(location),
                    )
                    .child(
                        div()
                            .id("cancel-comment")
                            .flex()
                            .flex_none()
                            .size(u(20.))
                            .items_center()
                            .justify_center()
                            .rounded(u(4.))
                            .tooltip(tooltip("Cancel comment"))
                            .hover(|s| s.bg(theme.content(0.10)))
                            .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                            .child(
                                icon(IconName::X)
                                    .size(u(12.))
                                    .text_color(theme.content(0.45)),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_h(u(72.))
                    .rounded(u(8.))
                    .border_1()
                    .border_color(theme.content(if focused { 0.20 } else { 0.10 }))
                    .bg(with_alpha(c.background_base, 0.70))
                    .px(u(10.))
                    .py(u(8.))
                    .text_px(13.)
                    .line_height(u(20.))
                    .child(plain_textarea(&self.comment, cx)),
            )
            .child(
                div()
                    .mt(u(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .child(
                        div()
                            .text_px(10.)
                            .text_color(theme.content(0.35))
                            .child(format!("{MOD}↩ to add")),
                    )
                    .child(add),
            );
        popover_at(
            self.position,
            gpui::Anchor::TopLeft,
            div()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                .child(popover_frame("diff-comment-frame").width(320.).child(body)),
            cx,
        )
    }
}
