//! Pickers: model, skills, file mentions, MCP servers, access, session
//! folders, and the searchable select. Ports of the pickers in
//! src/features/sessions/ui and src/features/skills/ui, and of
//! SearchableSelect, Shimmer, and MatchText in src/shared/ui.
//!
//! Every picker takes its data as plain values and reports through
//! callbacks. None of them depends on `monocode-engine`; the composer turns
//! engine types into the row structs defined here.

pub mod access_picker;
pub mod anchor;
pub mod effort_tiles;
mod field;
pub mod file_mention_picker;
pub mod match_text;
pub mod mcp_server_picker;
pub mod model_flyout;
pub mod model_logic;
pub mod model_picker;
pub mod model_pills;
pub mod model_settings;
pub mod model_source;
pub mod searchable_select;
pub mod session_folder_picker;
pub mod shimmer;
pub mod skill_document_preview;
pub mod skill_picker;
pub mod skill_prompt_field;
mod style;
#[cfg(test)]
mod tests;

pub use access_picker::AccessPicker;
pub use anchor::DismissReason;
pub use file_mention_picker::{FileMentionPicker, MentionFile, file_mention_picker};
pub use match_text::{MatchText, match_text};
pub use mcp_server_picker::{McpAvailability, McpServerPicker, McpServerRow};
pub use model_picker::{MenuEntry, ModelPicker, ModelPickerProps, Submenu, SwitchModel};
pub use model_pills::ModelControlPills;
pub use model_settings::ModelSettingsView;
pub use model_source::{LocalModelSource, ModelSource};
pub use searchable_select::{SearchableSelect, SearchableSelectOption, SelectVariant};
pub use session_folder_picker::{SessionFolderPicker, SessionFolderRow, SessionFolderTarget};
pub use shimmer::{Shimmer, shimmer};
pub use skill_document_preview::SkillDocumentPreview;
pub use skill_picker::{
    CreateScope, CreateSkillForm, PickerSkill, SkillKind, SkillPicker, SkillScope, skill_picker,
};
pub use skill_prompt_field::{SkillCompletions, SkillPromptField, SkillTextPart, SlashToken};

use gpui::{App, KeyBinding};

/// Binds the pickers' keys. Call once at startup, after
/// `gpui_component::init`.
pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-.", SwitchModel, None)]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("ctrl-.", SwitchModel, None)]);
}
