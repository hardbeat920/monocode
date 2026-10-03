//! Port of `LinkedWorkItemPanel` from src/features/inbox/ui/InboxView.tsx:
//! a session's linked GitHub issue or pull request as a closable,
//! resizable side panel. It shows the cached card at once, refreshes it,
//! and keeps its data while hidden so showing it again reuses it.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
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
        let mut panel = Self {
            services,
            target,
            props,
            item: None,
            error: None,
            loading: true,
            detail: None,
            resize: PaneResize::new(width, DEFAULT_WIDTH, MIN_WIDTH, ResizeEdge::Left),
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
            .border_l_1()
            .border_color(theme.colors.stroke)
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
            .child(body)
            .child(
                div().absolute().top(u(5.)).right(u(8.)).child(
                    icon_button("linked-close", IconName::PanelLeft)
                        .tooltip(format!("Close {kind} panel"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(LinkedPanelEvent::Close))),
                ),
            )
            .into_any_element()
    }
}
