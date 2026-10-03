//! The session composer. Port of src/features/sessions/ui/Composer.tsx and
//! the pieces around it: the prompt input with its highlights, attachments,
//! context chips, mode commands, the context meter, the runner mascot, and
//! the bottom bar with the send and stop button.
//!
//! The composer draws and handles input only. Everything it needs from the
//! engine goes through [`host::ComposerHost`].

pub mod host;
pub mod model;
pub mod prompt_input;
pub mod view;

pub use host::{
    ComposerHost, ComposerSubmission, FolderTarget, McpServers, NewSkillScope, ResendTicket,
    SessionFolder, SkillContext,
};
pub use view::{
    COMPOSER_MAX_HEIGHT, Composer, ComposerEvent, ComposerProps, LastTurnRecall, RemoteFeatures,
};

/// Binds the composer's keys. Call once at startup, after
/// `gpui_component::init`.
pub fn init(cx: &mut gpui::App) {
    prompt_input::init(cx);
}
