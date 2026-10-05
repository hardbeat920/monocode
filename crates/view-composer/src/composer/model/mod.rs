//! Pure composer logic, unit-tested without GPUI: token parsing, highlight
//! ranges, commands, chat context, clipboard payloads, the runner mascot's
//! motion, and the dock animation.

pub mod chat_context;
pub mod clipboard;
pub mod commands;
pub mod dock_motion;
pub mod fuzzy;
pub mod highlight;
pub mod mascots;
pub mod mcp;
pub mod mentions;
pub mod mode_commands;
pub mod paths;
pub mod quote_draft;
pub mod runner;
pub mod skills;
pub mod skills_context;
pub mod text;
