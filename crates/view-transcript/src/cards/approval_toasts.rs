//! Port of src/features/sessions/ui/ApprovalToasts.tsx: a stack in the top
//! right corner with one card per session that waits on the reader, an
//! approval with Allow and Deny or a clarifying question. Clicking a card
//! opens its session.
//!
//! The host passes the pending notices (`hiddenApprovalNotices` in the
//! engine) and a gate that asks the project's notification preferences
//! whether a notice may show (`allowsProjectNotification` with the
//! `agentInput` category). Each notice keeps the time it first appeared,
//! so resuming a muted project does not replay a request that arrived while
//! it was muted.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    Animation, AnimationExt as _, App, Context, ElementId, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window,
    deferred, div, px,
};
use monocode_core::HarnessId;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::session::session_display_title;
use monocode_ui::styled::{UiStyled as _, glass_backdrop};
use monocode_ui::{IconName, ProviderLogo, Theme, icon, provider_logo, u};

use super::style;
use super::util::now_ms;

/// `PendingApprovalNotice.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoticeKind {
    Approval,
    Question,
}

/// A pending approval or question in a session (`PendingApprovalNotice &
/// { session }`), with the session fields the card shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalNotice {
    pub session_id: String,
    pub request_id: i64,
    pub label: String,
    pub kind: NoticeKind,
    pub session_title: String,
    pub harness: HarnessId,
    /// The session's folder, for the project's notification preferences.
    pub cwd: String,
}

impl ApprovalNotice {
    /// The React key: `${sessionId}:${kind}:${requestId}`.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.session_id,
            match self.kind {
                NoticeKind::Approval => "approval",
                NoticeKind::Question => "question",
            },
            self.request_id
        )
    }
}

/// What the reader did.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalToastEvent {
    /// `onFocusSession`.
    FocusSession { session_id: String },
    /// `onApproval`.
    Approval {
        session_id: String,
        request_id: i64,
        decision: ApprovalDecision,
    },
}

/// Whether a notice may show, given when it first appeared (epoch ms).
pub type NoticeGate = Rc<dyn Fn(&ApprovalNotice, i64, &App) -> bool>;

/// `<ApprovalToasts notices topOffset onFocusSession onApproval />`.
pub struct ApprovalToasts {
    notices: Vec<ApprovalNotice>,
    /// `occurredAt` per notice key, kept while the notice stays mounted.
    first_seen: HashMap<String, i64>,
    gate: NoticeGate,
    top_offset: f32,
    clock: Rc<dyn Fn() -> i64>,
}

impl EventEmitter<ApprovalToastEvent> for ApprovalToasts {}

impl ApprovalToasts {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            notices: Vec::new(),
            first_seen: HashMap::new(),
            gate: Rc::new(|_, _, _| true),
            top_offset: 12.,
            clock: Rc::new(now_ms),
        }
    }

    /// The notification preference check. Called on every draw, so a
    /// category turned off hides a card at once.
    pub fn set_gate(
        &mut self,
        gate: impl Fn(&ApprovalNotice, i64, &App) -> bool + 'static,
        cx: &mut Context<Self>,
    ) {
        self.gate = Rc::new(gate);
        cx.notify();
    }

    /// The wall clock, for tests.
    pub fn set_clock(&mut self, clock: impl Fn() -> i64 + 'static) {
        self.clock = Rc::new(clock);
    }

    /// `topOffset` in CSS px.
    pub fn set_top_offset(&mut self, offset: f32, cx: &mut Context<Self>) {
        self.top_offset = offset;
        cx.notify();
    }

    /// The pending notices. A notice seen for the first time is stamped now;
    /// one that left forgets its stamp.
    pub fn set_notices(&mut self, notices: Vec<ApprovalNotice>, cx: &mut Context<Self>) {
        let now = (self.clock)();
        let keys: Vec<String> = notices.iter().map(ApprovalNotice::key).collect();
        self.first_seen.retain(|key, _| keys.contains(key));
        for key in keys {
            self.first_seen.entry(key).or_insert(now);
        }
        self.notices = notices;
        cx.notify();
    }

    /// The notices the gate lets through, in order.
    pub fn visible(&self, cx: &App) -> Vec<ApprovalNotice> {
        self.notices
            .iter()
            .filter(|notice| {
                let occurred_at = self
                    .first_seen
                    .get(&notice.key())
                    .copied()
                    .unwrap_or_else(|| (self.clock)());
                (self.gate)(notice, occurred_at, cx)
            })
            .cloned()
            .collect()
    }

    pub fn focus_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        cx.emit(ApprovalToastEvent::FocusSession {
            session_id: session_id.to_string(),
        });
    }

    pub fn decide(
        &mut self,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ApprovalToastEvent::Approval {
            session_id: session_id.to_string(),
            request_id,
            decision,
        });
    }

    fn render_card(&self, notice: &ApprovalNotice, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let key = notice.key();
        let child = |part: &str| ElementId::Name(format!("approval-toast:{key}:{part}").into());
        let title = session_display_title(&notice.session_title, notice.harness);
        let harness = notice.harness.title();
        let warning = style::amber_400();
        let session_id = notice.session_id.clone();
        let logo = match ProviderLogo::from_id(notice.harness.as_str()) {
            Some(logo) => provider_logo(logo).size(16.).into_any_element(),
            None => div().size(u(16.)).into_any_element(),
        };
        let main = div()
            .id(child("open"))
            .relative()
            .flex()
            .flex_col()
            .gap(u(8.))
            .px(u(14.))
            .py(u(12.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.content(0.05)))
            .on_click(cx.listener(move |this, _, _, cx| this.focus_session(&session_id, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(logo)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_px(13.)
                            .semibold()
                            .leading(1.375)
                            .text_color(theme.colors.content)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(u(4.))
                            .text_px(11.)
                            .text_color(warning)
                            .child(icon(IconName::CircleAlert).size(u(14.)).text_color(warning))
                            .child(match notice.kind {
                                NoticeKind::Question => "Question",
                                NoticeKind::Approval => "Approval",
                            }),
                    ),
            )
            .child(
                div()
                    .line_clamp(3)
                    .text_px(12.)
                    .leading(1.625)
                    .text_color(theme.content(0.7))
                    .child(notice.label.clone()),
            )
            .child(
                div()
                    .text_px(11.)
                    .text_color(theme.content(0.4))
                    .child(harness),
            );
        let actions = (notice.kind == NoticeKind::Approval).then(|| {
            let button =
                |part: &str, label: &'static str, primary: bool, decision: ApprovalDecision| {
                    let session_id = notice.session_id.clone();
                    let request_id = notice.request_id;
                    let (fill, ink, hover) = if primary {
                        (
                            theme.colors.content,
                            theme.colors.background_base,
                            theme.content(0.8),
                        )
                    } else {
                        (theme.content(0.1), theme.content(0.7), theme.content(0.2))
                    };
                    div()
                        .id(child(part))
                        .flex_1()
                        .flex()
                        .justify_center()
                        .rounded(u(6.))
                        .px(u(10.))
                        .py(u(4.))
                        .bg(fill)
                        .text_color(ink)
                        .text_px(11.)
                        .medium()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.decide(&session_id, request_id, decision, cx)
                        }))
                        .child(label)
                };
            div()
                .relative()
                .flex()
                .gap(u(8.))
                .border_t(px(1.))
                .border_color(theme.colors.stroke)
                .px(u(14.))
                .py(u(10.))
                .child(button("allow", "Allow", true, ApprovalDecision::Allow))
                .child(button("deny", "Deny", false, ApprovalDecision::Deny))
        });
        let card = div()
            .relative()
            .overflow_hidden()
            .rounded(u(12.))
            .border_1()
            .border_dashed()
            .border_color(theme.content(0.2))
            .shadow_xl()
            .font_family(theme.fonts.sans.clone())
            .child(glass_backdrop(12., 24., theme.content(0.1)))
            .child(main)
            .children(actions);
        // `approval-toast-in`: 180ms, an 8px drop and a fade.
        let motion = theme.motion;
        card.with_animation(
            child("in"),
            Animation::new(motion.toast_in)
                .with_easing(monocode_ui::theme::CubicBezier(0.22, 1., 0.36, 1.).easing()),
            |el, t| el.opacity(t).top(u(-8. * (1. - t))),
        )
    }
}

impl Render for ApprovalToasts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible = self.visible(cx);
        if visible.is_empty() {
            return div().into_any_element();
        }
        let theme = Theme::of(cx);
        let layer = theme.layer.toast;
        // `w-[min(360px,calc(100vw-24px))]`.
        let width = u(360.)
            .to_pixels(window.rem_size())
            .min(window.viewport_size().width - u(24.).to_pixels(window.rem_size()));
        let mut column = div()
            .absolute()
            .top(u(self.top_offset))
            .right(u(12.))
            .w(width)
            .flex()
            .flex_col()
            .gap(u(8.));
        for notice in &visible {
            column = column.child(self.render_card(notice, cx));
        }
        deferred(column).with_priority(layer).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn notice(id: &str, kind: NoticeKind, request_id: i64) -> ApprovalNotice {
        ApprovalNotice {
            session_id: id.into(),
            request_id,
            label: format!("Request {id}"),
            kind,
            session_title: id.into(),
            harness: HarnessId::Codex,
            cwd: format!("/projects/{id}"),
        }
    }

    #[test]
    fn keys_notices_like_react() {
        assert_eq!(
            notice("work", NoticeKind::Approval, 3).key(),
            "work:approval:3"
        );
        assert_eq!(
            notice("work", NoticeKind::Question, 1).key(),
            "work:question:1"
        );
    }
}
