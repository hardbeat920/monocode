//! Original project navigation. Its list and shared image cache stay independent
//! from streamed conversations; Git counters use one visible-project batch.
use super::*;
use crate::tooltip::Tip;
use crate::{
    right_panel::{DIFF_ADD, DIFF_DEL, format_integer, tabular},
    workspace,
};
use gpui::{ListAlignment, ListState, list};
use monocode_core::scm::DiffStats;

const ICON_RAIL_WIDTH: f32 = 48.;

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProjectRailRow {
    Monos,
    Heading(&'static str),
    Group(String),
    Project { path: String, grouped: bool },
    Gap,
    Empty,
}
pub(super) struct ProjectRailList {
    state: ListState,
    rows: Vec<ProjectRailRow>,
    compact: bool,
    bounds: HashMap<String, AnchorSlot>,
    group_bounds: HashMap<String, AnchorSlot>,
    shifts: HashMap<String, TabShift>,
    settling: Option<String>,
    skip_click: bool,
}
impl ProjectRailList {
    pub(super) fn new(cx: &mut Context<Shell>) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(96.));
        let owner = cx.weak_entity();
        state.set_scroll_handler(move |_, _, cx| {
            let _ = owner.update(cx, |this, cx| {
                for bounds in this
                    .project_rail_list
                    .bounds
                    .values()
                    .chain(this.project_rail_list.group_bounds.values())
                {
                    bounds.set(None);
                }
                this.projects_frame.update(cx, |_, cx| cx.notify());
            });
        });
        Self {
            state,
            rows: Vec::new(),
            compact: false,
            bounds: HashMap::new(),
            group_bounds: HashMap::new(),
            shifts: HashMap::new(),
            settling: None,
            skip_click: false,
        }
    }
    fn update(&mut self, rows: Vec<ProjectRailRow>, compact: bool) {
        if compact != self.compact {
            self.compact = compact;
            self.rows = rows;
            self.state.reset(self.rows.len());
        } else if self.rows != rows {
            let prefix = self
                .rows
                .iter()
                .zip(&rows)
                .take_while(|(a, b)| a == b)
                .count();
            let suffix = self.rows[prefix..]
                .iter()
                .rev()
                .zip(rows[prefix..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            self.state.splice(
                prefix..self.rows.len() - suffix,
                rows.len() - prefix - suffix,
            );
            self.rows = rows;
        }
        // The global Mono roster is one naturally sized item, not a project
        // card. Its row count can change without the project list changing.
        if self.rows.first() == Some(&ProjectRailRow::Monos) {
            self.state.remeasure_items(0..1);
        }
    }
}
#[derive(Default)]
pub(super) struct ProjectRailStats {
    values: HashMap<String, Option<DiffStats>>,
    queued: HashSet<String>,
    loading: bool,
    deferred: bool,
}
pub(super) struct ProjectRailDrag {
    path: String,
    group: bool,
    order: Vec<String>,
    bounds: Vec<Bounds<Pixels>>,
    from: usize,
    to: usize,
    origin: Point<Pixels>,
    offset: Pixels,
    active: bool,
}
impl ProjectRailDrag {
    /// Variable-height groups keep their full measured block when they move.
    fn layout(&self, to: usize) -> Vec<Pixels> {
        let mut order: Vec<_> = (0..self.order.len()).collect();
        let moved = order.remove(self.from);
        order.insert(to, moved);
        let gap = self
            .bounds
            .get(1)
            .map_or(px(0.), |b| b.top() - self.bounds[0].bottom());
        let mut top = self.bounds[0].top();
        let mut starts = vec![px(0.); self.order.len()];
        for ix in order {
            starts[ix] = top;
            top += self.bounds[ix].size.height + gap;
        }
        starts
    }
}
fn rail_shift_key(id: &str, group: bool) -> String {
    format!("{}:{id}", if group { "group" } else { "project" })
}
impl Shell {
    pub(super) fn project_rail_drawn_width(&self) -> f32 {
        if !self.workspace.classic_layout {
            0.
        } else if self.workspace.project_rail_open || self.settings.is_some() {
            self.workspace.project_rail_width
        } else {
            ICON_RAIL_WIDTH
        }
    }
    pub(super) fn left_sidebar_width(&self, cx: &App) -> f32 {
        self.project_rail_drawn_width()
            + if !self.session_sidebar_available(cx) {
                0.
            } else {
                self.workspace.sidebar_width * self.sidebar_fraction
            }
    }
    pub(super) fn toggle_primary_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Settings stand in for the sidebar and can't be hidden.
        if self.settings.is_some() {
            return;
        }
        if self.workspace.classic_layout && self.settings.is_none() {
            self.workspace.project_rail_open = !self.workspace.project_rail_open;
            self.project_resize = None;
            self.fit_sidebar(cx);
            self.persist(cx);
            self.invalidate_chrome(cx);
            cx.notify();
        } else {
            self.toggle_sidebar(window, cx);
        }
    }
    pub(super) fn project_rail_frame(&self, cx: &mut Context<Self>) -> AnyElement {
        let width = self.project_rail_drawn_width();
        let frame = div()
            .id("project-rail-frame")
            .debug_selector(|| "project-rail-frame".into())
            .relative()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(content(0.07))
            .child(
                self.projects_frame
                    .clone()
                    .cached(StyleRefinement::default().w(px(width - 1.)).h_full()),
            )
            .when(
                self.workspace.project_rail_open && self.settings.is_none(),
                |d| {
                    d.child(
                        div()
                            .id("project-rail-resize")
                            .debug_selector(|| "project-rail-resize".into())
                            .group("resize-handle")
                            .role(Role::Splitter)
                            .aria_label("Resize project sidebar")
                            .absolute()
                            .right(px(-3.5))
                            .top_0()
                            .bottom_0()
                            .w(px(6.))
                            .cursor(CursorStyle::ResizeLeftRight)
                            .child(resize_border(true, self.project_resize.is_some()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                    this.project_resize =
                                        Some((e.position.x, this.workspace.project_rail_width));
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                if event.click_count() == 2 {
                                    this.workspace.project_rail_width = 220.;
                                    this.fit_sidebar(cx);
                                    this.persist(cx);
                                    this.invalidate_chrome(cx);
                                    cx.notify();
                                }
                            })),
                    )
                },
            )
            .into_any_element();
        if cfg!(target_os = "macos")
            && !self.workspace.project_rail_open
            && self.settings.is_none()
            && self.classic_surface_open(cx)
        {
            // Destination headers live beside the rail. Keep its divider below
            // the traffic lights, at the same height as the sessions view.
            div()
                .w(px(width))
                .h_full()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .child(
                    titlebar_drag_area()
                        .w_full()
                        .h(px(TITLE_BAR_HEIGHT))
                        .flex_shrink_0()
                        .border_b_1()
                        .border_color(content(0.07)),
                )
                .child(div().flex_1().min_h_0().child(frame))
                .into_any_element()
        } else {
            frame
        }
    }
    pub(super) fn project_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let compact = !self.workspace.project_rail_open && self.settings.is_none();
        let header = titlebar_drag_area().h(px(TITLE_BAR_HEIGHT)).flex_shrink_0();
        if self.settings.is_some() {
            let nav = self.sidebar(cx);
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .child(nav)
                .into_any();
        }
        if compact {
            return div()
                .id("project-rail")
                .debug_selector(|| "project-rail".into())
                .role(Role::Navigation)
                .aria_label("Project shortcuts")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .overflow_hidden()
                .when(!cfg!(target_os = "macos"), |d| d.child(header))
                .child(self.project_rail_compact_actions(cx))
                .child(div().min_h(px(8.)).flex_1())
                .child(
                    div().w_full().py(px(6.)).flex().justify_center().child(
                        icon_button("rail-settings", "Settings", "settings")
                            .size(px(32.))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.open_settings(crate::settings::Section::General, w, cx)
                            })),
                    ),
                )
                .into_any();
        }
        self.prepare_project_rail(compact);
        let owner = cx.weak_entity();
        let rows = list(self.project_rail_list.state.clone(), move |ix, _, cx| {
            owner
                .update(cx, |this, cx| this.project_rail_list_row(ix, compact, cx))
                .unwrap_or_else(|_| div().into_any())
        })
        .size_full();
        div()
            .id("project-rail")
            .debug_selector(|| "project-rail".into())
            .role(Role::Navigation)
            .aria_label("Projects")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .when(!self.classic_titlebar_spans_window(cx), |d| d.child(header))
            .when(!compact, |d| d.child(self.original_rail_navigation(cx)))
            .child(
                div()
                    .id("project-rail-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(rows),
            )
            .children(self.live_agents_preview(cx))
            .child(self.app_update_footer(cx))
            .child(self.project_rail_footer(compact, cx))
            .into_any_element()
    }
    fn prepare_project_rail(&mut self, compact: bool) {
        let mut rows = Vec::new();
        if self.workspace.preferences.monos {
            rows.push(ProjectRailRow::Monos);
        }
        let projects = &self.workspace.projects;
        let pinned: Vec<_> = projects
            .iter()
            .filter(|p| self.workspace.project_rail_pinned.contains(*p))
            .cloned()
            .collect();
        if !pinned.is_empty() {
            if !compact {
                rows.push(ProjectRailRow::Heading("Pinned"));
            }
            rows.extend(pinned.into_iter().map(|path| ProjectRailRow::Project {
                path,
                grouped: false,
            }));
            rows.push(ProjectRailRow::Gap);
        }
        if !self.workspace.project_rail_groups.is_empty() {
            if !compact {
                rows.push(ProjectRailRow::Heading("Groups"));
            }
            for group in &self.workspace.project_rail_groups {
                let items: Vec<_> = projects
                    .iter()
                    .filter(|p| {
                        !self.workspace.project_rail_pinned.contains(*p)
                            && self.workspace.project_rail_assignments.get(*p) == Some(&group.id)
                    })
                    .cloned()
                    .collect();
                if !compact {
                    rows.push(ProjectRailRow::Group(group.id.clone()));
                }
                if compact || !group.collapsed {
                    rows.extend(items.into_iter().map(|path| ProjectRailRow::Project {
                        path,
                        grouped: !compact,
                    }));
                }
            }
            rows.push(ProjectRailRow::Gap);
        }
        rows.push(ProjectRailRow::Heading("Projects"));
        let mut count = 0;
        for path in projects {
            let grouped = self
                .workspace
                .project_rail_assignments
                .get(path)
                .is_some_and(|id| {
                    self.workspace
                        .project_rail_groups
                        .iter()
                        .any(|g| &g.id == id)
                });
            if !self.workspace.project_rail_pinned.contains(path) && !grouped {
                count += 1;
                rows.push(ProjectRailRow::Project {
                    path: path.clone(),
                    grouped: false,
                });
            }
        }
        if count == 0 && self.workspace.project_rail_groups.is_empty() {
            rows.push(ProjectRailRow::Empty);
        }
        rows.push(ProjectRailRow::Gap);
        self.project_rail_list.update(rows, compact);
    }
    fn project_rail_list_row(
        &mut self,
        ix: usize,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.project_rail_list.rows.get(ix).cloned() else {
            return div().into_any();
        };
        let row_gap = matches!(
            self.project_rail_list.rows.get(ix + 1),
            Some(ProjectRailRow::Project { .. })
        );
        match row {
            ProjectRailRow::Monos => self
                .monos
                .clone()
                .map(|m| m.update(cx, |m, cx| m.project_navigation_rail(compact, cx)))
                .unwrap_or_else(|| div().into_any()),
            ProjectRailRow::Heading(label) => self.project_rail_heading(label, compact, cx),
            ProjectRailRow::Project { path, grouped } => {
                self.project_rail_row(ix, path, compact, grouped, row_gap, cx)
            }
            ProjectRailRow::Group(id) => self.project_rail_group_header(id, cx),
            ProjectRailRow::Gap => div().h(px(8.)).into_any(),
            ProjectRailRow::Empty => div()
                .px_4()
                .pb_1()
                .text_size(px(11.))
                .line_height(px(13.75))
                .text_color(content(0.4))
                .when(!compact, |d| d.child("No projects yet"))
                .into_any(),
        }
    }
    fn project_rail_heading(
        &self,
        label: &'static str,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .debug_selector(move || format!("rail-heading-{}", label.to_lowercase()))
            .w_full()
            .h(px(30.))
            .pl(px(10.))
            .pr_3()
            .pt_1()
            .pb(px(6.))
            .flex()
            .items_center()
            .gap_1()
            .when(compact, |d| d.px(px(14.)))
            .when(!compact, |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px_1()
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .text_color(content(0.5))
                        .child(label),
                )
            })
            .when(label == "Groups", |d| {
                d.child(
                    icon_button("rail-new-project-group", "New project group", "folder-tree")
                        .size(px(20.))
                        .on_click(cx.listener(|this, e: &gpui::ClickEvent, w, cx| {
                            let id = this.create_project_rail_group(None, cx);
                            this.open_menu(
                                MenuTarget::ProjectRailGroup(id),
                                Anchor::Point(e.position()),
                                w,
                                cx,
                            );
                        })),
                )
            })
            .when(label == "Projects", |d| {
                let anchor = AnchorSlot::default();
                d.child(
                    icon_button("rail-open-project", "Open project", "plus")
                        .debug_selector(|| "rail-open-project".into())
                        .child(anchor.probe())
                        .aria_expanded(self.menu_open(&MenuTarget::ProjectRailAdd))
                        .size(px(20.))
                        .when(self.menu_open(&MenuTarget::ProjectRailAdd), |d| {
                            d.bg(content(0.08)).text_color(content(1.))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.menu_open(&MenuTarget::ProjectRailAdd) {
                                this.close_menu(cx);
                            } else {
                                this.open_menu(
                                    MenuTarget::ProjectRailAdd,
                                    Anchor::Slot(anchor.clone()),
                                    window,
                                    cx,
                                );
                            }
                        })),
                )
            })
            .into_any()
    }
    fn project_rail_footer(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .py_2()
            .pl(px(if compact { 8. } else { 6. }))
            .pr_2()
            .flex_shrink_0()
            .child(
                div()
                    .id("rail-settings")
                    .debug_selector(|| "rail-settings".into())
                    .role(Role::Button)
                    .aria_label("Settings")
                    .cursor_pointer()
                    .h(px(32.))
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(0.5))
                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                    .child(icon("settings", 16.).text_color(content(0.35)))
                    .when(!compact, |d| {
                        d.child(div().flex_1().child("Settings")).child(
                            div()
                                .font_weight(FontWeight::NORMAL)
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child(if cfg!(target_os = "macos") {
                                    "⌘,"
                                } else {
                                    "Ctrl ,"
                                }),
                        )
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(crate::settings::Section::General, window, cx)
                    })),
            )
            .into_any()
    }
    fn original_rail_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .pl(px(6.))
            .pr_2()
            .pt(px(2.))
            .pb_2()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .id("rail-search")
                    .debug_selector(|| "sidebar-search-field".into())
                    .group("rail-search")
                    .role(Role::Button)
                    .aria_label("Search")
                    .cursor_pointer()
                    .h(px(32.))
                    .mb(px(2.))
                    .px(px(6.))
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.08))
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(0x0000001a).into()).blur_radius(px(3.)),
                        BoxShadow::new(px(0.), px(1.), rgba(0x0000001a).into())
                            .blur_radius(px(2.))
                            .spread_radius(px(-1.)),
                    ])
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(0.5))
                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                    .child(icon("search", 16.).text_color(content(0.35)))
                    .child(div().flex_1().min_w_0().child("Search"))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(content(0.4))
                            .child(if cfg!(target_os = "macos") {
                                "⌘K"
                            } else {
                                "Ctrl K"
                            }),
                    )
                    .on_click(cx.listener(|this, _, w, cx| this.focus_search(w, cx))),
            )
            .children(
                [
                    ("Inbox", "inbox"),
                    ("Notes", "file"),
                    ("Automations", "zap"),
                ]
                .into_iter()
                .filter(|(label, _)| match *label {
                    "Inbox" => self.workspace.sidebar_inbox_visible,
                    "Notes" => {
                        self.workspace.preferences.notes && self.workspace.sidebar_notes_visible
                    }
                    "Automations" => self.workspace.sidebar_automations_visible,
                    _ => true,
                })
                .map(|(label, glyph)| {
                    let selected = self.app_view_label() == Some(label);
                    let unread = label == "Inbox"
                        && self
                            .inbox
                            .as_ref()
                            .is_some_and(|inbox| inbox.read(cx).unread_count() > 0);
                    div()
                        .id(label)
                        .debug_selector(move || format!("sidebar-navigation-{label}"))
                        .role(Role::Button)
                        .aria_label(if unread { "Inbox, new items" } else { label })
                        .aria_selected(selected)
                        .cursor_pointer()
                        .h(px(32.))
                        .px_2()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(if selected { 1. } else { 0.5 }))
                        .when(selected, |d| d.bg(content(0.1)))
                        .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                        .child(icon(glyph, 16.).text_color(content(if selected {
                            0.7
                        } else {
                            0.35
                        })))
                        .child(div().flex_1().child(label))
                        .when(unread, |d| {
                            d.child(div().size(px(8.)).rounded_full().bg(accent_color()))
                        })
                        .on_click(cx.listener(move |this, _, w, cx| match label {
                            "Inbox" => this.open_inbox(w, cx),
                            "Notes" => this.open_notes(w, cx),
                            "Automations" => this.open_automations(w, cx),
                            _ => (),
                        }))
                }),
            )
            .into_any()
    }
    fn project_rail_compact_actions(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use classic_sidebar::ClassicSidebarTab;
        let workspace_active =
            !self.app_view_open() && self.app_search.is_none() && !self.mono_active(cx);
        let inbox_unread = self
            .inbox
            .as_ref()
            .is_some_and(|v| v.read(cx).unread_count() > 0);
        let mut tabs = Vec::new();
        for id in self
            .workspace
            .classic_sidebar_tab_order
            .iter()
            .map(String::as_str)
            .chain(["sessions", "files", "changes"])
        {
            let tab = ClassicSidebarTab::from_id(id);
            if !tabs.contains(&tab) {
                tabs.push(tab);
            }
        }
        let mono_rail =
            if self.workspace.project_rail_monos_pinned && self.workspace.preferences.monos {
                self.monos
                    .clone()
                    .filter(|m| !m.read(cx).roster.monos.is_empty())
                    .map(|m| m.update(cx, |m, cx| m.project_navigation_rail(true, cx)))
            } else {
                None
            };
        let active_mono = self
            .monos
            .as_ref()
            .filter(|_| self.workspace.preferences.monos)
            .and_then(|manager| {
                let manager = manager.read(cx);
                manager.current().map(|mono| {
                    let status = manager
                        .chat()
                        .map_or("idle", |chat| chat.read(cx).mono_status());
                    (
                        mono.label(),
                        crate::monos::avatar::avatar(&mono.mascot, mono.color, status, 16., true),
                    )
                })
            });
        let (picker_label, picker_mark) = active_mono.map_or_else(
            || {
                (
                    format!(
                        "Switch project, {}",
                        self.project_label(&self.workspace.project)
                    ),
                    self.project_mark(&self.workspace.project, 16.),
                )
            },
            |(name, mark)| (format!("Switch project, current mono {name}"), mark),
        );
        div()
            .w_full()
            .py(px(6.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .when_some(mono_rail, |d, monos| {
                d.child(monos)
                    .child(div().w(px(24.)).h(px(1.)).bg(content(0.07)))
            })
            .child(
                div()
                    .id("rail-current-project")
                    .debug_selector(|| "rail-current-project".into())
                    .role(Role::Button)
                    .aria_label(picker_label.clone())
                    .tip(picker_label.clone())
                    .aria_expanded(self.menu_open(&MenuTarget::CompactProjectPicker))
                    .size(px(32.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(content(0.1)))
                    .child(picker_mark)
                    .on_click(cx.listener(|this, e: &gpui::ClickEvent, w, cx| {
                        if this.menu_open(&MenuTarget::CompactProjectPicker) {
                            this.close_menu(cx);
                        } else {
                            this.open_menu(
                                MenuTarget::CompactProjectPicker,
                                Anchor::Point(e.position()),
                                w,
                                cx,
                            )
                        }
                    })),
            )
            .children(tabs.into_iter().map(|tab| {
                let (label, glyph) = match tab {
                    ClassicSidebarTab::Sessions => ("Sessions", "chatting"),
                    ClassicSidebarTab::Files => ("Explorer", "file-script"),
                    ClassicSidebarTab::Changes => ("Changes", "git-branch"),
                };
                let selected = workspace_active
                    && self.workspace.sidebar_visible
                    && self.classic_sidebar_tab == tab;
                let changed = tab == ClassicSidebarTab::Changes
                    && self
                        .git
                        .read(cx)
                        .stats
                        .as_ref()
                        .is_some_and(|s| s.files > 0 || s.additions > 0 || s.deletions > 0);
                icon_button(
                    match tab {
                        ClassicSidebarTab::Sessions => "rail-workspace-sessions",
                        ClassicSidebarTab::Files => "rail-workspace-files",
                        ClassicSidebarTab::Changes => "rail-workspace-changes",
                    },
                    label,
                    glyph,
                )
                .debug_selector(move || format!("rail-workspace-{}", tab.id()))
                .role(Role::Tab)
                .aria_selected(selected)
                .size(px(32.))
                .relative()
                .when(selected, |d| d.bg(content(0.1)))
                .when(changed, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(6.))
                            .top(px(6.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(accent_color()),
                    )
                })
                .on_click(cx.listener(move |this, _, w, cx| {
                    this.leave_app_views(cx);
                    this.leave_app_search(cx);
                    if let Some(m) = &this.monos {
                        m.update(cx, |m, cx| m.close(cx));
                    }
                    this.select_classic_sidebar_tab(tab, w, cx);
                    this.projects_frame.update(cx, |_, cx| cx.notify());
                }))
            }))
            .child(
                icon_button("rail-global-search", "Search", "search")
                    .size(px(32.))
                    .when(self.app_search.is_some(), |d| d.bg(content(0.1)))
                    .on_click(cx.listener(|this, _, w, cx| {
                        if this.app_search.is_some() {
                            this.close_app_search(w, cx)
                        } else {
                            this.focus_search(w, cx)
                        }
                    })),
            )
            .children(
                [
                    (
                        workspace::AppView::Inbox,
                        self.workspace.sidebar_inbox_visible,
                    ),
                    (
                        workspace::AppView::Notes,
                        self.workspace.preferences.notes && self.workspace.sidebar_notes_visible,
                    ),
                    (
                        workspace::AppView::Automations,
                        self.workspace.sidebar_automations_visible,
                    ),
                ]
                .into_iter()
                .filter(|(_, v)| *v)
                .map(|(view, _)| {
                    let selected = self.app_view_label() == Some(view.label());
                    let glyph = if view == workspace::AppView::Notes {
                        "sticky-note"
                    } else {
                        view.icon()
                    };
                    icon_button(
                        match view {
                            workspace::AppView::Inbox => "rail-inbox",
                            workspace::AppView::Notes => "rail-notes",
                            workspace::AppView::Automations => "rail-automations",
                        },
                        view.label(),
                        glyph,
                    )
                    .size(px(32.))
                    .relative()
                    .when(selected, |d| d.bg(content(0.1)))
                    .when(view == workspace::AppView::Inbox && inbox_unread, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right(px(6.))
                                .top(px(6.))
                                .size(px(6.))
                                .rounded_full()
                                .bg(accent_color()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if this.app_view_label() == Some(view.label()) {
                            this.close_app_view(w, cx);
                        } else {
                            match view {
                                workspace::AppView::Inbox => this.open_inbox(w, cx),
                                workspace::AppView::Notes => this.open_notes(w, cx),
                                workspace::AppView::Automations => this.open_automations(w, cx),
                            }
                        }
                    }))
                }),
            )
            .into_any()
    }
    fn project_rail_group_header(&mut self, id: String, cx: &mut Context<Self>) -> AnyElement {
        let Some(group) = self
            .workspace
            .project_rail_groups
            .iter()
            .find(|g| g.id == id)
        else {
            return div().into_any();
        };
        let name = group.name.clone();
        let collapsed = group.collapsed;
        let context = id.clone();
        let options = id.clone();
        let drag_id = id.clone();
        let bounds = self
            .project_rail_list
            .group_bounds
            .entry(id.clone())
            .or_default()
            .clone();
        let offset = self.project_rail_item_offset(&id, true);
        let dragging = self.project_rail_item_raised(&id, true);
        let color = group
            .custom_color
            .as_ref()
            .and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok())
            .or_else(|| {
                group
                    .color_index
                    .and_then(|i| monocode_core::project_appearance::COLORS.get(i).copied())
            });
        let appearance = monocode_core::project_appearance::ProjectAppearance {
            color,
            mascot: group.mascot.clone(),
            ..Default::default()
        };
        let row = div()
            .w_full()
            .px_2()
            .child(
                div()
                    .id(SharedString::from(format!("rail-group-{id}")))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("rail-group-{id}")
                    })
                    .group("project-rail-group")
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .aria_expanded(!collapsed)
                    .h(px(32.))
                    .px_2()
                    .rounded_md()
                    .relative()
                    .top(px(offset))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .opacity(0.65)
                    .when(!collapsed, |d| d.bg(content(0.05)))
                    .hover(|s| s.bg(content(0.08)).opacity(1.))
                    .child(
                        div()
                            .size(px(16.))
                            .flex_shrink_0()
                            .relative()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(collapsed, |d| {
                                d.child(
                                    div()
                                        .group_hover("project-rail-group", |s| s.invisible())
                                        .child(crate::project_settings::project_icon(
                                            &appearance,
                                            &id,
                                            12.,
                                        )),
                                )
                            })
                            .child(
                                icon(
                                    if collapsed {
                                        "chevron-right"
                                    } else {
                                        "chevron-down"
                                    },
                                    14.,
                                )
                                .when(collapsed, |d| {
                                    d.invisible()
                                        .group_hover("project-rail-group", |s| s.visible())
                                })
                                .absolute()
                                .left(px(1.))
                                .top(px(1.)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .group_hover("project-rail-group", |s| s.pr(px(24.)))
                            .child(name),
                    )
                    .child(
                        div()
                            .absolute()
                            .right_1()
                            .top(px(4.))
                            .invisible()
                            .group_hover("project-rail-group", |s| s.visible())
                            .child(
                                icon_button(
                                    "rail-group-options",
                                    "Group options",
                                    "more-horizontal",
                                )
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(
                                    move |this, e: &gpui::ClickEvent, w, cx| {
                                        cx.stop_propagation();
                                        this.open_menu(
                                            MenuTarget::ProjectRailGroup(options.clone()),
                                            Anchor::Point(e.position()),
                                            w,
                                            cx,
                                        );
                                    },
                                )),
                            ),
                    )
                    .child(bounds.probe())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.start_project_rail_drag(drag_id.clone(), true, event, window, cx)
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, w, cx| {
                            cx.stop_propagation();
                            this.open_menu(
                                MenuTarget::ProjectRailGroup(context.clone()),
                                Anchor::Point(e.position),
                                w,
                                cx,
                            );
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if std::mem::take(&mut this.project_rail_list.skip_click) {
                            return;
                        }
                        if let Some(g) = this
                            .workspace
                            .project_rail_groups
                            .iter_mut()
                            .find(|g| g.id == id)
                        {
                            g.collapsed = !g.collapsed;
                        }
                        this.persist(cx);
                        this.projects_frame.update(cx, |_, cx| cx.notify());
                    })),
            )
            .into_any();
        if dragging {
            gpui::deferred(row).with_priority(1).into_any()
        } else {
            row
        }
    }
    pub(super) fn project_rail_group_controls(
        &self,
        id: &str,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<crate::project_settings::AppearanceControls> {
        use crate::project_settings::{AppearanceControls, Event};
        let group = self
            .workspace
            .project_rail_groups
            .iter()
            .find(|g| g.id == id)
            .cloned()
            .unwrap_or_default();
        let color = group
            .custom_color
            .as_ref()
            .and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok())
            .or_else(|| {
                group
                    .color_index
                    .and_then(|i| monocode_core::project_appearance::COLORS.get(i).copied())
            });
        let appearance = monocode_core::project_appearance::ProjectAppearance {
            name: group.name,
            color,
            mascot: group.mascot,
            ..Default::default()
        };
        let controls =
            cx.new(|cx| AppearanceControls::new(id.into(), appearance, cx).for_project_group());
        let id = id.to_owned();
        events.push(cx.subscribe_in(
            &controls,
            window,
            move |this, _, event: &Event, window, cx| match event {
                Event::Changed(appearance) => {
                    if let Some(group) = this
                        .workspace
                        .project_rail_groups
                        .iter_mut()
                        .find(|g| g.id == id)
                    {
                        if !appearance.name.trim().is_empty() {
                            group.name = appearance.name.trim().into();
                        }
                        group.mascot = appearance.mascot.clone();
                        group.color_index = appearance.color.and_then(|c| {
                            monocode_core::project_appearance::COLORS
                                .iter()
                                .position(|v| *v == c)
                        });
                        group.custom_color = appearance
                            .color
                            .filter(|_| group.color_index.is_none())
                            .map(|c| format!("#{c:06x}"));
                    }
                    this.persist(cx);
                    this.projects_frame.update(cx, |_, cx| cx.notify());
                }
                Event::Close => {
                    this.close_menu(cx);
                    this.focus_workspace(window, cx);
                }
            },
        ));
        controls
    }
    fn project_rail_row(
        &mut self,
        ix: usize,
        path: String,
        compact: bool,
        grouped: bool,
        row_gap: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ix = self
            .workspace
            .projects
            .iter()
            .position(|p| p == &path)
            .unwrap_or(ix);
        let selected = self.workspace.project == path
            && !self.app_view_open()
            && self.app_search.is_none()
            && !self.mono_active(cx);
        let label = self.project_label(&path);
        let tooltip = label.clone();
        let menu_path = path.clone();
        let options_path = path.clone();
        let pin_path = path.clone();
        let drag_path = path.clone();
        let pinned = self.workspace.project_rail_pinned.contains(&path);
        let offset = self.project_rail_project_offset(&path);
        let raised = self.project_rail_project_raised(&path);
        let reordering = self.project_rail_reordering();
        let bounds = self
            .project_rail_list
            .bounds
            .entry(path.clone())
            .or_default()
            .clone();
        if !compact {
            self.queue_project_rail_stats(&path, cx);
        }
        let stats = self
            .project_rail_stats
            .values
            .get(&path)
            .and_then(Option::as_ref)
            .cloned();
        let appearance = self.appearance(&path);
        let seed = workspace::project_name(&path);
        let mark = if appearance.logo.is_some() {
            self.project_mark(&path, 16.)
        } else {
            crate::project_settings::project_icon(&appearance, &seed, 12.)
        };
        let row = div()
            .id(("rail-project", ix))
            .debug_selector(move || format!("rail-project-{ix}"))
            .h(px(if row_gap { 33. } else { 32. }))
            .pl(px(6.))
            .pr_2()
            .when(row_gap, |d| d.pb_px())
            .when(grouped, |d| d.px_3())
            .child(
                div()
                    .id(("rail-project-button", ix))
                    .debug_selector(move || format!("rail-project-button-{ix}"))
                    .group("project-rail-row")
                    .role(Role::Button)
                    .aria_label(format!("Switch project, {label}"))
                    .aria_description(path.clone())
                    .aria_selected(selected)
                    .cursor_pointer()
                    .size_full()
                    .relative()
                    .top(px(offset))
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .opacity(if selected { 1. } else { 0.65 })
                    .when(selected, |d| d.bg(content(0.1)))
                    .hover(|s| s.bg(content(0.08)).opacity(1.))
                    .child(
                        div()
                            .size(px(16.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(!compact && !reordering, |d| {
                                d.group_hover("project-rail-row", |s| s.invisible())
                            })
                            .child(mark),
                    )
                    .when(!compact, |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when(!reordering, |d| {
                                    d.group_hover("project-rail-row", |s| s.pr(px(24.)))
                                })
                                .child(label),
                        )
                    })
                    .when(!compact, |d| {
                        d.when_some(stats, |d, stats| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .gap_1()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .font_features(tabular())
                                    .when(!reordering, |d| {
                                        d.group_hover("project-rail-row", |s| s.invisible())
                                    })
                                    .when(stats.additions > 0, |d| {
                                        d.child(
                                            div().text_color(rgb(DIFF_ADD)).child(format!(
                                                "+{}",
                                                format_integer(stats.additions)
                                            )),
                                        )
                                    })
                                    .when(stats.deletions > 0, |d| {
                                        d.child(
                                            div().text_color(rgb(DIFF_DEL)).child(format!(
                                                "-{}",
                                                format_integer(stats.deletions)
                                            )),
                                        )
                                    }),
                            )
                        })
                    })
                    .when(!compact && !reordering, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_2()
                                .top(px(8.))
                                .invisible()
                                .group_hover("project-rail-row", |s| s.visible())
                                .child(
                                    icon_button(
                                        "rail-pin-project",
                                        if pinned {
                                            "Unpin project"
                                        } else {
                                            "Pin project"
                                        },
                                        if pinned { "pin-off" } else { "pin" },
                                    )
                                    .size(px(16.))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_project_rail_pin(&pin_path, cx);
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .right_1()
                                .top(px(4.))
                                .invisible()
                                .group_hover("project-rail-row", |s| s.visible())
                                .child(
                                    icon_button(
                                        "rail-project-options",
                                        "Project options",
                                        "more-horizontal",
                                    )
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(
                                        move |this, e: &gpui::ClickEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.open_menu(
                                                MenuTarget::Project {
                                                    path: options_path.clone(),
                                                    rail: true,
                                                },
                                                Anchor::Point(e.position()),
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                                ),
                        )
                    })
                    .child(bounds.probe())
                    .tip(tooltip)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if std::mem::take(&mut this.project_rail_list.skip_click) {
                            return;
                        }
                        this.select_project(path.clone(), window, cx)
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            this.start_project_rail_drag(drag_path.clone(), false, e, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            this.open_menu(
                                MenuTarget::Project {
                                    path: menu_path.clone(),
                                    rail: true,
                                },
                                Anchor::Point(e.position),
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    ),
            )
            .into_any_element();
        if raised {
            gpui::deferred(row).with_priority(1).into_any()
        } else {
            row
        }
    }
    pub(super) fn reveal_active_project_rail(&self) {
        if let Some(ix) = self.project_rail_list.rows.iter().position(
            |row| matches!(row,ProjectRailRow::Project {path,..} if path==&self.workspace.project),
        ) {
            self.project_rail_list.state.scroll_to_reveal_item(ix);
        }
    }
    pub(super) fn toggle_project_rail_pin(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.workspace.project_rail_pinned.remove(path) {
            self.workspace.project_rail_pinned.insert(path.into());
        }
        self.persist(cx);
        self.projects_frame.update(cx, |_, cx| cx.notify());
    }
    pub(super) fn assign_project_rail_group(
        &mut self,
        path: &str,
        group: &str,
        cx: &mut Context<Self>,
    ) {
        if group.is_empty() {
            self.workspace.project_rail_assignments.remove(path);
        } else {
            self.workspace
                .project_rail_assignments
                .insert(path.into(), group.into());
        }
        self.persist(cx);
        self.projects_frame.update(cx, |_, cx| cx.notify());
    }
    pub(super) fn create_project_rail_group(
        &mut self,
        path: Option<&str>,
        cx: &mut Context<Self>,
    ) -> String {
        let id = format!(
            "group-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        self.workspace
            .project_rail_groups
            .push(workspace::ProjectRailGroup {
                id: id.clone(),
                name: "New group".into(),
                ..Default::default()
            });
        if let Some(path) = path {
            self.workspace
                .project_rail_assignments
                .insert(path.into(), id.clone());
        }
        self.persist(cx);
        self.projects_frame.update(cx, |_, cx| cx.notify());
        id
    }
    fn project_rail_item_offset(&self, id: &str, group: bool) -> f32 {
        if let Some(drag) = &self.project_rail_drag
            && drag.group == group
            && drag.path == id
            && drag.active
        {
            return drag.offset.as_f32();
        }
        self.project_rail_list
            .shifts
            .get(&rail_shift_key(id, group))
            .map_or(0., |shift| shift.at(std::time::Instant::now()).0)
    }
    fn project_rail_project_offset(&self, path: &str) -> f32 {
        let project = self.project_rail_item_offset(path, false);
        if self.workspace.project_rail_pinned.contains(path) {
            return project;
        }
        project
            + self
                .workspace
                .project_rail_assignments
                .get(path)
                .map_or(0., |group| self.project_rail_item_offset(group, true))
    }
    fn project_rail_item_raised(&self, id: &str, group: bool) -> bool {
        let key = rail_shift_key(id, group);
        self.project_rail_drag
            .as_ref()
            .is_some_and(|drag| drag.active && drag.group == group && drag.path == id)
            || self.project_rail_list.settling.as_ref() == Some(&key)
                && self
                    .project_rail_list
                    .shifts
                    .get(&key)
                    .is_some_and(|shift| !shift.at(std::time::Instant::now()).1)
    }
    fn project_rail_project_raised(&self, path: &str) -> bool {
        self.project_rail_item_raised(path, false)
            || !self.workspace.project_rail_pinned.contains(path)
                && self
                    .workspace
                    .project_rail_assignments
                    .get(path)
                    .is_some_and(|id| self.project_rail_item_raised(id, true))
    }
    fn project_rail_reordering(&self) -> bool {
        self.project_rail_drag
            .as_ref()
            .is_some_and(|drag| drag.active)
    }
    pub(super) fn project_rail_animating(&self) -> bool {
        self.workspace.classic_layout
            && self.workspace.project_rail_open
            && self
                .project_rail_list
                .shifts
                .values()
                .any(|shift| shift.from != shift.to && !shift.at(std::time::Instant::now()).1)
    }
    fn start_project_rail_drag(
        &mut self,
        id: String,
        group: bool,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Tauri's row buttons take keyboard focus on press. Keep Escape on the
        // workspace while a native div row is being dragged as well.
        window.focus(&self.focus, cx);
        let now = std::time::Instant::now();
        let items: Vec<(String, Bounds<Pixels>)> = if group {
            self.workspace
                .project_rail_groups
                .iter()
                .filter_map(|candidate| {
                    let mut bounds = self
                        .project_rail_list
                        .group_bounds
                        .get(&candidate.id)?
                        .get()?;
                    if !candidate.collapsed {
                        for path in &self.workspace.projects {
                            if !self.workspace.project_rail_pinned.contains(path)
                                && self.workspace.project_rail_assignments.get(path)
                                    == Some(&candidate.id)
                                && let Some(row) = self
                                    .project_rail_list
                                    .bounds
                                    .get(path)
                                    .and_then(AnchorSlot::get)
                            {
                                bounds.size.height =
                                    bounds.size.height.max(row.bottom() - bounds.top());
                            }
                        }
                    }
                    let offset = self.project_rail_item_offset(&candidate.id, true);
                    bounds.origin.y -= px(offset);
                    Some((candidate.id.clone(), bounds))
                })
                .collect()
        } else {
            self.workspace
                .projects
                .iter()
                .filter(|path| self.workspace.project_rail_peers(path, &id))
                .filter_map(|path| {
                    let mut bounds = self.project_rail_list.bounds.get(path)?.get()?;
                    bounds.origin.y -= px(self.project_rail_project_offset(path));
                    Some((path.clone(), bounds))
                })
                .collect()
        };
        let Some(from) = items.iter().position(|(path, _)| *path == id) else {
            return;
        };
        // A new press finishes any previous settling, as useAnimatedReorder does.
        self.project_rail_list
            .shifts
            .retain(|_, shift| !shift.at(now).1);
        self.project_rail_list.shifts.clear();
        self.project_rail_list.settling = None;
        self.project_rail_list.skip_click = false;
        self.project_rail_drag = Some(ProjectRailDrag {
            path: id,
            group,
            order: items.iter().map(|(id, _)| id.clone()).collect(),
            bounds: items.into_iter().map(|(_, bounds)| bounds).collect(),
            from,
            to: from,
            origin: event.position,
            offset: px(0.),
            active: false,
        });
    }
    pub(super) fn move_project_rail_drag(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.cancel_project_rail_drag(cx);
            return;
        }
        let Some(drag) = &mut self.project_rail_drag else {
            return;
        };
        let offset = event.position.y - drag.origin.y;
        if !drag.active && offset.abs() < px(5.) {
            return;
        }
        drag.active = true;
        self.project_rail_list.skip_click = true;
        let slots: Vec<_> = (0..drag.order.len())
            .map(|to| drag.layout(to)[drag.from] - drag.bounds[drag.from].top())
            .collect();
        drag.offset = offset.max(slots[0]).min(*slots.last().unwrap());
        let to = slots
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.as_f32() - drag.offset.as_f32())
                    .abs()
                    .total_cmp(&(b.as_f32() - drag.offset.as_f32()).abs())
            })
            .map_or(drag.from, |(ix, _)| ix);
        if drag.to != to {
            drag.to = to;
            let starts = drag.layout(to);
            let now = std::time::Instant::now();
            for (ix, id) in drag.order.iter().enumerate() {
                if ix == drag.from {
                    continue;
                }
                let key = rail_shift_key(id, drag.group);
                let target = (starts[ix] - drag.bounds[ix].top()).as_f32();
                let from = self
                    .project_rail_list
                    .shifts
                    .get(&key)
                    .map_or(0., |shift| shift.at(now).0);
                self.project_rail_list.shifts.insert(
                    key,
                    TabShift {
                        from: if cx.reduce_motion() { target } else { from },
                        to: target,
                        start: now,
                    },
                );
            }
        }
        self.projects_frame.update(cx, |_, cx| cx.notify());
    }
    pub(super) fn finish_project_rail_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.project_rail_drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        let mut reordered = drag.order.clone();
        let moved = reordered.remove(drag.from);
        reordered.insert(drag.to, moved);
        let starts = drag.layout(drag.to);
        let now = std::time::Instant::now();
        for (ix, id) in drag.order.iter().enumerate() {
            let key = rail_shift_key(id, drag.group);
            let old_offset = if ix == drag.from {
                drag.offset.as_f32()
            } else {
                self.project_rail_list
                    .shifts
                    .get(&key)
                    .map_or(0., |shift| shift.at(now).0)
            };
            self.project_rail_list.shifts.insert(
                key,
                TabShift {
                    from: (drag.bounds[ix].top() - starts[ix]).as_f32() + old_offset,
                    to: 0.,
                    start: now,
                },
            );
        }
        self.project_rail_list.settling = Some(rail_shift_key(&drag.path, drag.group));
        let subset: HashSet<String> = drag.order.iter().cloned().collect();
        let mut order = reordered.into_iter();
        if drag.group {
            let groups: HashMap<_, _> = self
                .workspace
                .project_rail_groups
                .iter()
                .map(|g| (g.id.clone(), g.clone()))
                .collect();
            for group in &mut self.workspace.project_rail_groups {
                if subset.contains(&group.id)
                    && let Some(id) = order.next()
                    && let Some(next) = groups.get(&id)
                {
                    *group = next.clone();
                }
            }
        } else {
            for path in &mut self.workspace.projects {
                if subset.contains(path.as_str())
                    && let Some(id) = order.next()
                {
                    *path = id;
                }
            }
        }
        self.project_rail_list.skip_click = true;
        if cx.reduce_motion() {
            self.project_rail_list.shifts.clear();
        }
        self.persist(cx);
        self.projects_frame.update(cx, |_, cx| cx.notify());
    }
    pub(super) fn cancel_project_rail_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.project_rail_drag.take() else {
            return false;
        };
        if drag.active {
            self.project_rail_list.settling = Some(rail_shift_key(&drag.path, drag.group));
            let now = std::time::Instant::now();
            for (ix, id) in drag.order.iter().enumerate() {
                let key = rail_shift_key(id, drag.group);
                let from = if ix == drag.from {
                    drag.offset.as_f32()
                } else {
                    self.project_rail_list
                        .shifts
                        .get(&key)
                        .map_or(0., |shift| shift.at(now).0)
                };
                self.project_rail_list.shifts.insert(
                    key,
                    TabShift {
                        from,
                        to: 0.,
                        start: now,
                    },
                );
            }
            self.project_rail_list.skip_click = true;
            if cx.reduce_motion() {
                self.project_rail_list.shifts.clear();
            }
            self.projects_frame.update(cx, |_, cx| cx.notify());
        }
        true
    }
    fn queue_project_rail_stats(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.project_rail_stats.values.contains_key(path)
            || path == "~"
            || path.starts_with("remote://")
        {
            return;
        }
        self.project_rail_stats.queued.insert(path.into());
        if self.project_rail_stats.deferred || self.project_rail_stats.loading {
            return;
        }
        self.project_rail_stats.deferred = true;
        let owner = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |this, cx| {
                this.project_rail_stats.deferred = false;
                this.load_project_rail_stats(cx);
            });
        });
    }
    fn load_project_rail_stats(&mut self, cx: &mut Context<Self>) {
        if self.project_rail_stats.loading || self.project_rail_stats.queued.is_empty() {
            return;
        }
        let paths: Vec<_> = self.project_rail_stats.queued.drain().collect();
        self.project_rail_stats.loading = true;
        let work = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .map(|path| {
                    let stats = monocode_core::scm::diff_stats(std::path::Path::new(&path)).ok();
                    (path, stats)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let values = work.await;
            let _ = this.update(cx, |this, cx| {
                this.project_rail_stats.loading = false;
                this.project_rail_stats.values.extend(values);
                this.projects_frame.update(cx, |_, cx| cx.notify());
                this.load_project_rail_stats(cx);
            });
        })
        .detach();
    }
    pub(super) fn sync_project_rail_git_stats(&mut self, cx: &mut Context<Self>) {
        if let Some(stats) = self.git.read(cx).stats.clone() {
            self.project_rail_stats
                .values
                .insert(self.workspace.project.clone(), Some(stats));
            if self.workspace.classic_layout {
                self.projects_frame.update(cx, |_, cx| cx.notify());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{EntityInputHandler, TestAppContext, VisualTestContext, size};

    #[gpui::test]
    fn original_layout_keeps_project_icons_when_sessions_are_hidden(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = super::super::tests::window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| shell.toggle_layout(cx))
            .unwrap();
        visual.run_until_parked();
        let projects = visual.debug_bounds("project-rail-frame").unwrap();
        let sessions = visual.debug_bounds("sidebar-frame").unwrap();
        let main = visual.debug_bounds("main-pane-frame").unwrap();
        assert_eq!(projects.left(), px(0.));
        assert_eq!(sessions.left(), projects.right());
        assert_eq!(main.left(), sessions.right());
        let grip = visual.debug_bounds("project-rail-resize").unwrap().center();
        let end = grip + gpui::point(px(28.), px(0.));
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("project-rail-frame")
                .unwrap()
                .size
                .width,
            projects.size.width + px(28.)
        );
        assert_eq!(
            visual.debug_bounds("sidebar-frame").unwrap().size.width,
            sessions.size.width
        );
        let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
        visual.simulate_click(toggle.center(), Default::default());
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("project-rail-frame")
                .unwrap()
                .size
                .width,
            px(48.)
        );
        assert!(visual.debug_bounds("sidebar-frame").is_some());
        window
            .update(visual, |shell, w, cx| shell.toggle_sidebar(w, cx))
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("sidebar-frame").is_none());
        assert_eq!(
            visual
                .debug_bounds("project-rail-frame")
                .unwrap()
                .size
                .width,
            px(48.)
        );
        assert_eq!(
            visual.debug_bounds("main-pane-frame").unwrap().left(),
            px(48.)
        );
        window
            .update(visual, |shell, w, cx| {
                shell.open_settings(crate::settings::Section::Appearance, w, cx);
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("sidebar-frame").is_none());
        assert!(visual.debug_bounds("settings-nav-appearance").is_some());
        window
            .update(visual, |shell, w, cx| shell.close_settings(w, cx))
            .unwrap();
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("project-rail-frame")
                .unwrap()
                .size
                .width,
            px(48.)
        );
    }

    #[gpui::test]
    fn original_project_rail_reorders_groups_and_projects(cx: &mut TestAppContext) {
        cx.update(crate::shell::bind_keys);
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = super::super::tests::window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.project_rail_open = true;
                shell.workspace.preferences.monos = false;
                shell.workspace.project = "/rail/a".into();
                shell.workspace.projects = ["/rail/a", "/rail/b", "/rail/c", "/rail/d"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                shell.workspace.project_rail_groups = [("first", "First"), ("second", "Second")]
                    .into_iter()
                    .map(|(id, name)| workspace::ProjectRailGroup {
                        id: id.into(),
                        name: name.into(),
                        ..Default::default()
                    })
                    .collect();
                shell.workspace.project_rail_assignments = [
                    ("/rail/a", "first"),
                    ("/rail/b", "first"),
                    ("/rail/c", "second"),
                    ("/rail/d", "second"),
                ]
                .into_iter()
                .map(|(p, g)| (p.into(), g.into()))
                .collect();
                shell.invalidate_chrome(cx);
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        let first = visual.debug_bounds("rail-group-first").unwrap();
        let second = visual.debug_bounds("rail-group-second").unwrap();
        let member = visual.debug_bounds("rail-project-button-0").unwrap();
        let delta = second.top() - first.top();
        visual.simulate_mouse_down(first.center(), MouseButton::Left, Default::default());
        window
            .update(visual, |shell, _, _| {
                let drag = shell
                    .project_rail_drag
                    .as_ref()
                    .expect("pressing a group header starts its reorder");
                assert_eq!(drag.path, "first");
                assert_eq!(drag.order, vec!["first", "second"]);
            })
            .unwrap();
        visual.simulate_mouse_move(second.center(), MouseButton::Left, Default::default());
        window
            .update(visual, |shell, _, _| {
                let drag = shell
                    .project_rail_drag
                    .as_ref()
                    .expect("drag survives pointer movement");
                assert!(drag.active);
                assert_eq!(drag.offset, delta);
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(
            visual.debug_bounds("rail-group-first").unwrap().top(),
            first.top() + delta
        );
        let moved = visual.debug_bounds("rail-project-button-0").unwrap();
        assert_eq!(
            moved.top(),
            member.top() + delta,
            "the group's members must travel with its header"
        );
        visual.simulate_mouse_up(second.center(), MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(
                    shell
                        .workspace
                        .project_rail_groups
                        .iter()
                        .map(|g| g.id.as_str())
                        .collect::<Vec<_>>(),
                    vec!["second", "first"]
                );
                assert!(
                    !shell.project_rail_animating(),
                    "reduced motion must settle immediately"
                );
                assert!(shell.project_rail_drag.is_none());
                assert_eq!(shell.workspace.project_rail_assignments["/rail/a"], "first");
                let saved = locked(&shell.store).unwrap().ui_state().unwrap().unwrap();
                let restored = workspace::Workspace::restore(Some(&saved), &[], "/rail/a");
                assert_eq!(restored.project_rail_groups[0].id, "second");
                shell.focus_workspace(w, cx);
            })
            .unwrap();
        let a = visual.debug_bounds("rail-project-button-0").unwrap();
        let b = visual.debug_bounds("rail-project-button-1").unwrap();
        visual.simulate_mouse_down(a.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_move(b.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_up(b.center(), MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert_eq!(
                    shell.workspace.projects,
                    vec!["/rail/b", "/rail/a", "/rail/c", "/rail/d"]
                );
                let saved = locked(&shell.store).unwrap().ui_state().unwrap().unwrap();
                let restored = workspace::Workspace::restore(Some(&saved), &[], "/rail/a");
                assert_eq!(restored.projects, shell.workspace.projects);
            })
            .unwrap();
        let second = visual.debug_bounds("rail-group-second").unwrap();
        let first = visual.debug_bounds("rail-group-first").unwrap();
        visual.simulate_mouse_down(second.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_move(first.center(), MouseButton::Left, Default::default());
        visual.simulate_keystrokes("escape");
        visual.simulate_mouse_up(first.center(), MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert_eq!(
                    shell
                        .workspace
                        .project_rail_groups
                        .iter()
                        .map(|g| g.id.as_str())
                        .collect::<Vec<_>>(),
                    vec!["second", "first"]
                );
                assert!(shell.project_rail_drag.is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn project_rail_virtualizes_projects_without_loading_their_sessions(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = super::super::tests::window(cx);
        let count = window
            .update(cx, |shell, _, cx| {
                for i in 0..1000 {
                    shell
                        .workspace
                        .add_project(&format!("/projects/project-{i}"));
                }
                shell.toggle_layout(cx);
                shell.sessions.len()
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("rail-project-0").is_some());
        assert!(visual.debug_bounds("rail-project-999").is_none());
        window
            .update(visual, |shell, _, _| {
                assert_eq!(shell.sessions.len(), count)
            })
            .unwrap();
    }

    #[gpui::test]
    fn project_rail_and_transcript_stay_cached_during_independent_updates(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, active) = super::super::tests::window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(1500.), px(900.)));
        window
            .update(visual, |shell, _, cx| shell.toggle_layout(cx))
            .unwrap();
        visual.run_until_parked();
        for compact in [false, true] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.project_rail_open = !compact;
                    shell.invalidate_chrome(cx);
                    cx.notify();
                })
                .unwrap();
            visual.run_until_parked();
            let (projects, sidebar, tabs, transcript) = window
                .update(visual, |shell, _, cx| {
                    (
                        shell.projects_frame.clone(),
                        shell.sidebar_frame.clone(),
                        shell.tabs_frame.clone(),
                        shell.sessions[&active].read(cx).transcript.clone(),
                    )
                })
                .unwrap();
            let before = [
                projects.read_with(visual, |v, _| v.renders),
                sidebar.read_with(visual, |v, _| v.renders),
                tabs.read_with(visual, |v, _| v.renders),
            ];
            assert!(before.iter().all(|count| *count > 0));
            window
                .update(visual, |shell, w, cx| {
                    shell.sessions[&active].update(cx, |view, cx| {
                        view.input.update(cx, |input, cx| {
                            input.replace_text_in_range(None, "typing", w, cx)
                        });
                        view.transcript.update(cx, |t, cx| {
                            t.patch(
                                0,
                                vec![monocode_core::transcript::Row {
                                    role: "assistant".into(),
                                    text: "Streaming output".into(),
                                    first: true,
                                    sent_at: None,
                                }],
                                cx,
                            )
                        });
                        cx.notify();
                    });
                })
                .unwrap();
            visual.refresh().unwrap();
            visual.run_until_parked();
            assert_eq!(
                projects.read_with(visual, |v, _| v.renders),
                before[0],
                "streaming must reuse the project rail"
            );
            assert_eq!(sidebar.read_with(visual, |v, _| v.renders), before[1]);
            assert_eq!(tabs.read_with(visual, |v, _| v.renders), before[2]);
            let before = transcript.read_with(visual, |v, _| v.render_passes);
            if !compact {
                let inbox = visual.debug_bounds("sidebar-navigation-Inbox").unwrap();
                visual.simulate_mouse_move(inbox.center(), None, Default::default());
                visual.run_until_parked();
                let notes = visual.debug_bounds("sidebar-navigation-Notes").unwrap();
                visual.simulate_mouse_move(notes.center(), None, Default::default());
                visual.run_until_parked();
                assert_eq!(
                    transcript.read_with(visual, |v, _| v.render_passes),
                    before,
                    "rail hover must not rebuild the chat"
                );
            }
        }
    }

    #[gpui::test]
    fn new_boxy_style_is_independent_of_the_navigation_layout(cx: &mut TestAppContext) {
        let (window, _, active) = super::super::tests::window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open_settings(crate::settings::Section::Appearance, w, cx);
                shell.settings.clone().unwrap().update(cx, |_, cx| {
                    cx.emit(crate::settings::SettingsEvent::NewLayoutBoxy(true))
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.workspace.classic_layout);
                assert!(shell.workspace.new_layout_boxy);
                shell.toggle_layout(cx);
                assert!(shell.workspace.classic_layout);
                shell.toggle_layout(cx);
                assert!(!shell.workspace.classic_layout && shell.workspace.new_layout_boxy);
                assert!(shell.sessions.contains_key(&active));
            })
            .unwrap();
    }
}
