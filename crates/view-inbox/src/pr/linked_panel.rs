//! Port of `LinkedWorkItemPanel` from src/features/inbox/ui/InboxView.tsx:
//! a session's linked GitHub issue or pull request as a closable,
//! resizable side panel. It shows the cached card at once, refreshes it,
//! and keeps its data while hidden so showing it again reuses it. The sheet
//! slides in when the panel mounts visible, and each new body (loading,
//! error, the item) fades in from the right, as the `linked-panel-slide` and
//! `linked-panel-reveal` rules in src/styles/index.css do.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, AppContext as _, Context, ElementId, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div,
};
use monocode_ui::widgets::icon_button;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{InboxItem, InboxProjectOption, InboxServices, LinkedWorkItem, WorkItemKind};
use crate::pr::detail::{DetailMode, DetailProps, InboxDetailEvent, InboxDetailView};
use crate::style::{
    ActionKind, PaneResize, ResizeEdge, action_button, closed_ink, loader, resize_handle,
};

const MIN_WIDTH: f32 = 360.;
const DEFAULT_WIDTH: f32 = 520.;

/// `linked-panel-slide`: 260ms in from the panel's right edge.
const SLIDE: Duration = Duration::from_millis(260);
/// `linked-panel-reveal`: 240ms from 12px right and transparent, after a
/// 90ms delay that holds the start frame (`both` fill).
const REVEAL_DELAY: Duration = Duration::from_millis(90);
const REVEAL: Duration = Duration::from_millis(240);
const REVEAL_SHIFT: f32 = 12.;

/// The `opening` and `revealedKey` state from the TypeScript. The panel
/// takes its full width up front so the sessions reflow once, and the sheet
/// slides on an offset. Animating the width instead would rewrap the
/// transcript and resize terminals on every frame.
#[derive(Debug, Default)]
struct PanelMotion {
    /// When the slide started. `None` once it ends, or when the panel
    /// mounted hidden or under reduced motion.
    opening: Option<Instant>,
    /// The body whose reveal runs, and when it started.
    revealing: Option<(String, Instant)>,
    /// The last body whose reveal finished. It does not play again.
    revealed: Option<String>,
}

impl PanelMotion {
    fn new(visible: bool, reduce_motion: bool, now: Instant) -> Self {
        Self {
            opening: (visible && !reduce_motion).then_some(now),
            ..Self::default()
        }
    }

    /// Whether the slide still runs. A finished slide stops for good, as
    /// `onAnimationEnd` clearing `opening` does.
    fn sliding(&mut self, now: Instant) -> bool {
        if self
            .opening
            .is_some_and(|start| now.duration_since(start) >= SLIDE)
        {
            self.opening = None;
        }
        self.opening.is_some()
    }

    /// Hiding the panel ends the slide. A reveal that had not finished
    /// plays again when the panel shows, as a CSS animation restarts.
    fn hide(&mut self) {
        self.opening = None;
        self.revealing = None;
    }

    /// Whether the body keyed `key` plays its reveal on this frame.
    fn reveals(&mut self, key: &str, reduce_motion: bool, now: Instant) -> bool {
        if reduce_motion || self.revealed.as_deref() == Some(key) {
            return false;
        }
        match &self.revealing {
            Some((current, start)) if current == key => {
                if now.duration_since(*start) < REVEAL_DELAY + REVEAL {
                    return true;
                }
                self.revealed = Some(key.to_string());
                self.revealing = None;
                false
            }
            _ => {
                self.revealing = Some((key.to_string(), now));
                true
            }
        }
    }
}

thread_local! {
    /// `rememberedLinkedPanelWidth`.
    static REMEMBERED_WIDTH: Cell<f32> = const { Cell::new(DEFAULT_WIDTH) };
}

/// What the panel asks its owner for.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkedPanelEvent {
    Close,
    OpenSession(String),
}

/// The panel's props besides the target.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinkedPanelProps {
    pub cwd: String,
    pub projects: Vec<InboxProjectOption>,
    pub visible: bool,
    pub can_repair: bool,
}

pub struct LinkedWorkItemPanel {
    services: Rc<dyn InboxServices>,
    target: LinkedWorkItem,
    props: LinkedPanelProps,
    item: Option<InboxItem>,
    error: Option<String>,
    loading: bool,
    detail: Option<(Entity<InboxDetailView>, Subscription)>,
    resize: PaneResize,
    motion: PanelMotion,
    animate: bool,
    _load: Option<Task<()>>,
}

impl EventEmitter<LinkedPanelEvent> for LinkedWorkItemPanel {}

/// "Pull request" or "Issue".
pub fn linked_kind_label(target: &LinkedWorkItem) -> &'static str {
    if target.kind == WorkItemKind::Pr {
        "Pull request"
    } else {
        "Issue"
    }
}

impl LinkedWorkItemPanel {
    pub fn new(
        services: Rc<dyn InboxServices>,
        target: LinkedWorkItem,
        props: LinkedPanelProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let width = REMEMBERED_WIDTH.with(Cell::get);
        let motion = PanelMotion::new(props.visible, cx.reduce_motion(), Instant::now());
        let mut panel = Self {
            services,
            target,
            props,
            item: None,
            error: None,
            loading: true,
            detail: None,
            resize: PaneResize::new(width, DEFAULT_WIDTH, MIN_WIDTH, ResizeEdge::Left),
            motion,
            animate: true,
            _load: None,
        };
        panel.load(window, cx);
        panel
    }

    /// Another target: show its cached card and fetch it again.
    pub fn set_target(
        &mut self,
        target: LinkedWorkItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target.kind == self.target.kind
            && target.repo == self.target.repo
            && target.number == self.target.number
        {
            return;
        }
        self.target = target;
        self.detail = None;
        self.load(window, cx);
    }

    /// Shows or hides the panel. Hidden panels keep their data.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.props.visible == visible {
            return;
        }
        self.props.visible = visible;
        if !visible {
            self.motion.hide();
        }
        self.push_detail_props(cx);
        cx.notify();
    }

    pub fn item(&self) -> Option<&InboxItem> {
        self.item.as_ref()
    }

    pub fn detail(&self) -> Option<&Entity<InboxDetailView>> {
        self.detail.as_ref().map(|(detail, _)| detail)
    }

    pub fn visible(&self) -> bool {
        self.props.visible
    }

    /// Turns the slide and reveal off, for screenshots.
    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        if !animate {
            self.motion.hide();
        }
        cx.notify();
    }

    /// `contentKey`: which body shows, so a new one plays its reveal.
    fn content_key(&self) -> String {
        if let Some(item) = &self.item {
            format!("{:?}:{}:{}", item.kind, item.repo, item.number)
        } else if self.error.is_some() {
            "error".into()
        } else {
            "loading".into()
        }
    }

    /// The close button.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(LinkedPanelEvent::Close);
    }

    fn detail_props(&self) -> DetailProps {
        DetailProps {
            cwd: self.props.cwd.clone(),
            projects: self.props.projects.clone(),
            related_sessions: Vec::new(),
            mode: DetailMode::Panel,
            visible: self.props.visible,
            revision: 0,
            can_discuss: false,
            can_start: false,
            can_repair: self.props.can_repair,
        }
    }

    fn push_detail_props(&mut self, cx: &mut Context<Self>) {
        let props = self.detail_props();
        if let Some((detail, _)) = &self.detail {
            detail.update(cx, |detail, cx| detail.set_props(props, cx));
        }
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cached = self
            .services
            .peek_github_work_item(&self.props.cwd, &self.target, cx);
        self.error = None;
        self.loading = cached.is_none();
        self.item = None;
        if let Some(item) = cached {
            self.show(item, window, cx);
        }
        let task = self
            .services
            .github_work_item(&self.props.cwd, &self.target, cx);
        let handle = window.window_handle();
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(item) => this.show(item, window, cx),
                        Err(error) => this.error = Some(error),
                    }
                    this.loading = false;
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    fn show(&mut self, item: InboxItem, window: &mut Window, cx: &mut Context<Self>) {
        self.item = Some(item.clone());
        if let Some((detail, _)) = &self.detail {
            detail.update(cx, |detail, cx| detail.set_item(item, cx));
            return;
        }
        let services = self.services.clone();
        let props = self.detail_props();
        let detail = cx.new(|cx| InboxDetailView::new(services, item, props, window, cx));
        let subscription = cx.subscribe(&detail, |this, _, event: &InboxDetailEvent, cx| {
            match event {
                InboxDetailEvent::ItemChanged(item) => this.item = Some(item.as_ref().clone()),
                InboxDetailEvent::OpenSession(id) => {
                    cx.emit(LinkedPanelEvent::OpenSession(id.clone()))
                }
                InboxDetailEvent::Discuss => {}
            }
            cx.notify();
        });
        self.detail = Some((detail, subscription));
    }

    fn max_width(window: &Window, scale: f32) -> f32 {
        let viewport = f32::from(window.viewport_size().width) / scale;
        MIN_WIDTH.max((viewport * 0.65).round())
    }
}

impl Render for LinkedWorkItemPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.props.visible {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let scale = theme.ui_scale();
        let kind = linked_kind_label(&self.target).to_lowercase();
        let narrow = f32::from(window.viewport_size().width) / scale <= 950.;
        let max = Self::max_width(window, scale);
        let width = self.resize.width.min(max);
        let now = Instant::now();
        let sliding = self.motion.sliding(now);
        let content_key = self.content_key();
        let reveal = self
            .motion
            .reveals(&content_key, !self.animate || cx.reduce_motion(), now);
        let ease = theme.motion.ease_out;
        let body = if let Some((detail, _)) = &self.detail {
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(detail.clone())
                .into_any_element()
        } else if let Some(error) = self.error.clone() {
            let url = self.target.url.clone();
            let services = self.services.clone();
            div()
                .flex()
                .flex_1()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(u(12.))
                .px(u(32.))
                .child(
                    icon(IconName::CircleX)
                        .size(u(20.))
                        .text_color(closed_ink()),
                )
                .child(
                    div()
                        .max_w(u(384.))
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.55))
                        .child(error),
                )
                .child(
                    action_button(
                        "linked-open",
                        ActionKind::Outline,
                        Some(IconName::ExternalLink),
                        "Open on GitHub",
                        false,
                        cx,
                    )
                    .on_click(move |_, _, cx| services.open_url(&url, cx)),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(loader("linked-loading", 16., theme.content(0.40)))
                .into_any_element()
        };
        let mut aside = div()
            .id("linked-work-item-panel")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .min_h_0()
            .w(u(width))
            .max_w_full()
            .text_color(theme.colors.content)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    cx.emit(LinkedPanelEvent::Close);
                }
            }))
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    let max = Self::max_width(window, scale);
                    if this.resize.drag_to(f32::from(event.position.x), scale, max) {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if let Some(width) = this.resize.end() {
                        REMEMBERED_WIDTH.with(|cell| cell.set(width));
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if let Some(width) = this.resize.end() {
                        REMEMBERED_WIDTH.with(|cell| cell.set(width));
                    }
                }),
            );
        if narrow {
            aside = aside.absolute().top_0().bottom_0().right_0().shadow_2xl();
        }
        if sliding {
            aside = aside.overflow_hidden();
        }
        // The animators also run after a motion ends, so the element ids,
        // and with them the detail's element state, stay put.
        let content = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(body)
            .with_animations(
                ElementId::Name(SharedString::from(format!(
                    "linked-panel-reveal-{content_key}"
                ))),
                vec![
                    Animation::new(REVEAL_DELAY),
                    Animation::new(REVEAL).with_easing(ease.easing()),
                ],
                move |content, step, t| {
                    if !reveal {
                        return content;
                    }
                    let t = if step == 0 { 0. } else { t };
                    content.opacity(t).left(u(REVEAL_SHIFT * (1. - t)))
                },
            );
        let sheet = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .border_l_1()
            .border_color(theme.colors.stroke)
            .child(content)
            .child(
                div().absolute().top(u(5.)).right(u(8.)).child(
                    icon_button("linked-close", IconName::PanelLeft)
                        .tooltip(format!("Close {kind} panel"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(LinkedPanelEvent::Close))),
                ),
            )
            .with_animation(
                "linked-panel-slide",
                Animation::new(SLIDE).with_easing(ease.easing()),
                move |sheet, t| {
                    if sliding {
                        sheet.left(u(width * (1. - t)))
                    } else {
                        sheet
                    }
                },
            );
        aside
            .child(
                resize_handle(
                    "linked-resize",
                    ResizeEdge::Left,
                    8.,
                    self.resize.dragging(),
                    cx,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        if event.click_count >= 2 {
                            let width = this.resize.reset();
                            REMEMBERED_WIDTH.with(|cell| cell.set(width));
                        } else {
                            this.resize.begin(f32::from(event.position.x));
                        }
                        cx.notify();
                    }),
                ),
            )
            .child(sheet)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn slides_once_when_mounted_visible_with_motion() {
        let start = Instant::now();
        let mut motion = PanelMotion::new(true, false, start);
        assert!(motion.sliding(start));
        assert!(motion.sliding(start + 259 * MS));
        assert!(!motion.sliding(start + SLIDE));
        assert!(!motion.sliding(start));
        assert!(!PanelMotion::new(false, false, start).sliding(start));
        assert!(!PanelMotion::new(true, true, start).sliding(start));
    }

    #[test]
    fn hiding_ends_the_slide_for_good() {
        let start = Instant::now();
        let mut motion = PanelMotion::new(true, false, start);
        motion.hide();
        assert!(!motion.sliding(start + MS));
    }

    #[test]
    fn reveals_each_new_body_once_after_its_delay() {
        let start = Instant::now();
        let mut motion = PanelMotion::default();
        assert!(motion.reveals("loading", false, start));
        assert!(motion.reveals("loading", false, start + 329 * MS));
        assert!(!motion.reveals("loading", false, start + REVEAL_DELAY + REVEAL));
        assert!(!motion.reveals("loading", false, start + 400 * MS));

        let later = start + 500 * MS;
        assert!(motion.reveals("Pr:acme/web:157", false, later));
        assert!(!motion.reveals("Pr:acme/web:157", false, later + 330 * MS));
        assert!(!motion.reveals("Pr:acme/web:157", false, later + 900 * MS));
    }

    #[test]
    fn skips_the_reveal_under_reduced_motion() {
        let mut motion = PanelMotion::default();
        assert!(!motion.reveals("loading", true, Instant::now()));
    }

    #[test]
    fn replays_an_unfinished_reveal_after_hiding() {
        let start = Instant::now();
        let mut motion = PanelMotion::default();
        assert!(motion.reveals("loading", false, start));
        motion.hide();
        let shown = start + 1000 * MS;
        assert!(motion.reveals("loading", false, shown));
        assert!(motion.reveals("loading", false, shown + 100 * MS));
    }
}
