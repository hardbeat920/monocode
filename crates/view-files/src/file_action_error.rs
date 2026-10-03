//! Port of src/features/files/ui/FileActionError.tsx: the alert in the
//! bottom right corner after a file action fails.

use std::rc::Rc;

use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, deferred, div,
};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::{Theme, UiStyled as _, u};

type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// `FileActionError`.
#[derive(IntoElement)]
pub struct FileActionError {
    message: SharedString,
    on_dismiss: DismissHandler,
}

pub fn file_action_error(
    message: impl Into<SharedString>,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> FileActionError {
    FileActionError {
        message: message.into(),
        on_dismiss: Rc::new(on_dismiss),
    }
}

impl RenderOnce for FileActionError {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let viewport = window.viewport_size();
        let on_dismiss = self.on_dismiss.clone();
        let hover = theme.colors.content;
        deferred(
            div().absolute().w(viewport.width).h(viewport.height).child(
                div()
                    .absolute()
                    .bottom(u(16.))
                    .right(u(16.))
                    .flex()
                    .max_w(u(384.))
                    .items_start()
                    .gap(u(12.))
                    .rounded(u(theme.radius.xl))
                    .border_1()
                    .border_color(with_alpha(theme.colors.danger, 0.30))
                    // `bg-[#252525]` in the React alert.
                    .bg(hex(0x252525))
                    .px(u(12.))
                    .py(u(8.))
                    .shadow_xl()
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger_soft)
                    .child(div().min_w_0().flex_1().child(self.message))
                    .child(
                        div()
                            .id("dismiss-file-action-error")
                            .flex_none()
                            .text_color(theme.content(0.60))
                            .hover(move |style| style.text_color(hover))
                            .on_click(move |_, window, cx| on_dismiss(window, cx))
                            .child("Dismiss"),
                    ),
            ),
        )
        .with_priority(theme.layer.toast)
    }
}
