//! Secondary full-page views: automations, notes, search, MCP settings,
//! skills, and the date and time picker.
//!
//! Each page takes its data and actions through a small trait the engine
//! implements: [`automations::AutomationsData`], [`notes::NotesData`],
//! [`search::SearchData`], [`mcp::McpData`], and [`skills::SkillsData`].
//! Projects (labels, logos, mascots, and the rail order the project picker
//! offers) come from [`data::ProjectsData`]. None of them names
//! `monocode-engine`; the types mirror the engine's with the same JSON
//! shapes.

pub mod automations;
pub mod data;
pub mod date_time_picker;
pub mod format;
pub mod mcp;
pub mod notes;
pub mod search;
pub mod skills;
pub mod widgets;

#[cfg(test)]
mod test_support;

pub use data::{DataTask, Listener, ProjectMark, ProjectsData, StaticProjects};

/// Installs the markdown renderer's key bindings and the pickers' bindings
/// the pages use. Call once at startup, after `gpui_component::init` and
/// `monocode_ui::init`.
pub fn init(cx: &mut gpui::App) {
    monocode_markdown::init(cx);
    monocode_view_composer::pickers::init(cx);
}
