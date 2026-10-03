//! Wraps prompt text into visual rows and maps between byte offsets and
//! points. Each hard line is wrapped with GPUI's `LineWrapper`, which takes
//! the first row's CSS `text-indent` as a fixed-width fragment, and each row
//! is shaped on its own so a row's x positions start at zero.

use std::ops::Range;

use gpui::{
    App, Bounds, Font, Pixels, Point, ShapedLine, SharedString, TextRun, Window, point, px, size,
};

/// One visual row.
pub struct Row {
    /// Bytes of the full text this row shows. A hard line's trailing `\n`
    /// is not part of any row.
    pub range: Range<usize>,
    pub line: ShapedLine,
    /// Left offset of the row's first glyph (the first-line indent).
    pub x: Pixels,
    pub y: Pixels,
    /// True when the row ends at a soft wrap, so its end offset is also the
    /// next row's start.
    pub soft_wrapped: bool,
}

/// The prompt's rows, in text order.
pub struct TextLayout {
    pub rows: Vec<Row>,
    pub line_height: Pixels,
    pub wrap_width: Pixels,
    pub text_len: usize,
}

/// Which row a caret at a soft wrap belongs to: `upstream` keeps it at the
/// end of the earlier row, otherwise it starts the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    pub offset: usize,
    pub upstream: bool,
}

impl Caret {
    pub fn new(offset: usize) -> Self {
        Self {
            offset,
            upstream: false,
        }
    }
}

impl TextLayout {
    /// Lays out `text` with `runs` covering it. `indent` shifts the first
    /// row only, like CSS `text-indent`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        text: &str,
        runs: &[TextRun],
        font: &Font,
        font_size: Pixels,
        line_height: Pixels,
        wrap_width: Pixels,
        indent: Pixels,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self {
        let wrap_width = wrap_width.max(px(1.));
        let text_system = window.text_system().clone();
        let mut wrapper = text_system.line_wrapper(font.clone(), font_size);
        let mut rows = Vec::new();
        let mut line_start = 0usize;
        for (line_ix, line_text) in text.split('\n').enumerate() {
            let first_indent = if line_ix == 0 { indent } else { px(0.) };
            let mut boundaries: Vec<usize> = Vec::new();
            {
                let fragments = if first_indent > px(0.) {
                    vec![
                        gpui::LineFragment::element(first_indent, 0),
                        gpui::LineFragment::text(line_text),
                    ]
                } else {
                    vec![gpui::LineFragment::text(line_text)]
                };
                for boundary in wrapper.wrap_line(&fragments, wrap_width) {
                    if boundary.ix > 0 && boundary.ix < line_text.len() {
                        boundaries.push(boundary.ix);
                    }
                }
            }
            let mut row_start = 0usize;
            let ends = boundaries.iter().copied().chain([line_text.len()]);
            let count = boundaries.len() + 1;
            for (ix, row_end) in ends.enumerate() {
                let range = line_start + row_start..line_start + row_end;
                let row_runs = slice_runs(runs, range.clone(), font);
                let shaped = text_system.shape_line(
                    SharedString::from(text[range.clone()].to_string()),
                    font_size,
                    &row_runs,
                    None,
                );
                let x = if line_ix == 0 && ix == 0 {
                    first_indent
                } else {
                    px(0.)
                };
                rows.push(Row {
                    range,
                    line: shaped,
                    x,
                    y: line_height * rows.len() as f32,
                    soft_wrapped: ix + 1 < count,
                });
                row_start = row_end;
            }
            line_start += line_text.len() + 1;
        }
        Self {
            rows,
            line_height,
            wrap_width,
            text_len: text.len(),
        }
    }

    /// Height of all rows.
    pub fn height(&self) -> Pixels {
        self.line_height * self.rows.len().max(1) as f32
    }

    /// The row a caret sits on.
    pub fn row_for(&self, caret: Caret) -> usize {
        let offset = caret.offset.min(self.text_len);
        for (ix, row) in self.rows.iter().enumerate() {
            if offset < row.range.start {
                return ix.saturating_sub(1);
            }
            if offset < row.range.end {
                return ix;
            }
            if offset == row.range.end {
                if row.soft_wrapped && !caret.upstream {
                    continue;
                }
                return ix;
            }
        }
        self.rows.len().saturating_sub(1)
    }

    /// The caret's top-left point, relative to the text origin.
    pub fn position_for(&self, caret: Caret) -> Point<Pixels> {
        let Some(row) = self.rows.get(self.row_for(caret)) else {
            return point(px(0.), px(0.));
        };
        let local = caret.offset.clamp(row.range.start, row.range.end) - row.range.start;
        point(row.x + row.line.x_for_index(local), row.y)
    }

    /// The caret closest to `position` (relative to the text origin).
    pub fn caret_for_point(&self, position: Point<Pixels>) -> Caret {
        if self.rows.is_empty() {
            return Caret::new(0);
        }
        if position.y < px(0.) {
            return Caret::new(0);
        }
        let row_ix = ((position.y / self.line_height).floor() as usize).min(self.rows.len() - 1);
        if position.y >= self.height() {
            return Caret::new(self.text_len);
        }
        self.caret_in_row(row_ix, position.x)
    }

    /// The caret in row `row_ix` closest to `x`.
    pub fn caret_in_row(&self, row_ix: usize, x: Pixels) -> Caret {
        let row = &self.rows[row_ix];
        let local = (x - row.x).max(px(0.));
        let index = closest_index(&row.line, local).min(row.range.len());
        let offset = row.range.start + index;
        Caret {
            offset,
            upstream: row.soft_wrapped && offset == row.range.end,
        }
    }

    /// The caret `delta` rows above (negative) or below, at `goal_x`. Moving
    /// up from the first row goes to the start; down from the last, the end.
    pub fn vertical(&self, caret: Caret, delta: isize, goal_x: Pixels) -> Caret {
        if self.rows.is_empty() {
            return caret;
        }
        let row = self.row_for(caret) as isize + delta;
        if row < 0 {
            return Caret::new(0);
        }
        if row as usize >= self.rows.len() {
            return Caret::new(self.text_len);
        }
        self.caret_in_row(row as usize, goal_x)
    }

    /// Start of the visual row holding the caret (Cmd-Left).
    pub fn row_start(&self, caret: Caret) -> Caret {
        match self.rows.get(self.row_for(caret)) {
            Some(row) => Caret::new(row.range.start),
            None => Caret::new(0),
        }
    }

    /// End of the visual row holding the caret (Cmd-Right).
    pub fn row_end(&self, caret: Caret) -> Caret {
        match self.rows.get(self.row_for(caret)) {
            Some(row) => Caret {
                offset: row.range.end,
                upstream: row.soft_wrapped,
            },
            None => Caret::new(self.text_len),
        }
    }

    /// Rectangles covering `range`, one per row, relative to the text
    /// origin. A selection that runs past a hard line end also covers a
    /// sliver for the newline, like a browser textarea.
    pub fn range_bounds(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut out = Vec::new();
        if range.is_empty() {
            return out;
        }
        for row in &self.rows {
            let end_with_newline = if row.soft_wrapped {
                row.range.end
            } else {
                row.range.end + 1
            };
            if range.end <= row.range.start || range.start >= end_with_newline {
                continue;
            }
            let start = range.start.max(row.range.start) - row.range.start;
            let end = range.end.min(row.range.end) - row.range.start;
            let x0 = row.x + row.line.x_for_index(start);
            let mut x1 = row.x + row.line.x_for_index(end);
            if !row.soft_wrapped && range.end > row.range.end {
                x1 += self.line_height * 0.25;
            }
            if x1 > x0 {
                out.push(Bounds::new(
                    point(x0, row.y),
                    size(x1 - x0, self.line_height),
                ));
            }
        }
        out
    }

    /// Bounds of one character's box, for drawing an overlay on it.
    pub fn char_bounds(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        let row = self
            .rows
            .iter()
            .find(|row| range.start >= row.range.start && range.start < row.range.end)?;
        let start = range.start - row.range.start;
        let end = range.end.min(row.range.end) - row.range.start;
        let x0 = row.x + row.line.x_for_index(start);
        let x1 = row.x + row.line.x_for_index(end);
        Some(Bounds::new(
            point(x0, row.y),
            size(x1 - x0, self.line_height),
        ))
    }
}

/// The character boundary nearest `x`. GPUI's `closest_index_for_x` maps
/// any point inside the last glyph to the line end, so a click on the left
/// half of the last character would land after it.
pub fn closest_index(line: &ShapedLine, x: Pixels) -> usize {
    let mut best = (0usize, x.abs());
    for run in line.runs.iter() {
        for glyph in &run.glyphs {
            let distance = (glyph.position.x - x).abs();
            if distance < best.1 {
                best = (glyph.index, distance);
            }
        }
    }
    let end = (line.width - x).abs();
    if end < best.1 {
        best = (line.len, end);
    }
    best.0
}

/// The runs for `range` of the text the full `runs` cover. Gaps fall back to
/// `font` in the first run's color.
pub fn slice_runs(runs: &[TextRun], range: Range<usize>, font: &Font) -> Vec<TextRun> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for run in runs {
        let run_range = at..at + run.len;
        at += run.len;
        let start = run_range.start.max(range.start);
        let end = run_range.end.min(range.end);
        if start < end {
            out.push(TextRun {
                len: end - start,
                ..run.clone()
            });
        }
    }
    let covered: usize = out.iter().map(|run| run.len).sum();
    if covered < range.len() {
        let color = runs.first().map(|run| run.color).unwrap_or_default();
        out.push(TextRun {
            len: range.len() - covered,
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    out
}
