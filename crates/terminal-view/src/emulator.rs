//! Terminal state: `alacritty_terminal`'s `Term` and its VT parser.
//!
//! Replaces the xterm.js `Terminal` object in
//! src/features/terminal/ui/TerminalView.tsx. Bytes from the PTY go in
//! through [`Emulator::feed`]; [`Emulator::frame`] returns the visible grid
//! with colors already resolved against a [`TerminalTheme`]. Nothing here
//! touches GPUI windows or IO, so the escape-sequence handling is testable
//! with byte strings.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::{Selection, SelectionRange};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    ClearMode, Color as AnsiColor, CursorStyle, Handler, NamedColor, Processor, Rgb,
};
use gpui::Rgba;
use regex::Regex;

use crate::theme::{TerminalTheme, opaque, rgb_u8, to_u8};

pub use alacritty_terminal::index::{Point as GridPoint, Side};
pub use alacritty_terminal::selection::SelectionType;
pub use alacritty_terminal::vte::ansi::CursorShape;

/// Size of the grid in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

impl GridSize {
    /// Alacritty needs at least two columns (for wide characters) and one row.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(2),
            rows: rows.max(1),
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// Which mouse reports the running program asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseMode {
    /// Press and release (DECSET 1000).
    pub click: bool,
    /// Motion while a button is held (DECSET 1002).
    pub drag: bool,
    /// All motion (DECSET 1003).
    pub motion: bool,
    /// SGR encoding (DECSET 1006).
    pub sgr: bool,
    /// UTF-8 coordinate encoding (DECSET 1005).
    pub utf8: bool,
}

impl MouseMode {
    /// True when any report mode is on.
    pub fn reporting(&self) -> bool {
        self.click || self.drag || self.motion
    }
}

/// Underline styles from SGR 4, 4:2 to 4:5, and 21.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineKind {
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// One visible cell with its paint colors resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameCell {
    pub ch: char,
    /// Combining marks drawn on top of `ch`.
    pub zerowidth: Option<Box<[char]>>,
    /// Text color, including dim (half alpha) and hidden (alpha 0).
    pub fg: Rgba,
    /// `None` means the default background, which the frame paints once.
    pub bg: Option<Rgba>,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<UnderlineKind>,
    /// SGR 58 underline color. `None` means "same as the text".
    pub underline_color: Option<Rgba>,
    pub strikeout: bool,
    /// The first half of a double-width character.
    pub wide: bool,
    /// The second half of a double-width character, or the padding cell
    /// before a wide character that did not fit at the end of a line. Never
    /// drawn as text.
    pub spacer: bool,
    /// Inside the selection.
    pub selected: bool,
}

/// Cursor in viewport coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    /// Whether the program wants this cursor to blink (DECSCUSR).
    pub blinking: bool,
    /// The cursor sits on a double-width character.
    pub wide: bool,
}

/// Everything needed to paint the viewport once.
#[derive(Debug, Clone)]
pub struct Frame {
    pub cols: usize,
    pub rows: usize,
    /// `rows * cols` cells, row-major, top row first.
    pub cells: Vec<FrameCell>,
    pub cursor: Option<FrameCursor>,
    /// Default background. OSC 11 can change it.
    pub background: Rgba,
    /// Default foreground. OSC 10 can change it.
    pub foreground: Rgba,
    /// Cursor color. OSC 12 can change it.
    pub cursor_color: Rgba,
    pub display_offset: usize,
    pub history_size: usize,
    pub alt_screen: bool,
}

impl Frame {
    /// The cells of one viewport row.
    pub fn row(&self, row: usize) -> &[FrameCell] {
        &self.cells[row * self.cols..(row + 1) * self.cols]
    }
}

/// A link under a grid point, either an OSC 8 hyperlink or a URL found in
/// the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub uri: String,
    /// First cell of the link, in grid coordinates.
    pub start: GridPoint,
    /// Last cell of the link, inclusive.
    pub end: GridPoint,
}

impl Link {
    /// Whether the cell at `point` belongs to this link.
    pub fn contains(&self, point: GridPoint) -> bool {
        self.start <= point && point <= self.end
    }
}

/// Collects `Term` callbacks. `send_event` takes `&self`, and the emulator
/// lives on one thread, so a `RefCell` is enough.
#[derive(Clone, Default)]
struct Listener(Rc<RefCell<Vec<Event>>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

/// The URL pattern of xterm.js's WebLinksAddon.
fn url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(https?|HTTPS?)://[^\s"'!*(){}|\\^<>`]*[^\s"':,.!?{}|\\^~\[\]`()<>]"#)
            .expect("URL regex compiles")
    })
}

/// The terminal state machine.
pub struct Emulator {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    theme: TerminalTheme,
    title: Option<String>,
    title_changed: bool,
    bell: bool,
    cell_width: f32,
    cell_height: f32,
}

impl Emulator {
    pub fn new(size: GridSize, theme: TerminalTheme) -> Self {
        let listener = Listener::default();
        let config = Config {
            scrolling_history: theme.scrollback,
            // xterm.js options in TerminalView.tsx: cursorStyle "bar",
            // cursorBlink true.
            default_cursor_style: CursorStyle {
                shape: CursorShape::Beam,
                blinking: true,
            },
            // xterm.js ignores OSC 52 unless the clipboard addon is loaded,
            // and TerminalView.tsx does not load it.
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let term = Term::new(config, &size, listener.clone());
        Self {
            term,
            parser: Processor::new(),
            listener,
            theme,
            title: None,
            title_changed: false,
            bell: false,
            cell_width: 8.0,
            cell_height: 16.0,
        }
    }

    pub fn theme(&self) -> &TerminalTheme {
        &self.theme
    }

    /// Swap the theme, for example when the app switches between light and
    /// dark (`SCHEME_CHANGE_EVENT` in TerminalView.tsx).
    pub fn set_theme(&mut self, theme: TerminalTheme) {
        self.theme = theme;
    }

    /// Cell size in pixels, used to answer CSI 14 t.
    pub fn set_cell_size(&mut self, width: f32, height: f32) {
        self.cell_width = width;
        self.cell_height = height;
    }

    /// Parse PTY output. Returns the bytes the terminal wants written back to
    /// the PTY: replies to DSR, DA, and color queries (OSC 10, 11, 12, 4).
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.advance(&mut self.term, bytes);
        let events: Vec<Event> = self.listener.0.borrow_mut().drain(..).collect();
        let mut replies = Vec::new();
        for event in events {
            match event {
                Event::PtyWrite(text) => replies.extend_from_slice(text.as_bytes()),
                Event::ColorRequest(index, format) => {
                    let (r, g, b) = to_u8(self.query_color(index));
                    replies.extend_from_slice(format(Rgb { r, g, b }).as_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = WindowSize {
                        num_lines: self.rows() as u16,
                        num_cols: self.cols() as u16,
                        cell_width: self.cell_width.round() as u16,
                        cell_height: self.cell_height.round() as u16,
                    };
                    replies.extend_from_slice(format(size).as_bytes());
                }
                Event::Title(title) => {
                    self.title = Some(title);
                    self.title_changed = true;
                }
                Event::ResetTitle => {
                    self.title = None;
                    self.title_changed = true;
                }
                Event::Bell => self.bell = true,
                _ => {}
            }
        }
        replies
    }

    /// The color an OSC query reports. TerminalView.tsx answers OSC 10, 11,
    /// and 12 with `oscColors()`: content, base background, and accent.
    fn query_color(&self, index: usize) -> Rgba {
        if let Some(rgb) = self.term.colors()[index] {
            return rgb_u8(rgb.r, rgb.g, rgb.b);
        }
        match index {
            0..=255 => self.theme.indexed(index as u8),
            i if i == NamedColor::Background as usize => self.theme.base_background,
            i if i == NamedColor::Cursor as usize => self.theme.cursor,
            _ => self.theme.foreground,
        }
    }

    pub fn resize(&mut self, size: GridSize) {
        self.term.resize(size);
    }

    pub fn cols(&self) -> usize {
        self.term.columns()
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    /// The window title set by OSC 0 or 2.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// True once after the title changes.
    pub fn take_title_changed(&mut self) -> bool {
        std::mem::take(&mut self.title_changed)
    }

    /// True once after a BEL.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// DECCKM: arrows send `ESC O A` instead of `ESC [ A`.
    pub fn app_cursor(&self) -> bool {
        self.mode().contains(TermMode::APP_CURSOR)
    }

    /// DECSET 2004: wrap pastes in `ESC [200~` and `ESC [201~`.
    pub fn bracketed_paste(&self) -> bool {
        self.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// DECSET 1004: report focus changes with `ESC [I` and `ESC [O`.
    pub fn focus_reporting(&self) -> bool {
        self.mode().contains(TermMode::FOCUS_IN_OUT)
    }

    /// The alternate screen (DECSET 1049) is active. Full-screen programs use
    /// it; TerminalView.tsx calls this "tui" mode.
    pub fn alt_screen(&self) -> bool {
        self.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn mouse_mode(&self) -> MouseMode {
        let mode = self.mode();
        MouseMode {
            click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            drag: mode.contains(TermMode::MOUSE_DRAG),
            motion: mode.contains(TermMode::MOUSE_MOTION),
            sgr: mode.contains(TermMode::SGR_MOUSE),
            utf8: mode.contains(TermMode::UTF8_MOUSE),
        }
    }

    /// Lines scrolled up into history. 0 means the live bottom.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Lines of history above the screen.
    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Scroll the viewport. Positive moves up into history.
    pub fn scroll(&mut self, lines: i32) {
        if lines != 0 {
            self.term.scroll_display(Scroll::Delta(lines));
        }
    }

    pub fn scroll_page_up(&mut self) {
        self.term.scroll_display(Scroll::PageUp);
    }

    pub fn scroll_page_down(&mut self) {
        self.term.scroll_display(Scroll::PageDown);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn scroll_to_top(&mut self) {
        self.term.scroll_display(Scroll::Top);
    }

    /// Port of xterm.js `Terminal.clear()`, which TerminalView.tsx runs for
    /// Cmd+K on macOS: the cursor's line becomes the first line and the
    /// scrollback is dropped.
    pub fn clear(&mut self) {
        let cursor = self.term.grid().cursor.point;
        if cursor.line.0 > 0 {
            self.term.scroll_up(cursor.line.0 as usize);
            self.term.goto(0, cursor.column.0);
        }
        self.term.clear_screen(ClearMode::Saved);
        self.term.scroll_display(Scroll::Bottom);
        self.term.selection = None;
    }

    /// Write the notice TerminalView.tsx prints when the shell exits.
    pub fn write_exit_notice(&mut self, code: Option<i32>) {
        let status = code.map(|c| format!(" ({c})")).unwrap_or_default();
        self.feed(format!("\r\n[process exited{status}]\r\n").as_bytes());
    }

    /// The cursor's viewport cell, if it is on screen. Hidden cursors still
    /// report a cell, for placing IME candidate windows.
    pub fn cursor_cell(&self) -> Option<(usize, usize)> {
        let point = self.term.grid().cursor.point;
        let row = self.viewport_row(point.line)?;
        Some((row, point.column.0.min(self.cols().saturating_sub(1))))
    }

    /// Tell the terminal whether it has focus.
    pub fn set_focused(&mut self, focused: bool) {
        self.term.is_focused = focused;
    }

    // Selection. `Term` owns it and moves its anchors when output scrolls the
    // grid, so a selection stays on its text.

    /// The grid point under a viewport cell (row 0 is the top of the view).
    pub fn grid_point(&self, row: usize, col: usize) -> GridPoint {
        Point::new(
            Line(row as i32 - self.display_offset() as i32),
            Column(col.min(self.cols().saturating_sub(1))),
        )
    }

    /// The viewport row of a grid line, if it is on screen.
    pub fn viewport_row(&self, line: Line) -> Option<usize> {
        let row = line.0 + self.display_offset() as i32;
        (row >= 0 && (row as usize) < self.rows()).then_some(row as usize)
    }

    /// Start a selection. `Simple` for a drag, `Semantic` for a word
    /// (double click), `Lines` for a line (triple click).
    pub fn start_selection(&mut self, ty: SelectionType, point: GridPoint, side: Side) {
        self.term.selection = Some(Selection::new(ty, point, side));
    }

    /// Move the moving end of the selection.
    pub fn update_selection(&mut self, point: GridPoint, side: Side) {
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn select_all(&mut self) {
        let grid = self.term.grid();
        let start = Point::new(grid.topmost_line(), Column(0));
        let end = Point::new(grid.bottommost_line(), grid.last_column());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
    }

    fn selection_range(&self) -> Option<SelectionRange> {
        self.term.selection.as_ref()?.to_range(&self.term)
    }

    /// Whether a selection covers at least one cell.
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// The selected text, or `None` when nothing is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    /// A viewport row as text with trailing spaces trimmed. Wide-character
    /// spacers are skipped.
    pub fn row_text(&self, row: usize) -> String {
        let line = Line(row as i32 - self.display_offset() as i32);
        let mut text: String = self.term.grid()[line][..]
            .iter()
            .filter(|cell| !cell.flags.intersects(spacer_flags()))
            .map(|cell| cell.c)
            .collect();
        text.truncate(text.trim_end().len());
        text
    }

    /// The whole screen as text, one row per line, for tests and logs.
    pub fn screen_text(&self) -> String {
        (0..self.rows())
            .map(|row| self.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }

    /// Resolve the visible grid for painting.
    pub fn frame(&self) -> Frame {
        let content = self.term.renderable_content();
        let cols = self.cols();
        let rows = self.rows();
        let offset = content.display_offset;
        let colors = content.colors;
        let theme = &self.theme;
        let override_rgb = |index: usize| colors[index].map(|c| rgb_u8(c.r, c.g, c.b));
        let foreground = override_rgb(NamedColor::Foreground as usize).unwrap_or(theme.foreground);
        let background = override_rgb(NamedColor::Background as usize).unwrap_or(theme.background);
        let cursor_color = override_rgb(NamedColor::Cursor as usize).unwrap_or(theme.cursor);
        let resolve = |color: AnsiColor, bold: bool| -> Option<Rgba> {
            match color {
                AnsiColor::Spec(rgb) => Some(rgb_u8(rgb.r, rgb.g, rgb.b)),
                AnsiColor::Indexed(index) => {
                    // xterm.js drawBoldTextInBrightColors (on by default).
                    let index = if bold && index < 8 { index + 8 } else { index };
                    Some(override_rgb(index as usize).unwrap_or_else(|| theme.indexed(index)))
                }
                AnsiColor::Named(named) => {
                    let index = named as usize;
                    match named {
                        NamedColor::Foreground
                        | NamedColor::BrightForeground
                        | NamedColor::DimForeground
                        | NamedColor::Cursor => Some(foreground),
                        NamedColor::Background => None,
                        _ if index < 16 => {
                            let index = if bold && index < 8 { index + 8 } else { index };
                            Some(override_rgb(index).unwrap_or(theme.ansi[index]))
                        }
                        // DimBlack to DimWhite: the base color. The DIM flag
                        // dims it at paint time.
                        _ => {
                            let base = index - NamedColor::DimBlack as usize;
                            Some(override_rgb(base).unwrap_or(theme.ansi[base % 16]))
                        }
                    }
                }
            }
        };

        let selection = content.selection;
        let mut cells = vec![blank_cell(foreground); rows * cols];
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset as i32;
            if row < 0 || row as usize >= rows {
                continue;
            }
            let (row, col) = (row as usize, indexed.point.column.0);
            if col >= cols {
                continue;
            }
            let cell: &Cell = indexed.cell;
            let flags = cell.flags;
            let bold = flags.contains(Flags::BOLD);
            let mut fg = resolve(cell.fg, bold).unwrap_or(foreground);
            let mut bg = resolve(cell.bg, false);
            if flags.contains(Flags::INVERSE) {
                let new_fg = bg.unwrap_or_else(|| opaque(background));
                bg = Some(fg);
                fg = new_fg;
            }
            if flags.contains(Flags::DIM) {
                // xterm.js DIM_OPACITY.
                fg.a *= 0.5;
            }
            if flags.contains(Flags::HIDDEN) {
                fg.a = 0.0;
            }
            let underline = if flags.contains(Flags::UNDERCURL) {
                Some(UnderlineKind::Curly)
            } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
                Some(UnderlineKind::Double)
            } else if flags.contains(Flags::DOTTED_UNDERLINE) {
                Some(UnderlineKind::Dotted)
            } else if flags.contains(Flags::DASHED_UNDERLINE) {
                Some(UnderlineKind::Dashed)
            } else if flags.contains(Flags::UNDERLINE) {
                Some(UnderlineKind::Single)
            } else {
                None
            };
            cells[row * cols + col] = FrameCell {
                ch: cell.c,
                zerowidth: cell.zerowidth().map(|chars| chars.into()),
                fg,
                bg,
                bold,
                italic: flags.contains(Flags::ITALIC),
                underline,
                underline_color: cell
                    .underline_color()
                    .and_then(|color| resolve(color, false)),
                strikeout: flags.contains(Flags::STRIKEOUT),
                wide: flags.contains(Flags::WIDE_CHAR),
                spacer: flags.intersects(spacer_flags()),
                selected: selection.is_some_and(|range| range.contains(indexed.point)),
            };
        }

        let cursor = {
            let point = content.cursor.point;
            let row = point.line.0 + offset as i32;
            let shape = content.cursor.shape;
            (shape != CursorShape::Hidden && row >= 0 && (row as usize) < rows).then(|| {
                let cell = &self.term.grid()[point];
                FrameCursor {
                    row: row as usize,
                    col: point.column.0.min(cols.saturating_sub(1)),
                    shape,
                    blinking: self.term.cursor_style().blinking,
                    wide: cell.flags.contains(Flags::WIDE_CHAR),
                }
            })
        };

        Frame {
            cols,
            rows,
            cells,
            cursor,
            background,
            foreground,
            cursor_color,
            display_offset: offset,
            history_size: self.history_size(),
            alt_screen: self.alt_screen(),
        }
    }

    /// The link at a grid point: an OSC 8 hyperlink if the cell has one,
    /// otherwise an http or https URL in the text around it. URLs that wrap
    /// across lines are found as one link.
    pub fn link_at(&self, point: GridPoint) -> Option<Link> {
        let grid = self.term.grid();
        if point.line < grid.topmost_line() || point.line > grid.bottommost_line() {
            return None;
        }
        let last_col = grid.last_column();
        let wraps = |line: Line| grid[line][last_col].flags.contains(Flags::WRAPLINE);

        // The logical line: rows joined by soft wraps.
        let mut first = point.line;
        while first > grid.topmost_line() && wraps(first - 1) {
            first -= 1;
        }
        let mut last = point.line;
        while last < grid.bottommost_line() && wraps(last) {
            last += 1;
        }

        if let Some(hyperlink) = grid[point].hyperlink() {
            let same = |p: Point| grid[p].hyperlink().as_ref() == Some(&hyperlink);
            let mut start = point;
            loop {
                let prev = if start.column.0 > 0 {
                    Point::new(start.line, start.column - 1)
                } else if start.line > first {
                    Point::new(start.line - 1, last_col)
                } else {
                    break;
                };
                if !same(prev) {
                    break;
                }
                start = prev;
            }
            let mut end = point;
            loop {
                let next = if end.column < last_col {
                    Point::new(end.line, end.column + 1)
                } else if end.line < last {
                    Point::new(end.line + 1, Column(0))
                } else {
                    break;
                };
                if !same(next) {
                    break;
                }
                end = next;
            }
            return Some(Link {
                uri: hyperlink.uri().to_string(),
                start,
                end,
            });
        }

        // Text of the logical line, with the grid point of every character.
        let mut text = String::new();
        let mut points: Vec<(usize, Point)> = Vec::new();
        let mut line = first;
        loop {
            for col in 0..=last_col.0 {
                let p = Point::new(line, Column(col));
                let cell = &grid[p];
                if cell.flags.intersects(spacer_flags()) {
                    continue;
                }
                points.push((text.len(), p));
                text.push(cell.c);
            }
            if line == last {
                break;
            }
            line += 1;
        }
        let point_at = |byte: usize| {
            let ix = points.partition_point(|(b, _)| *b <= byte);
            points[ix.saturating_sub(1)].1
        };
        url_regex().find_iter(&text).find_map(|found| {
            let start = point_at(found.start());
            let end = point_at(found.end() - 1);
            let link = Link {
                uri: found.as_str().to_string(),
                start,
                end,
            };
            link.contains(point).then_some(link)
        })
    }
}

fn spacer_flags() -> Flags {
    Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER
}

fn blank_cell(fg: Rgba) -> FrameCell {
    FrameCell {
        ch: ' ',
        zerowidth: None,
        fg,
        bg: None,
        bold: false,
        italic: false,
        underline: None,
        underline_color: None,
        strikeout: false,
        wide: false,
        spacer: false,
        selected: false,
    }
}

impl std::fmt::Debug for Emulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Emulator")
            .field("cols", &self.cols())
            .field("rows", &self.rows())
            .field("display_offset", &self.display_offset())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emu(cols: u16, rows: u16) -> Emulator {
        Emulator::new(GridSize::new(cols, rows), TerminalTheme::dark())
    }

    fn cursor(e: &Emulator) -> Option<(usize, usize)> {
        e.frame().cursor.map(|c| (c.row, c.col))
    }

    #[test]
    fn plain_text_and_line_breaks() {
        let mut e = emu(20, 5);
        e.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(e.screen_text(), "one\ntwo\nthree");
        assert_eq!(cursor(&e), Some((2, 5)));
        e.feed(b"\rXX");
        assert_eq!(e.row_text(2), "XXree");
    }

    #[test]
    fn long_lines_wrap() {
        let mut e = emu(10, 4);
        e.feed(b"abcdefghijKLM");
        assert_eq!(e.row_text(0), "abcdefghij");
        assert_eq!(e.row_text(1), "KLM");
    }

    #[test]
    fn utf8_split_across_feeds() {
        let mut e = emu(10, 2);
        let bytes = "é".as_bytes();
        e.feed(&bytes[..1]);
        e.feed(&bytes[1..]);
        assert_eq!(e.row_text(0), "é");
    }

    #[test]
    fn sgr_colors_resolve_against_the_theme() {
        let theme = TerminalTheme::dark();
        let mut e = emu(40, 2);
        e.feed(b"\x1b[31mR\x1b[0m \x1b[44mB\x1b[0m\x1b[38;5;196mX\x1b[38;2;10;20;30mT");
        let frame = e.frame();
        let row = frame.row(0);
        assert_eq!(row[0].fg, theme.ansi[1]);
        assert_eq!(row[0].bg, None);
        assert_eq!(row[1].fg, theme.foreground);
        assert_eq!(row[2].bg, Some(theme.ansi[4]));
        assert_eq!(to_u8(row[3].fg), (255, 0, 0));
        assert_eq!(to_u8(row[4].fg), (10, 20, 30));
    }

    #[test]
    fn bold_draws_low_ansi_colors_bright() {
        let theme = TerminalTheme::dark();
        let mut e = emu(20, 2);
        e.feed(b"\x1b[1;32mG\x1b[0m\x1b[1;38;5;2mH\x1b[0m\x1b[1;92mI");
        let frame = e.frame();
        let row = frame.row(0);
        assert!(row[0].bold);
        assert_eq!(row[0].fg, theme.ansi[10]);
        assert_eq!(row[1].fg, theme.ansi[10]);
        assert_eq!(row[2].fg, theme.ansi[10]);
    }

    #[test]
    fn inverse_dim_hidden_italic_underline_strikeout() {
        let theme = TerminalTheme::dark();
        let mut e = emu(20, 2);
        e.feed(b"\x1b[7mI\x1b[0m\x1b[2mD\x1b[0m\x1b[8mH\x1b[0m\x1b[3mT\x1b[0m\x1b[4mU\x1b[0m\x1b[9mS\x1b[0m\x1b[4:3mC");
        let frame = e.frame();
        let row = frame.row(0);
        // Inverse on default colors: text in the opaque default background
        // on a foreground fill, as xterm.js draws it.
        assert_eq!(row[0].bg, Some(theme.foreground));
        assert_eq!(row[0].fg, opaque(theme.background));
        assert!((row[1].fg.a - 0.5).abs() < 1e-6);
        assert_eq!(row[2].fg.a, 0.0);
        assert!(row[3].italic);
        assert_eq!(row[4].underline, Some(UnderlineKind::Single));
        assert!(row[5].strikeout);
        assert_eq!(row[6].underline, Some(UnderlineKind::Curly));
    }

    #[test]
    fn cursor_addressing_shape_and_visibility() {
        let mut e = emu(20, 6);
        e.feed(b"\x1b[3;5Hx");
        assert_eq!(e.frame().row(2)[4].ch, 'x');
        assert_eq!(cursor(&e), Some((2, 5)));
        let c = e.frame().cursor.unwrap();
        assert_eq!(c.shape, CursorShape::Beam, "TerminalView.tsx uses a bar");
        assert!(c.blinking);
        e.feed(b"\x1b[2 q");
        let c = e.frame().cursor.unwrap();
        assert_eq!(c.shape, CursorShape::Block);
        assert!(!c.blinking);
        e.feed(b"\x1b[?25l");
        assert_eq!(e.frame().cursor, None);
        e.feed(b"\x1b[?25h");
        assert!(e.frame().cursor.is_some());
    }

    #[test]
    fn erase_and_clear_screen() {
        let mut e = emu(20, 4);
        e.feed(b"abcdef\x1b[3D\x1b[K");
        assert_eq!(e.row_text(0), "abc");
        e.feed(b"\r\nbbb\r\nccc\x1b[2J\x1b[H");
        assert_eq!(e.screen_text(), "");
        assert_eq!(cursor(&e), Some((0, 0)));
    }

    #[test]
    fn scrollback_and_viewport_scrolling() {
        let mut e = emu(10, 3);
        for i in 1..=8 {
            e.feed(format!("line{i}\r\n").as_bytes());
        }
        assert_eq!(e.row_text(0), "line7");
        assert_eq!(e.history_size(), 6);
        e.scroll(2);
        assert_eq!(e.display_offset(), 2);
        assert_eq!(e.row_text(0), "line5");
        assert_eq!(e.frame().cursor, None, "cursor is below the scrolled view");
        e.scroll(100);
        assert_eq!(e.row_text(0), "line1");
        e.scroll_to_bottom();
        assert_eq!(e.display_offset(), 0);
    }

    #[test]
    fn scrollback_is_capped_by_the_theme() {
        let theme = TerminalTheme {
            scrollback: 4,
            ..TerminalTheme::dark()
        };
        let mut e = Emulator::new(GridSize::new(10, 2), theme);
        for i in 0..20 {
            e.feed(format!("{i}\r\n").as_bytes());
        }
        assert_eq!(e.history_size(), 4);
    }

    #[test]
    fn alt_screen_restores_the_primary_screen() {
        let mut e = emu(20, 4);
        e.feed(b"primary");
        assert!(!e.alt_screen());
        e.feed(b"\x1b[?1049h\x1b[Halt");
        assert!(e.alt_screen());
        assert_eq!(e.row_text(0), "alt");
        e.feed(b"\x1b[?1049l");
        assert!(!e.alt_screen());
        assert_eq!(e.row_text(0), "primary");
    }

    #[test]
    fn mode_flags_follow_decset() {
        let mut e = emu(10, 2);
        assert!(!e.app_cursor());
        e.feed(b"\x1b[?1h");
        assert!(e.app_cursor());
        e.feed(b"\x1b[?1l\x1b[?2004h\x1b[?1004h");
        assert!(!e.app_cursor());
        assert!(e.bracketed_paste());
        assert!(e.focus_reporting());
        assert!(!e.mouse_mode().reporting());
        e.feed(b"\x1b[?1000h\x1b[?1006h");
        let mouse = e.mouse_mode();
        assert!(mouse.click && mouse.sgr && !mouse.drag && !mouse.motion);
        e.feed(b"\x1b[?1002h");
        assert!(e.mouse_mode().drag);
        e.feed(b"\x1b[?1003h");
        assert!(e.mouse_mode().motion);
    }

    #[test]
    fn device_status_reply() {
        let mut e = emu(20, 4);
        e.feed(b"\x1b[2;3H");
        assert_eq!(e.feed(b"\x1b[6n"), b"\x1b[2;3R".to_vec());
    }

    #[test]
    fn osc_color_queries_answer_with_theme_colors() {
        let mut e = emu(20, 2);
        // OSC 10, 11, 12 with the ST terminator, the form oscColorReply
        // writes in terminalChrome.ts.
        assert_eq!(
            String::from_utf8(e.feed(b"\x1b]10;?\x1b\\")).unwrap(),
            "\x1b]10;rgb:ebeb/ebeb/ebeb\x1b\\"
        );
        assert_eq!(
            String::from_utf8(e.feed(b"\x1b]11;?\x1b\\")).unwrap(),
            "\x1b]11;rgb:1717/1717/1717\x1b\\"
        );
        assert_eq!(
            String::from_utf8(e.feed(b"\x1b]12;?\x1b\\")).unwrap(),
            "\x1b]12;rgb:4545/9b9b/f7f7\x1b\\"
        );
        // BEL-terminated queries get a BEL-terminated reply.
        assert_eq!(
            String::from_utf8(e.feed(b"\x1b]10;?\x07")).unwrap(),
            "\x1b]10;rgb:ebeb/ebeb/ebeb\x07"
        );
    }

    #[test]
    fn title_and_bell() {
        let mut e = emu(20, 2);
        assert_eq!(e.title(), None);
        e.feed(b"\x1b]0;my title\x07");
        assert_eq!(e.title(), Some("my title"));
        assert!(e.take_title_changed());
        assert!(!e.take_title_changed());
        assert!(!e.take_bell());
        e.feed(b"\x07");
        assert!(e.take_bell());
        assert!(!e.take_bell());
    }

    #[test]
    fn resize_keeps_content() {
        let mut e = emu(20, 5);
        e.feed(b"keep\r\nsecond");
        e.resize(GridSize::new(30, 3));
        assert_eq!((e.cols(), e.rows()), (30, 3));
        assert_eq!(e.row_text(0), "keep");
        assert_eq!(e.row_text(1), "second");
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let mut e = emu(10, 2);
        e.feed("宽w".as_bytes());
        let frame = e.frame();
        let row = frame.row(0);
        assert!(row[0].wide);
        assert_eq!(row[0].ch, '宽');
        assert!(row[1].spacer);
        assert_eq!(row[2].ch, 'w');
        assert_eq!(e.row_text(0), "宽w");
        assert_eq!(cursor(&e), Some((0, 3)));
    }

    #[test]
    fn combining_marks_stay_on_their_cell() {
        let mut e = emu(10, 2);
        e.feed("e\u{301}x".as_bytes());
        let frame = e.frame();
        assert_eq!(frame.row(0)[0].zerowidth.as_deref(), Some(&['\u{301}'][..]));
        assert_eq!(frame.row(0)[1].ch, 'x');
    }

    #[test]
    fn drag_word_and_line_selection() {
        let mut e = emu(20, 3);
        e.feed(b"hello world\r\nsecond row");
        e.start_selection(SelectionType::Simple, e.grid_point(0, 0), Side::Left);
        assert!(!e.has_selection(), "a click without a drag selects nothing");
        e.update_selection(e.grid_point(0, 4), Side::Right);
        assert_eq!(e.selection_text().as_deref(), Some("hello"));
        let frame = e.frame();
        assert!(frame.row(0)[..5].iter().all(|c| c.selected));
        assert!(!frame.row(0)[5].selected);

        e.start_selection(SelectionType::Semantic, e.grid_point(0, 7), Side::Left);
        assert_eq!(e.selection_text().as_deref(), Some("world"));

        e.start_selection(SelectionType::Lines, e.grid_point(1, 2), Side::Left);
        assert_eq!(e.selection_text().as_deref(), Some("second row\n"));

        e.start_selection(SelectionType::Simple, e.grid_point(0, 0), Side::Left);
        e.update_selection(e.grid_point(1, 5), Side::Right);
        assert_eq!(e.selection_text().as_deref(), Some("hello world\nsecond"));

        e.clear_selection();
        assert!(!e.has_selection());
        e.select_all();
        assert_eq!(
            e.selection_text().as_deref(),
            Some("hello world\nsecond row\n")
        );
    }

    #[test]
    fn selection_follows_text_when_output_scrolls() {
        let mut e = emu(10, 3);
        e.feed(b"target\r\n");
        e.start_selection(SelectionType::Simple, e.grid_point(0, 0), Side::Left);
        e.update_selection(e.grid_point(0, 5), Side::Right);
        e.feed(b"a\r\nb\r\nc\r\n");
        assert_eq!(e.selection_text().as_deref(), Some("target"));
    }

    #[test]
    fn clear_keeps_the_cursor_line_and_drops_history() {
        let mut e = emu(20, 4);
        for i in 0..10 {
            e.feed(format!("old{i}\r\n").as_bytes());
        }
        e.feed(b"$ prompt");
        assert!(e.history_size() > 0);
        e.clear();
        assert_eq!(e.history_size(), 0);
        assert_eq!(e.screen_text(), "$ prompt");
        assert_eq!(cursor(&e), Some((0, 8)));
    }

    #[test]
    fn exit_notice_matches_terminal_view_tsx() {
        let mut e = emu(40, 4);
        e.feed(b"$ exit");
        e.write_exit_notice(Some(0));
        assert_eq!(e.row_text(1), "[process exited (0)]");
        e.write_exit_notice(None);
        assert_eq!(e.row_text(2), "[process exited]");
    }

    #[test]
    fn urls_are_found_in_text_and_across_wraps() {
        let mut e = emu(20, 4);
        e.feed(b"see https://example.com/a?b=1, ok");
        let link = e.link_at(e.grid_point(0, 10)).unwrap();
        assert_eq!(link.uri, "https://example.com/a?b=1");
        assert_eq!(link.start, e.grid_point(0, 4));
        assert_eq!(link.end, e.grid_point(1, 8));
        assert!(e.link_at(e.grid_point(0, 1)).is_none());
        // The trailing comma is not part of the link.
        assert!(e.link_at(e.grid_point(1, 9)).is_none());
    }

    #[test]
    fn osc8_hyperlinks_win_over_text() {
        let mut e = emu(30, 2);
        e.feed(b"go \x1b]8;;https://monocode.dev/docs\x1b\\docs here\x1b]8;;\x1b\\ end");
        let link = e.link_at(e.grid_point(0, 5)).unwrap();
        assert_eq!(link.uri, "https://monocode.dev/docs");
        assert_eq!(link.start, e.grid_point(0, 3));
        assert_eq!(link.end, e.grid_point(0, 11));
        assert!(e.link_at(e.grid_point(0, 13)).is_none());
    }
}
