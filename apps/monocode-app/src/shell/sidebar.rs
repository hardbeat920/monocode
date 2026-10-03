//! The session sidebar. Port of the `sidebarContent` block and `SessionCard`
//! in src/app/shell/Sidebar.tsx: the Workspace header, the Sessions / Inbox /
//! Explorer / Changes tabs, the session search row, and the session cards.

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_ui::widgets::{diff_stat, icon_button, spinner, text_field};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::{ResizeTarget, Shell, SidebarTab, WhenMac as _};
use crate::format::{format_relative, now_ms};
use crate::view_data::{SessionCard, SessionStatus, ShellData};

impl Shell {
    pub(super) fn render_sidebar(
        &self,
        data: &ShellData,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let metrics = theme.metrics;
        let compact_title_bar = cfg!(target_os = "macos") && self.compact_rail_visible();

        let mut pane = div()
            .id("session-sidebar")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .min_h_0()
            .w(u(self.sidebar_width))
            .bg(c.sidebar_pane)
            .border_r_1()
            .border_color(c.stroke);

        if self.project_rail_open {
            let header = self.drag_region(
                div()
                    .id("sidebar-header")
                    .flex()
                    .flex_none()
                    .h(u(metrics.title_bar_height))
                    .items_center()
                    .gap(u(4.))
                    .pl(u(12.))
                    .pr(u(6.))
                    .border_b_1()
                    .border_color(c.stroke)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.ui)
                            .medium()
                            .leading(theme.leading.tight)
                            .child("Workspace"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(u(2.))
                            .child(
                                icon_button("sidebar-goto", IconName::Search)
                                    .tooltip("Go to File (⌘P)"),
                            )
                            .child(
                                icon_button("sidebar-new", IconName::Plus)
                                    .tooltip("New session (⌘T)")
                                    .on_click(cx.listener(|this, _, _, cx| this.new_session(cx))),
                            ),
                    ),
                cx,
            );
            pane = pane
                .child(header)
                .child(self.render_sidebar_tabs(data, &theme, cx));
        } else if !compact_title_bar {
            let header = self.drag_region(
                div()
                    .id("sidebar-header")
                    .flex()
                    .flex_none()
                    .h(u(metrics.title_bar_height))
                    .items_center()
                    .pr(u(6.))
                    .border_b_1()
                    .border_color(c.stroke)
                    .when_mac(|el| el.child(div().flex_none().w(u(metrics.traffic_light_inset))))
                    .child(div().flex_1())
                    .child(self.render_visit_nav(false, cx)),
                cx,
            );
            pane = pane.child(header);
            if !self.compact_rail_visible() {
                pane = pane
                    .child(self.render_project_picker_row(data, &theme, cx))
                    .child(self.render_sidebar_tabs(data, &theme, cx));
            }
        }

        let body: AnyElement = match self.sidebar_tab {
            SidebarTab::Sessions => self
                .render_session_list(data, &theme, cx)
                .into_any_element(),
            other => div()
                .px(u(12.))
                .py(u(8.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .child(format!("{} lands with its feature port.", other.label()))
                .into_any_element(),
        };

        // `showSidebarFooter`: with the project rail closed, Settings moves
        // here (the compact rail has its own).
        let footer =
            (!self.project_rail_open && !self.compact_rail).then(|| {
                div().flex().flex_col().flex_none().p(u(8.)).child(
                    super::project_rail::rail_action(
                        "sidebar-settings",
                        "Settings",
                        IconName::Settings,
                        false,
                        Some(super::project_rail::shortcut("⌘,", &theme)),
                        &theme,
                    ),
                )
            });
        pane.child(body)
            .children(footer)
            .child(self.resize_handle(ResizeTarget::SessionSidebar, cx))
    }

    /// The project picker row shown when the project rail is closed.
    fn render_project_picker_row(
        &self,
        data: &ShellData,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let (name, color) = data
            .active_project
            .and_then(|index| data.projects.get(index))
            .map(|project| (project.name.clone(), project.color))
            .unwrap_or_else(|| ("~".to_string(), 0x7dd3fc));
        let picker = div()
            .id("sidebar-project-picker")
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .h(u(26.))
            .px(u(8.))
            .rounded(u(theme.radius.md))
            .text_px(theme.text.label)
            .leading(theme.leading.none)
            .hover({
                let fill = theme.content(0.05);
                move |s| s.bg(fill)
            })
            .child(
                div()
                    .flex_none()
                    .size(u(12.))
                    .rounded(u(3.))
                    .bg(monocode_ui::color::hex(color)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .medium()
                    .text_color(theme.content(0.90))
                    .child(name),
            )
            .child(
                icon(IconName::ChevronDown)
                    .size(u(12.))
                    .text_color(theme.content(0.45)),
            );
        div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(2.))
            .px(u(8.))
            .border_b_1()
            .border_color(c.stroke)
            .child(div().flex_1().min_w_0().child(picker))
            .child(
                icon_button("picker-new", IconName::Plus)
                    .tooltip("New tab (⌘T)")
                    .on_click(cx.listener(|this, _, _, cx| this.new_session(cx))),
            )
            .child(icon_button("picker-search", IconName::Search).tooltip("Search (⌘K)"))
            .child(icon_button("picker-inbox", IconName::Inbox).tooltip("Inbox"))
            .child(icon_button("picker-automations", IconName::Zap).tooltip("Automations"))
    }

    /// The workspace tab strip (`h-9`, four equal 24px tabs).
    fn render_sidebar_tabs(
        &self,
        data: &ShellData,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let (additions, deletions) = data
            .active_project
            .and_then(|index| data.projects.get(index))
            .map(|project| (project.additions, project.deletions))
            .unwrap_or_default();
        let mut row = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(gpui::px(1.))
            .px(u(8.))
            .border_b_1()
            .border_color(c.stroke);
        for tab in SidebarTab::ORDER {
            let active = self.sidebar_tab == tab;
            let has_stats = tab == SidebarTab::Changes && (additions > 0 || deletions > 0);
            let label: AnyElement = if has_stats {
                diff_stat(additions, deletions).into_any_element()
            } else {
                div()
                    .truncate()
                    .leading(theme.leading.label)
                    .child(tab.label())
                    .into_any_element()
            };
            let mut button = div()
                .id(tab.label())
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .h(u(24.))
                .items_center()
                // Large counts do not fit; keep their sign in view.
                .when(has_stats, |el| el.justify_start())
                .when(!has_stats, |el| el.justify_center())
                .px(u(8.))
                .rounded(u(theme.radius.md))
                .text_px(theme.text.label)
                .leading(theme.leading.none)
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.sidebar_tab = tab;
                    cx.notify();
                }));
            if active {
                button = button.bg(c.selection).text_color(c.content);
            } else {
                let fill = theme.content(0.05);
                let ink = c.content;
                button = button
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.bg(fill).text_color(ink));
            }
            row = row.child(button);
        }
        row
    }

    fn render_session_list(
        &self,
        data: &ShellData,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let search_row = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(4.))
            .px(u(8.))
            .border_b_1()
            .border_color(c.stroke)
            .child(text_field(&self.session_search).icon(IconName::Search))
            .child(
                icon_button("sessions-filter", IconName::ListFilter)
                    .size(24.)
                    .icon_size(12.)
                    .tooltip("Filter sessions"),
            );
        let mut list = div().flex().flex_col().gap(u(2.)).p(u(6.));
        let now = now_ms();
        for (index, session) in data.sessions.iter().enumerate() {
            let active = data.active_session_id.as_deref() == Some(session.id.as_str());
            list = list.child(self.render_session_card(index, session, active, now, theme, cx));
        }
        if data.sessions.is_empty() && !data.sessions_loading {
            list = list.child(
                div()
                    .px(u(6.))
                    .py(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("No sessions yet"),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(search_row)
            .child(
                div()
                    .id("session-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
    }

    /// `SessionCard`: provider and model, title, branch, diff stats, and a
    /// status or relative time.
    fn render_session_card(
        &self,
        index: usize,
        session: &SessionCard,
        is_active: bool,
        now: i64,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let is_selected = self.menu_session.as_deref() == Some(session.id.as_str())
            && self.session_menu.is_some();
        let needs_approval = session.status == SessionStatus::NeedsApproval;
        let draft = session.status == SessionStatus::Draft;

        let status: AnyElement = {
            let row = div()
                .flex()
                .flex_none()
                .items_center()
                .gap(u(4.))
                .text_px(theme.text.caption)
                .tabular();
            match session.status {
                SessionStatus::NeedsApproval => row
                    .text_color(c.warning)
                    .child(
                        icon(IconName::CircleAlert)
                            .size(u(12.))
                            .text_color(c.warning),
                    )
                    .child("Need approval")
                    .into_any_element(),
                SessionStatus::Busy => row
                    .text_color(c.accent)
                    .child(spinner(("session-spinner", index)))
                    .child("Working...")
                    .into_any_element(),
                SessionStatus::Done => row
                    .text_color(c.success)
                    .child(icon(IconName::Check).size(u(12.)).text_color(c.success))
                    .child("Done")
                    .into_any_element(),
                SessionStatus::Draft => row
                    .text_color(theme.content(0.55))
                    .child(
                        icon(IconName::CircleDashed)
                            .size(u(12.))
                            .text_color(theme.content(0.55)),
                    )
                    .child("Draft")
                    .into_any_element(),
                SessionStatus::Idle => row
                    .text_color(theme.content(0.45))
                    .child(format_relative(session.updated_at, now))
                    .into_any_element(),
            }
        };

        let header = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .child(provider_logo(session.provider).size(14.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.50))
                            .child(session.model.clone()),
                    ),
            )
            .child(status);
        let title = div()
            .mt(u(4.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .when(session.pinned, |el| {
                el.child(
                    icon(IconName::Pin)
                        .size(u(12.))
                        .text_color(theme.content(0.45)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.body)
                    .semibold()
                    .leading(theme.leading.snug)
                    .text_color(c.content)
                    .child(session.title.clone()),
            );
        let branch = session.git.clone();
        let footer = div()
            .mt(u(4.))
            .flex()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .child(
                        icon(IconName::GitBranch)
                            .size(u(12.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(div().min_w_0().truncate().child(branch)),
            )
            .child(diff_stat(session.additions, session.deletions));

        let mut card = div()
            .id(("session", index))
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .px(u(10.))
            .py(u(8.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(gpui::transparent_black())
            .child(header)
            .child(title)
            .child(footer)
            .on_click({
                let id = session.id.clone();
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_session(&id, cx);
                    cx.notify();
                })
            })
            .on_mouse_down(MouseButton::Right, {
                let id = session.id.clone();
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.session_menu = Some(event.position);
                    this.menu_session = Some(id.clone());
                    cx.notify();
                })
            });
        if is_selected {
            card = card.bg(theme.accent(0.15)).text_color(c.content);
        } else if needs_approval {
            card = card
                .bg(theme.content(0.20))
                .border_dashed()
                .border_color(theme.content(0.30));
        } else if is_active {
            card = card.bg(c.selection);
        } else if draft {
            card = card.border_dashed().border_color(theme.content(0.25));
        } else {
            let hover = theme.content(0.05);
            card = card.hover(move |s| s.bg(hover));
        }
        card
    }
}

use gpui::prelude::FluentBuilder as _;
