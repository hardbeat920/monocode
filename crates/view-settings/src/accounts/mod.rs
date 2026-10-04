//! Usage chips, provider accounts, and notification controls. Ports of
//! src/app/shell/UsageProviderChip.tsx, UsageFooter.tsx, and PiUsage.tsx,
//! src/features/providers/ui (HarnessUpdateNotice, ProviderAccountUsage,
//! ProviderAccountSubtitle), src/features/notifications/ui
//! (ProjectNotificationSettings, NotificationMuteControl,
//! NotificationMuteDatePicker, notificationMuteActions), the
//! ProviderAccountsSettings card in SettingsView.tsx, and the pieces they
//! draw with (ProviderSignInPanel, DateTimePicker, the project mascots).
//!
//! The views take plain data shaped like the engine's attention package
//! ([`model`], [`notification_model`]) and reach the app through the traits
//! in [`host`].

pub mod account_usage;
pub mod date_time_picker;
pub mod harness_update_notice;
pub mod host;
pub mod mascot;
pub mod model;
pub mod mute_control;
pub mod notification_model;
pub mod pi_usage;
pub mod popover;
pub mod project_notifications;
pub mod provider_accounts;
pub mod sign_in_panel;
pub mod slots;
pub mod style;
pub mod usage_chip;
pub mod usage_footer;

pub use harness_update_notice::{HarnessUpdateNotice, HarnessUpdateNoticeEvent};
pub use host::{
    HarnessUpdateHost, NoopAccountsHost, NotificationsHost, OnChange, ProjectAppearance, UsageHost,
};
pub use mute_control::{MuteControlEvent, NotificationMuteControl, NotificationMuteDatePicker};
pub use pi_usage::PiUsage;
pub use project_notifications::{ProjectNotificationProps, ProjectNotificationSettings};
pub use provider_accounts::{AccountEditor, ProviderAccountsSettings};
pub use slots::{accounts_slot, project_notifications_slot};
pub use usage_chip::{AccountView, ChipActions, ChipProps, Presentation, UsageProviderChip};
pub use usage_footer::{UsageFooter, UsageFooterCallbacks, UsageFooterProps, UsageFooterSession};

#[cfg(test)]
mod tests;
