//! `CompactProjectRail` from src/app/shell/Sidebar.tsx: the 48px rail shown
//! when the project rail is collapsed in compact mode.

use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div,
};
use monocode_engine::inbox::inbox::Inbox;
use monocode_ui::widgets::{dot, tooltip};
use monocode_ui::{IconName, Theme, icon, u};

use crate::shell::project_rail::{Project, inbox_unseen, project_mark, rail_projects};
use crate::shell::{Shell, ShellLayout, SidebarTab, drag_region};

/// The compact rail of one window.
pub struct CompactRail {
    shell: WeakEntity<Shell>,
}

impl CompactRail {
    pub fn new(shell: WeakEntity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Some(inbox) = Inbox::try_global(cx) {
            cx.observe(&inbox, |_, _, cx| cx.notify()).detach();
        }
        Self { shell }
    }

    fn with_shell(&self, cx: &mut App, f: impl FnOnce(&mut Shell, &mut Context<Shell>)) {
        self.shell.update(cx, f).ok();
    }
}

impl Render for CompactRail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        let (layout, cwd) = {
            let shell = shell.read(cx);
            (shell.layout.clone(), shell.sidebar_cwd(cx))
        };
        let (projects, active) = rail_projects(&cwd, cx);
        self.render_compact_rail(&layout, &projects, active, cx)
            .into_any_element()
    }
}

impl CompactRail {
    fn render_compact_rail(
        &self,
        layout: &ShellLayout,
        projects: &[Project],
        active_project: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let metrics = theme.metrics;
        let project = active_project.and_then(|index| projects.get(index));
        let has_changes =
            project.is_some_and(|project| project.additions > 0 || project.deletions > 0);
        let inbox_unseen = inbox_unseen(cx);

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
            let active = layout.session_sidebar_open && layout.sidebar_tab == tab;
            let dotted = tab == SidebarTab::Changes && has_changes;
            tabs = tabs.child(
                action(id, tab.label(), tab_icon(tab), active, dotted).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.with_shell(cx, |shell, cx| {
                            shell.set_sidebar_tab(tab, cx);
                            shell.set_session_sidebar_open(true, cx);
                        })
                    },
                )),
            );
        }

        // With the title bar above (macOS compact mode) the rail starts at the
        // top edge and needs no header of its own.
        let title_bar_above = cfg!(target_os = "macos");
        let header = (!title_bar_above).then(|| {
            drag_region(
                div()
                    .id("compact-header")
                    .flex_none()
                    .w_full()
                    .h(u(metrics.title_bar_height))
                    .border_b_1()
                    .border_color(c.stroke),
                self.shell.clone(),
            )
        });

        let expand = action(
            "compact-expand",
            "Expand projects",
            IconName::PanelLeft,
            false,
            false,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.with_shell(cx, |shell, cx| shell.toggle_project_rail(cx))
        }));
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
            .tooltip(tooltip(
                project
                    .map(|project| project.name.clone())
                    .unwrap_or_default(),
            ))
            .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
                this.shell
                    .update(cx, |shell, cx| {
                        shell.show_project_picker(event.position(), window, cx)
                    })
                    .ok();
            }))
            .on_mouse_down(gpui::MouseButton::Right, {
                let project = project.map(|project| project.path.clone());
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    if let Some(project) = &project {
                        cx.stop_propagation();
                        this.shell
                            .update(cx, |shell, cx| {
                                shell.show_project_menu(project, event.position, window, cx)
                            })
                            .ok();
                    }
                })
            });

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
                    .child(
                        action(
                            "compact-search",
                            "Search (⌘K)",
                            IconName::Search,
                            false,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.with_shell(cx, |shell, cx| {
                                shell.open_page(crate::slots::Page::Search, cx)
                            })
                        })),
                    )
                    .child(
                        action(
                            "compact-inbox",
                            "Inbox",
                            IconName::Inbox,
                            false,
                            inbox_unseen,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.with_shell(cx, |shell, cx| {
                                shell.open_page(crate::slots::Page::Inbox, cx)
                            })
                        })),
                    )
                    .child(
                        action("compact-notes", "Notes", IconName::StickyNote, false, false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.with_shell(cx, |shell, cx| {
                                    shell.open_page(crate::slots::Page::Notes, cx)
                                })
                            })),
                    )
                    .child(
                        action(
                            "compact-automations",
                            "Automations",
                            IconName::Zap,
                            false,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.with_shell(cx, |shell, cx| {
                                shell.open_page(crate::slots::Page::Automations, cx)
                            })
                        })),
                    ),
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
                    .child(
                        action(
                            "compact-settings",
                            "Settings (⌘,)",
                            IconName::Settings,
                            false,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.with_shell(cx, |shell, cx| {
                                shell.open_page(crate::slots::Page::Settings, cx)
                            })
                        })),
                    ),
            )
    }
}
