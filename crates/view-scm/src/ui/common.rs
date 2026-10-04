//! Small pieces the source control views share: the Tailwind colors the
//! React views used that monocode-ui's theme does not carry, the 20px icon
//! action, the spinning loader, anchored popovers, the git picker trigger,
//! and the editor theme built from the app theme.

use std::rc::Rc;
use std::time::Duration;

use gpui::Entity;
use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, ClickEvent, ElementId, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Transformation, Window, anchored, deferred, div,
    percentage, point, px, svg,
};
use gpui_base::input::{InputEditorStyle, Textarea as BaseTextarea, TextareaState};
use monocode_editor::{ColorScheme as EditorScheme, EditorTheme};
use monocode_ui::color::hex;
use monocode_ui::widgets::tooltip;
use monocode_ui::{ColorScheme, IconName, Theme, UiStyled as _, icon, u};

use crate::model::changes::StatusTone;

/// Tailwind v4 colors the React views named directly.
pub mod palette {
    use gpui::Hsla;
    use monocode_ui::color::hex;

    pub fn sky_400() -> Hsla {
        hex(0x00bcff)
    }
    pub fn rose_300() -> Hsla {
        hex(0xffa1ad)
    }
    pub fn rose_400() -> Hsla {
        hex(0xff637e)
    }
    pub fn rose_500() -> Hsla {
        hex(0xff2056)
    }
    pub fn rose_700() -> Hsla {
        hex(0xc70036)
    }
    pub fn emerald_300() -> Hsla {
        hex(0x5ee9b5)
    }
    pub fn emerald_500() -> Hsla {
        hex(0x00bc7d)
    }
    pub fn emerald_700() -> Hsla {
        hex(0x007a55)
    }
    pub fn red_500() -> Hsla {
        hex(0xfb2c36)
    }
    pub fn white() -> Hsla {
        hex(0xffffff)
    }
    pub fn black() -> Hsla {
        hex(0x000000)
    }
}

/// `statusColor`.
pub fn status_color(tone: StatusTone, theme: &Theme) -> Hsla {
    match tone {
        StatusTone::Untracked => palette::sky_400(),
        StatusTone::Added => theme.colors.diff_add_fg,
        StatusTone::Deleted => theme.colors.diff_del_fg,
        StatusTone::Modified => theme.colors.warning,
    }
}

/// A `#RRGGBB` graph color.
pub fn hex_color(value: &str) -> Hsla {
    u32::from_str_radix(value.trim_start_matches('#'), 16)
        .map(hex)
        .unwrap_or(gpui::transparent_black())
}

pub fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    monocode_ui::color::with_alpha(color, alpha)
}

/// `color-mix(in srgb, a weight%, b)`.
pub fn mix(a: Hsla, b: Hsla, weight: f32) -> Hsla {
    monocode_ui::color::mix(a, b, weight)
}

/// The `Loader` icon spinning (`animate-spin`, 1 s per turn).
pub fn spin_icon(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(IconName::Loader.path())
        .flex_none()
        .size(u(size))
        .text_color(color)
        .with_animation(
            id,
            Animation::new(Duration::from_secs(1)).repeat(),
            |svg, t| svg.with_transformation(Transformation::rotate(percentage(t))),
        )
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// `IconAction`: a 20px square button with a 14px glyph at 55% ink that
/// brightens on hover.
#[derive(IntoElement)]
pub struct IconAction {
    id: ElementId,
    icon: Option<IconName>,
    child: Option<AnyElement>,
    title: SharedString,
    disabled: bool,
    rounded: f32,
    on_click: Option<ClickHandler>,
}

pub fn icon_action(
    id: impl Into<ElementId>,
    icon: IconName,
    title: impl Into<SharedString>,
) -> IconAction {
    IconAction {
        id: id.into(),
        icon: Some(icon),
        child: None,
        title: title.into(),
        disabled: false,
        rounded: 4.,
        on_click: None,
    }
}

impl IconAction {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Replace the glyph with any element (a spinner, a smaller icon).
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(child.into_any_element());
        self.icon = None;
        self
    }

    /// `rounded-md` instead of `rounded`.
    pub fn rounded_md(mut self) -> Self {
        self.rounded = 6.;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for IconAction {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let ink = theme.content(0.55);
        let hover_ink = theme.colors.content;
        let mut el = div()
            .id(self.id)
            .group("scm-icon-action")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(20.))
            .rounded(u(self.rounded))
            .tooltip(tooltip(self.title.clone()));
        if self.disabled {
            el = el.opacity(0.4);
        } else {
            el = el.hover(|s| s.bg(theme.content(0.10)));
        }
        match (self.icon, self.child) {
            (Some(name), _) => {
                let glyph = icon(name).size(u(14.)).text_color(ink);
                el = el.child(if self.disabled {
                    glyph
                } else {
                    glyph.group_hover("scm-icon-action", move |s| s.text_color(hover_ink))
                });
            }
            (None, Some(child)) => el = el.child(child),
            (None, None) => {}
        }
        if let (Some(handler), false) = (self.on_click, self.disabled) {
            el = el.on_click(move |event, window, cx| {
                cx.stop_propagation();
                handler(event, window, cx)
            });
        }
        el
    }
}

/// Which side of its anchor a popover opens on, and which edge it lines up
/// with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverPlacement {
    TopStart,
    TopEnd,
    BottomStart,
    BottomEnd,
    RightStart,
}

/// A popover in the popover layer, placed against the parent element. The
/// parent must be `relative`. `gap` is in CSS px.
pub fn anchored_popover(
    placement: PopoverPlacement,
    gap: f32,
    layer: usize,
    content: impl IntoElement,
    window: &Window,
) -> impl IntoElement {
    let gap = u(gap).to_pixels(window.rem_size());
    let (pin, anchor, offset) = match placement {
        PopoverPlacement::TopStart => (
            div().top_0().left_0(),
            Anchor::BottomLeft,
            point(px(0.), -gap),
        ),
        PopoverPlacement::TopEnd => (
            div().top_0().right_0(),
            Anchor::BottomRight,
            point(px(0.), -gap),
        ),
        PopoverPlacement::BottomStart => (
            div().bottom_0().left_0(),
            Anchor::TopLeft,
            point(px(0.), gap),
        ),
        PopoverPlacement::BottomEnd => (
            div().bottom_0().right_0(),
            Anchor::TopRight,
            point(px(0.), gap),
        ),
        PopoverPlacement::RightStart => {
            (div().top_0().right_0(), Anchor::TopLeft, point(gap, px(0.)))
        }
    };
    pin.absolute().size_0().child(
        deferred(
            anchored()
                .anchor(anchor)
                .offset(offset)
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(layer),
    )
}

/// `GitPickerTrigger`: the branch or working copy button in the composer.
#[derive(IntoElement)]
pub struct GitPickerTrigger {
    id: ElementId,
    label: SharedString,
    title: SharedString,
    loading: bool,
    worktree: bool,
    expanded: bool,
    disabled: bool,
    dim_when_disabled: bool,
    on_click: Option<ClickHandler>,
}

pub fn git_picker_trigger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
) -> GitPickerTrigger {
    GitPickerTrigger {
        id: id.into(),
        label: label.into(),
        title: SharedString::default(),
        loading: false,
        worktree: false,
        expanded: false,
        disabled: false,
        dim_when_disabled: true,
        on_click: None,
    }
}

impl GitPickerTrigger {
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }
    pub fn worktree(mut self, worktree: bool) -> Self {
        self.worktree = worktree;
        self
    }
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for GitPickerTrigger {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let ink = if self.expanded {
            theme.colors.content
        } else {
            theme.content(0.55)
        };
        let glyph = if self.worktree {
            IconName::FolderTree
        } else {
            IconName::GitBranch
        };
        let mut el = div()
            .id(self.id)
            .group("scm-picker-trigger")
            .ml(u(-6.))
            .flex()
            .h(u(24.))
            .min_w_0()
            .max_w(u(256.))
            .items_center()
            .gap(u(6.))
            .rounded(u(6.))
            .px(u(6.))
            .text_px(12.)
            .text_color(ink);
        if self.expanded {
            el = el.bg(theme.content(0.08));
        }
        if self.disabled {
            if self.dim_when_disabled {
                el = el.opacity(0.4);
            }
        } else {
            let hover_ink = theme.colors.content;
            el = el.hover(move |s| s.bg(theme.content(0.08)).text_color(hover_ink));
        }
        if !self.title.is_empty() {
            el = el.tooltip(tooltip(self.title.clone()));
        }
        el = el.child(icon(glyph).size(u(14.)).text_color(ink));
        let label = if self.loading {
            // Keep the line box of a loaded label while the branch loads.
            div()
                .relative()
                .min_w_0()
                .flex_1()
                .child(div().invisible().child("main"))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(u(5.))
                        .h(u(6.))
                        .rounded_full()
                        .bg(with_alpha(ink, 0.5)),
                )
        } else {
            div().min_w_0().flex_1().truncate().child(self.label)
        };
        el = el.child(label);
        if self.worktree {
            el = el.child(
                div()
                    .flex_none()
                    .rounded(u(4.))
                    .bg(theme.content(0.08))
                    .px(u(4.))
                    .text_px(10.)
                    .text_color(theme.content(0.45))
                    .child("Worktree"),
            );
        }
        if let (Some(handler), false) = (self.on_click, self.disabled) {
            el = el.on_click(move |event, window, cx| handler(event, window, cx));
        }
        el
    }
}

/// The diff views' theme from the app theme.
pub fn editor_theme(cx: &App) -> EditorTheme {
    let theme = Theme::of(cx);
    let scheme = match theme.scheme {
        ColorScheme::Dark => EditorScheme::Dark,
        ColorScheme::Light => EditorScheme::Light,
    };
    let c = &theme.colors;
    let mut editor = EditorTheme::new(scheme, c.background_base, c.content).with_diff_colors(
        monocode_editor::DiffColors {
            add: c.diff_add,
            add_fg: c.diff_add_fg,
            add_bg: c.diff_add_bg,
            add_gutter: c.diff_add_gutter,
            del: c.diff_del,
            del_fg: c.diff_del_fg,
            del_bg: c.diff_del_bg,
            del_gutter: c.diff_del_gutter,
        },
    );
    editor.ui_font = theme.fonts.sans.clone();
    editor.mono_font = theme.fonts.mono.clone();
    editor
}

/// A centered status line for the diff wrappers (`grid h-full place-items-center`).
pub fn centered(content: impl IntoElement) -> gpui::Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(content)
}

/// Pixels to CSS px at the window's rem size.
pub fn css_px(value: gpui::Pixels, window: &Window) -> f32 {
    let rem: f32 = window.rem_size().into();
    let value: f32 = value.into();
    value / rem * 16.
}

/// The text colors for a plain multi-line input: content ink, a 35%
/// placeholder, and the accent selection.
pub fn textarea_style(theme: &Theme) -> InputEditorStyle {
    InputEditorStyle {
        foreground: theme.colors.content,
        muted_foreground: theme.content(0.35),
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        ..Default::default()
    }
}

/// A multi-line input with no frame or padding of its own, so the caller's
/// box sets its size (`textarea` with Tailwind classes). Text size and line
/// height come from the parent.
pub fn plain_textarea(state: &Entity<TextareaState>, cx: &mut App) -> impl IntoElement {
    let style = textarea_style(Theme::of(cx));
    state.update(cx, |state, _| state.set_editor_style(style));
    BaseTextarea::new(state)
}

/// An unstyled single-line input in the theme's text colors. Text size and
/// line height come from the parent. The styled gpui-component `Input`
/// would draw its own frame and padding.
pub fn plain_input(
    state: &Entity<gpui_component::input::InputState>,
    cx: &mut App,
) -> impl IntoElement {
    let style = textarea_style(Theme::of(cx));
    state.update(cx, |state, _| state.set_editor_style(style));
    gpui_base::input::Input::new(state)
}

/// A bordered single-line field: `h-9 rounded-md border border-content/10
/// px-2.5 text-[13px]`, with `focus:border-content/25`.
pub fn field_input(
    state: &Entity<gpui_component::input::InputState>,
    background: Hsla,
    window: &Window,
    cx: &mut App,
) -> gpui::Div {
    let theme = Theme::of(cx).clone();
    let focused = gpui::Focusable::focus_handle(state.read(cx), cx).is_focused(window);
    div()
        .flex()
        .h(u(36.))
        .w_full()
        .items_center()
        .rounded(u(6.))
        .border_1()
        .border_color(theme.content(if focused { 0.25 } else { 0.10 }))
        .bg(background)
        .px(u(10.))
        .text_px(13.)
        .line_height(u(20.))
        .text_color(theme.colors.content)
        .child(div().flex_1().min_w_0().child(plain_input(state, cx)))
}

/// A search row's input with no frame (`bg-transparent outline-none`).
pub fn bare_input(
    state: &Entity<gpui_component::input::InputState>,
    text_size: f32,
    cx: &mut App,
) -> impl IntoElement {
    div()
        .w_full()
        .text_px(text_size)
        .line_height(u(20.))
        .child(plain_input(state, cx))
}

/// Where an element was last painted, for telling a click on a popover's
/// own trigger from a click outside it.
pub type BoundsCell = Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>;

/// Records the parent's bounds into `cell`. The parent must be `relative`.
pub fn track_bounds(cell: &BoundsCell) -> impl IntoElement {
    let cell = cell.clone();
    gpui::canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

pub fn contains(cell: &BoundsCell, position: gpui::Point<gpui::Pixels>) -> bool {
    cell.get().is_some_and(|bounds| bounds.contains(&position))
}
