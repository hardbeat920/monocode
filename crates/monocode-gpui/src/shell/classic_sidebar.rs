//! Sidebar.tsx's original workspace sidebar: working copy, tabs, and search.
//! Project navigation lives in ProjectRail, independently of these panes.
use super::*;
use crate::right_panel::RightTab;
use crate::worktree_switcher::{Event, TabSession, WorktreeSwitcher};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum ClassicSidebarTab {
    #[default]
    Sessions,
    Files,
    Changes,
}

impl ClassicSidebarTab {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Files => "files",
            Self::Changes => "changes",
        }
    }

    pub(super) fn from_id(id: &str) -> Self {
        match id {
            "files" => Self::Files,
            "changes" => Self::Changes,
            _ => Self::Sessions,
        }
    }

    pub(super) fn tool_tab(self) -> Option<RightTab> {
        match self {
            Self::Sessions => None,
            Self::Files => Some(RightTab::Files),
            Self::Changes => Some(RightTab::Changes),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Files => "Explorer",
            Self::Changes => "Changes",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Sessions => 0,
            Self::Files => 1,
            Self::Changes => 2,
        }
    }

    fn normalize(order: &[String]) -> Vec<Self> {
        let mut tabs = Vec::with_capacity(3);
        for id in order
            .iter()
            .map(String::as_str)
            .chain(["sessions", "files", "changes"])
        {
            let tab = match id {
                "sessions" => Self::Sessions,
                "files" => Self::Files,
                "changes" => Self::Changes,
                _ => continue,
            };
            if !tabs.contains(&tab) {
                tabs.push(tab);
            }
        }
        tabs
    }
}

#[derive(Default)]
pub(super) struct WorkspaceTabs {
    bounds: [AnchorSlot; 3],
    drag: Option<WorkspaceTabDrag>,
    shifts: HashMap<ClassicSidebarTab, TabShift>,
    skip_click: bool,
}

struct WorkspaceTabDrag {
    tab: ClassicSidebarTab,
    order: Vec<ClassicSidebarTab>,
    bounds: Vec<Bounds<Pixels>>,
    from: usize,
    to: usize,
    origin: Pixels,
    offset: Pixels,
    active: bool,
}

impl WorkspaceTabs {
    pub(super) fn animating(&self) -> bool {
        self.shifts.values().any(|shift| {
            shift.from != shift.to && shift.start.elapsed().as_secs_f32() < REORDER_DURATION
        })
    }

    fn offset(&self, tab: ClassicSidebarTab) -> f32 {
        if let Some(drag) = &self.drag
            && drag.active
            && drag.tab == tab
        {
            return drag.offset.as_f32();
        }
        self.shifts
            .get(&tab)
            .map_or(0., |shift| shift.at(std::time::Instant::now()).0)
    }
}

pub(super) fn session_search(cx: &mut Context<Shell>) -> Entity<input::Input> {
    cx.new(|cx| {
        let mut input = input::Input::field(
            "Search conversations",
            "Search conversations...",
            content(0.35),
            cx,
        );
        input.set_font_size(12.);
        input.set_line_height(20.);
        input
    })
}

pub(super) fn bind_session_search(
    search: &Entity<input::Input>,
    cx: &mut Context<Shell>,
) -> Subscription {
    cx.subscribe(search, |this, input, _: &input::Changed, cx| {
        let query = input.read(cx).content.trim().to_lowercase();
        if this.query == query {
            return;
        }
        this.query = query;
        this.close_preview(cx);
        this.selected_sessions.clear();
        this.selection_anchor = None;
        this.notify_sidebar(cx);
    })
}

pub(super) struct WorktreePicker {
    project: String,
    view: Entity<WorktreeSwitcher>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

struct ChangeStatLayout {
    font_size: f32,
    gap: f32,
    width: f32,
}

/// DiffStat.tsx measures its complete label and scales the font and 0.55em
/// gap together. Measure the same system font as the rendered runs, including
/// tabular digits and weight, rather than estimating from the character count.
fn change_stat_layout(
    added: &str,
    deleted: &str,
    available: f32,
    window: &Window,
) -> ChangeStatLayout {
    let mut font = gpui::font(crate::fonts::UI);
    font.weight = FontWeight::SEMIBOLD;
    font.features = crate::right_panel::tabular();
    let width_at = |text: &str, size: f32| {
        if text.is_empty() {
            return 0.;
        }
        window
            .text_system()
            .shape_line(
                text.to_owned().into(),
                px(size),
                &[gpui::TextRun {
                    len: text.len(),
                    font: font.clone(),
                    color: content(1.),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width
            .as_f32()
            // GPUI's TextLayout rounds each individual run's measured width
            // up before flex layout. Reserve those same rounded boxes.
            .ceil()
    };
    let gap_em = if added.is_empty() || deleted.is_empty() {
        0.
    } else {
        0.55
    };
    let natural = width_at(added, 11.) + width_at(deleted, 11.) + 11. * gap_em;
    // Leave half a point for fractional layout and rasterization rounding.
    let available = (available - 0.5).max(0.);
    let scale = crate::right_panel::diff_stat_scale(available, natural);
    let mut font_size = (11. * scale * 10.).floor() / 10.;
    let mut width = width_at(added, font_size) + width_at(deleted, font_size) + font_size * gap_em;
    // Shaping at the chosen size can differ slightly from a proportional
    // estimate, so keep the final measured glyphs inside the available slot.
    while width > available && font_size > 0.1 {
        font_size = ((font_size * 10.).round() - 1.).max(1.) / 10.;
        width = width_at(added, font_size) + width_at(deleted, font_size) + font_size * gap_em;
    }
    ChangeStatLayout {
        font_size,
        gap: font_size * gap_em,
        width,
    }
}

#[derive(gpui::IntoElement)]
struct ChangeStats {
    added: String,
    deleted: String,
    available: f32,
}

impl gpui::RenderOnce for ChangeStats {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let layout = change_stat_layout(&self.added, &self.deleted, self.available, window);
        div()
            .id("classic-changes-stat")
            .debug_selector(|| "classic-changes-stat".into())
            .w(px(layout.width))
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .items_center()
            .justify_center()
            .whitespace_nowrap()
            .gap(px(layout.gap))
            .text_size(px(layout.font_size))
            .font_family(crate::fonts::UI)
            .font_weight(FontWeight::SEMIBOLD)
            .font_features(crate::right_panel::tabular())
            .when(!self.added.is_empty(), |d| {
                d.child(
                    div()
                        .id("classic-changes-added")
                        .debug_selector(|| "classic-changes-added".into())
                        .flex_shrink_0()
                        .text_color(diff_foreground(true))
                        .child(self.added),
                )
            })
            .when(!self.deleted.is_empty(), |d| {
                d.child(
                    div()
                        .id("classic-changes-deleted")
                        .debug_selector(|| "classic-changes-deleted".into())
                        .flex_shrink_0()
                        .text_color(diff_foreground(false))
                        .child(self.deleted),
                )
            })
    }
}

impl Shell {
    pub(super) fn classic_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_sidebar_structure(cx);
        if self
            .classic_worktree_picker
            .as_ref()
            .is_some_and(|picker| picker.project != self.workspace.project)
        {
            self.classic_worktree_picker = None;
        }
        let sessions = self.classic_sidebar_tab == ClassicSidebarTab::Sessions;
        let live_agents = if self.workspace.project_rail_open {
            None
        } else {
            self.live_agents_preview(cx)
        };
        div()
            .id("monocode-sidebar")
            .debug_selector(|| "sidebar-content".into())
            .track_focus(&self.sidebar_focus)
            .when(sessions, |d| {
                d.key_context("SessionSidebar")
                    .on_action(cx.listener(
                        |this, _: &session_groups::RenameSidebarItem, window, cx| {
                            this.rename_sidebar_item(window, cx)
                        },
                    ))
                    .on_action(cx.listener(
                        |this, _: &session_groups::DeleteSidebarSelection, window, cx| {
                            this.delete_sidebar_selection(window, cx)
                        },
                    ))
            })
            .w(px(self.workspace.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("classic-workspace-header")
                    .debug_selector(|| "classic-workspace-header".into())
                    .h(px(40.))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(content(0.07))
                    .child(
                        div()
                            .relative()
                            .h_full()
                            .pl_3()
                            .pr_1p5()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(titlebar_drag_area().absolute().top_0().left_0().size_full())
                            .child(self.classic_workspace_picker(cx))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_0p5()
                                    .child(self.file_search_button(cx))
                                    .child(
                                        icon_button(
                                            "classic-new-session",
                                            if cfg!(target_os = "macos") {
                                                "New session (⌘T)"
                                            } else {
                                                "New session (Ctrl+T)"
                                            },
                                            "plus",
                                        )
                                        .debug_selector(|| "classic-new-session".into())
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| this.new_chat(cx))),
                                    ),
                            ),
                    ),
            )
            .when(self.workspace.project_rail_open, |d| {
                d.child(self.classic_workspace_tabs(cx))
            })
            .when(
                sessions && !self.workspace.project.is_empty() && self.workspace.project != "~",
                |d| d.child(self.classic_conversation_search(cx)),
            )
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    div()
                        .relative()
                        .size_full()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(if sessions {
                            self.virtual_sidebar(cx)
                        } else {
                            self.classic_tool_pane(cx)
                        }),
                ),
            )
            .children(live_agents)
            .when(!self.workspace.project_rail_open, |d| {
                d.child(self.app_update_footer(cx))
            })
            .children(self.classic_worktree_popover(cx))
            .children(self.classic_session_preview(cx))
            .into_any()
    }

    fn classic_workspace_picker(&self, cx: &Context<Self>) -> AnyElement {
        if self.workspace.project.is_empty()
            || self.workspace.project == "~"
            || self.workspace.project.starts_with("remote://")
        {
            return div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(content(1.))
                .child("Workspace")
                .into_any();
        }
        let focus = self.worktree_focus.get(&self.workspace.project);
        let title = focus
            .map(|focus| focus.branch.as_deref().unwrap_or("Detached worktree"))
            .unwrap_or("Workspace")
            .to_owned();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .child(
                div()
                    .id("classic-workspace-picker")
                    .debug_selector(|| "classic-workspace-picker".into())
                    .role(Role::Button)
                    .aria_label("Switch working copy")
                    .aria_description(self.working_copy().to_owned())
                    .aria_expanded(self.classic_worktree_picker.is_some())
                    .cursor_pointer()
                    .relative()
                    .h(px(26.))
                    .ml(px(-6.))
                    .px_1p5()
                    .max_w_full()
                    .min_w_0()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .hover(|s| s.bg(content(0.08)))
                    .when(self.classic_worktree_picker.is_some(), |d| {
                        d.bg(content(0.08))
                    })
                    .child(div().min_w_0().truncate().child(title))
                    .child(icon("chevrons-up-down", 14.).text_color(content(0.45)))
                    .child(self.project_anchor.probe())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.classic_worktree_picker.is_some() {
                            this.close_classic_worktree_picker(window, cx);
                        } else {
                            this.open_classic_worktree_picker(window, cx);
                        }
                    }))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "down" && this.classic_worktree_picker.is_none() {
                            this.open_classic_worktree_picker(window, cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .into_any()
    }

    fn classic_workspace_tabs(&self, cx: &Context<Self>) -> AnyElement {
        let stats = self.git.read(cx).stats.as_ref();
        let (additions, deletions) =
            stats.map_or((0, 0), |s| (s.additions.max(0), s.deletions.max(0)));
        let changes = additions > 0 || deletions > 0;
        let added = if additions > 0 {
            format!("+{}", crate::right_panel::format_integer(additions))
        } else {
            String::new()
        };
        let deleted = if deletions > 0 {
            format!("-{}", crate::right_panel::format_integer(deletions))
        } else {
            String::new()
        };
        // Three equal tabs share the strip after px-2 and its two 1pt gaps;
        // each button has a further 8pt of horizontal padding per side.
        let changes_width = ((self.workspace.sidebar_width - 18.) / 3. - 16.).max(0.);
        let reordering = self
            .classic_tabs
            .drag
            .as_ref()
            .is_some_and(|drag| drag.active);
        div()
            .id("classic-workspace-tabs")
            .debug_selector(|| "classic-workspace-tabs".into())
            .role(Role::TabList)
            .aria_label("Workspace")
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_px()
            .px_2()
            .border_b_1()
            .border_color(content(0.07))
            .children(
                ClassicSidebarTab::normalize(&self.workspace.classic_sidebar_tab_order)
                    .into_iter()
                    .map(|tab| {
                        let active = self.classic_sidebar_tab == tab;
                        let offset = self.classic_tabs.offset(tab);
                        let dragging = self
                            .classic_tabs
                            .drag
                            .as_ref()
                            .is_some_and(|drag| drag.active && drag.tab == tab);
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .child(self.classic_tabs.bounds[tab.index()].probe())
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "classic-workspace-tab-{}",
                                        tab.id()
                                    )))
                                    .debug_selector(move || {
                                        format!("classic-workspace-tab-{}", tab.id())
                                    })
                                    .role(Role::Tab)
                                    .aria_label(if tab == ClassicSidebarTab::Changes && changes {
                                        format!("Changes +{additions} -{deletions}")
                                    } else {
                                        tab.label().to_owned()
                                    })
                                    .aria_selected(active)
                                    .cursor_pointer()
                                    .when(dragging, |d| d.cursor(CursorStyle::ClosedHand))
                                    .relative()
                                    .left(px(offset))
                                    .h(px(24.))
                                    .w_full()
                                    .min_w_0()
                                    .px_2()
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.))
                                    .text_color(content(if active { 1. } else { 0.5 }))
                                    .when(active, |d| d.bg(content(0.1)))
                                    .when(!active && (!reordering || dragging), |d| {
                                        d.hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                                    })
                                    .when(dragging && !active, |d| {
                                        d.bg(content(0.05)).text_color(content(1.))
                                    })
                                    .child(if tab == ClassicSidebarTab::Changes && changes {
                                        ChangeStats {
                                            added: added.clone(),
                                            deleted: deleted.clone(),
                                            available: changes_width,
                                        }
                                        .into_any_element()
                                    } else {
                                        div().min_w_0().truncate().child(tab.label()).into_any()
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event, window, cx| {
                                            window.focus(&this.focus, cx);
                                            this.start_classic_sidebar_tab(tab, event);
                                            cx.stop_propagation();
                                        }),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if std::mem::take(&mut this.classic_tabs.skip_click) {
                                            return;
                                        }
                                        this.select_classic_sidebar_tab(tab, window, cx)
                                    })),
                            )
                    }),
            )
            .into_any()
    }

    fn classic_conversation_search(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("classic-conversation-search")
            .debug_selector(|| "classic-conversation-search".into())
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(content(0.07))
            .child(
                div()
                    .relative()
                    .h(px(28.))
                    .flex_1()
                    .min_w_0()
                    .pl(px(28.))
                    .pr_2()
                    .flex()
                    .items_center()
                    .child(
                        icon("search", 12.)
                            .absolute()
                            .left_2()
                            .top_2()
                            .text_color(content(0.5)),
                    )
                    .child(self.classic_session_search.clone())
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            if !this.classic_session_search.read(cx).content.is_empty() {
                                this.classic_session_search
                                    .update(cx, |input, cx| input.set_text(String::new(), cx));
                            }
                            cx.stop_propagation();
                        }
                    })),
            )
            .child(self.sidebar_filter_button(cx))
            .into_any()
    }

    fn start_classic_sidebar_tab(&mut self, tab: ClassicSidebarTab, event: &MouseDownEvent) {
        let order = ClassicSidebarTab::normalize(&self.workspace.classic_sidebar_tab_order);
        let Some(bounds) = order
            .iter()
            .map(|tab| self.classic_tabs.bounds[tab.index()].get())
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let from = order
            .iter()
            .position(|candidate| *candidate == tab)
            .unwrap_or(0);
        self.classic_tabs.skip_click = false;
        self.classic_tabs.shifts.clear();
        self.classic_tabs.drag = Some(WorkspaceTabDrag {
            tab,
            order,
            bounds,
            from,
            to: from,
            origin: event.position.x,
            offset: px(0.),
            active: false,
        });
    }

    pub(super) fn move_classic_sidebar_tab(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.cancel_classic_sidebar_tab(cx);
            return;
        }
        let Some(drag) = &mut self.classic_tabs.drag else {
            return;
        };
        let offset = event.position.x - drag.origin;
        if !drag.active && offset.abs() < px(5.) {
            return;
        }
        drag.active = true;
        let base = drag.bounds[drag.from].left();
        let slots: Vec<_> = drag
            .bounds
            .iter()
            .map(|bounds| bounds.left() - base)
            .collect();
        drag.offset = offset.max(slots[0]).min(*slots.last().unwrap());
        let to = slots
            .iter()
            .enumerate()
            .map(|(ix, slot)| (ix, (slot.as_f32() - drag.offset.as_f32()).abs()))
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map_or(drag.from, |(ix, _)| ix);
        if drag.to != to {
            drag.to = to;
            let mut preview = drag.order.clone();
            let moved = preview.remove(drag.from);
            preview.insert(to, moved);
            let now = std::time::Instant::now();
            for (original, tab) in drag.order.iter().enumerate() {
                if *tab == drag.tab {
                    continue;
                }
                let to = preview
                    .iter()
                    .position(|candidate| candidate == tab)
                    .unwrap();
                let target = (drag.bounds[to].left() - drag.bounds[original].left()).as_f32();
                let from = self
                    .classic_tabs
                    .shifts
                    .get(tab)
                    .map_or(0., |shift| shift.at(now).0);
                self.classic_tabs.shifts.insert(
                    *tab,
                    TabShift {
                        from: if cx.reduce_motion() { target } else { from },
                        to: target,
                        start: now,
                    },
                );
            }
        }
        self.sidebar_frame.update(cx, |_, cx| cx.notify());
    }

    pub(super) fn end_classic_sidebar_tab(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.classic_tabs.drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        let mut reordered = drag.order.clone();
        let tab = reordered.remove(drag.from);
        reordered.insert(drag.to, tab);
        let now = std::time::Instant::now();
        for (previous, tab) in drag.order.iter().enumerate() {
            let destination = reordered
                .iter()
                .position(|candidate| candidate == tab)
                .unwrap();
            let old_offset = if *tab == drag.tab {
                drag.offset.as_f32()
            } else {
                self.classic_tabs
                    .shifts
                    .get(tab)
                    .map_or(0., |shift| shift.at(now).0)
            };
            let from = (drag.bounds[previous].left() - drag.bounds[destination].left()).as_f32()
                + old_offset;
            self.classic_tabs.shifts.insert(
                *tab,
                TabShift {
                    from,
                    to: 0.,
                    start: now,
                },
            );
        }
        self.classic_tabs.skip_click = true;
        if cx.reduce_motion() {
            self.classic_tabs.shifts.clear();
        }
        self.workspace.classic_sidebar_tab_order = reordered
            .into_iter()
            .map(|tab| tab.id().to_owned())
            .collect();
        self.persist(cx);
        self.sidebar_frame.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn cancel_classic_sidebar_tab(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.classic_tabs.drag.take() else {
            return false;
        };
        if drag.active {
            let now = std::time::Instant::now();
            for tab in drag.order {
                let from = if tab == drag.tab {
                    drag.offset.as_f32()
                } else {
                    self.classic_tabs
                        .shifts
                        .get(&tab)
                        .map_or(0., |shift| shift.at(now).0)
                };
                self.classic_tabs.shifts.insert(
                    tab,
                    TabShift {
                        from,
                        to: 0.,
                        start: now,
                    },
                );
            }
            self.classic_tabs.skip_click = true;
            if cx.reduce_motion() {
                self.classic_tabs.shifts.clear();
            }
            self.sidebar_frame.update(cx, |_, cx| cx.notify());
        }
        true
    }

    pub(super) fn close_classic_worktree_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(picker) = self.classic_worktree_picker.take() {
            if let Some(restore) = picker.restore {
                window.focus(&restore, cx);
            } else {
                self.focus_workspace(window, cx);
            }
            self.notify_sidebar(cx);
            cx.notify();
        }
    }

    fn open_classic_worktree_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.close_preview(cx);
        let project = self.workspace.project.clone();
        let ids = self
            .order
            .iter()
            .copied()
            .filter(|id| self.session_metadata(*id, cx).summary.cwd == project)
            .collect();
        let sessions = self
            .workspace
            .tabs
            .iter()
            .flat_map(|tab| {
                self.workspace
                    .layout(*tab)
                    .leaf_ids()
                    .into_iter()
                    .filter_map(|id| {
                        if !self.session_summaries.contains_key(&id) {
                            return None;
                        }
                        let session = self.session_metadata(id, cx);
                        (session.summary.cwd == project).then(|| TabSession {
                            id,
                            tab: *tab,
                            cwd: self.session_working_copy(id, cx).to_owned(),
                            workspace: self.workspace.tab_workspaces.get(tab).cloned(),
                            busy: session.busy,
                        })
                    })
            })
            .collect();
        let focused = self.worktree_focus.get(&project).cloned();
        let view = cx.new(|cx| {
            WorktreeSwitcher::new(
                project.clone(),
                focused,
                sessions,
                ids,
                self.store.clone(),
                cx,
            )
        });
        let observed_project = project.clone();
        let events = cx.subscribe_in(&view, window, move |this, _, event: &Event, window, cx| {
            match event {
                Event::Metadata(paths) => {
                    for (id, path) in paths {
                        this.session_worktrees.insert(*id, path.clone());
                        if let Some(session) = this.sessions.get(id) {
                            let path = session.update(cx, |session, cx| {
                                session.restore_worktree(path.clone(), None, cx)
                            });
                            this.session_worktrees.insert(*id, path);
                        }
                    }
                    this.sync_git(cx);
                    this.invalidate_chrome(cx);
                }
                Event::Selected(focus) if this.workspace.project == observed_project => {
                    this.classic_worktree_picker = None;
                    this.select_worktree(focus.clone(), window, cx);
                    this.notify_sidebar(cx);
                }
                Event::Selected(_) | Event::Dismissed => {
                    this.close_classic_worktree_picker(window, cx);
                }
            }
            cx.notify();
        });
        let restore = window.focused(cx);
        window.focus(&view.read(cx).focus(cx), cx);
        self.classic_worktree_picker = Some(WorktreePicker {
            project,
            view,
            restore,
            _events: events,
        });
        self.notify_sidebar(cx);
        cx.notify();
    }

    fn classic_worktree_popover(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let picker = self.classic_worktree_picker.as_ref()?;
        let weak = cx.weak_entity();
        Some(
            popover(
                "classic-worktree-picker",
                Anchor::Slot(self.project_anchor.clone()),
            )
            .width(280.)
            .max_height(360.)
            .align(Align::Start)
            .gap(6.)
            .on_dismiss(move |window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.close_classic_worktree_picker(window, cx)
                });
            })
            .child(picker.view.clone())
            .into_any_element(),
        )
    }

    fn classic_session_preview(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.classic_sidebar_tab != ClassicSidebarTab::Sessions {
            return None;
        }
        let preview = self.session_preview.as_ref()?;
        let id = preview.id;
        Some(
            popover("session-preview", Anchor::Slot(preview.anchor.clone()))
                .side(Side::Right)
                .gap(8.)
                .width(260.)
                .max_height(440.)
                .track(preview.surface.clone())
                .child(
                    div()
                        .id("session-preview-content")
                        .role(Role::Tooltip)
                        .max_h(px(438.))
                        .overflow_y_scroll()
                        .on_hover(cx.listener(move |this, hovered, window, cx| {
                            if this
                                .session_preview
                                .as_ref()
                                .is_none_or(|preview| preview.id != id)
                            {
                                return;
                            }
                            if *hovered {
                                this.preview_close = None;
                            } else {
                                this.schedule_preview_close(window, cx);
                            }
                        }))
                        .child(self.sessions.get(&id).map_or_else(
                            || {
                                session_preview::render(
                                    self.session_metadata(id, cx).summary,
                                    preview.data.as_ref(),
                                    self.unseen_finished.contains(&id),
                                )
                            },
                            |view| view.read(cx).preview(self.unseen_finished.contains(&id)),
                        )),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, WindowHandle, point, size};

    fn original_window(cx: &mut TestAppContext) -> WindowHandle<Shell> {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = super::super::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.classic_layout = true;
                shell.workspace.project_rail_open = true;
                shell.classic_sidebar_tab = ClassicSidebarTab::Sessions;
                shell.layout_changed(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
    }

    fn tab_has_fill(
        window: WindowHandle<Shell>,
        bounds: Bounds<Pixels>,
        alpha: f32,
        cx: &mut VisualTestContext,
    ) -> bool {
        window
            .update(cx, |_, window, _| {
                let scale = window.scale_factor();
                window.painted_quads().iter().any(|quad| {
                    (quad.bounds.left().0 - bounds.left().as_f32() * scale).abs() < 0.5
                        && (quad.bounds.top().0 - bounds.top().as_f32() * scale).abs() < 0.5
                        && (quad.bounds.size.width.0 - bounds.size.width.as_f32() * scale).abs()
                            < 0.5
                        && (quad.bounds.size.height.0 - bounds.size.height.as_f32() * scale).abs()
                            < 0.5
                        && quad
                            .background
                            .as_solid()
                            .is_some_and(|color| (color.a - alpha).abs() < 0.001)
                })
            })
            .unwrap()
    }

    #[gpui::test]
    fn original_workspace_tabs_hover_without_replacing_the_selected_fill(cx: &mut TestAppContext) {
        let window = original_window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(1500.), px(900.)));
        visual.simulate_mouse_move(
            point(px(1200.), px(500.)),
            None::<MouseButton>,
            Default::default(),
        );
        visual.run_until_parked();
        let sessions = visual
            .debug_bounds("classic-workspace-tab-sessions")
            .unwrap();
        let files = visual.debug_bounds("classic-workspace-tab-files").unwrap();
        assert!(tab_has_fill(window, sessions, 0.1, visual));
        assert!(!tab_has_fill(window, files, 0.05, visual));

        visual.simulate_mouse_move(files.center(), None::<MouseButton>, Default::default());
        visual.run_until_parked();
        assert!(
            tab_has_fill(window, files, 0.05, visual),
            "an inactive workspace tab gains the Tauri hover wash"
        );
        assert!(
            tab_has_fill(window, sessions, 0.1, visual),
            "hovering another tab keeps the current selection"
        );

        visual.simulate_mouse_move(sessions.center(), None::<MouseButton>, Default::default());
        visual.run_until_parked();
        assert!(tab_has_fill(window, sessions, 0.1, visual));
        assert!(
            !tab_has_fill(window, files, 0.05, visual),
            "the hover wash follows the pointer"
        );
        assert!(
            !tab_has_fill(window, sessions, 0.05, visual),
            "the selected tab keeps its stronger wash on hover"
        );
    }

    #[gpui::test]
    fn original_workspace_changes_counts_fit_completely_after_sidebar_resize(
        cx: &mut TestAppContext,
    ) {
        let window = original_window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(1500.), px(900.)));
        for (width, additions, deletions) in [
            (220., 5_476, 408),
            (260., 5_476, 408),
            (360., 5_476, 408),
            (220., 99_999_999, 99_999_999),
            (220., 5_476, 0),
            (220., 0, 408),
        ] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.sidebar_width = width;
                    shell.git.update(cx, |git, cx| {
                        git.stats = Some(monocode_core::scm::DiffStats {
                            files: 1,
                            additions,
                            deletions,
                        });
                        cx.notify();
                    });
                    shell.notify_sidebar(cx);
                    cx.notify();
                })
                .unwrap();
            visual.run_until_parked();
            let tab = visual
                .debug_bounds("classic-workspace-tab-changes")
                .unwrap();
            let stat = visual.debug_bounds("classic-changes-stat").unwrap();
            assert!(stat.left() >= tab.left() + px(8.));
            assert!(
                stat.right() <= tab.right() - px(8.),
                "the complete diff count stays inside its padded tab at {width}pt"
            );
            let added = visual.debug_bounds("classic-changes-added");
            let deleted = visual.debug_bounds("classic-changes-deleted");
            assert_eq!(added.is_some(), additions > 0);
            assert_eq!(deleted.is_some(), deletions > 0);
            for (name, count) in [("added", added), ("deleted", deleted)]
                .into_iter()
                .filter_map(|(name, count)| count.map(|count| (name, count)))
            {
                assert!(count.left() >= stat.left() - px(0.25));
                assert!(
                    count.right() <= stat.right() + px(0.25),
                    "both full glyph runs fit, including every deletion digit: sidebar={width}, counts=+{additions}/-{deletions}, run={name}, run_bounds={count:?}, stat_bounds={stat:?}, tab_bounds={tab:?}"
                );
            }
            if let (Some(added), Some(deleted)) = (added, deleted) {
                assert!(
                    added.right() < deleted.left(),
                    "the separating gap scales along with both numbers"
                );
            }
        }
    }
}
