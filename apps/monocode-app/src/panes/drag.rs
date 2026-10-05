//! The label GPUI displays while a session or workspace tab is dragged.
use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div};
use monocode_ui::{Theme, u};

pub(crate) struct WorkspaceDragPreview {
    pub label: String,
}
impl Render for WorkspaceDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .px(u(10.0))
            .py(u(6.0))
            .rounded(u(theme.radius.md))
            .bg(theme.colors.body_glass)
            .border_1()
            .border_color(theme.colors.stroke)
            .text_color(theme.colors.content)
            .text_size(u(12.0))
            .child(self.label.clone())
    }
}
