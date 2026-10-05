//! What the composer needs from the engine. Composer.tsx took these as props
//! callbacks (`onSubmit`, `onStop`, `onSaveDraft`, `onBtwCommand`, ...) and
//! read the rest from module caches (the skill catalog, the file index, MCP
//! settings, the draft cache). Here one [`ComposerHost`] carries all of it,
//! so the engine wires a composer by implementing a single trait.
//!
//! Callbacks that only notify the owner (focus, model changes, queue edits)
//! are [`crate::composer::ComposerEvent`]s instead.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, Task, Window};
use monocode_core::session::{ComposerTurnOptions, EditedResendRejection};
use monocode_core::{Attachment, HarnessId, ModelPrefs, ProjectProviders};

use super::model::clipboard::ClipboardFile;
use super::model::mcp::{McpConnection, McpTag};
use super::model::mentions::{ProjectFile, RankedFile};
use super::model::skills::Skill;
use crate::pickers::ModelSource;

/// One message leaving the composer: `onSubmit(text, attachments, options)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposerSubmission {
    /// The text as the React composer passed it: MCP context line, the
    /// `/operator` prefix, the inbox prompt, and the chat context block
    /// already applied.
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub options: ComposerTurnOptions,
    /// Set for an edited resend. When the resend fails later, hand this back
    /// to `Composer::resend_rejected` (the TypeScript's `onResendRejected`).
    pub resend: Option<ResendTicket>,
}

/// Identifies an edited resend so a late rejection restores the right draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResendTicket(pub u64);

/// What a late resend rejection says. Re-exported for hosts.
pub type ResendRejection = EditedResendRejection;

/// The skill catalog's key: `SkillCatalogContext`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SkillContext {
    pub harness: HarnessId,
    /// Empty for a remote session: local indexes never read a host path.
    pub cwd: String,
    pub session_id: Option<String>,
}

/// `SkillScope` for a new skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewSkillScope {
    Project,
    User,
}

/// The MCP settings snapshot the picker shows (`McpSettingsSnapshot`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpServers {
    pub servers: Vec<McpConnection>,
    /// Claude's `mcp list` health text by server name.
    pub claude_status: HashMap<String, String>,
    pub loading: bool,
    pub error: String,
}

/// A sidebar folder the session can join (`SessionFolder`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFolder {
    pub id: String,
    pub name: String,
    pub session_count: usize,
}

/// `SessionFolderTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderTarget {
    Existing { folder_id: String },
    New { name: String },
}

/// Everything the composer asks the engine for. Methods with defaults are
/// the optional props of Composer.tsx; leaving one out hides the feature.
pub trait ComposerHost: 'static {
    // Turns.

    /// `onSubmit`. Return false to reject the turn before it is recorded;
    /// the composer then keeps the text, files, and mode.
    fn submit(&self, submission: ComposerSubmission, window: &mut Window, cx: &mut App) -> bool;

    /// `onStop`: the Stop button.
    fn stop(&self, _window: &mut Window, _cx: &mut App) {}

    /// `onSaveDraft`, for `/draft` and the Draft mode. Only called when the
    /// props allow saving drafts.
    fn save_draft(
        &self,
        _text: String,
        _attachments: Vec<Attachment>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> bool {
        false
    }

    /// `onBtwCommand`. `draft` opens the side question with the text unsent,
    /// for a typed `/btw `. Return false to reject.
    fn btw(&self, _text: String, _draft: bool, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// `onCompactContext`. Return true when compaction started.
    fn compact_context(&self, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// `onPlaceInFolder`.
    fn place_in_folder(&self, _target: FolderTarget, _window: &mut Window, _cx: &mut App) {}

    /// The sidebar folders for `/add-to-folder` (`loadSessionFolders`).
    fn session_folders(&self, _cwd: &str, _cx: &mut App) -> Vec<SessionFolder> {
        Vec::new()
    }

    // Drafts.

    /// `onDraftChange`: the draft with its context chips appended as a
    /// context block. The draft cache (`setComposerDraft`) lives behind it.
    fn draft_changed(&self, _text: &str, _cx: &mut App) {}

    /// `getComposerMcpTags`.
    fn load_mcp_tags(&self, _session_id: &str, _cx: &mut App) -> Vec<McpTag> {
        Vec::new()
    }

    /// `setComposerMcpTags`.
    fn save_mcp_tags(&self, _session_id: &str, _tags: &[McpTag], _cx: &mut App) {}

    // Attachments.

    /// `attachmentsFromPaths`: files dropped, picked, or copied in a file
    /// manager. Unreadable paths yield no attachment.
    fn attachments_from_paths(&self, paths: Vec<String>, cx: &mut App) -> Task<Vec<Attachment>>;

    /// `attachmentsFromFiles`: pasted bytes (a screenshot, files from a
    /// copied MonoCode message).
    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> Task<Vec<Attachment>>;

    /// `pickAttachments`: the open-file dialog.
    fn pick_attachments(&self, window: &mut Window, cx: &mut App) -> Task<Vec<Attachment>>;

    /// `revokeAttachment`: release what an attachment holds (a temp file, a
    /// preview). Borrowed attachments (a recalled turn's) are never revoked.
    fn revoke_attachment(&self, _attachment: &Attachment, _cx: &mut App) {}

    // Slash commands and skills.

    /// The skill catalog for this context (`useComposerSkills().skills`).
    /// Call `Composer::refresh_suggestions` when it changes.
    fn skills(&self, context: &SkillContext, cx: &mut App) -> Vec<Skill>;

    /// Reload the catalog: the picker opened, or a skill was created.
    /// `refresh` re-reads files on disk.
    fn reload_skills(&self, _context: &SkillContext, _refresh: bool, _cx: &mut App) {}

    /// `hasNativeCommands`: the provider reports its own slash commands.
    fn has_native_commands(&self, _harness: HarnessId) -> bool {
        false
    }

    /// The provider's commands take raw slash arguments
    /// (`commands.rawSlashCommands`).
    fn raw_slash_commands(&self, _harness: HarnessId) -> bool {
        false
    }

    /// `createBlankSkill`: writes a SKILL.md and returns its path.
    fn create_skill(
        &self,
        _cwd: &str,
        _name: &str,
        _scope: NewSkillScope,
        _cx: &mut App,
    ) -> Task<Result<String, String>> {
        Task::ready(Err("Creating skills is not available here.".into()))
    }

    // `@` mentions.

    /// The files and folders `@` can name in `cwd`, notes included (as
    /// `note:` paths) when notes are on. Feeds the highlight's label index.
    fn mention_files(&self, cwd: &str, cx: &mut App) -> Vec<ProjectFile>;

    /// `rankedFiles`: picker rows for `query`, notes first, then files
    /// (recents first without a query).
    fn rank_mentions(&self, cwd: &str, query: &str, cx: &mut App) -> Vec<RankedFile>;

    /// The project file index is still loading.
    fn mentions_loading(&self, _cwd: &str, _cx: &mut App) -> bool {
        false
    }

    // Models.

    /// Where the model picker reads models and provider availability
    /// (`modelSource.ts`). `None` shows a plain model chip instead.
    fn model_source(&self, _cx: &mut App) -> Option<Rc<dyn ModelSource>> {
        None
    }

    /// The model picker's saved preferences (favorites, recents, tab).
    fn model_prefs(&self, _cx: &mut App) -> ModelPrefs {
        ModelPrefs::default()
    }

    /// Per-project provider overrides that hide providers from the picker.
    fn project_providers(&self, _cx: &mut App) -> ProjectProviders {
        ProjectProviders::default()
    }

    // MCP.

    /// The MCP settings snapshot for the picker.
    fn mcp_servers(&self, _cwd: &str, _harness: HarnessId, _cx: &mut App) -> McpServers {
        McpServers::default()
    }
}
