//! The project rail. Port of src/app/shell/ProjectRail.tsx (the 200px
//! rail). The compact 48px rail lives with the sidebar, as
//! `CompactProjectRail` did in Sidebar.tsx.

use gpui::{
    AnyElement, App, AppContext as _, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    WeakEntity, Window, div,
};
use monocode_engine::inbox::inbox::Inbox;
use monocode_engine::projects::{ProjectsGlobal, project_groups::ProjectGroup};
use monocode_engine::runtime::Engine;
use monocode_engine::runtime::util::project_path::same_project_path;
use monocode_layout::paths::{project_key, project_name};
use monocode_layout::tab_groups::{JsRecord, resolve_tab_group_color};
use monocode_ui::color::hex;
use monocode_ui::widgets::{diff_stat, dot, icon_button, spinner, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::rail_action::{rail_action, shortcut};
use super::title_bar::tab_visit_nav;
use super::{ResizeTarget, Shell, WhenMac as _, drag_region, resize_handle};
use crate::slots::Page;

/// One project card.
#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
    pub busy: bool,
    /// The tint (`resolveTabGroupColor`), as `0xrrggbb`.
    pub color: u32,
    pub logo: Option<String>,
    pub mascot: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RailSection {
    Pinned,
    Groups,
    Projects,
}

impl RailSection {
    fn label(self) -> &'static str {
        match self {
            Self::Pinned => "Pinned",
            Self::Groups => "Groups",
            Self::Projects => "Projects",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RailRow {
    Section(RailSection),
    Group(usize),
    Project(usize),
}

struct RailGrouping<'a> {
    groups: &'a [ProjectGroup],
    assignments: &'a JsRecord<String>,
    pinned: &'a [String],
}

#[derive(PartialEq, Eq)]
enum ProjectSection<'a> {
    Pinned,
    Group(&'a str),
    Ungrouped,
}

impl RailGrouping<'_> {
    fn section(&self, path: &str) -> ProjectSection<'_> {
        let key = monocode_core::paths::path_key(path);
        if self
            .pinned
            .iter()
            .any(|path| monocode_core::paths::path_key(path) == key)
        {
            ProjectSection::Pinned
        } else if let Some(id) = self.assignments.get(&key)
            && self.groups.iter().any(|group| &group.id == id)
        {
            ProjectSection::Group(id)
        } else {
            ProjectSection::Ungrouped
        }
    }
}

struct RailProjectDrag {
    path: String,
}

fn reorder_rail_project(
    order: &[String],
    moved: &str,
    target: &str,
    after: bool,
    grouping: &RailGrouping<'_>,
) -> Option<Vec<String>> {
    if same_project_path(moved, target) || grouping.section(moved) != grouping.section(target) {
        return None;
    }
    let section = grouping.section(target);
    let mut subset: Vec<String> = order
        .iter()
        .filter(|path| grouping.section(path) == section)
        .cloned()
        .collect();
    let from = subset
        .iter()
        .position(|path| same_project_path(path, moved))?;
    let moved = subset.remove(from);
    let destination = subset
        .iter()
        .position(|path| same_project_path(path, target))?
        + usize::from(after);
    subset.insert(destination, moved);
    let mut subset = subset.into_iter();
    let next: Vec<String> = order
        .iter()
        .map(|path| {
            if grouping.section(path) == section {
                subset.next().unwrap_or_else(|| path.clone())
            } else {
                path.clone()
            }
        })
        .collect();
    (next != order).then_some(next)
}

fn rail_rows(
    projects: &[Project],
    groups: &[ProjectGroup],
    assignments: &JsRecord<String>,
    pinned: &[String],
) -> Vec<RailRow> {
    let pinned: std::collections::HashSet<_> = pinned
        .iter()
        .map(|path| monocode_core::paths::path_key(path))
        .collect();
    let mut pinned_rows = Vec::new();
    let mut group_rows = vec![Vec::new(); groups.len()];
    let mut ungrouped = Vec::new();
    for (index, project) in projects.iter().enumerate() {
        let key = monocode_core::paths::path_key(&project.path);
        let row = RailRow::Project(index);
        if pinned.contains(&key) {
            pinned_rows.push(row);
        } else if let Some(group_index) = assignments
            .get(&key)
            .and_then(|id| groups.iter().position(|group| &group.id == id))
        {
            group_rows[group_index].push(row);
        } else {
            ungrouped.push(row);
        }
    }
    let mut rows = Vec::new();
    if !pinned_rows.is_empty() {
        rows.push(RailRow::Section(RailSection::Pinned));
        rows.extend(pinned_rows);
    }
    if !groups.is_empty() {
        rows.push(RailRow::Section(RailSection::Groups));
        for (index, group) in groups.iter().enumerate() {
            rows.push(RailRow::Group(index));
            if !group.collapsed {
                rows.append(&mut group_rows[index]);
            }
        }
    }
    rows.push(RailRow::Section(RailSection::Projects));
    rows.extend(ungrouped);
    rows
}

fn parse_color(value: &str) -> u32 {
    monocode_view_workbench::panes::tab_group_menu::parse_css_color(value)
        .map(|color| {
            let color: gpui::Rgba = color.into();
            ((color.r * 255.0).round() as u32) << 16
                | ((color.g * 255.0).round() as u32) << 8
                | (color.b * 255.0).round() as u32
        })
        .unwrap_or(0x7dd3fc)
}

pub(super) fn inbox_unseen(cx: &App) -> bool {
    Inbox::try_global(cx).is_some_and(|inbox| inbox.read(cx).unseen())
}

/// The rail's projects, pinned first, and the index of the sidebar's one.
pub fn rail_projects(cwd: &str, cx: &mut App) -> (Vec<Project>, Option<usize>) {
    let Some(global) = ProjectsGlobal::try_global(cx) else {
        return (Vec::new(), None);
    };
    let projects = global.projects.clone();
    let git = global.git.clone();
    let (items, labels, colors, custom, logos, mascots) = projects.update(cx, |projects, _| {
        (
            projects.rail_items(cwd),
            projects.labels(),
            projects.colors(),
            projects.custom_colors(),
            projects.logos(),
            projects.mascots(),
        )
    });
    let sessions = Engine::sessions(cx).read(cx).all().to_vec();
    let list: Vec<Project> = items
        .into_iter()
        .map(|item| {
            let key = project_key(&item.path);
            let seed = project_name(&item.path);
            let stats = git
                .read(cx)
                .get(&item.path)
                .and_then(|status| status.read(cx).diff_stats().cloned());
            Project {
                name: labels
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| project_name(&item.path)),
                logo: logos.get(&key).cloned(),
                mascot: mascots.get(&key).cloned(),
                color: parse_color(&resolve_tab_group_color(
                    &key,
                    Some(&colors),
                    Some(&custom),
                    Some(&seed),
                )),
                busy: sessions.iter().any(|session| {
                    session.is_busy() && same_project_path(&session.cwd, &item.path)
                }),
                additions: stats.as_ref().map_or(0, |stats| stats.additions),
                deletions: stats.as_ref().map_or(0, |stats| stats.deletions),
                path: item.path,
            }
        })
        .collect();
    let active = list
        .iter()
        .position(|project| same_project_path(&project.path, cwd));
    (list, active)
}

/// The project rail of one window.
pub struct ProjectRail {
    shell: WeakEntity<Shell>,
    git_watches: std::collections::HashMap<
        String,
        (
            monocode_engine::projects::git_status::GitWatch,
            gpui::Subscription,
        ),
    >,
}

impl ProjectRail {
    pub fn new(shell: WeakEntity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Some(global) = ProjectsGlobal::try_global(cx) {
            let projects = global.projects.clone();
            cx.observe(&projects, |_, _, cx| cx.notify()).detach();
        }
        if let Some(inbox) = Inbox::try_global(cx) {
            cx.observe(&inbox, |_, _, cx| cx.notify()).detach();
        }
        Self {
            shell,
            git_watches: Default::default(),
        }
    }

    fn with_shell(&self, cx: &mut App, f: impl FnOnce(&mut Shell, &mut Context<Shell>)) {
        self.shell.update(cx, f).ok();
    }

    fn reorder_project(&self, moved: &str, target: &str, after: bool, cx: &mut App) {
        let Some(shell) = self.shell.upgrade() else {
            return;
        };
        let cwd = shell.read(cx).sidebar_cwd(cx);
        ProjectsGlobal::projects(cx).update(cx, |projects, cx| {
            let known =
                monocode_engine::projects::recents::collect_rail_projects(projects.recents(), &cwd);
            let order = monocode_engine::projects::recents::sync_project_rail_order(
                projects.rail_order(),
                &known,
            );
            let grouping = RailGrouping {
                groups: projects.groups(),
                assignments: projects.assignments(),
                pinned: projects.pinned(),
            };
            if let Some(order) = reorder_rail_project(&order, moved, target, after, &grouping) {
                projects.save_rail_order(&order, cx);
            }
        });
    }
}

impl Render for ProjectRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        let (cwd, page, width) = {
            let shell = shell.read(cx);
            (
                shell.sidebar_cwd(cx),
                shell.layout.page,
                shell.layout.rail_width,
            )
        };
        let (projects, active) = rail_projects(&cwd, cx);
        self.git_watches
            .retain(|path, _| projects.iter().any(|project| &project.path == path));
        if let Some(global) = ProjectsGlobal::try_global(cx) {
            let git = global.git.clone();
            for project in &projects {
                if self.git_watches.contains_key(&project.path) {
                    continue;
                }
                let status = git.update(cx, |git, cx| git.status(&project.path, cx));
                let watch = status.update(cx, |status, cx| {
                    status.watch(
                        monocode_engine::projects::git_status::WatchKind::DiffStats,
                        cx,
                    )
                });
                let subscription = cx.observe(&status, |_, _, cx| cx.notify());
                self.git_watches
                    .insert(project.path.clone(), (watch, subscription));
            }
        }
        self.render_project_rail(&projects, active, page, width, window, cx)
            .into_any_element()
    }
}

/// Saved project logos fall back to the project's pixel mascot.
pub(super) fn project_mark(project: Option<&Project>, size: f32, theme: &Theme) -> AnyElement {
    let Some(project) = project else {
        return icon(IconName::Folder)
            .size(u(size))
            .text_color(theme.content(0.55))
            .into_any_element();
    };
    if project.busy {
        return spinner(gpui::SharedString::from(format!(
            "rail-busy-{}",
            project.path
        )))
        .color(theme.colors.accent)
        .into_any_element();
    }
    if let Some(logo) = &project.logo {
        return gpui::img(std::path::PathBuf::from(logo))
            .size(u(size))
            .object_fit(gpui::ObjectFit::Contain)
            .into_any_element();
    }
    let mascot = monocode_view_workbench::panes::pixel_art::project_mascot(
        &monocode_layout::paths::project_name(&project.path),
        project.mascot.as_deref(),
    );
    monocode_view_workbench::panes::pixel_art::sprite(mascot.rest, hex(project.color))
        .size(u(size))
        .into_any_element()
}

impl ProjectRail {
    fn render_project_rail(
        &self,
        projects: &[Project],
        active: Option<usize>,
        page: Option<Page>,
        width: f32,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let metrics = theme.metrics;

        let header = drag_region(
            div()
                .id("rail-header")
                .flex()
                .flex_none()
                .h(u(metrics.title_bar_height))
                .items_center()
                .pr(u(6.))
                .when_mac(|el| el.child(div().flex_none().w(u(metrics.traffic_light_inset))))
                .child(div().flex_1())
                .child(tab_visit_nav(&self.shell, Some(true), cx)),
            self.shell.clone(),
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
            .child(shortcut("⌘K", &theme))
            .on_click(cx.listener(|this, _, _, cx| {
                this.with_shell(cx, |shell, cx| shell.toggle_page(Page::Search, cx))
            }));

        let inbox_dot = inbox_unseen(cx).then(|| dot(8.).into_any_element());
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
            .child(
                rail_action(
                    "rail-inbox",
                    "Inbox",
                    IconName::Inbox,
                    page == Some(Page::Inbox),
                    inbox_dot,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.with_shell(cx, |shell, cx| shell.toggle_page(Page::Inbox, cx))
                })),
            )
            .child(
                rail_action(
                    "rail-notes",
                    "Notes",
                    IconName::File,
                    page == Some(Page::Notes),
                    None,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.with_shell(cx, |shell, cx| shell.toggle_page(Page::Notes, cx))
                })),
            )
            .child(
                rail_action(
                    "rail-automations",
                    "Automations",
                    IconName::Zap,
                    page == Some(Page::Automations),
                    None,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.with_shell(cx, |shell, cx| shell.toggle_page(Page::Automations, cx))
                })),
            );

        let (groups, assignments, pinned) = ProjectsGlobal::try_global(cx)
            .map(|global| {
                let model = global.projects.read(cx);
                (
                    model.groups().to_vec(),
                    model.assignments().clone(),
                    model.pinned().to_vec(),
                )
            })
            .unwrap_or_default();
        let rows = rail_rows(projects, &groups, &assignments, &pinned);
        let mut cards = div().flex().flex_col().gap(gpui::px(1.));
        for row in rows {
            cards = match row {
                RailRow::Section(section) => {
                    cards.child(self.render_section_header(section, &theme, cx))
                }
                RailRow::Group(index) => cards.child(
                    div()
                        .px(u(8.))
                        .child(self.render_group_header(&groups[index], &theme, cx)),
                ),
                RailRow::Project(index) => {
                    cards.child(div().px(u(8.)).child(self.render_project_card(
                        index,
                        &projects[index],
                        page.is_none() && active == Some(index),
                        &theme,
                        cx,
                    )))
                }
            };
        }
        if projects.is_empty() && groups.is_empty() {
            cards = cards.child(
                div()
                    .px(u(16.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.40))
                    .child("No projects yet"),
            );
        }
        let projects = div()
            .id("rail-projects")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(u(8.))
            .child(cards);

        let footer = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(gpui::px(1.))
            .p(u(8.))
            .child(
                rail_action(
                    "rail-settings",
                    "Settings",
                    IconName::Settings,
                    page == Some(Page::Settings),
                    Some(shortcut("⌘,", &theme)),
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.with_shell(cx, |shell, cx| shell.toggle_page(Page::Settings, cx))
                })),
            );

        div()
            .id("project-rail")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .w(u(width))
            .bg(c.sidebar_glass)
            .border_r_1()
            .border_color(c.stroke)
            .child(header)
            .child(actions)
            .child(projects)
            .children(
                self.shell
                    .upgrade()
                    .map(|shell| shell.read(cx).live_agents.clone()),
            )
            .child(super::sidebar_update::view(cx))
            .child(super::github_star::view(cx))
            .child(footer)
            .child(resize_handle(
                ResizeTarget::ProjectRail,
                self.shell.clone(),
                cx,
            ))
    }

    fn render_section_header(
        &self,
        section: RailSection,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut header = div()
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
                    .child(section.label()),
            );
        match section {
            RailSection::Pinned => {}
            RailSection::Groups => {
                header = header.child(
                    icon_button("rail-add-group", IconName::Plus)
                        .size(20.)
                        .tooltip("New project group")
                        .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
                            this.shell
                                .update(cx, |shell, cx| {
                                    shell.create_project_group(None, event.position(), window, cx)
                                })
                                .ok();
                        })),
                );
            }
            RailSection::Projects => {
                header = header.child(
                    icon_button("rail-add-project", IconName::Plus)
                        .size(20.)
                        .tooltip("Add project")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.with_shell(cx, |shell, cx| shell.open_project_folder(cx))
                        })),
                );
            }
        }
        header
    }

    fn render_group_header(
        &self,
        group: &ProjectGroup,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mark = Project {
            name: group.name.clone(),
            path: group.id.clone(),
            additions: 0,
            deletions: 0,
            busy: false,
            color: parse_color(
                &monocode_engine::projects::project_groups::project_group_color(group),
            ),
            logo: None,
            mascot: group.mascot.clone(),
        };
        let tint = theme.content(0.05);
        div()
            .id(gpui::SharedString::from(format!(
                "project-group-{}",
                group.id
            )))
            .group("project-group")
            .flex()
            .items_center()
            .gap(u(6.))
            .h(u(28.))
            .px(u(6.))
            .rounded(u(theme.radius.md))
            .text_color(theme.content(0.65))
            .hover(move |style| style.bg(tint))
            .child(
                icon(if group.collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(u(12.)),
            )
            .child(project_mark(Some(&mark), 12., theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.label)
                    .medium()
                    .child(group.name.clone()),
            )
            .child(
                div()
                    .opacity(0.)
                    .group_hover("project-group", |style| style.opacity(1.))
                    .child(
                        icon_button(
                            gpui::SharedString::from(format!("project-group-options-{}", group.id)),
                            IconName::MoreHorizontal,
                        )
                        .size(20.)
                        .tooltip("Group options")
                        .on_click({
                            let id = group.id.clone();
                            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                                cx.stop_propagation();
                                this.shell
                                    .update(cx, |shell, cx| {
                                        shell.show_project_group_menu(
                                            &id,
                                            event.position(),
                                            window,
                                            cx,
                                        )
                                    })
                                    .ok();
                            })
                        }),
                    ),
            )
            .on_click({
                let id = group.id.clone();
                cx.listener(move |_, _, _, cx| {
                    ProjectsGlobal::projects(cx).update(cx, |projects, cx| {
                        projects.update_group(
                            &id,
                            |mut group| {
                                group.collapsed = !group.collapsed;
                                group
                            },
                            cx,
                        );
                    });
                })
            })
            .on_mouse_down(gpui::MouseButton::Right, {
                let id = group.id.clone();
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.shell
                        .update(cx, |shell, cx| {
                            shell.show_project_group_menu(&id, event.position, window, cx)
                        })
                        .ok();
                })
            })
    }

    fn render_project_card(
        &self,
        index: usize,
        project: &Project,
        selected: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let bounds = std::rc::Rc::new(std::cell::Cell::new(None::<gpui::Bounds<gpui::Pixels>>));
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
                gpui::canvas(
                    {
                        let bounds = bounds.clone();
                        move |measured, _, _| bounds.set(Some(measured))
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(u(16.))
                    .items_center()
                    .justify_center()
                    .child(project_mark(Some(project), 12., theme)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.ui)
                    .medium()
                    .leading(theme.leading.tight)
                    .child(project.name.clone()),
            )
            .child(diff_stat(project.additions, project.deletions).gap(4.))
            .on_drag(
                RailProjectDrag {
                    path: project.path.clone(),
                },
                {
                    let label = project.name.clone();
                    move |_, _, _, cx| {
                        cx.new(|_| crate::panes::drag::WorkspaceDragPreview {
                            label: label.clone(),
                        })
                    }
                },
            )
            .drag_over::<RailProjectDrag>({
                let fill = theme.accent(0.15);
                move |style, _, _, _| style.bg(fill)
            })
            .on_drop({
                let target = project.path.clone();
                cx.listener(move |this, source: &RailProjectDrag, window, cx| {
                    let Some(bounds) = bounds.get() else {
                        return;
                    };
                    let after =
                        window.mouse_position().y >= bounds.origin.y + bounds.size.height / 2.;
                    this.reorder_project(&source.path, &target, after, cx);
                    cx.stop_propagation();
                })
            })
            .on_mouse_down(gpui::MouseButton::Right, {
                let path = project.path.clone();
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.shell
                        .update(cx, |shell, cx| {
                            shell.show_project_menu(&path, event.position, window, cx)
                        })
                        .ok();
                })
            })
            .on_click({
                let path = project.path.clone();
                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.with_shell(cx, |shell, cx| shell.select_project(&path, cx))
                })
            });
        if selected {
            card = card.bg(c.selection_strong).text_color(c.content);
        } else {
            let hover = theme.content(0.05);
            card = card
                .opacity(0.65)
                .text_color(c.content)
                .hover(move |s| s.bg(hover).opacity(1.0));
        }
        card.tooltip(tooltip(project.path.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use monocode_engine::inbox::{
        backend::InboxBackend, client::InboxClient, hooks::InboxHooks,
        inbox_notifications::InboxNotificationSubject, inbox_seen::InboxSeenEntry,
    };
    use serde_json::{Value, json};
    use std::{cell::Cell, rc::Rc, sync::Arc};

    struct BadgeBackend;

    impl InboxBackend for BadgeBackend {
        fn invoke(&self, command: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
            let result = match command {
                "git_github_repositories" => Ok(json!(["acme/app"])),
                "git_github_work_items" => Ok(if args["kind"] == "pr" {
                    json!([{
                        "kind": "pr", "repo": "acme/app", "number": 42,
                        "title": "Rail badge fixture", "url": "https://github.com/acme/app/pull/42",
                        "state": "open", "updatedAt": "2026-09-13T12:00:00Z",
                        "labels": [], "assignees": [], "draft": false,
                    }])
                } else {
                    json!([])
                }),
                command if command.ends_with("_status") => Ok(json!({"connected": false})),
                _ => Err(format!("Unexpected command: {command}")),
            };
            Box::pin(async move { result })
        }

        fn fetch_media(&self, _: &str) -> BoxFuture<'static, Result<Vec<u8>, String>> {
            Box::pin(async { Err("The badge fixture has no media".into()) })
        }
    }

    struct BadgeHooks(Rc<Cell<bool>>);

    impl InboxHooks for BadgeHooks {
        fn allows_notification_indicator(&self, _: &InboxNotificationSubject, _: &App) -> bool {
            !self.0.get()
        }
    }

    #[gpui::test]
    fn rail_inbox_badge_follows_unread_mute_resume_and_seen(cx: &mut gpui::TestAppContext) {
        monocode_engine::runtime::testing::init_test_engine(cx);
        let client = InboxClient::new(
            Arc::new(BadgeBackend),
            monocode_settings::Kv::in_memory(),
            cx.executor(),
        );
        client.seed_inbox_seen_if_needed(&[InboxSeenEntry::new(
            "github:acme/app:pr:42",
            "2026-09-12T12:00:00Z",
        )]);
        let muted = Rc::new(Cell::new(false));
        let inbox = cx.update(|cx| Inbox::init(client.clone(), cx));
        inbox.update(cx, |inbox, cx| {
            inbox.set_hooks(Rc::new(BadgeHooks(muted.clone())));
            inbox.set_activity_inputs(vec![], "/tmp/app".into(), vec![], cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|cx| inbox_unseen(cx)));
        muted.set(true);
        inbox.update(cx, |inbox, cx| inbox.notification_preferences_changed(cx));
        cx.run_until_parked();
        assert!(!cx.update(|cx| inbox_unseen(cx)));
        muted.set(false);
        inbox.update(cx, |inbox, cx| inbox.notification_preferences_changed(cx));
        cx.run_until_parked();
        assert!(cx.update(|cx| inbox_unseen(cx)));
        client.mark_inbox_item_seen(&InboxSeenEntry::new(
            "github:acme/app:pr:42",
            "2026-09-13T12:00:00Z",
        ));
        cx.run_until_parked();
        assert!(!cx.update(|cx| inbox_unseen(cx)));
    }

    fn project(path: &str) -> Project {
        Project {
            name: project_name(path),
            path: path.into(),
            additions: 0,
            deletions: 0,
            busy: false,
            color: 0x7dd3fc,
            logo: None,
            mascot: None,
        }
    }

    #[test]
    fn groups_keep_saved_order_and_empty_headings_without_duplicating_pins() {
        let projects = vec![
            project("/work/pinned"),
            project("/work/second"),
            project("/work/first"),
            project("/work/ungrouped"),
            project("/work/stale"),
        ];
        let groups = vec![
            ProjectGroup::new("first", "First", false),
            ProjectGroup::new("empty", "Empty", false),
            ProjectGroup::new("second", "Second", false),
        ];
        let mut assignments = JsRecord::new();
        for (path, group) in [
            ("/work/pinned", "first"),
            ("/work/first", "first"),
            ("/work/second", "second"),
            ("/work/stale", "deleted"),
        ] {
            assignments.insert(monocode_core::paths::path_key(path), group.into());
        }
        let pinned = vec!["/work/pinned".into()];
        assert_eq!(
            rail_rows(&projects, &groups, &assignments, &pinned),
            vec![
                RailRow::Section(RailSection::Pinned),
                RailRow::Project(0),
                RailRow::Section(RailSection::Groups),
                RailRow::Group(0),
                RailRow::Project(2),
                RailRow::Group(1),
                RailRow::Group(2),
                RailRow::Project(1),
                RailRow::Section(RailSection::Projects),
                RailRow::Project(3),
                RailRow::Project(4),
            ],
        );
        let mut collapsed = groups;
        collapsed[0].collapsed = true;
        let rows = rail_rows(&projects, &collapsed, &assignments, &pinned);
        assert!(rows.contains(&RailRow::Group(0)));
        assert!(rows.contains(&RailRow::Project(0)));
        assert!(!rows.contains(&RailRow::Project(2)));
        assert!(rows.contains(&RailRow::Project(1)));
    }

    #[test]
    fn project_reordering_changes_only_the_matching_sections_saved_slots() {
        let order: Vec<String> = [
            "/work/a",
            "/work/ungrouped",
            "/work/b",
            "/work/pin-one",
            "/work/other",
            "/work/pin-two",
        ]
        .map(String::from)
        .into();
        let groups = vec![
            ProjectGroup::new("group", "Group", false),
            ProjectGroup::new("other", "Other", false),
        ];
        let mut assignments = JsRecord::new();
        for (path, group) in [
            ("/work/a", "group"),
            ("/work/b", "group"),
            ("/work/other", "other"),
            ("/work/pin-one", "group"),
        ] {
            assignments.insert(monocode_core::paths::path_key(path), group.into());
        }
        let pins = vec!["/work/pin-one".into(), "/work/pin-two".into()];
        let grouping = RailGrouping {
            groups: &groups,
            assignments: &assignments,
            pinned: &pins,
        };
        let mut expected = order.clone();
        expected.swap(0, 2);
        assert_eq!(
            reorder_rail_project(&order, "/work/b", "/work/a", false, &grouping),
            Some(expected.clone()),
        );
        assert_eq!(
            reorder_rail_project(&order, "/work/a", "/work/b", true, &grouping),
            Some(expected),
        );
        let mut expected = order.clone();
        expected.swap(3, 5);
        assert_eq!(
            reorder_rail_project(&order, "/work/pin-two", "/work/pin-one", false, &grouping),
            Some(expected),
        );
        assert_eq!(
            reorder_rail_project(&order, "/work/a", "/work/ungrouped", false, &grouping),
            None,
        );
        assert_eq!(
            reorder_rail_project(&order, "/work/pin-one", "/work/a", false, &grouping),
            None,
        );
        assert_eq!(
            reorder_rail_project(&order, "/work/a", "/work/b", false, &grouping),
            None,
        );
    }
}
