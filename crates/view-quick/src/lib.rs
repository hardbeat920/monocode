//! The floating quick composer and the quick git popup. Port of
//! src/features/quick-composer/ui and the window half of
//! src-tauri/src/quick_composer.rs.
//!
//! - [`QuickComposer`]: the card. It reports window work as
//!   [`QuickComposerEvent`]s and reaches the engine through
//!   [`QuickComposerHost`].
//! - [`QuickGitPopup`]: the working copy, base, and branch pickers shown in
//!   their own panel.
//! - [`QuickModelSelector`], [`QuickPermissions`]: the lists under the
//!   toolbar.
//!
//! This crate does not depend on `monocode-engine`. The app implements the
//! host traits with the automations package's `QuickLaunch` and the projects
//! package, and converts [`QuickLaunchRequest`] with serde.

mod colors;
pub mod composer;
mod field;
pub mod git_popup;
pub mod host;
pub mod model;
pub mod panels;
pub mod permissions;
pub mod project_icon;
pub mod selector;

pub use composer::{QuickComposer, QuickComposerEvent};
pub use git_popup::{QuickGitPopup, QuickGitPopupEvent};
pub use host::{HostTask, NativeClipboard, QuickComposerHost, QuickGitHost, QuickSnapshot};
pub use model::launch::{
    GitBranchInfo, GitBranches, QuickGitAnchor, QuickGitKind, QuickGitRequest, QuickGitResult,
    QuickIntent, QuickLaunchRequest, QuickWorkspace, Worktree,
};
pub use model::motion::Picker;
pub use panels::QuickPanels;
pub use permissions::{QuickPermissions, QuickPermissionsEvent};
pub use selector::{QuickModelSelector, QuickModelSelectorEvent, SelectorProps};

/// Binds the quick composer's keys. Call once at startup, after
/// `gpui_component::init` and `monocode_view_composer::composer::init`.
pub fn init(cx: &mut gpui::App) {
    composer::init(cx);
}
