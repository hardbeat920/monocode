//! Transcript cards: link previews, question form, prompt outline, find bar,
//! tool diff previews, approval toasts, the selection menu, plan and task
//! previews, generated images, the note card, the Markdown mode toggle and
//! document preview, the terminal spinner, and the one-shot bursts on fresh
//! prompts. The orchestration card is `crate::threads::OrchestrationPreview`.
//!
//! Each card takes plain data and reports what the reader did, either as
//! events on its entity or through callbacks on its element. None of them
//! depend on the engine.

pub mod approval_toasts;
pub mod celebration;
pub mod find_bar;
pub mod generated_image;
pub mod link_preview;
pub mod markdown_document;
pub mod markdown_mode;
pub mod note_card;
pub mod particle_text;
pub mod plan_preview;
pub mod prompt_outline;
pub mod prompt_outline_model;
pub mod question_form;
pub mod selection_menu;
pub mod spinner;
pub mod style;
pub mod task_list;
pub mod tool_diff;
pub mod util;

pub use approval_toasts::{ApprovalNotice, ApprovalToastEvent, ApprovalToasts, NoticeKind};
pub use celebration::{BurstKind, CelebrationBurst, Celebrations, celebration_burst};
pub use find_bar::{FindSide, TranscriptFind, TranscriptFindEvent};
pub use generated_image::{GeneratedImage, GeneratedImageState};
pub use link_preview::{
    LinkPreviewMetadata, LinkPreviewRequest, LinkPreviews, LinkWorkItem, LinkWorkItemDetails,
    UserLinkPreview, UserLinkPreviewEvent,
};
pub use markdown_document::{MarkdownDocumentEvent, MarkdownDocumentPreview};
pub use markdown_mode::{
    MarkdownModeToggle, MarkdownViewMode, MarkdownViewShell, markdown_mode, markdown_mode_toggle,
    markdown_view_shell, set_markdown_mode,
};
pub use note_card::{NoteCard, NoteProject, note_card};
pub use particle_text::{ParticleText, particle_text};
pub use plan_preview::{PlanPreview, plan_preview};
pub use prompt_outline::{PromptOutline, PromptOutlineEvent, jump_to_prompt};
pub use question_form::{QuestionForm, QuestionFormEvent};
pub use selection_menu::{
    SelectionAction, SelectionMenuEvent, TranscriptSelection, TranscriptSelectionMenu,
};
pub use spinner::{TerminalSpinner, terminal_spinner};
pub use task_list::{TaskListPreview, task_list_preview};
pub use tool_diff::{ToolDiffEvent, ToolDiffPopover, ToolDiffPreview};

/// What a card inside the transcript asks the host to do, beyond
/// [`crate::transcript::TranscriptEvent`]. Subscribe to it on the
/// [`crate::transcript::TranscriptView`] entity.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptCardEvent {
    /// Quote the selected text in the composer ("Add to chat").
    AddToChat { text: String },
}

#[cfg(test)]
mod tests;
