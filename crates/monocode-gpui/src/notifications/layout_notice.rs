//! Controlled native counterpart of NewLayoutNotice.tsx. The shell owns and
//! persists the inset-layout and dismissed preferences after receiving events.
use crate::theme::*;
use gpui::{Context, EventEmitter, FontWeight, Render, Role, Window, div, prelude::*, px};

pub(crate) enum LayoutNoticeEvent {
    Try,
    Dismiss,
}
pub(crate) struct LayoutNotice {
    inset_enabled: bool,
    dismissed: bool,
}
impl EventEmitter<LayoutNoticeEvent> for LayoutNotice {}
impl LayoutNotice {
    pub(crate) fn new(inset_enabled: bool, dismissed: bool) -> Self {
        Self {
            inset_enabled,
            dismissed,
        }
    }
    pub(crate) fn set_snapshot(
        &mut self,
        inset_enabled: bool,
        dismissed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.inset_enabled != inset_enabled || self.dismissed != dismissed {
            self.inset_enabled = inset_enabled;
            self.dismissed = dismissed;
            cx.notify();
        }
    }
    pub(crate) fn is_visible(&self) -> bool {
        !self.inset_enabled && !self.dismissed
    }
}
impl Render for LayoutNotice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_visible() {
            return div().into_any_element();
        }
        super::surface::glass_frame()
            .id("new-layout-notice").role(Role::Status).aria_label("Try the Focused layout")
            .child(div().relative().p(px(12.))
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Try the Focused layout"))
            .child(div().mt(px(4.)).text_size(px(12.)).line_height(px(19.5)).text_color(content(0.6))
                .child("A project switcher, workspace tabs and a right panel. Switch back to Classic anytime in Settings → Appearance → Layout."))
            .child(div().mt(px(12.)).flex().justify_end().gap(px(8.))
                .child(div().id("new-layout-not-now").role(Role::Button).aria_label("Not now").cursor_pointer()
                    .rounded(px(6.)).px(px(10.)).py(px(6.)).text_size(px(12.)).text_color(content(0.6))
                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(LayoutNoticeEvent::Dismiss))).child("Not now"))
                .child(div().id("new-layout-try").role(Role::Button).aria_label("Try it").cursor_pointer()
                    .rounded(px(6.)).px(px(10.)).py(px(6.)).text_size(px(12.)).font_weight(FontWeight::MEDIUM)
                    .bg(content(0.1)).hover(|s| s.bg(content(0.15)))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(LayoutNoticeEvent::Try))).child("Try it")))
            ).into_any_element()
    }
}
