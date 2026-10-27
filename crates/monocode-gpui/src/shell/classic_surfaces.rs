//! Original application destinations replace the project workspace temporarily.
use super::*;

impl Shell {
    pub(super) fn classic_surface_open(&self, cx: &App) -> bool {
        self.workspace.classic_layout
            && (self.settings.is_some()
                || self.app_view_open()
                || self.app_search.is_some()
                || self.mono_active(cx))
    }

    pub(super) fn session_sidebar_available(&self, cx: &App) -> bool {
        !self.classic_surface_open(cx)
    }

    pub(super) fn classic_app_surface_header(&self, cx: &App) -> AnyElement {
        let view = self
            .active_app_view
            .expect("an application destination is open");
        let inset = if !self.workspace.project_rail_open {
            self.strip_lead(cx) + 6.
        } else {
            12.
        };
        titlebar_drag_area()
            .id("classic-app-surface-header")
            .debug_selector(|| "classic-app-surface-header".into())
            .h(px(TITLE_BAR_HEIGHT))
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .border_b_1()
            .border_color(content(0.07))
            .flex()
            .items_center()
            .pl(px(inset))
            .pr_3()
            .gap_2()
            .child(
                icon(
                    if view == app_views::AppView::Notes {
                        "file"
                    } else {
                        view.icon()
                    },
                    14.,
                )
                .text_color(content(0.45)),
            )
            .child(
                div()
                    .debug_selector(|| "classic-app-surface-title".into())
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(content(1.))
                    .child(view.label()),
            )
            .into_any_element()
    }

    pub(super) fn classic_mono_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let monos = self.monos.as_ref().expect("a Mono is open").clone();
        let open = self.workspace.right_panel_open;
        let compact = !self.workspace.project_rail_open;
        let title = monos
            .read(cx)
            .original_title(if compact { 6. } else { 12. }, cx);
        div()
            .id("classic-mono-title")
            .debug_selector(|| "classic-mono-title".into())
            .h(px(TITLE_BAR_HEIGHT))
            .min_w_0()
            .flex_shrink_0()
            .border_b_1()
            .border_color(content(0.07))
            .flex()
            .items_center()
            .when(compact, |d| d.pl(px(self.strip_lead(cx))))
            .child(titlebar_drag_area().flex_1().min_w_0().child(title))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .child(
                        div()
                            .id("classic-mono-overview")
                            .debug_selector(|| "classic-mono-overview".into())
                            .role(Role::Button)
                            .aria_label(if open {
                                "Hide overview"
                            } else {
                                "Show overview"
                            })
                            .h(px(24.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .rounded_md()
                            .text_size(px(12.))
                            .text_color(content(if open { 1. } else { 0.7 }))
                            .bg(content(if open { 0.1 } else { 0.05 }))
                            .hover(|s| s.bg(content(0.1)))
                            .child(icon("dashboard", 14.))
                            .child("Overview")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_right_panel(window, cx)
                            })),
                    )
                    .child(
                        icon_button("classic-mono-settings", "Mono settings", "settings")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |_, _, window, cx| {
                                monos
                                    .update(cx, |monos, cx| monos.open_current_settings(window, cx))
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn classic_rail_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            self.close_settings(window, cx);
        } else if self.mono_active(cx) {
            if let Some(monos) = &self.monos {
                monos.update(cx, |monos, cx| monos.close(cx));
            }
            self.ensure_project_session(window, cx);
            self.update_chat_visibility(cx);
            self.sync_right_panel(cx);
            self.focus_active(window, cx);
            if let Some(tab) = self.active_title_tab(cx) {
                self.remember_title_tab(tab);
            }
            self.invalidate_chrome(cx);
            self.persist(cx);
            cx.notify();
        } else {
            self.visit(-1, window, cx);
        }
    }
}
