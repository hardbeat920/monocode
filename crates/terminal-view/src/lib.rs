//! Terminal emulator view for MonoCode: an `alacritty_terminal` grid drawn
//! with GPUI. Replaces src/features/terminal/ui/TerminalView.tsx (xterm.js).
//!
//! A host creates a [`TerminalView`] over something that implements [`Pty`],
//! puts it in its layout, and subscribes to [`TerminalEvent`]s:
//!
//! ```ignore
//! let terminal = cx.new(|cx| TerminalView::new(pty, TerminalTheme::dark(), window, cx));
//! cx.subscribe(&terminal, |_, _, event: &TerminalEvent, cx| {
//!     if let TerminalEvent::OpenUrl(url) = event {
//!         cx.open_url(url);
//!     }
//! })
//! .detach();
//! ```
//!
//! The pieces also work on their own: [`Emulator`] is the terminal state
//! without GPUI windows, and [`keys`] encodes keystrokes and mouse reports.

pub mod element;
pub mod emulator;
pub mod keys;
pub mod pty;
pub mod theme;
pub mod view;

pub use element::{CellHit, LayoutInfo, TerminalElement};
pub use emulator::{Emulator, Frame, FrameCell, FrameCursor, GridSize, Link, MouseMode};
pub use pty::{Pty, PtyEvent, PtySize, RecordingPty};
pub use theme::TerminalTheme;
pub use view::{
    Clear, Copy, KEY_CONTEXT, Paste, ScrollPageDown, ScrollPageUp, ScrollToBottom, ScrollToTop,
    SelectAll, TerminalEvent, TerminalView,
};
