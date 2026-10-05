//! Port of src/features/inbox/ui/InboxDiscussionPanel.tsx: the Ask panel
//! beside an item. It owns no chat state; it opens (or reuses) the item's
//! Ask session and shows that session's pane.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyView, AppContext as _, Context, EventEmitter, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement as _, Render, Styled as _, Task,
    Window, div,
};
use monocode_ui::widgets::icon_button;
use monocode_ui::{IconName, Theme, UiStyled as _, u};

use crate::data::{InboxItem, InboxServices};
use crate::model::inbox_item_ref;
use crate::style::{PaneResize, ResizeEdge, resize_handle};

const MIN_WIDTH: f32 = 360.;
const DEFAULT_WIDTH: f32 = 440.;

thread_local! {
    static REMEMBERED_WIDTH: Cell<f32> = const { Cell::new(DEFAULT_WIDTH) };
}

/// The panel asks to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseDiscussion;

pub struct InboxDiscussionPanel {
    services: Rc<dyn InboxServices>,
    item: InboxItem,
    session_id: Option<String>,
    pane: Option<AnyView>,
    error: Option<String>,
    loading: bool,
    resize: PaneResize,
    _task: Option<Task<()>>,
}

impl EventEmitter<CloseDiscussion> for InboxDiscussionPanel {}

impl InboxDiscussionPanel {
    pub fn new(
        services: Rc<dyn InboxServices>,
        item: InboxItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let width = REMEMBERED_WIDTH.with(Cell::get);
        let mut panel = Self {
            services,
            item,
            session_id: None,
            pane: None,
            error: None,
            loading: true,
            resize: PaneResize::new(width, DEFAULT_WIDTH, MIN_WIDTH, ResizeEdge::Left),
            _task: None,
        };
        panel.open(false, window, cx);
        cx.on_release(|this, cx| this.services.ask_unmounted(cx))
            .detach();
        panel
    }

    pub fn item(&self) -> &InboxItem {
        &self.item
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Fresh metadata for the same item; the composer stays mounted.
    pub fn set_item(&mut self, item: InboxItem, cx: &mut Context<Self>) {
        self.item = item;
        cx.notify();
    }

    fn open(&mut self, restart: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let task = if restart {
            self.services.ask_restart(&self.item, cx)
        } else {
            self.services.ask(&self.item, cx)
        };
        let handle = window.window_handle();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(session_id) => {
                            this.pane = this.services.ask_pane(&session_id, window, cx);
                            this.session_id = Some(session_id);
                        }
                        Err(error) => this.error = Some(error),
                    }
                    this.loading = false;
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }

    /// "Restart conversation".
    pub fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.loading {
            self.open(true, window, cx);
        }
    }
}

impl Render for InboxDiscussionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let scale = theme.ui_scale();
        let viewport = f32::from(window.viewport_size().width) / scale;
        let narrow = viewport <= 1100.;
        let max = MIN_WIDTH.max(viewport - 650.);
        let mut aside = div()
            .id("inbox-discussion")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .min_h_0()
            .border_l_1()
            .border_color(theme.colors.stroke)
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if this.resize.drag_to(f32::from(event.position.x), scale, max) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if let Some(width) = this.resize.end() {
                        REMEMBERED_WIDTH.with(|cell| cell.set(width));
                    }
                }),
            );
        if narrow {
            aside = aside
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(theme.colors.background_base);
        } else {
            aside = aside.w(u(self.resize.width.min(max))).child(
                resize_handle(
                    "discussion-resize",
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
            );
        }
        let loading = self.loading;
        let header = div()
            .flex()
            .flex_none()
            .h(u(44.))
            .items_center()
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.body)
                    .medium()
                    .child(format!("Ask · {}", inbox_item_ref(&self.item))),
            )
            .child(
                icon_button("discussion-restart", IconName::RotateCcw)
                    .tooltip("Restart conversation")
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, window, cx| this.restart(window, cx))),
            )
            .child(
                icon_button("discussion-close", IconName::PanelLeft)
                    .tooltip("Close panel")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseDiscussion))),
            );
        aside = aside.child(header);
        if let Some(error) = self.error.clone() {
            aside = aside.child(
                div()
                    .p(u(12.))
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        let mut host = div().flex().flex_1().flex_col().min_h_0().min_w_0();
        if let Some(pane) = self.pane.clone() {
            host = host.child(pane);
        }
        aside.child(host)
    }
}

/// Opens a discussion panel entity for `item`.
pub fn open_discussion(
    services: Rc<dyn InboxServices>,
    item: InboxItem,
    window: &mut Window,
    cx: &mut gpui::App,
) -> gpui::Entity<InboxDiscussionPanel> {
    cx.new(|cx| InboxDiscussionPanel::new(services, item, window, cx))
}
