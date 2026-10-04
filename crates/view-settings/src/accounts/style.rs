//! Colors, small drawing helpers, and test selectors the account views share.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Div, ElementId, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _, Transformation, div, percentage,
};
use monocode_core::js;
use monocode_ui::color::{hsl_to_rgb, parse_hex};
use monocode_ui::{IconName, Theme, icon, u};

use super::model::AccountStatusTone;

/// Tailwind shades the theme has no token for.
// TODO(port): move these into monocode-ui's theme once another view needs them.
pub mod palette {
    use gpui::Hsla;
    use monocode_ui::color::hex;

    /// `amber-300`: dark-mode warning ink.
    pub fn amber_300() -> Hsla {
        hex(0xffd230)
    }
    /// `amber-600`: light-mode "sign in" ink.
    pub fn amber_600() -> Hsla {
        hex(0xe17100)
    }
    /// `amber-700`: light-mode warning ink.
    pub fn amber_700() -> Hsla {
        hex(0xbb4d00)
    }
    /// `emerald-300`: dark-mode success ink.
    pub fn emerald_300() -> Hsla {
        hex(0x5ee9b5)
    }
    /// `emerald-700`: light-mode success ink.
    pub fn emerald_700() -> Hsla {
        hex(0x007a55)
    }
    /// The running terminal bars (`.terminal-live-bar`).
    pub fn terminal_live() -> Hsla {
        hex(0xe39b4a)
    }
    /// `white`, for checkbox marks and switch thumbs.
    pub fn white() -> Hsla {
        hex(0xffffff)
    }
}

/// `text-amber-700 dark:text-amber-300`.
pub fn warning_ink(cx: &App) -> Hsla {
    if Theme::of(cx).is_dark() {
        palette::amber_300()
    } else {
        palette::amber_700()
    }
}

/// `text-emerald-700 dark:text-emerald-300`.
pub fn success_ink(cx: &App) -> Hsla {
    if Theme::of(cx).is_dark() {
        palette::emerald_300()
    } else {
        palette::emerald_700()
    }
}

/// `text-red-500`.
pub fn error_ink(cx: &App) -> Hsla {
    Theme::of(cx).colors.danger_fill
}

/// `barClass`: the bar color by percent used, shared by the footer chip and
/// the account meters.
pub fn bar_color(pct: f64, cx: &App) -> Hsla {
    let theme = Theme::of(cx);
    if pct >= 90.0 {
        theme.colors.danger
    } else if pct >= 80.0 {
        theme.colors.warning
    } else {
        theme.content(0.45)
    }
}

/// `STATUS_DOT`.
pub fn status_dot_color(tone: AccountStatusTone, cx: &App) -> Hsla {
    let theme = Theme::of(cx);
    match tone {
        AccountStatusTone::Ready => theme.colors.success,
        AccountStatusTone::Low => theme.colors.warning,
        AccountStatusTone::Exhausted => theme.colors.danger,
        AccountStatusTone::Checking | AccountStatusTone::Unknown => theme.content(0.25),
    }
}

/// `STATUS_TEXT`.
pub fn status_text_color(tone: AccountStatusTone, cx: &App) -> Hsla {
    let theme = Theme::of(cx);
    let at = |color: Hsla, alpha: f32| Hsla {
        a: color.a * alpha,
        ..color
    };
    match tone {
        AccountStatusTone::Ready => theme.content(0.60),
        AccountStatusTone::Low => at(theme.colors.warning, 0.9),
        AccountStatusTone::Exhausted => at(theme.colors.danger, 0.9),
        AccountStatusTone::Checking | AccountStatusTone::Unknown => theme.content(0.35),
    }
}

/// A CSS color the project palette stores: `#rrggbb` or `hsl(h s% l%)`.
pub fn parse_css_color(value: &str) -> Option<Hsla> {
    if let Some(color) = parse_hex(value) {
        return Some(color);
    }
    let inner = value.trim().strip_prefix("hsl(")?.strip_suffix(')')?;
    let mut parts = inner
        .split_whitespace()
        .map(|part| part.trim_end_matches('%').parse::<f64>());
    let (h, s, l) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    Some(hsl_to_rgb(h, s, l).to_hsla())
}

/// A percentage the way a CSS width wrote it: `58`, `57.5`.
pub fn css_percent(value: f64) -> String {
    js::number_to_string(value)
}

/// A text run with the test selector `text:<text>`.
pub fn text(content: impl Into<SharedString>) -> Div {
    let content: SharedString = content.into();
    let selector = content.clone();
    div()
        .debug_selector(move || format!("text:{selector}"))
        .child(content)
}

/// An icon that spins once a second (`animate-spin`).
pub fn spin_icon(id: impl Into<ElementId>, name: IconName, size: f32, color: Hsla) -> AnyElement {
    icon(name)
        .size(u(size))
        .text_color(color)
        .with_animation(
            id,
            Animation::new(Duration::from_secs(1)).repeat(),
            |svg, t| svg.with_transformation(Transformation::rotate(percentage(t))),
        )
        .into_any_element()
}

/// The hover fill of a `-mx-1 px-1` control: it reaches `x` CSS px past the
/// content on each side (and `y` above and below) without moving the
/// layout. Taffy sizes a flex item with negative margins to zero, so the
/// footer draws the overhang as an absolute layer. The control sets
/// `.group(group)`.
pub fn hover_halo(group: &'static str, x: f32, y: f32, radius: f32, fill: Hsla) -> Div {
    div()
        .absolute()
        .top(u(-y))
        .bottom(u(-y))
        .left(u(-x))
        .right(u(-x))
        .rounded(u(radius))
        .group_hover(group, move |s| s.bg(fill))
}

/// `animate-pulse`: opacity 1 to 0.5 and back over two seconds.
pub fn pulse(id: impl Into<ElementId>, element: Div) -> AnyElement {
    element
        .with_animation(
            id,
            Animation::new(Duration::from_secs(2)).repeat(),
            |el, t| {
                // `cubic-bezier(0.4, 0, 0.6, 1)` on 1 → 0.5 → 1.
                let phase = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
                el.opacity(1.0 - 0.5 * phase)
            },
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_palette_colors() {
        assert!(parse_css_color("#3b82f6").is_some());
        assert!(parse_css_color("hsl(211 92% 62%)").is_some());
        assert!(parse_css_color("blue").is_none());
        assert_eq!(css_percent(58.0), "58");
        assert_eq!(css_percent(57.5), "57.5");
    }
}
