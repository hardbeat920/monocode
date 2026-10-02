//! `DiffStat` from Sidebar.tsx and `ProjectDiffStat` from ProjectRail.tsx:
//! `+478 -2` in the success and danger colors, 11px semibold tabular.

use gpui::{App, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div};

use crate::styled::{UiStyled as _, format_integer};
use crate::{Theme, u};

#[derive(IntoElement)]
pub struct DiffStat {
    additions: i64,
    deletions: i64,
    gap: f32,
    size: Option<f32>,
}

/// Renders nothing when both counts are zero, like the React component.
pub fn diff_stat(additions: i64, deletions: i64) -> DiffStat {
    DiffStat {
        additions,
        deletions,
        gap: 6.,
        size: None,
    }
}

impl DiffStat {
    /// Gap between the counts: 6px in the sidebar, 4px in the project rail.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Font size in CSS px. Default 11.
    pub fn text_size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }
}

impl RenderOnce for DiffStat {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(self.gap))
            .text_px(self.size.unwrap_or(theme.text.caption))
            .semibold()
            .tabular()
            .when(self.additions > 0, |el| {
                el.child(
                    div()
                        .text_color(c.success)
                        .child(format!("+{}", format_integer(self.additions))),
                )
            })
            .when(self.deletions > 0, |el| {
                el.child(
                    div()
                        .text_color(c.danger)
                        .child(format!("-{}", format_integer(self.deletions))),
                )
            })
    }
}

use gpui::prelude::FluentBuilder as _;
