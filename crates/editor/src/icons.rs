//! Small stroked icons drawn with GPUI paths, so the views need no icon assets.
//! The shapes follow the inline SVGs in editorSearch.ts and editorGit.ts.

use gpui::{
    App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled, Window, canvas, point, px,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    ChevronUp,
    ChevronDown,
    ChevronRight,
    Close,
    Plus,
    Minus,
    Undo,
    Comment,
    Check,
}

/// An icon `size` pixels square, stroked in `color`.
pub fn icon(kind: IconKind, size: Pixels, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| paint_icon(kind, bounds, color, window, cx),
    )
    .size(size)
    .flex_none()
}

/// Paint `kind` into `bounds`. Coordinates are on a 16 by 16 grid, like the
/// `viewBox="0 0 16 16"` icons of the find panel.
pub fn paint_icon(
    kind: IconKind,
    bounds: Bounds<Pixels>,
    color: Hsla,
    window: &mut Window,
    _: &mut App,
) {
    let scale = bounds.size.width.min(bounds.size.height) / 16.;
    let at = |x: f32, y: f32| -> Point<Pixels> {
        point(bounds.origin.x + scale * x, bounds.origin.y + scale * y)
    };
    let width = (scale * 1.75).max(px(1.));
    let strokes: &[&[(f32, f32)]] = match kind {
        IconKind::ChevronUp => &[&[(4., 10.), (8., 6.), (12., 10.)]],
        IconKind::ChevronDown => &[&[(4., 6.), (8., 10.), (12., 6.)]],
        IconKind::ChevronRight => &[&[(6., 4.), (10., 8.), (6., 12.)]],
        IconKind::Close => &[&[(4.5, 4.5), (11.5, 11.5)], &[(11.5, 4.5), (4.5, 11.5)]],
        IconKind::Plus => &[&[(3.5, 8.), (12.5, 8.)], &[(8., 3.5), (8., 12.5)]],
        IconKind::Minus => &[&[(3.5, 8.), (12.5, 8.)]],
        IconKind::Undo => &[
            &[(2.5, 4.5), (2.5, 8.5), (6.5, 8.5)],
            &[
                (2.5, 8.5),
                (4.6, 6.4),
                (6.6, 5.2),
                (9., 5.0),
                (11.3, 5.9),
                (13., 7.8),
                (13.5, 10.),
                (13.5, 11.5),
            ],
        ],
        IconKind::Comment => &[
            &[
                (13.5, 9.5),
                (13.5, 4.5),
                (12., 3.),
                (4., 3.),
                (2.5, 4.5),
                (2.5, 14.),
                (5.5, 12.),
                (12., 12.),
                (13.5, 10.5),
                (13.5, 9.5),
            ],
            &[(8., 5.5), (8., 9.5)],
            &[(6., 7.5), (10., 7.5)],
        ],
        IconKind::Check => &[&[(3.5, 8.5), (6.5, 11.5), (12.5, 4.5)]],
    };
    for stroke in strokes {
        let mut builder = PathBuilder::stroke(width);
        let mut points = stroke.iter();
        if let Some((x, y)) = points.next() {
            builder.move_to(at(*x, *y));
        }
        for (x, y) in points {
            builder.line_to(at(*x, *y));
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}
