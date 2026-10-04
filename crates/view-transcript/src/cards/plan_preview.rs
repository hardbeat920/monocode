//! Port of src/features/sessions/ui/PlanPreview.tsx: a plan's title and
//! summary with Open and Build, and the chevron that builds with another
//! model (`BuildTargetButton`). The target picker itself belongs to the
//! second opinion menu, so the chevron reports a click and the host opens it.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyView, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, px,
};
use monocode_core::block::PlanStatus;
use monocode_core::plan::{plan_summary, plan_title};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, u};

use super::style;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// `<PlanPreview />`.
#[derive(IntoElement)]
pub struct PlanPreview {
    id: ElementId,
    text: String,
    streaming: bool,
    busy: bool,
    status: Option<PlanStatus>,
    /// Shows the build target chevron (`harness` was set).
    targets: bool,
    on_open: Option<Handler>,
    on_build: Option<Handler>,
    on_pick_target: Option<Handler>,
    target_picker: Option<AnyView>,
}

pub fn plan_preview(id: impl Into<ElementId>, text: impl Into<String>) -> PlanPreview {
    PlanPreview {
        id: id.into(),
        text: text.into(),
        streaming: false,
        busy: false,
        status: None,
        targets: false,
        on_open: None,
        on_build: None,
        on_pick_target: None,
        target_picker: None,
    }
}

/// `buildDisabled`.
pub fn build_disabled(text: &str, streaming: bool, busy: bool, status: Option<PlanStatus>) -> bool {
    busy || streaming
        || monocode_core::js::trim(text).is_empty()
        || matches!(
            status,
            Some(PlanStatus::Streaming | PlanStatus::Building | PlanStatus::Built)
        )
}

/// `buildLabel`.
pub fn build_label(status: Option<PlanStatus>) -> &'static str {
    match status {
        Some(PlanStatus::Building) => "Building\u{2026}",
        Some(PlanStatus::Built) => "Built",
        _ => "Build",
    }
}

impl PlanPreview {
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn status(mut self, status: Option<PlanStatus>) -> Self {
        self.status = status;
        self
    }

    /// `onOpen`: the title and the Open button open the plan in a pane.
    pub fn on_open(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open = Some(Rc::new(handler));
        self
    }

    /// `onBuild()` with the session's own model.
    pub fn on_build(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_build = Some(Rc::new(handler));
        self
    }

    /// `BuildTargetButton`: shown when the session has a harness. The host
    /// opens the model picker and calls its own build with the target.
    pub fn on_pick_target(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.targets = true;
        self.on_pick_target = Some(Rc::new(handler));
        self
    }

    pub fn target_picker(mut self, picker: AnyView) -> Self {
        self.targets = true;
        self.target_picker = Some(picker);
        self
    }
}

impl RenderOnce for PlanPreview {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let title = plan_title(&self.text);
        let summary = plan_summary(&self.text);
        let disabled = build_disabled(&self.text, self.streaming, self.busy, self.status);
        let child_id =
            |part: &str| ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), part.into());
        let sans = theme.fonts.sans.clone();

        let title_el: AnyElement = match self.on_open.clone() {
            Some(open) => div()
                .id(child_id("title"))
                .w_full()
                .truncate()
                .font_family(sans.clone())
                .text_px(13.)
                .line_height(u(20.))
                .medium()
                .text_color(theme.content(0.9))
                .cursor_pointer()
                .hover(|s| s.text_color(style::yellow_100()))
                .tooltip(tooltip(title.clone()))
                .on_click(move |event, window, cx| open(event, window, cx))
                .child(title.clone())
                .into_any_element(),
            None => div()
                .id(child_id("title"))
                .w_full()
                .truncate()
                .font_family(sans.clone())
                .text_px(13.)
                .line_height(u(20.))
                .medium()
                .text_color(theme.content(0.9))
                .tooltip(tooltip(title.clone()))
                .child(title.clone())
                .into_any_element(),
        };

        let actions = (self.on_open.is_some() || self.on_build.is_some()).then(|| {
            div()
                .mt(u(8.))
                .flex()
                .items_center()
                .justify_end()
                .gap(u(6.))
                .when_some(self.on_open.clone(), |el, open| {
                    el.child(
                        div()
                            .id(child_id("open"))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(u(4.))
                            .h(u(24.))
                            .px(u(8.))
                            .rounded(u(6.))
                            .bg(theme.content(0.08))
                            .font_family(sans.clone())
                            .text_px(11.)
                            .text_color(theme.content(0.7))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.content(0.12)).text_color(theme.colors.content))
                            .tooltip(tooltip("Open in pane"))
                            .on_click(move |event, window, cx| open(event, window, cx))
                            .child(
                                icon(IconName::PanelRight)
                                    .size(u(12.))
                                    .text_color(theme.content(0.7)),
                            )
                            .child("Open"),
                    )
                })
                .when_some(self.on_build.clone(), |el, build| {
                    let fill = theme.colors.content;
                    let ink = theme.colors.background_base;
                    let split = self.targets;
                    let button = div()
                        .id(child_id("build"))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(u(4.))
                        .h(u(24.))
                        .px(u(8.))
                        .bg(fill)
                        .map(|el| {
                            if split {
                                el.rounded_l(u(6.))
                            } else {
                                el.rounded(u(6.))
                            }
                        })
                        .font_family(sans.clone())
                        .text_px(11.)
                        .text_color(ink)
                        .tooltip(tooltip("Build this plan"))
                        .when(disabled, |el| el.opacity(0.4))
                        .when(!disabled, |el| {
                            el.cursor_pointer()
                                .hover(|s| s.bg(theme.content(0.9)))
                                .on_click(move |event, window, cx| build(event, window, cx))
                        })
                        .child(icon(IconName::Play).size(u(12.)).text_color(ink))
                        .child(build_label(self.status));
                    let chevron = self.on_pick_target.clone().map(|pick| {
                        div()
                            .id(child_id("target"))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(u(24.))
                            .rounded_r(u(6.))
                            .border_l(px(1.))
                            .border_color(monocode_ui::color::with_alpha(ink, 0.2))
                            .bg(fill)
                            .tooltip(tooltip("Build with another model"))
                            .when(disabled, |el| el.opacity(0.4))
                            .when(!disabled, |el| {
                                el.cursor_pointer()
                                    .hover(|s| s.bg(theme.content(0.9)))
                                    .on_click(move |event, window, cx| pick(event, window, cx))
                            })
                            .child(icon(IconName::ChevronDown).size(u(14.)).text_color(ink))
                    });
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .child(button)
                            .children(chevron)
                            .children(self.target_picker.clone()),
                    )
                })
        });

        div()
            .mb(u(8.))
            .overflow_hidden()
            .rounded(u(12.))
            .border_1()
            .border_color(theme.content(0.1))
            .bg(theme.content(0.07))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(u(10.))
                    .px(u(12.))
                    .py(u(10.))
                    .child(
                        icon(if self.streaming {
                            IconName::CircleDashed
                        } else {
                            IconName::AiIdea
                        })
                        .mt(u(2.))
                        .size(u(16.))
                        .flex_none()
                        .text_color(theme.content(0.4)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(title_el)
                            .when(!summary.is_empty(), |el| {
                                el.child(
                                    div()
                                        .mt(u(2.))
                                        .line_clamp(3)
                                        .font_family(sans.clone())
                                        .text_px(12.)
                                        .line_height(u(18.))
                                        .text_color(theme.content(0.5))
                                        .child(SharedString::from(summary.clone())),
                                )
                            })
                            .children(actions),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_is_disabled_while_busy_streaming_empty_or_done() {
        assert!(!build_disabled("# Plan", false, false, None));
        assert!(build_disabled("# Plan", false, true, None));
        assert!(build_disabled("# Plan", true, false, None));
        assert!(build_disabled("   ", false, false, None));
        assert!(build_disabled(
            "# Plan",
            false,
            false,
            Some(PlanStatus::Building)
        ));
        assert!(build_disabled(
            "# Plan",
            false,
            false,
            Some(PlanStatus::Built)
        ));
        assert!(build_disabled(
            "# Plan",
            false,
            false,
            Some(PlanStatus::Streaming)
        ));
        assert!(!build_disabled(
            "# Plan",
            false,
            false,
            Some(PlanStatus::Ready)
        ));
    }

    #[test]
    fn labels_the_build_button_by_status() {
        assert_eq!(build_label(None), "Build");
        assert_eq!(build_label(Some(PlanStatus::Building)), "Building\u{2026}");
        assert_eq!(build_label(Some(PlanStatus::Built)), "Built");
    }
}
