//! Fills the settings page's `accounts` and `project_notifications` slots.
//!
//! ```ignore
//! let hosts = SettingsHosts {
//!     accounts: Some(accounts_slot(usage_host)),
//!     project_notifications: Some(project_notifications_slot(notifications_host)),
//!     ..hosts
//! };
//! ```
//!
//! Each view follows the page's live slot context, so a repeated
//! notification request, a new project path, and the reveal highlight reach
//! it after it was built, as React props did.

use std::rc::Rc;

use gpui::{AppContext as _, Context, Entity};

use super::host::{NotificationsHost, UsageHost};
use super::project_notifications::{ProjectNotificationProps, ProjectNotificationSettings};
use super::provider_accounts::ProviderAccountsSettings;
use crate::settings::chrome::Reveal;
use crate::settings::{LiveSlotContext, SlotContext, ViewSlot};

/// The card's props from the page's slot context.
pub fn project_notification_props(slot: &SlotContext) -> ProjectNotificationProps {
    ProjectNotificationProps {
        cwd: slot.cwd.clone(),
        recents: slot.recents.clone(),
        notification_project_path: slot.notification_project_path.clone(),
        notification_settings_request: slot.notification_settings_request,
        highlighted: slot.highlighted,
    }
}

fn reveal(slot: &SlotContext) -> Reveal {
    Reveal {
        revealed: slot.revealed.clone(),
        ..Default::default()
    }
}

impl ProjectNotificationSettings {
    /// Takes new props whenever the page's slot context changes.
    pub fn follow(&mut self, live: &Entity<LiveSlotContext>, cx: &mut Context<Self>) {
        let subscription = cx.observe(live, |this, live, cx| {
            let props = project_notification_props(&live.read(cx).0);
            if props != *this.props() {
                this.set_props(props, cx);
            }
        });
        self.keep(subscription);
    }
}

impl ProviderAccountsSettings {
    /// Flashes with the page's reveal highlight.
    pub fn follow(&mut self, live: &Entity<LiveSlotContext>, cx: &mut Context<Self>) {
        let subscription = cx.observe(live, |this, live, cx| {
            let reveal = reveal(&live.read(cx).0);
            this.set_reveal(reveal, cx);
        });
        self.keep(subscription);
    }
}

/// `ProjectNotificationSettings` for the Inbox page.
pub fn project_notifications_slot(host: Rc<dyn NotificationsHost>) -> ViewSlot {
    Rc::new(move |slot, _, cx| {
        let host = host.clone();
        let props = project_notification_props(slot);
        let live = slot.live.clone();
        cx.new(|cx| {
            let mut view = ProjectNotificationSettings::new(host, props, cx);
            if let Some(live) = &live {
                view.follow(live, cx);
            }
            view
        })
        .into()
    })
}

/// `ProviderAccountsSettings` for the Providers page.
pub fn accounts_slot(host: Rc<dyn UsageHost>) -> ViewSlot {
    Rc::new(move |slot, window, cx| {
        let host = host.clone();
        let live = slot.live.clone();
        let initial = reveal(slot);
        cx.new(|cx| {
            let mut view = ProviderAccountsSettings::new(host, window, cx);
            view.set_reveal(initial, cx);
            if let Some(live) = &live {
                view.follow(live, cx);
            }
            view
        })
        .into()
    })
}
