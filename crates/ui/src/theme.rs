//! Port of the design tokens in src/styles/index.css (the `@theme` and `:root`
//! blocks, the light overrides, the glass surfaces, primary actions, and the
//! motion keyframes) and src/shared/lib/layers.ts.
//!
//! [`Theme`] is a GPUI global. Views read it with `Theme::of(cx)` and never
//! hard-code colors. [`set_appearance`] recomputes it and maps it onto
//! gpui-component's theme so that crate's widgets match.

use std::time::Duration;

use gpui::{
    App, Global, Hsla, Pixels, SharedString, Window, WindowAppearance, WindowBackgroundAppearance,
    px,
};

use crate::appearance::{AppearanceSettings, ColorScheme};
use crate::color::{hex, hsl_to_rgb, mix, parse_hex, with_alpha};

/// Every color token. Alpha variants that the React code writes inline
/// (`text-content/50`, `bg-accent/15`) come from [`Theme::content`] and
/// [`Theme::accent`].
#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    /// `--color-background-base`: hsl(hue sat background-lightness).
    pub background_base: Hsla,
    /// `--color-content`: hsl(hue sat content-lightness).
    pub content: Hsla,
    /// `--color-stroke`: content at 7%, for structural separators.
    pub stroke: Hsla,
    pub selection_subtle: Hsla,
    pub selection: Hsla,
    pub selection_strong: Hsla,
    pub selection_hover: Hsla,
    pub selection_emphasis: Hsla,
    /// `--color-accent`: hsl(211 92% 62%). The user accent does not replace it.
    pub accent: Hsla,
    /// `--user-accent-color`, set when the user picked an accent.
    pub user_accent: Option<Hsla>,
    /// `--user-accent-foreground`: black or white, whichever reads on the accent.
    pub user_accent_foreground: Hsla,
    pub skill: Hsla,
    pub mention: Hsla,
    pub link: Hsla,
    pub markdown_heading: Hsla,

    /// Additions, done checks, online dots (Tailwind emerald-400).
    pub success: Hsla,
    /// Deletions and errors (red-400).
    pub danger: Hsla,
    /// Destructive menu items (red-300 text over red-500 fills).
    pub danger_soft: Hsla,
    pub danger_fill: Hsla,
    /// Approvals and attention (amber-400).
    pub warning: Hsla,
    /// A finished response in a tab (teal-400).
    pub done: Hsla,
    /// Orchestration marks (fuchsia-300).
    pub orchestration: Hsla,

    /// The app root: `bg-background-base/40` over native glass, else opaque.
    pub root_background: Hsla,
    /// `.sidebar-glass`: the project rail and compact rail.
    pub sidebar_glass: Hsla,
    /// `.body-glass`: the main pane.
    pub body_glass: Hsla,
    /// `.body-glass.sidebar-pane`: the session sidebar.
    pub sidebar_pane: Hsla,
    /// `.popover-backdrop`: the tint over the popover blur.
    pub popover_backdrop: Hsla,
    /// The modal panel's glass tint (`bg-background-base/55`).
    pub modal_backdrop: Hsla,
    /// The dim layer behind a modal (`bg-black/40`).
    pub modal_overlay: Hsla,
    /// Popover and toast borders (`border-content/10`).
    pub popover_border: Hsla,
    /// Modal border (`border-content/7`).
    pub modal_border: Hsla,

    /// `.primary-action` fills and ink, including the user accent variants.
    pub primary: Hsla,
    pub primary_hover: Hsla,
    pub primary_disabled: Hsla,
    pub primary_foreground: Hsla,
    pub primary_disabled_foreground: Hsla,

    /// The non-macOS scrollbar thumb (`content` at 16%, 32% on hover).
    pub scrollbar_thumb: Hsla,
    pub scrollbar_thumb_hover: Hsla,
}

/// Corner radii (Tailwind `rounded-*` at the app's sizes).
#[derive(Clone, Copy, Debug)]
pub struct Radii {
    /// `rounded`: 4px.
    pub sm: f32,
    /// `rounded-md`: 6px. Buttons, cards, rows.
    pub md: f32,
    /// `rounded-lg`: 8px. Menu items, composer.
    pub lg: f32,
    /// `rounded-xl`: 12px. Popovers, menus, toasts.
    pub xl: f32,
    /// `rounded-2xl`: 16px. Modals.
    pub xxl: f32,
    pub full: f32,
}

/// Font sizes in CSS px at interface scale 1. Pass them through [`crate::u`].
#[derive(Clone, Copy, Debug)]
pub struct TypeScale {
    /// `text-[10px]`: tab meta lines, badges.
    pub micro: f32,
    /// `text-[11px]`: card meta, footer, shortcuts.
    pub caption: f32,
    /// `text-[12px]`: sidebar tabs, secondary buttons, search fields.
    pub label: f32,
    /// `text-[13px]`: session titles, tab titles, menu items.
    pub body: f32,
    /// `text-sm`: 14px. Rail rows, project names.
    pub ui: f32,
    /// `text-xl`: 20px. Modal titles.
    pub title: f32,
}

/// Line heights as multiples of the font size.
#[derive(Clone, Copy, Debug)]
pub struct LineHeights {
    /// The body default from Tailwind's preflight (`line-height: 1.5`). Set it
    /// on each window's root view; GPUI's own default is taller.
    pub normal: f32,
    /// `leading-none`.
    pub none: f32,
    /// `leading-tight`.
    pub tight: f32,
    /// `leading-snug`.
    pub snug: f32,
    /// `--leading-label`: keeps descenders in compact labels.
    pub label: f32,
    /// `leading-relaxed`.
    pub relaxed: f32,
}

/// Paint order for floating layers, from src/shared/lib/layers.ts. Use them
/// as `deferred(...).with_priority(layer)`.
#[derive(Clone, Copy, Debug)]
pub struct Layers {
    pub popover: usize,
    pub submenu: usize,
    pub dialog: usize,
    pub dialog_popover: usize,
    pub toast: usize,
}

/// A cubic-bezier timing curve, as CSS writes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier(pub f32, pub f32, pub f32, pub f32);

impl CubicBezier {
    /// The eased progress for linear progress `t` in 0..=1.
    pub fn ease(self, t: f32) -> f32 {
        let CubicBezier(x1, y1, x2, y2) = self;
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        let bezier = |a: f32, b: f32, s: f32| {
            let inv = 1.0 - s;
            3.0 * inv * inv * s * a + 3.0 * inv * s * s * b + s * s * s
        };
        let slope = |a: f32, b: f32, s: f32| {
            let inv = 1.0 - s;
            3.0 * inv * inv * a + 6.0 * inv * s * (b - a) + 3.0 * s * s * (1.0 - b)
        };
        // Newton steps on x(s) = t, then bisection if the slope is flat.
        let mut s = t;
        for _ in 0..8 {
            let error = bezier(x1, x2, s) - t;
            if error.abs() < 1e-5 {
                return bezier(y1, y2, s);
            }
            let d = slope(x1, x2, s);
            if d.abs() < 1e-6 {
                break;
            }
            s = (s - error / d).clamp(0.0, 1.0);
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        s = t;
        for _ in 0..30 {
            let x = bezier(x1, x2, s);
            if (x - t).abs() < 1e-5 {
                break;
            }
            if x < t {
                lo = s;
            } else {
                hi = s;
            }
            s = (lo + hi) / 2.0;
        }
        bezier(y1, y2, s)
    }

    /// The curve as an easing function for `gpui::Animation::with_easing`.
    pub fn easing(self) -> impl Fn(f32) -> f32 + 'static {
        move |t| self.ease(t)
    }
}

/// Durations and curves from `:root` and the keyframe rules in index.css.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// `--motion-reorder-duration`.
    pub reorder: Duration,
    /// `--motion-tab-close-duration`.
    pub tab_close: Duration,
    /// `--motion-feedback-duration`: hover and color transitions.
    pub feedback: Duration,
    /// `--motion-ease-out`.
    pub ease_out: CubicBezier,
    /// `--motion-tab-ease-out`.
    pub tab_ease_out: CubicBezier,
    /// `.popover-open`: 170ms, scale 0.94 and an 8px lift.
    pub popover_open: Duration,
    pub popover_ease: CubicBezier,
    pub popover_lift: f32,
    pub popover_scale: f32,
    /// `.modal-backdrop`: 160ms ease-out fade.
    pub modal_backdrop: Duration,
    /// `.modal-panel`: 200ms, 8px rise and scale 0.98.
    pub modal_panel: Duration,
    pub modal_ease: CubicBezier,
    pub modal_rise: f32,
    /// `.approval-toast`: 180ms.
    pub toast_in: Duration,
}

impl Motion {
    /// CSS `ease-out`.
    pub const EASE_OUT: CubicBezier = CubicBezier(0.0, 0.0, 0.58, 1.0);
}

/// Font families. `system-ui` and `ui-monospace` resolve to the platform
/// faces.
#[derive(Clone, Debug)]
pub struct Fonts {
    pub sans: SharedString,
    pub mono: SharedString,
}

/// Fixed chrome sizes in CSS px, from the React shell.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// `h-10` title bars and rail headers.
    pub title_bar_height: f32,
    /// `h-9` sidebar tab and search rows.
    pub toolbar_height: f32,
    /// `h-7` usage footer.
    pub footer_height: f32,
    /// The macOS spacer left of the first control (`w-[78px]`).
    pub traffic_light_inset: f32,
    /// Where AppKit draws the traffic lights: 12px in, centered in 40px.
    pub traffic_light_x: f32,
    pub traffic_light_y: f32,
    /// `w-12` compact project rail.
    pub compact_rail_width: f32,
    /// Hit width of a resize handle (`w-1.5`).
    pub resize_handle_width: f32,
}

/// The theme global.
#[derive(Clone, Debug)]
pub struct Theme {
    pub appearance: AppearanceSettings,
    /// The OS appearance, used when the preference is "system".
    pub system_scheme: ColorScheme,
    pub scheme: ColorScheme,
    /// `has-native-glass`: the window is transparent and blurred.
    pub native_glass: bool,
    pub colors: ThemeColors,
    pub radius: Radii,
    pub text: TypeScale,
    pub leading: LineHeights,
    pub layer: Layers,
    pub motion: Motion,
    pub fonts: Fonts,
    pub metrics: Metrics,
}

impl Global for Theme {}

const LAYERS: Layers = Layers {
    popover: 80,
    submenu: 81,
    dialog: 90,
    dialog_popover: 91,
    toast: 100,
};

const MOTION: Motion = Motion {
    reorder: Duration::from_millis(160),
    tab_close: Duration::from_millis(200),
    feedback: Duration::from_millis(120),
    ease_out: CubicBezier(0.22, 1.0, 0.36, 1.0),
    tab_ease_out: CubicBezier(0.3333, 0.6667, 0.6667, 1.0),
    popover_open: Duration::from_millis(170),
    popover_ease: CubicBezier(0.16, 1.0, 0.3, 1.0),
    popover_lift: 8.0,
    popover_scale: 0.94,
    modal_backdrop: Duration::from_millis(160),
    modal_panel: Duration::from_millis(200),
    modal_ease: CubicBezier(0.16, 1.0, 0.3, 1.0),
    modal_rise: 8.0,
    toast_in: Duration::from_millis(180),
};

const RADII: Radii = Radii {
    sm: 4.0,
    md: 6.0,
    lg: 8.0,
    xl: 12.0,
    xxl: 16.0,
    full: 9999.0,
};

const TYPE_SCALE: TypeScale = TypeScale {
    micro: 10.0,
    caption: 11.0,
    label: 12.0,
    body: 13.0,
    ui: 14.0,
    title: 20.0,
};

const LINE_HEIGHTS: LineHeights = LineHeights {
    normal: 1.5,
    none: 1.0,
    tight: 1.25,
    snug: 1.375,
    label: 1.4,
    relaxed: 1.625,
};

const METRICS: Metrics = Metrics {
    title_bar_height: 40.0,
    toolbar_height: 36.0,
    footer_height: 28.0,
    traffic_light_inset: 78.0,
    traffic_light_x: 12.0,
    traffic_light_y: 13.0,
    compact_rail_width: 48.0,
    resize_handle_width: 6.0,
};

/// Per-scheme values from `:root` and `html.theme-light`.
struct SchemeTokens {
    background_lightness: f64,
    content_lightness: f64,
    link: Hsla,
    skill: Hsla,
    mention: Hsla,
    markdown_heading: Hsla,
    selection_subtle: f32,
    selection: f32,
    selection_strong: f32,
    selection_hover: f32,
    selection_emphasis: f32,
}

fn scheme_tokens(scheme: ColorScheme, dark_lightness: f64) -> SchemeTokens {
    match scheme {
        ColorScheme::Dark => SchemeTokens {
            background_lightness: dark_lightness,
            content_lightness: 92.0,
            link: hex(0x7dd3fc),
            skill: hex(0xe8c547),
            mention: hex(0x38bdf8),
            markdown_heading: hex(0xf9a8c9),
            selection_subtle: 0.08,
            selection: 0.10,
            selection_strong: 0.12,
            selection_hover: 0.15,
            selection_emphasis: 0.20,
        },
        ColorScheme::Light => SchemeTokens {
            background_lightness: 97.0,
            content_lightness: 18.0,
            link: hsl(211.0, 92.0, 40.0),
            skill: hex(0xa07c10),
            mention: hex(0x0284c7),
            markdown_heading: hex(0xbe185d),
            selection_subtle: 0.05,
            selection: 0.06,
            selection_strong: 0.07,
            selection_hover: 0.10,
            selection_emphasis: 0.14,
        },
    }
}

/// `hsl(h s% l%)`, rounded to 8-bit channels the way the browser paints it.
fn hsl(hue: f64, saturation: f64, lightness: f64) -> Hsla {
    hsl_to_rgb(hue, saturation, lightness).to_hsla()
}

/// Picks the first installed monospace face for `ui-monospace`.
fn resolve_mono_font(cx: &App) -> SharedString {
    let names = cx.text_system().all_font_names();
    for candidate in [
        "SF Mono",
        "SFMono-Regular",
        "Menlo",
        "Monaco",
        "Consolas",
        "Liberation Mono",
    ] {
        if names.iter().any(|name| name == candidate) {
            return candidate.into();
        }
    }
    if cfg!(target_os = "macos") {
        "Menlo".into()
    } else if cfg!(target_os = "windows") {
        "Consolas".into()
    } else {
        "DejaVu Sans Mono".into()
    }
}

impl Theme {
    /// Computes every token for `appearance` under the given OS appearance.
    pub fn new(appearance: AppearanceSettings, system_scheme: ColorScheme, fonts: Fonts) -> Self {
        let appearance = appearance.normalized();
        let scheme = appearance.color_scheme(system_scheme);
        let native_glass = appearance.native_glass(scheme);
        let tokens = scheme_tokens(scheme, appearance.theme_dark_lightness);
        let hue = appearance.theme_hue;
        let sat = appearance.theme_saturation;
        let base = hsl(hue, sat, tokens.background_lightness);
        let content = hsl(hue, sat, tokens.content_lightness);
        let ink = |alpha: f32| with_alpha(content, alpha);
        let white = hex(0xffffff);
        let black = hex(0x000000);
        let user_accent = appearance.accent_color.as_deref().and_then(parse_hex);
        let user_accent_foreground = appearance
            .accent_color
            .as_deref()
            .map(crate::color::accent_foreground)
            .and_then(parse_hex)
            .unwrap_or(white);
        let dark = scheme == ColorScheme::Dark;

        let (
            primary,
            primary_hover,
            primary_disabled,
            primary_foreground,
            primary_disabled_foreground,
        ) = match (user_accent, dark) {
            (Some(accent), _) => (
                accent,
                mix(accent, content, 0.88),
                with_alpha(accent, 0.28),
                user_accent_foreground,
                ink(0.35),
            ),
            (None, true) => (
                white,
                with_alpha(white, 0.9),
                with_alpha(white, 0.3),
                black,
                with_alpha(black, 0.4),
            ),
            (None, false) => (content, ink(0.9), ink(0.25), base, with_alpha(base, 0.75)),
        };

        let sidebar_glass = if !dark {
            base
        } else if native_glass {
            with_alpha(base, appearance.sidebar_opacity)
        } else {
            mix(base, black, 0.9)
        };
        let body_glass = if native_glass && appearance.body_glass {
            with_alpha(base, appearance.main_opacity)
        } else {
            base
        };
        let sidebar_pane = if native_glass {
            with_alpha(base, appearance.sidebar_opacity)
        } else {
            base
        };

        let colors = ThemeColors {
            background_base: base,
            content,
            stroke: ink(0.07),
            selection_subtle: ink(tokens.selection_subtle),
            selection: ink(tokens.selection),
            selection_strong: ink(tokens.selection_strong),
            selection_hover: ink(tokens.selection_hover),
            selection_emphasis: ink(tokens.selection_emphasis),
            accent: hsl(211.0, 92.0, 62.0),
            user_accent,
            user_accent_foreground,
            skill: tokens.skill,
            mention: tokens.mention,
            link: tokens.link,
            markdown_heading: tokens.markdown_heading,
            success: hex(0x00d492),
            danger: hex(0xff6467),
            danger_soft: hex(0xffa2a2),
            danger_fill: hex(0xfb2c36),
            warning: hex(0xffb900),
            done: hex(0x00d5be),
            orchestration: hex(0xf4a8ff),
            root_background: if native_glass {
                with_alpha(base, 0.4)
            } else {
                base
            },
            sidebar_glass,
            body_glass,
            sidebar_pane,
            popover_backdrop: if dark { ink(0.02) } else { base },
            modal_backdrop: with_alpha(base, 0.55),
            modal_overlay: with_alpha(black, 0.4),
            popover_border: ink(0.10),
            modal_border: ink(0.07),
            primary,
            primary_hover,
            primary_disabled,
            primary_foreground,
            primary_disabled_foreground,
            scrollbar_thumb: ink(0.16),
            scrollbar_thumb_hover: ink(0.32),
        };

        Self {
            appearance,
            system_scheme,
            scheme,
            native_glass,
            colors,
            radius: RADII,
            text: TYPE_SCALE,
            leading: LINE_HEIGHTS,
            layer: LAYERS,
            motion: MOTION,
            fonts,
            metrics: METRICS,
        }
    }

    /// The theme global.
    #[inline]
    pub fn of(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    pub fn is_dark(&self) -> bool {
        self.scheme == ColorScheme::Dark
    }

    /// `content` at `alpha`, the `text-content/50` and `bg-content/5` idiom.
    #[inline]
    pub fn content(&self, alpha: f32) -> Hsla {
        with_alpha(self.colors.content, alpha)
    }

    /// `accent` at `alpha`, the `bg-accent/15` idiom.
    #[inline]
    pub fn accent(&self, alpha: f32) -> Hsla {
        with_alpha(self.colors.accent, alpha)
    }

    /// The user accent if set, else the default accent
    /// (`var(--user-accent-color, var(--color-accent))`).
    pub fn user_accent_or_accent(&self) -> Hsla {
        self.colors.user_accent.unwrap_or(self.colors.accent)
    }

    /// Interface scale (`monocode.uiScale`).
    pub fn ui_scale(&self) -> f32 {
        self.appearance.ui_scale
    }

    /// The rem size that makes [`crate::u`] lengths follow the interface scale.
    pub fn rem_size(&self) -> Pixels {
        px(16.0 * self.appearance.ui_scale)
    }

    /// Window background blur radius in points (`monocode.sidebarBlur`).
    ///
    /// The zui backend fixes its own radius, so this is not applied yet.
    /// `monocode-platform` owns the native call.
    pub fn blur_radius(&self) -> f32 {
        self.appearance.sidebar_blur
    }

    /// How the window composites behind our paint. Re-apply after every
    /// theme change: the macOS backend drops its blur view whenever the
    /// value is anything other than `Blurred`.
    pub fn window_background(&self) -> WindowBackgroundAppearance {
        if self.native_glass {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        }
    }
}

fn scheme_of(appearance: WindowAppearance) -> ColorScheme {
    match appearance {
        WindowAppearance::Light | WindowAppearance::VibrantLight => ColorScheme::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => ColorScheme::Dark,
    }
}

/// Installs the theme global and the gpui-component theme. Call once, after
/// `gpui_component::init`.
pub fn init(appearance: AppearanceSettings, cx: &mut App) {
    let fonts = Fonts {
        sans: ".SystemUIFont".into(),
        mono: resolve_mono_font(cx),
    };
    let theme = Theme::new(appearance, scheme_of(cx.window_appearance()), fonts);
    cx.set_global(theme);
    apply_component_theme(cx);
}

/// Recomputes the theme from new appearance settings, re-applies each
/// window's glass and rem size, and refreshes windows.
pub fn set_appearance(appearance: AppearanceSettings, cx: &mut App) {
    let current = Theme::of(cx);
    let theme = Theme::new(appearance, current.system_scheme, current.fonts.clone());
    cx.set_global(theme);
    apply_component_theme(cx);
    sync_all_windows(cx);
    cx.refresh_windows();
}

/// Runs [`sync_window`] on every open window. A window that is mid-update
/// (the caller's own) is skipped; sync it directly.
fn sync_all_windows(cx: &mut App) {
    for handle in cx.windows() {
        let _ = handle.update(cx, |_, window, cx| sync_window(window, cx));
    }
}

/// Follows an OS appearance change, for the "system" preference.
pub fn set_system_scheme(appearance: WindowAppearance, cx: &mut App) {
    let current = Theme::of(cx);
    let scheme = scheme_of(appearance);
    if current.system_scheme == scheme {
        return;
    }
    let theme = Theme::new(current.appearance.clone(), scheme, current.fonts.clone());
    cx.set_global(theme);
    apply_component_theme(cx);
    sync_all_windows(cx);
    cx.refresh_windows();
}

/// Applies the window-level parts of the theme: glass and rem size. Call it
/// when a window opens and after each theme change.
pub fn sync_window(window: &mut Window, cx: &mut App) {
    let theme = Theme::of(cx);
    window.set_background_appearance(theme.window_background());
    window.set_rem_size(theme.rem_size());
}

/// Maps the theme onto gpui-component's legacy theme and gpui-base's
/// semantic theme, so their inputs, scrollbars, and lists match ours.
fn apply_component_theme(cx: &mut App) {
    use gpui_component::{Theme as ComponentTheme, ThemeMode, ThemeTokens};

    let theme = Theme::of(cx).clone();
    let mode = if theme.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    ComponentTheme::change(mode, None, cx);

    let c = theme.colors;
    let ink = |alpha: f32| theme.content(alpha);
    let component = ComponentTheme::global_mut(cx);
    {
        let k = &mut component.colors;
        k.background = c.background_base;
        k.foreground = c.content;
        k.border = c.stroke;
        k.input = ink(0.10);
        k.ring = c.accent;
        k.caret = c.content;
        k.selection = theme.accent(0.35);
        k.muted = ink(0.05);
        k.muted_foreground = ink(0.50);
        k.accent = c.selection;
        k.accent_foreground = c.content;
        k.primary = c.primary;
        k.primary_hover = c.primary_hover;
        k.primary_active = c.primary_hover;
        k.primary_foreground = c.primary_foreground;
        k.secondary = ink(0.10);
        k.secondary_hover = ink(0.15);
        k.secondary_active = ink(0.20);
        k.secondary_foreground = c.content;
        k.button_primary = c.primary;
        k.button_primary_hover = c.primary_hover;
        k.button_primary_active = c.primary_hover;
        k.button_primary_foreground = c.primary_foreground;
        k.popover = c.background_base;
        k.popover_foreground = c.content;
        k.overlay = c.modal_overlay;
        k.link = c.link;
        k.link_hover = c.link;
        k.link_active = c.link;
        k.danger = c.danger;
        k.danger_hover = c.danger;
        k.danger_active = c.danger;
        k.success = c.success;
        k.warning = c.warning;
        k.info = c.mention;
        k.list = gpui::transparent_black();
        k.list_hover = ink(0.05);
        k.list_active = theme.accent(0.15);
        k.list_active_border = gpui::transparent_black();
        k.scrollbar = gpui::transparent_black();
        k.scrollbar_thumb = c.scrollbar_thumb;
        k.scrollbar_thumb_hover = c.scrollbar_thumb_hover;
        k.switch = ink(0.20);
        k.switch_thumb = hex(0xffffff);
        k.slider_bar = c.accent;
        k.slider_thumb = hex(0xffffff);
        k.tab_bar = gpui::transparent_black();
        k.tab = gpui::transparent_black();
        k.tab_active = c.selection;
        k.tab_foreground = ink(0.50);
        k.tab_active_foreground = c.content;
        k.title_bar = gpui::transparent_black();
        k.title_bar_border = c.stroke;
        k.sidebar = c.sidebar_glass;
        k.sidebar_border = c.stroke;
        k.sidebar_foreground = c.content;
        k.sidebar_accent = c.selection;
        k.sidebar_accent_foreground = c.content;
        k.drag_border = theme.accent(0.6);
        k.drop_target = theme.accent(0.2);
        k.window_border = c.stroke;
    }
    component.tokens = ThemeTokens::from(&component.colors);
    component.font_family = theme.fonts.sans.clone();
    component.mono_font_family = theme.fonts.mono.clone();
    component.font_size = theme.rem_size();
    component.mono_font_size = px(13.0 * theme.ui_scale());
    component.radius = px(theme.radius.md);
    component.radius_lg = px(theme.radius.xl);
    component.shadow = true;
    component.focus_ring = false;
    component.transparent = gpui::transparent_black();

    let base_theme = gpui_base::Theme {
        tokens: component.semantic_tokens(),
        scrollbar: gpui_base::ScrollbarTheme {
            mode: component.scrollbar_mode,
            styles: gpui_base::ScrollbarStyles::default()
                .track(|style| style.bg(component.scrollbar))
                .track_hover(|style| style.bg(component.scrollbar))
                .track_active(|style| style.bg(component.scrollbar).border_color(component.border))
                .thumb(|style| {
                    style
                        .bg(component.tokens.scrollbar_thumb)
                        .radius(component.radius)
                })
                .thumb_hover(|style| {
                    style
                        .bg(component.tokens.scrollbar_thumb_hover)
                        .radius(component.radius)
                })
                .thumb_active(|style| {
                    style
                        .bg(component.tokens.scrollbar_thumb_hover)
                        .radius(component.radius)
                }),
        },
        resizable: gpui_base::ResizableTheme {
            handle: component.border,
            active_handle: component.drag_border,
        },
    };
    cx.set_global(base_theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts {
            sans: ".SystemUIFont".into(),
            mono: "Menlo".into(),
        }
    }

    fn rgb8(color: Hsla) -> (u8, u8, u8, u8) {
        let rgb = color.to_rgb();
        let byte = |v: f32| (v * 255.0).round() as u8;
        (byte(rgb.r), byte(rgb.g), byte(rgb.b), byte(rgb.a))
    }

    #[test]
    fn default_dark_theme_matches_the_css_tokens() {
        let theme = Theme::new(AppearanceSettings::default(), ColorScheme::Dark, fonts());
        assert_eq!(rgb8(theme.colors.background_base), (23, 23, 23, 255));
        assert_eq!(rgb8(theme.colors.content), (235, 235, 235, 255));
        assert_eq!(rgb8(theme.colors.stroke).3, 18);
        assert_eq!(rgb8(theme.colors.selection).3, 26);
    }

    #[test]
    fn light_theme_uses_the_light_overrides() {
        let appearance = AppearanceSettings {
            theme_preference: crate::appearance::ThemePreference::Light,
            ..Default::default()
        };
        let theme = Theme::new(appearance, ColorScheme::Dark, fonts());
        assert_eq!(theme.scheme, ColorScheme::Light);
        assert!(!theme.native_glass);
        assert_eq!(rgb8(theme.colors.background_base), (247, 247, 247, 255));
        assert_eq!(rgb8(theme.colors.sidebar_glass), (247, 247, 247, 255));
        assert_eq!(
            theme.window_background(),
            WindowBackgroundAppearance::Opaque
        );
    }

    #[test]
    fn glass_surfaces_follow_the_opacity_settings() {
        if cfg!(target_os = "linux") {
            return;
        }
        let appearance = AppearanceSettings {
            sidebar_opacity: 0.5,
            main_opacity: 0.25,
            ..Default::default()
        };
        let theme = Theme::new(appearance, ColorScheme::Dark, fonts());
        assert!(theme.native_glass);
        assert!((theme.colors.sidebar_glass.a - 0.5).abs() < 1e-6);
        assert!((theme.colors.sidebar_pane.a - 0.5).abs() < 1e-6);
        assert!((theme.colors.body_glass.a - 0.25).abs() < 1e-6);
        assert!((theme.colors.root_background.a - 0.4).abs() < 1e-6);
        assert_eq!(
            theme.window_background(),
            WindowBackgroundAppearance::Blurred
        );
    }

    #[test]
    fn user_accent_drives_primary_actions() {
        let appearance = AppearanceSettings {
            accent_color: Some("#ff0000".into()),
            ..Default::default()
        };
        let theme = Theme::new(appearance, ColorScheme::Dark, fonts());
        assert_eq!(rgb8(theme.colors.primary), (255, 0, 0, 255));
        assert_eq!(rgb8(theme.colors.primary_foreground), (0, 0, 0, 255));
        // The default accent is not replaced.
        assert_ne!(rgb8(theme.colors.accent), (255, 0, 0, 255));
    }

    #[test]
    fn hue_and_saturation_tint_the_base() {
        let appearance = AppearanceSettings {
            theme_hue: 0.0,
            theme_saturation: 100.0,
            ..Default::default()
        };
        let theme = Theme::new(appearance, ColorScheme::Dark, fonts());
        let (r, g, b, _) = rgb8(theme.colors.background_base);
        assert!(r > g && g == b);
    }

    #[test]
    fn cubic_bezier_hits_the_ends_and_eases_out() {
        let curve = MOTION.ease_out;
        assert_eq!(curve.ease(0.0), 0.0);
        assert_eq!(curve.ease(1.0), 1.0);
        assert!(curve.ease(0.5) > 0.8);
        let linear = CubicBezier(0.0, 0.0, 1.0, 1.0);
        assert!((linear.ease(0.3) - 0.3).abs() < 1e-3);
    }
}
