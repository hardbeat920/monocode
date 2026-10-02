//! Port of the pure parts of src/features/terminal/model/terminalLayout.ts.
//!
//! The TypeScript measured an xterm.js renderer and resized it in place. The
//! GPUI terminal view owns its renderer, so this module keeps the gutter and
//! grid arithmetic: it takes the host size and cell size and returns the
//! grid, the way `fitTerminal` computed it before calling `term.resize`.

/// `TerminalFitMode`: a shell keeps a scrollbar gutter, a full-screen TUI
/// covers the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFitMode {
    Shell,
    Tui,
}

/// `DEFAULT_SCROLLBAR_WIDTH`.
pub const DEFAULT_SCROLLBAR_WIDTH: f64 = 14.0;
/// `MIN_TUI_SCROLLBAR_WIDTH`.
pub const MIN_TUI_SCROLLBAR_WIDTH: f64 = 1.0;

/// `terminalScrollbarWidth`: the overview ruler width, 14px when unset.
pub fn terminal_scrollbar_width(overview_ruler_width: Option<f64>) -> f64 {
    overview_ruler_width.unwrap_or(DEFAULT_SCROLLBAR_WIDTH)
}

/// A width and height in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// `availableSize`: the host minus the scrollbar gutter, or `None` when it
/// is too small to hold a grid.
pub fn available_size(
    client_width: f64,
    client_height: f64,
    mode: TerminalFitMode,
    overview_ruler_width: Option<f64>,
) -> Option<Size> {
    let gutter = match mode {
        TerminalFitMode::Tui => MIN_TUI_SCROLLBAR_WIDTH,
        TerminalFitMode::Shell => terminal_scrollbar_width(overview_ruler_width),
    };
    let width = client_width - gutter;
    let height = client_height;
    if width < 8.0 || height < 8.0 {
        return None;
    }
    Some(Size { width, height })
}

/// The grid `fitTerminal` asked for before resizing: a TUI rounds up so the
/// grid covers the host, a shell rounds down. `None` when the cell size is
/// not measured yet.
pub fn fit_grid(size: Size, cell: Size, mode: TerminalFitMode) -> Option<(u32, u32)> {
    if cell.width < 1.0 || cell.height < 1.0 {
        return None;
    }
    let round = |value: f64| match mode {
        TerminalFitMode::Tui => value.ceil(),
        TerminalFitMode::Shell => value.floor(),
    };
    let cols = round(size.width / cell.width).max(2.0);
    let rows = round(size.height / cell.height).max(1.0);
    Some((cols as u32, rows as u32))
}

/// The overview ruler width `applyTerminalChrome` set: thin in the
/// alternate screen, the default otherwise.
pub fn chrome_overview_ruler_width(tui: bool) -> Option<f64> {
    tui.then_some(MIN_TUI_SCROLLBAR_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_scrollbar_width_defaults_to_14px_when_overview_ruler_width_is_unset() {
        assert_eq!(terminal_scrollbar_width(None), 14.0);
    }

    #[test]
    fn terminal_scrollbar_width_honors_an_explicit_overview_ruler_width() {
        assert_eq!(terminal_scrollbar_width(Some(1.0)), 1.0);
        assert_eq!(terminal_scrollbar_width(Some(0.0)), 0.0);
    }

    #[test]
    fn fits_a_grid_inside_the_gutter() {
        let size = available_size(814.0, 400.0, TerminalFitMode::Shell, None).unwrap();
        assert_eq!(
            size,
            Size {
                width: 800.0,
                height: 400.0
            }
        );
        let cell = Size {
            width: 7.5,
            height: 17.0,
        };
        assert_eq!(
            fit_grid(size, cell, TerminalFitMode::Shell),
            Some((106, 23))
        );
        assert_eq!(fit_grid(size, cell, TerminalFitMode::Tui), Some((107, 24)));
        assert_eq!(
            available_size(10.0, 400.0, TerminalFitMode::Shell, None),
            None
        );
        assert_eq!(
            fit_grid(
                size,
                Size {
                    width: 0.5,
                    height: 17.0
                },
                TerminalFitMode::Shell
            ),
            None
        );
        assert_eq!(chrome_overview_ruler_width(true), Some(1.0));
        assert_eq!(chrome_overview_ruler_width(false), None);
    }
}
