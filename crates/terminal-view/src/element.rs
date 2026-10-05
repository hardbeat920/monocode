//! The GPUI element that draws a [`TerminalView`]'s grid.
//!
//! Each frame it measures the cell size from the resolved monospace font,
//! fits columns and rows to its bounds (the fit logic of
//! src/features/terminal/model/terminalLayout.ts), reports the size to the
//! view, and paints: default background, cell background runs as quads,
//! the selection, text shaped in runs of same-styled cells, the cursor, IME
//! preedit text, and a scrollbar thumb. It also registers the window-level
//! mouse listeners and the platform input handler.

use std::sync::Arc;

use gpui::{
    App, Bounds, ContentMask, CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler,
    Entity, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, ScrollWheelEvent, ShapedLine, SharedString,
    StrikethroughStyle, Style, TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point,
    px, quad, relative, size,
};

use crate::emulator::{CursorShape, FrameCell, Side, UnderlineKind};
use crate::theme::{hsla, platform_family_name};
use crate::view::TerminalView;

/// `.monocode-terminal { padding: 8px 10px }` in src/styles/index.css. The
/// alternate screen drops it to 0.
pub const SHELL_PADDING_X: f32 = 10.0;
pub const SHELL_PADDING_Y: f32 = 8.0;
/// `DEFAULT_SCROLLBAR_WIDTH` in terminalLayout.ts: xterm's scrollbar gutter
/// in shell mode.
pub const SCROLLBAR_GUTTER: f32 = 14.0;
/// `MIN_TUI_SCROLLBAR_WIDTH` in terminalLayout.ts.
pub const TUI_GUTTER: f32 = 1.0;
/// `availableSize` in terminalLayout.ts skips fitting below 8px.
const MIN_FIT_SIZE: f32 = 8.0;
/// xterm.js `cursorWidth` default, for the bar cursor.
const BAR_CURSOR_WIDTH: f32 = 1.0;

/// Where the grid sits in the window and how big its cells are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutInfo {
    /// The element's bounds.
    pub bounds: Bounds<Pixels>,
    /// Top-left corner of cell (0, 0).
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub cols: usize,
    pub rows: usize,
}

/// The cell a pointer is over, and which edge of it a selection anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellHit {
    pub row: usize,
    pub col: usize,
    pub side: Side,
}

impl LayoutInfo {
    /// Map a window position to a cell. Positions outside the grid clamp to
    /// the nearest cell, so a drag that leaves the terminal extends to the
    /// edge it left through. Past the right or bottom edge the side is Right,
    /// so the last cell is included; above the top it is Left.
    pub fn cell_at(&self, position: Point<Pixels>) -> CellHit {
        let cell_w = f32::from(self.cell_width);
        let line_h = f32::from(self.line_height);
        let usable = |v: f32| v.is_finite() && v > 0.0;
        if self.cols == 0 || self.rows == 0 || !usable(cell_w) || !usable(line_h) {
            return CellHit {
                row: 0,
                col: 0,
                side: Side::Left,
            };
        }
        let x = f32::from(position.x - self.origin.x);
        let y = f32::from(position.y - self.origin.y);
        let (x, y) = (
            if x.is_finite() { x } else { 0.0 },
            if y.is_finite() { y } else { 0.0 },
        );
        let last_col = self.cols - 1;
        let last_row = self.rows - 1;

        let raw_col = (x / cell_w).floor();
        let mut side = if x.max(0.0) % cell_w > cell_w / 2.0 {
            Side::Right
        } else {
            Side::Left
        };
        let col = if raw_col > last_col as f32 {
            side = Side::Right;
            last_col
        } else {
            raw_col.max(0.0) as usize
        };
        let raw_row = (y / line_h).floor();
        let row = if raw_row > last_row as f32 {
            side = Side::Right;
            last_row
        } else if raw_row < 0.0 {
            side = Side::Left;
            0
        } else {
            raw_row as usize
        };
        CellHit { row, col, side }
    }

    /// Bounds of `width` cells starting at a viewport cell.
    pub fn cell_bounds(&self, row: usize, col: usize, width: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.cell_width * col as f32,
                self.origin.y + self.line_height * row as f32,
            ),
            size(self.cell_width * width as f32, self.line_height),
        )
    }

    /// Lines to scroll while a drag selection is near or past the top or
    /// bottom edge. Positive scrolls up into history.
    pub fn edge_scroll_lines(&self, position: Point<Pixels>) -> i32 {
        let line_h = f32::from(self.line_height);
        let top = f32::from(self.origin.y);
        let bottom = top + line_h * self.rows as f32;
        let y = f32::from(position.y);
        let speed = |overshoot: f32| (1.0 + overshoot / line_h).clamp(1.0, 5.0) as i32;
        if y < top {
            speed(top - y)
        } else if y > bottom {
            -speed(y - bottom)
        } else {
            0
        }
    }
}

/// Columns and rows that fit an available size. Port of `fitTerminal` in
/// terminalLayout.ts: shell mode floors, the alternate screen ("tui" mode)
/// rounds up so the grid covers the whole pane.
pub fn fit_grid(
    available_width: f32,
    available_height: f32,
    cell_width: f32,
    line_height: f32,
    tui: bool,
) -> Option<(usize, usize)> {
    if available_width < MIN_FIT_SIZE
        || available_height < MIN_FIT_SIZE
        || cell_width.is_nan()
        || cell_width < 1.0
        || line_height.is_nan()
        || line_height < 1.0
    {
        return None;
    }
    let round = |v: f32| if tui { v.ceil() } else { v.floor() };
    let cols = (round(available_width / cell_width) as usize).clamp(2, 1000);
    let rows = (round(available_height / line_height) as usize).clamp(1, 1000);
    Some((cols, rows))
}

/// Draws a [`TerminalView`]. The view's `render` creates it.
pub struct TerminalElement {
    view: Entity<TerminalView>,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>) -> Self {
        Self { view }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// One shaped piece of a row, pinned to its starting column.
struct Segment {
    col: usize,
    row: usize,
    line: ShapedLine,
}

pub struct TerminalPrepaint {
    hitbox: Hitbox,
    layout: LayoutInfo,
    background: Option<PaintQuad>,
    cell_backgrounds: Vec<PaintQuad>,
    selection: Vec<PaintQuad>,
    /// Painted before the text (a block cursor), or after it (bar,
    /// underline, outline).
    cursor_under_text: Option<PaintQuad>,
    cursor_over_text: Option<PaintQuad>,
    segments: Vec<Segment>,
    preedit: Option<(PaintQuad, Point<Pixels>, ShapedLine)>,
    scrollbar: Option<ScrollbarMetrics>,
    pointer: CursorStyle,
    /// Primary font ascent minus descent. GPUI centers each shaped line by
    /// its own font's metrics, so a glyph from a fallback font would sit on
    /// a different baseline. Paint shifts each segment back onto this one.
    ascent_minus_descent: Pixels,
}

#[derive(Clone, Copy)]
struct ScrollbarMetrics {
    track: Bounds<Pixels>,
    rows: usize,
    history: usize,
    offset: usize,
    color: gpui::Hsla,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = TerminalPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let family = self.view.update(cx, |view, _| view.font_family(window));
        let theme = self.view.read(cx).theme().clone();
        let fallbacks: Vec<String> = theme
            .font_families()
            .filter_map(|f| platform_family_name(f))
            .filter(|f| *f != family.as_ref())
            .map(str::to_string)
            .collect();
        let font = Font {
            family,
            // A terminal is a fixed grid. Ligatures would draw several cells
            // as fewer glyphs and shift the rest of the row.
            features: FontFeatures(Arc::new(vec![
                ("liga".into(), 0),
                ("calt".into(), 0),
                ("dlig".into(), 0),
            ])),
            fallbacks: (!fallbacks.is_empty()).then(|| FontFallbacks::from_fonts(fallbacks)),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };
        let font_size = theme.font_size;
        let scale = window.scale_factor();
        let (cell_width, line_height, ascent_minus_descent) = {
            let text_system = window.text_system();
            let font_id = text_system.resolve_font(&font);
            let cell_width = text_system
                .em_advance(font_id, font_size)
                .unwrap_or(font_size * 0.6);
            let ascent = f32::from(text_system.ascent(font_id, font_size));
            let descent = f32::from(text_system.descent(font_id, font_size)).abs();
            let natural = (ascent + descent) * theme.line_height;
            // Whole device pixels, like xterm.js's cell height, so row
            // backgrounds meet without seams.
            let line_height = ((natural * scale).round() / scale).max(1.0);
            (
                if cell_width > px(0.0) {
                    cell_width
                } else {
                    font_size * 0.6
                },
                px(line_height),
                px(ascent - descent),
            )
        };

        let tui = self.view.read(cx).emulator().alt_screen();
        let (pad_x, pad_y, gutter) = if tui {
            (0.0, 0.0, TUI_GUTTER)
        } else {
            (SHELL_PADDING_X, SHELL_PADDING_Y, SCROLLBAR_GUTTER)
        };
        let origin = point(bounds.left() + px(pad_x), bounds.top() + px(pad_y));
        let available_width = f32::from(bounds.size.width) - 2.0 * pad_x - gutter;
        let available_height = f32::from(bounds.size.height) - 2.0 * pad_y;
        let (cols, rows) = fit_grid(
            available_width,
            available_height,
            f32::from(cell_width),
            f32::from(line_height),
            tui,
        )
        .unwrap_or_else(|| {
            let emulator = self.view.read(cx).emulator();
            (emulator.cols(), emulator.rows())
        });
        let layout = LayoutInfo {
            bounds,
            origin,
            cell_width,
            line_height,
            cols,
            rows,
        };
        self.view
            .update(cx, |view, cx| view.apply_layout(layout, cx));

        let view = self.view.read(cx);
        let focused = view.focus_handle_ref().is_focused(window);
        let frame = view.emulator().frame();
        let hovered_link = view.hovered_link().cloned();
        let marked_text = view.marked_text().map(str::to_string);
        let cursor_visible = view.cursor_visible(window) && marked_text.is_none();
        let reporting = view.mouse_reporting(&window.modifiers());
        let emulator = view.emulator();

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        let background = (frame.background.a > 0.0).then(|| fill(bounds, hsla(frame.background)));

        let cursor = frame.cursor.filter(|_| cursor_visible);
        let block_cursor = cursor
            .and_then(|c| (focused && c.shape == CursorShape::Block).then_some((c.row, c.col)));

        let mut cell_backgrounds = Vec::new();
        let mut selection = Vec::new();
        let mut segments = Vec::new();
        let selection_color = hsla(if focused {
            theme.selection
        } else {
            theme.selection_inactive
        });
        for row in 0..frame.rows.min(rows) {
            let cells = frame.row(row);
            push_runs(&mut cell_backgrounds, &layout, row, cells, |cell| {
                cell.bg.map(hsla)
            });
            push_runs(&mut selection, &layout, row, cells, |cell| {
                cell.selected.then_some(selection_color)
            });
            let link_cols = hovered_link.as_ref().map(|link| {
                let line = emulator.grid_point(row, 0).line;
                (0..cells.len())
                    .map(|col| link.contains(gpui_point(line, col)))
                    .collect::<Vec<_>>()
            });
            let accent = block_cursor
                .filter(|(r, _)| *r == row)
                .map(|(_, c)| (c, hsla(theme.cursor_accent)));
            shape_row(
                &mut segments,
                row,
                cells,
                &font,
                font_size,
                cell_width,
                link_cols.as_deref(),
                accent,
                window,
            );
        }

        let (cursor_under_text, cursor_over_text) = match cursor {
            None => (None, None),
            Some(c) => {
                let color = hsla(frame.cursor_color);
                let cell = layout.cell_bounds(c.row, c.col, if c.wide { 2 } else { 1 });
                if !focused {
                    // xterm.js cursorInactiveStyle "outline".
                    (None, Some(outline(cell, color, gpui::BorderStyle::Solid)))
                } else {
                    match c.shape {
                        CursorShape::Block => (Some(fill(cell, color)), None),
                        CursorShape::Beam => (
                            None,
                            Some(fill(
                                Bounds::new(cell.origin, size(px(BAR_CURSOR_WIDTH), line_height)),
                                color,
                            )),
                        ),
                        CursorShape::Underline => (
                            None,
                            Some(fill(
                                Bounds::new(
                                    point(cell.left(), cell.bottom() - px(1.0)),
                                    size(cell.size.width, px(1.0)),
                                ),
                                color,
                            )),
                        ),
                        CursorShape::HollowBlock => {
                            (None, Some(outline(cell, color, gpui::BorderStyle::Solid)))
                        }
                        CursorShape::Hidden => (None, None),
                    }
                }
            }
        };

        let preedit = marked_text.and_then(|text| {
            let (row, col) = emulator.cursor_cell()?;
            let color = hsla(frame.foreground);
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color,
                background_color: None,
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let line =
                window
                    .text_system()
                    .shape_line(SharedString::from(text), font_size, &[run], None);
            let origin = layout.cell_bounds(row, col, 1).origin;
            let cover = fill(
                Bounds::new(origin, size(line.width.max(cell_width), line_height)),
                hsla(theme.base_background),
            );
            Some((cover, origin, line))
        });

        let scrollbar = (!tui && frame.history_size > 0).then(|| ScrollbarMetrics {
            track: Bounds::new(
                point(bounds.right() - px(SCROLLBAR_GUTTER), origin.y),
                size(
                    px(SCROLLBAR_GUTTER),
                    (bounds.size.height - px(2.0 * pad_y)).max(px(0.0)),
                ),
            ),
            rows: frame.rows,
            history: frame.history_size,
            offset: frame.display_offset,
            color: hsla(frame.foreground),
        });

        let pointer = if hovered_link.is_some() {
            CursorStyle::PointingHand
        } else if reporting {
            CursorStyle::Arrow
        } else {
            CursorStyle::IBeam
        };

        TerminalPrepaint {
            hitbox,
            layout,
            background,
            cell_backgrounds,
            selection,
            cursor_under_text,
            cursor_over_text,
            segments,
            preedit,
            scrollbar,
            pointer,
            ascent_minus_descent,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.view.read(cx).focus_handle_ref().clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        window.set_cursor_style(prepaint.pointer, &prepaint.hitbox);
        register_mouse_listeners(&self.view, &prepaint.hitbox, window);

        let line_height = prepaint.layout.line_height;
        let hovered = prepaint.hitbox.is_hovered(window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(background) = prepaint.background.take() {
                window.paint_quad(background);
            }
            for quad in prepaint.cell_backgrounds.drain(..) {
                window.paint_quad(quad);
            }
            for quad in prepaint.selection.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(cursor) = prepaint.cursor_under_text.take() {
                window.paint_quad(cursor);
            }
            for segment in &prepaint.segments {
                let mut origin = prepaint
                    .layout
                    .cell_bounds(segment.row, segment.col, 1)
                    .origin;
                let own = segment.line.ascent - segment.line.descent;
                origin.y += (prepaint.ascent_minus_descent - own) / 2.0;
                let _ = segment
                    .line
                    .paint(origin, line_height, TextAlign::Left, None, window, cx);
            }
            if let Some(cursor) = prepaint.cursor_over_text.take() {
                window.paint_quad(cursor);
            }
            if let Some((cover, origin, line)) = prepaint.preedit.take() {
                window.paint_quad(cover);
                let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
            }
            if let Some(bar) = prepaint.scrollbar
                && (hovered || bar.offset > 0)
            {
                paint_scrollbar(bar, window);
            }
        });
    }
}

/// A grid point from a grid line and column, for link hit tests.
fn gpui_point(line: alacritty_terminal::index::Line, col: usize) -> crate::emulator::GridPoint {
    crate::emulator::GridPoint::new(line, alacritty_terminal::index::Column(col))
}

/// Merge runs of cells with the same color into one quad each.
fn push_runs(
    out: &mut Vec<PaintQuad>,
    layout: &LayoutInfo,
    row: usize,
    cells: &[FrameCell],
    color_of: impl Fn(&FrameCell) -> Option<gpui::Hsla>,
) {
    let mut run: Option<(usize, gpui::Hsla)> = None;
    for (col, color) in cells
        .iter()
        .map(&color_of)
        .chain(std::iter::once(None))
        .enumerate()
    {
        match (run, color) {
            (Some((_, current)), Some(next)) if current == next => {}
            (current, next) => {
                if let Some((start, color)) = current {
                    out.push(fill(layout.cell_bounds(row, start, col - start), color));
                }
                run = next.map(|color| (col, color));
            }
        }
    }
}

/// Shape one row into column-pinned segments. ASCII cells shape together in
/// runs, which a monospace font advances by exactly one cell each. Any other
/// glyph (box drawing, CJK, emoji, combining marks) may come from a fallback
/// font with a different advance, so it becomes its own segment at its own
/// column and cannot shift the rest of the row. Undecorated spaces end a
/// segment and are skipped.
#[allow(clippy::too_many_arguments)]
fn shape_row(
    out: &mut Vec<Segment>,
    row: usize,
    cells: &[FrameCell],
    font: &Font,
    font_size: Pixels,
    cell_width: Pixels,
    link_cols: Option<&[bool]>,
    accent: Option<(usize, gpui::Hsla)>,
    window: &Window,
) {
    let mut text = String::new();
    let mut runs: Vec<TextRun> = Vec::new();
    let mut start_col = 0;

    let flush = |out: &mut Vec<Segment>,
                 text: &mut String,
                 runs: &mut Vec<TextRun>,
                 start_col: usize,
                 force: bool| {
        if text.is_empty() {
            return;
        }
        let line = window.text_system().shape_line(
            SharedString::from(std::mem::take(text)),
            font_size,
            runs,
            force.then_some(cell_width),
        );
        runs.clear();
        out.push(Segment {
            col: start_col,
            row,
            line,
        });
    };

    for (col, cell) in cells.iter().enumerate() {
        if cell.spacer {
            continue;
        }
        let in_link = link_cols.is_some_and(|cols| cols[col]);
        let mut color = hsla(cell.fg);
        if let Some((accent_col, accent_color)) = accent
            && accent_col == col
        {
            color = accent_color;
        }
        let invisible = cell.fg.a == 0.0;
        let decorated = cell.underline.is_some() || cell.strikeout || in_link;
        if (cell.ch == ' ' || invisible) && !decorated && cell.zerowidth.is_none() {
            flush(out, &mut text, &mut runs, start_col, true);
            continue;
        }
        let simple = cell.ch.is_ascii() && cell.zerowidth.is_none() && !cell.wide;
        if !simple {
            flush(out, &mut text, &mut runs, start_col, true);
        }
        if text.is_empty() {
            start_col = col;
        }
        let before = text.len();
        text.push(if invisible { ' ' } else { cell.ch });
        if let Some(marks) = &cell.zerowidth {
            text.extend(marks.iter());
        }
        let len = text.len() - before;

        let mut cell_font = font.clone();
        if cell.bold {
            cell_font.weight = FontWeight::BOLD;
        }
        if cell.italic {
            cell_font.style = FontStyle::Italic;
        }
        let underline = if in_link {
            Some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(color),
                wavy: false,
            })
        } else {
            cell.underline.map(|kind| UnderlineStyle {
                thickness: px(1.0),
                color: Some(cell.underline_color.map(hsla).unwrap_or(color)),
                wavy: kind == UnderlineKind::Curly,
            })
        };
        let strikethrough = cell.strikeout.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        });
        match runs.last_mut() {
            Some(last)
                if last.color == color
                    && last.font == cell_font
                    && last.underline == underline
                    && last.strikethrough == strikethrough =>
            {
                last.len += len;
            }
            _ => runs.push(TextRun {
                len,
                font: cell_font,
                color,
                background_color: None,
                underline,
                strikethrough,
            }),
        }
        if !simple {
            flush(out, &mut text, &mut runs, start_col, false);
        }
    }
    flush(out, &mut text, &mut runs, start_col, true);
}

fn paint_scrollbar(bar: ScrollbarMetrics, window: &mut Window) {
    let track_h = f32::from(bar.track.size.height);
    if track_h <= 0.0 {
        return;
    }
    let total = (bar.rows + bar.history) as f32;
    let thumb_h = (track_h * bar.rows as f32 / total).clamp(20.0_f32.min(track_h), track_h);
    let travel = track_h - thumb_h;
    let top =
        f32::from(bar.track.top()) + travel * (1.0 - bar.offset as f32 / bar.history.max(1) as f32);
    let width = 6.0;
    let left = f32::from(bar.track.left()) + (f32::from(bar.track.size.width) - width) / 2.0;
    // xterm.js scrollbarSliderBackground: the foreground at 20%.
    let color = bar.color.opacity(0.2);
    window.paint_quad(quad(
        Bounds::new(point(px(left), px(top)), size(px(width), px(thumb_h))),
        px(width / 2.0),
        color,
        px(0.0),
        gpui::transparent_black(),
        gpui::BorderStyle::Solid,
    ));
}

fn register_mouse_listeners(view: &Entity<TerminalView>, hitbox: &Hitbox, window: &mut Window) {
    window.on_mouse_event({
        let view = view.clone();
        let hitbox = hitbox.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                cx.stop_propagation();
                view.update(cx, |view, cx| view.mouse_down(event, window, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        let hitbox = hitbox.clone();
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                let hovered = hitbox.is_hovered(window);
                view.update(cx, |view, cx| view.mouse_move(event, hovered, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| view.mouse_up(event, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        let hitbox = hitbox.clone();
        move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                cx.stop_propagation();
                view.update(cx, |view, cx| view.scroll_wheel(event, cx));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> LayoutInfo {
        // 10 by 20 cells, an 8 by 4 grid starting at (5, 5).
        LayoutInfo {
            bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(100.0))),
            origin: point(px(5.0), px(5.0)),
            cell_width: px(10.0),
            line_height: px(20.0),
            cols: 8,
            rows: 4,
        }
    }

    fn hit(x: f32, y: f32) -> CellHit {
        layout().cell_at(point(px(x + 5.0), px(y + 5.0)))
    }

    #[test]
    fn pointer_maps_to_cells_and_sides() {
        assert_eq!(
            hit(25.0, 45.0),
            CellHit {
                row: 2,
                col: 2,
                side: Side::Left
            }
        );
        assert_eq!(hit(26.0, 0.0).side, Side::Right);
        assert_eq!(hit(25.0, 0.0).side, Side::Left);
    }

    #[test]
    fn overshoot_clamps_and_forces_the_side() {
        assert_eq!(hit(9999.0, 0.0).col, 7);
        assert_eq!(hit(9999.0, 0.0).side, Side::Right);
        assert_eq!(hit(1.0, 9999.0).row, 3);
        assert_eq!(hit(1.0, 9999.0).side, Side::Right);
        assert_eq!(
            hit(-50.0, -50.0),
            CellHit {
                row: 0,
                col: 0,
                side: Side::Left
            }
        );
        assert_eq!(hit(f32::NAN, 0.0).col, 0);
    }

    #[test]
    fn edge_scroll_speed() {
        let l = layout();
        assert_eq!(l.edge_scroll_lines(point(px(10.0), px(50.0))), 0);
        assert_eq!(l.edge_scroll_lines(point(px(10.0), px(0.0))), 1);
        assert_eq!(l.edge_scroll_lines(point(px(10.0), px(-200.0))), 5);
        assert_eq!(l.edge_scroll_lines(point(px(10.0), px(90.0))), -1);
    }

    #[test]
    fn fit_matches_terminal_layout_ts() {
        // Shell mode floors.
        assert_eq!(fit_grid(805.0, 405.0, 8.0, 16.0, false), Some((100, 25)));
        // TUI mode rounds up so the grid covers the pane.
        assert_eq!(fit_grid(805.0, 405.0, 8.0, 16.0, true), Some((101, 26)));
        // Too small to fit: keep the old size.
        assert_eq!(fit_grid(7.0, 400.0, 8.0, 16.0, false), None);
        // Minimums.
        assert_eq!(fit_grid(9.0, 9.0, 8.0, 16.0, false), Some((2, 1)));
    }
}
