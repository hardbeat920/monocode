//! Small styling shortcuts for the Tailwind idioms the React views repeat.

use std::sync::Arc;

use gpui::{
    Corners, FontFeatures, FontWeight, Hsla, IntoElement, ParentElement as _, Styled, canvas, div,
    relative,
};

use crate::u;

/// Shortcuts on any styled element. Sizes are CSS px through [`u`].
pub trait UiStyled: Styled + Sized {
    /// `text-[Npx]`.
    fn text_px(self, size: f32) -> Self {
        self.text_size(u(size))
    }

    /// `leading-*` as a multiple of the font size.
    fn leading(self, multiple: f32) -> Self {
        self.line_height(relative(multiple))
    }

    /// `tabular-nums`.
    fn tabular(self) -> Self {
        self.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
    }

    /// `font-medium`.
    fn medium(self) -> Self {
        self.font_weight(FontWeight::MEDIUM)
    }

    /// `font-semibold`.
    fn semibold(self) -> Self {
        self.font_weight(FontWeight::SEMIBOLD)
    }
}

impl<T: Styled> UiStyled for T {}

/// `GlassBackdrop`: a frosted layer for a positioned, rounded frame. Put it
/// first among the frame's children. The blur samples whatever was painted
/// beneath the frame, then `tint` goes over it.
pub fn glass_backdrop(radius: f32, blur: f32, tint: Hsla) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let rem = window.rem_size();
                    window.paint_backdrop_blur(
                        bounds,
                        Corners::all(u(radius).to_pixels(rem)),
                        u(blur).to_pixels(rem),
                    );
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded(u(radius))
                .bg(tint),
        )
}

/// Formats an integer with thousands separators, like `formatInteger`.
pub fn format_integer(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_integer;

    #[test]
    fn formats_thousands() {
        assert_eq!(format_integer(0), "0");
        assert_eq!(format_integer(949), "949");
        assert_eq!(format_integer(1949), "1,949");
        assert_eq!(format_integer(1234567), "1,234,567");
        assert_eq!(format_integer(-1200), "-1,200");
    }
}
