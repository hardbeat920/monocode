//! The project rail. Port of src/app/shell/ProjectRail.tsx (the 200px rail),
//! src/app/shell/RailAction.tsx, and `CompactProjectRail` in Sidebar.tsx
//! (the 48px rail).

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::widgets::{diff_stat, dot, icon_button, spinner, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::{ResizeTarget, Shell, SidebarTab, WhenMac as _};
use crate::mock::Project;

/// One `RailAction` row: 32px, 16px icon at 70% of the row ink, 14px label.
pub(super) fn rail_action(
    id: &'static str,
    label: &'static str,
    glyph: IconName,
    active: bool,
    trailing: Option<AnyElement>,
    theme: &Theme,
) -> impl IntoElement {
    let c = theme.colors;
    let (ink, fill) = if active {
        (c.content, Some(c.selection))
    } else {
        (theme.content(0.50), None)
    };
    let hover_fill = theme.content(0.10);
    let hover_ink = c.content;
    let mut row = div()
        .id(id)
        .group(id)
        .relative()
        .flex()
        .w_full()
        .items_center()
        .gap(u(8.))
        .px(u(8.))
        .h(u(32.))
        .rounded(u(theme.radius.md))
        .text_color(ink)
        .child(
            icon(glyph)
                .size(u(16.))
                .text_color(with_alpha(ink, 0.7))
                .group_hover(id, move |s| s.text_color(with_alpha(hover_ink, 0.7))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_px(theme.text.ui)
                .medium()
                .leading(theme.leading.tight)
                .child(label),
        )
        .children(trailing);
    if let Some(fill) = fill {
        row = row.bg(fill);
    } else {
        row = row.hover(move |s| s.bg(hover_fill).text_color(hover_ink));
    }
    row
}

/// The shortcut hint at the end of a rail row (`text-[11px] text-content/40`).
pub(super) fn shortcut(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .flex_none()
        .text_px(theme.text.caption)
        .text_color(theme.content(0.40))
        .child(text)
        .into_any_element()
}

/// A stand-in for `ProjectMascot`: the project's tint in a 12px tile.
// TODO(port): draw the pixel mascots from ProjectMascot.tsx.
fn project_mark(project: &Project, size: f32, theme: &Theme) -> AnyElement {
    if project.busy {
        return spinner(gpui::SharedString::from(format!(
            "project-busy-{}",
            project.name
        )))
        .color(theme.colors.accent)
        .into_any_element();
    }
    div()
        .flex_none()
        .size(u(size))
        .rounded(u(3.))
        .bg(hex(project.color))
        .into_any_element()
}

impl Shell {
    pub(super) fn render_project_rail(
        &self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let metrics = theme.metrics;

        let header = self.drag_region(
            div()
                .id("rail-header")
                .flex()
                .flex_none()
                .h(u(metrics.title_bar_height))
                .items_center()
                .pr(u(6.))
                .when_mac(|el| el.child(div().flex_none().w(u(metrics.traffic_light_inset))))
                .child(div().flex_1())
                .child(self.render_visit_nav(true, cx)),
            cx,
        );

        let search = div()
            .id("rail-search")
            .relative()
            .flex()
            .w_full()
            .items_center()
            .gap(u(8.))
            .px(u(6.))
            .h(u(32.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.08))
            .shadow_sm()
            .text_color(theme.content(0.50))
            .hover({
                let fill = theme.content(0.10);
                let ink = c.content;
                move |s| s.bg(fill).text_color(ink)
            })
            .child(
                icon(IconName::Search)
                    .size(u(16.))
                    .text_color(theme.content(0.35)),
            )
            .child(
                div()
                    .flex_1()
                    .text_px(theme.text.ui)
                    .medium()
                    .leading(theme.leading.tight)
                    .child("Search"),
            )
            .child(shortcut("⌘K", &theme));

        let inbox_dot = self.data.inbox_unseen.then(|| dot(8.).into_any_element());
        let actions = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(gpui::px(1.))
            .px(u(8.))
            .pb(u(8.))
            .pt(u(2.))
            .child(search)
            .child(div().mt(u(2.)))
            .child(rail_action(
                "rail-inbox",
                "Inbox",
                IconName::Inbox,
                false,
                inbox_dot,
                &theme,
            ))
            .child(rail_action(
                "rail-notes",
                "Notes",
                IconName::File,
                false,
                None,
                &theme,
            ))
            .child(rail_action(
                "rail-automations",
                "Automations",
                IconName::Zap,
                false,
                None,
                &theme,
            ));

        let mut cards = div().flex().flex_col().gap(gpui::px(1.)).px(u(8.));
        for (index, project) in self.data.projects.iter().enumerate() {
            cards = cards.child(self.render_project_card(index, project, &theme, cx));
        }
        let projects = div()
            .id("rail-projects")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(u(8.))
            .child(
                div()
                    .flex_none()
                    .mb(u(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(4.))
                            .px(u(12.))
                            .pb(u(6.))
                            .pt(u(4.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .px(u(4.))
                                    .text_px(theme.text.label)
                                    .text_color(theme.content(0.50))
                                    .child("Projects"),
                            )
                            .child(
                                icon_button("rail-add-project", IconName::Plus)
                                    .size(20.)
                                    .tooltip("Add project"),
                            ),
                    )
                    .child(cards),
            );

        let footer = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(gpui::px(1.))
            .p(u(8.))
            .child(rail_action(
                "rail-settings",
                "Settings",
                IconName::Settings,
                false,
                Some(shortcut("⌘,", &theme)),
                &theme,
            ));

        div()
            .id("project-rail")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .w(u(self.rail_width))
            .bg(c.sidebar_glass)
            .border_r_1()
            .border_color(c.stroke)
            .child(header)
            .child(actions)
            .child(projects)
            .child(footer)
            .child(self.resize_handle(ResizeTarget::ProjectRail, cx))
    }

    fn render_project_card(
        &self,
        index: usize,
        project: &Project,
        theme: &Theme,
        _: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let selected = index == self.data.active_project;
        let mut card = div()
            .id(("project", index))
            .relative()
            .flex()
            .items_center()
            .gap(u(8.))
            .h(u(32.))
            .px(u(8.))
            .rounded(u(theme.radius.md))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(u(16.))
                    .items_center()
                    .justify_center()
                    .child(project_mark(project, 12., theme)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.ui)
                    .medium()
                    .leading(theme.leading.tight)
                    .child(project.name),
            )
            .child(diff_stat(project.additions, project.deletions).gap(4.));
        if selected {
            card = card.bg(c.selection_strong).text_color(c.content);
        } else {
            let hover = theme.content(0.05);
            card = card
                .opacity(0.65)
                .text_color(c.content)
                .hover(move |s| s.bg(hover).opacity(1.0));
        }
        card.tooltip(tooltip(project.path))
    }

    /// `TabVisitNav`: back, forward, and the project panel toggle.
    pub(super) fn render_visit_nav(
        &self,
        panel_active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(
                icon_button("nav-back", IconName::ChevronLeft)
                    .disabled(true)
                    .tooltip("Back (⌘[)"),
            )
            .child(
                icon_button("nav-forward", IconName::ChevronRight)
                    .disabled(true)
                    .tooltip("Forward (⌘])"),
            )
            .child(
                icon_button("nav-projects", IconName::PanelLeft)
                    .active(panel_active)
                    .tooltip("Toggle Projects")
                    .on_click(cx.listener(Self::toggle_project_rail)),
            )
    }

    /// The 48px rail shown when the project rail is collapsed in compact mode.
    pub(super) fn render_compact_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let metrics = theme.metrics;
        let project = &self.data.projects[self.data.active_project];
        let has_changes = project.additions > 0 || project.deletions > 0;

        let action =
            |id: &'static str, label: &'static str, glyph: IconName, active: bool, dotted: bool| {
                let (ink, fill) = if active {
                    (c.content, Some(c.selection))
                } else {
                    (theme.content(0.50), None)
                };
                let hover_fill = theme.content(0.10);
                let hover_ink = c.content;
                let mut button = div()
                    .id(id)
                    .group(id)
                    .relative()
                    .flex()
                    .flex_none()
                    .size(u(32.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .child(
                        icon(glyph)
                            .size(u(16.))
                            .text_color(ink)
                            .group_hover(id, move |s| s.text_color(hover_ink)),
                    )
                    .tooltip(tooltip(label));
                if let Some(fill) = fill {
                    button = button.bg(fill);
                } else {
                    button = button.hover(move |s| s.bg(hover_fill));
                }
                if dotted {
                    button = button.child(div().absolute().right(u(6.)).top(u(6.)).child(dot(6.)));
                }
                button
            };

        let tab_icon = |tab: SidebarTab| match tab {
            SidebarTab::Sessions => IconName::Chatting,
            SidebarTab::Inbox => IconName::Inbox,
            SidebarTab::Files => IconName::FileScript,
            SidebarTab::Changes => IconName::GitBranch,
        };
        let mut tabs = div().flex().flex_col().items_center().gap(u(6.));
        for tab in SidebarTab::ORDER {
            let id: &'static str = match tab {
                SidebarTab::Sessions => "compact-sessions",
                SidebarTab::Inbox => "compact-inbox-tab",
                SidebarTab::Files => "compact-files",
                SidebarTab::Changes => "compact-changes",
            };
            let active = self.session_sidebar_open && self.sidebar_tab == tab;
            let dotted = tab == SidebarTab::Changes && has_changes;
            tabs = tabs.child(action(id, tab.label(), tab_icon(tab), active, dotted));
        }

        // With the title bar above (macOS compact mode) the rail starts at the
        // top edge and needs no header of its own.
        let title_bar_above = cfg!(target_os = "macos");
        let header = (!title_bar_above).then(|| {
            self.drag_region(
                div()
                    .id("compact-header")
                    .flex_none()
                    .w_full()
                    .h(u(metrics.title_bar_height))
                    .border_b_1()
                    .border_color(c.stroke),
                cx,
            )
        });

        let expand = action(
            "compact-expand",
            "Expand projects",
            IconName::PanelLeft,
            false,
            false,
        )
        .on_click(cx.listener(Self::toggle_project_rail));
        let project_button = div()
            .id("compact-project")
            .flex()
            .flex_none()
            .size(u(32.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.md))
            .hover({
                let fill = theme.content(0.10);
                move |s| s.bg(fill)
            })
            .child(project_mark(project, 14., &theme))
            .tooltip(tooltip(project.name));

        div()
            .id("compact-rail")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .h_full()
            .w(u(metrics.compact_rail_width))
            .bg(c.sidebar_glass)
            .children(header)
            .child(
                div()
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .top(u(if title_bar_above {
                        0.
                    } else {
                        metrics.title_bar_height
                    }))
                    .w(gpui::px(1.))
                    .bg(c.stroke),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w_full()
                    .items_center()
                    .gap(u(6.))
                    .py(u(6.))
                    .child(expand)
                    .child(project_button)
                    .child(tabs)
                    .child(action(
                        "compact-search",
                        "Search (⌘K)",
                        IconName::Search,
                        false,
                        false,
                    ))
                    .child(action(
                        "compact-inbox",
                        "Inbox",
                        IconName::Inbox,
                        false,
                        self.data.inbox_unseen,
                    ))
                    .child(action(
                        "compact-notes",
                        "Notes",
                        IconName::StickyNote,
                        false,
                        false,
                    ))
                    .child(action(
                        "compact-automations",
                        "Automations",
                        IconName::Zap,
                        false,
                        false,
                    )),
            )
            .child(div().flex_1().min_h(u(8.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .items_center()
                    .gap(u(4.))
                    .py(u(6.))
                    .child(action(
                        "compact-settings",
                        "Settings (⌘,)",
                        IconName::Settings,
                        false,
                        false,
                    )),
            )
    }
}
