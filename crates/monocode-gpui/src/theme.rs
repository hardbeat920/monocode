use gpui::{Div, Hsla, Role, Stateful, Svg, div, img, prelude::*, px, rgb, rgba, svg};
// Shared appearance tokens. Interaction fills use translucent content ink
// rather than opaque replacement greys.
pub(crate) const CHAT: u32 = 0x141414;
pub(crate) const TEXT: u32 = 0xebebeb;
pub(crate) const MUTED: u32 = 0x969696;
pub(crate) const ACCENT: u32 = 0x4da3f5;
pub(crate) const COLUMN: f32 = 832.;
pub(crate) const TITLE_BAR_HEIGHT: f32 = 40.;
pub(crate) const TITLE_TAB_TEXT_SIZE: f32 = 13.;
pub(crate) const TITLE_TAB_MAX_WIDTH: f32 = 224.;
pub(crate) const PANE_HEADER_HEIGHT: f32 = 36.;
/// Every tab's plate: title, file and terminal tabs, and the new-tab button.
pub(crate) const TAB_PLATE_HEIGHT: f32 = 27.;

/// Attachment previews keep a fixed crop, independent of image dimensions.
pub(crate) fn attachment_thumbnail(source: impl Into<gpui::ImageSource>) -> gpui::AnyElement {
    div()
        .size(px(36.))
        .min_w(px(36.))
        .min_h(px(36.))
        .max_w(px(36.))
        .max_h(px(36.))
        .flex_shrink_0()
        .rounded_lg()
        .overflow_hidden()
        .child(
            img(source)
                .size(px(36.))
                .aspect_square()
                .flex_shrink_0()
                .rounded_lg()
                .object_fit(gpui::ObjectFit::Cover),
        )
        .into_any_element()
}

#[derive(Clone, Copy, PartialEq)]
struct Tokens {
    light: bool,
    ink: Hsla,
    accent: Hsla,
    user_accent: Option<Hsla>,
    shell: Hsla,
    pane: Hsla,
    inset_pane: Hsla,
    syntax: [u32; 11],
    diff: [u32; 4],
    diff_strong: bool,
}
thread_local! { static TOKENS: std::cell::RefCell<Option<Tokens>> = const { std::cell::RefCell::new(None) }; }
pub(crate) fn configure(p: &crate::settings::Preferences, system_light: bool) -> bool {
    let light = p.theme == "light" || (p.theme == "system" && system_light);
    let palette = crate::settings::palette(&p.workspace_theme, light);
    let glass = window_glass(p, system_light);
    // Both layouts derive their surface roles from the same saved settings.
    let shell = gpui::hsla(
        p.theme_hue.min(360) as f32 / 360.,
        p.theme_saturation.min(100) as f32 / 100.,
        if light {
            0.97
        } else {
            p.theme_dark_lightness.min(30) as f32 / 100.
        },
        if glass {
            p.sidebar_opacity.clamp(15, 100) as f32 / 100.
        } else {
            1.
        },
    );
    let mut pane = shell;
    pane.a = if glass && p.body_glass { 0.28 } else { 1. };
    // The window already paints its glass tint. A glass content card reveals
    // that layer directly; adding another tint would hide the desktop twice.
    // Solid content keeps its deeper dark tone or white light appearance.
    let mut inset_pane = shell;
    inset_pane.l = if light { 1. } else { shell.l * 0.45 };
    inset_pane.a = if glass && p.body_glass { 0. } else { 1. };
    let custom = p
        .accent_color
        .as_deref()
        .and_then(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
        .filter(|_| p.accent_color.as_ref().is_some_and(|s| s.len() == 7));
    let tokens = Tokens {
        light,
        ink: rgb(palette.content).into(),
        accent: rgb(custom.unwrap_or(palette.accent)).into(),
        user_accent: custom.map(|c| rgb(c).into()),
        shell,
        pane,
        inset_pane,
        diff: diff_palette(&p.diff_palette, light),
        diff_strong: p.diff_palette == "high-contrast",
        syntax: syntax_palette(&p.workspace_theme, light),
    };
    TOKENS.with(|current| {
        let changed = *current.borrow() != Some(tokens);
        *current.borrow_mut() = Some(tokens);
        changed
    })
}

/// Platforms whose compositor blurs the desktop behind the window: macOS
/// vibrancy and Windows Acrylic. Linux blur is compositor-dependent, so it
/// stays opaque rather than exposing the raw desktop.
pub(crate) const GLASS_PLATFORM: bool = cfg!(any(target_os = "macos", target_os = "windows"));
pub(crate) fn window_glass(p: &crate::settings::Preferences, system_light: bool) -> bool {
    GLASS_PLATFORM && p.theme != "light" && !(p.theme == "system" && system_light)
}
/// What the compositor puts behind a window that does or doesn't paint glass.
pub(crate) fn window_backdrop(glass: bool) -> gpui::WindowBackgroundAppearance {
    if glass && GLASS_PLATFORM {
        gpui::WindowBackgroundAppearance::Blurred
    } else {
        gpui::WindowBackgroundAppearance::Opaque
    }
}
pub(crate) fn accent_color() -> Hsla {
    TOKENS.with(|t| {
        t.borrow()
            .as_ref()
            .map(|t| t.accent)
            .unwrap_or_else(|| rgb(ACCENT).into())
    })
}
pub(crate) fn content(alpha: f32) -> Hsla {
    let mut color: Hsla = TOKENS.with(|t| {
        t.borrow()
            .as_ref()
            .map(|t| t.ink)
            .unwrap_or_else(|| rgb(TEXT).into())
    });
    color.a = alpha;
    color
}

/// `[data-composer-box]::before` uses content ink at 3% over its backdrop.
/// Light mode instead gives the box the solid `background-base` surface.
pub(crate) fn composer_fill() -> Hsla {
    let light = TOKENS.with(|tokens| tokens.borrow().as_ref().is_some_and(|t| t.light));
    if light {
        let mut background = shell_fill();
        background.a = 1.;
        background
    } else {
        content(0.03)
    }
}

/// The inset composer's frost; the solid light surface has no backdrop blur.
pub(crate) fn composer_blur() -> f32 {
    let light = TOKENS.with(|tokens| tokens.borrow().as_ref().is_some_and(|t| t.light));
    if light { 0. } else { 24. }
}

/// The accent picked in Settings; `None` keeps the theme's neutral ink.
fn user_accent() -> Option<Hsla> {
    TOKENS.with(|t| t.borrow().as_ref().and_then(|t| t.user_accent))
}
/// `var(--user-accent-color, var(--color-accent))`.
pub(crate) fn picked_accent() -> Hsla {
    user_accent().unwrap_or_else(accent_color)
}
/// `.user-message-bubble`: a picked accent tints sent messages at 24%.
pub(crate) fn user_bubble_fill() -> Hsla {
    user_accent().map_or(content(0.1), |a| a.opacity(0.24))
}
/// `.user-message-bubble[data-draft]`: an 8% wash inside a 38% dashed edge.
pub(crate) fn draft_bubble_fill() -> Hsla {
    user_accent().map_or(content(0.04), |a| a.opacity(0.08))
}
pub(crate) fn draft_bubble_edge() -> Hsla {
    user_accent().map_or(content(0.3), |a| a.opacity(0.38))
}
/// `.primary-action` under `has-user-accent`: the accent, with black or white
/// ink by its relative luminance; `None` keeps the default solid button.
pub(crate) fn primary_action(enabled: bool) -> Option<(Hsla, Hsla)> {
    let accent = user_accent()?;
    if !enabled {
        return Some((accent.opacity(0.28), content(0.35)));
    }
    let rgba = accent.to_rgb();
    let linear = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b);
    Some((
        accent,
        if luminance > 0.179 {
            black(1.)
        } else {
            white(1.)
        },
    ))
}

/// Selected text is tinted with the accent; light surfaces get a paler wash
/// so dark ink stays readable on top.
pub(crate) fn text_selection() -> Hsla {
    accent_color().opacity(if is_light() { 0.2 } else { 0.35 })
}

/// Light mode lifts the solid composer off the matching page with a soft
/// two-layer drop; dark mode's frosted box needs no shadow.
pub(crate) fn composer_shadow() -> Vec<gpui::BoxShadow> {
    if !is_light() {
        return Vec::new();
    }
    vec![
        gpui::BoxShadow::new(px(0.), px(1.), black(0.04)).blur_radius(px(2.)),
        gpui::BoxShadow::new(px(0.), px(6.), black(0.07))
            .blur_radius(px(20.))
            .spread_radius(px(-4.)),
    ]
}

/// Light themes paint floating surfaces opaque instead of frosting them.
pub(crate) fn is_light() -> bool {
    TOKENS.with(|tokens| tokens.borrow().as_ref().is_some_and(|t| t.light))
}
/// The opaque card behind popovers and modals in light themes, and wherever
/// the renderer has no backdrop blur.
pub(crate) fn floating_fill() -> Hsla {
    let mut fill = if is_light() {
        inset_pane_fill()
    } else {
        pane_fill()
    };
    if is_light() {
        fill.l = fill.l.max(0.995);
    }
    fill.a = 1.;
    fill
}

pub(crate) fn composer_border(focused: bool) -> Hsla {
    content(if focused { 0.2 } else { 0.1 })
}

pub(crate) fn title_tab_fill(selected: bool, hovered: bool) -> Hsla {
    content(if selected {
        0.1
    } else if hovered {
        0.07
    } else {
        0.04
    })
}

/// The plate for session tabs in the title bar.
/// Headline ink owns inactive dimming; the plate never fades its children.
pub(crate) fn title_tab_plate(selected: bool, hovered: bool) -> Div {
    div()
        .h(px(TAB_PLATE_HEIGHT))
        .rounded_md()
        .px_2()
        .flex()
        .items_center()
        .bg(title_tab_fill(selected, hovered))
}
pub(crate) fn shell_fill() -> Hsla {
    if let Some(fill) = TOKENS.with(|t| t.borrow().as_ref().map(|t| t.shell)) {
        return fill;
    }
    if GLASS_PLATFORM {
        // #0d0d0d at 80%, with the stylesheet's #ebebeb 5% wash above it.
        rgba(0x1b1b1bcf).into()
    } else {
        rgb(0x181818).into()
    }
}
pub(crate) fn pane_fill() -> Hsla {
    if let Some(fill) = TOKENS.with(|t| t.borrow().as_ref().map(|t| t.pane)) {
        return fill;
    }
    if GLASS_PLATFORM {
        rgba(0x06060666).into()
    } else {
        rgb(CHAT).into()
    }
}
/// Apply to surrounding chrome, leaving the content on the window's glass.
pub(crate) fn inset_chrome_wash() -> Hsla {
    content(0.05)
}
pub(crate) fn inset_pane_fill() -> Hsla {
    TOKENS
        .with(|tokens| tokens.borrow().as_ref().map(|t| t.inset_pane))
        .unwrap_or_else(pane_fill)
}
pub(crate) fn section_label(label: &'static str) -> Div {
    div()
        .h(px(28.))
        .px_2()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(content(0.5))
        .child(div().flex_1().child(label))
}
pub(crate) fn icon(path: &'static str, side: f32) -> Svg {
    svg()
        .path(path)
        .size(px(side))
        .text_color(content(0.5))
        .flex_shrink_0()
}
pub(crate) fn provider_icon(provider: &str, side: f32) -> Div {
    let (path, monochrome, scale) = match provider {
        "claude" => ("claude.svg", false, 1.),
        "cursor" => ("cursor.svg", true, 1.),
        "grok" => ("grok.svg", true, 1.),
        "opencode" => ("opencode.svg", true, 1.),
        "antigravity" => ("antigravity.svg", false, 1.),
        "pi" => ("pi.svg", true, 1.),
        "omp" => ("omp.svg", false, 1.),
        "fx" => ("fx.svg", true, 1.),
        "hermes" => ("hermes.svg", true, 0.72),
        "devin" => ("devin.svg", true, 0.88),
        _ => ("codex.svg", false, 1.),
    };
    // Like React's currentColor / CSS masks, GPUI's SVG mask follows the theme.
    // Image rendering preserves brand colors but has no inherited currentColor.
    let mark = if monochrome {
        svg()
            .path(path)
            .size(px(side * scale))
            .text_color(content(1.))
            .flex_shrink_0()
            .into_any_element()
    } else {
        img(path).size(px(side)).flex_shrink_0().into_any_element()
    };
    div()
        .size(px(side))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(mark)
}
pub(crate) fn icon_button(
    id: &'static str,
    label: &'static str,
    path: &'static str,
) -> Stateful<Div> {
    icon_button_hover(id, label, path, 0.1)
}
pub(crate) fn icon_button_hover(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    hover_alpha: f32,
) -> Stateful<Div> {
    chrome_button(id, label, path, true, hover_alpha)
}
pub(crate) fn icon_button_enabled(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    chrome_button(id, label, path, enabled, 0.1)
}
fn chrome_button(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    enabled: bool,
    hover_alpha: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .group("chrome-button")
        .cursor_pointer()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(24.))
        .rounded_md()
        .text_color(content(if enabled { 0.5 } else { 0.25 }))
        .when(enabled, |d| {
            d.hover(|s| s.bg(content(hover_alpha)).text_color(content(1.)))
        })
        .child(
            icon(path, 14.)
                .text_color(content(if enabled { 0.5 } else { 0.25 }))
                .when(enabled, |i| {
                    i.group_hover("chrome-button", |s| s.text_color(content(1.)))
                }),
        )
}
// `.primary-action` paints with literal white and black, not theme ink.
pub(crate) fn white(alpha: f32) -> Hsla {
    gpui::hsla(0., 0., 1., alpha)
}
pub(crate) fn black(alpha: f32) -> Hsla {
    gpui::hsla(0., 0., 0., alpha)
}

const SYNTAX_KEYS: [&str; 11] = [
    "keyword", "callable", "string", "type", "tag", "number", "comment", "property", "meta",
    "invalid", "heading",
];
fn syntax_palette(theme: &str, light: bool) -> [u32; 11] {
    static PALETTES: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let palettes = PALETTES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/editor-palettes.json"))
            .expect("bundled editor palettes")
    });
    let palette = palettes.get(theme).unwrap_or(&palettes["default"]);
    let palette = &palette[if light { "light" } else { "dark" }];
    SYNTAX_KEYS.map(|key| {
        u32::from_str_radix(
            palette[key]
                .as_str()
                .unwrap_or("#ebebeb")
                .trim_start_matches('#'),
            16,
        )
        .unwrap_or(0xebebeb)
    })
}
pub(crate) fn syntax_color(kind: &str) -> Hsla {
    let name = kind
        .strip_prefix("var(--syntax-")
        .and_then(|s| s.strip_suffix(')'));
    let index = if kind == "var(--color-markdown-heading)" {
        Some(10)
    } else {
        name.and_then(|name| SYNTAX_KEYS.iter().position(|key| *key == name))
    };
    index.map_or_else(
        || content(1.),
        |index| {
            let color = TOKENS
                .with(|t| t.borrow().as_ref().map(|t| t.syntax[index]))
                .unwrap_or_else(|| syntax_palette("default", false)[index]);
            rgb(color).into()
        },
    )
}

fn diff_palette(name: &str, light: bool) -> [u32; 4] {
    match (name, light) {
        ("colorblind", false) => [0x388bfd, 0xdb6d28, 0x79c0ff, 0xffa657],
        ("colorblind", true) => [0x0969da, 0xbc4c00, 0x0550ae, 0x953800],
        ("high-contrast", false) => [0x58a6ff, 0xf0883e, 0xcae8ff, 0xffdfb6],
        ("high-contrast", true) => [0x0550ae, 0x953800, 0x032563, 0x471700],
        (_, true) => [0x10b981, 0xf43f5e, 0x047857, 0xbe123c],
        _ => [0x10b981, 0xf43f5e, 0x6ee7b7, 0xfda4af],
    }
}
pub(crate) fn diff_foreground(add: bool) -> Hsla {
    let index = if add { 2 } else { 3 };
    let color = TOKENS
        .with(|t| t.borrow().as_ref().map(|t| t.diff[index]))
        .unwrap_or_else(|| diff_palette("default", false)[index]);
    rgb(color).into()
}
pub(crate) fn diff_background(add: bool, gutter: bool) -> Hsla {
    let index = if add { 0 } else { 1 };
    let (color, strong) = TOKENS
        .with(|t| t.borrow().as_ref().map(|t| (t.diff[index], t.diff_strong)))
        .unwrap_or((diff_palette("default", false)[index], false));
    let mut color: Hsla = rgb(color).into();
    color.a = match (gutter, strong) {
        (true, true) => 0.45,
        (true, false) => 0.25,
        (false, true) => 0.28,
        _ => 0.15,
    };
    color
}

#[cfg(test)]
mod composer_tests {
    use super::*;

    #[test]
    fn composer_surface_matches_react_dark_ink_and_solid_light_background() {
        struct Restore(Option<Tokens>);
        impl Drop for Restore {
            fn drop(&mut self) {
                TOKENS.with(|tokens| *tokens.borrow_mut() = self.0);
            }
        }
        let _restore = Restore(TOKENS.with(|tokens| *tokens.borrow()));
        let mut preferences = crate::settings::Preferences::default();
        configure(&preferences, false);
        assert_eq!(composer_fill(), content(0.03));
        assert_eq!(composer_blur(), 24.);
        assert_eq!(composer_border(false), content(0.1));
        assert_eq!(composer_border(true), content(0.2));

        preferences.theme = "light".into();
        preferences.theme_hue = 180;
        preferences.theme_saturation = 20;
        configure(&preferences, false);
        assert_eq!(composer_fill(), gpui::hsla(0.5, 0.2, 0.97, 1.));
        assert_eq!(composer_blur(), 0.);
        assert_eq!(composer_border(false), content(0.1));
        assert_eq!(composer_border(true), content(0.2));
    }
}
