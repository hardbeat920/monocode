//! Agent CLI processes: the harness supervisor, binary resolution, the login
//! shell environment, the loopback control server and its CLI client, MCP
//! config, and skill discovery. Moved from `src-tauri/src`.

#[cfg(target_os = "macos")]
pub mod claude_keychain;
pub mod control;
pub mod control_cli;
pub mod external_editor;
pub mod harness;
pub mod mcp;
pub mod skills;
pub mod worktree_lifecycle;
