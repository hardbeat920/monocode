//! The 40px title bar with workspace tabs. Port of src/app/shell/TitleBar.tsx
//! (`TitleBarComponent`, `TitleTabItem`, `TabHarnesses`, `TabVisitNav`).

mod actions;
mod model;

use std::collections::{HashMap, HashSet};

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, Point, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div,
};
use monocode_engine::attention::Attention;
use monocode_ui::widgets::{icon_button, spinner, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, provider_logo, u};

use super::{Shell, WhenMac as _, drag_region};
pub use model::{HarnessState, TabLead, TitleTabView, title_tab_view};

/// A tab slot is `w-56 min-w-28`; at 176px and wider its text splits into a
/// 10px headline and a meta line (`@min-[11rem]`).
const TAB_WIDTH: f32 = 224.0;
const TAB_MIN_WIDTH: f32 = 112.0;

/// The title bar of one window.
pub struct TitleBar {
    shell: WeakEntity<Shell>,
    /// The tab strip, and the tab it last scrolled to.
    scroll: ScrollHandle,
    scrolled_tab: Option<String>,
    title_drop: Option<monocode_layout::pane_drop::TitleTabDrop>,
    menu: Option<(String, Point<Pixels>)>,
}

impl TitleBar {
    pub(super) fn has_open_menu(&self) -> bool {
        self.menu.is_some()
    }

    pub fn new(shell: WeakEntity<Shell>, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            shell,
            scroll: ScrollHandle::new(),
            scrolled_tab: None,
            title_drop: None,
            menu: None,
        }
    }

    /// The window's title tabs and the active tab id.
    fn tabs(&self, cx: &App) -> (Vec<TitleTabView>, String) {
        let Some(shell) = self.shell.upgrade() else {
            return (Vec::new(), String::new());
        };
        let Some(workspace) = shell.read(cx).workspace.clone() else {
            return (Vec::new(), String::new());
        };
        let unseen: HashSet<String> = Attention::try_global(cx)
            .map(|attention| attention.notifier.read(cx).unseen_finished_ids().clone())
            .unwrap_or_default();
        let workspace = workspace.read(cx);
        let tabs = workspace
            .title_tabs(&unseen, cx)
            .iter()
            .map(title_tab_view)
            .collect();
        (tabs, workspace.active_tab_id().to_string())
    }

    fn with_shell(&self, cx: &mut App, f: impl FnOnce(&mut Shell, &mut Context<Shell>)) {
        self.shell.update(cx, f).ok();
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().into_any_element();
        };
        let layout = shell.read(cx).layout.clone();
        let (tabs, active_tab_id) = self.tabs(cx);
        let ids = tabs
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<HashSet<_>>();
        if self.menu.as_ref().is_some_and(|(id, _)| !ids.contains(id)) {
            self.menu = None;
        }
        let weak = cx.weak_entity();
        cx.default_global::<TitleTabBounds>()
            .0
            .entry(window.window_handle().window_id())
            .or_default()
            .title_bar = Some(weak);
        cx.default_global::<TitleTabBounds>()
            .0
            .entry(window.window_handle().window_id())
            .or_default()
            .tabs
            .retain(|id, _| ids.contains(id));
        if !active_tab_id.is_empty()
            && self.scrolled_tab.as_deref() != Some(active_tab_id.as_str())
            && let Some(index) = tabs.iter().position(|tab| tab.id == active_tab_id)
        {
            self.scroll.scroll_to_item(index);
            self.scrolled_tab = Some(active_tab_id.clone());
        }
        self.render_title_bar(&layout, &tabs, &active_tab_id, cx)
            .into_any_element()
    }
}

impl TitleBar {
    fn render_title_bar(
        &self,
        layout: &super::ShellLayout,
        tabs: &[TitleTabView],
        active_tab_id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let compact_title_bar = layout.compact_title_bar();
        let rail_closed = !layout.project_rail_open;

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
                    .child(tab_visit_nav(&self.shell, None, cx)),
            );
        }
        if !layout.session_sidebar_open {
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
                                this.with_shell(cx, |shell, cx| shell.toggle_session_sidebar(cx));
                            })),
                    ),
            );
        }

        // The strip scrolls sideways once the tabs reach their minimum
        // width, and keeps the active tab in view.
        let mut strip = div()
            .id("title-tabs")
            .relative()
            .child(
                gpui::canvas(
                    |bounds, window, cx| {
                        cx.default_global::<TitleTabBounds>()
                            .0
                            .entry(window.window_handle().window_id())
                            .or_default()
                            .strip = Some(bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .items_center()
            .gap(u(2.))
            .pl(u(6.))
            .pr(u(10.))
            .overflow_x_scroll()
            .track_scroll(&self.scroll);
        for (index, tab) in tabs.iter().enumerate() {
            let active = tab.id == active_tab_id;
            strip = strip.child(self.render_title_tab(
                index,
                tab,
                active,
                model::tab_closable(tab, tabs.len()),
                &theme,
                cx,
            ));
        }
        bar = bar.child(drag_region(strip, self.shell.clone()));

        if rail_closed {
            bar = bar.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(2.))
                    .px(u(8.))
                    .child(
                        icon_button("title-goto", IconName::Search)
                            .tooltip("Go to File (⌘P)")
                            .on_click(cx.listener(|_, _, window, cx| {
                                window.dispatch_action(Box::new(super::keymap::GoToFile), cx);
                            })),
                    )
                    .child(
                        icon_button("title-new", IconName::Plus)
                            .tooltip("New session (⌘T)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.with_shell(cx, |shell, cx| {
                                    shell.new_session(cx);
                                })
                            })),
                    ),
            );
        }
        bar.children(self.render_tab_menu(tabs, cx))
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
            .on_drag(monocode_view_workbench::panes::pane_tree::PaneDragSource::WorkspaceTab(tab.id.clone()), {
                let label = tab.headline.clone();
                move |_, _, _, cx| cx.new(|_| crate::panes::drag::WorkspaceDragPreview { label: label.clone() })
            })
            .on_drop({
                let target = tab.id.clone();
                cx.listener(move |this, source: &monocode_view_workbench::panes::pane_tree::PaneDragSource, window, cx| {
                    let monocode_view_workbench::panes::pane_tree::PaneDragSource::WorkspaceTab(moved) = source else { return };
                    let Some((_, position)) = title_tab_hit_test(window.mouse_position(), window, cx) else { return };
                    this.with_shell(cx, |shell, cx| {
                        let Some(workspace) = &shell.workspace else { return };
                        let mut ids = workspace.read(cx).tabs().iter().map(|tab| tab.id.clone()).collect::<Vec<_>>();
                        ids.retain(|id| id != moved);
                        if let Some(index) = ids.iter().position(|id| id == &target) {
                            let index = index + usize::from(position == monocode_layout::pane_drop::TitleTabDropPosition::After);
                            ids.insert(index, moved.clone());
                            workspace.update(cx, |workspace, cx| workspace.reorder_tabs(&ids, Some(moved), cx));
                        }
                    });
                })
            })
            .on_click({
                let id = tab.id.clone();
                let preview_file_id = tab.preview_file_id.clone();
                cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.with_shell(cx, |shell, cx| {
                        shell.activate_tab(&id, cx);
                        if event.click_count() == 2
                            && let Some(file_id) = &preview_file_id
                            && let Some(workspace) = &shell.workspace
                        {
                            workspace.update(cx, |workspace, cx| workspace.pin_file(file_id, cx));
                        }
                    });
                })
            })
            .on_mouse_down(MouseButton::Right, {
                let id = tab.id.clone();
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.menu = Some((id.clone(), event.position));
                    cx.notify();
                })
            })
            .on_mouse_down(MouseButton::Middle, {
                let id = tab.id.clone();
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if closable {
                        this.with_shell(cx, |shell, cx| shell.close_tab(&id, cx));
                    }
                })
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
            .child(
                gpui::canvas(
                    {
                        let id = tab.id.clone();
                        move |bounds, window, cx| {
                            cx.default_global::<TitleTabBounds>()
                                .0
                                .entry(window.window_handle().window_id())
                                .or_default()
                                .tabs
                                .insert(id.clone(), bounds);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(button);
        if let Some(drop) = self
            .title_drop
            .as_ref()
            .filter(|drop| drop.target_tab_id == tab.id)
        {
            let marker = div()
                .absolute()
                .top(u(4.0))
                .bottom(u(4.0))
                .w(u(2.0))
                .bg(c.accent);
            slot = slot.child(
                if drop.position == monocode_layout::pane_drop::TitleTabDropPosition::Before {
                    marker.left(u(-2.0))
                } else {
                    marker.right(u(-2.0))
                },
            );
        }
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
                            this.with_shell(cx, |shell, cx| shell.close_tab(&id, cx))
                        })
                    }),
            );
        }
        slot
    }
}

/// `TabVisitNav`: back, forward, and the project panel toggle. `panel` is
/// `Some(active)` when the toggle shows (`onTogglePanel`).
pub(crate) fn tab_visit_nav(
    shell: &WeakEntity<Shell>,
    panel: Option<bool>,
    cx: &App,
) -> impl IntoElement {
    let (can_go_back, can_go_forward) = shell
        .upgrade()
        .map(|shell| shell.read(cx).visit_nav(cx))
        .unwrap_or((false, false));
    let back = shell.clone();
    let forward = shell.clone();
    let toggle = shell.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(
            icon_button("nav-back", IconName::ChevronLeft)
                .disabled(!can_go_back)
                .tooltip("Back (⌘[)")
                .on_click(move |_, _, cx| {
                    back.update(cx, |shell, cx| shell.go_back(cx)).ok();
                }),
        )
        .child(
            icon_button("nav-forward", IconName::ChevronRight)
                .disabled(!can_go_forward)
                .tooltip("Forward (⌘])")
                .on_click(move |_, _, cx| {
                    forward.update(cx, |shell, cx| shell.go_forward(cx)).ok();
                }),
        )
        .when_some(panel, |el, active| {
            el.child(
                icon_button("nav-projects", IconName::PanelLeft)
                    .active(active)
                    .tooltip("Toggle Projects")
                    .on_click(move |_, _, cx| {
                        toggle
                            .update(cx, |shell, cx| shell.toggle_project_rail(cx))
                            .ok();
                    }),
            )
        })
}

use gpui::prelude::FluentBuilder as _;

#[derive(Default)]
struct TabRects {
    title_bar: Option<WeakEntity<TitleBar>>,
    strip: Option<gpui::Bounds<gpui::Pixels>>,
    tabs: HashMap<String, gpui::Bounds<gpui::Pixels>>,
}
#[derive(Default)]
struct TitleTabBounds(HashMap<gpui::WindowId, TabRects>);
impl gpui::Global for TitleTabBounds {}

pub(crate) fn title_tab_hit_test(
    position: gpui::Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Option<(String, monocode_layout::pane_drop::TitleTabDropPosition)> {
    let rects = cx
        .try_global::<TitleTabBounds>()?
        .0
        .get(&window.window_handle().window_id())?;
    if !rects.strip?.contains(&position) {
        return None;
    }
    let hovered = rects
        .tabs
        .iter()
        .find(|(_, bounds)| bounds.contains(&position))
        .map(|(id, _)| id.as_str());
    let mut tabs = rects
        .tabs
        .iter()
        .map(|(id, bounds)| monocode_layout::pane_drop::TitleTabHit {
            id: id.clone(),
            left: f32::from(bounds.origin.x) as f64,
            width: f32::from(bounds.size.width) as f64,
        })
        .collect::<Vec<_>>();
    tabs.sort_by(|a, b| a.left.total_cmp(&b.left));
    monocode_layout::pane_drop::title_tab_drop_from_point(
        f32::from(position.x) as f64,
        hovered,
        &tabs,
    )
}

pub(crate) fn set_title_tab_drop(
    drop: Option<monocode_layout::pane_drop::TitleTabDrop>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx
        .try_global::<TitleTabBounds>()
        .and_then(|global| global.0.get(&window.window_handle().window_id()))
        .and_then(|rects| rects.title_bar.clone());
    if let Some(view) = view {
        view.update(cx, |bar, cx| {
            if bar.title_drop != drop {
                bar.title_drop = drop;
                cx.notify();
            }
        })
        .ok();
    }
}
pub(crate) fn forget_window_bounds(id: gpui::WindowId, cx: &mut App) {
    cx.default_global::<TitleTabBounds>().0.remove(&id);
}
