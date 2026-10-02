//! Toasts. The look follows src/features/sessions/ui/ApprovalToasts.tsx: a
//! 360px column at the top right, each toast a dashed 12px glass card that
//! drops in 8px over 180ms.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, AsyncApp, BorrowAppContext as _, Global, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, deferred, div,
};

use crate::styled::{UiStyled as _, glass_backdrop};
use crate::{IconName, ProviderLogo, Theme, icon, provider_logo, u};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

type ActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct ToastAction {
    pub label: SharedString,
    /// The solid `bg-content` button. Others use `bg-content/10`.
    pub primary: bool,
    pub handler: ActionHandler,
}

#[derive(Clone, Default)]
pub struct Toast {
    pub id: u64,
    pub title: SharedString,
    /// The status label at the right of the title row ("Approval").
    pub status: Option<SharedString>,
    pub body: Option<SharedString>,
    /// The quiet line under the body (the provider name).
    pub meta: Option<SharedString>,
    pub kind: ToastKind,
    pub provider: Option<ProviderLogo>,
    pub actions: Vec<ToastAction>,
}

impl Toast {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn kind(mut self, kind: ToastKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn status(mut self, status: impl Into<SharedString>) -> Self {
        self.status = Some(status.into());
        self
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn provider(mut self, provider: ProviderLogo) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        primary: bool,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions.push(ToastAction {
            label: label.into(),
            primary,
            handler: Rc::new(handler),
        });
        self
    }
}

/// The live toasts, a GPUI global.
#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Global for Toasts {}

impl Toasts {
    pub fn items(cx: &App) -> &[Toast] {
        cx.try_global::<Toasts>()
            .map(|toasts| toasts.items.as_slice())
            .unwrap_or(&[])
    }

    /// Shows a toast and returns its id.
    pub fn push(toast: Toast, cx: &mut App) -> u64 {
        let id = cx.update_default_global::<Toasts, _>(|toasts, _| {
            toasts.next_id += 1;
            let id = toasts.next_id;
            toasts.items.push(Toast { id, ..toast });
            id
        });
        cx.refresh_windows();
        id
    }

    /// Shows a toast that dismisses itself after `duration`.
    pub fn push_timed(toast: Toast, duration: Duration, cx: &mut App) -> u64 {
        let id = Self::push(toast, cx);
        cx.spawn(async move |cx: &mut AsyncApp| {
            cx.background_executor().timer(duration).await;
            cx.update(|cx| Self::dismiss(id, cx));
        })
        .detach();
        id
    }

    pub fn dismiss(id: u64, cx: &mut App) {
        cx.update_default_global::<Toasts, _>(|toasts, _| toasts.items.retain(|t| t.id != id));
        cx.refresh_windows();
    }
}

/// Draws the live toasts in the toast layer. Put one at the root of a window.
#[derive(IntoElement, Default)]
pub struct ToastStack {
    top_offset: f32,
    animate: bool,
}

pub fn toast_stack() -> ToastStack {
    ToastStack {
        top_offset: 12.,
        animate: true,
    }
}

impl ToastStack {
    /// Distance from the window top in CSS px, below other notices.
    pub fn top_offset(mut self, offset: f32) -> Self {
        self.top_offset = offset;
        self
    }

    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }
}

fn status_color(theme: &Theme, kind: ToastKind) -> Hsla {
    match kind {
        ToastKind::Info => theme.colors.accent,
        ToastKind::Success => theme.colors.success,
        ToastKind::Warning => theme.colors.warning,
        ToastKind::Error => theme.colors.danger,
    }
}

impl RenderOnce for ToastStack {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let motion = theme.motion;
        let mut column = div()
            .absolute()
            .top(u(self.top_offset))
            .right(u(12.))
            .w(u(360.))
            .flex()
            .flex_col()
            .gap(u(8.));
        for toast in Toasts::items(cx).iter().cloned() {
            let status = status_color(theme, toast.kind);
            let mut title_row = div().flex().items_center().gap(u(8.));
            if let Some(provider) = toast.provider {
                title_row = title_row.child(provider_logo(provider).size(16.));
            }
            title_row = title_row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.body)
                    .semibold()
                    .leading(theme.leading.snug)
                    .text_color(c.content)
                    .child(toast.title.clone()),
            );
            if let Some(label) = toast.status.clone() {
                title_row = title_row.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(u(4.))
                        .text_px(theme.text.caption)
                        .text_color(status)
                        .child(icon(IconName::CircleAlert).size(u(14.)).text_color(status))
                        .child(label),
                );
            }
            let mut main = div()
                .id(("toast-main", toast.id))
                .relative()
                .flex()
                .flex_col()
                .gap(u(8.))
                .px(u(14.))
                .py(u(12.))
                .hover(|s| s.bg(theme.content(0.05)))
                .child(title_row);
            if let Some(body) = toast.body.clone() {
                main = main.child(
                    div()
                        .line_clamp(3)
                        .text_px(theme.text.label)
                        .leading(theme.leading.relaxed)
                        .text_color(theme.content(0.70))
                        .child(body),
                );
            }
            if let Some(meta) = toast.meta.clone() {
                main = main.child(
                    div()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.40))
                        .child(meta),
                );
            }
            let mut card = div()
                .relative()
                .overflow_hidden()
                .rounded(u(theme.radius.xl))
                .border_1()
                .border_dashed()
                .border_color(theme.content(0.20))
                .shadow_xl()
                .child(glass_backdrop(theme.radius.xl, 24., theme.content(0.10)))
                .child(main);
            if !toast.actions.is_empty() {
                let mut actions = div()
                    .relative()
                    .flex()
                    .gap(u(8.))
                    .border_t_1()
                    .border_color(c.stroke)
                    .px(u(14.))
                    .py(u(10.));
                for (index, action) in toast.actions.iter().cloned().enumerate() {
                    let (fill, ink, hover) = if action.primary {
                        (c.content, c.background_base, theme.content(0.80))
                    } else {
                        (
                            theme.content(0.10),
                            theme.content(0.70),
                            theme.content(0.20),
                        )
                    };
                    let handler = action.handler.clone();
                    actions = actions.child(
                        div()
                            .id(("toast-action", toast.id * 16 + index as u64))
                            .flex_1()
                            .flex()
                            .justify_center()
                            .rounded(u(theme.radius.md))
                            .px(u(10.))
                            .py(u(4.))
                            .bg(fill)
                            .text_color(ink)
                            .text_px(theme.text.caption)
                            .medium()
                            .hover(move |s| s.bg(hover))
                            .on_click(move |_, window, cx| handler(window, cx))
                            .child(action.label.clone()),
                    );
                }
                card = card.child(actions);
            }
            let card = if self.animate {
                card.with_animation(
                    ("toast", toast.id),
                    Animation::new(motion.toast_in).with_easing(motion.ease_out.easing()),
                    |el, t| el.opacity(t).top(u(-8.0 * (1.0 - t))),
                )
                .into_any_element()
            } else {
                card.into_any_element()
            };
            column = column.child(card);
        }
        deferred(column).with_priority(theme.layer.toast)
    }
}
