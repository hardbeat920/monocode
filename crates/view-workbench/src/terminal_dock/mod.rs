//! The project terminal dock and the empty-session arcade background.
//!
//! - [`dock`]: `ProjectTerminalDock.tsx`, the docked terminals with their
//!   tab strip, resize sash, and side menu, plus [`dock::dock_grid`], the
//!   layout App.tsx gave the main area and the dock.
//! - [`grid_background`]: `TerminalGridBackground.tsx`, the arcade grid
//!   behind the empty-session composer.
//! - [`arcade`]: the games it plays.

pub mod arcade;
pub mod dock;
pub mod grid_background;

#[cfg(test)]
mod test_support;

pub use dock::{
    DockControls, DockControlsEvent, DockGridLayout, DockTerminals, TerminalDock,
    TerminalDockEvent, dock_grid, dock_grid_layout,
};
pub use grid_background::TerminalGridBackground;
