//! Count badges and unread dots. RailAction.tsx draws the count pill
//! (`bg-accent text-[10px] font-semibold text-white`) and the `size-2` dot;
//! the sidebar uses `size-1.5` dots.

use gpui::{
    App, Hsla, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
};

use crate::styled::UiStyled as _;
use crate::{Theme, u};

#[derive(IntoElement)]
pub struct Badge {
    count: u32,
    color: Option<Hsla>,
}

/// A count pill. Counts over 99 read `99+`.
pub fn badge(count: u32) -> Badge {
    Badge { count, color: None }
}

impl Badge {
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let label: SharedString = if self.count > 99 {
            "99+".into()
        } else {
            self.count.to_string().into()
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .min_w(u(16.))
            .h(u(16.))
            .px(u(4.))
            .rounded_full()
            .bg(self.color.unwrap_or(theme.colors.accent))
            .text_px(theme.text.micro)
            .semibold()
            .leading(theme.leading.none)
            .tabular()
            .text_color(crate::color::hex(0xffffff))
            .child(label)
    }
}

/// An accent dot. `size` is the diameter in CSS px (6 or 8).
#[derive(IntoElement)]
pub struct Dot {
    size: f32,
    color: Option<Hsla>,
}

pub fn dot(size: f32) -> Dot {
    Dot { size, color: None }
}

impl Dot {
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Dot {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex_none()
            .size(u(self.size))
            .rounded_full()
            .bg(self.color.unwrap_or(theme.colors.accent))
    }
}

/// A small tinted label, like the title bar's Development badge
/// (`rounded-md bg-skill/15 px-1.5 py-0.5 text-[10px] font-medium text-skill`).
#[derive(IntoElement)]
pub struct Tag {
    label: SharedString,
    color: Option<Hsla>,
}

pub fn tag(label: impl Into<SharedString>) -> Tag {
    Tag {
        label: label.into(),
        color: None,
    }
}

impl Tag {
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Tag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let color = self.color.unwrap_or(theme.colors.skill);
        div()
            .flex_none()
            .px(u(6.))
            .py(u(2.))
            .rounded(u(theme.radius.md))
            .bg(crate::color::with_alpha(color, 0.15))
            .text_color(color)
            .text_px(theme.text.micro)
            .medium()
            .leading(theme.leading.label)
            .child(self.label)
    }
}
