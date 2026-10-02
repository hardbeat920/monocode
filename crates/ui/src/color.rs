//! Color helpers for the theme. Port of the parts of src/shared/lib/colorUtils.ts
//! that the theme uses, plus the `color-mix(in srgb, ...)` arithmetic from
//! src/styles/index.css.

use gpui::{Hsla, Rgba};

/// An 8-bit sRGB color, as `colorUtils.ts` returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn to_hsla(self) -> Hsla {
        rgba8(self.r, self.g, self.b, 1.0)
    }
}

/// Hue in degrees, saturation and lightness in percent, as the theme uses them.
pub fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> Rgb {
    let sat = saturation.clamp(0.0, 100.0) / 100.0;
    let light = lightness.clamp(0.0, 100.0) / 100.0;
    let h = hue.rem_euclid(360.0);
    let c = (1.0 - (2.0 * light - 1.0).abs()) * sat;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = light - c / 2.0;
    let (r, g, b) = match (h / 60.0).floor() as i64 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let channel = |value: f64| ((value + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb {
        r: channel(r),
        g: channel(g),
        b: channel(b),
    }
}

/// `^#[0-9a-fA-F]{6}$`, the only accent format the settings accept.
pub fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Parses `#rrggbb` into a color. Returns `None` for anything else.
pub fn parse_hex(value: &str) -> Option<Hsla> {
    if !is_hex_color(value) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).ok();
    Some(rgba8(byte(1)?, byte(3)?, byte(5)?, 1.0))
}

/// A color from 8-bit channels and an alpha in 0..=1.
pub fn rgba8(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
    .into()
}

/// A color from a `0xrrggbb` literal.
pub fn hex(value: u32) -> Hsla {
    rgba8(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
        1.0,
    )
}

/// `color-mix(in srgb, color pct%, transparent)`: the same color at `alpha`.
pub fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla {
        a: color.a * alpha,
        ..color
    }
}

/// `color-mix(in srgb, a weight%, b)` for opaque or translucent inputs.
/// Channels mix premultiplied, as CSS Color 4 specifies.
pub fn mix(a: Hsla, b: Hsla, weight: f32) -> Hsla {
    let a = a.to_rgb();
    let b = b.to_rgb();
    let w = weight.clamp(0.0, 1.0);
    let alpha = a.a * w + b.a * (1.0 - w);
    if alpha <= 0.0 {
        return Hsla::transparent_black();
    }
    let channel = |x: f32, y: f32| (x * a.a * w + y * b.a * (1.0 - w)) / alpha;
    Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
    .into()
}

/// The black or white text color that reads on `color`, by WCAG luminance.
/// Port of `accentForeground` in appearance.ts.
pub fn accent_foreground(color: &str) -> &'static str {
    let channel = |offset: usize| {
        let value = u8::from_str_radix(&color[offset..offset + 2], 16).unwrap_or(0) as f64 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
    if luminance > 0.179 {
        "#000000"
    } else {
        "#ffffff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    #[test]
    fn matches_the_theme_backgrounds_the_native_window_is_filled_with() {
        assert_eq!(hsl_to_rgb(240.0, 0.0, 9.0), rgb(23, 23, 23));
        assert_eq!(hsl_to_rgb(240.0, 0.0, 97.0), rgb(247, 247, 247));
    }

    #[test]
    fn tints_toward_the_accent_hue() {
        assert_eq!(hsl_to_rgb(0.0, 100.0, 50.0), rgb(255, 0, 0));
        assert_eq!(hsl_to_rgb(120.0, 100.0, 50.0), rgb(0, 255, 0));
        assert_eq!(hsl_to_rgb(240.0, 100.0, 50.0), rgb(0, 0, 255));
    }

    #[test]
    fn wraps_hue_and_clamps_the_ends() {
        assert_eq!(
            hsl_to_rgb(-120.0, 100.0, 50.0),
            hsl_to_rgb(240.0, 100.0, 50.0)
        );
        assert_eq!(
            hsl_to_rgb(600.0, 100.0, 50.0),
            hsl_to_rgb(240.0, 100.0, 50.0)
        );
        assert_eq!(hsl_to_rgb(240.0, 0.0, -20.0), rgb(0, 0, 0));
        assert_eq!(hsl_to_rgb(240.0, 0.0, 120.0), rgb(255, 255, 255));
    }

    #[test]
    fn hex_colors_are_validated() {
        assert!(is_hex_color("#aabbcc"));
        assert!(is_hex_color("#AABBCC"));
        assert!(!is_hex_color("tomato"));
        assert!(!is_hex_color("#abc"));
        assert!(parse_hex("#ff0000").is_some());
        assert!(parse_hex("red").is_none());
    }

    #[test]
    fn accent_foreground_picks_readable_ink() {
        assert_eq!(accent_foreground("#ffffff"), "#000000");
        assert_eq!(accent_foreground("#000000"), "#ffffff");
        assert_eq!(accent_foreground("#1e40af"), "#ffffff");
        assert_eq!(accent_foreground("#facc15"), "#000000");
    }

    #[test]
    fn mixing_with_black_darkens() {
        let white = hex(0xffffff);
        let black = hex(0x000000);
        let mixed = mix(white, black, 0.9).to_rgb();
        assert!((mixed.r - 0.9).abs() < 1e-4);
        assert!((mixed.a - 1.0).abs() < 1e-4);
    }
}
