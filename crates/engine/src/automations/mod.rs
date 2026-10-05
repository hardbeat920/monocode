//! Engine package `automations`: scheduled and Inbox-triggered automations,
//! session reminders, and the launches the floating quick composer hands to
//! a workspace window.
//!
//! - `Automations`: the list, create and edit, run now, run history, and the
//!   scheduler with restart recovery (src/features/automations/model, the
//!   App.tsx launch and scheduler code).
//! - `Reminders`: reminders, the due list, open requests, and the poller
//!   that src-tauri ran every 5 s (sessionReminders.ts, useSessionReminders).
//! - `QuickLaunch`: the launch queue, delivery to a window, and the
//!   acceptance into its workspace (src/features/quick-composer/model,
//!   useQuickComposerLaunches, quickLaunchSession.ts).
//!
//! Start with `AutomationsPackage::init` after `Engine::init`, then
//! `AutomationsPackage::start`. Windows attach their hosts:
//! `Automations::set_host`, `Reminders::attach_window`, and
//! `QuickLaunch::attach_window`.

#[allow(clippy::module_inception)]
pub mod automations;
pub mod backend;
pub mod events;
pub mod host;
pub mod launch_delivery;
pub mod local_time;
pub mod model;
pub mod prepare;
pub mod quick_composer;
pub mod quick_launch;
pub mod quick_launch_session;
pub mod reminders;
pub mod templates;

#[cfg(test)]
mod entity_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod quick_tests;
#[cfg(test)]
mod testing;

use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext, Entity, Global};
use monocode_core::Platform;
use monocode_settings::Kv;
use monocode_store::StoreEvents;
use monocode_store::session_store::SessionStore;

pub use automations::{Automations, AutomationsEvent};
pub use backend::{
    AutomationsBackend, ReminderTarget, RemindersBackend, SessionReminder, StoreAutomationsBackend,
    StoreRemindersBackend,
};
pub use host::{
    LaunchHost, NoReminderApp, NoWorkspace, ReminderApp, ReminderHost, SessionPlacement,
};
pub use quick_launch::{NoQuickLaunchApp, QuickLaunch, QuickLaunchApp, QuickLaunchEvent};
pub use reminders::{Reminders, RemindersEvent};

use crate::attention::{AttentionPlatform, Clock, system_clock};

/// What `AutomationsPackage::init` needs.
pub struct AutomationsConfig {
    pub kv: Kv,
    pub automations: Arc<dyn AutomationsBackend>,
    pub reminders: Arc<dyn RemindersBackend>,
    /// Shows reminder banners; `None` skips them.
    pub platform: Option<Arc<dyn AttentionPlatform>>,
    pub clock: Clock,
    /// The quick composer exists on macOS only.
    pub os: Platform,
}

impl AutomationsConfig {
    /// Storage in `monocode.db`, the system clock, and this platform.
    /// `events` hears the store's change notices.
    pub fn from_store(
        kv: Kv,
        store: Arc<SessionStore>,
        events: Arc<dyn StoreEvents>,
        platform: Option<Arc<dyn AttentionPlatform>>,
        cx: &App,
    ) -> Self {
        let executor = cx.background_executor().clone();
        Self {
            kv,
            automations: Arc::new(StoreAutomationsBackend::new(
                store.clone(),
                events.clone(),
                executor.clone(),
            )),
            reminders: Arc::new(StoreRemindersBackend::new(store, events, executor)),
            platform,
            clock: system_clock(),
            os: Platform::current(),
        }
    }
}

/// The package's entities. Views observe them.
pub struct AutomationsPackage {
    pub automations: Entity<Automations>,
    pub reminders: Entity<Reminders>,
    pub quick_launch: Entity<QuickLaunch>,
}

impl Global for AutomationsPackage {}

impl AutomationsPackage {
    /// Create the entities and install the global. Call after `Engine::init`.
    pub fn init(config: AutomationsConfig, cx: &mut App) {
        let AutomationsConfig {
            kv,
            automations,
            reminders,
            platform,
            clock,
            os,
        } = config;
        let automations = cx.new({
            let kv = kv.clone();
            let clock = clock.clone();
            move |cx| Automations::new(automations, kv, clock, cx)
        });
        let reminders = cx.new({
            let kv = kv.clone();
            move |cx| Reminders::new(reminders, kv, clock, platform, Rc::new(NoReminderApp), cx)
        });
        let quick_launch = cx.new(|cx| QuickLaunch::new(kv, os, cx));
        cx.set_global(AutomationsPackage {
            automations,
            reminders,
            quick_launch,
        });
    }

    /// Load the lists, start the automation scheduler and the reminder
    /// poller, and claim the quick composer shortcut.
    pub fn start(cx: &mut App) {
        let package = Self::global(cx);
        let (automations, reminders, quick_launch) = (
            package.automations.clone(),
            package.reminders.clone(),
            package.quick_launch.clone(),
        );
        automations.update(cx, |automations, cx| {
            automations.refresh(cx).detach();
            automations.start_scheduler(cx);
        });
        reminders.update(cx, |reminders, cx| {
            reminders.refresh(cx).detach();
            reminders.start(cx);
        });
        quick_launch.update(cx, |quick_launch, cx| {
            if let Err(error) = quick_launch.apply_shortcut(cx) {
                log::warn!("Could not claim the quick composer shortcut: {error}");
            }
        });
    }

    pub fn global(cx: &App) -> &AutomationsPackage {
        cx.global::<AutomationsPackage>()
    }

    pub fn try_global(cx: &App) -> Option<&AutomationsPackage> {
        cx.try_global::<AutomationsPackage>()
    }
}
