//! Mono details content inside the shell's shared right sidebar.
use super::panel_stack::{PanelStack, Pose};
use super::*;
use std::time::Instant;

impl Monos {
    pub(crate) fn native_detail_content(&self, cx: &App) -> bool {
        self.details.read(cx).native_content()
            || matches!(&self.route, Some(Route::Document(id))
                if self.current().is_some_and(|m| m.documents.iter().any(|d| &d.id == id && d.kind == "html")))
    }
}

pub(crate) struct Overview {
    manager: WeakEntity<Monos>,
    focus: gpui::FocusHandle,
    top_inset: f32,
    original_chrome: bool,
    stack: PanelStack,
    native_content: bool,
    _observation: Subscription,
}

impl Overview {
    pub(super) fn new(manager: Entity<Monos>, cx: &mut Context<Self>) -> Self {
        Self {
            manager: manager.downgrade(),
            focus: cx.focus_handle(),
            top_inset: 0.,
            original_chrome: false,
            stack: PanelStack::default(),
            native_content: false,
            _observation: cx.observe(&manager, |_, _, cx| cx.notify()),
        }
    }

    pub(crate) fn set_top_inset(&mut self, inset: f32, cx: &mut Context<Self>) {
        if self.top_inset != inset {
            self.top_inset = inset;
            cx.notify();
        }
    }

    pub(crate) fn set_original_chrome(&mut self, original: bool, cx: &mut Context<Self>) {
        if self.original_chrome == original {
            return;
        }
        self.original_chrome = original;
        cx.notify();
    }

    pub(crate) fn native_content(&self) -> bool {
        self.native_content
    }

    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        self.stack = PanelStack::default();
        self.native_content = false;
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn elapsed(&mut self, elapsed: std::time::Duration, cx: &mut Context<Self>) {
        self.stack.elapsed(elapsed);
        cx.notify();
    }
}

fn layer(id: String, pose: Pose, interactive: bool, body: AnyElement) -> AnyElement {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .absolute()
        .top_0()
        .left(gpui::relative(pose.x))
        .w_full()
        .h_full()
        .opacity(pose.opacity)
        .flex()
        .flex_col()
        .occlude()
        .child(body)
        .when(!interactive, |d| {
            d.child(div().absolute().size_full().occlude())
        })
        .into_any_element()
}

impl Focusable for Overview {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Overview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let state = self
            .manager
            .read_with(cx, |m, _| {
                m.current().cloned().map(|mono| (mono, m.route.clone()))
            })
            .ok()
            .flatten();
        let mut layers = Vec::new();
        self.native_content = false;
        if let Some((mono, route)) = state {
            let changed = self.stack.sync(&mono.id, route, now, cx.reduce_motion());
            if changed && self.focus.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
            let frame = self.stack.frame(now);
            if frame.animating {
                window.request_animation_frame();
            }
            self.native_content = frame.pages.iter().any(|page| {
                matches!(&page.route, Route::Document(id) if mono.documents.iter().any(|d| &d.id == id && d.kind == "html"))
            });
            let original = self.original_chrome;
            let _ = self.manager.update(cx, |m, cx| {
                let content = m.overview_panel(&mono, window, cx);
                let base = if original {
                    div()
                        .size_full()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(m.original_overview_header(window, cx))
                        .child(div().flex_1().min_w_0().min_h_0().child(content))
                        .into_any_element()
                } else {
                    content
                };
                layers.push(layer(
                    "mono-detail-base".into(),
                    frame.base,
                    !frame.covered,
                    base,
                ));
                for page in frame.pages {
                    let key = format!("mono-detail-page-{}", page.route.key());
                    let body = m.route_page(&mono, page.route, window, cx);
                    layers.push(layer(key, page.pose, page.interactive, body));
                }
            });
        }
        div()
            .id("mono-overview-panel")
            .debug_selector(|| "mono-overview-panel".into())
            .role(Role::Complementary)
            .aria_label("Mono details")
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(self.top_inset > 0., |d| {
                d.child(
                    div()
                        .h(px(self.top_inset))
                        .flex_shrink_0()
                        .border_b_1()
                        .border_color(crate::right_panel::stroke()),
                )
            })
            .child(
                div()
                    .id("mono-detail-stack")
                    .debug_selector(|| "mono-detail-stack".into())
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .children(layers),
            )
    }
}
