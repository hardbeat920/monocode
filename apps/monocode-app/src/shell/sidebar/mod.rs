//! The session sidebar. Port of the `sidebarContent` block in
//! src/app/shell/Sidebar.tsx: the Workspace header, the project picker row
//! when the rail is closed, the Sessions / Inbox / Explorer / Changes tabs,
//! the tab content, and the footer. The Sessions tab is `SessionList`
//! (`session_list/`); `CompactProjectRail` is `CompactRail`
//! (`compact_rail.rs`).

mod compact_rail;
mod session_list;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div,
};
use monocode_ui::widgets::{diff_stat, icon_button};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

pub use compact_rail::CompactRail;
pub use session_list::SessionList;

use super::project_rail::{Project, rail_projects};
use super::rail_action::{rail_action, shortcut};
use super::title_bar::tab_visit_nav;
use super::{
    ResizeTarget, Shell, ShellLayout, SidebarTab, WhenMac as _, drag_region, resize_handle,
};

/// What the sidebar frame draws for one frame.
#[derive(Clone, Debug, Default)]
pub struct SidebarData {
    pub projects: Vec<Project>,
    pub active_project: Option<usize>,
}

/// The session sidebar of one window.
pub struct SessionSidebar {
    shell: WeakEntity<Shell>,
    session_list: Entity<SessionList>,
}

impl SessionSidebar {
    pub(super) fn has_open_overlay(&self, cx: &App) -> bool {
        self.session_list.read(cx).has_open_overlay()
    }

    pub fn new(
        shell: WeakEntity<Shell>,
        demo_menu: Option<(f32, f32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_list = cx.new(|cx| SessionList::new(shell.clone(), demo_menu, window, cx));
        Self {
            shell,
            session_list,
        }
    }

    fn with_shell(&self, cx: &mut App, f: impl FnOnce(&mut Shell, &mut Context<Shell>)) {
        self.shell.update(cx, f).ok();
    }

    fn data(&self, cx: &mut App) -> SidebarData {
        let Some(shell) = self.shell.upgrade() else {
            return SidebarData::default();
        };
        let cwd = shell.read(cx).sidebar_cwd(cx);
        let (projects, active_project) = rail_projects(&cwd, cx);
        SidebarData {
            projects,
            active_project,
        }
    }
}

impl Render for SessionSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        let layout = shell.read(cx).layout.clone();
        let data = self.data(cx);
        self.render_sidebar(&layout, &data, window, cx)
            .into_any_element()
    }
}

impl SessionSidebar {
    fn render_sidebar(
        &self,
        layout: &ShellLayout,
        data: &SidebarData,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let metrics = theme.metrics;
        let compact_title_bar = layout.compact_title_bar();

        let mut pane = div()
            .id("session-sidebar")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .min_h_0()
            .w(u(layout.sidebar_width))
            .bg(c.sidebar_pane)
            .border_r_1()
            .border_color(c.stroke);

        if layout.project_rail_open {
            let header = drag_region(
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
                                    .tooltip("Go to File (⌘P)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.shell
                                            .update(cx, |shell, cx| {
                                                shell.open_file_picker(false, window, cx)
                                            })
                                            .ok();
                                    })),
                            )
                            .child(
                                icon_button("sidebar-new", IconName::Plus)
                                    .tooltip("New session (⌘T)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.with_shell(cx, |shell, cx| {
                                            shell.new_session(cx);
                                        })
                                    })),
                            ),
                    ),
                self.shell.clone(),
            );
            pane = pane
                .child(header)
                .child(self.render_sidebar_tabs(layout, data, &theme, cx));
        } else if !compact_title_bar {
            let header = drag_region(
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
                    .child(tab_visit_nav(
                        &self.shell,
                        (!layout.compact_rail).then_some(false),
                        cx,
                    )),
                self.shell.clone(),
            );
            pane = pane.child(header);
            if !layout.compact_rail_visible() {
                pane = pane
                    .child(self.render_project_picker_row(data, &theme, cx))
                    .child(self.render_sidebar_tabs(layout, data, &theme, cx));
            }
        }

        let slot = crate::slots::AppSlots::get(cx).sidebar_tab;
        let body: AnyElement = match layout.sidebar_tab {
            SidebarTab::Sessions => self.session_list.clone().into_any_element(),
            other => match slot.as_ref().and_then(|slot| slot(other, _window, cx)) {
                Some(view) => div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(view)
                    .into_any_element(),
                None => div()
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child(format!("{} lands with its feature port.", other.label()))
                    .into_any_element(),
            },
        };

        // `showSidebarFooter`: with the project rail closed, Settings moves
        // here (the compact rail has its own).
        let footer = (!layout.project_rail_open).then(|| {
            let mut footer = div()
                .flex()
                .flex_col()
                .flex_none()
                .p(u(8.))
                .children(
                    self.shell
                        .upgrade()
                        .map(|shell| shell.read(cx).live_agents.clone()),
                )
                .child(super::sidebar_update::view(cx))
                .child(super::github_star::view(cx));
            if !layout.compact_rail {
                footer = footer.child(
                    rail_action(
                        "sidebar-settings",
                        "Settings",
                        IconName::Settings,
                        false,
                        Some(shortcut("⌘,", &theme)),
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |shell, cx| {
                            shell.toggle_page(crate::slots::Page::Settings, cx)
                        })
                    })),
                );
            }
            footer
        });
        pane.child(body).children(footer).child(resize_handle(
            ResizeTarget::SessionSidebar,
            self.shell.clone(),
            cx,
        ))
    }

    /// The project picker row shown when the project rail is closed.
    fn render_project_picker_row(
        &self,
        data: &SidebarData,
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
            )
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                this.shell
                    .update(cx, |shell, cx| {
                        shell.show_project_picker(event.position(), window, cx)
                    })
                    .ok();
            }));
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |shell, cx| {
                            shell.new_session(cx);
                        })
                    })),
            )
            .child(
                icon_button("picker-search", IconName::Search)
                    .tooltip("Search (⌘K)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |shell, cx| {
                            shell.toggle_page(crate::slots::Page::Search, cx)
                        })
                    })),
            )
            .child(
                icon_button("picker-inbox", IconName::Inbox)
                    .tooltip("Inbox")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |shell, cx| {
                            shell.toggle_page(crate::slots::Page::Inbox, cx)
                        })
                    })),
            )
            .child(
                icon_button("picker-automations", IconName::Zap)
                    .tooltip("Automations")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |shell, cx| {
                            shell.toggle_page(crate::slots::Page::Automations, cx)
                        })
                    })),
            )
    }

    /// The workspace tab strip (`h-9`, four equal 24px tabs).
    fn render_sidebar_tabs(
        &self,
        layout: &ShellLayout,
        data: &SidebarData,
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
            let active = layout.sidebar_tab == tab;
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
                    this.with_shell(cx, |shell, cx| shell.set_sidebar_tab(tab, cx));
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
}

use gpui::prelude::FluentBuilder as _;
