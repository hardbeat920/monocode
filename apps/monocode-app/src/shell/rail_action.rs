//! `RailAction`: a 32px rail row with a 16px icon, a label, and an
//! optional trailing hint. Port of src/app/shell/RailAction.tsx.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div,
};
use monocode_ui::color::with_alpha;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

/// One `RailAction` row: 32px, 16px icon at 70% of the row ink, 14px label.
pub(crate) fn rail_action(
    id: &'static str,
    label: &'static str,
    glyph: IconName,
    active: bool,
    trailing: Option<AnyElement>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let c = theme.colors;
    let (ink, fill) = if active {
        (c.content, Some(c.selection))
    } else {
        (theme.content(0.50), None)
    };
    let hover_fill = theme.content(0.10);
    let hover_ink = c.content;
    let mut row = div()
        .id(id)
        .group(id)
        .relative()
        .flex()
        .w_full()
        .items_center()
        .gap(u(8.))
        .px(u(8.))
        .h(u(32.))
        .rounded(u(theme.radius.md))
        .text_color(ink)
        .child(
            icon(glyph)
                .size(u(16.))
                .text_color(with_alpha(ink, 0.7))
                .group_hover(id, move |s| s.text_color(with_alpha(hover_ink, 0.7))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_px(theme.text.ui)
                .medium()
                .leading(theme.leading.tight)
                .child(label),
        )
        .children(trailing);
    if let Some(fill) = fill {
        row = row.bg(fill);
    } else {
        row = row.hover(move |s| s.bg(hover_fill).text_color(hover_ink));
    }
    row
}

/// The shortcut hint at the end of a rail row (`text-[11px] text-content/40`).
pub(crate) fn shortcut(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .flex_none()
        .text_px(theme.text.caption)
        .text_color(theme.content(0.40))
        .child(text)
        .into_any_element()
}
