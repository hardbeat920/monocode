//! Shared pieces of the inbox views: provider marks, label chips, people,
//! the detail action buttons, spinners, and the markdown style.
//!
//! The palette constants are the Tailwind v4 colors the React views name
//! (`text-violet-400/90`, `text-rose-400/90`) that `monocode-ui`'s theme has
//! no token for. Theme colors are used wherever a token exists.
// TODO(port): move the violet, rose, amber, and sky tones into
// monocode-ui's theme so other views can share them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ElementId, Hsla, Image, ImageFormat,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Transformation, div, img,
    percentage, prelude::FluentBuilder as _,
};
use monocode_markdown::{MarkdownStyle, SyntaxColors};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{GithubLabel, InboxProvider, ProjectMark};
use crate::model::label_color;

/// Tailwind v4 tones the inbox views use.
pub mod palette {
    use gpui::Hsla;
    use monocode_ui::color::hex;

    /// `violet-400`: merged and completed marks.
    pub fn violet_400() -> Hsla {
        hex(0xa684ff)
    }
    /// `rose-400`: closed marks, failures.
    pub fn rose_400() -> Hsla {
        hex(0xff637e)
    }
    /// `rose-300`: the dark-mode close confirmation ink.
    pub fn rose_300() -> Hsla {
        hex(0xffa1ad)
    }
    /// `rose-500`: the close confirmation fill.
    pub fn rose_500() -> Hsla {
        hex(0xff2056)
    }
    /// `rose-700`: light-mode failure ink.
    pub fn rose_700() -> Hsla {
        hex(0xc70036)
    }
    /// `emerald-300`: the dark-mode merge confirmation ink.
    pub fn emerald_300() -> Hsla {
        hex(0x5ee9b5)
    }
    /// `emerald-500`: the merge confirmation fill.
    pub fn emerald_500() -> Hsla {
        hex(0x00bc7d)
    }
    /// `emerald-700`: light-mode success ink.
    pub fn emerald_700() -> Hsla {
        hex(0x007a55)
    }
    /// `amber-700`: light-mode running ink.
    pub fn amber_700() -> Hsla {
        hex(0xbb4d00)
    }
    /// `sky-400`: media fallback links.
    pub fn sky_400() -> Hsla {
        hex(0x00bcff)
    }
}

/// `text-emerald-400/90`.
pub fn open_ink(theme: &Theme) -> Hsla {
    with_alpha(theme.colors.success, 0.9)
}

/// `text-rose-400/90`.
pub fn closed_ink() -> Hsla {
    with_alpha(palette::rose_400(), 0.9)
}

/// `text-violet-400/90`.
pub fn merged_ink() -> Hsla {
    with_alpha(palette::violet_400(), 0.9)
}

/// `text-emerald-700 dark:text-emerald-400`.
pub fn positive_ink(theme: &Theme) -> Hsla {
    if theme.is_dark() {
        theme.colors.success
    } else {
        palette::emerald_700()
    }
}

/// `text-rose-700 dark:text-rose-400`.
pub fn negative_ink(theme: &Theme) -> Hsla {
    if theme.is_dark() {
        palette::rose_400()
    } else {
        palette::rose_700()
    }
}

/// `text-amber-700 dark:text-amber-400`.
pub fn active_ink(theme: &Theme) -> Hsla {
    if theme.is_dark() {
        theme.colors.warning
    } else {
        palette::amber_700()
    }
}

// Provider marks. Port of src/features/inbox/ui/InboxProviderMark.tsx.

const GITLAB_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="96" viewBox="0 0 50 48" fill="none"><path d="m49.014 19-.067-.18-6.784-17.696a1.792 1.792 0 0 0-3.389.182l-4.579 14.02H15.651l-4.58-14.02a1.795 1.795 0 0 0-3.388-.182l-6.78 17.7-.071.175A12.595 12.595 0 0 0 5.01 33.556l.026.02.057.044 10.32 7.734 5.12 3.87 3.11 2.351a2.102 2.102 0 0 0 2.535 0l3.11-2.352 5.12-3.869 10.394-7.779.029-.022a12.595 12.595 0 0 0 4.182-14.554Z" fill="#E24329"/><path d="m49.014 19-.067-.18a22.88 22.88 0 0 0-9.12 4.103L24.931 34.187l9.485 7.167 10.393-7.779.03-.022a12.595 12.595 0 0 0 4.175-14.554Z" fill="#FC6D26"/><path d="m15.414 41.354 5.12 3.87 3.11 2.351a2.102 2.102 0 0 0 2.535 0l3.11-2.352 5.12-3.869-9.484-7.167-9.51 7.167Z" fill="#FCA326"/><path d="M10.019 22.923a22.86 22.86 0 0 0-9.117-4.1L.832 19A12.595 12.595 0 0 0 5.01 33.556l.026.02.057.044 10.32 7.734 9.491-7.167L10.02 22.923Z" fill="#FC6D26"/></svg>"##;

const AZURE_PATH: &str = "M15 3.62172V12.1336L11.5 15L6.075 13.025V14.9825L3.00375 10.9713L11.955 11.6704V4.00624L15 3.62172ZM12.0163 4.04994L6.99375 1V3.00125L2.3825 4.35581L1 6.12984V10.1586L2.9775 11.0325V5.86767L12.0163 4.04994Z";

const LINEAR_PATH: &str = "M1.22541 61.5228c-.2225-.9485.90748-1.5459 1.59638-.857L39.3342 97.1782c.6889.6889.0915 1.8189-.857 1.5964C20.0515 94.4522 5.54779 79.9485 1.22541 61.5228ZM.00189135 46.8891c-.01764375.2833.08887215.5599.28957165.7606L52.3503 99.7085c.2007.2007.4773.3075.7606.2896 2.3692-.1476 4.6938-.46 6.9624-.9259.7645-.157 1.0301-1.0963.4782-1.6481L2.57595 39.4485c-.55186-.5519-1.49117-.2863-1.648174.4782-.465915 2.2686-.77832 4.5932-.92588465 6.9624ZM4.21093 29.7054c-.16649.3738-.08169.8106.20765 1.1l64.77602 64.776c.2894.2894.7262.3742 1.1.2077 1.7861-.7956 3.5171-1.6927 5.1855-2.684.5521-.328.6373-1.0867.1832-1.5407L8.43566 24.3367c-.45409-.4541-1.21271-.3689-1.54074.1832-.99132 1.6684-1.88843 3.3994-2.68399 5.1855ZM12.6587 18.074c-.3701-.3701-.393-.9637-.0443-1.3541C21.7795 6.45931 35.1114 0 49.9519 0 77.5927 0 100 22.4073 100 50.0481c0 14.8405-6.4593 28.1724-16.7199 37.3375-.3903.3487-.984.3258-1.3542-.0443L12.6587 18.074Z";

const JIRA_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 24 24" fill="#2684FF"><path d="M11.571 11.513H0a5.218 5.218 0 0 0 5.232 5.215h2.13v2.057A5.215 5.215 0 0 0 12.575 24V12.518a1.005 1.005 0 0 0-1.005-1.005Zm5.723-5.756H5.736a5.215 5.215 0 0 0 5.215 5.214h2.129v2.058a5.218 5.218 0 0 0 5.215 5.214V6.758a1.001 1.001 0 0 0-1.001-1.001ZM23.013 0H11.455a5.215 5.215 0 0 0 5.215 5.215h2.129v2.057A5.215 5.215 0 0 0 24 12.483V1.005A1.001 1.001 0 0 0 23.013 0Z"/></svg>"##;

const GITHUB_PATH: &str = "M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12";

thread_local! {
    static MARKS: RefCell<HashMap<(u8, u32), Arc<Image>>> = RefCell::new(HashMap::new());
}

fn rgba_u32(color: Hsla) -> u32 {
    let rgb = color.to_rgb();
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(rgb.r) << 24) | (byte(rgb.g) << 16) | (byte(rgb.b) << 8) | byte(rgb.a)
}

fn monochrome_svg(view_box: f32, path: &str, rgba: u32) -> String {
    let color = format!("#{:06x}", rgba >> 8);
    let alpha = (rgba & 0xff) as f32 / 255.0;
    // Rasterize at 4x the view box so 14px marks stay crisp at 2x scale.
    let size = (view_box * 4.0).max(64.0);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {view_box} {view_box}"><path fill="{color}" fill-opacity="{alpha:.3}" d="{path}"/></svg>"#
    )
}

fn mark_image(provider: InboxProvider, ink: Hsla) -> Option<Arc<Image>> {
    let (slot, needs_color) = match provider {
        InboxProvider::Github => (0u8, true),
        InboxProvider::Linear => (1, true),
        InboxProvider::AzureDevops => (2, true),
        InboxProvider::Gitlab => (3, false),
        InboxProvider::Jira => (4, false),
    };
    let rgba = if needs_color { rgba_u32(ink) } else { 0 };
    MARKS.with(|marks| {
        let mut marks = marks.borrow_mut();
        let image = marks.entry((slot, rgba)).or_insert_with(|| {
            let svg = match provider {
                InboxProvider::Github => monochrome_svg(24.0, GITHUB_PATH, rgba),
                InboxProvider::Linear => monochrome_svg(100.0, LINEAR_PATH, rgba),
                InboxProvider::AzureDevops => monochrome_svg(16.0, AZURE_PATH, rgba),
                InboxProvider::Gitlab => GITLAB_SVG.to_string(),
                InboxProvider::Jira => JIRA_SVG.to_string(),
            };
            Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
        });
        Some(image.clone())
    })
}

/// `InboxProviderMark`: the provider's logo at `size` CSS px. GitHub,
/// Linear, and ADO draw in `ink` (`currentColor`); GitLab and Jira keep
/// their brand colors.
pub fn provider_mark(provider: InboxProvider, size: f32, ink: Hsla) -> AnyElement {
    match mark_image(provider, ink) {
        Some(image) => img(image).flex_none().size(u(size)).into_any_element(),
        None => icon(IconName::Inbox)
            .size(u(size))
            .text_color(ink)
            .into_any_element(),
    }
}

// Spinners.

/// An icon that turns once a second (`animate-spin`).
pub fn spin_icon(id: impl Into<ElementId>, name: IconName, size: f32, ink: Hsla) -> AnyElement {
    icon(name)
        .size(u(size))
        .text_color(ink)
        .with_animation(
            id,
            Animation::new(Duration::from_secs(1)).repeat(),
            |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
}

/// `LoaderCircle` spinning at `size`, the loading state of every panel.
pub fn loader(id: impl Into<ElementId>, size: f32, ink: Hsla) -> AnyElement {
    spin_icon(id, IconName::LoaderCircle, size, ink)
}

/// `flex justify-center py-10 text-content/40` with a 16px spinner.
pub fn centered_loader(id: impl Into<ElementId>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    div()
        .flex()
        .justify_center()
        .py(u(40.))
        .child(loader(id, 16., theme.content(0.40)))
        .into_any_element()
}

// Labels and people.

/// `InboxLabel`: a muted chip with the label's color dot.
pub fn label_chip(label: &GithubLabel, compact: bool, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let color = label_color(&label.color);
    div()
        .flex()
        .flex_none()
        .min_w_0()
        .items_center()
        .gap(u(4.))
        .rounded(u(theme.radius.sm))
        .px(u(6.))
        .py(gpui::px(1.))
        .bg(theme.content(0.08))
        .text_color(theme.content(0.50))
        .text_px(if compact { 10. } else { 11. })
        .leading(theme.leading.label)
        .when(compact, |chip| chip.max_w(u(80.)))
        .when_some(color, |chip, color| {
            chip.child(div().flex_none().size(u(6.)).rounded_full().bg(hex(color)))
        })
        .child(div().min_w_0().truncate().child(label.name.clone()))
        .into_any_element()
}

/// The round avatar of `InboxPerson` and `InboxCommentPerson`: the picture,
/// or the name's initial on `bg-content/12` while it loads or when it fails.
pub fn avatar(name: &str, url: &str, size: f32, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let initial: SharedString = monocode_core::js::trim(name)
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into())
        .into();
    let font = (size * 0.45).round().max(9.);
    let fill = theme.content(0.12);
    let ink = theme.content(0.55);
    let placeholder = move || {
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(size))
            .rounded_full()
            .bg(fill)
            .text_px(font)
            .medium()
            .text_color(ink)
            .child(initial.clone())
            .into_any_element()
    };
    if url.is_empty() {
        return placeholder();
    }
    let loading = placeholder.clone();
    img(SharedString::from(url.to_string()))
        .flex_none()
        .size(u(size))
        .rounded_full()
        .bg(theme.content(0.10))
        .with_loading(loading)
        .with_fallback(placeholder)
        .into_any_element()
}

/// `InboxPerson`: an avatar and a truncated name.
pub fn person(name: &str, url: &str, size: f32, cx: &App) -> AnyElement {
    div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(u(6.))
        .child(avatar(name, url, size, cx))
        .child(div().min_w_0().truncate().child(name.to_string()))
        .into_any_element()
}

/// The default project mark: the logo, else a dot in the mascot color.
pub fn default_project_mark(mark: &ProjectMark, size: f32, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    if let Some(path) = mark.logo_path.clone() {
        return img(std::path::PathBuf::from(path))
            .flex_none()
            .size(u(size + 2.))
            .rounded(u(theme.radius.sm))
            .into_any_element();
    }
    div()
        .flex_none()
        .size(u(size))
        .rounded(u(2.))
        .bg(mark.mascot_color.unwrap_or(theme.content(0.45)))
        .into_any_element()
}

// Detail action buttons (the `ACTION_*` classes in InboxView.tsx).

/// Which `ACTION_*` class a detail button uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// `ACTION_FILLED`: `h-6.5 bg-content text-background-base`.
    Filled,
    /// `ACTION_OUTLINE`: `h-7 border border-content/15 text-content/80`.
    Outline,
    /// `ACTION_PANEL_HEADER`: `h-6.5 text-content/70`, hover fill.
    PanelHeader,
    /// `ACTION_GHOST`: `h-7 text-content/70`, hover fill.
    Ghost,
}

/// A detail action button: `inline-flex items-center gap-1.5 rounded-md
/// px-3 text-[12px]` in one of the [`ActionKind`] looks.
pub fn action_button(
    id: impl Into<ElementId>,
    kind: ActionKind,
    icon_name: Option<IconName>,
    label: impl Into<SharedString>,
    disabled: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    action_button_with_hover_ink(id, kind, icon_name, label, disabled, None, cx)
}

/// [`action_button`] whose hover ink is `hover_ink` (the close button's
/// `hover:text-rose-400`).
pub fn action_button_with_hover_ink(
    id: impl Into<ElementId>,
    kind: ActionKind,
    icon_name: Option<IconName>,
    label: impl Into<SharedString>,
    disabled: bool,
    hover_ink_override: Option<Hsla>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let c = theme.colors;
    let (height, fill, ink, hover_fill, hover_ink, border) = match kind {
        ActionKind::Filled => (
            26.,
            c.content,
            c.background_base,
            Some(theme.content(0.80)),
            None,
            None,
        ),
        ActionKind::Outline => (
            28.,
            gpui::transparent_black(),
            theme.content(0.80),
            Some(theme.content(0.05)),
            None,
            Some(theme.content(0.15)),
        ),
        ActionKind::PanelHeader => (
            26.,
            gpui::transparent_black(),
            theme.content(0.70),
            Some(theme.content(0.10)),
            Some(c.content),
            None,
        ),
        ActionKind::Ghost => (
            28.,
            gpui::transparent_black(),
            theme.content(0.70),
            Some(theme.content(0.10)),
            Some(c.content),
            None,
        ),
    };
    let hover_ink = hover_ink_override.or(hover_ink);
    let group: SharedString = "inbox-action".into();
    let mut el = div()
        .id(id)
        .group(group.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(u(6.))
        .h(u(height))
        .px(u(12.))
        .rounded(u(theme.radius.md))
        .text_px(theme.text.label)
        .whitespace_nowrap()
        .bg(fill)
        .text_color(ink);
    if let Some(border) = border {
        el = el.border_1().border_color(border);
    }
    if disabled {
        el = el.opacity(0.4);
    } else {
        el = el.hover(move |s| {
            let mut s = s;
            if let Some(fill) = hover_fill {
                s = s.bg(fill);
            }
            if let Some(ink) = hover_ink {
                s = s.text_color(ink);
            }
            s
        });
    }
    if let Some(name) = icon_name {
        let glyph = icon(name).size(u(14.)).text_color(ink);
        let glyph = match (disabled, hover_ink) {
            (false, Some(hover)) => glyph.group_hover(group, move |s| s.text_color(hover)),
            _ => glyph,
        };
        el = el.child(glyph);
    }
    el.child(label.into())
}

/// The small square buttons of the list toolbar and the checks header
/// (`grid size-6 rounded-md text-content/45 hover:bg-content/10`).
pub fn square_button(
    id: impl Into<ElementId>,
    glyph: AnyElement,
    selected: bool,
    disabled: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let mut el = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(24.))
        .rounded(u(theme.radius.md))
        .child(glyph);
    if selected {
        el = el.bg(theme.colors.selection);
    }
    if disabled {
        el = el.opacity(0.3);
    } else {
        let hover = theme.content(0.10);
        el = el.hover(move |s| s.bg(hover));
    }
    el
}

/// Adds a tooltip to an element, the stand-in for a native `title`.
pub fn titled(
    el: gpui::Stateful<gpui::Div>,
    title: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    let title: SharedString = title.into();
    el.tooltip(tooltip(title))
}

/// `.agent-markdown` at `text-sm leading-6`, for item bodies and comments.
pub fn markdown_style(theme: &Theme) -> MarkdownStyle {
    let syntax = if theme.is_dark() {
        SyntaxColors::github_dark()
    } else {
        SyntaxColors::github_light()
    };
    let mut style = MarkdownStyle::with_content(
        theme.colors.content,
        theme.colors.markdown_heading,
        with_alpha(palette::sky_400(), 0.9),
        theme.colors.link,
        syntax,
    );
    style.font_family = theme.fonts.sans.clone();
    style.mono_font_family = theme.fonts.mono.clone();
    style.selection = theme.accent(0.35);
    let scale = theme.ui_scale();
    if (scale - 1.).abs() > f32::EPSILON {
        let s = |value: gpui::Pixels| value * scale;
        style.text_size = s(style.text_size);
        style.line_height = s(style.line_height);
        style.heading_sizes = style.heading_sizes.map(s);
        style.heading_line_heights = style.heading_line_heights.map(s);
        style.inline_code_size = s(style.inline_code_size);
        style.code_size = s(style.code_size);
        style.code_line_height = s(style.code_line_height);
    }
    style
}

/// A thin horizontal rule in the stroke color.
pub fn rule(cx: &App) -> AnyElement {
    div()
        .h(gpui::px(1.))
        .w_full()
        .bg(Theme::of(cx).colors.stroke)
        .into_any_element()
}

/// The muted section label of the filter and connect menus (`text-[10px]
/// font-semibold uppercase tracking-[0.08em] text-content/40`).
pub fn section_label(text: &str, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    div()
        .px(u(8.))
        .pb(u(2.))
        .pt(u(8.))
        .text_px(theme.text.micro)
        .semibold()
        .text_color(theme.content(0.40))
        .child(text.to_uppercase())
        .into_any_element()
}

/// Which edge of the anchor a popover lines up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverAlign {
    Start,
    End,
}

/// Pins `content` under its relative parent in the popover layer, `gap`
/// CSS px below it, aligned to the parent's start or end edge. Popover.tsx
/// with `side="bottom"`.
pub fn popover_below(
    align: PopoverAlign,
    gap: f32,
    content: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let rem = theme.rem_size();
    let pin = div().absolute().top_full();
    let (pin, anchor) = match align {
        PopoverAlign::Start => (pin.left_0(), gpui::Anchor::TopLeft),
        PopoverAlign::End => (pin.right_0(), gpui::Anchor::TopRight),
    };
    pin.child(
        gpui::deferred(
            gpui::anchored()
                .anchor(anchor)
                .offset(gpui::point(gpui::px(0.), u(gap).to_pixels(rem)))
                .snap_to_window_with_margin(gpui::px(monocode_ui::widgets::POPOVER_PADDING))
                .child(content),
        )
        .with_priority(theme.layer.popover),
    )
    .into_any_element()
}

/// The color family of a status mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    /// `text-content/50`: drafts.
    Muted,
    /// `text-violet-400/90`: merged, and issues closed as completed.
    Merged,
    /// `text-rose-400/90`: closed.
    Closed,
    /// `text-emerald-400/90`: open.
    Open,
}

/// `InboxStatusMark`: the glyph, its color, and the status label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusMark {
    pub icon: IconName,
    pub tone: StatusTone,
    pub label: &'static str,
}

/// `inboxStatusMark`: status reads from the glyph first and the color
/// second, so it survives color blindness.
pub fn inbox_status_mark(item: &crate::data::InboxItem) -> StatusMark {
    use crate::data::InboxKind;
    let label = crate::model::inbox_item_status(item);
    let pr = item.kind == InboxKind::Pr;
    match label {
        "Draft" => StatusMark {
            icon: IconName::GitPullRequestDraft,
            tone: StatusTone::Muted,
            label,
        },
        "Merged" => StatusMark {
            icon: IconName::GitMerge,
            tone: StatusTone::Merged,
            label,
        },
        "Closed" => {
            let completed = item.provider == InboxProvider::Github
                && item.kind == InboxKind::Issue
                && item
                    .state_reason
                    .as_deref()
                    .is_some_and(|reason| reason.trim().to_lowercase() == "completed");
            if completed {
                StatusMark {
                    icon: IconName::CheckCircle,
                    tone: StatusTone::Merged,
                    label,
                }
            } else {
                StatusMark {
                    icon: if pr {
                        IconName::GitPullRequestClosed
                    } else {
                        IconName::CircleX
                    },
                    tone: StatusTone::Closed,
                    label,
                }
            }
        }
        _ => StatusMark {
            icon: if pr {
                IconName::GitPullRequest
            } else {
                IconName::CircleDot
            },
            tone: StatusTone::Open,
            label,
        },
    }
}

/// The ink of a status tone.
pub fn status_ink(tone: StatusTone, theme: &Theme) -> Hsla {
    match tone {
        StatusTone::Muted => theme.content(0.50),
        StatusTone::Merged => merged_ink(),
        StatusTone::Closed => closed_ink(),
        StatusTone::Open => open_ink(theme),
    }
}

/// Which side of a pane its resize handle sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    /// The handle is on the right; dragging right widens (`useDragResize`).
    Right,
    /// The handle is on the left; dragging left widens (`direction: "left"`).
    Left,
}

/// `useDragResize`: a pane width in CSS px with a drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneResize {
    pub width: f32,
    pub default_width: f32,
    pub min: f32,
    pub edge: ResizeEdge,
    drag: Option<(f32, f32)>,
}

impl PaneResize {
    pub fn new(width: f32, default_width: f32, min: f32, edge: ResizeEdge) -> Self {
        Self {
            width,
            default_width,
            min,
            edge,
            drag: None,
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Pointer down on the handle at window x `x` (logical px).
    pub fn begin(&mut self, x: f32) {
        self.drag = Some((x, self.width));
    }

    /// Pointer moved to `x`. `scale` is the interface scale; `max` the
    /// largest width in CSS px. Returns whether the width changed.
    pub fn drag_to(&mut self, x: f32, scale: f32, max: f32) -> bool {
        let Some((start, start_width)) = self.drag else {
            return false;
        };
        let delta = (x - start) / scale.max(0.01);
        let delta = match self.edge {
            ResizeEdge::Right => delta,
            ResizeEdge::Left => -delta,
        };
        let next = (start_width + delta)
            .round()
            .clamp(self.min, max.max(self.min));
        let changed = next != self.width;
        self.width = next;
        changed
    }

    /// Pointer up. Returns the width to remember when a drag ended.
    pub fn end(&mut self) -> Option<f32> {
        self.drag.take().map(|_| self.width)
    }

    /// Double click: back to the default width.
    pub fn reset(&mut self) -> f32 {
        self.width = self.default_width;
        self.width
    }
}

/// The resize handle strip: `w-1.5` (or `w-2`) that tints while hovered or
/// dragged.
pub fn resize_handle(
    id: impl Into<ElementId>,
    edge: ResizeEdge,
    width: f32,
    dragging: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover = theme.content(0.10);
    let mut handle = div()
        .id(id)
        .absolute()
        .top_0()
        .bottom_0()
        .w(u(width))
        .cursor(gpui::CursorStyle::ResizeLeftRight);
    handle = match edge {
        ResizeEdge::Right => handle.right(gpui::px(-1.)),
        ResizeEdge::Left => handle.left(u(-(width / 2.))),
    };
    if dragging {
        handle.bg(theme.content(0.15))
    } else {
        handle.hover(move |s| s.bg(hover))
    }
}
