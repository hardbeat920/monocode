//! The settings page and its sections. Port of
//! src/features/settings/ui/SettingsView.tsx, JiraSettings.tsx,
//! GradientBlurBackground.tsx, src/shared/ui/ColorPickerPopover.tsx,
//! src/features/projects/ui/ProjectBackgroundDialog.tsx and
//! useProjectBackgroundEffect.ts, and the chat background effects in
//! src/features/settings/model.
//!
//! [`SettingsPage`] is the view. Settings that lived in localStorage read and
//! write `monocode_settings::Kv` directly; appearance changes recompute
//! `monocode_ui`'s theme at once. Everything else reaches the app through the
//! per-section traits in [`host`], and pages other crates own plug into the
//! slots of [`SettingsHosts`].

pub mod appearance_page;
pub mod appearance_state;
pub mod archive;
pub mod background;
pub mod binary_control;
pub mod chat;
pub mod chrome;
pub mod color_picker;
pub mod controls;
pub mod general;
pub mod host;
pub mod inbox;
pub mod jira;
pub mod keybindings;
pub mod native_glass;
pub mod page;
pub mod private_email;
pub mod project_background_dialog;
pub mod providers;
pub mod search;
pub mod section;
pub mod select;
pub mod shortcut_editor;
pub mod store;

#[cfg(test)]
mod tests;

pub use appearance_state::AppearanceState;
pub use background::{BackgroundEffects, BackgroundImage, ChatBackground};
pub use chrome::page_header;
pub use host::{
    AppearanceHost, ArchiveHost, ArchivedProject, BinaryInspection, GeneralHost, GithubStatus,
    HostTask, InboxHost, JiraProject, JiraStatus, KeybindingsHost, LinearTeam, LiveSlotContext,
    NoopHost, NotificationPermission, ProjectBackgroundHost, ProjectBackgroundSettings,
    ProvidersHost, SessionSummary, SettingsCallbacks, SettingsHosts, SettingsProps, SlotContext,
    UpdatePhase, UpdaterSnapshot, UrlStatus, ViewSlot,
};
pub use native_glass::{GlassWindow, NativeGlass};
pub use page::{SectionBody, SettingsPage};
pub use project_background_dialog::ProjectBackgroundDialog;
