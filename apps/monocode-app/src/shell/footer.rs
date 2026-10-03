//! The 28px usage footer. Port of src/app/shell/UsageFooter.tsx with the
//! `UsageProviderChip` trigger and its `MiniBar`.

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, relative,
};
use monocode_ui::widgets::{icon_button, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::Shell;
use crate::format::format_percent;
use crate::view_data::{ShellData, UsageChip};

/// `barClass` in ProviderAccountUsage.tsx.
fn bar_color(theme: &Theme, used: f32) -> gpui::Hsla {
    if used >= 90.0 {
        theme.colors.danger
    } else if used >= 80.0 {
        theme.colors.warning
    } else {
        theme.content(0.45)
    }
}

impl Shell {
    pub(super) fn render_footer(
        &self,
        data: &ShellData,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let mut footer = div()
            .id("usage-footer")
            .flex()
            .flex_none()
            .h(u(theme.metrics.footer_height))
            .items_center()
            .gap(u(6.))
            .px(u(12.))
            .border_t_1()
            .border_color(c.stroke)
            .text_px(theme.text.caption)
            .text_color(theme.content(0.55));
        for (index, chip) in data.usage.iter().enumerate() {
            footer = footer.child(usage_chip(index, chip, &theme));
        }
        footer
            .child(
                icon_button("usage-refresh", IconName::RefreshCw)
                    .size(18.)
                    .icon_size(10.)
                    .tooltip("Refresh usage"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("footer-terminal")
                    .group("footer-terminal")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(6.))
                    .h(u(20.))
                    .px(u(6.))
                    .rounded(u(theme.radius.sm))
                    .text_color(theme.content(0.40))
                    .hover({
                        let fill = theme.content(0.10);
                        let ink = c.content;
                        move |s| s.bg(fill).text_color(ink)
                    })
                    .child(
                        icon(IconName::Terminal)
                            .size(u(14.))
                            .text_color(theme.content(0.40))
                            .group_hover("footer-terminal", {
                                let ink = c.content;
                                move |s| s.text_color(ink)
                            }),
                    )
                    .child("Terminal")
                    .tooltip(tooltip("Show terminal")),
            )
    }
}

fn usage_chip(index: usize, chip: &UsageChip, theme: &Theme) -> impl IntoElement {
    let tightest = chip
        .windows
        .iter()
        .map(|w| w.used_percent)
        .fold(0.0f32, f32::max);
    let mut windows = div().flex().min_w_0().items_center().gap(u(4.)).tabular();
    for (i, window) in chip.windows.iter().enumerate() {
        if i > 0 {
            windows = windows.child(div().text_color(theme.content(0.25)).child("·"));
        }
        windows = windows.child(format!(
            "{} {}",
            format_percent(window.used_percent),
            window.label.clone()
        ));
    }
    div()
        .id(("usage-chip", index))
        .flex()
        .flex_none()
        .items_center()
        .gap(u(6.))
        .h(u(20.))
        .px(u(4.))
        .mx(u(-4.))
        .rounded(u(theme.radius.sm))
        .whitespace_nowrap()
        .hover({
            let fill = theme.content(0.10);
            let ink = theme.colors.content;
            move |s| s.bg(fill).text_color(ink)
        })
        .child(provider_logo(chip.provider).size(12.))
        .child(
            div()
                .flex_none()
                .h(u(4.))
                .w(u(32.))
                .rounded_full()
                .overflow_hidden()
                .bg(theme.content(0.10))
                .child(
                    div()
                        .h_full()
                        .w(relative((100.0 - tightest.clamp(0.0, 100.0)) / 100.0))
                        .rounded_full()
                        .bg(bar_color(theme, tightest)),
                ),
        )
        .child(windows)
}
