//! Text field wrapper over gpui-component's input, styled like the session
//! search field in Sidebar.tsx: a 28px row with a leading 12px icon and 12px
//! text, no border.

use gpui::{App, Entity, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div};
use gpui_component::input::{Input, InputState};

use crate::styled::UiStyled as _;
use crate::{IconName, Theme, icon, u};

#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    icon: Option<IconName>,
    bordered: bool,
}

/// A text field for an `InputState` the caller owns:
/// `cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions"))`.
pub fn text_field(state: &Entity<InputState>) -> TextField {
    TextField {
        state: state.clone(),
        icon: None,
        bordered: false,
    }
}

impl TextField {
    /// A leading icon, drawn at half strength like the search glyph.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Adds the `border-content/10` outline used by settings fields.
    pub fn bordered(mut self) -> Self {
        self.bordered = true;
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut row = div()
            .relative()
            .flex()
            .items_center()
            .h(u(28.))
            .min_w_0()
            .flex_1()
            .rounded(u(theme.radius.md))
            .text_px(theme.text.label)
            .text_color(theme.colors.content);
        if self.bordered {
            row = row.border_1().border_color(theme.content(0.10));
        }
        if let Some(name) = self.icon {
            row = row.child(
                div()
                    .absolute()
                    .left(u(8.))
                    .child(icon(name).size(u(12.)).text_color(theme.content(0.50))),
            );
        }
        row.child(
            div()
                .size_full()
                .pl(u(if self.icon.is_some() { 28. } else { 8. }))
                .pr(u(8.))
                .child(
                    Input::new(&self.state)
                        .appearance(false)
                        .h_full()
                        .p_0()
                        .text_px(theme.text.label),
                ),
        )
    }
}
