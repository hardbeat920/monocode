// Navigation and frame style are independent. The original layout adds a
// project rail beside the sessions sidebar. The new layout keeps one sidebar
// and offers either an inset rounded card or the existing full-height boxy frame.
use super::{CORNER_BUTTONS_WIDTH, CORNER_CONTROL_GAP, Shell, titlebar_drag_area};
use crate::{
    popover::{Anchor, popover},
    project_tabs::{TabReveal, tab_button},
    resource_pressure::SessionInfo,
    right_panel::{RightPanel, RightTab},
    theme::*,
    workspace::TitleTab,
};
use gpui::{
    AnyElement, Context, CursorStyle, Entity, MouseButton, MouseDownEvent, Role, StyleRefinement,
    Window, div, prelude::*, px,
};
use std::collections::HashMap;

const CARD_RADIUS: f32 = 10.;

// GPUI overflow clips rectangles. Capture outside the bordered frame so the
// final mask includes its background, border and children at the outer radius.
// A mask inside the frame only covers its inset content rectangle.
fn card_corner_mask(begin: bool) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let r = px(CARD_RADIUS)
                .min(bounds.size.width / 2.)
                .min(bounds.size.height / 2.);
            #[cfg(target_os = "macos")]
            let corners = [
                bounds.origin,
                gpui::point(bounds.right() - r, bounds.top()),
                gpui::point(bounds.right() - r, bounds.bottom() - r),
                gpui::point(bounds.left(), bounds.bottom() - r),
            ];
            #[cfg(target_os = "macos")]
            for (corner, origin) in corners.into_iter().enumerate() {
                window.paint_quad(
                    gpui::fill(gpui::Bounds::new(origin, gpui::size(r, r)), gpui::black())
                        .border_color(gpui_apple::metal_renderer::corner_clip_marker(
                            begin,
                            corner as u8,
                        )),
                );
            }
            if !begin {
                // The areas outside the curve belong to the surrounding
                // chrome, just like the gutters and footer. Restoring the
                // window's unwashed glass here leaves dark square corners.
                #[cfg(target_os = "macos")]
                let outside = inset_chrome_wash();
                // Without the mask an opaque fill covers whatever the card
                // painted past its curve. Over Windows' glass the root's
                // translucent fill is already there, and a second one darkens
                // the corners.
                #[cfg(target_os = "windows")]
                let outside = if shell_fill().a < 1. {
                    inset_chrome_wash()
                } else {
                    shell_fill().blend(inset_chrome_wash())
                };
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let outside = shell_fill().blend(inset_chrome_wash());
                let vertices = [
                    (
                        bounds.origin,
                        gpui::point(bounds.left() + r, bounds.top()),
                        gpui::point(bounds.left(), bounds.top() + r),
                    ),
                    (
                        gpui::point(bounds.right(), bounds.top()),
                        gpui::point(bounds.right(), bounds.top() + r),
                        gpui::point(bounds.right() - r, bounds.top()),
                    ),
                    (
                        gpui::point(bounds.right(), bounds.bottom()),
                        gpui::point(bounds.right() - r, bounds.bottom()),
                        gpui::point(bounds.right(), bounds.bottom() - r),
                    ),
                    (
                        gpui::point(bounds.left(), bounds.bottom()),
                        gpui::point(bounds.left(), bounds.bottom() - r),
                        gpui::point(bounds.left() + r, bounds.bottom()),
                    ),
                ];
                for (corner, start, end) in vertices {
                    let mut path = gpui::PathBuilder::fill();
                    path.move_to(corner);
                    path.line_to(start);
                    path.arc_to(gpui::point(r, r), px(0.), false, false, end);
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, outside);
                    }
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The right panel toggle: chrome_button's 24px square.
fn panel_button(enabled: bool, dot: bool) -> gpui::Stateful<gpui::Div> {
    let id = "toggle-right-panel";
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(super::TOGGLE_RIGHT_PANEL_LABEL)
        .group(id)
        .cursor_pointer()
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .when(enabled, |d| d.hover(|s| s.bg(content(0.1))))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .relative()
                .size(px(14.))
                .child(
                    icon("sidebar-right", 14.)
                        .text_color(content(if enabled { 0.5 } else { 0.25 }))
                        .when(enabled, |i| {
                            i.group_hover(id, |s| s.text_color(content(1.)))
                        }),
                )
                .when(dot, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(-2.))
                            .right(px(-2.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(accent_color()),
                    )
                }),
        )
}

impl Shell {
    pub(super) fn classic_titlebar_spans_window(&self, cx: &gpui::App) -> bool {
        cfg!(target_os = "macos")
            && self.workspace.classic_layout
            && !self.workspace.project_rail_open
            && self.settings.is_none()
            && !self.app_view_open()
            && self.app_search.is_none()
            && !self.mono_active(cx)
    }
    /// Switch navigation layouts without changing the new layout's frame style.
    pub(super) fn toggle_layout(&mut self, cx: &mut Context<Self>) {
        // Someone leaving the original layout before answering the new-layout
        // notice has never picked a frame, so they land on the boxy default.
        // Those who already went through it keep the frame they had.
        if self.workspace.classic_layout && !self.workspace.new_layout_prompt_dismissed {
            self.workspace.new_layout_boxy = true;
            self.workspace.new_layout_prompt_dismissed = true;
        }
        self.workspace.classic_layout = !self.workspace.classic_layout;
        self.project_resize = None;
        self.layout_changed(cx);
        self.sync_settings(cx);
    }
    /// Flip the Focused layout between its rounded card and boxy frame.
    /// Classic has no frame choice, so the shortcut does nothing there.
    pub(super) fn toggle_workspace_style(&mut self, cx: &mut Context<Self>) {
        if self.workspace.classic_layout {
            return;
        }
        self.workspace.new_layout_boxy = !self.workspace.new_layout_boxy;
        self.layout_changed(cx);
        self.sync_settings(cx);
    }
    pub(super) fn layout_changed(&mut self, cx: &mut Context<Self>) {
        let classic = self.workspace.boxy_layout();
        if !self.workspace.classic_layout
            && let Some(view) = self.active_app_view
        {
            self.remember_title_tab(TitleTab::App(view));
        }
        if !self.workspace.classic_layout
            && let Some(id) = self.monos.as_ref().and_then(|m| m.read(cx).active.clone())
        {
            self.remember_title_tab(self.mono_tab(id));
            self.reveal_active_tab(cx);
        }
        if let Some(search) = &self.app_search {
            search.view.update(cx, |v, cx| v.set_classic(classic, cx));
        }
        self.usage_footer
            .update(cx, |footer, cx| footer.set_classic(classic, cx));
        self.resources
            .update(cx, |monitor, cx| monitor.set_classic(classic, cx));
        self.keep_awake
            .update(cx, |keep, cx| keep.set_classic(classic, cx));
        self.invalidate_chrome(cx);
        self.drop_peek();
        if !self.workspace.classic_layout && !self.query.is_empty() {
            self.query.clear();
            self.classic_session_search
                .update(cx, |input, cx| input.set_text(String::new(), cx));
        }
        self.sync_right_panel(cx);
        self.fit_sidebar(cx);
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn layout_body(
        &mut self,
        sidebar_width: f32,
        right_shown: bool,
        right_width: f32,
        full: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let classic = self.workspace.boxy_layout();
        self.usage_footer
            .update(cx, |footer, cx| footer.set_classic(classic, cx));
        self.resources
            .update(cx, |monitor, cx| monitor.set_classic(classic, cx));
        self.keep_awake
            .update(cx, |keep, cx| keep.set_classic(classic, cx));
        if classic {
            return vec![self.classic_body(
                sidebar_width,
                right_shown,
                right_width,
                full,
                window,
                cx,
            )];
        }
        let card = self.workspace_content(right_shown, right_width, full, window, cx);
        let card = div()
            .id("rounded-main-pane")
            .debug_selector(|| "rounded-main-pane".into())
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .overflow_hidden()
            .child(card_corner_mask(true))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .overflow_hidden()
                    .bg(inset_pane_fill())
                    .rounded(px(CARD_RADIUS))
                    .border_1()
                    .border_color(content(0.08))
                    .child(card),
            )
            .child(card_corner_mask(false));
        let footer = self.footer_row(cx);
        vec![
            div()
                .h(px(TITLE_BAR_HEIGHT))
                .flex_shrink_0()
                .bg(inset_chrome_wash())
                .child(self.titlebar(window, cx))
                .into_any_element(),
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .relative()
                .when(sidebar_width > 0., |d| {
                    d.child(
                        // Keep the sidebar still beneath the moving card. Clip
                        // its covered part so text cannot bleed through the
                        // translucent workspace or receive covered clicks.
                        div()
                            .id("sidebar-frame")
                            .debug_selector(|| "sidebar-frame".into())
                            .absolute()
                            .left_0()
                            .top_0()
                            .w(px(sidebar_width))
                            .h_full()
                            .overflow_hidden()
                            .bg(inset_chrome_wash())
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .w(px(self.workspace.sidebar_width))
                                    .h_full()
                                    .opacity(self.sidebar_fraction)
                                    .child(self.sidebar_contents()),
                            ),
                    )
                })
                .child(div().w(px(sidebar_width)).h_full().flex_shrink_0())
                .child(
                    div()
                        .id("main-pane-frame")
                        .debug_selector(|| "main-pane-frame".into())
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .pr_1p5()
                        .pl(px(6. * (1. - self.sidebar_fraction)))
                        .child(
                            div()
                                .absolute()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .w(px(6.))
                                .bg(inset_chrome_wash()),
                        )
                        .when(self.sidebar_fraction < 1., |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(6. * (1. - self.sidebar_fraction)))
                                    .bg(inset_chrome_wash()),
                            )
                        })
                        .child(card)
                        .child(footer),
                )
                .children(self.sidebar_resize(cx))
                .into_any_element(),
        ]
    }
    fn classic_body(
        &mut self,
        sidebar_width: f32,
        right_shown: bool,
        right_width: f32,
        full: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sidebar_width = if !self.session_sidebar_available(cx) {
            0.
        } else {
            sidebar_width
        };
        // The right panel runs full height beside the workspace column, so
        // opening it pushes the tabs, body and footer together.
        let card = self.workspace_content(false, right_width, full, window, cx);
        let panel = self
            .right_panel
            .clone()
            .filter(|_| right_shown && self.settings.is_none())
            .map(|panel| self.right_panel_frame(panel, right_width, full, window, cx));
        let footer = (!self.classic_surface_open(cx)).then(|| self.footer_row(cx));
        let spans_window = self.classic_titlebar_spans_window(cx);
        let original = self.workspace.classic_layout;
        let compact_surface = cfg!(target_os = "macos")
            && original
            && !self.workspace.project_rail_open
            && self.settings.is_none()
            && self.classic_surface_open(cx);
        let tab_bar = if spans_window || (original && self.app_search.is_some()) {
            None
        } else if original && self.app_view_open() {
            Some(self.titlebar_controls_space(self.classic_app_surface_header(cx), window))
        } else if original && self.mono_active(cx) {
            let bar = self.classic_mono_header(cx);
            Some(self.titlebar_controls_space(bar, window))
        } else {
            Some(self.titlebar(window, cx))
        };
        let show_navigation = !spans_window;
        let navigation = show_navigation.then(|| self.navigation(cx));
        let corner = (self.settings.is_none() && !original).then(|| self.panel_toggle(window, cx));
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .relative()
            .when(self.workspace.classic_layout, |d| {
                d.child(self.project_rail_frame(cx))
            })
            .when(sidebar_width > 0., |d| {
                d.child(
                    div()
                        .id("sidebar-frame")
                        .debug_selector(|| "sidebar-frame".into())
                        .relative()
                        .w(px(sidebar_width))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .border_r_1()
                        .border_color(if original {
                            content(0.07)
                        } else {
                            crate::right_panel::stroke()
                        })
                        .when(original, |d| d.bg(pane_fill()))
                        .child(
                            // Full width while it slides; the frame clips it.
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .w(px(self.workspace.sidebar_width))
                                .h_full()
                                .flex()
                                .flex_col()
                                .opacity(self.sidebar_fraction)
                                .when(!original, |d| {
                                    d.child(
                                        // The traffic lights head the sidebar, where
                                        // the window's top is; the toggle floats
                                        // above them (see below).
                                        titlebar_drag_area()
                                            .w_full()
                                            .h(px(TITLE_BAR_HEIGHT))
                                            .flex_shrink_0(),
                                    )
                                })
                                .child(div().flex_1().min_h_0().child(self.sidebar_contents())),
                        ),
                )
            })
            .child(
                div()
                    .id("main-pane-frame")
                    .debug_selector(|| "main-pane-frame".into())
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .when(!compact_surface, |d| d.bg(pane_fill()))
                    .when(compact_surface, |d| {
                        // Keep one continuous shell tint behind all traffic
                        // lights; the content wash starts below the title bar.
                        d.relative().child(
                            div()
                                .debug_selector(|| "classic-surface-background".into())
                                .absolute()
                                .top(px(TITLE_BAR_HEIGHT))
                                .bottom_0()
                                .left_0()
                                .right_0()
                                .bg(pane_fill()),
                        )
                    })
                    .children(tab_bar)
                    .child(card)
                    .children(footer.map(|footer| {
                        footer
                            .h(px(28.))
                            .px_3()
                            .border_t_1()
                            .border_color(crate::right_panel::stroke())
                    })),
            )
            .children(self.sidebar_resize(cx))
            .children(panel.map(|panel| div().h_full().bg(pane_fill()).child(panel)))
            .when(show_navigation, |d| {
                d.child(
                    // The traffic lights, toggle and arrows ride above the sidebar's
                    // header as in the inset title bar: the toggle holds still, and
                    // the arrows sit at the open sidebar's edge or after the toggle.
                    div().absolute().top_0().left_0().children(navigation),
                )
            })
            .when(!spans_window, |d| {
                d.child(
                    // The right panel toggle holds the window's corner too, above
                    // the strip or an open panel's tab row. Its padding keeps
                    // clear of Windows' controls; Linux's sit beside it.
                    div()
                        .absolute()
                        .top_0()
                        .right(px(
                            super::client_controls_width(window) - self.window_controls
                        ))
                        .h(px(TITLE_BAR_HEIGHT))
                        .children(corner),
                )
            });
        if spans_window {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.titlebar(window, cx))
                .child(body)
                .into_any_element()
        } else {
            body.into_any_element()
        }
    }
    /// Classic: the corner's buttons, `pr-3` from the edge.
    pub(super) fn corner_room(&self, cx: &gpui::App) -> f32 {
        // Client-drawn window controls hold the corner's right end.
        let controls = self.window_controls;
        if self.workspace.classic_layout && !self.mono_active(cx) {
            return controls;
        }
        if self.mono_active(cx) {
            return if self.corner_tabs_shown() {
                let minimum = if self.right_panel_width() >= 312. + controls {
                    312.
                } else {
                    252.
                };
                self.right_drawn.max(minimum + controls)
            } else {
                6. + CORNER_BUTTONS_WIDTH + 12. + controls
            };
        }
        if !self.git.read(cx).available() {
            return 6. + 24. + 12. + controls;
        }
        // The toggle and the compact tool tabs, including the open label's padding.
        let tabs = if self.corner_tabs_shown() {
            self.project_tabs_width(cx) + 6.
        } else {
            0.
        };
        6. + tabs + self.diff_stat_room(cx) + CORNER_BUTTONS_WIDTH + 12. + controls
    }
    fn corner_tabs_shown(&self) -> bool {
        self.workspace.right_panel_open
            && !self.app_view_open()
            && self.settings.is_none()
            && self.app_search.is_none()
    }
    /// Classic: the panel toggle holds the corner while Explorer, Terminal (when
    /// it lives in the panel) and History start at the panel's left edge. The
    /// open tab names itself.
    pub(super) fn classic_corner(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let row = div()
            .h(px(TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center();
        if !self.git.read(cx).available() {
            return row
                .pl_1p5()
                .pr(px(12. + self.window_controls))
                .child(self.file_search_button(cx))
                .into_any_element();
        }
        let panel_off = self.app_view_open();
        let changes = !panel_off && self.git.read(cx).has_changes();
        let open = self.corner_tabs_shown();
        let current = self
            .right_panel
            .as_ref()
            .map_or(self.workspace.right_panel_tab, |p| p.read(cx).tab());
        // How far the open tab's label has unfolded, and the old one folded.
        let progress = self
            .corner_reveal
            .as_ref()
            .and_then(|reveal| reveal.progress(current, window, cx));
        let mut tabs = vec![
            (
                "right-panel-explorer",
                "Explorer",
                "file-script",
                RightTab::Files,
            ),
            ("right-panel-history", "History", "clock", RightTab::Graph),
        ];
        if self.terminal_in_panel(cx) {
            tabs.insert(
                1,
                (
                    "right-panel-terminal",
                    "Terminal",
                    "terminal",
                    RightTab::Terminal,
                ),
            );
        }
        let count = tabs.len();
        row.pl_1p5()
            .pr(px(12. + self.window_controls))
            .when(open, |d| {
                // Follow the panel's leading edge, reserving enough room for
                // the controls while the panel is still sliding open.
                d.w(px(self.right_drawn.max(self.corner_room(cx))))
                    .child(
                        div()
                            .id("right-panel-corner-tabs")
                            .role(Role::TabList)
                            .aria_label("Project")
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .mr(px(6.))
                            .children(tabs.into_iter().map(|(id, label, path, tab)| {
                                let selected = tab == current;
                                let unfolded = match progress {
                                    Some((_, p)) if selected => p,
                                    Some((closing, p)) if tab == closing => 1. - p,
                                    _ if selected => 1.,
                                    _ => 0.,
                                };
                                tab_button(
                                    id,
                                    crate::project_tabs::TabLabel::from(label).max_width(
                                        crate::right_panel::tab_label_width(
                                            self.project_tabs_width(cx),
                                            count,
                                        ),
                                    ),
                                    path,
                                    selected,
                                    false,
                                    unfolded,
                                    window,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let Some(panel) = this.right_panel.clone() else {
                                            return;
                                        };
                                        let closing = panel.read(cx).tab();
                                        if closing == tab {
                                            return;
                                        }
                                        this.corner_reveal = Some(TabReveal::new(tab, closing));
                                        panel.update(cx, |p, cx| p.select(tab, cx));
                                        this.invalidate_chrome(cx);
                                        cx.notify();
                                    },
                                ))
                            })),
                    )
                    .child(titlebar_drag_area().flex_1().min_w_0())
            })
            .children(self.project_diff_stat(cx))
            .child(self.file_search_button(cx).mx(px(CORNER_CONTROL_GAP)))
            .child(
                // Closed, the toggle carries the changes dot itself.
                panel_button(!panel_off, !open && changes).when(!panel_off, |d| {
                    d.on_click(
                        cx.listener(|this, _, window, cx| this.toggle_right_panel(window, cx)),
                    )
                }),
            )
            .into_any_element()
    }
    fn sidebar_contents(&self) -> AnyElement {
        // Cached paint doesn't include inherited opacity in its key. Repaint
        // during the fade, even if the clip rounds to the same pixel on
        // successive frames.
        if self.sidebar_uncached() {
            self.sidebar_frame.clone().into_any_element()
        } else {
            self.sidebar_frame
                .clone()
                .cached(
                    StyleRefinement::default()
                        .w(px(self.workspace.sidebar_width))
                        .h_full(),
                )
                .into_any_element()
        }
    }
    fn sidebar_resize(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        (self.session_sidebar_available(cx)
            && self.workspace.sidebar_visible
            && self.sidebar_slide.is_none())
        .then(|| {
            // The classic sidebar owns its right rule; the inset workspace
            // owns the rule just beyond the sidebar's edge.
            let border = self.project_rail_drawn_width()
                + self.workspace.sidebar_width
                + if self.workspace.boxy_layout() {
                    -0.5
                } else {
                    0.5
                };
            div()
                .id("sidebar-resize")
                .absolute()
                .left(px(border - 2.))
                .w(px(4.))
                .h_full()
                .cursor(CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.resize = Some((event.position.x, this.workspace.sidebar_width));
                        cx.stop_propagation();
                    }),
                )
                .into_any_element()
        })
    }
    /// The workspace body, its error banner and the right panel beside them.
    fn workspace_content(
        &mut self,
        right_shown: bool,
        right_width: f32,
        full: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        if let Some(search) = &self.app_search {
            return div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .overflow_hidden()
                .child(search.view.clone());
        }
        if let Some(settings) = &self.settings {
            return div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .overflow_hidden()
                .child(settings.clone());
        }
        let panel: Option<Entity<RightPanel>> = self
            .right_panel
            .clone()
            .filter(|_| right_shown && self.settings.is_none());
        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .child(self.background_card.probe())
            .child(
                div().flex_1().min_w_0().h_full().flex().flex_col().child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        // Every project lands on a blank session, as in
                        // Monocode; this is empty only while one is created.
                        .child({
                            if let Some(monos) =
                                self.monos.clone().filter(|m| m.read(cx).active.is_some())
                            {
                                let session = monos.read(cx).active_notification_session(cx);
                                let linked = session.and_then(|id| self.linked_notice(id, cx));
                                div()
                                    .relative()
                                    .size_full()
                                    .child(self.mono_bounds.layout_probe())
                                    .child(monos)
                                    .children(self.drop_hint(super::mono_panes::MONO_VIEW))
                                    .children(linked.map(|notice| {
                                        div()
                                            .absolute()
                                            .top(px(12.))
                                            .right(px(12.))
                                            .w(px(320.))
                                            .max_w_full()
                                            .child(notice)
                                    }))
                                    .into_any_element()
                            } else {
                                let body = self.workspace_body(window, cx);
                                self.with_dock(body, window, cx)
                            }
                        }),
                ),
            )
            .when_some(panel, |d, panel| {
                d.child(self.right_panel_frame(panel, right_width, full, window, cx))
            })
    }
    /// The usage chips with the terminal controls at the far end.
    fn footer_row(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let height = if self.workspace.boxy_layout() {
            28.
        } else {
            30.
        };
        div()
            .w_full()
            .h(px(30.))
            .flex_shrink_0()
            .when(!self.workspace.boxy_layout(), |d| {
                d.overflow_hidden().bg(inset_chrome_wash())
            })
            .flex()
            .items_center()
            .child(
                self.usage_footer
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().h(px(height))),
            )
            .children(self.pressure_chip(cx))
            .children(self.keep_awake_pill())
            .children(self.terminal_footer(cx))
    }
    /// The keep-awake pill, just before the terminal controls (macOS only).
    fn keep_awake_pill(&self) -> Option<AnyElement> {
        if !crate::keep_awake::KeepAwake::supported() {
            return None;
        }
        let classic = self.workspace.boxy_layout();
        let alone = self.app_view_open() || self.workspace.project.is_empty();
        Some(
            div()
                .flex_shrink_0()
                .map(|d| match (classic, alone) {
                    (true, true) => d,
                    (true, false) => d.mr(px(super::terminal_dock::FOOTER_GAP)),
                    (false, true) => d.mr(px(6.)).pb(px(2.)),
                    (false, false) => d.mr(px(super::terminal_dock::FOOTER_GAP)).pb(px(2.)),
                })
                .child(self.keep_awake.clone())
                .into_any_element(),
        )
    }
    /// ResourcePressureNotice's chip, just before the terminal controls.
    fn pressure_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.resources.read(cx).chip_shown() {
            return None;
        }
        let classic = self.workspace.boxy_layout();
        // The terminal group sits one footer gap on; alone, the chip takes its inset margin.
        let alone = self.app_view_open() || self.workspace.project.is_empty();
        Some(
            div()
                .flex_shrink_0()
                .map(|d| match (classic, alone) {
                    (true, true) => d,
                    (true, false) => d.mr(px(super::terminal_dock::FOOTER_GAP)),
                    (false, true) => d.mr(px(6.)).pb(px(2.)),
                    (false, false) => d.mr(px(super::terminal_dock::FOOTER_GAP)).pb(px(2.)),
                })
                .child(self.resources.clone())
                .into_any_element(),
        )
    }
    /// Modal.tsx at size `sm` with `fitViewport`: 420px, centred.
    pub(super) fn resource_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.resources.read(cx).is_open() {
            return None;
        }
        let mut sessions: HashMap<i64, SessionInfo> = self
            .sessions
            .iter()
            .map(|(id, session)| {
                let summary = &session.read(cx).summary;
                let info = SessionInfo {
                    title: summary.title.clone().into(),
                    provider: summary.provider.clone(),
                };
                (*id, info)
            })
            .collect();
        if let Some(m) = &self.monos {
            sessions.extend(m.read(cx).resource_sessions(cx));
        }
        let content = self
            .resources
            .update(cx, |monitor, cx| monitor.dialog(&sessions, window, cx));
        let viewport = window.viewport_size();
        let width = 420f32.min(viewport.width.as_f32() - 24.);
        let height = (self.resources.read(cx).measured_height().unwrap_or(240.) + 2.)
            .min(viewport.height.as_f32() - 32.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px((viewport.height.as_f32() - height) / 2.),
        ));
        let monitor = self.resources.downgrade();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("running-work-scrim"))
                .child(
                    popover("running-work-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .max_height(viewport.height.as_f32() - 32.)
                        .priority(60)
                        .on_dismiss(move |window, cx| {
                            let _ = monitor.update(cx, |monitor, cx| monitor.close(window, cx));
                        })
                        .child(content),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{monos::Monos, shell::tests::window};
    use gpui::{TestAppContext, WindowHandle};
    use std::time::{Duration, Instant};

    fn mixed_window(cx: &mut TestAppContext) -> (WindowHandle<Shell>, Entity<Monos>, String, i64) {
        let (window, _, session) = window(cx);
        let (manager, id) = window
            .update(cx, |shell, w, cx| {
                let manager = shell.mono_manager();
                manager.update(cx, |m, cx| {
                    m.create(Vec::new(), w, cx);
                    m.dismiss_settings(w, cx);
                });
                let id = manager.read(cx).active.clone().unwrap();
                (manager, id)
            })
            .unwrap();
        cx.run_until_parked();
        (window, manager, id, session)
    }

    #[gpui::test]
    fn project_tabs_abbreviate_then_become_icons_without_losing_header_controls(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.toggle_right_panel(w, cx);
                shell.git.update(cx, |git, cx| {
                    git.stats = Some(monocode_core::scm::DiffStats {
                        files: 550,
                        additions: 246_137,
                        deletions: 11_081,
                    });
                    cx.notify();
                });
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        for boxy in [false, true] {
            let selectors = if boxy {
                ["right-panel-explorer", "right-panel-history"]
            } else {
                ["right-panel-tab-Explorer", "right-panel-tab-History"]
            };
            let label_selector = Box::leak(format!("{}-label", selectors[1]).into_boxed_str());
            let mut full_label_width = None;
            for width in [420., 290., 270., 260., 220., 420.] {
                window
                    .update(visual, |shell, _, cx| {
                        shell.workspace.new_layout_boxy = boxy;
                        shell.workspace.right_panel_width = width;
                        shell
                            .right_panel
                            .as_ref()
                            .unwrap()
                            .update(cx, |panel, cx| panel.select(RightTab::Graph, cx));
                        shell.invalidate_chrome(cx);
                        cx.notify();
                    })
                    .unwrap();
                visual.run_until_parked();
                visual.refresh().unwrap();
                let tabs = selectors.map(|selector| visual.debug_bounds(selector).unwrap());
                let stats = visual.debug_bounds("project-diff-stat").unwrap();
                let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
                let search = visual.debug_bounds("titlebar-file-search").unwrap();
                assert!(tabs[0].right() <= tabs[1].left());
                assert!(tabs[1].right() <= stats.left());
                assert!(stats.right() <= search.left());
                assert!(search.right() <= toggle.left());
                assert_eq!(search.left() - stats.right(), px(CORNER_CONTROL_GAP));
                assert_eq!(toggle.left() - search.right(), px(CORNER_CONTROL_GAP));
                assert_eq!(toggle.size.width, px(24.));
                if width == 420. {
                    let label = visual.debug_bounds(label_selector).unwrap();
                    if let Some(full_width) = full_label_width {
                        assert_eq!(label.size.width, full_width);
                    }
                    full_label_width = Some(label.size.width);
                } else if width == 290. {
                    let label = visual.debug_bounds(label_selector).unwrap();
                    assert!(label.size.width > px(0.));
                    assert!(label.size.width < full_label_width.unwrap());
                } else {
                    assert!(visual.debug_bounds(label_selector).is_none());
                    assert!(tabs.iter().all(|tab| tab.size.width == px(26.)));
                    // The icon keeps its action after the label disappears.
                    visual.simulate_click(tabs[0].center(), Default::default());
                    visual.run_until_parked();
                    window
                        .update(visual, |shell, _, cx| {
                            assert_eq!(
                                shell.right_panel.as_ref().unwrap().read(cx).tab(),
                                RightTab::Files
                            )
                        })
                        .unwrap();
                }
            }
        }
    }

    #[gpui::test]
    fn right_sidebar_width_and_visibility_follow_both_tab_types_and_layouts(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, manager, mono, session) = mixed_window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for classic in [false, true] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.classic_layout = classic;
                    assert!(
                        !shell.workspace.right_panel_open,
                        "opening a Mono respects the session's closed sidebar"
                    );
                    cx.notify();
                })
                .unwrap();
            visual.run_until_parked();
            assert!(visual.debug_bounds("right-panel-frame").is_none());
            let toggle = visual
                .debug_bounds(if classic {
                    "classic-mono-overview"
                } else {
                    "toggle-right-panel"
                })
                .unwrap();
            visual.simulate_click(toggle.center(), Default::default());
            visual.run_until_parked();
            assert!(visual.debug_bounds("mono-overview-panel").is_some());
            let frame = visual.debug_bounds("right-panel-frame").unwrap();
            let bounds = visual.debug_bounds("right-panel-resize").unwrap();
            assert_eq!(bounds.size.width, px(6.));
            assert!((bounds.center().x - frame.left() - px(0.5)).abs() <= px(0.5));
            // Both halves of the transparent grab area must receive presses,
            // including the half outside the panel's clipped contents.
            for x in [bounds.left() + px(0.5), bounds.right() - px(0.5)] {
                let point = gpui::point(x, bounds.center().y);
                visual.simulate_mouse_down(point, MouseButton::Left, Default::default());
                window
                    .update(visual, |shell, _, _| assert!(shell.right_resize.is_some()))
                    .unwrap();
                visual.simulate_mouse_up(point, MouseButton::Left, Default::default());
                visual.run_until_parked();
            }
            let handle = visual.debug_bounds("right-panel-resize").unwrap().center();
            let end = handle - gpui::point(px(30.), px(0.));
            visual.simulate_mouse_down(handle, MouseButton::Left, Default::default());
            visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
            visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
            visual.run_until_parked();
            let expanded = window
                .update(visual, |shell, _, _| shell.workspace.right_panel_width)
                .unwrap();
            let handle = visual.debug_bounds("right-panel-resize").unwrap().center();
            let end = handle + gpui::point(px(30.), px(0.));
            visual.simulate_mouse_down(handle, MouseButton::Left, Default::default());
            visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
            // The pointer is now over a Mono detail layer that occludes mouse
            // events. Shrinking and releasing must still reach the shell.
            window
                .update(visual, |shell, _, _| {
                    assert_eq!(shell.workspace.right_panel_width, expanded - 30.);
                })
                .unwrap();
            visual.simulate_mouse_up(
                end + gpui::point(px(20.), px(0.)),
                MouseButton::Left,
                Default::default(),
            );
            visual.run_until_parked();
            let mono_bounds = visual.debug_bounds("right-panel-frame").unwrap();
            let width = window
                .update(visual, |shell, w, cx| {
                    assert!(shell.right_resize.is_none());
                    let width = shell.workspace.right_panel_width;
                    assert_eq!(mono_bounds.size.width, px(width));
                    shell.open(session, w, cx);
                    width
                })
                .unwrap();
            visual.run_until_parked();
            if classic {
                assert!(visual.debug_bounds("right-panel-frame").is_none());
            } else {
                assert_eq!(
                    visual.debug_bounds("right-panel-frame").unwrap(),
                    mono_bounds
                );
            }
            assert!(visual.debug_bounds("mono-overview-panel").is_none());
            if !classic {
                let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
                visual.simulate_click(toggle.center(), Default::default());
                visual.run_until_parked();
            }
            window
                .update(visual, |_, w, cx| {
                    manager.update(cx, |m, cx| m.open(&mono, w, cx))
                })
                .unwrap();
            visual.run_until_parked();
            if classic {
                let toggle = visual.debug_bounds("classic-mono-overview").unwrap();
                visual.simulate_click(toggle.center(), Default::default());
                visual.run_until_parked();
            }
            assert!(visual.debug_bounds("right-panel-frame").is_none());
            window
                .update(visual, |shell, _, _| {
                    assert_eq!(shell.workspace.right_panel_width, width);
                    let saved = crate::runtime::locked(&shell.store)
                        .unwrap()
                        .ui_state()
                        .unwrap()
                        .unwrap();
                    let saved: crate::workspace::Workspace = serde_json::from_str(&saved).unwrap();
                    assert_eq!(saved.right_panel_width, width);
                    assert!(!saved.right_panel_open);
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn switching_tabs_keeps_the_same_sidebar_spring_and_clipped_contents(cx: &mut TestAppContext) {
        let (window, manager, mono, session) = mixed_window(cx);
        cx.update(|cx| cx.set_reduce_motion(false));
        window
            .update(cx, |shell, w, cx| shell.toggle_right_panel(w, cx))
            .unwrap();
        for ms in [35, 70, 110] {
            window
                .update(cx, |shell, w, cx| {
                    let slide = shell.right_slide.as_ref().unwrap();
                    slide.elapsed(Duration::from_millis(ms));
                    let now = Instant::now();
                    let before = slide.at(now);
                    if shell.mono_active(cx) {
                        shell.open(session, w, cx);
                    } else {
                        manager.update(cx, |m, cx| m.open(&mono, w, cx));
                    }
                    assert_eq!(
                        shell.right_slide.as_ref().unwrap().at(now),
                        before,
                        "tab switches preserve the spring's position and velocity"
                    );
                    assert!(shell.workspace.right_panel_open);
                    cx.notify();
                })
                .unwrap();
            cx.update_window(window.into(), |_, w, cx| {
                w.simulate_next_frame(cx);
                w.draw(cx).clear(cx);
            })
            .unwrap();
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            let frame = visual.debug_bounds("right-panel-frame").unwrap();
            let content = visual.debug_bounds("right-panel-content").unwrap();
            let width = window
                .update(visual, |shell, _, _| shell.workspace.right_panel_width)
                .unwrap();
            assert!(frame.size.width > px(0.) && frame.size.width < px(width));
            assert_eq!(content.size.width, px(width - 1.));
        }
        window
            .update(cx, |shell, w, cx| {
                shell.toggle_right_panel(w, cx);
                assert!(!shell.workspace.right_panel_open);
                assert_eq!(shell.right_slide.as_ref().unwrap().to, 0.);
                shell
                    .right_slide
                    .as_ref()
                    .unwrap()
                    .elapsed(Duration::from_millis(45));
                shell.open(session, w, cx);
                shell.toggle_right_panel(w, cx);
                assert!(shell.workspace.right_panel_open);
                assert_eq!(
                    shell.right_slide.as_ref().unwrap().to,
                    shell.workspace.right_panel_width
                );
                manager.update(cx, |m, cx| m.open(&mono, w, cx));
                shell
                    .right_slide
                    .as_ref()
                    .unwrap()
                    .elapsed(Duration::from_secs(1));
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.right_slide.is_none()))
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        assert_eq!(
            visual.debug_bounds("right-panel-frame").unwrap().size.width,
            px(window
                .update(visual, |shell, _, _| shell.workspace.right_panel_width)
                .unwrap())
        );
        assert!(visual.debug_bounds("mono-overview-panel").is_some());
    }
}
