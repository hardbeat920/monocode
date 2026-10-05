//! Port of src/features/sessions/model/speechBubble.ts: a pixel-art speech
//! bubble drawn straight onto the grid canvas. Every edge snaps to a chunky
//! unit and the corners and tail are stepped rather than curved, so it reads
//! as sprite art next to the square grid instead of a smooth tooltip.
//!
//! [`speech_bubble_layout`] is the geometry the TypeScript traced on a 2D
//! context; [`draw_speech_bubble`] paints it with GPUI.

use gpui::{
    App, Bounds, Font, FontWeight, Hsla, PathBuilder, Pixels, SharedString, TextAlign, TextRun,
    Window, font, point, px,
};
use monocode_ui::color::with_alpha;

/// One "pixel" of the bubble art, in CSS px.
pub const UNIT: f32 = 2.0;
/// Corner chamfer and tail step, both one unit.
const STEP: f32 = UNIT;
const TAIL_STEPS: usize = 3;
const TAIL_H: f32 = TAIL_STEPS as f32 * STEP;
const SHADOW_OFFSET: f32 = 2.0 * UNIT;

pub const FONT_PX: f32 = 10.0;
const PAD_X: f32 = 3.0 * UNIT;
const PAD_Y: f32 = 2.0 * UNIT;
/// `letterSpacing = "1px"`.
pub const LETTER_SPACING: f32 = 1.0;

fn snap(value: f32) -> f32 {
    (value / UNIT).round() * UNIT
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    min.max(max.min(value))
}

/// A point on the canvas, in CSS px.
pub type Point = (f32, f32);

/// `outline`: the bubble clockwise from the top-left chamfer. The tail is a
/// staircase hanging off whichever edge faces the snake, with its tip at
/// `x + tail_x` so it can be aimed at the head.
fn outline(x: f32, y: f32, w: f32, h: f32, tail_x: f32, below: bool) -> Vec<Point> {
    let mut points = Vec::new();
    let mut at = |px: f32, py: f32| points.push((px, py));

    at(x + STEP, y);
    if below {
        // The bubble sits under the head, so the tail points up out of the top edge.
        at(x + tail_x, y);
        at(x + tail_x, y - TAIL_H);
        for step in (1..=TAIL_STEPS).rev() {
            let step_x = x + tail_x + (TAIL_STEPS - step + 1) as f32 * STEP;
            at(step_x, y - step as f32 * STEP);
            at(step_x, y - (step - 1) as f32 * STEP);
        }
    }
    at(x + w - STEP, y);
    at(x + w - STEP, y + STEP);
    at(x + w, y + STEP);

    at(x + w, y + h - STEP);
    at(x + w - STEP, y + h - STEP);
    at(x + w - STEP, y + h);
    if !below {
        // The tail hangs off the bottom edge, stepping down toward the head.
        for step in 1..=TAIL_STEPS {
            let step_x = x + tail_x + (TAIL_STEPS - step + 1) as f32 * STEP;
            at(step_x, y + h + (step - 1) as f32 * STEP);
            at(step_x, y + h + step as f32 * STEP);
        }
        at(x + tail_x, y + h + TAIL_H);
        at(x + tail_x, y + h);
    }
    at(x + STEP, y + h);
    at(x + STEP, y + h - STEP);
    at(x, y + h - STEP);

    at(x, y + STEP);
    at(x + STEP, y + STEP);

    points
}

/// What [`draw_speech_bubble`] traces and fills, in canvas px.
#[derive(Debug, Clone, PartialEq)]
pub struct BubbleLayout {
    /// The hard offset shadow, traced first.
    pub shadow: Vec<Point>,
    /// The bubble itself.
    pub bubble: Vec<Point>,
    /// The text's left edge and vertical center.
    pub text_origin: Point,
    pub width: f32,
    pub height: f32,
    /// The bubble flipped under the head for lack of room above.
    pub below: bool,
}

/// The geometry `drawSpeechBubble` traced for text `text_width` px wide,
/// pointing at the cell at `head_x`, `head_y`. Returns `None` once the
/// bubble has faded out (`alpha <= 0.01`), where the TypeScript drew
/// nothing.
#[allow(clippy::too_many_arguments)]
pub fn speech_bubble_layout(
    text_width: f32,
    head_x: f32,
    head_y: f32,
    head_size: f32,
    bounds_width: f32,
    alpha: f32,
) -> Option<BubbleLayout> {
    if alpha <= 0.01 {
        return None;
    }
    let w = snap(text_width + PAD_X * 2.0);
    let h = snap(FONT_PX + PAD_Y * 2.0);
    let margin = UNIT + SHADOW_OFFSET;

    let above = head_y - UNIT - TAIL_H - h;
    let below = above < margin;
    // Snap the whole bubble to the art grid: the head position is eased, so
    // it arrives fractional and the chunky edges would land off-pixel.
    let y = snap(if below {
        head_y + head_size + UNIT + TAIL_H
    } else {
        above
    });

    // Aim the tail at the head, then keep the whole bubble on screen.
    let preferred = head_x - 4.0 * STEP;
    let x = snap(clamp(
        preferred,
        margin,
        margin.max(bounds_width - w - margin),
    ));
    let tail_x = clamp(
        snap(head_x - x),
        2.0 * STEP,
        (2.0 * STEP).max(w - (TAIL_STEPS as f32 + 2.0) * STEP),
    );

    Some(BubbleLayout {
        shadow: outline(x + SHADOW_OFFSET, y + SHADOW_OFFSET, w, h, tail_x, below),
        bubble: outline(x, y, w, h, tail_x, below),
        text_origin: (x + PAD_X, y + h / 2.0 + 1.0),
        width: w,
        height: h,
        below,
    })
}

/// `BubbleTheme`.
#[derive(Debug, Clone, Copy)]
pub struct BubbleTheme {
    pub fg: Hsla,
    pub bg: Hsla,
}

fn bubble_font() -> Font {
    let mut font = font("Menlo");
    font.weight = FontWeight::SEMIBOLD;
    font
}

/// The width the bubble's font gives `text`, with the 1px letter spacing.
pub fn measure_bubble_text(text: &str, window: &Window) -> f32 {
    let run = TextRun {
        len: text.len(),
        font: bubble_font(),
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(text.to_string()),
        px(FONT_PX),
        &[run],
        None,
    );
    f32::from(line.width) + LETTER_SPACING * text.chars().count() as f32
}

fn trace(points: &[Point], origin: gpui::Point<Pixels>, builder: &mut PathBuilder) {
    for (index, (x, y)) in points.iter().enumerate() {
        let at = point(origin.x + px(*x), origin.y + px(*y));
        if index == 0 {
            builder.move_to(at);
        } else {
            builder.line_to(at);
        }
    }
    builder.close();
}

/// `drawSpeechBubble`: draws `text` in a bubble whose tail points at the
/// cell at `head_x`, `head_y`, in px relative to `bounds`. Flips below the
/// head when there is no room above.
#[allow(clippy::too_many_arguments)]
pub fn draw_speech_bubble(
    text: &str,
    head_x: f32,
    head_y: f32,
    head_size: f32,
    bounds: Bounds<Pixels>,
    alpha: f32,
    theme: BubbleTheme,
    window: &mut Window,
    cx: &mut App,
) {
    let text_width = measure_bubble_text(text, window);
    let Some(layout) = speech_bubble_layout(
        text_width,
        head_x,
        head_y,
        head_size,
        f32::from(bounds.size.width),
        alpha,
    ) else {
        return;
    };
    let origin = bounds.origin;

    // Hard offset shadow, the way sprite art fakes depth.
    let mut shadow = PathBuilder::fill();
    trace(&layout.shadow, origin, &mut shadow);
    if let Ok(path) = shadow.build() {
        window.paint_path(path, with_alpha(theme.fg, alpha * 0.2));
    }
    let mut fill = PathBuilder::fill();
    trace(&layout.bubble, origin, &mut fill);
    if let Ok(path) = fill.build() {
        window.paint_path(path, with_alpha(theme.bg, alpha));
    }
    let mut stroke = PathBuilder::stroke(px(UNIT));
    trace(&layout.bubble, origin, &mut stroke);
    if let Ok(path) = stroke.build() {
        window.paint_path(path, with_alpha(theme.fg, alpha));
    }

    // GPUI has no letter spacing, so each glyph is placed on its own.
    let line_height = px(FONT_PX * 1.2);
    let mut x = origin.x + px(layout.text_origin.0);
    let y = origin.y + px(layout.text_origin.1) - line_height / 2.0;
    for ch in text.chars() {
        let glyph = ch.to_string();
        let run = TextRun {
            len: glyph.len(),
            font: bubble_font(),
            color: with_alpha(theme.fg, alpha),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line =
            window
                .text_system()
                .shape_line(SharedString::from(glyph), px(FONT_PX), &[run], None);
        let advance = line.width;
        line.paint(point(x, y), line_height, TextAlign::Left, None, window, cx)
            .ok();
        x += advance + px(LETTER_SPACING);
    }
}

#[cfg(test)]
mod tests {
    //! Port of speechBubble.test.ts. The recording context measured text
    //! at 6.5px per character.

    use super::*;

    const BOUNDS_WIDTH: f32 = 700.0;
    const HEAD: f32 = 6.0;

    fn bubble_path(head_x: f32, head_y: f32, text: &str) -> Vec<Point> {
        speech_bubble_layout(
            text.len() as f32 * 6.5,
            head_x,
            head_y,
            HEAD,
            BOUNDS_WIDTH,
            1.0,
        )
        .expect("a bubble")
        .bubble
    }

    struct Box {
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    }

    fn bbox(path: &[Point]) -> Box {
        Box {
            left: path.iter().map(|p| p.0).fold(f32::INFINITY, f32::min),
            right: path.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max),
            top: path.iter().map(|p| p.1).fold(f32::INFINITY, f32::min),
            bottom: path.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max),
        }
    }

    #[test]
    fn points_its_tail_at_the_snakes_head() {
        let head_x = 300.0;
        let path = bubble_path(head_x, 90.0, "HELLO THERE!");
        // The tail's tip is the lowest point, and its leading edge sits on the head.
        let bottom = bbox(&path).bottom;
        let tip_left = path
            .iter()
            .filter(|p| p.1 == bottom)
            .map(|p| p.0)
            .fold(f32::INFINITY, f32::min);
        assert_eq!(tip_left, head_x);
    }

    #[test]
    fn sits_above_the_head_when_there_is_room() {
        let head_y = 90.0;
        assert!(bbox(&bubble_path(300.0, head_y, "HELLO THERE!")).bottom <= head_y);
    }

    #[test]
    fn flips_below_the_head_when_it_would_run_off_the_top() {
        let head_y = 4.0;
        assert!(bbox(&bubble_path(300.0, head_y, "HELLO THERE!")).top >= head_y);
    }

    #[test]
    fn keeps_itself_on_screen_at_either_edge() {
        for head_x in [0.0, 4.0, 340.0, BOUNDS_WIDTH - 4.0, BOUNDS_WIDTH] {
            let bbox = bbox(&bubble_path(head_x, 90.0, "HELLO THERE!"));
            assert!(bbox.left >= 0.0);
            assert!(bbox.right <= BOUNDS_WIDTH);
        }
    }

    #[test]
    fn grows_with_the_text() {
        let short = bbox(&bubble_path(300.0, 90.0, "YOINK"));
        let long = bbox(&bubble_path(300.0, 90.0, "RESOLVING DEPENDENCY"));
        assert!(long.right - long.left > short.right - short.left);
    }

    #[test]
    fn snaps_every_edge_to_the_pixel_grid() {
        for (x, y) in bubble_path(301.0, 91.0, "HELLO THERE!") {
            assert_eq!(x % 2.0, 0.0);
            assert_eq!(y % 2.0, 0.0);
        }
    }

    #[test]
    fn draws_nothing_once_it_has_faded_out() {
        assert_eq!(
            speech_bubble_layout(78.0, 300.0, 90.0, HEAD, BOUNDS_WIDTH, 0.0),
            None
        );
    }
}
