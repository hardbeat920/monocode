//! Port of src/shared/ui/Shimmer.tsx (`.shimmer-text` in index.css): text at
//! 40% ink with a full-strength band sweeping across it.
//!
//! CSS paints the band with a clipped background gradient. GPUI text has no
//! background clip, so each character gets the band's color at its position.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, ElementId, HighlightStyle, Hsla, IntoElement, ParentElement as _,
    SharedString, Styled as _, StyledText, div,
};
use monocode_ui::Theme;

/// Band half-width as a share of the text width. CSS uses 2px per character
/// on each side; at about 7.5px per character that is a quarter.
const SPREAD: f32 = 0.27;

/// The ink at one point of the sweep: the band composited over 40% ink.
fn band_alpha(position: f32, center: f32) -> f32 {
    let band = (1. - (position - center).abs() / SPREAD).max(0.);
    band + 0.4 * (1. - band)
}

/// Per-character colors for one frame, merged into runs of equal ink.
fn highlights(
    text: &str,
    content: Hsla,
    progress: f32,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let count = text.chars().count().max(1) as f32;
    // `background-position` 100% to 0% over a 250% wide background moves the
    // band's center from -25% to 125% of the text width.
    let center = -0.25 + 1.5 * progress;
    let mut runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut last_alpha = -1.;
    for (index, (offset, ch)) in text.char_indices().enumerate() {
        let position = (index as f32 + 0.5) / count;
        // Quantize so a long label does not turn into one run per character.
        let alpha = (band_alpha(position, center) * 20.).round() / 20.;
        let end = offset + ch.len_utf8();
        if alpha == last_alpha
            && let Some(run) = runs.last_mut()
        {
            run.0.end = end;
            continue;
        }
        last_alpha = alpha;
        runs.push((
            offset..end,
            HighlightStyle {
                color: Some(Hsla {
                    a: content.a * alpha,
                    ..content
                }),
                ..Default::default()
            },
        ));
    }
    runs
}

/// `<Shimmer duration={…}>`: `text` swept once every `duration`.
pub fn shimmer(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    duration: Duration,
    theme: &Theme,
) -> impl IntoElement {
    let text: SharedString = text.into();
    let content = theme.colors.content;
    div()
        .min_w_0()
        .truncate()
        .text_color(theme.content(0.4))
        .with_animation(
            id,
            Animation::new(duration).repeat(),
            move |el, progress| {
                el.child(
                    StyledText::new(text.clone())
                        .with_highlights(highlights(&text, content, progress)),
                )
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_brightens_only_near_its_center() {
        assert!((band_alpha(0.5, 0.5) - 1.).abs() < 1e-6);
        assert!((band_alpha(0., 0.5) - 0.4).abs() < 1e-6);
        let runs = highlights("Working for 3s", gpui::white(), 0.5);
        assert_eq!(runs.first().unwrap().0.start, 0);
        assert_eq!(runs.last().unwrap().0.end, "Working for 3s".len());
        assert!(runs.len() > 2);
    }
}
