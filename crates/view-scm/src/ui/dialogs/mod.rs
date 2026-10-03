//! The branch and worktree dialogs, and the buttons they share.

pub mod create_branch;
pub mod create_worktree;
pub mod delete_worktree;
pub mod switch_branch;
pub mod switch_while_running;

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_ui::{Theme, UiStyled as _, u};

use crate::ui::common::{palette, spin_icon, with_alpha};

/// The button styles the dialogs use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    /// `text-content/70 hover:bg-content/8`.
    Ghost,
    /// `bg-content text-background-base`.
    Primary,
    /// `bg-content/10 text-content`.
    Secondary,
    /// `bg-red-500/20 text-red-400`.
    Danger,
}

/// A dialog button, `rounded-md px-3 py-1.5 text-[12px]`, with a spinner
/// before the label while `busy`.
pub fn dialog_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: DialogButton,
    disabled: bool,
    busy: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let c = theme.colors;
    let (bg, hover_bg, ink, hover_ink) = match kind {
        DialogButton::Ghost => (
            gpui::transparent_black(),
            theme.content(0.08),
            theme.content(0.70),
            c.content,
        ),
        DialogButton::Primary => (
            c.content,
            theme.content(0.80),
            c.background_base,
            c.background_base,
        ),
        DialogButton::Secondary => (
            theme.content(0.10),
            theme.content(0.15),
            c.content,
            c.content,
        ),
        DialogButton::Danger => (
            with_alpha(palette::red_500(), 0.20),
            with_alpha(palette::red_500(), 0.30),
            c.danger,
            c.danger,
        ),
    };
    let mut el = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(u(6.))
        .rounded(u(6.))
        .px(u(12.))
        .py(u(6.))
        .text_px(12.)
        .bg(bg)
        .text_color(ink);
    if kind != DialogButton::Ghost {
        el = el.medium();
    }
    if busy {
        el = el.child(spin_icon("dialog-button-spin", 14., ink));
    }
    el = el.child(label.into());
    if disabled {
        el.opacity(0.4)
    } else {
        el.hover(move |s| s.bg(hover_bg).text_color(hover_ink))
            .on_click(on_click)
    }
}

/// `text-[11px] text-red-400/90` error line.
pub fn error_line(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let theme = Theme::of(cx);
    div()
        .text_px(11.)
        .line_height(u(16.))
        .text_color(with_alpha(theme.colors.danger, 0.9))
        .child(text.into())
}
