//! Port of src/shared/ui/Shimmer.tsx and the `.shimmer-text` rule in
//! index.css: text at `content/40` with a brighter band sweeping left to
//! right, `duration` seconds per pass.
//!
//! CSS clips a 250%-wide gradient to the glyphs. GPUI cannot clip a
//! background to text, so each character takes the band's color at its
//! position instead. The band's half-width is `spread` px per character, as
//! `--spread` is, converted to characters with an average glyph width.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, ElementId, Hsla, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StyledText, TextRun, Window, div,
};
use monocode_ui::Theme;

/// Average glyph width in CSS px for the 13 and 14 px labels that shimmer.
const AVERAGE_GLYPH: f32 = 7.0;

/// The highlight strength (0 to 1) for character `index` of `len` at
/// animation progress `t`.
pub fn shimmer_strength(index: usize, len: usize, t: f32, spread: f32) -> f32 {
    if len == 0 {
        return 0.0;
    }
    let n = len as f32;
    // background-position runs 100% to 0% over a 250% wide image, so the
    // band's center travels from -25% to 125% of the text width.
    let center = (1.25 - 1.5 * (1.0 - t)) * n;
    let half = (n * spread / AVERAGE_GLYPH).max(0.5);
    let x = index as f32 + 0.5;
    (1.0 - (x - center).abs() / half).clamp(0.0, 1.0)
}

#[derive(IntoElement)]
pub struct Shimmer {
    id: ElementId,
    text: SharedString,
    duration: f32,
    spread: f32,
}

/// `<Shimmer>` with the React defaults: 2 s per pass, spread 2.
pub fn shimmer(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Shimmer {
    Shimmer {
        id: id.into(),
        text: text.into(),
        duration: 2.0,
        spread: 2.0,
    }
}

impl Shimmer {
    /// Seconds per pass.
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = seconds;
        self
    }

    pub fn spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }
}

fn mix_alpha(content: Hsla, strength: f32) -> Hsla {
    // The band (content at `strength`) composited over content at 40%.
    let alpha = strength + 0.4 * (1.0 - strength);
    Hsla {
        a: alpha,
        ..content
    }
}

impl RenderOnce for Shimmer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let content = Theme::of(cx).colors.content;
        let text = self.text;
        let spread = self.spread;
        let base = window.text_style();
        let period = Duration::from_secs_f32(self.duration.max(0.1));
        div().with_animation(self.id, Animation::new(period).repeat(), move |el, t| {
            let len = text.chars().count();
            let runs = text
                .char_indices()
                .enumerate()
                .map(|(index, (_, ch))| {
                    let color = mix_alpha(content, shimmer_strength(index, len, t, spread));
                    TextRun {
                        color,
                        ..base.clone().to_run(ch.len_utf8())
                    }
                })
                .collect::<Vec<_>>();
            el.child(StyledText::new(text.clone()).with_runs(runs))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::shimmer_strength;

    #[test]
    fn the_band_sweeps_left_to_right() {
        let len = 10;
        // At the start the band sits left of the text, at the end right of it.
        assert_eq!(shimmer_strength(9, len, 0.0, 2.0), 0.0);
        assert_eq!(shimmer_strength(0, len, 1.0, 2.0), 0.0);
        // Midway it lights the middle of the text most.
        let mid = shimmer_strength(5, len, 0.5, 2.0);
        assert!(mid > 0.8, "{mid}");
        assert!(shimmer_strength(0, len, 0.5, 2.0) < mid);
        assert_eq!(shimmer_strength(0, 0, 0.5, 2.0), 0.0);
    }
}
