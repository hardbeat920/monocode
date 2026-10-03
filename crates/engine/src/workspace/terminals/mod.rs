//! Terminals: the PTYs behind terminal files (`pty`) and the per-project
//! docks of a window (`project_terminals`).

pub mod project_terminals;
pub mod pty;

pub use project_terminals::{DockToggle, ProjectTerminals};
pub use pty::{EnginePty, HostPty, PtyBackend, TerminalMetaChanged, TerminalSignal, Terminals};
