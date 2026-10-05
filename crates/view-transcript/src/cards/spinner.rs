//! Port of src/features/sessions/ui/TerminalSpinner.tsx: a braille spinner
//! that steps one frame every 80ms.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, ElementId, Hsla, IntoElement, ParentElement as _,
    RenderOnce, Styled as _, Window, div,
};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{Theme, u};

/// `FRAMES`.
pub const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// The interval between frames.
pub const FRAME_MS: u64 = 80;

/// The frame shown `elapsed_ms` after the spinner mounted.
pub fn frame_at(elapsed_ms: u64) -> &'static str {
    FRAMES[((elapsed_ms / FRAME_MS) as usize) % FRAMES.len()]
}

/// `<TerminalSpinner />`: `inline-block w-3.5 text-center text-[11px]
/// leading-none`, in the current text color unless one is set.
#[derive(IntoElement)]
pub struct TerminalSpinner {
    id: ElementId,
    color: Option<Hsla>,
    size: f32,
}

pub fn terminal_spinner(id: impl Into<ElementId>) -> TerminalSpinner {
    TerminalSpinner {
        id: id.into(),
        color: None,
        size: 11.,
    }
}

impl TerminalSpinner {
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Font size in CSS px.
    pub fn text_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for TerminalSpinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let cycle = Duration::from_millis(FRAME_MS * FRAMES.len() as u64);
        div()
            .w(u(14.))
            .flex_none()
            .flex()
            .justify_center()
            .text_px(self.size)
            .line_height(u(self.size))
            .text_color(self.color.unwrap_or(theme.content(0.45)))
            .with_animation(self.id, Animation::new(cycle).repeat(), |el, delta| {
                let frame = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
                el.child(FRAMES[frame])
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_one_frame_every_80ms_and_wraps() {
        assert_eq!(frame_at(0), "⠋");
        assert_eq!(frame_at(79), "⠋");
        assert_eq!(frame_at(80), "⠙");
        assert_eq!(frame_at(80 * 10), "⠋");
    }
}
