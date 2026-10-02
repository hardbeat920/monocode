//! Centered modal dialogs. Port of src/shared/ui/Modal.tsx: a 16px rounded
//! glass panel (`rounded-2xl border-content/7 shadow-2xl`, tinted
//! `bg-background-base/55`) over a `bg-black/40` backdrop. The backdrop
//! fades in over 160ms; the panel content rises 8px over 200ms.
//!
//! Like the popover, the panel animation drops the CSS `scale(0.98)`.

use std::rc::Rc;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, RenderOnce, SharedString, Styled as _, Window, deferred, div,
    relative,
};

use crate::styled::{UiStyled as _, glass_backdrop};
use crate::widgets::button::icon_button;
use crate::{IconName, Theme, u};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModalSize {
    /// 420px wide, 22% from the top.
    Sm,
    /// 560px wide, 10% from the top.
    #[default]
    Md,
}

type CloseHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    size: ModalSize,
    minimal_header: bool,
    animate: bool,
    on_close: Option<CloseHandler>,
    children: Vec<AnyElement>,
}

/// A modal. Render it from the view that owns its open state; it paints in
/// the dialog layer over the whole window.
pub fn modal(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Modal {
    Modal {
        id: id.into(),
        title: title.into(),
        description: None,
        size: ModalSize::Md,
        minimal_header: false,
        animate: true,
        on_close: None,
        children: Vec::new(),
    }
}

impl Modal {
    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn size(mut self, size: ModalSize) -> Self {
        self.size = size;
        self
    }

    /// Hides the title and keeps only the close button in the corner.
    pub fn minimal_header(mut self) -> Self {
        self.minimal_header = true;
        self
    }

    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    /// Runs on the close button and on a click on the backdrop.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Modal {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Modal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let motion = theme.motion;
        let (width, top) = match self.size {
            ModalSize::Sm => (420.0, 0.22),
            ModalSize::Md => (560.0, 0.10),
        };

        let close = self.on_close.clone();
        let mut close_button = icon_button("modal-close", IconName::X).size(28.);
        if let Some(close) = close.clone() {
            close_button = close_button.on_click(move |_, window, cx| close(window, cx));
        }
        let header = if self.minimal_header {
            div()
                .absolute()
                .top(u(12.))
                .right(u(12.))
                .child(close_button)
        } else {
            let mut text = div().flex_1().min_w_0().pt(u(2.)).child(
                div()
                    .text_px(theme.text.title)
                    .medium()
                    .leading(theme.leading.tight)
                    .text_color(c.content)
                    .child(self.title.clone()),
            );
            if let Some(description) = self.description.clone() {
                text = text.child(
                    div()
                        .mt(u(2.))
                        .truncate()
                        .text_px(theme.text.label)
                        .leading(theme.leading.snug)
                        .text_color(theme.content(0.50))
                        .child(description),
                );
            }
            div()
                .flex()
                .flex_none()
                .items_start()
                .gap(u(8.))
                .px(u(16.))
                .pt(u(12.))
                .child(text)
                .child(close_button)
        };
        let body = div()
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .child(header)
            .child(div().flex().flex_col().min_h_0().children(self.children));
        let body = if self.animate {
            let rise = motion.modal_rise;
            body.with_animation(
                ElementId::from(SharedString::from(format!("{:?}-panel", self.id))),
                Animation::new(motion.modal_panel).with_easing(motion.modal_ease.easing()),
                move |el, t| el.opacity(t).top(u(rise * (1.0 - t))),
            )
            .into_any_element()
        } else {
            body.into_any_element()
        };
        let panel = div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_col()
            .w(u(width))
            .max_w_full()
            .rounded(u(theme.radius.xxl))
            .border_1()
            .border_color(c.modal_border)
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(glass_backdrop(theme.radius.xxl, 24., c.modal_backdrop))
            .child(body);

        let mut backdrop = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(c.modal_overlay);
        if let Some(close) = close {
            backdrop =
                backdrop.on_mouse_down(MouseButton::Left, move |_, window, cx| close(window, cx));
        }
        let backdrop = if self.animate {
            backdrop
                .with_animation(
                    ElementId::from(SharedString::from(format!("{:?}-backdrop", self.id))),
                    Animation::new(motion.modal_backdrop)
                        .with_easing(crate::theme::Motion::EASE_OUT.easing()),
                    |el, t| el.opacity(t),
                )
                .into_any_element()
        } else {
            backdrop.into_any_element()
        };

        deferred(
            div()
                .id("modal-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(backdrop)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .px(u(12.))
                        .child(div().flex_none().h(relative(top)))
                        .child(panel),
                ),
        )
        .with_priority(theme.layer.dialog)
    }
}
