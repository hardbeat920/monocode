//! The 40px title bar with workspace tabs. Port of src/app/shell/TitleBar.tsx
//! (`TitleBarComponent`, `TitleTabItem`, `TabHarnesses`).

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use monocode_ui::widgets::{icon_button, spinner, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, provider_logo, u};

use super::{Shell, WhenMac as _};
use crate::view_data::{HarnessState, ShellData, TabLead, TitleTabView};

/// A tab slot is `w-56 min-w-28`; at 176px and wider its text splits into a
/// 10px headline and a meta line (`@min-[11rem]`).
const TAB_WIDTH: f32 = 224.0;
const TAB_MIN_WIDTH: f32 = 112.0;

impl Shell {
    pub(super) fn render_title_bar(
        &self,
        data: &ShellData,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let compact_title_bar = cfg!(target_os = "macos") && self.compact_rail_visible();
        let rail_closed = !self.project_rail_open;

        let mut bar = div()
            .id("title-bar")
            .flex()
            .flex_none()
            .h(u(theme.metrics.title_bar_height))
            .items_stretch()
            .border_b_1()
            .border_color(c.stroke);
        if compact_title_bar {
            bar = bar.bg(c.body_glass).child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .pl(u(70.))
                    .child(self.render_visit_nav_without_panel()),
            );
        }
        if !self.session_sidebar_open {
            bar = bar.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .px(u(6.))
                    .when_mac(|el| {
                        if rail_closed && !compact_title_bar {
                            el.child(div().flex_none().w(u(70.)))
                        } else {
                            el
                        }
                    })
                    .child(
                        icon_button("toggle-session-sidebar", IconName::DashboardSquare)
                            .tooltip("Toggle Session Sidebar (⌘⇧B)")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.session_sidebar_open = !this.session_sidebar_open;
                                cx.notify();
                            })),
                    ),
            );
        }

        // The strip scrolls sideways once the tabs reach their minimum
        // width, and keeps the active tab in view.
        let mut strip = div()
            .id("title-tabs")
            .flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .items_center()
            .gap(u(2.))
            .pl(u(6.))
            .pr(u(10.))
            .overflow_x_scroll()
            .track_scroll(&self.title_scroll);
        let closable = data.tabs.len() > 1;
        for (index, tab) in data.tabs.iter().enumerate() {
            let active = tab.id == data.active_tab_id;
            strip = strip.child(self.render_title_tab(index, tab, active, closable, &theme, cx));
        }
        let tabs = self.drag_region(strip, cx);
        bar = bar.child(tabs);

        if rail_closed {
            bar = bar.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(2.))
                    .px(u(8.))
                    .child(icon_button("title-goto", IconName::Search).tooltip("Go to File (⌘P)"))
                    .child(
                        icon_button("title-new", IconName::Plus)
                            .tooltip("New session (⌘T)")
                            .on_click(cx.listener(|this, _, _, cx| this.new_session(cx))),
                    ),
            );
        }
        bar
    }

    fn render_visit_nav_without_panel(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(icon_button("compact-back", IconName::ChevronLeft).disabled(true))
            .child(icon_button("compact-forward", IconName::ChevronRight).disabled(true))
    }

    fn render_title_tab(
        &self,
        index: usize,
        tab: &TitleTabView,
        active: bool,
        closable: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let dim = if active { 1.0 } else { 0.55 };
        let group = format!("title-tab-{index}");

        let lead: AnyElement = match &tab.lead {
            TabLead::Harnesses(providers) => {
                let mut row = div().flex().flex_none().items_center();
                for (i, (provider, state)) in providers.iter().enumerate() {
                    let mark = div()
                        .flex()
                        .flex_none()
                        .size(u(14.))
                        .items_center()
                        .justify_center()
                        .when(i > 0, |el| el.ml(u(-2.)));
                    let mark = match state {
                        HarnessState::Busy => mark.child(
                            spinner(gpui::SharedString::from(format!("tab-busy-{}-{i}", tab.id)))
                                .color(c.accent),
                        ),
                        HarnessState::Done => mark.child(
                            icon(IconName::CheckCircle)
                                .size(u(14.))
                                .text_color(c.success),
                        ),
                        HarnessState::Idle => {
                            mark.opacity(dim).child(provider_logo(*provider).size(14.))
                        }
                    };
                    row = row.child(mark);
                }
                row.into_any_element()
            }
            TabLead::File(name) => div()
                .flex_none()
                .opacity(dim)
                .child(file_type_icon(name.clone()).size(14.))
                .into_any_element(),
            TabLead::Terminal => icon(IconName::Terminal)
                .size(u(14.))
                .text_color(if active {
                    c.content
                } else {
                    theme.content(0.55)
                })
                .into_any_element(),
        };

        let mut text = div().flex().flex_col().flex_1().min_w_0().justify_center();
        let headline = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(4.))
            .child({
                let line = div().min_w_0().truncate().leading(theme.leading.tight);
                let line = if tab.preview { line.italic() } else { line };
                if tab.meta.is_some() {
                    line.text_px(theme.text.micro).medium()
                } else {
                    line.text_px(theme.text.body)
                }
                .child(tab.headline.clone())
            })
            .when(tab.dirty, |el| {
                el.child(
                    div()
                        .flex_none()
                        .size(u(6.))
                        .rounded_full()
                        .bg(theme.content(0.70)),
                )
            });
        text = text.child(headline);
        if let Some(meta) = tab.meta.clone() {
            text = text.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.micro)
                    .leading(theme.leading.tight)
                    .text_color(theme.content(0.45))
                    .child(meta),
            );
        }

        let (ink, fill) = if active {
            (c.content, Some(c.selection))
        } else {
            (theme.content(0.50), None)
        };
        let mut button = div()
            .id(("title-tab", index))
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .h(u(30.))
            .items_center()
            .gap(u(6.))
            .pl(u(8.))
            .pr(u(if closable { 28. } else { 10. }))
            .rounded(u(theme.radius.md))
            .text_color(ink)
            .child(lead)
            .child(text)
            .tooltip(tooltip(tab.tooltip.clone()))
            .on_click({
                let id = tab.id.clone();
                cx.listener(move |this, _: &ClickEvent, _, cx| this.activate_tab(&id, cx))
            });
        if let Some(fill) = fill {
            button = button.bg(fill);
        } else {
            let hover_fill = theme.content(0.05);
            let hover_ink = c.content;
            button = button.hover(move |s| s.bg(hover_fill).text_color(hover_ink));
        }

        let mut slot = div()
            .id(("title-tab-slot", index))
            .group(group.clone())
            .relative()
            .flex()
            .h_full()
            .w(u(TAB_WIDTH))
            .min_w(u(TAB_MIN_WIDTH))
            .items_center()
            .child(button);
        if closable {
            let close_ink = theme.content(0.50);
            let close_hover = c.content;
            let close_fill = theme.content(0.10);
            slot = slot.child(
                div()
                    .id(("title-tab-close", index))
                    .group(format!("{group}-close"))
                    .absolute()
                    .right(u(4.))
                    .flex()
                    .size(u(20.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.sm))
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .hover(move |s| s.bg(close_fill))
                    .child(
                        icon(IconName::X)
                            .size(u(12.))
                            .text_color(close_ink)
                            .group_hover(format!("title-tab-{index}-close"), move |s| {
                                s.text_color(close_hover)
                            }),
                    )
                    .tooltip(tooltip("Close Tab"))
                    .on_click({
                        let id = tab.id.clone();
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.close_tab(&id, cx)
                        })
                    }),
            );
        }
        slot
    }
}

use gpui::prelude::FluentBuilder as _;
