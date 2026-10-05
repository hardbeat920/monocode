//! Tailwind class groups the picker components repeat, as element builders.

use gpui::{
    Div, ElementId, Hsla, InteractiveElement as _, ParentElement as _, Stateful, Styled as _, div,
};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

/// The toolbar pill: `flex h-6.5 items-center gap-1 rounded-md px-1.5
/// bg-selection text-content hover:bg-selection-hover`.
pub fn toolbar_pill(id: impl Into<ElementId>, open: bool, theme: &Theme) -> Stateful<Div> {
    let c = theme.colors;
    let hover = c.selection_hover;
    let pill = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(u(4.))
        .h(u(26.))
        .px(u(6.))
        .rounded(u(theme.radius.md))
        .bg(c.selection)
        .text_color(c.content);
    if open {
        pill
    } else {
        pill.hover(move |style| style.bg(hover))
    }
}

/// `text-[11px]` truncated label inside a toolbar pill.
pub fn pill_label(text: impl Into<gpui::SharedString>, color: Option<Hsla>, theme: &Theme) -> Div {
    let mut label = div()
        .min_w_0()
        .truncate()
        .text_px(theme.text.caption)
        .leading(theme.leading.normal);
    if let Some(color) = color {
        label = label.text_color(color);
    }
    label.child(text.into())
}

/// The `size-3 text-content/50` chevron, pointing up while its menu is open.
pub fn pill_chevron(open: bool, theme: &Theme) -> gpui::Svg {
    icon(if open {
        IconName::ChevronUp
    } else {
        IconName::ChevronDown
    })
    .size(u(12.))
    .text_color(theme.content(0.50))
}

/// A menu row: `flex w-full items-center gap-2 rounded-lg px-2 text-[13px]`,
/// `bg-selection` when highlighted, else `hover:bg-content/5`.
pub fn menu_row(
    id: impl Into<ElementId>,
    height: f32,
    highlighted: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let hover = theme.content(0.05);
    let row = div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .items_center()
        .gap(u(8.))
        .h(u(height))
        .px(u(8.))
        .rounded(u(theme.radius.lg))
        .text_px(theme.text.body)
        .text_color(theme.colors.content);
    if highlighted {
        row.bg(theme.colors.selection)
    } else {
        row.hover(move |style| style.bg(hover))
    }
}

/// The group caption inside a menu: `px-2.5 pb-1 text-[10px] font-medium
/// uppercase tracking-wide text-content/40`.
pub fn group_caption(text: &str, top: f32, theme: &Theme) -> Div {
    div()
        .px(u(10.))
        .pt(u(top))
        .pb(u(4.))
        .text_px(theme.text.micro)
        .medium()
        .leading(theme.leading.normal)
        .text_color(theme.content(0.40))
        .child(text.to_uppercase())
}

/// `my-1 h-px bg-content/10`.
pub fn menu_separator(theme: &Theme) -> Div {
    div()
        .my(u(4.))
        .h(gpui::px(1.))
        .flex_none()
        .bg(theme.content(0.10))
}

/// The `size-3.5` check with `text-content/50`, stroke 2 in React.
pub fn check_mark(alpha: f32, theme: &Theme) -> gpui::Svg {
    icon(IconName::Check)
        .size(u(14.))
        .text_color(theme.content(alpha))
}
