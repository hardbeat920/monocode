//! Port of src/features/sessions/model/sessionFolderCommand.ts: the local
//! `/add-to-folder` composer command.

use super::commands::ConsumedCommand;
use super::skills::BuiltinSkill;
use super::text::{leading_command, standalone_command};

/// `SESSION_FOLDER_COMMAND`.
pub const SESSION_FOLDER_COMMAND: BuiltinSkill = BuiltinSkill::new(
    "add-to-folder",
    "add-to-folder",
    "Place this session in an existing or new sidebar folder.",
);

/// `isSessionFolderCommand`: the standalone command, without prompt text.
pub fn is_session_folder_command(text: &str) -> bool {
    standalone_command(text, "add-to-folder")
}

/// `consumeSessionFolderCommand`: remove the leading local command and keep
/// the user's actual prompt.
pub fn consume_session_folder_command(text: &str) -> ConsumedCommand {
    match leading_command(text, &["add-to-folder"]) {
        Some(end) => ConsumedCommand {
            text: text[end..].to_string(),
            matched: true,
        },
        None => ConsumedCommand {
            text: text.to_string(),
            matched: false,
        },
    }
}

/// The input to `runsSessionFolderCommandOnSpace`. Selection offsets are
/// byte offsets.
#[derive(Debug, Clone, Default)]
pub struct SpaceKeyInput<'a> {
    pub text: &'a str,
    pub selection_start: usize,
    pub selection_end: usize,
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
}

/// `runsSessionFolderCommandOnSpace`: an unmodified space at the end of
/// `/add-to-folder` runs the command.
pub fn runs_session_folder_command_on_space(input: &SpaceKeyInput<'_>) -> bool {
    !input.alt_key
        && !input.ctrl_key
        && !input.meta_key
        && input.selection_start == input.text.len()
        && input.selection_end == input.text.len()
        // `/^\s*\/add-to-folder$/i`: nothing may follow the command.
        && leading_command(input.text, &["add-to-folder"]) == Some(input.text.len())
        && !input.text.ends_with(monocode_core::js::is_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_a_standalone_add_to_folder_command() {
        assert!(is_session_folder_command("/add-to-folder"));
        assert!(is_session_folder_command("  /ADD-TO-FOLDER\n"));
    }

    #[test]
    fn does_not_consume_ordinary_prompt_text() {
        assert!(!is_session_folder_command("/add-to-folder project"));
        assert!(!is_session_folder_command("mention /add-to-folder in docs"));
        assert!(!is_session_folder_command("/add-to-folder-later"));
    }

    #[test]
    fn removes_the_leading_command_and_keeps_the_agent_prompt() {
        assert_eq!(
            consume_session_folder_command("/add-to-folder   Build the settings screen"),
            ConsumedCommand {
                text: "Build the settings screen".into(),
                matched: true
            }
        );
        assert_eq!(
            consume_session_folder_command("Explain /add-to-folder"),
            ConsumedCommand {
                text: "Explain /add-to-folder".into(),
                matched: false
            }
        );
    }

    #[test]
    fn runs_on_an_unmodified_space_at_the_end_of_add_to_folder() {
        assert!(runs_session_folder_command_on_space(&SpaceKeyInput {
            text: "/add-to-folder",
            selection_start: 14,
            selection_end: 14,
            ..SpaceKeyInput::default()
        }));
    }

    #[test]
    fn leaves_edited_selections_and_modified_space_shortcuts_alone() {
        assert!(!runs_session_folder_command_on_space(&SpaceKeyInput {
            text: "/add-to-folder",
            selection_start: 0,
            selection_end: 14,
            ..SpaceKeyInput::default()
        }));
        assert!(!runs_session_folder_command_on_space(&SpaceKeyInput {
            text: "/add-to-folder ",
            selection_start: 15,
            selection_end: 15,
            ..SpaceKeyInput::default()
        }));
        assert!(!runs_session_folder_command_on_space(&SpaceKeyInput {
            text: "/add-to-folder",
            selection_start: 14,
            selection_end: 14,
            meta_key: true,
            ..SpaceKeyInput::default()
        }));
    }
}
