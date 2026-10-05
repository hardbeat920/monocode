//! `TerminalSpinner`: a braille dot spinner that steps every 80ms.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, ElementId, Hsla, IntoElement, ParentElement as _,
    RenderOnce, Styled as _, Window, div,
};

use crate::styled::UiStyled as _;
use crate::{Theme, u};

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME: Duration = Duration::from_millis(80);

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    color: Option<Hsla>,
    size: f32,
}

pub fn spinner(id: impl Into<ElementId>) -> Spinner {
    Spinner {
        id: id.into(),
        color: None,
        size: 11.,
    }
}

impl Spinner {
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Font size in CSS px. Default 11.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex()
            .flex_none()
            .justify_center()
            .w(u(12.))
            .text_px(self.size)
            .leading(theme.leading.none)
            .text_color(self.color.unwrap_or(theme.colors.accent))
            .with_animation(
                self.id,
                Animation::new(FRAME * FRAMES.len() as u32).repeat(),
                |el, t| {
                    let index = ((t * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
                    el.child(FRAMES[index])
                },
            )
    }
}
